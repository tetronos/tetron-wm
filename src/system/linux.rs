//! Linux backend: volume via `wpctl` (PipeWire), WiFi via `nmcli`, Bluetooth via
//! `bluetoothctl`/`rfkill`. All commands go through [`run_capped`].

use super::{
    run_capped, AudioOutput, BackendCaps, BluetoothInfo, BtDevice, ControlIntent, SystemControl, SystemMonitor,
    NetInfo, SystemReadout, VolumeInfo, Want, WifiInfo,
};

/// argv for `wpctl set-volume` (level 0..=100 → 0.00..=1.00).
pub fn set_volume_argv(level: u8) -> Vec<String> {
    let frac = level.min(100) as f32 / 100.0;
    vec![
        "set-volume".into(),
        "@DEFAULT_AUDIO_SINK@".into(),
        format!("{:.2}", frac),
    ]
}
/// argv for `nmcli radio wifi on|off`.
pub fn wifi_radio_argv(on: bool) -> Vec<String> {
    vec![
        "radio".into(),
        "wifi".into(),
        if on { "on".into() } else { "off".into() },
    ]
}
/// argv for `nmcli dev wifi connect <ssid>` (known network).
pub fn wifi_connect_argv(ssid: &str) -> Vec<String> {
    vec!["dev".into(), "wifi".into(), "connect".into(), ssid.into()]
}

pub struct LinuxBackend {
    has_wpctl: bool,
    has_nmcli: bool,
    has_bt: bool,
}

impl LinuxBackend {
    pub fn new() -> Self {
        Self {
            has_wpctl: run_capped("which", &["wpctl"], 1).is_some(),
            has_nmcli: run_capped("which", &["nmcli"], 1).is_some(),
            has_bt: run_capped("which", &["bluetoothctl"], 1).is_some(),
        }
    }
}

impl Default for LinuxBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemMonitor for LinuxBackend {
    fn read_parts(&self, want: Want) -> SystemReadout {
        SystemReadout {
            volume: if want.volume && self.has_wpctl { read_volume() } else { VolumeInfo::default() },
            wifi: if want.net && self.has_nmcli { read_wifi() } else { None },
            net: if want.net { read_net() } else { None },
            known_networks: if want.net && self.has_nmcli { read_known() } else { Vec::new() },
            bluetooth: if want.bluetooth && self.has_bt { read_bt() } else { BluetoothInfo::default() },
        }
    }
    fn caps(&self) -> BackendCaps {
        BackendCaps {
            volume: self.has_wpctl,
            wifi: self.has_nmcli,
            bluetooth: self.has_bt,
        }
    }
}

impl SystemControl for LinuxBackend {
    fn apply(&self, intent: &ControlIntent) {
        let wp = |a: &[String]| {
            run_capped("wpctl", &a.iter().map(String::as_str).collect::<Vec<_>>(), 2);
        };
        let nm = |a: &[String]| {
            run_capped("nmcli", &a.iter().map(String::as_str).collect::<Vec<_>>(), 5);
        };
        match intent {
            ControlIntent::VolumeSet(l) => wp(&set_volume_argv(*l)),
            ControlIntent::VolumeUp => {
                let v = read_volume();
                wp(&set_volume_argv(v.level.saturating_add(5)));
            }
            ControlIntent::VolumeDown => {
                let v = read_volume();
                wp(&set_volume_argv(v.level.saturating_sub(5)));
            }
            ControlIntent::BrightnessSet(p) => {
                crate::brightness::set_pct(*p);
            }
            ControlIntent::BrightnessUp | ControlIntent::BrightnessDown => {
                if let Some(p) = crate::brightness::read_pct() {
                    crate::brightness::set_pct(crate::brightness::step(p, *intent == ControlIntent::BrightnessUp));
                }
            }
            ControlIntent::ToggleMute => {
                run_capped("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"], 2);
            }
            ControlIntent::SetOutput(id) => {
                crate::dbg_log(&format!("sound: wpctl set-default {id}"));
                run_capped("wpctl", &["set-default", &id.to_string()], 2);
            }
            ControlIntent::WifiSetEnabled(on) => nm(&wifi_radio_argv(*on)),
            ControlIntent::WifiConnectKnown(ssid) => nm(&wifi_connect_argv(ssid)),
            ControlIntent::BtSetEnabled(on) => {
                run_capped("bluetoothctl", &["power", if *on { "on" } else { "off" }], 3);
            }
            ControlIntent::BtConnect { addr, connect } => {
                run_capped(
                    "bluetoothctl",
                    &[if *connect { "connect" } else { "disconnect" }, addr],
                    5,
                );
            }
            // UI-only intents (calendar nav, notifications) are handled by the
            // session's tray state, never the OS backend.
            _ => {}
        }
    }
}

/// Audio outputs from `wpctl status` (empty if wpctl is missing or fails).
pub fn read_outputs() -> Vec<AudioOutput> {
    run_capped("wpctl", &["status"], 2).map(|t| parse_wpctl_sinks(&t)).unwrap_or_default()
}

