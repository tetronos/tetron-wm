//! Mirror the real GUI clipboard (X11 or Wayland) with the tetron-wm one, on a
//! machine that has a display: copies made in a browser or by a GUI app land in
//! the history, and entries chosen from the history reach GUI apps. A background
//! thread polls with short, time-limited helper commands, so the render loop
//! never waits on it. See `clipboard_gui_bridge`.

use crate::clipboard::{lock, ClipSource, SharedClip};
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    X11,
    Wayland,
}

/// Largest clipboard text mirrored (bytes).
const MAX_BYTES: usize = 1024 * 1024;

fn have(cmd: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(cmd).is_file()))
        .unwrap_or(false)
}

fn set_env(name: &str) -> bool { std::env::var_os(name).is_some_and(|v| !v.is_empty()) }

/// Pick a backend for `setting` ("auto" | "on" | "off") from the environment.
pub fn detect(setting: &str) -> Option<Backend> {
    if setting == "off" {
        return None;
    }
    if set_env("WAYLAND_DISPLAY") && have("wl-paste") && have("wl-copy") {
        return Some(Backend::Wayland);
    }
    if set_env("DISPLAY") && have("xclip") {
        return Some(Backend::X11);
    }
    None
}

/// Read the GUI clipboard as text. `None` when empty, not text, or the helper failed.
pub fn read(b: Backend) -> Option<String> {
    let (prog, args): (&str, &[&str]) = match b {
        Backend::X11 => ("xclip", &["-o", "-selection", "clipboard"]),
        Backend::Wayland => ("wl-paste", &["--no-newline"]),
    };
    let t = crate::system::run_capped(prog, args, 2)?;
    (!t.is_empty() && t.len() <= MAX_BYTES).then_some(t)
}

/// Set the GUI clipboard. Output is discarded: `xclip -i` forks a background
/// process to own the selection, which would keep a pipe open forever.
pub fn write(b: Backend, text: &str) -> bool {
    let (prog, args): (&str, &[&str]) = match b {
        Backend::X11 => ("xclip", &["-i", "-selection", "clipboard"]),
        Backend::Wayland => ("wl-copy", &[]),
    };
    let Ok(mut child) = Command::new(prog).args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else {
        return false;
    };
    if let Some(mut si) = child.stdin.take() {
        if si.write_all(text.as_bytes()).is_err() {
            crate::system::kill_and_reap(child);
            return false;
        }
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return st.success(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                crate::system::kill_and_reap(child);
                return false;
            }
        }
    }
}

/// One synchronisation step. The GUI wins when it changed since the last step
/// (recorded as a `gui` entry and made the clipboard); otherwise a clipboard
/// changed inside tetron-wm is pushed out to the GUI.
pub fn sync_step(shared: &SharedClip, gui: Option<String>, mut write_gui: impl FnMut(&str) -> bool) {
    let mut s = lock(shared);
    if !s.enabled {
        return;
    }
    if let Some(g) = gui {
        if g != s.gui_synced {
            s.gui_synced = g.clone();
            if g != s.clipboard {
                s.history.record(&g, ClipSource::Gui);
                s.clipboard = g;
            }
            return;
        }
    }
    if !s.clipboard.is_empty() && s.clipboard != s.gui_synced && s.clipboard.len() <= MAX_BYTES {
        let text = s.clipboard.clone();
        // Mark synced first so a failed helper is not retried every tick.
        s.gui_synced = text.clone();
        drop(s);
        write_gui(&text);
    }
}

/// Start the polling thread.
pub fn spawn(shared: SharedClip, b: Backend) {
    crate::dbg_log(&format!("clipboard: GUI bridge started ({b:?})"));
    std::thread::spawn(move || {
        let mut ok_once = false;
        let mut fails = 0u32;
        loop {
            let g = read(b);
            if g.is_some() {
                ok_once = true;
            } else if !ok_once {
                fails += 1;
                if fails == 20 {
                    crate::log_warn("clipboard: GUI bridge cannot read the clipboard (compositor without data-control?)");
                }
            }
            sync_step(&shared, g, |t| write(b, t));
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::{new_shared, ClipLimits};

    fn shared() -> SharedClip { new_shared(ClipLimits::default(), true) }

    #[test]
    fn gui_copy_is_recorded_and_becomes_the_clipboard() {
        let c = shared();
        sync_step(&c, Some("from browser".into()), |_| panic!("must not write back"));
        let s = lock(&c);
        assert_eq!(s.clipboard, "from browser");
        assert_eq!(s.history.get(0).unwrap().source, ClipSource::Gui);
    }

    #[test]
    fn unchanged_gui_clipboard_is_not_rerecorded() {
        let c = shared();
        sync_step(&c, Some("x".into()), |_| true);
        sync_step(&c, Some("x".into()), |_| true);
        assert_eq!(lock(&c).history.len(), 1);
    }

    #[test]
    fn internal_change_is_written_to_the_gui_once() {
        let c = shared();
        sync_step(&c, Some("old".into()), |_| true);
        lock(&c).clipboard = "from tetron".into();
        let mut written = Vec::new();
        sync_step(&c, Some("old".into()), |t| { written.push(t.to_string()); true });
        sync_step(&c, Some("old".into()), |t| { written.push(t.to_string()); true });
        assert_eq!(written, vec!["from tetron".to_string()], "written exactly once");
    }

    #[test]
    fn the_gui_echo_of_our_own_write_is_not_a_new_entry() {
        let c = shared();
        lock(&c).clipboard = "ours".into();
        sync_step(&c, None, |_| true); // pushed out
        sync_step(&c, Some("ours".into()), |_| panic!("no rewrite")); // GUI now shows it
        assert!(lock(&c).history.is_empty(), "history only records via the owner of the copy");
    }

    #[test]
    fn disabled_does_nothing() {
        let c = new_shared(ClipLimits::default(), false);
        sync_step(&c, Some("x".into()), |_| panic!("no write"));
        assert!(lock(&c).history.is_empty());
    }

    #[test]
    fn detect_off_is_none() {
        assert_eq!(detect("off"), None);
    }
}
