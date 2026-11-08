pub mod clipboard;
pub mod clipbridge;
pub mod clipcli;
pub mod clipmenu;
pub mod keymap;
pub mod cli_help;
pub mod geometry;
pub mod cell;
pub mod theme;
pub mod themefetch;
pub mod listfetch;
pub mod packaging;
pub mod display;
pub mod buffer;
pub mod wallpaper;
pub mod compositor;
pub mod terminal;
pub mod ptyhost;
pub mod apphost;
pub mod window;
pub mod winpicker;
pub mod wm;
pub mod chrome;
pub mod catalog;
pub mod store;
pub mod settings;
pub mod activity;
pub mod launcher;
pub mod powermenu;
pub mod powerctl;
pub mod confirmclose;
pub mod launchwarn;
pub mod input;
pub mod screenshot;
pub mod screensaver;
pub mod session;
pub mod config;
pub mod configfile;
pub mod state;
pub mod system;
pub mod poller;
pub mod tray;
pub mod traytools;
pub mod dirpicker;
pub mod help;
pub mod imagestore;
pub mod thumbnail;
pub mod icons;
pub mod glyphs;
pub mod iconset;
pub mod kitty;
pub mod kittygfx;
pub mod contextmenu;
pub mod fullscreen;
pub mod imageview;
pub mod openwith;
pub mod fileops;
pub mod filemanager;
pub mod desktop;
pub mod mouse;
pub mod gpm;
pub mod badge;
pub mod service;
pub mod toolchain;
pub mod systems;
pub mod calendar;
pub mod logsview;
pub mod logfile;
pub mod brightness;
pub mod assistant;

/// The crate version (from Cargo.toml).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The git commit this binary was built from (stamped by `build.rs`).
pub const GIT_SHA: &str = env!("TETRON_WM_GIT_SHA");
/// Repository the in-app updater checks/installs from (the tetron-wm release
/// repo). The updater must only ever point here.
pub const REPO_URL: &str = "https://github.com/tetronos/tetron-wm";

/// Log an event (level `info`) to the tetron-wm log (see [`logfile`]). Always
/// on at the default level: the in-app Logs viewer (launcher → tetron-wm →
/// Logs) and the Assistant read this file, so there must be something to show
/// without re-running with an env var. The file is rotated at 4 MiB, four
/// files kept (16 MiB), oldest dropped.
pub fn dbg_log(msg: &str) {
    logfile::write(logfile::Level::Info, msg);
}

/// Log a verbose line (level `debug`: repeats, every message, every reload).
pub fn log_debug(msg: &str) {
    logfile::write(logfile::Level::Debug, msg);
}

/// Log something that went wrong but tetron-wm carries on (level `warn`).
pub fn log_warn(msg: &str) {
    logfile::write(logfile::Level::Warn, msg);
}

/// Log `msg` at `info` the first time and whenever `value` differs from the
/// last one seen for `kind` in this process, at `debug` otherwise. For lines
/// that repeat on every refresh (wallpaper loads, desktop reloads, systems
/// loads): the log keeps the change, not the repeats.
pub fn log_changed(kind: &'static str, value: &str, msg: &str) {
    use std::collections::HashMap;
    use std::sync::Mutex;
    static LAST: Mutex<Option<HashMap<&'static str, String>>> = Mutex::new(None);
    let changed = {
        let mut g = LAST.lock().unwrap_or_else(|e| e.into_inner());
        let m = g.get_or_insert_with(HashMap::new);
        if m.get(kind).map(String::as_str) == Some(value) {
            false
        } else {
            m.insert(kind, value.to_string());
            true
        }
    };
    logfile::write(if changed { logfile::Level::Info } else { logfile::Level::Debug }, msg);
}

/// Log a failure (level `error`).
pub fn log_error(msg: &str) {
    logfile::write(logfile::Level::Error, msg);
}

/// Append a session-start banner to the log (running version, git sha, and
/// binary path). Called once at daemon startup. Moves the pre-0.15.6
/// `~/tetron-wm-debug.log` into the log directory first (see [`logfile::migrate`]).
///
/// Deliberately APPENDS rather than truncates: when an in-app update reloads
/// the daemon, the previous session's trace (the `update:` steps, the reload)
/// must survive alongside the new banner so the *whole* update is visible in
/// one log — truncating here is what made update failures invisible (the new
/// daemon wiped the evidence). Rotation keeps the files bounded.
pub fn dbg_init() {
    // Route every panic into the debug log before the default handler runs. The
    // daemon/apphost stderr is not captured (the console is torn down on exit),
    // so without this a panic leaves no trace — which is exactly why an earlier
    // pane-induced crash was invisible. Chains to the default hook so behaviour
    // is otherwise unchanged.
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log_error(&format!("PANIC: {info}"));
        default(info);
    }));
    if let (Some(home), Some(dir)) = (dirs::home_dir(), logfile::dir()) {
        if let Some(to) = logfile::migrate(&home, &dir) {
            dbg_log(&format!("log: moved ~/{} to {}", logfile::LEGACY_FILE, to.display()));
        }
    }
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "?".into());
    dbg_log(&format!("=== tetron-wm session start (v{VERSION}, git {GIT_SHA}, exe {exe}) ==="));
}
/// The path of the running tetron-wm binary, for re-running itself (daemon,
/// apphost, shims). After the binary is replaced on disk while running (an
/// install or update), Linux reports the old path as "PATH (deleted)"; that file
/// does not exist, so a reload could not start the daemon again. Use the
/// installed binary at the same path instead.
pub fn self_exe() -> std::io::Result<std::path::PathBuf> {
    let exe = std::env::current_exe()?;
    Ok(undeleted(&exe).unwrap_or(exe))
}

/// `PATH` for "PATH (deleted)" when a file now exists at `PATH`.
fn undeleted(exe: &std::path::Path) -> Option<std::path::PathBuf> {
    let s = exe.to_str()?;
    let live = std::path::PathBuf::from(s.strip_suffix(" (deleted)")?);
    live.exists().then_some(live)
}

#[cfg(test)]
mod self_exe_tests {
    #[test]
    fn a_replaced_binary_resolves_to_the_new_file() {
        let dir = std::env::temp_dir().join(format!("tetron-wm-selfexe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("tetron-wm");
        std::fs::write(&bin, b"").unwrap();
        let deleted = std::path::PathBuf::from(format!("{} (deleted)", bin.display()));
        assert_eq!(super::undeleted(&deleted), Some(bin.clone()));
        assert_eq!(super::undeleted(&bin), None, "a normal path is left alone");
        std::fs::remove_file(&bin).unwrap();
        assert_eq!(super::undeleted(&deleted), None, "nothing installed there: keep the original");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

pub mod protocol;
pub mod daemon;
pub mod client;
