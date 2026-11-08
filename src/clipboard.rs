//! Clipboard history: a capped, de-duplicated ring of everything copied inside
//! tetron-wm (OSC 52 stores from apps, Ctrl+Shift+C, mouse selections, the
//! `clip` CLI, the GUI bridge). Pure state with load/save helpers; the session
//! decides when to record and when to write the file.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Where an entry came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClipSource {
    /// An app's OSC 52 store.
    App,
    /// Ctrl+Shift+C.
    Copy,
    /// A mouse text selection.
    Selection,
    /// `tetron-wm clip copy` or an xclip/xsel/wl-copy shim.
    Cli,
    /// The real GUI clipboard (X11/Wayland bridge).
    Gui,
}

impl ClipSource {
    /// Short tag shown in the list.
    pub fn tag(self) -> &'static str {
        match self {
            ClipSource::App => "app",
            ClipSource::Copy => "copy",
            ClipSource::Selection => "sel",
            ClipSource::Cli => "cli",
            ClipSource::Gui => "gui",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipEntry {
    pub text: String,
    /// Unix time in milliseconds when it was last copied.
    #[serde(default)]
    pub time_ms: u64,
    pub source: ClipSource,
    #[serde(default)]
    pub pinned: bool,
}

/// Size limits, from the `clipboard_*` config keys.
#[derive(Clone, Copy, Debug)]
pub struct ClipLimits {
    /// Maximum number of entries (pinned entries do not count against it).
    pub max_entries: usize,
    /// Entries larger than this many bytes are not recorded.
    pub max_item_bytes: usize,
    /// Total size cap; the oldest unpinned entries are dropped past it.
    pub max_total_bytes: usize,
}

impl Default for ClipLimits {
    fn default() -> Self {
        ClipLimits { max_entries: 100, max_item_bytes: 256 * 1024, max_total_bytes: 4 * 1024 * 1024 }
    }
}

/// Newest first; pinned entries are kept (and listed) ahead of the rest.
#[derive(Clone, Debug, Default)]
pub struct ClipHistory {
    entries: Vec<ClipEntry>,
    limits: ClipLimits,
    paused: bool,
    /// Bumped on every change so the session can debounce disk writes.
    revision: u64,
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl ClipHistory {
    pub fn new(limits: ClipLimits) -> Self {
        ClipHistory { entries: Vec::new(), limits, paused: false, revision: 0 }
    }

    pub fn set_limits(&mut self, limits: ClipLimits) {
        self.limits = limits;
        self.enforce();
    }

    pub fn paused(&self) -> bool { self.paused }
    pub fn set_paused(&mut self, p: bool) { self.paused = p; }
    pub fn revision(&self) -> u64 { self.revision }
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
    pub fn entries(&self) -> &[ClipEntry] { &self.entries }

    /// Record a copy. Returns whether it was recorded (not paused, not empty or
    /// whitespace-only, not over the item size cap). A duplicate moves to the
    /// front and keeps its pinned flag.
    pub fn record(&mut self, text: &str, source: ClipSource) -> bool {
        if self.paused || text.trim().is_empty() || text.len() > self.limits.max_item_bytes {
            return false;
        }
        let mut pinned = false;
        if let Some(i) = self.entries.iter().position(|e| e.text == text) {
            pinned = self.entries.remove(i).pinned;
        }
        self.entries.insert(0, ClipEntry { text: text.to_string(), time_ms: now_ms(), source, pinned });
        self.sort_pinned_first();
        self.enforce();
        self.revision += 1;
        true
    }

    /// Stable sort keeping newest-first order inside each group.
    fn sort_pinned_first(&mut self) {
        self.entries.sort_by_key(|e| !e.pinned);
    }

    /// Drop the oldest unpinned entries past the count and byte caps.
    fn enforce(&mut self) {
        let unpinned = |v: &[ClipEntry]| v.iter().filter(|e| !e.pinned).count();
        while unpinned(&self.entries) > self.limits.max_entries {
            self.drop_oldest_unpinned();
        }
        let total = |v: &[ClipEntry]| v.iter().map(|e| e.text.len()).sum::<usize>();
        while total(&self.entries) > self.limits.max_total_bytes && unpinned(&self.entries) > 0 {
            self.drop_oldest_unpinned();
        }
    }

    fn drop_oldest_unpinned(&mut self) {
        if let Some(i) = self.entries.iter().rposition(|e| !e.pinned) {
            self.entries.remove(i);
        }
    }

    pub fn get(&self, i: usize) -> Option<&ClipEntry> { self.entries.get(i) }

    /// Drop unpinned entries last copied more than `ttl_secs` before `now_ms`.
    /// Returns whether anything was removed.
    pub fn expire(&mut self, now_ms: u64, ttl_secs: u64) -> bool {
        if ttl_secs == 0 {
            return false;
        }
        let before = self.entries.len();
        self.entries.retain(|e| e.pinned || now_ms.saturating_sub(e.time_ms) <= ttl_secs * 1000);
        let removed = self.entries.len() != before;
        if removed {
            self.revision += 1;
        }
        removed
    }

    /// Newest entry regardless of pinning (what a plain paste would give).
    pub fn latest(&self) -> Option<&ClipEntry> {
        // First strict maximum: on equal timestamps the earlier (newer) index wins.
        self.entries.iter().fold(None, |best: Option<&ClipEntry>, e| match best {
            Some(b) if b.time_ms >= e.time_ms => Some(b),
            _ => Some(e),
        })
    }

    /// Indices matching a case-insensitive substring filter (all when empty).
    pub fn matching(&self, filter: &str) -> Vec<usize> {
        let f = filter.to_lowercase();
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| f.is_empty() || e.text.to_lowercase().contains(&f))
            .map(|(i, _)| i)
            .collect()
    }

    pub fn toggle_pin(&mut self, i: usize) {
        if let Some(e) = self.entries.get_mut(i) {
            e.pinned = !e.pinned;
            self.sort_pinned_first();
            self.enforce();
            self.revision += 1;
        }
    }

    pub fn delete(&mut self, i: usize) {
        if i < self.entries.len() {
            self.entries.remove(i);
            self.revision += 1;
        }
    }

    /// Remove everything, pinned included.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.revision += 1;
    }

    // ── Disk ────────────────────────────────────────────────────────────────

    /// Load a jsonl file; unreadable or malformed lines are skipped.
    pub fn load(&mut self, path: &Path) {
        let Ok(text) = std::fs::read_to_string(path) else { return };
        let mut v: Vec<ClipEntry> = text.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
        v.retain(|e| !e.text.trim().is_empty());
        self.entries = v;
        self.sort_pinned_first();
        self.enforce();
    }

    /// Write the history as jsonl, atomically, readable only by the owner.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
            }
        }
        let tmp = path.with_extension("jsonl.tmp");
        {
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            let mut f = opts.open(&tmp)?;
            for e in &self.entries {
                writeln!(f, "{}", serde_json::to_string(e).map_err(std::io::Error::other)?)?;
            }
            f.flush()?;
        }
        std::fs::rename(&tmp, path)
    }
}

