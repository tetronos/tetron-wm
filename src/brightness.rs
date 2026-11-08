//! Screen brightness of a laptop panel (Linux): read from
//! `/sys/class/backlight`, set through logind's `SetBrightness` on the
//! caller's session (no root, no extra package), else `brightnessctl`, else a
//! direct write to sysfs (works where udev gave the group `video` write
//! access). Desktops with external monitors and virtual machines have no
//! backlight device: then there is nothing to show and the controls hide.

use std::path::{Path, PathBuf};

/// Where the kernel lists backlight devices.
pub const SYSFS: &str = "/sys/class/backlight";

/// Step for the arrow keys and the wheel, in percent.
pub const STEP: u8 = 5;

/// Lowest level set from the controls, in percent: 0 turns many panels fully
/// dark, which looks like a dead screen.
pub const MIN_PCT: u8 = 1;

/// A backlight device: its sysfs name and directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub name: String,
    pub dir: PathBuf,
}

fn read_u32(p: &Path) -> Option<u32> {
    std::fs::read_to_string(p).ok()?.trim().parse().ok()
}

/// The device to control under `root`: the kernel's own ranking by `type`,
/// firmware (ACPI) first, then platform, then raw (a GPU driver), as the
/// sysfs ABI document advises; ties by name. Devices with no range are skipped.
pub fn device_in(root: &Path) -> Option<Device> {
    let rank = |dir: &Path| match std::fs::read_to_string(dir.join("type")).unwrap_or_default().trim() {
        "firmware" => 0,
        "platform" => 1,
        _ => 2,
    };
    let mut devs: Vec<(u8, String, PathBuf)> = std::fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|d| read_u32(&d.join("max_brightness")).is_some_and(|m| m > 0))
        .filter_map(|d| Some((rank(&d), d.file_name()?.to_string_lossy().into_owned(), d)))
        .collect();
    devs.sort();
    devs.into_iter().next().map(|(_, name, dir)| Device { name, dir })
}

/// The device of this machine, if any.
pub fn device() -> Option<Device> {
    device_in(Path::new(SYSFS))
}

/// `(current, max)` raw values of `dev`.
pub fn raw(dev: &Device) -> Option<(u32, u32)> {
    let max = read_u32(&dev.dir.join("max_brightness")).filter(|m| *m > 0)?;
    // actual_brightness is what the hardware reports; brightness what was asked.
    let cur = read_u32(&dev.dir.join("actual_brightness")).or_else(|| read_u32(&dev.dir.join("brightness")))?;
    Some((cur.min(max), max))
}

/// Raw value to percent, rounded.
pub fn to_pct(cur: u32, max: u32) -> u8 {
    if max == 0 {
        return 0;
    }
    ((cur as u64 * 100 + max as u64 / 2) / max as u64).min(100) as u8
}

/// Percent to raw value, at least 1 when `pct` is above 0 (a coarse range,
/// such as 0..=15, must not round a low setting to dark).
pub fn from_pct(pct: u8, max: u32) -> u32 {
    let v = (pct.min(100) as u64 * max as u64 + 50) / 100;
    if pct > 0 { v.max(1) as u32 } else { 0 }
}

/// The brightness of this machine in percent, or `None` without a backlight.
pub fn read_pct() -> Option<u8> {
    let d = device()?;
    raw(&d).map(|(c, m)| to_pct(c, m))
}

/// `pct` after one step `up` or down, kept in `MIN_PCT..=100`.
pub fn step(pct: u8, up: bool) -> u8 {
    if up { pct.saturating_add(STEP).min(100) } else { pct.saturating_sub(STEP).max(MIN_PCT) }
}

/// argv for logind's `Session.SetBrightness` on the caller's own session.
pub fn logind_argv(dev: &str, value: u32) -> Vec<String> {
    [
        "--system", "call", "org.freedesktop.login1", "/org/freedesktop/login1/session/auto",
        "org.freedesktop.login1.Session", "SetBrightness", "ssu", "backlight",
    ]
    .iter()
    .map(|s| s.to_string())
    .chain([dev.to_string(), value.to_string()])
    .collect()
}

