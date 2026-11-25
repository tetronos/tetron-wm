//! Drives for the Files app: discovery with `lsblk`, actions with `udisksctl`.
//!
//! No tetron-wm dependency, so it moves with Files when Files becomes its own
//! app. Nothing here mounts by itself: the caller asks.
//!
//! Which filesystems are listed: partitions, whole disks without a partition
//! table, and the opened side of LUKS / LVM / RAID, when they hold a
//! filesystem a user would open. Hidden: the running system's own mounts
//! (`/`, `/boot`, `/home`, every fstab mount, swap), the live medium and the
//! other partitions of its stick, loop and zram devices, EFI system and
//! Microsoft reserved partitions, LVM and RAID members.

use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// One filesystem the Files app can show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drive {
    /// The device to mount, e.g. `/dev/sdb1` (for a locked LUKS: the LUKS partition).
    pub dev: String,
    /// The whole disk it lives on, e.g. `/dev/sdb` (eject powers this off).
    pub disk: String,
    /// Kernel name, e.g. `sdb1`.
    pub name: String,
    pub label: Option<String>,
    pub uuid: Option<String>,
    pub size: u64,
    pub fstype: Option<String>,
    /// Where it is mounted now (the first mount point), if mounted.
    pub mountpoint: Option<PathBuf>,
    /// USB sticks, SD cards and other hot-pluggable disks.
    pub removable: bool,
    /// `usb`, `sata`, `nvme`, `mmc`, … of the disk (lsblk TRAN).
    pub bus: Option<String>,
    /// The disk model, when the kernel knows it.
    pub model: Option<String>,
    /// A LUKS partition that is not unlocked yet.
    pub locked: bool,
    /// For an unlocked LUKS volume: the LUKS partition (locked again after unmount).
    pub luks: Option<String>,
}

impl Drive {
    /// The name the pane shows: the label, else the kernel name.
    pub fn title(&self) -> &str {
        self.label.as_deref().filter(|l| !l.trim().is_empty()).unwrap_or(&self.name)
    }
    pub fn mounted(&self) -> bool {
        self.mountpoint.is_some()
    }
}

/// Size for a narrow column: `64M`, `29G`, `1.8T`.
pub fn human_size(bytes: u64) -> String {
    const U: [&str; 6] = ["B", "K", "M", "G", "T", "P"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1000.0 && i + 1 < U.len() {
        v /= 1024.0;
        i += 1;
    }
    if v < 10.0 && i > 0 && v.fract() >= 0.05 {
        format!("{v:.1}{}", U[i])
    } else {
        format!("{}{}", v.round() as u64, U[i])
    }
}

/// The lsblk columns `parse` reads.
pub const LSBLK_COLUMNS: &str = "NAME,PATH,PKNAME,TYPE,SIZE,FSTYPE,LABEL,UUID,MOUNTPOINTS,RM,HOTPLUG,TRAN,PARTTYPE,MODEL";

/// The drives of this machine, filtered for the Files app. Empty when lsblk
/// is missing or fails (not Linux): Files then shows no Drives section.
pub fn list() -> Vec<Drive> {
    let out = Command::new("lsblk")
        .args(["--json", "--bytes", "--list", "-o", LSBLK_COLUMNS])
        .stderr(Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() => parse(&String::from_utf8_lossy(&o.stdout), &fstab_targets()),
        _ => Vec::new(),
    }
}

/// Mount points named in /etc/fstab: the system's own, hidden from the list.
fn fstab_targets() -> Vec<String> {
    std::fs::read_to_string("/etc/fstab").map(|s| parse_fstab(&s)).unwrap_or_default()
}

pub fn parse_fstab(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_whitespace().nth(1))
        .filter(|t| t.starts_with('/'))
        .map(|t| t.replace("\\040", " "))
        .collect()
}

/// One row of `lsblk --json --list`.
#[derive(Clone, Debug, Default)]
struct Row {
    name: String,
    path: String,
    pkname: Option<String>,
    kind: String,
    size: u64,
    fstype: Option<String>,
    label: Option<String>,
    uuid: Option<String>,
    mounts: Vec<String>,
    rm: bool,
    hotplug: bool,
    tran: Option<String>,
    parttype: Option<String>,
    model: Option<String>,
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_string).filter(|x| !x.is_empty())
}

