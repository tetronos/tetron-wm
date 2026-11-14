use std::time::{Duration, Instant};
use tetron_wm::config::Config;
use tetron_wm::session::{ClientMsg, SessionCore};

/// Pump the session until `done` is true or two seconds pass.
fn pump_until(core: &mut SessionCore, mut done: impl FnMut(&SessionCore) -> bool) -> bool {
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(3) {
        core.pump_app_events();
        if done(core) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    false
}

fn launch(core: &mut SessionCore, script: &str) {
    core.apply(ClientMsg::Launch { name: "t".into(), command: "sh".into(), args: vec!["-c".into(), script.into()], cwd_prompt: false });
}

#[test]
fn osc52_store_is_recorded_in_history() {
    let mut core = SessionCore::new(80, 24, Config::default());
    // "hello" in base64 is aGVsbG8=
    launch(&mut core, r"printf '\033]52;c;aGVsbG8=\033\\'; sleep 3");
    assert!(pump_until(&mut core, |c| c.clip_history().len() == 1), "store recorded");
    let hist = core.clip_history();
    let e = hist.get(0).unwrap();
    assert_eq!(e.text, "hello");
    assert_eq!(e.source, tetron_wm::clipboard::ClipSource::App);
    core.shutdown();
}

#[test]
fn history_off_records_nothing() {
    let cfg = Config { clipboard_history: false, ..Config::default() };
    let mut core = SessionCore::new(80, 24, cfg);
    launch(&mut core, r"printf '\033]52;c;aGVsbG8=\033\\'; sleep 2");
    assert!(!pump_until(&mut core, |c| c.clip_history().len() == 1));
    core.shutdown();
}

#[test]
fn ignore_apps_skips_recording() {
    let cfg = Config { clipboard_ignore_apps: vec!["secret".into()], ..Config::default() };
    let mut core = SessionCore::new(80, 24, cfg);
    // The window title is "t"; the command line carries the marker.
    launch(&mut core, r"# secret-manager
printf '\033]52;c;aGVsbG8=\033\\'; sleep 2");
    assert!(!pump_until(&mut core, |c| c.clip_history().len() == 1));
    core.shutdown();
}

/// An app that asks for the clipboard with `ESC ] 52 ; c ; ? BEL` gets an OSC 52
/// reply carrying the current clipboard.
#[test]
fn osc52_read_is_answered_with_the_clipboard() {
    let out = std::env::temp_dir().join(format!("twm-osc52-{}", std::process::id()));
    let _ = std::fs::remove_file(&out);
    let mut core = SessionCore::new(80, 24, Config::default());
    // First window sets the clipboard, second one reads it (and is focused).
    launch(&mut core, r"printf '\033]52;c;aGVsbG8=\033\\'; sleep 4");
    assert!(pump_until(&mut core, |c| c.clip_history().len() == 1));
    let script = format!(
        r"stty raw -echo; printf '\033]52;c;?\007'; dd bs=1 count=16 2>/dev/null > {}; sleep 1",
        out.display()
    );
    launch(&mut core, &script);
    let got = pump_until(&mut core, |_| std::fs::metadata(&out).map(|m| m.len() >= 16).unwrap_or(false));
    assert!(got, "reply written to the app's stdin");
    assert_eq!(std::fs::read(&out).unwrap(), b"\x1b]52;c;aGVsbG8=\x07");
    let _ = std::fs::remove_file(&out);
    core.shutdown();
}

#[test]
fn osc52_read_never_is_not_answered() {
    let out = std::env::temp_dir().join(format!("twm-osc52n-{}", std::process::id()));
    let _ = std::fs::remove_file(&out);
    let cfg = Config { clipboard_osc52_read: "never".into(), ..Config::default() };
    let mut core = SessionCore::new(80, 24, cfg);
    let script = format!(
        r"stty raw -echo; printf '\033]52;c;?\007'; dd bs=1 count=1 2>/dev/null > {} & sleep 1.5; kill %1 2>/dev/null",
        out.display()
    );
    launch(&mut core, &script);
    let got = pump_until(&mut core, |_| std::fs::metadata(&out).map(|m| m.len() > 0).unwrap_or(false));
    assert!(!got, "no reply when reads are denied");
    let _ = std::fs::remove_file(&out);
    core.shutdown();
}

#[test]
fn persistence_round_trip_through_a_session() {
    let dir = std::env::temp_dir().join(format!("twm-clip-sess-{}", std::process::id()));
    let path = dir.join("h.jsonl");
    let mut core = SessionCore::new(80, 24, Config::default());
    core.enable_clipboard_persistence(path.clone());
    launch(&mut core, r"printf '\033]52;c;aGVsbG8=\033\\'; sleep 3");
    assert!(pump_until(&mut core, |c| c.clip_history().len() == 1));
    core.flush_clipboard();
    core.shutdown();
    let mut again = SessionCore::new(80, 24, Config::default());
    again.enable_clipboard_persistence(path.clone());
    let h = again.clip_history();
    assert_eq!(h.get(0).map(|e| e.text.as_str()), Some("hello"));
    again.shutdown();
    let off = Config { clipboard_persist: false, ..Config::default() };
    let mut mem = SessionCore::new(80, 24, off);
    mem.enable_clipboard_persistence(path);
    assert!(mem.clip_history().is_empty(), "persist off: nothing loaded");
    mem.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

fn seed(core: &SessionCore, texts: &[&str]) {
    for t in texts {
        tetron_wm::clipboard::handle_cmd(
            &core.clip_handle(),
            tetron_wm::clipboard::ClipCmd::Copy { text: t.to_string(), primary: false },
        );
    }
}

fn key(core: &mut SessionCore, k: tetron_wm::clipmenu::ClipKey) {
    core.apply(ClientMsg::ClipMenuKey(k));
}

/// Launch an app that reads `n` raw bytes into `out`, optionally enabling bracketed paste first.
fn launch_reader(core: &mut SessionCore, out: &std::path::Path, n: usize, bracketed: bool) {
    let pre = if bracketed { r"printf '\033[?2004h'; " } else { "" };
    launch(core, &format!("{pre}stty raw -echo; dd bs=1 count={n} 2>/dev/null > {}; sleep 1", out.display()));
}

fn tmp(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("twm-{name}-{}", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

#[test]
fn popover_enter_pastes_the_selected_entry_into_the_focused_app() {
    use tetron_wm::clipmenu::ClipKey;
    let out = tmp("menu-paste");
    let mut core = SessionCore::new(100, 30, Config::default());
    seed(&core, &["older", "newer"]);
    launch_reader(&mut core, &out, 5, false);
    std::thread::sleep(Duration::from_millis(300));
    core.apply(ClientMsg::ClipMenuToggle);
    assert!(core.clipmenu_open());
    key(&mut core, ClipKey::Down); // "older"
    key(&mut core, ClipKey::Enter);
    assert!(!core.clipmenu_open(), "closes after pasting");
    assert!(pump_until(&mut core, |_| std::fs::metadata(&out).map(|m| m.len() >= 5).unwrap_or(false)));
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "older");
    assert_eq!(core.clip_history().get(0).unwrap().text, "older", "chosen entry moves to the front");
    let _ = std::fs::remove_file(&out);
    core.shutdown();
}

#[test]
fn paste_is_bracketed_when_the_app_asks_for_it() {
    use tetron_wm::clipmenu::ClipKey;
    let out = tmp("menu-bracketed");
    let mut core = SessionCore::new(100, 30, Config::default());
    seed(&core, &["a\nb"]);
    launch_reader(&mut core, &out, 15, true);
    std::thread::sleep(Duration::from_millis(500));
    core.apply(ClientMsg::ClipMenuToggle);
    key(&mut core, ClipKey::Enter);
    assert!(pump_until(&mut core, |_| std::fs::metadata(&out).map(|m| m.len() >= 15).unwrap_or(false)));
    assert_eq!(std::fs::read(&out).unwrap(), b"\x1b[200~a\nb\x1b[201~");
    let _ = std::fs::remove_file(&out);
    core.shutdown();
}

#[test]
fn popover_pin_delete_pause_filter_and_esc() {
    use tetron_wm::clipmenu::ClipKey;
    let mut core = SessionCore::new(100, 30, Config::default());
    seed(&core, &["alpha", "beta", "gamma"]); // newest first: gamma, beta, alpha
    core.apply(ClientMsg::ClipMenuToggle);
    key(&mut core, ClipKey::Down);
    key(&mut core, ClipKey::Pin); // beta pinned -> moves first
    assert_eq!(core.clip_history().get(0).unwrap().text, "beta");
    assert!(core.clip_history().get(0).unwrap().pinned);
    key(&mut core, ClipKey::Delete);
    assert_eq!(core.clip_history().len(), 2);
    key(&mut core, ClipKey::Pause);
    assert!(core.clip_history().paused());
    key(&mut core, ClipKey::Char('z')); // filter matches nothing
    key(&mut core, ClipKey::Enter);
    assert!(core.clipmenu_open(), "nothing selected, nothing happens");
    key(&mut core, ClipKey::Esc); // clears filter
    assert!(core.clipmenu_open());
    key(&mut core, ClipKey::Esc); // closes
    assert!(!core.clipmenu_open());
    core.shutdown();
}

#[test]
fn clicking_the_menubar_segment_toggles_the_popover() {
    use tetron_wm::geometry::Point;
    let mut core = SessionCore::new(100, 30, Config::default());
    seed(&core, &["x"]);
    let mut found = None;
    for x in 0..100 {
        core.apply(ClientMsg::MouseDown(Point::new(x, 0)));
        if core.clipmenu_open() {
            found = Some(x);
            break;
        }
    }
    assert!(found.is_some(), "some menubar cell opens the clipboard popover");
    // A click outside closes it.
    core.apply(ClientMsg::MouseDown(Point::new(5, 25)));
    assert!(!core.clipmenu_open());
    core.shutdown();
}

#[test]
fn history_off_hides_the_popover() {
    let cfg = Config { clipboard_history: false, ..Config::default() };
    let mut core = SessionCore::new(100, 30, cfg);
    core.apply(ClientMsg::ClipMenuToggle);
    assert!(!core.clipmenu_open());
    core.shutdown();
}

#[test]
fn frame_with_popover_open_renders() {
    let mut core = SessionCore::new(100, 30, Config::default());
    seed(&core, &["one", "two\nlines", "three"]);
    core.apply(ClientMsg::ClipMenuToggle);
    let with = core.build_frame().layers.len();
    core.apply(ClientMsg::ClipMenuToggle);
    let without = core.build_frame().layers.len();
    assert_eq!(with, without + 1, "the popover adds exactly one layer");
    core.shutdown();
}

#[test]
fn settings_toggle_persistence_and_limits_apply_live() {
    let dir = std::env::temp_dir().join(format!("twm-clip-live-{}", std::process::id()));
    let path = dir.join("h.jsonl");
    let mut core = SessionCore::new(80, 24, Config::default());
    core.enable_clipboard_persistence(path.clone());
    seed(&core, &["a", "b", "c"]);
    core.flush_clipboard();
    assert!(path.exists());
    // Persistence off: the file is deleted, memory keeps the history.
    core.set_config_for_test(Config { clipboard_persist: false, ..Config::default() });
    assert!(!path.exists());
    assert_eq!(core.clip_history().len(), 3);
    // Back on: written again.
    core.set_config_for_test(Config::default());
    assert!(path.exists());
    // Smaller limit trims the oldest.
    core.set_config_for_test(Config { clipboard_history_size: 2, ..Config::default() });
    assert_eq!(core.clip_history().len(), 2);
    // History off hides new records.
    core.set_config_for_test(Config { clipboard_history: false, ..Config::default() });
    seed(&core, &["zzz"]);
    assert!(core.clip_history().entries().iter().all(|e| e.text != "zzz"));
    core.shutdown();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mouse_selection_in_an_app_is_recorded() {
    use tetron_wm::geometry::Point;
    let mut core = SessionCore::new(100, 30, Config::default());
    launch(&mut core, "echo hello world; sleep 4");
    std::thread::sleep(Duration::from_millis(500));
    let r = core.focused_window_rect_for_test().expect("a window");
    // Content starts at the window left edge (no left border), below the titlebar.
    let (x0, y0) = (r.x, r.y + 1);
    core.apply(ClientMsg::MouseDown(Point::new(x0, y0)));
    core.apply(ClientMsg::MouseDrag(Point::new(x0 + 4, y0)));
    core.apply(ClientMsg::MouseUp(Point::new(x0 + 4, y0)));
    let hist = core.clip_history();
    assert_eq!(hist.get(0).map(|e| e.text.as_str()), Some("hello"), "history: {:?}", hist.entries());
    assert_eq!(hist.get(0).unwrap().source, tetron_wm::clipboard::ClipSource::Selection);
    core.shutdown();
}

#[test]
fn select_mode_stops_mouse_passthrough_until_a_selection_is_made() {
    use tetron_wm::geometry::Point;
    let mut core = SessionCore::new(100, 30, Config::default());
    // An app that turns on mouse reporting (like w3m) and prints some text.
    launch(&mut core, r"printf '\033[?1000h\033[?1006hhello world\n'; sleep 4");
    let t0 = Instant::now();
    while core.app_mouse_area().is_none() && t0.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(30));
    }
    assert!(core.app_mouse_area().is_some(), "the app captures the mouse");
    core.apply(ClientMsg::ToggleSelectMode);
    assert!(core.select_mode());
    assert!(core.app_mouse_area().is_none(), "select mode: no passthrough");
    let r = core.focused_window_rect_for_test().unwrap();
    let (x0, y0) = (r.x, r.y + 1);
    core.apply(ClientMsg::MouseDown(Point::new(x0, y0)));
    core.apply(ClientMsg::MouseDrag(Point::new(x0 + 4, y0)));
    core.apply(ClientMsg::MouseUp(Point::new(x0 + 4, y0)));
    assert!(!core.select_mode(), "one-shot: cleared after the selection");
    assert!(core.app_mouse_area().is_some(), "passthrough restored");
    assert_eq!(core.clip_history().get(0).map(|e| e.text.clone()), Some("hello".to_string()));
    core.shutdown();
}

#[test]
fn clipboard_popover_select_button_starts_select_mode() {
    use tetron_wm::geometry::Point;
    let mut core = SessionCore::new(100, 30, Config::default());
    core.apply(ClientMsg::ClipMenuToggle);
    assert!(core.clipmenu_open());
    assert!(!core.select_mode());
    let btn = core.clipmenu_select_button_for_test().expect("popover open");
    core.apply(ClientMsg::MouseDown(btn));
    assert!(core.select_mode(), "[Select] starts select mode, like Ctrl+Space x");
    assert!(!core.clipmenu_open(), "popover closes so the drag lands on the app");
    let _: Point = btn;
    core.shutdown();
}

#[test]
fn clicking_the_select_marker_cancels_select_mode() {
    use tetron_wm::geometry::Point;
    use tetron_wm::system::SystemState;
    use tetron_wm::tray::{tray_segments_with, SegmentKind};
    let mut core = SessionCore::new(100, 30, Config::default());
    core.apply(ClientMsg::ToggleSelectMode);
    assert!(core.select_mode());
    // The session lays the tray out from its (default) snapshot; do the same.
    let segs = tray_segments_with(&SystemState::default(), 100, 0, 0, Some((0, false)), true);
    let sel = segs.iter().find(|s| s.kind == SegmentKind::Select).expect("SELECT segment");
    core.apply(ClientMsg::MouseDown(Point::new(sel.rect.x, 0)));
    assert!(!core.select_mode(), "clicking SELECT cancels it");
    core.shutdown();
}

/// Real w3m (skipped when not installed): select-mode drag over its page text
/// lands in the history.
#[test]
fn select_mode_selects_text_out_of_w3m() {
    use tetron_wm::geometry::Point;
    if std::process::Command::new("w3m").arg("-version").output().is_err() {
        eprintln!("w3m not installed; skipping");
        return;
    }
    let page = std::env::temp_dir().join(format!("twm-w3m-{}.html", std::process::id()));
    std::fs::write(&page, "<html><body><p>Copy this sentence from the web</p></body></html>").unwrap();
    let mut core = SessionCore::new(100, 30, Config::default());
    core.apply(ClientMsg::Launch { name: "w3m".into(), command: "w3m".into(), args: vec![page.display().to_string()], cwd_prompt: false });
    std::thread::sleep(Duration::from_millis(1500));
    let mouse_captured = core.app_mouse_area().is_some();
    core.apply(ClientMsg::ToggleSelectMode);
    let r = core.focused_window_rect_for_test().unwrap();
    // The text is on the first content row; select across it.
    let (x0, y0) = (r.x, r.y + 1);
    core.apply(ClientMsg::MouseDown(Point::new(x0, y0)));
    core.apply(ClientMsg::MouseDrag(Point::new(x0 + 12, y0)));
    core.apply(ClientMsg::MouseUp(Point::new(x0 + 12, y0)));
    let hist = core.clip_history();
    eprintln!("w3m captures mouse: {mouse_captured}; history: {:?}", hist.entries());
    assert_eq!(hist.get(0).map(|e| e.text.clone()), Some("Copy this sen".to_string()));
    let _ = std::fs::remove_file(&page);
    core.shutdown();
}

#[test]
fn bare_w3m_launch_gets_the_web_home_page() {
    if std::process::Command::new("w3m").arg("-version").output().is_err() {
        eprintln!("w3m not installed; skipping");
        return;
    }
    let mut core = SessionCore::new(100, 30, Config::default());
    core.apply(ClientMsg::Launch { name: "w3m".into(), command: "w3m".into(), args: vec![], cwd_prompt: false });
    let (_cmd, args) = core.focused_app_launch_cmd_for_test().expect("launched");
    assert_eq!(args, vec!["https://lite.duckduckgo.com/lite/".to_string()]);
    core.shutdown();
    // An explicit URL, and an empty web_home, are left alone.
    let mut core = SessionCore::new(100, 30, Config::default());
    core.apply(ClientMsg::Launch { name: "w3m".into(), command: "w3m".into(), args: vec!["file:///x".into()], cwd_prompt: false });
    assert_eq!(core.focused_app_launch_cmd_for_test().unwrap().1, vec!["file:///x".to_string()]);
    core.shutdown();
    let mut core = SessionCore::new(100, 30, Config { web_home: String::new(), ..Config::default() });
    core.apply(ClientMsg::Launch { name: "w3m".into(), command: "w3m".into(), args: vec![], cwd_prompt: false });
    assert!(core.focused_app_launch_cmd_for_test().unwrap().1.is_empty());
    core.shutdown();
}