/// Parse the `Sinks:` list of the `Audio` section of `wpctl status`. A row is
/// `│  *   60. Speaker   [vol: 0.46]`: `*` marks the default, then the id, the
/// name, and a bracketed volume. The list ends at the next `├─` / `└─` heading.
/// A device-name prefix shared by every output is cut so rows stay short.
pub fn parse_wpctl_sinks(text: &str) -> Vec<AudioOutput> {
    let mut out = Vec::new();
    let mut in_audio = false;
    let mut in_sinks = false;
    for line in text.lines() {
        // Top-level sections (Audio, Video, Settings) start in column 0.
        if !line.starts_with(' ') && !line.is_empty() {
            in_audio = line.trim() == "Audio";
            in_sinks = false;
            continue;
        }
        if !in_audio {
            continue;
        }
        if line.contains('├') || line.contains('└') {
            in_sinks = line.contains("Sinks:");
            continue;
        }
        if !in_sinks {
            continue;
        }
        let body = line.trim_start_matches([' ', '│']);
        let default = body.starts_with('*');
        let body = body.trim_start_matches('*').trim_start();
        let Some((id, rest)) = body.split_once(". ") else { continue };
        let Ok(id) = id.trim().parse::<u32>() else { continue };
        let name = rest.split(" [vol:").next().unwrap_or(rest).trim().to_string();
        out.push(AudioOutput { id, name, default });
    }
    strip_common_prefix(&mut out);
    out
}

/// Remove the leading words every output name shares (the sound card name),
/// keeping at least one word of each.
fn strip_common_prefix(outs: &mut [AudioOutput]) {
    if outs.len() < 2 {
        return;
    }
    let words: Vec<Vec<&str>> = outs.iter().map(|o| o.name.split(' ').collect()).collect();
    let min = words.iter().map(|w| w.len()).min().unwrap_or(0);
    let mut n = 0;
    while n + 1 < min && words.iter().all(|w| w[n] == words[0][n]) {
        n += 1;
    }
    if n == 0 {
        return;
    }
    let names: Vec<String> = words.iter().map(|w| w[n..].join(" ")).collect();
    for (o, name) in outs.iter_mut().zip(names) {
        o.name = name;
    }
}

fn read_volume() -> VolumeInfo {
    let out = run_capped("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"], 2).unwrap_or_default();
    let muted = out.contains("MUTED");
    let level = out
        .split_whitespace()
        .nth(1)
        .and_then(|f| f.parse::<f32>().ok())
        .map(|f| (f * 100.0).round() as u8)
        .unwrap_or(0);
    VolumeInfo { level: level.min(100), muted }
}

fn read_wifi() -> Option<WifiInfo> {
    let out = run_capped("nmcli", &["-t", "-f", "ACTIVE,SSID,SIGNAL", "dev", "wifi"], 3)?;
    for line in out.lines() {
        let mut f = line.split(':');
        if f.next() == Some("yes") {
            let ssid = f.next().unwrap_or("").to_string();
            let sig: u8 = f.next().unwrap_or("0").parse().unwrap_or(0);
            return Some(WifiInfo { ssid, signal: (sig / 25).min(4), enabled: true });
        }
    }
    Some(WifiInfo { ssid: String::new(), signal: 0, enabled: true })
}

/// The default-route interface and its source IP; WiFi if the interface has a
/// `wireless` directory in sysfs.
fn read_net() -> Option<NetInfo> {
    let out = run_capped("ip", &["-4", "route", "get", "1.1.1.1"], 2)?;
    let (iface, ip) = super::parse_ip_route_get(&out)?;
    let wifi = std::path::Path::new("/sys/class/net").join(&iface).join("wireless").exists();
    Some(NetInfo { iface, ip, wifi })
}

fn read_known() -> Vec<String> {
    run_capped("nmcli", &["-t", "-f", "NAME", "connection", "show"], 3)
        .map(|s| {
            s.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn read_bt() -> BluetoothInfo {
    let enabled = run_capped("bluetoothctl", &["show"], 2)
        .map(|s| s.contains("Powered: yes"))
        .unwrap_or(false);
    let devices = run_capped("bluetoothctl", &["devices"], 3)
        .map(|s| {
            s.lines()
                .filter_map(|l| {
                    let mut p = l.splitn(3, ' ');
                    if p.next() != Some("Device") {
                        return None;
                    }
                    let addr = p.next()?.to_string();
                    let name = p.next().unwrap_or("").to_string();
                    Some(BtDevice { name, addr, connected: false })
                })
                .collect()
        })
        .unwrap_or_default();
    BluetoothInfo { enabled, devices }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed `wpctl status` from a laptop with three HDMI outputs.
    const STATUS: &str = "PipeWire 'pipewire-0' [1.4.2, user@host, cookie:1]
 └─ Clients:
        33. WirePlumber                         [1.4.2, user@host, pid:2027]

Audio
 ├─ Devices:
 │      49. Raptor Lake-P/U/H cAVS              [alsa]
 │  
 ├─ Sinks:
 │      56. Raptor Lake-P/U/H cAVS HDMI / DisplayPort 3 Output [vol: 1.00]
 │      59. Raptor Lake-P/U/H cAVS Headphones   [vol: 1.00]
 │  *   60. Raptor Lake-P/U/H cAVS Speaker      [vol: 0.46]
 │  
 ├─ Sources:
 │  *   62. Raptor Lake-P/U/H cAVS SoundWire microphones [vol: 1.00 MUTED]
 │  
 └─ Streams:

Video
 ├─ Sinks:
 │      90. Not audio                           [vol: 1.00]
";

    #[test]
    fn parses_audio_sinks_only_with_default_and_short_names() {
        let outs = parse_wpctl_sinks(STATUS);
        let got: Vec<(u32, &str, bool)> = outs.iter().map(|o| (o.id, o.name.as_str(), o.default)).collect();
        assert_eq!(got, vec![
            (56, "HDMI / DisplayPort 3 Output", false),
            (59, "Headphones", false),
            (60, "Speaker", true),
        ]);
    }

    #[test]
    fn a_single_output_keeps_its_full_name() {
        let one = "Audio\n ├─ Sinks:\n │  *   47. Built-in Audio Analog Stereo [vol: 0.40]\n ├─ Sources:\n";
        assert_eq!(parse_wpctl_sinks(one)[0].name, "Built-in Audio Analog Stereo");
        assert!(parse_wpctl_sinks("").is_empty());
    }
}
