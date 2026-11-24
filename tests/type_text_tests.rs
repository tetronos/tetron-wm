//! `tetron-wm type`: text from a program (the tetron-stt TUI) is pasted into the
//! focused app; from the focused app itself it goes to the clipboard instead.

use std::time::Duration;
use tetron_wm::config::Config;
use tetron_wm::session::{ClientMsg, SessionCore};

fn launch(core: &mut SessionCore, script: &str) {
    core.apply(ClientMsg::Launch { name: "t".into(), command: "sh".into(), args: vec!["-c".into(), script.into()], cwd_prompt: false });
    std::thread::sleep(Duration::from_millis(400));
}

/// The pid of the process whose command line holds `marker` (the launched sh).
fn pid_with(marker: &str) -> u32 {
    for _ in 0..30 {
        for e in std::fs::read_dir("/proc").unwrap().flatten() {
            let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else { continue };
            let cmd = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
            if pid != std::process::id() && String::from_utf8_lossy(&cmd).contains(marker) {
                return pid;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("no process with {marker}");
}

#[test]
fn text_is_pasted_into_the_focused_app_and_enter_follows() {
    let out = std::env::temp_dir().join(format!("twm-type-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&out);
    let mut core = SessionCore::new(100, 30, Config::default());
    launch(&mut core, &format!("cat > '{}'", out.display()));
    core.apply(ClientMsg::TypeText { text: "hello ".into(), enter: false, pid: None });
    core.apply(ClientMsg::TypeText { text: "world".into(), enter: true, pid: Some(std::process::id()) });
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(std::fs::read_to_string(&out).unwrap_or_default(), "hello world\n");
    assert!(core.clip_history().entries().is_empty(), "clipboard untouched");
    let _ = std::fs::remove_file(&out);
    core.shutdown();
}

#[test]
fn from_the_focused_app_itself_the_text_goes_to_the_clipboard() {
    let out = std::env::temp_dir().join(format!("twm-type-self-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&out);
    let marker = format!("twm-type-self-marker-{}", std::process::id());
    let mut core = SessionCore::new(100, 30, Config::default());
    launch(&mut core, &format!("cat > '{}' # {marker}", out.display()));
    let app = pid_with(&marker);
    core.apply(ClientMsg::TypeText { text: "not for me".into(), enter: true, pid: Some(app) });
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(std::fs::read_to_string(&out).unwrap_or_default(), "", "nothing typed into the caller");
    assert_eq!(core.clip_history().get(0).map(|e| e.text.as_str()), Some("not for me"));
    let _ = std::fs::remove_file(&out);
    core.shutdown();
}

#[test]
fn type_text_json_defaults() {
    let m: ClientMsg = serde_json::from_str(r#"{"TypeText":{"text":"hi"}}"#).unwrap();
    assert!(matches!(m, ClientMsg::TypeText { ref text, enter: false, pid: None } if text == "hi"));
}
