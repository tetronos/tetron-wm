//! Leader-key bindings. Defaults live in [`DEFAULTS`]; the `[keys]` table in
//! `config.toml` overrides them per action (`launcher = "l"` or
//! `launcher = ["l", "enter"]`), and `leader = "ctrl+space"` rebinds the
//! leader itself. The client builds one [`Keymap`] at startup (a change needs
//! `tetron-wm reload`).

use crossterm::event::{KeyCode, KeyModifiers};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// A `[keys]` value: one key or a list of keys.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum KeySpec {
    One(String),
    Many(Vec<String>),
}

impl KeySpec {
    fn keys(&self) -> Vec<&str> {
        match self {
            KeySpec::One(s) => vec![s.as_str()],
            KeySpec::Many(v) => v.iter().map(String::as_str).collect(),
        }
    }
}

/// `(action, default keys, description)`. Order matters only when a user
/// override collides with another action: the earlier action keeps the key.
pub const DEFAULTS: &[(&str, &[&str], &str)] = &[
    ("spotlight", &["space"], "spotlight search"),
    ("launcher", &["l", "enter"], "app menu (launcher)"),
    ("maximize", &["m", "M"], "maximize focused window"),
    ("minimize", &["n", "N"], "minimize focused window"),
    ("focus_next", &["tab"], "focus the next window (Tab again keeps cycling)"),
    ("focus_prev", &["backtab"], "focus the previous window (Shift+Tab)"),
    ("windows", &["f"], "window picker: numbered list of all windows"),
    ("move", &["w"], "move the focused window with the arrows (r resizes, Enter done, Esc undo)"),
    ("volume", &["="], "volume popover: Left/Right level, m mute, Down + Enter opens the mixer"),
    ("brightness", &["-"], "screen brightness popover: Left/Right level (laptops with a backlight)"),
    ("info", &["i"], "Info menu: session, version, tray details; arrows and Enter open Sessions, Logs, Settings, ..."),
    ("close", &["esc"], "close focused window (asks first for app windows)"),
    ("snap_left", &["[", "left"], "snap left half"),
    ("snap_right", &["]", "right"], "snap right half"),
    ("store", &["S"], "app store"),
    ("settings", &[","], "settings"),
    ("activity", &["A"], "activity monitor"),
    ("assistant", &["*"], "the AI Assistant: open, show or hide it (like the menubar button)"),
    ("clipboard", &["C"], "clipboard history"),
    ("select", &["x", "y"], "select mode: drag selects text even in mouse-capturing apps (one-shot)"),
    ("copy", &["c"], "copy the mouse selection to the clipboard (Ctrl+Shift+C)"),
    ("paste", &["v", "p"], "paste the clipboard into the focused app (Ctrl+Shift+V)"),
    ("help", &["?", "h"], "shortcut help"),
    ("rename", &["r", "R"], "rename focused window"),
    ("tile", &["t"], "tile all windows into the grid"),
    ("context_menu", &["."], "right-click menu for the selected file (Files or desktop), arrows and Enter pick"),
    ("auto_tile", &["T"], "toggle auto-tile"),
    ("screensaver", &["z"], "start the screensaver (any key or mouse wakes it)"),
    ("screenshot", &["s"], "save a screenshot (PNG) of the whole screen"),
    ("detach", &["q"], "detach (apps keep running)"),
    ("shutdown", &["Q"], "quit tetron-wm (closes every app)"),
];

/// Parse one key name: a single character, or `space`, `enter`, `tab`,
/// `backtab` (Shift+Tab), `esc`,
/// `left`, `right`, `up`, `down`, `backspace`.
pub fn parse_key(s: &str) -> Option<KeyCode> {
    let mut chars = s.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return Some(KeyCode::Char(c));
    }
    Some(match s.to_ascii_lowercase().as_str() {
        "space" => KeyCode::Char(' '),
        "enter" | "return" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "backtab" | "shift+tab" => KeyCode::BackTab,
        "esc" | "escape" => KeyCode::Esc,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "backspace" => KeyCode::Backspace,
        _ => return None,
    })
}

/// Parse a leader spec like `ctrl+space` / `ctrl+b`. Only Ctrl chords are
/// accepted (a bare key would steal normal typing).
pub fn parse_leader(s: &str) -> Option<(KeyModifiers, KeyCode)> {
    let rest = s.to_ascii_lowercase();
    let key = rest.strip_prefix("ctrl+")?;
    Some((KeyModifiers::CONTROL, parse_key(key)?))
}

pub struct Keymap {
    map: HashMap<KeyCode, &'static str>,
    leader: (KeyModifiers, KeyCode),
}

