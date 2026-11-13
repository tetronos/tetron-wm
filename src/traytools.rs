//! The "Open <tool>" row of the tray popovers: an ordered list of candidate
//! commands per item (CPU, Memory, Battery, Network); the first whose program is
//! installed wins. Defaults live here; `[tray_tools]` in the config overrides
//! them per item.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Prefix marking a candidate whose output is short-lived: it is run through
/// `sh -c` and held open until Enter.
pub const HOLD_PREFIX: &str = "hold:";

/// A `[tray_tools]` value: one command line or an ordered list of them.
/// An empty list (or empty string) removes the item's Open row.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ToolSpec {
    One(String),
    Many(Vec<String>),
}

impl ToolSpec {
    /// The non-empty command lines this value holds, in order.
    pub fn lines(&self) -> Vec<String> {
        match self {
            ToolSpec::One(s) => vec![s.clone()],
            ToolSpec::Many(v) => v.clone(),
        }
        .into_iter()
        .filter(|l| !l.trim().is_empty())
        .collect()
    }
}

/// `[tray_tools]`: item name (`cpu`, `mem`, `battery`, `bluetooth`, `network`,
/// `volume`) → candidates.
pub type TrayTools = BTreeMap<String, ToolSpec>;

/// A window the popover offers to open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolAction {
    pub title: String,
    /// Popover row text, e.g. "Open htop".
    pub label: String,
    pub command: String,
    pub args: Vec<String>,
}

/// Built-in candidates per item, in preference order.
pub fn defaults(item: &str) -> Vec<&'static str> {
    match item {
        "cpu" => vec!["htop", "top"],
        "mem" => vec!["hold:free -h", "hold:vm_stat"],
        "battery" => vec![
            "jolt",
            "hold:upower -i $(upower -e | grep -m1 -i bat)",
            "hold:pmset -g batt",
        ],
        "bluetooth" => vec!["bluetuith", "bluetoothctl", "hold:blueutil --paired"],
        // PipeWire native first, then the PulseAudio API ones (pipewire-pulse
        // serves them too), then ALSA's mixer, which ships almost everywhere
        // but sets the card's level, not the sink the menubar shows.
        "volume" => vec!["wiremix", "pulsemixer", "ncpamixer", "alsamixer"],
        "calendar" => vec!["calcurse"],
        "network" => vec![
            "wlctl",
            "nmtui",
            "iwctl",
            "hold:ip -br addr",
            "hold:networksetup -listallhardwareports",
            "hold:ifconfig",
        ],
        _ => Vec::new(),
    }
}

/// Window title for an item.
fn title(item: &str) -> &'static str {
    match item {
        "cpu" => "CPU",
        "mem" => "Memory",
        "battery" => "Battery",
        "bluetooth" => "Bluetooth",
        "network" => "Network",
        "volume" => "Volume",
        "calendar" => "Calendar",
        _ => "Tool",
    }
}

/// The first candidate whose program `installed` accepts, as a window action.
/// `cfg` entries replace the built-in list for that item.
pub fn pick(item: &str, cfg: &TrayTools, installed: &dyn Fn(&str) -> bool) -> Option<ToolAction> {
    let lines: Vec<String> = match cfg.get(item) {
        Some(spec) => spec.lines(),
        None => defaults(item).into_iter().map(String::from).collect(),
    };
    let found = lines.iter().find_map(|l| action(title(item), l, installed));
    // `top` ships with every Unix, so the default CPU row never disappears.
    if found.is_none() && item == "cpu" && !cfg.contains_key(item) {
        return action(title(item), "top", &|_| true);
    }
    found
}

fn action(title: &str, line: &str, installed: &dyn Fn(&str) -> bool) -> Option<ToolAction> {
    let (hold, body) = match line.trim().strip_prefix(HOLD_PREFIX) {
        Some(b) => (true, b.trim()),
        None => (false, line.trim()),
    };
    if !installed(body.split_whitespace().next()?) {
        return None;
    }
    let label = format!("Open {}", short(body));
    let simple = !body.contains(|c| "$|;&<>()`\"'\\*".contains(c));
    let (command, args) = if hold {
        let hold_tail = "; printf '\\nPress Enter to close. '; read _k";
        ("sh".to_string(), vec!["-c".to_string(), format!("{body}{hold_tail}")])
    } else if simple {
        let mut w = body.split_whitespace().map(String::from);
        (w.next()?, w.collect())
    } else {
        ("sh".to_string(), vec!["-c".to_string(), body.to_string()])
    };
    Some(ToolAction { title: title.into(), label, command, args })
}

/// Row label text: the command up to any substitution/pipe, capped to fit the
/// 24-column popover.
fn short(body: &str) -> String {
    let cut = body.find(['$', '|', ';']).unwrap_or(body.len());
    body[..cut].trim().chars().take(15).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn have<'a>(bins: &'a [&'a str]) -> impl Fn(&str) -> bool + 'a {
        move |b| bins.contains(&b)
    }

    #[test]
    fn first_installed_default_wins() {
        let none = TrayTools::new();
        assert_eq!(pick("cpu", &none, &have(&["htop", "top"])).unwrap().command, "htop");
        assert_eq!(pick("cpu", &none, &have(&["top"])).unwrap().command, "top");
        assert_eq!(pick("cpu", &none, &have(&[])).unwrap().command, "top", "top is the unconditional default");
        assert_eq!(pick("battery", &none, &have(&["jolt", "upower"])).unwrap().command, "jolt");
        let up = pick("battery", &none, &have(&["upower"])).unwrap();
        assert_eq!(up.command, "sh");
        assert!(up.args[1].starts_with("upower -i $(upower -e"), "{:?}", up.args);
        assert!(up.args[1].contains("read _k"), "held open");
        assert_eq!(up.label, "Open upower -i");
        let pm = pick("battery", &none, &have(&["pmset"])).unwrap();
        assert!(pm.args[1].starts_with("pmset -g batt"));
        assert_eq!(pick("network", &none, &have(&["nmtui", "ip"])).unwrap().command, "nmtui");
        assert!(pick("network", &none, &have(&["ip"])).unwrap().args[1].starts_with("ip -br addr"));
        assert!(pick("network", &none, &have(&["ifconfig"])).unwrap().args[1].starts_with("ifconfig"));
    }

    #[test]
    fn config_overrides_and_can_disable() {
        let mut cfg = TrayTools::new();
        cfg.insert("cpu".into(), ToolSpec::Many(vec!["btop".into(), "htop".into()]));
        assert_eq!(pick("cpu", &cfg, &have(&["htop", "top"])).unwrap().command, "htop");
        assert_eq!(pick("cpu", &cfg, &have(&["btop"])).unwrap().command, "btop");
        cfg.insert("cpu".into(), ToolSpec::One("btop --utf-force".into()));
        let a = pick("cpu", &cfg, &have(&["btop"])).unwrap();
        assert_eq!((a.command.as_str(), a.args), ("btop", vec!["--utf-force".to_string()]));
        cfg.insert("cpu".into(), ToolSpec::Many(Vec::new()));
        assert!(pick("cpu", &cfg, &have(&["htop"])).is_none(), "empty list disables the row");
        cfg.insert("mem".into(), ToolSpec::One("hold:vmstat 1 3".into()));
        let m = pick("mem", &cfg, &have(&["vmstat"])).unwrap();
        assert_eq!(m.label, "Open vmstat 1 3");
        assert!(m.args[1].contains("read _k"));
    }
}