/// lsblk prints booleans as true/false (2.38+) or "0"/"1" / 0/1 (older).
fn b(v: &Value, k: &str) -> bool {
    match v.get(k) {
        Some(Value::Bool(x)) => *x,
        Some(Value::Number(n)) => n.as_u64() == Some(1),
        Some(Value::String(x)) => x == "1" || x == "true",
        _ => false,
    }
}

fn row(v: &Value) -> Row {
    let mut mounts: Vec<String> = match v.get("mountpoints") {
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        _ => Vec::new(),
    };
    if let Some(m) = s(v, "mountpoint") {
        mounts.push(m); // lsblk older than 2.37 has only MOUNTPOINT
    }
    let size = match v.get("size") {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(0),
        Some(Value::String(x)) => x.parse().unwrap_or(0),
        _ => 0,
    };
    Row {
        name: s(v, "name").unwrap_or_default(),
        path: s(v, "path").unwrap_or_default(),
        pkname: s(v, "pkname"),
        kind: s(v, "type").unwrap_or_default(),
        size,
        fstype: s(v, "fstype"),
        label: s(v, "label"),
        uuid: s(v, "uuid"),
        mounts,
        rm: b(v, "rm"),
        hotplug: b(v, "hotplug"),
        tran: s(v, "tran"),
        parttype: s(v, "parttype").map(|p| p.to_lowercase()),
        model: s(v, "model").map(|m| m.trim().to_string()),
    }
}

const ESP: &str = "c12a7328-f81f-11d2-ba4b-00a0c93ec93b";
const MS_RESERVED: &str = "e3c9e316-0b5c-4db8-817d-f92df00215ae";
/// MBR type codes lsblk prints for an extended partition and an ESP.
const MBR_HIDDEN: [&str; 4] = ["0x5", "0xf", "0x85", "0xef"];
const NOT_A_FILESYSTEM: [&str; 5] = ["swap", "LVM2_member", "linux_raid_member", "crypto_LUKS", "zfs_member"];

/// A mount point that belongs to the running system.
fn system_mount(m: &str, fstab: &[String]) -> bool {
    m == "/"
        || m == "[SWAP]"
        || m.starts_with("/boot")
        || m.starts_with("/run/live")
        || m.starts_with("/lib/live")
        || m.starts_with("/snap/")
        || fstab.iter().any(|t| t == m)
}

/// Filter `lsblk --json --bytes --list -o LSBLK_COLUMNS` output (see the module doc).
pub fn parse(json: &str, fstab: &[String]) -> Vec<Drive> {
    let Ok(v) = serde_json::from_str::<Value>(json) else { return Vec::new() };
    let rows: Vec<Row> = v.get("blockdevices").and_then(Value::as_array).map(|a| a.iter().map(row).collect()).unwrap_or_default();
    let by_name = |n: &str| rows.iter().find(|r| r.name == n);
    // Walk up to the top-level disk (a crypt or lvm volume's parent chain).
    let top = |r: &Row| -> Row {
        let mut cur = r.clone();
        for _ in 0..8 {
            match cur.pkname.as_deref().and_then(by_name) {
                Some(p) => cur = p.clone(),
                None => break,
            }
        }
        cur
    };
    let children = |r: &Row| rows.iter().filter(|c| c.pkname.as_deref() == Some(r.name.as_str())).count();
    // Disks holding a system mount anywhere below them are hidden whole when
    // they are the live medium (its other partitions are the ISO's own).
    let live_disks: Vec<String> = rows
        .iter()
        .filter(|r| r.mounts.iter().any(|m| m.starts_with("/run/live") || m.starts_with("/lib/live")))
        .map(|r| top(r).name)
        .collect();
    let mut out = Vec::new();
    for r in &rows {
        if matches!(r.kind.as_str(), "loop" | "zram") || r.name.starts_with("zram") || r.name.starts_with("loop") {
            continue;
        }
        let disk = top(r);
        if live_disks.contains(&disk.name) || r.mounts.iter().any(|m| system_mount(m, fstab)) {
            continue;
        }
        if r.parttype.as_deref().is_some_and(|p| p == ESP || p == MS_RESERVED || MBR_HIDDEN.contains(&p)) {
            continue;
        }
        let fs = r.fstype.as_deref();
        let luks = fs == Some("crypto_LUKS");
        // A whole disk with partitions shows its partitions instead.
        if r.kind == "disk" && children(r) > 0 {
            continue;
        }
        if luks {
            // Unlocked: its child (type crypt) is listed instead.
            if children(r) > 0 {
                continue;
            }
        } else if fs.is_none() || fs.is_some_and(|f| NOT_A_FILESYSTEM.contains(&f)) {
            continue;
        }
        let parent_luks = (r.kind == "crypt")
            .then(|| r.pkname.as_deref().and_then(by_name))
            .flatten()
            .filter(|p| p.fstype.as_deref() == Some("crypto_LUKS"))
            .map(|p| p.path.clone());
        out.push(Drive {
            dev: r.path.clone(),
            disk: disk.path.clone(),
            name: r.name.clone(),
            label: r.label.clone(),
            uuid: r.uuid.clone(),
            size: r.size,
            fstype: r.fstype.clone(),
            mountpoint: r.mounts.first().map(PathBuf::from),
            removable: disk.rm || disk.hotplug || r.rm || r.hotplug,
            bus: disk.tran.clone(),
            model: disk.model.clone(),
            locked: luks,
            luks: parent_luks,
        });
    }
    out
}