impl Keymap {
    /// Build from the config: `keys` overrides per action, `leader` rebinds the
    /// leader. Unparsable entries are skipped (defaults stay) and described in
    /// the returned warnings.
    pub fn new(keys: &BTreeMap<String, KeySpec>, leader: Option<&str>) -> (Keymap, Vec<String>) {
        let mut warns = Vec::new();
        let mut map = HashMap::new();
        for (action, defaults, _) in DEFAULTS {
            let mut codes: Vec<KeyCode> = defaults.iter().filter_map(|k| parse_key(k)).collect();
            if let Some(spec) = keys.get(*action) {
                let parsed: Vec<KeyCode> = spec.keys().iter().filter_map(|k| parse_key(k)).collect();
                if parsed.is_empty() {
                    warns.push(format!("keys.{action}: no valid key in {spec:?}, using default"));
                } else {
                    codes = parsed;
                }
            }
            for c in codes {
                map.entry(c).or_insert(*action);
            }
        }
        for name in keys.keys() {
            if !DEFAULTS.iter().any(|d| d.0 == name) {
                warns.push(format!("keys.{name}: unknown action"));
            }
        }
        let mut lead = (KeyModifiers::CONTROL, KeyCode::Char(' '));
        if let Some(s) = leader {
            match parse_leader(s) {
                Some(l) => lead = l,
                None => warns.push(format!("leader: '{s}' is not a ctrl+<key> chord, using ctrl+space")),
            }
        }
        (Keymap { map, leader: lead }, warns)
    }

    /// The action bound to `code` after the leader, if any.
    pub fn action(&self, code: KeyCode) -> Option<&'static str> {
        self.map.get(&code).copied()
    }

    /// Whether this key event is the leader chord. `KeyCode::Null` is how some
    /// terminals report Ctrl+Space, so it also counts when the leader is that.
    pub fn is_leader(&self, code: KeyCode, mods: KeyModifiers) -> bool {
        (mods.contains(self.leader.0) && code == self.leader.1)
            || (self.leader == (KeyModifiers::CONTROL, KeyCode::Char(' ')) && code == KeyCode::Null)
    }
}

impl Default for Keymap {
    fn default() -> Keymap {
        Keymap::new(&BTreeMap::new(), None).0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_have_no_duplicate_keys() {
        let mut seen = std::collections::HashSet::new();
        for (action, keys, _) in DEFAULTS {
            for k in *keys {
                assert!(seen.insert(parse_key(k).unwrap()), "{action}: duplicate default key {k}");
            }
        }
    }

    #[test]
    fn default_launcher_keys() {
        let km = Keymap::default();
        for c in [KeyCode::Char('l'), KeyCode::Enter] {
            assert_eq!(km.action(c), Some("launcher"));
        }
        assert_ne!(km.action(KeyCode::Esc), Some("launcher"), "Esc must never open things");
        assert_eq!(km.action(KeyCode::Esc), Some("close"));
    }

    #[test]
    fn mnemonic_defaults() {
        let km = Keymap::default();
        for (c, a) in [('v', "paste"), ('p', "paste"), ('C', "clipboard"), ('s', "screenshot"), ('S', "store"), ('x', "select"), ('y', "select"), ('c', "copy")] {
            assert_eq!(km.action(KeyCode::Char(c)), Some(a), "{c}");
        }
        for c in ['a', 'P'] {
            assert_eq!(km.action(KeyCode::Char(c)), None, "{c} is freed");
        }
    }

    #[test]
    fn override_replaces_defaults() {
        let mut keys = BTreeMap::new();
        keys.insert("launcher".into(), KeySpec::One("x".into()));
        let (km, warns) = Keymap::new(&keys, None);
        assert!(warns.is_empty());
        assert_eq!(km.action(KeyCode::Char('x')), Some("launcher"));
        assert_eq!(km.action(KeyCode::Char('a')), None);
    }

    #[test]
    fn bad_entries_warn_and_keep_defaults() {
        let mut keys = BTreeMap::new();
        keys.insert("launcher".into(), KeySpec::One("nonsense".into()));
        keys.insert("bogus".into(), KeySpec::One("x".into()));
        let (km, warns) = Keymap::new(&keys, Some("b"));
        assert_eq!(warns.len(), 3);
        assert_eq!(km.action(KeyCode::Char('l')), Some("launcher"));
        assert!(km.is_leader(KeyCode::Char(' '), KeyModifiers::CONTROL));
    }

    #[test]
    fn leader_is_configurable() {
        let (km, _) = Keymap::new(&BTreeMap::new(), Some("ctrl+b"));
        assert!(km.is_leader(KeyCode::Char('b'), KeyModifiers::CONTROL));
        assert!(!km.is_leader(KeyCode::Char(' '), KeyModifiers::CONTROL));
    }
}