/// Everything about the clipboard that more than one thread touches: the session
/// (render loop) and the daemon's control thread, which serves `tetron-wm clip`
/// and the xclip/xsel/wl-copy shims even while no client is attached.
#[derive(Default)]
pub struct ClipShared {
    pub history: ClipHistory,
    /// The explicit clipboard (what a plain paste inserts).
    pub clipboard: String,
    /// The mouse-selection buffer.
    pub primary: String,
    /// Text set through the CLI that the session still has to forward to the
    /// host terminal clipboard (OSC 52).
    pub host_out: Option<String>,
    /// `clipboard_history` switch: when false nothing is recorded.
    pub enabled: bool,
    /// What the GUI clipboard last held, as seen by the GUI bridge (so its own
    /// writes are not recorded again).
    pub gui_synced: String,
    /// Where the history is saved (set when persistence is enabled).
    pub path: Option<PathBuf>,
    saved_rev: u64,
    saved_at: Option<std::time::Instant>,
}

impl ClipShared {
    /// Write the history if it changed (at most about once a second unless
    /// `force`). No-op without a path.
    pub fn save_if_due(&mut self, force: bool) {
        let Some(path) = self.path.clone() else { return };
        if self.history.revision() == self.saved_rev {
            return;
        }
        if !force && self.saved_at.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(1)) {
            return;
        }
        match self.history.save(&path) {
            Ok(()) => crate::dbg_log(&format!("clipboard: saved {} entries", self.history.len())),
            Err(e) => crate::log_warn(&format!("clipboard: save failed: {e}")),
        }
        self.saved_rev = self.history.revision();
        self.saved_at = Some(std::time::Instant::now());
    }

    /// Start saving to `path` (keeping the in-memory history); the next save
    /// writes everything.
    pub fn persist_to(&mut self, path: PathBuf) {
        self.path = Some(path);
        self.saved_rev = u64::MAX;
    }

    /// Stop saving and delete the history file.
    pub fn stop_persisting(&mut self) {
        if let Some(p) = self.path.take() {
            let _ = std::fs::remove_file(p);
        }
    }

    /// Load the history from `path` and save there from now on.
    pub fn enable_persistence(&mut self, path: PathBuf) {
        self.history.load(&path);
        if let Some(e) = self.history.latest() {
            self.clipboard = e.text.clone();
        }
        self.saved_rev = self.history.revision();
        self.path = Some(path);
    }
}

