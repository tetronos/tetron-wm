//! The in-app help window: a keyboard-shortcut cheatsheet, opened with
//! `leader ?`. It is a real window (move, resize, overlap, stays open as a
//! reference); content is a pure data model so it is unit-testable.

use crate::buffer::CellBuffer;
use crate::cell::Cell;
use std::cell::Cell as StdCell;

/// A titled group of `(keys, description)` shortcut rows.
pub struct HelpSection {
    pub title: &'static str,
    pub rows: &'static [(&'static str, &'static str)],
    /// Free-text tips shown under the rows (one line each).
    pub notes: &'static [&'static str],
}

/// All shortcut groups shown in the overlay.
pub fn help_sections() -> &'static [HelpSection] {
    &[
        HelpSection {
            title: "Windows",
            rows: &[
                ("Tab/S-Tab", "next / previous window (Tab repeats)"),
                ("f", "window picker (1-9 or Enter)"),
                ("w", "move window: arrows, r resize"),
                ("=", "volume: Left/Right, m mute"),
                ("-", "brightness: Left/Right (laptops)"),
                ("m / n", "maximize / minimize"),
                ("Esc", "close window (asks first for apps)"),
                ("[ / ]", "snap left / right half"),
            ],
            notes: &[],
        },
        HelpSection {
            title: "Tiling",
            rows: &[
                ("t", "tile all into the grid"),
                ("T", "toggle auto-tile mode"),
                (".", "right-click menu (Files, desktop)"),
                ("i", "Info menu (version, Logs, Settings)"),
                ("s", "save a screenshot (PNG)"),
                ("1–9", "send window to cell N"),
            ],
            notes: &[],
        },
        HelpSection {
            title: "Apps",
            rows: &[
                ("Space", "spotlight search"),
                ("l / Enter", "app menu (launcher)"),
                ("S", "app store"),
                (",", "settings"),
                ("A", "activity monitor"),
                ("*", "the AI Assistant (open, show or hide)"),
            ],
            notes: &["In Settings: Tab switches between sections and rows."],
        },
        HelpSection {
            title: "Clipboard",
            rows: &[
                ("C", "clipboard history (also the menubar clipboard segment)"),
                ("x / y", "select mode: the next drag selects text"),
                ("c / v", "copy selection / paste clipboard (p also pastes)"),
            ],
            notes: &[
                "Copying out of w3m, tetron-browser or vim (they grab the mouse):",
                "  Ctrl+Space x, drag over the text, then Ctrl+Space C to pick",
                "  it, or Ctrl+Space v to paste it into another window.",
            ],
        },
        HelpSection {
            title: "Session",
            rows: &[
                ("z", "screensaver (any key or mouse wakes it)"),
                ("q / Q", "detach / quit tetron-wm"),
                ("?", "this help (a window; Esc or q closes it)"),
            ],
            notes: &[],
        },
    ]
}

#[allow(clippy::vec_init_then_push)]
fn body_lines() -> Vec<(String, bool)> {
    let sections = help_sections();
    let mut lines: Vec<(String, bool)> = Vec::new(); // (text, is_heading)
    lines.push(("Leader = Ctrl+Space  (press, release, then a key)".into(), false));
    lines.push(("Remap in config.toml: leader = \"ctrl+b\" and a [keys] table".into(), false));
    lines.push(("Empty desktop: Enter or Space opens the launcher".into(), false));
    lines.push((String::new(), false));
    for s in sections {
        lines.push((s.title.to_uppercase(), true));
        for (keys, desc) in s.rows {
            lines.push((format!("  {keys:<8} {desc}"), false));
        }
        for note in s.notes {
            lines.push((format!("  {note}"), false));
        }
    }
    lines.push((String::new(), false));
    lines.push(("In overlays:  ↑↓ move · →← expand · Enter open · Esc cancel".into(), false));
    lines.push((format!("Menubar:  tetron-wm = launcher · {}/{} = view · {} = assistant · host \u{25BE} = power/Systems",
        crate::glyphs::get(crate::glyphs::Glyph::ModeDesktop), crate::glyphs::get(crate::glyphs::Glyph::ModeSimple),
        crate::glyphs::get(crate::glyphs::Glyph::Assistant)), false));
    lines.push((String::new(), false));
    lines.push(("↑↓ PgUp PgDn Home End scroll · Esc or q closes this window".into(), false));

    lines
}

/// Preferred window size `(w, h)` for a `screen_w × screen_h` screen: wide enough
/// for the longest line and tall enough for everything, clamped to the screen.
pub fn preferred_size(screen_w: i32, screen_h: i32) -> (i32, i32) {
    let lines = body_lines();
    let inner_w = lines.iter().map(|(l, _)| l.chars().count()).max().unwrap_or(40) as i32;
    let w = (inner_w + 4).clamp(30, (screen_w - 2).max(30));
    // Titlebar + borders around the body.
    let h = (lines.len() as i32 + 2).clamp(6, (screen_h - 3).max(6));
    (w, h)
}

/// The scrolling body of the help window (pure state; the window chrome is
/// drawn by the window manager).
#[derive(Default)]
pub struct HelpView {
    scroll: StdCell<usize>,
    /// Body rows shown by the last render (bounds the scroll, sizes PgUp/PgDn).
    rows: StdCell<usize>,
}

impl HelpView {
    pub fn new() -> Self {
        HelpView { scroll: StdCell::new(0), rows: StdCell::new(20) }
    }

    fn max_scroll(&self) -> usize {
        body_lines().len().saturating_sub(self.rows.get().max(1))
    }

    /// Scroll by `delta` rows (saturating; `i32::MIN` / `i32::MAX` jump to the
    /// ends), clamped to the content.
    pub fn scroll_by(&self, delta: i32) {
        let max = self.max_scroll() as i64;
        let now = self.scroll.get() as i64;
        self.scroll.set((now.saturating_add(delta as i64)).clamp(0, max) as usize);
    }

    /// Rows moved by PgUp / PgDn.
    pub fn page(&self) -> i32 {
        (self.rows.get().saturating_sub(1)).max(1) as i32
    }

    pub fn scroll(&self) -> usize {
        self.scroll.get()
    }

    /// Render the body into a `w × h` buffer (the window's content area).
    pub fn render(&self, w: i32, h: i32) -> CellBuffer {
        let t = crate::theme::current();
        let mut buf = CellBuffer::new(w, h);
        buf.fill(Cell { ch: ' ', fg: t.text, bg: t.window_bg, attrs: Default::default() });
        self.rows.set(h.max(1) as usize);
        let max = self.max_scroll();
        if self.scroll.get() > max {
            self.scroll.set(max);
        }
        let lines = body_lines();
        for (i, (text, heading)) in lines.iter().skip(self.scroll.get()).enumerate() {
            let y = i as i32;
            if y >= h {
                break;
            }
            let fg = if *heading { t.accent } else { t.text };
            let clipped: String = text.chars().take((w - 2).max(0) as usize).collect();
            buf.write_str(1, y, &clipped, fg, t.window_bg);
        }
        if self.scroll.get() < max && h > 0 && w > 12 {
            buf.write_str(w - 11, h - 1, " ↓ more ↓ ", t.accent, t.window_bg);
        }
        buf
    }
}
