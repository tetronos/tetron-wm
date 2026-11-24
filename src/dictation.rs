//! Dictation: an optional addon seam. A provider program (`tetron-stt serve`)
//! speaks JSON lines on its stdin and stdout. The client, which owns the real
//! terminal and so the microphone, starts it and forwards what it says to the
//! daemon as [`DictationEvent`]s. The daemon pastes commits into the focused
//! window, shows the live text in a one line overlay and the state in the tray.
//! tetron-wm contains no speech code, no model and no audio dependency.
//!
//! Provider to tetron-wm: `{"state":"idle|loading|listening|finishing","mode":"segment"}`,
//! `{"partial":"text so far"}`, `{"commit":"text","enter":false}`, `{"error":"..."}`,
//! `{"info":"..."}`. tetron-wm to provider: `{"cmd":"start"}`, `{"cmd":"stop"}`,
//! `{"cmd":"toggle"}`, `{"cmd":"mode"}`, `{"cmd":"mode","mode":"final"}`, `{"cmd":"quit"}`.
//! EOF on the provider's stdin quits it.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};

/// What the provider reported, as the client forwards it to the daemon.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum DictationEvent {
    /// The provider's state and mode.
    State { state: String, mode: Option<String> },
    /// The live text so far (replaces the previous partial).
    Partial(String),
    /// Final text to paste into the focused window; `enter` presses Enter after it.
    Commit { text: String, enter: bool },
    /// Something went wrong (no microphone, models missing, provider not installed).
    Error(String),
    /// A note from the provider (shown briefly).
    Info(String),
    /// The provider process ended; the next toggle starts it again.
    Gone,
}

/// Parse one line the provider printed. Unknown lines and unknown fields are
/// ignored (`None`), so the protocol can grow.
pub fn parse_provider_line(line: &str) -> Option<DictationEvent> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let obj = v.as_object()?;
    if let Some(c) = obj.get("commit").and_then(|c| c.as_str()) {
        let enter = obj.get("enter").and_then(|e| e.as_bool()).unwrap_or(false);
        return Some(DictationEvent::Commit { text: c.to_string(), enter });
    }
    if let Some(p) = obj.get("partial").and_then(|p| p.as_str()) {
        return Some(DictationEvent::Partial(p.to_string()));
    }
    if let Some(e) = obj.get("error").and_then(|e| e.as_str()) {
        return Some(DictationEvent::Error(e.to_string()));
    }
    if let Some(s) = obj.get("state").and_then(|s| s.as_str()) {
        let mode = obj.get("mode").and_then(|m| m.as_str()).map(str::to_string);
        return Some(DictationEvent::State { state: s.to_string(), mode });
    }
    obj.get("info").and_then(|i| i.as_str()).map(|i| DictationEvent::Info(i.to_string()))
}

/// How long an error or info note stays in the overlay.
pub const NOTE_SECS: u64 = 8;

/// What the daemon knows about dictation: shown in the tray and the overlay.
#[derive(Clone, Debug, Default)]
pub struct DictationState {
    /// The provider's state; empty before it has reported anything.
    pub state: String,
    /// The provider's mode (`segment` or `final`), as reported.
    pub mode: String,
    /// Live text, cleared on commit and when listening stops.
    pub partial: String,
    /// An error or info note and when it arrived.
    pub note: Option<(String, std::time::Instant, bool)>,
}

impl DictationState {
    /// Apply an event. Returns the text to paste (and whether to press Enter) for a commit.
    pub fn apply(&mut self, ev: DictationEvent) -> Option<(String, bool)> {
        match ev {
            DictationEvent::State { state, mode } => {
                if state != "listening" && state != "finishing" {
                    self.partial.clear();
                }
                self.state = state;
                if let Some(m) = mode { self.mode = m; }
                None
            }
            DictationEvent::Partial(p) => { self.partial = p; None }
            DictationEvent::Commit { text, enter } => {
                self.partial.clear();
                (!text.is_empty() || enter).then_some((text, enter))
            }
            DictationEvent::Error(e) => { self.note = Some((e, std::time::Instant::now(), true)); None }
            DictationEvent::Info(i) => { self.note = Some((i, std::time::Instant::now(), false)); None }
            DictationEvent::Gone => {
                self.state.clear();
                self.partial.clear();
                None
            }
        }
    }

    pub fn listening(&self) -> bool { self.state == "listening" || self.state == "finishing" }

