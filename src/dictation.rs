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
}