pub type SharedClip = std::sync::Arc<std::sync::Mutex<ClipShared>>;

/// Fresh shared state with the given limits; `enabled` is `clipboard_history`.
pub fn new_shared(limits: ClipLimits, enabled: bool) -> SharedClip {
    std::sync::Arc::new(std::sync::Mutex::new(ClipShared { history: ClipHistory::new(limits), enabled, ..Default::default() }))
}

/// Lock, recovering from a poisoned mutex (one panic must not wedge the clipboard).
pub fn lock(c: &SharedClip) -> std::sync::MutexGuard<'_, ClipShared> {
    c.lock().unwrap_or_else(|e| e.into_inner())
}

/// A request over the daemon control socket (`tetron-wm clip ...`, the shims).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ClipCmd {
    /// Set the clipboard (or the primary selection) and record it.
    Copy { text: String, primary: bool },
    /// Read the clipboard/primary, or history entry `index` (0 = newest).
    Paste { primary: bool, index: Option<usize> },
    List,
    Clear,
    Pin { index: usize },
    Delete { index: usize },
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ClipReply {
    pub ok: bool,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub entries: Vec<ClipEntry>,
    #[serde(default)]
    pub error: String,
}

impl ClipReply {
    fn err(msg: &str) -> Self { ClipReply { ok: false, error: msg.into(), ..Default::default() } }
}

/// Apply a control-socket request to the shared state.
pub fn handle_cmd(shared: &SharedClip, cmd: ClipCmd) -> ClipReply {
    let mut s = lock(shared);
    let r = apply_cmd(&mut s, cmd);
    s.save_if_due(true);
    r
}

fn apply_cmd(s: &mut ClipShared, cmd: ClipCmd) -> ClipReply {
    match cmd {
        ClipCmd::Copy { text, primary } => {
            // Empty text is an explicit clear (xsel -c, wl-copy --clear): empty the
            // buffer, record nothing, do not touch the host clipboard.
            if primary {
                s.primary = text.clone();
            } else {
                s.clipboard = text.clone();
                if !text.is_empty() {
                    s.host_out = Some(text.clone());
                }
            }
            if s.enabled && !text.is_empty() {
                s.history.record(&text, ClipSource::Cli);
            }
            ClipReply { ok: true, ..Default::default() }
        }
        ClipCmd::Paste { primary, index } => match index {
            Some(i) => match s.history.get(i) {
                Some(e) => ClipReply { ok: true, text: e.text.clone(), ..Default::default() },
                None => ClipReply::err("no such history entry"),
            },
            None => {
                let text = if primary { s.primary.clone() } else { s.clipboard.clone() };
                ClipReply { ok: true, text, ..Default::default() }
            }
        },
        ClipCmd::List => ClipReply { ok: true, entries: s.history.entries().to_vec(), ..Default::default() },
        ClipCmd::Clear => {
            s.history.clear();
            ClipReply { ok: true, ..Default::default() }
        }
        ClipCmd::Pin { index } if index < s.history.len() => {
            s.history.toggle_pin(index);
            ClipReply { ok: true, ..Default::default() }
        }
        ClipCmd::Delete { index } if index < s.history.len() => {
            s.history.delete(index);
            ClipReply { ok: true, ..Default::default() }
        }
        ClipCmd::Pin { .. } | ClipCmd::Delete { .. } => ClipReply::err("no such history entry"),
    }
}

