//! The tetron-wm log: `debug.log` in the user's state directory
//! (`$XDG_STATE_HOME/tetron-wm/`, default `~/.local/state/tetron-wm/`;
//! `~/Library/Logs/tetron-wm/` on macOS), rotated at [`MAX_BYTES`] into
//! `debug.log.1` .. `debug.log.3`, so at most 16 MiB is kept and the oldest
//! file is dropped. A level (`log_level` in config.toml) filters the lines.
//!
//! A cargo test binary never writes here (it would fill the user's log with
//! test traffic) unless `TETRON_WM_LOG_DIR` points somewhere.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

/// Size at which `debug.log` is rotated.
pub const MAX_BYTES: u64 = 4 * 1024 * 1024;
/// Rotated files kept beside `debug.log` (`.1` newest .. `.3` oldest).
pub const KEEP: usize = 3;
/// The current file's name.
pub const FILE: &str = "debug.log";
/// Where tetron-wm logged before 0.15.6 (moved on first start, see [`migrate`]).
pub const LEGACY_FILE: &str = "tetron-wm-debug.log";

/// How much is logged. Ordered: a line is written when its level is at or
/// below the configured one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Error = 0,
    Warn = 1,
    /// Events: session start, launches, closes, settings changes, updates.
    #[default]
    Info = 2,
    /// Also every apphost message, wallpaper load and desktop reload.
    Debug = 3,
}

impl Level {
    pub const ALL: [Level; 4] = [Level::Error, Level::Warn, Level::Info, Level::Debug];

    pub fn name(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Info => "info",
            Level::Debug => "debug",
        }
    }

    fn from_u8(v: u8) -> Level {
        Level::ALL.get(v as usize).copied().unwrap_or_default()
    }
}

static LEVEL: AtomicU8 = AtomicU8::new(Level::Info as u8);

/// `TETRON_WM_DEBUG=1` forces `debug` for this run, whatever the config says.
fn forced_debug() -> bool {
    static F: OnceLock<bool> = OnceLock::new();
    *F.get_or_init(|| std::env::var("TETRON_WM_DEBUG").is_ok_and(|v| !v.is_empty() && v != "0"))
}

/// Apply the configured level (daemon and apphost, at start and on reload).
pub fn set_level(l: Level) {
    LEVEL.store(l as u8, Ordering::Relaxed);
}

/// The level in effect.
pub fn level() -> Level {
    if forced_debug() { Level::Debug } else { Level::from_u8(LEVEL.load(Ordering::Relaxed)) }
}

/// Whether a line of this level is written.
pub fn enabled(l: Level) -> bool {
    l <= level()
}

/// Whether this process is a cargo test harness (`target/<profile>/deps/<name>-<hash>`).
fn is_test_harness() -> bool {
    cfg!(test)
        || std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().and_then(|d| d.file_name()).map(|n| n == "deps"))
            .unwrap_or(false)
}

/// The log directory for this platform, or `None` when there is none (no home,
/// or a test run without `TETRON_WM_LOG_DIR`).
pub fn dir() -> Option<PathBuf> {
    static D: OnceLock<Option<PathBuf>> = OnceLock::new();
    D.get_or_init(|| {
        if let Some(d) = std::env::var_os("TETRON_WM_LOG_DIR").filter(|d| !d.is_empty()) {
            return Some(PathBuf::from(d));
        }
        if is_test_harness() {
            return None;
        }
        default_dir()
    })
    .clone()
}

fn default_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        dirs::home_dir().map(|h| h.join("Library/Logs/tetron-wm"))
    } else {
        dirs::state_dir().map(|d| d.join("tetron-wm"))
    }
}

/// The current log file.
pub fn path() -> Option<PathBuf> {
    dir().map(|d| d.join(FILE))
}

/// `debug.log`, then `debug.log.1` .. `.KEEP` (newest first), as names in `dir`.
pub fn files_in(dir: &Path) -> Vec<PathBuf> {
    let mut v = vec![dir.join(FILE)];
    v.extend((1..=KEEP).map(|i| dir.join(format!("{FILE}.{i}"))));
    v
}