/// Set this machine's brightness to `pct` percent (clamped to `MIN_PCT..=100`).
/// Returns how it was set, or `None` when nothing worked (or no backlight).
pub fn set_pct(pct: u8) -> Option<&'static str> {
    let dev = device()?;
    let (_, max) = raw(&dev)?;
    let value = from_pct(pct.clamp(MIN_PCT, 100), max);
    let ok = |prog: &str, args: &[String]| {
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        std::process::Command::new(prog)
            .args(&a)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    let how = if ok("busctl", &logind_argv(&dev.name, value)) {
        "logind"
    } else if ok("brightnessctl", &["-q".into(), "-d".into(), dev.name.clone(), "set".into(), value.to_string()]) {
        "brightnessctl"
    } else if std::fs::write(dev.dir.join("brightness"), value.to_string()).is_ok() {
        "sysfs"
    } else {
        crate::log_warn(&format!(
            "brightness: could not set {} to {value} (logind refused, no brightnessctl, sysfs not writable)",
            dev.name
        ));
        return None;
    };
    crate::log_debug(&format!("brightness: {} {pct}% ({value}/{max}) via {how}", dev.name));
    Some(how)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(root: &Path, name: &str, kind: &str, cur: u32, max: u32) {
        let d = root.join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("type"), format!("{kind}\n")).unwrap();
        std::fs::write(d.join("max_brightness"), format!("{max}\n")).unwrap();
        std::fs::write(d.join("brightness"), format!("{cur}\n")).unwrap();
        std::fs::write(d.join("actual_brightness"), format!("{cur}\n")).unwrap();
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tetron-wm-bl-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn no_device_without_a_backlight() {
        let r = tmp("none");
        assert_eq!(device_in(&r), None);
        assert_eq!(device_in(&r.join("missing")), None);
        fake(&r, "broken", "raw", 0, 0);
        assert_eq!(device_in(&r), None, "a device with no range is skipped");
    }

    #[test]
    fn firmware_beats_platform_beats_raw() {
        let r = tmp("rank");
        fake(&r, "intel_backlight", "raw", 400, 1000);
        fake(&r, "dell_backlight", "platform", 7, 15);
        assert_eq!(device_in(&r).unwrap().name, "dell_backlight");
        fake(&r, "acpi_video0", "firmware", 50, 100);
        let d = device_in(&r).unwrap();
        assert_eq!(d.name, "acpi_video0");
        assert_eq!(raw(&d), Some((50, 100)));
    }

    #[test]
    fn percent_round_trip_never_rounds_a_low_level_to_dark() {
        assert_eq!(to_pct(400, 1000), 40);
        assert_eq!(to_pct(7, 15), 47);
        assert_eq!(to_pct(5, 0), 0);
        assert_eq!(from_pct(40, 1000), 400);
        assert_eq!(from_pct(1, 15), 1, "1% of a 0..=15 range is 1, not 0");
        assert_eq!(from_pct(0, 15), 0);
        assert_eq!(from_pct(200, 15), 15);
    }

    #[test]
    fn steps_stay_in_range() {
        assert_eq!(step(50, true), 55);
        assert_eq!(step(98, true), 100);
        assert_eq!(step(50, false), 45);
        assert_eq!(step(3, false), MIN_PCT);
    }

    #[test]
    fn logind_call_names_the_session_method_and_device() {
        let a = logind_argv("intel_backlight", 400);
        assert_eq!(a.last().unwrap(), "400");
        assert!(a.iter().any(|x| x == "SetBrightness") && a.iter().any(|x| x == "intel_backlight"));
        assert!(a.iter().any(|x| x == "/org/freedesktop/login1/session/auto"));
    }
}