/// The listed drive `dev` names: its path, its kernel name (`sdb1`, `dm-0`), or
/// another name of the same device node (`/dev/dm-0` for `/dev/mapper/luks-…`,
/// which is what udisksctl reports after an unlock).
pub fn find<'a>(all: &'a [Drive], dev: &str) -> Option<&'a Drive> {
    let real = |p: &str| std::fs::canonicalize(p).ok();
    let want = real(dev).or_else(|| real(&format!("/dev/{dev}")));
    all.iter().find(|d| d.dev == dev || d.name == dev || (want.is_some() && real(&d.dev) == want))
}

/// Whether `udisksctl` (udisks2) is on PATH. Without it drives are listed but
/// the actions explain what to install.
pub fn available() -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join("udisksctl").is_file()))
}

/// What an action failed with, for the error box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// udisks2 is not installed.
    NoUdisks,
    /// The filesystem is in use: the programs holding it (name and pid).
    Busy(Vec<(String, u32)>),
    /// Anything else, cleaned of the D-Bus prefix.
    Failed(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NoUdisks => write!(f, "mounting needs udisks2 (udisksctl); on TetronOS it comes with tetronos-storage"),
            Error::Busy(users) if users.is_empty() => write!(f, "in use by a program; close it and try again"),
            Error::Busy(users) => {
                let names: Vec<String> = users.iter().map(|(n, p)| format!("{n} ({p})")).collect();
                write!(f, "in use by {}; close it and try again", names.join(", "))
            }
            Error::Failed(m) => write!(f, "{m}"),
        }
    }
}

/// `Error mounting /dev/sdb1: GDBus.Error:org.freedesktop.UDisks2.Error.Failed: X` → `X`.
pub fn clean_error(stderr: &str) -> String {
    let line = stderr.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("failed");
    let tail = line.rsplit_once("GDBus.Error:").map(|(_, t)| t).unwrap_or(line);
    let tail = match tail.split_once(": ") {
        Some((head, rest)) if head.starts_with("org.freedesktop.") => rest,
        _ => tail,
    };
    tail.trim().trim_end_matches('.').to_string()
}

fn udisksctl(args: &[&str], stdin: Option<&str>) -> Result<String, Error> {
    if !available() {
        return Err(Error::NoUdisks);
    }
    let mut cmd = Command::new("udisksctl");
    cmd.args(args).arg("--no-user-interaction").stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
    let mut child = cmd.spawn().map_err(|e| Error::Failed(e.to_string()))?;
    if let (Some(text), Some(mut w)) = (stdin, child.stdin.take()) {
        let _ = w.write_all(text.as_bytes());
    }
    let o = child.wait_with_output().map_err(|e| Error::Failed(e.to_string()))?;
    if o.status.success() {
        Ok(String::from_utf8_lossy(&o.stdout).into_owned())
    } else {
        Err(Error::Failed(clean_error(&String::from_utf8_lossy(&o.stderr))))
    }
}

/// The mount point of `dev` now (lsblk), for after a mount.
fn mountpoint_of(dev: &str) -> Option<PathBuf> {
    let o = Command::new("lsblk").args(["--noheadings", "--list", "-o", "MOUNTPOINT", dev]).output().ok()?;
    String::from_utf8_lossy(&o.stdout).lines().map(str::trim).find(|l| !l.is_empty()).map(PathBuf::from)
}