/// The path shown to people (Settings, help): `~` for the home directory.
pub fn display_path() -> String {
    let Some(p) = path() else { return "(no log directory)".into() };
    match dirs::home_dir().and_then(|h| p.strip_prefix(&h).ok().map(Path::to_path_buf)) {
        Some(rel) => format!("~/{}", rel.display()),
        None => p.display().to_string(),
    }
}

/// The log file as a quoted shell word, for lines appended by shell commands
/// (the updater, the apps installer); `/dev/null` when there is no log.
pub fn shell_path() -> String {
    path().map(|p| crate::systems::sh_quote(&p.to_string_lossy())).unwrap_or_else(|| "/dev/null".into())
}

thread_local! {
    /// Lines that passed the level filter on this thread, written or not (a
    /// test binary has no log file). Lets a test count what a stretch of use
    /// would write.
    static PASSED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Lines that passed the level filter on this thread so far.
pub fn lines_passed() -> usize {
    PASSED.with(|c| c.get())
}

/// Write one line at `l` if the level allows it.
pub fn write(l: Level, msg: &str) {
    if !enabled(l) {
        return;
    }
    PASSED.with(|c| c.set(c.get() + 1));
    let Some(p) = path() else { return };
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let line = match l {
        Level::Error => format!("{ms} ERROR {msg}"),
        Level::Warn => format!("{ms} WARN {msg}"),
        _ => format!("{ms} {msg}"),
    };
    append(&p, MAX_BYTES, &line);
}

/// Append `line` to `path`, rotating first when the file is past `max` bytes.
/// Creates the directory when it is missing.
pub fn append(path: &Path, max: u64, line: &str) {
    if std::fs::metadata(path).is_ok_and(|m| m.len() >= max) {
        rotate(path);
    }
    let open = || std::fs::OpenOptions::new().create(true).append(true).open(path);
    let f = open().or_else(|_| {
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        open()
    });
    if let Ok(mut f) = f {
        let _ = writeln!(f, "{line}");
    }
}

/// Shift `path.N` to `path.N+1` (dropping `path.KEEP`), then `path` to `path.1`.
/// The daemon and the apphost both write: when both rotate at once, the second
/// finds `path` gone or small and only shifts an almost empty file.
pub fn rotate(path: &Path) {
    let nth = |i: usize| {
        let mut s = path.as_os_str().to_owned();
        s.push(format!(".{i}"));
        PathBuf::from(s)
    };
    let _ = std::fs::remove_file(nth(KEEP));
    for i in (1..KEEP).rev() {
        let _ = std::fs::rename(nth(i), nth(i + 1));
    }
    let _ = std::fs::rename(path, nth(1));
}

/// Move the pre-0.15.6 `~/tetron-wm-debug.log` into `dir` as the next rotated
/// file after the existing ones (`debug.log.1` on a first start), so it ages
/// through `.2` and `.3` like any rotated file and lasts [`KEEP`] rotations.
/// When every slot is taken it stays where it is. Never deletes it: when the
/// move fails (another filesystem) the old file stays too. Returns the new path
/// when it moved.
pub fn migrate(home: &Path, dir: &Path) -> Option<PathBuf> {
    let old = home.join(LEGACY_FILE);
    if !old.is_file() {
        return None;
    }
    // Older than every rotated file: the slot after the highest one present.
    let next = (1..=KEEP).rev().find(|i| dir.join(format!("{FILE}.{i}")).exists()).map_or(1, |i| i + 1);
    if next > KEEP {
        return None;
    }
    let new = dir.join(format!("{FILE}.{next}"));
    std::fs::create_dir_all(dir).ok()?;
    std::fs::rename(&old, &new).ok()?;
    Some(new)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tetron-wm-logfile-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn levels_order_and_names() {
        assert!(Level::Error < Level::Warn && Level::Warn < Level::Info && Level::Info < Level::Debug);
        assert_eq!(Level::default(), Level::Info);
        let names: Vec<_> = Level::ALL.iter().map(|l| l.name()).collect();
        assert_eq!(names, ["error", "warn", "info", "debug"]);
        for l in Level::ALL {
            assert_eq!(Level::from_u8(l as u8), l);
        }
    }

    #[test]
    fn a_test_binary_has_no_log_dir() {
        // The point of the whole change: cargo test must not reach ~/.
        if std::env::var_os("TETRON_WM_LOG_DIR").is_none() {
            assert_eq!(dir(), None);
            assert_eq!(path(), None);
        }
    }

    #[test]
    fn append_creates_the_dir_and_rotates_keeping_four_files() {
        let d = tmp("rotate");
        let p = d.join(FILE);
        // A small cap: each line is 10 bytes, so every 3rd line rotates.
        for i in 0..40 {
            append(&p, 25, &format!("line {i:04}"));
        }
        let files = files_in(&d);
        assert_eq!(files.len(), 4);
        for f in &files {
            assert!(f.is_file(), "{} kept", f.display());
            assert!(std::fs::metadata(f).unwrap().len() <= 30);
        }
        assert!(!d.join(format!("{FILE}.4")).exists(), "the oldest is dropped");
        let cur = std::fs::read_to_string(&p).unwrap();
        assert!(cur.contains("line 0039"), "newest in debug.log: {cur:?}");
        let oldest = std::fs::read_to_string(&files[3]).unwrap();
        assert!(!oldest.contains("line 0000"), "early lines are gone");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn migrate_moves_the_old_home_log_once_and_never_overwrites() {
        let d = tmp("migrate");
        let home = d.join("home");
        let logs = d.join("state/tetron-wm");
        std::fs::create_dir_all(&home).unwrap();
        assert_eq!(migrate(&home, &logs), None, "nothing to move");
        std::fs::write(home.join(LEGACY_FILE), "old\n").unwrap();
        let moved = migrate(&home, &logs).unwrap();
        assert_eq!(moved, logs.join("debug.log.1"), "first start: the newest rotated slot");
        assert_eq!(std::fs::read_to_string(&moved).unwrap(), "old\n");
        assert!(!home.join(LEGACY_FILE).exists());
        // A second old file (an older binary ran again) goes after it, never over it.
        std::fs::write(home.join(LEGACY_FILE), "again\n").unwrap();
        assert_eq!(migrate(&home, &logs), Some(logs.join("debug.log.2")));
        assert_eq!(std::fs::read_to_string(&moved).unwrap(), "old\n");
        std::fs::write(logs.join("debug.log.3"), "x\n").unwrap();
        std::fs::write(home.join(LEGACY_FILE), "third\n").unwrap();
        assert_eq!(migrate(&home, &logs), None, "all slots taken");
        assert!(home.join(LEGACY_FILE).exists(), "left in place, not deleted");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_migrated_log_survives_the_first_rotation() {
        let d = tmp("migrate-rotate");
        let home = d.join("home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join(LEGACY_FILE), "old\n").unwrap();
        migrate(&home, &d).unwrap();
        let p = d.join(FILE);
        for _ in 0..(KEEP - 1) {
            std::fs::write(&p, "full\n").unwrap();
            rotate(&p);
        }
        let oldest = d.join(format!("{FILE}.{KEEP}"));
        assert_eq!(std::fs::read_to_string(&oldest).unwrap(), "old\n", "kept for {KEEP} rotations");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn level_serde_is_lowercase() {
        #[derive(serde::Deserialize)]
        struct C {
            log_level: Level,
        }
        let c: C = toml::from_str("log_level = \"debug\"").unwrap();
        assert_eq!(c.log_level, Level::Debug);
        assert!(toml::from_str::<C>("log_level = \"off\"").is_err(), "no off");
    }
}