    /// The tray text: `MIC`, `MIC ON live` (or `final`) while listening, `MIC ...` while loading.
    pub fn tray_text(&self) -> String {
        match self.state.as_str() {
            "listening" | "finishing" => format!("MIC ON {}", self.mode_short()).trim_end().to_string(),
            "loading" => "MIC ...".to_string(),
            _ => "MIC".to_string(),
        }
    }

    fn mode_short(&self) -> &str {
        match self.mode.as_str() { "final" => "final", "" => "", _ => "live" }
    }

    /// The overlay line: the live text while listening, else a fresh note. `None`
    /// when there is nothing to show. The flag is true for an error.
    pub fn overlay(&self) -> Option<(String, bool)> {
        if let Some((t, at, err)) = &self.note {
            if at.elapsed().as_secs() < NOTE_SECS { return Some((t.clone(), *err)); }
        }
        if self.listening() {
            let shown = if self.partial.is_empty() { "listening...".to_string() } else { self.partial.clone() };
            return Some((shown, false));
        }
        None
    }
}

/// Whether dictation is on and can be offered: enabled, and a provider command is
/// set or tetron-stt is installed.
pub fn available(cfg: &crate::config::Config) -> bool {
    cfg.dictation_enabled && (!cfg.dictation_command.trim().is_empty() || crate::catalog::is_installed("tetron-stt"))
}

/// The program and arguments that start the provider: `<command> serve [options]`
/// from the `dictation_*` settings (the provider's own defaults for unset ones).
pub fn provider_command(cfg: &crate::config::Config) -> (String, Vec<String>) {
    let program = match cfg.dictation_command.trim() { "" => "tetron-stt", p => p }.to_string();
    let mut args = vec!["serve".to_string()];
    let mode = cfg.dictation_mode.trim();
    if mode == "segment" || mode == "final" { args.extend(["--mode".to_string(), mode.to_string()]); }
    if cfg.dictation_pause > 0.0 { args.extend(["--pause".to_string(), format!("{}", cfg.dictation_pause)]); }
    args.extend(["--enter-word".to_string(), cfg.dictation_enter_word.clone()]);
    if cfg.dictation_threads > 0 { args.extend(["--threads".to_string(), cfg.dictation_threads.to_string()]); }
    (program, args)
}

/// The tetron-stt TUI is the dictation engine unless a provider command is set.
pub fn uses_tui(cfg: &crate::config::Config) -> bool {
    cfg.dictation_command.trim().is_empty()
}

/// The TUI's arguments: the `dictation_*` settings, `--start` to listen at once.
pub fn tui_args(cfg: &crate::config::Config, start: bool) -> Vec<String> {
    let (_, mut args) = provider_command(cfg);
    args.remove(0); // "serve"
    if start {
        args.insert(0, "--start".to_string());
    }
    args
}

/// The running tetron-stt TUI, from the pid file it writes (same path rule as
/// tetron-stt: `$XDG_RUNTIME_DIR/tetron-stt.pid`, else the temp dir per uid).
pub fn tui_pid() -> Option<u32> {
    let f = match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(d) => std::path::PathBuf::from(d).join("tetron-stt.pid"),
        None => std::env::temp_dir().join(format!("tetron-stt-{}.pid", unsafe { libc::getuid() })),
    };
    let pid: u32 = std::fs::read_to_string(f).ok()?.trim().parse().ok()?;
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
    (comm.trim() == "tetron-stt").then_some(pid)
}

/// Ask the TUI to toggle listening (SIGUSR1) or switch mode (SIGUSR2).
pub fn signal_tui(pid: u32, mode: bool) -> bool {
    let sig = if mode { libc::SIGUSR2 } else { libc::SIGUSR1 };
    unsafe { libc::kill(pid as i32, sig) == 0 }
}

/// A running provider process, owned by the client.
pub struct Provider {
    child: Child,
    stdin: ChildStdin,
}