/// One-line preview: newlines shown as `⏎`, cut to `n` characters with `…`.
pub fn preview(text: &str, n: usize) -> String {
    let one: String = text.chars().map(|c| if c == '\n' { crate::glyphs::ch(crate::glyphs::Glyph::Newline) } else if c == '\t' { ' ' } else { c }).collect();
    if one.chars().count() > n {
        format!("{}\u{2026}", one.chars().take(n.saturating_sub(1)).collect::<String>())
    } else {
        one
    }
}

/// `~/.local/share/tetron-wm/clipboard-history.jsonl` (honours `XDG_DATA_HOME`).
pub fn history_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")))?;
    Some(base.join("tetron-wm").join("clipboard-history.jsonl"))
}

/// The OSC 52 reply to a clipboard query: `ESC ] 52 ; <kind> ; <base64> <term>`.
/// `kind` is `c` (clipboard) or `p` (primary); `bel` picks the terminator the
/// app used for its query (BEL, else ST).
pub fn osc52_reply(kind: char, text: &str, bel: bool) -> String {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    format!("\x1b]52;{kind};{b64}{}", if bel { "\x07" } else { "\x1b\\" })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h() -> ClipHistory { ClipHistory::new(ClipLimits::default()) }

    #[test]
    fn records_newest_first_and_dedupes() {
        let mut c = h();
        assert!(c.record("one", ClipSource::Copy));
        assert!(c.record("two", ClipSource::App));
        assert!(c.record("one", ClipSource::Selection));
        let t: Vec<&str> = c.entries().iter().map(|e| e.text.as_str()).collect();
        assert_eq!(t, ["one", "two"]);
        assert_eq!(c.get(0).unwrap().source, ClipSource::Selection);
    }

    #[test]
    fn ignores_blank_oversize_and_paused() {
        let mut c = ClipHistory::new(ClipLimits { max_item_bytes: 4, ..ClipLimits::default() });
        assert!(!c.record("", ClipSource::Copy));
        assert!(!c.record("  \n\t", ClipSource::Copy));
        assert!(!c.record("12345", ClipSource::Copy));
        assert!(c.record("1234", ClipSource::Copy));
        c.set_paused(true);
        assert!(!c.record("abc", ClipSource::Copy));
        assert_eq!(c.len(), 1);
    }

    #[test]
    fn count_cap_drops_oldest_but_keeps_pinned() {
        let mut c = ClipHistory::new(ClipLimits { max_entries: 2, ..ClipLimits::default() });
        c.record("keep", ClipSource::Copy);
        c.toggle_pin(0);
        for t in ["a", "b", "c"] {
            c.record(t, ClipSource::Copy);
        }
        let t: Vec<&str> = c.entries().iter().map(|e| e.text.as_str()).collect();
        assert_eq!(t, ["keep", "c", "b"], "pinned first, two newest unpinned");
    }

    #[test]
    fn byte_cap_drops_oldest_unpinned() {
        let mut c = ClipHistory::new(ClipLimits { max_total_bytes: 6, ..ClipLimits::default() });
        c.record("aaaa", ClipSource::Copy);
        c.record("bbbb", ClipSource::Copy);
        assert_eq!(c.len(), 1);
        assert_eq!(c.get(0).unwrap().text, "bbbb");
    }

    #[test]
    fn pin_survives_rerecord_and_sorts_first() {
        let mut c = h();
        c.record("x", ClipSource::Copy);
        c.record("y", ClipSource::Copy);
        c.toggle_pin(1); // x
        assert_eq!(c.get(0).unwrap().text, "x");
        c.record("x", ClipSource::App);
        assert!(c.get(0).unwrap().pinned);
        assert_eq!(c.latest().unwrap().text, "x");
    }

    #[test]
    fn expire_drops_old_unpinned_only() {
        let mut c = h();
        c.record("old", ClipSource::Copy);
        c.record("pinned-old", ClipSource::Copy);
        c.toggle_pin(0);
        c.record("fresh", ClipSource::Copy);
        let t0 = c.entries().iter().map(|e| e.time_ms).max().unwrap();
        assert!(!c.expire(t0 + 10_000, 0), "ttl 0 never expires");
        assert!(!c.expire(t0 + 10_000, 60));
        assert!(c.expire(t0 + 120_000, 60));
        let left: Vec<&str> = c.entries().iter().map(|e| e.text.as_str()).collect();
        assert_eq!(left, ["pinned-old"]);
    }

    #[test]
    fn matching_filters_case_insensitively() {
        let mut c = h();
        c.record("Hello World", ClipSource::Copy);
        c.record("other", ClipSource::Copy);
        assert_eq!(c.matching("WORLD").len(), 1);
        assert_eq!(c.matching("").len(), 2);
        assert!(c.matching("zzz").is_empty());
    }

    #[test]
    fn jsonl_round_trip_with_private_mode() {
        let dir = std::env::temp_dir().join(format!("twm-clip-{}", std::process::id()));
        let path = dir.join("sub/clipboard-history.jsonl");
        let mut c = h();
        c.record("line1\nline2", ClipSource::Selection);
        c.record("p", ClipSource::Cli);
        c.toggle_pin(1);
        c.save(&path).unwrap();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        let mut d = h();
        d.load(&path);
        assert_eq!(d.entries(), c.entries());
        std::fs::write(&path, "garbage\n{\"text\":\"ok\",\"source\":\"copy\"}\n").unwrap();
        d.load(&path);
        assert_eq!(d.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn shared() -> SharedClip {
        let s = ClipShared { enabled: true, ..Default::default() };
        std::sync::Arc::new(std::sync::Mutex::new(s))
    }

    #[test]
    fn cmd_copy_then_paste_and_list() {
        let c = shared();
        assert!(handle_cmd(&c, ClipCmd::Copy { text: "hi".into(), primary: false }).ok);
        assert_eq!(handle_cmd(&c, ClipCmd::Paste { primary: false, index: None }).text, "hi");
        assert_eq!(handle_cmd(&c, ClipCmd::Paste { primary: true, index: None }).text, "");
        assert_eq!(lock(&c).host_out.as_deref(), Some("hi"));
        let l = handle_cmd(&c, ClipCmd::List);
        assert_eq!(l.entries.len(), 1);
        assert_eq!(l.entries[0].source, ClipSource::Cli);
        assert_eq!(handle_cmd(&c, ClipCmd::Paste { primary: false, index: Some(0) }).text, "hi");
        assert!(!handle_cmd(&c, ClipCmd::Paste { primary: false, index: Some(5) }).ok);
    }

    #[test]
    fn cmd_primary_does_not_touch_clipboard_and_empty_is_rejected() {
        let c = shared();
        assert!(handle_cmd(&c, ClipCmd::Copy { text: "sel".into(), primary: true }).ok);
        assert_eq!(handle_cmd(&c, ClipCmd::Paste { primary: false, index: None }).text, "");
        assert_eq!(handle_cmd(&c, ClipCmd::Paste { primary: true, index: None }).text, "sel");
        // An empty copy is an explicit clear, not an error and not a history entry.
        assert!(handle_cmd(&c, ClipCmd::Copy { text: String::new(), primary: true }).ok);
        assert_eq!(handle_cmd(&c, ClipCmd::Paste { primary: true, index: None }).text, "");
        assert_eq!(lock(&c).history.len(), 1, "only the earlier real copy is recorded");
    }

    #[test]
    fn cmd_pin_delete_clear_and_disabled() {
        let c = shared();
        handle_cmd(&c, ClipCmd::Copy { text: "a".into(), primary: false });
        handle_cmd(&c, ClipCmd::Copy { text: "b".into(), primary: false });
        assert!(handle_cmd(&c, ClipCmd::Pin { index: 1 }).ok);
        assert!(lock(&c).history.get(0).unwrap().pinned);
        assert!(handle_cmd(&c, ClipCmd::Delete { index: 0 }).ok);
        assert!(!handle_cmd(&c, ClipCmd::Delete { index: 9 }).ok);
        assert!(handle_cmd(&c, ClipCmd::Clear).ok);
        assert!(lock(&c).history.is_empty());
        lock(&c).enabled = false;
        handle_cmd(&c, ClipCmd::Copy { text: "z".into(), primary: false });
        assert!(lock(&c).history.is_empty(), "disabled: clipboard set but not recorded");
        assert_eq!(handle_cmd(&c, ClipCmd::Paste { primary: false, index: None }).text, "z");
    }

    #[test]
    fn osc52_reply_format() {
        assert_eq!(osc52_reply('c', "hi", true), "\x1b]52;c;aGk=\x07");
        assert_eq!(osc52_reply('p', "hi", false), "\x1b]52;p;aGk=\x1b\\");
    }
}