/// `Mounted /dev/sdb1 at /media/u/STICK` (some versions add a final dot).
pub fn parse_mounted(stdout: &str) -> Option<PathBuf> {
    let (_, at) = stdout.trim().split_once(" at ")?;
    Some(PathBuf::from(at.trim().trim_end_matches('.')))
}

/// `Unlocked /dev/sdb1 as /dev/dm-3.`
pub fn parse_unlocked(stdout: &str) -> Option<String> {
    let (_, as_) = stdout.trim().split_once(" as ")?;
    Some(as_.trim().trim_end_matches('.').to_string())
}

/// Mount `dev` (under /media/<user>) and return where.
pub fn mount(dev: &str) -> Result<PathBuf, Error> {
    let out = udisksctl(&["mount", "-b", dev], None)?;
    parse_mounted(&out).or_else(|| mountpoint_of(dev)).ok_or_else(|| Error::Failed(format!("mounted {dev}, mount point unknown")))
}

/// Unmount `d`; an unlocked LUKS volume is locked again after.
pub fn unmount(d: &Drive) -> Result<(), Error> {
    let at = d.mountpoint.clone();
    if d.mounted() {
        if let Err(e) = udisksctl(&["unmount", "-b", &d.dev], None) {
            return Err(busy_or(e, at.as_deref()));
        }
    }
    if let Some(luks) = &d.luks {
        udisksctl(&["lock", "-b", luks], None)?;
    }
    Ok(())
}

fn busy_or(e: Error, at: Option<&Path>) -> Error {
    match (&e, at) {
        (Error::Failed(m), Some(at)) if m.to_lowercase().contains("busy") => Error::Busy(users_of(at)),
        _ => e,
    }
}

/// Unmount every listed filesystem of `d.disk`, then power the disk off (safe to unplug).
pub fn eject(d: &Drive, all: &[Drive]) -> Result<(), Error> {
    for x in all.iter().filter(|x| x.disk == d.disk) {
        unmount(x)?;
    }
    udisksctl(&["power-off", "-b", &d.disk], None).map(|_| ())
}

/// Unlock a LUKS partition with `passphrase`; returns the cleartext device.
pub fn unlock(dev: &str, passphrase: &str) -> Result<String, Error> {
    let out = udisksctl(&["unlock", "-b", dev, "--key-file", "/dev/stdin"], Some(passphrase))?;
    parse_unlocked(&out).ok_or_else(|| Error::Failed(format!("unlocked {dev}, device unknown")))
}

/// Programs (name, pid) with a file, folder or working directory under `at`.
pub fn users_of(at: &Path) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    let Ok(procs) = std::fs::read_dir("/proc") else { return out };
    for p in procs.flatten() {
        let Ok(pid) = p.file_name().to_string_lossy().parse::<u32>() else { continue };
        let base = p.path();
        let under = |l: PathBuf| l.starts_with(at);
        let mut hit = ["cwd", "root"].iter().any(|k| std::fs::read_link(base.join(k)).is_ok_and(under));
        if !hit {
            if let Ok(fds) = std::fs::read_dir(base.join("fd")) {
                hit = fds.flatten().any(|f| std::fs::read_link(f.path()).is_ok_and(under));
            }
        }
        if hit {
            let name = std::fs::read_to_string(base.join("comm")).unwrap_or_default().trim().to_string();
            out.push((name, pid));
        }
    }
    out.sort_by_key(|(_, p)| *p);
    out
}

/// Notices when drives come, go, or are mounted elsewhere: the names under
/// /sys/class/block and the mount table, compared with the last call.
/// Cheap enough for once a second.
#[derive(Default)]
pub struct Watcher {
    last: Option<u64>,
}

impl Watcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// True on the first call and whenever the drives or mounts changed since the last.
    pub fn changed(&mut self) -> bool {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        if let Ok(rd) = std::fs::read_dir("/sys/class/block") {
            let mut names: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
            names.sort();
            names.hash(&mut h);
            // A card reader keeps its device when the card changes: its size does not.
            for n in &names {
                std::fs::read_to_string(format!("/sys/class/block/{n}/size")).unwrap_or_default().hash(&mut h);
            }
        }
        std::fs::read_to_string("/proc/self/mounts").unwrap_or_default().hash(&mut h);
        let now = h.finish();
        let changed = self.last != Some(now);
        self.last = Some(now);
        changed
    }
}