impl Provider {
    /// Start the provider. Everything it prints is parsed and passed to `forward`;
    /// when it exits, `Gone` follows (after an `Error` with its last stderr line
    /// when it died on its own).
    pub fn spawn(cfg: &crate::config::Config, forward: impl Fn(DictationEvent) + Send + 'static) -> Result<Provider, String> {
        let (program, args) = provider_command(cfg);
        let mut child = Command::new(&program)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    format!("dictation needs {program} (install it from the Store)")
                } else {
                    format!("cannot run {program}: {e}")
                }
            })?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let stderr = child.stderr.take().ok_or("no stderr")?;
        let last_err = Arc::new(Mutex::new(String::new()));
        let le = last_err.clone();
        let err_thread = std::thread::spawn(move || {
            for l in BufReader::new(stderr).lines().map_while(Result::ok) {
                if !l.trim().is_empty() { *le.lock().unwrap_or_else(|e| e.into_inner()) = l; }
            }
        });
        std::thread::spawn(move || {
            for l in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(ev) = parse_provider_line(&l) { forward(ev); }
            }
            let _ = err_thread.join();
            let e = last_err.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if !e.is_empty() { forward(DictationEvent::Error(e)); }
            forward(DictationEvent::Gone);
        });
        Ok(Provider { child, stdin })
    }

    /// Send a command line (`{"cmd":"toggle"}`). False when the provider is gone.
    pub fn command(&mut self, cmd: &str, mode: Option<&str>) -> bool {
        let mut v = serde_json::json!({ "cmd": cmd });
        if let Some(m) = mode { v["mode"] = serde_json::Value::from(m); }
        writeln!(self.stdin, "{v}").and_then(|()| self.stdin.flush()).is_ok()
    }

    /// Whether the process is still running.
    pub fn alive(&mut self) -> bool { matches!(self.child.try_wait(), Ok(None)) }
}

