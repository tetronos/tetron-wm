//! The dictation seam in the session: provider events become a paste into the
//! focused app, an overlay line and a tray mic.

use std::time::Duration;
use tetron_wm::config::Config;
use tetron_wm::dictation::DictationEvent as Ev;
use tetron_wm::geometry::Point;
use tetron_wm::session::{ClientMsg, SessionCore};
use tetron_wm::tray::SegmentKind;

fn core() -> SessionCore {
    SessionCore::new(100, 30, Config { dictation_command: "fake-provider".into(), ..Config::default() })
}

#[test]
fn a_commit_is_pasted_into_the_focused_app_and_enter_follows() {
    let out = std::env::temp_dir().join(format!("twm-dictation-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&out);
    let mut core = core();
    core.apply(ClientMsg::Launch { name: "cat".into(), command: "sh".into(), args: vec!["-c".into(), format!("cat > '{}'", out.display())], cwd_prompt: false });
    std::thread::sleep(Duration::from_millis(400));
    core.apply(ClientMsg::Dictation(Ev::Commit { text: "hello world ".into(), enter: false }));
    core.apply(ClientMsg::Dictation(Ev::Commit { text: "second line".into(), enter: true }));
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(std::fs::read_to_string(&out).unwrap_or_default(), "hello world second line\n");
    let _ = std::fs::remove_file(&out);
    core.shutdown();
}

#[test]
fn live_text_shows_in_the_overlay_and_clears_on_commit() {
    let mut core = core();
    let overlay = |c: &SessionCore| {
        c.build_frame().layers.into_iter().find(|l| l.z == 990).map(|l| (0..l.buf.width()).map(|x| l.buf.get(x, 0).unwrap().ch).collect::<String>())
    };
    assert!(overlay(&core).is_none(), "nothing before the provider reports");
    core.apply(ClientMsg::Dictation(Ev::State { state: "listening".into(), mode: Some("segment".into()) }));
    assert!(overlay(&core).unwrap().contains("listening"));
    core.apply(ClientMsg::Dictation(Ev::Partial("open the red".into())));
    assert!(overlay(&core).unwrap().contains("open the red"));
    core.apply(ClientMsg::Dictation(Ev::Commit { text: "open the red door ".into(), enter: false }));
    assert!(overlay(&core).unwrap().contains("listening"), "the live text is gone, still listening");
    core.apply(ClientMsg::Dictation(Ev::State { state: "idle".into(), mode: None }));
    assert!(overlay(&core).is_none());
    core.apply(ClientMsg::Dictation(Ev::Error("no microphone".into())));
    assert!(overlay(&core).unwrap().contains("no microphone"));
    core.shutdown();
}

#[test]
fn a_click_on_the_tray_mic_asks_the_client_to_toggle() {
    let mut core = core();
    let segs = core.tray_segments_for_test();
    let mic = segs.iter().find(|s| s.kind == SegmentKind::Mic).expect("the mic segment");
    assert_eq!(core.take_dictate_request(), None);
    core.apply(ClientMsg::MouseDown(Point::new(mic.rect.x, 0)));
    assert_eq!(core.take_dictate_request().as_deref(), Some("toggle"));
    assert_eq!(core.take_dictate_request(), None, "one frame only");
    core.shutdown();
}

#[test]
fn switched_off_dictation_hides_the_mic_ignores_text_and_stops_the_provider() {
    let mut core = SessionCore::new(100, 30, Config { dictation_command: "fake-provider".into(), dictation_enabled: false, ..Config::default() });
    assert!(core.tray_segments_for_test().iter().all(|s| s.kind != SegmentKind::Mic), "no MIC in the tray when off");
    // Events that arrive anyway (a provider still running) show nothing.
    core.apply(ClientMsg::Dictation(Ev::State { state: "listening".into(), mode: Some("segment".into()) }));
    core.apply(ClientMsg::Dictation(Ev::Partial("ignored".into())));
    assert!(core.build_frame().layers.iter().all(|l| l.z != 990), "no overlay when off");
    assert_eq!(core.take_dictate_request(), None, "nothing to stop when it never reported");
    // A key press while off is answered on the bottom line.
    core.apply(ClientMsg::Dictation(Ev::Error("dictation is off (Settings, Dictation)".into())));
    let note = core.build_frame().layers.into_iter().find(|l| l.z == 990).map(|l| (0..l.buf.width()).map(|x| l.buf.get(x, 0).unwrap().ch).collect::<String>());
    assert!(note.is_some_and(|t| t.contains("dictation is off")));
    core.shutdown();

    // Switched off while listening: the client is told to stop the provider, once.
    let mut core = SessionCore::new(100, 30, Config { dictation_command: "fake-provider".into(), ..Config::default() });
    core.apply(ClientMsg::Dictation(Ev::State { state: "listening".into(), mode: None }));
    assert!(core.tray_segments_for_test().iter().any(|s| s.kind == SegmentKind::Mic));
    let mut cfg = Config { dictation_command: "fake-provider".into(), ..Config::default() };
    cfg.dictation_enabled = false;
    core.set_config_for_test(cfg);
    assert_eq!(core.take_dictate_request().as_deref(), Some("off"));
    assert_eq!(core.take_dictate_request(), None);
    assert!(core.tray_segments_for_test().iter().all(|s| s.kind != SegmentKind::Mic));
    core.shutdown();
}