impl Drop for Provider {
    fn drop(&mut self) {
        let _ = writeln!(self.stdin, r#"{{"cmd":"quit"}}"#);
        let _ = self.stdin.flush();
        let t0 = std::time::Instant::now();
        while t0.elapsed() < std::time::Duration::from_millis(1500) {
            if matches!(self.child.try_wait(), Ok(Some(_))) { return; }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Turn a toggle or mode request into the provider command and send it, starting
/// the provider first when none runs. Errors come back as an event for the daemon.
pub fn dispatch(provider: &mut Option<Provider>, cfg: &crate::config::Config, what: &str, forward: &(impl Fn(DictationEvent) + Clone + Send + 'static)) {
    // Switched off (Settings): stop the provider, and say so when a key asked for it.
    if what == "off" || !cfg.dictation_enabled {
        *provider = None; // Drop quits the process and frees the microphone
        if what != "off" {
            forward(DictationEvent::Error("dictation is off (Settings, Dictation)".into()));
        }
        return;
    }
    if provider.as_mut().is_some_and(|p| !p.alive()) { *provider = None; }
    if provider.is_none() {
        match Provider::spawn(cfg, forward.clone()) {
            Ok(p) => *provider = Some(p),
            Err(e) => return forward(DictationEvent::Error(e)),
        }
    }
    let (cmd, mode) = match what { "mode" => ("mode", None), _ => ("toggle", None) };
    if let Some(p) = provider.as_mut() {
        if !p.command(cmd, mode) {
            *provider = None;
            forward(DictationEvent::Error("the dictation provider stopped; press again to restart it".into()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_lines_parse() {
        assert_eq!(parse_provider_line(r#"{"state":"listening","mode":"segment"}"#), Some(DictationEvent::State { state: "listening".into(), mode: Some("segment".into()) }));
        assert_eq!(parse_provider_line(r#"{"partial":"hello wor"}"#), Some(DictationEvent::Partial("hello wor".into())));
        assert_eq!(parse_provider_line(r#"{"commit":"hello world ","enter":true}"#), Some(DictationEvent::Commit { text: "hello world ".into(), enter: true }));
        assert_eq!(parse_provider_line(r#"{"commit":"x"}"#), Some(DictationEvent::Commit { text: "x".into(), enter: false }));
        assert_eq!(parse_provider_line(r#"{"error":"no mic"}"#), Some(DictationEvent::Error("no mic".into())));
        assert_eq!(parse_provider_line(r#"{"info":"models loaded"}"#), Some(DictationEvent::Info("models loaded".into())));
        assert_eq!(parse_provider_line(r#"{"future":1,"state":"idle"}"#), Some(DictationEvent::State { state: "idle".into(), mode: None }), "unknown fields are ignored");
        for bad in ["", "not json", "[]", r#"{"unknown":1}"#] {
            assert_eq!(parse_provider_line(bad), None, "{bad}");
        }
    }

    #[test]
    fn state_follows_events() {
        let mut s = DictationState::default();
        assert_eq!(s.tray_text(), "MIC");
        assert!(s.overlay().is_none());
        s.apply(DictationEvent::State { state: "loading".into(), mode: Some("segment".into()) });
        assert_eq!(s.tray_text(), "MIC ...");
        s.apply(DictationEvent::State { state: "listening".into(), mode: None });
        assert!(s.listening() && s.tray_text() == "MIC ON live");
        assert_eq!(s.overlay(), Some(("listening...".into(), false)));
        s.apply(DictationEvent::Partial("open the".into()));
        assert_eq!(s.overlay(), Some(("open the".into(), false)));
        assert_eq!(s.apply(DictationEvent::Commit { text: "open the file ".into(), enter: false }), Some(("open the file ".into(), false)));
        assert!(s.partial.is_empty(), "a commit clears the live text");
        assert_eq!(s.apply(DictationEvent::Commit { text: String::new(), enter: true }), Some((String::new(), true)), "Enter alone is a commit");
        assert_eq!(s.apply(DictationEvent::Commit { text: String::new(), enter: false }), None);
        s.apply(DictationEvent::Partial("half".into()));
        s.apply(DictationEvent::State { state: "idle".into(), mode: None });
        assert!(s.partial.is_empty() && s.overlay().is_none(), "idle drops the live text");
        s.apply(DictationEvent::Error("no microphone".into()));
        assert_eq!(s.overlay(), Some(("no microphone".into(), true)));
        s.apply(DictationEvent::Gone);
        assert_eq!(s.tray_text(), "MIC");
    }

    #[test]
    fn provider_command_from_settings() {
        let mut c = crate::config::Config::default();
        let (prog, args) = provider_command(&c);
        assert_eq!(prog, "tetron-stt");
        assert_eq!(args, ["serve", "--mode", "segment", "--pause", "1.2", "--enter-word", "make it so"]);
        c.dictation_command = "/opt/x/stt".into();
        c.dictation_mode = "final".into();
        c.dictation_threads = 3;
        c.dictation_enter_word.clear();
        let (prog, args) = provider_command(&c);
        assert_eq!(prog, "/opt/x/stt");
        assert_eq!(args, ["serve", "--mode", "final", "--pause", "1.2", "--enter-word", "", "--threads", "3"]);
    }

    fn fake(body: &str) -> (std::path::PathBuf, crate::config::Config) {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("twm-dict-{}-{}", std::process::id(), body.len()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("provider.sh");
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        (dir, crate::config::Config { dictation_command: p.to_string_lossy().into_owned(), ..Default::default() })
    }

    #[test]
    fn a_provider_is_started_commanded_and_stopped() {
        // Answers a toggle with state, partial and commit; `quit` or EOF ends it.
        let (dir, cfg) = fake(r#"while read l; do case "$l" in *toggle*) echo '{"state":"listening","mode":"segment"}'; echo '{"partial":"hel"}'; echo '{"commit":"hello ","enter":true}';; *quit*) exit 0;; esac; done"#);
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = Mutex::new(tx);
        let forward = move |e| { let _ = tx.lock().unwrap().send(e); };
        let mut p = Provider::spawn(&cfg, forward).unwrap();
        assert!(p.alive() && p.command("toggle", None));
        let next = || rx.recv_timeout(std::time::Duration::from_secs(3)).unwrap();
        assert_eq!(next(), DictationEvent::State { state: "listening".into(), mode: Some("segment".into()) });
        assert_eq!(next(), DictationEvent::Partial("hel".into()));
        assert_eq!(next(), DictationEvent::Commit { text: "hello ".into(), enter: true });
        drop(p);
        assert_eq!(next(), DictationEvent::Gone, "the provider ending is reported");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_provider_that_dies_reports_its_last_error_line_and_a_missing_one_says_so() {
        let (dir, cfg) = fake(r#"echo "no microphone found" >&2; exit 1"#);
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = Mutex::new(tx);
        let _p = Provider::spawn(&cfg, move |e| { let _ = tx.lock().unwrap().send(e); }).unwrap();
        assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(3)).unwrap(), DictationEvent::Error("no microphone found".into()));
        assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(3)).unwrap(), DictationEvent::Gone);
        let _ = std::fs::remove_dir_all(dir);
        let cfg = crate::config::Config { dictation_command: "/nonexistent/tetron-stt-x".into(), ..Default::default() };
        match Provider::spawn(&cfg, |_| {}) {
            Err(e) => assert!(e.contains("dictation needs"), "{e}"),
            Ok(_) => panic!("spawned a missing program"),
        }
    }
}
