//! The app launcher: a menubar dropdown (mouse) and a Spotlight overlay
//! (keyboard), sharing one app list and selection state.
//!
//! [`Launcher`] owns its open/closed state, the typed query (Spotlight), and the
//! highlighted index. It renders itself to compositor [`Layer`]s and reports
//! clickable hit regions, so the session core just forwards events and launches
//! whatever entry the launcher resolves.

use crate::buffer::CellBuffer;
use crate::cell::{Cell, Rgba};
use crate::compositor::Layer;
use crate::config::AppEntry;
use crate::geometry::{Point, Rect};

// Menu colors follow the active desktop theme. These were hardcoded midnight,
// which left the launcher dark under every theme (not just light). A dropdown is
// a floating surface, so it uses `window_bg`/`text`; the highlighted row uses
// `active_bg`/`accent`, the border `border`, and dim text `dim`.
fn menu_bg() -> Rgba { crate::theme::current().window_bg }
fn menu_fg() -> Rgba { crate::theme::current().text }
fn sel_bg() -> Rgba { crate::theme::current().active_bg }
fn sel_fg() -> Rgba { crate::theme::current().accent }
fn border() -> Rgba { crate::theme::current().border }
fn hint() -> Rgba { crate::theme::current().dim }
fn accent() -> Rgba { crate::theme::current().accent }

/// Internal category of the Suspend / Reboot / Power off entries: they are
/// gathered into the bottom "Power" submenu instead of an app category.
pub const POWER_CATEGORY: &str = "@power";

/// Internal category of the "Files on <system>" entries: not an app category, they
/// form the Remote access > Remote files submenu.
pub const REMOTE_FILES_CATEGORY: &str = "@remote-files";

/// A node in the cascading menu.
#[derive(Clone, Debug)]
enum MenuEntry {
    Launch(AppEntry),
    Submenu { label: String, items: Vec<MenuEntry> },
    /// A non-selectable divider line (nav and clicks skip it).
    Separator,
}

/// The widest a menu panel grows (names longer than this are still cut with an ellipsis).
const MENU_PANEL_MAX_W: i32 = 48;

impl MenuEntry {
    fn label(&self) -> &str {
        match self {
            MenuEntry::Launch(a) => &a.name,
            MenuEntry::Submenu { label, .. } => label,
            MenuEntry::Separator => "",
        }
    }
    /// Cells a row needs besides its name: the `CLI` tag (and its gap) for a CLI app, the
    /// right-edge marker of an entry that opens more.
    fn extra_w(&self) -> usize {
        let cli = matches!(self, MenuEntry::Launch(a) if a.cli.unwrap_or(false));
        (if cli { 4 } else { 0 }) + (if self.has_marker() { 2 } else { 0 })
    }
    fn is_submenu(&self) -> bool {
        matches!(self, MenuEntry::Submenu { .. })
    }
    fn is_separator(&self) -> bool {
        matches!(self, MenuEntry::Separator)
    }
    /// Entries drawn with a `▸` "opens more" marker: real submenus, and the
    /// Systems action (which opens the systems popover).
    fn has_marker(&self) -> bool {
        self.is_submenu() || matches!(self, MenuEntry::Launch(a) if a.command == "@systems")
    }
}

/// A bottom-of-menu system action entry (`@`-sentinel command routed by the
/// session's `launch_entry`, like `@files`/`@store`).
fn system_entry(name: &str, command: &str) -> AppEntry {
    AppEntry {
        name: name.into(),
        command: command.into(),
        args: Vec::new(),
        category: None,
        requires_cwd: None,
        cwd: None,
        cli: None,
        warn: None,
    }
}

/// How the launcher is currently presented.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LauncherMode {
    /// Apple-menu-style dropdown anchored under the menubar brand (mouse-first).
    Menu,
    /// Centered, filterable search overlay (keyboard-first).
    Spotlight,
}

/// The launcher widget: state + rendering + hit-testing.
pub struct Launcher {
    items: Vec<AppEntry>,
    open: Option<LauncherMode>,
    query: String,
    /// Highlighted row index into the currently *filtered* list.
    selected: usize,
    /// Cascade root (Menu mode), rebuilt on open.
    menu_root: Vec<MenuEntry>,
    /// Open chain: selected row at each open level. Non-empty while Menu is open.
    path: Vec<usize>,
    /// Last rendered screen size, so `hover`/`point_in_menu` can recompute geometry.
    last_w: std::cell::Cell<i32>,
    last_h: std::cell::Cell<i32>,
    /// Per open level: the path prefix that led to it and its first visible row, so a long
    /// list keeps its scroll position when the highlight moves inside the view (hovering a
    /// row must not slide the list under the pointer).
    scroll: std::cell::RefCell<Vec<(Vec<usize>, usize)>>,
}

/// A rendered launcher frame: layers plus the clickable regions for this frame.
pub struct Rendered {
    /// Compositor layers (drawn above all chrome).
    pub layers: Vec<Layer>,
    /// `(entry, screen_rect)` for each visible, clickable app row.
    pub items: Vec<(AppEntry, Rect)>,
}

impl Launcher {
    /// Create a launcher offering `items`, initially closed.
    pub fn new(items: Vec<AppEntry>) -> Self {
        Self {
            items,
            open: None,
            query: String::new(),
            selected: 0,
            menu_root: Vec::new(),
            path: vec![0],
            last_w: std::cell::Cell::new(80),
            last_h: std::cell::Cell::new(24),
            scroll: std::cell::RefCell::new(Vec::new()),
        }
    }

    /// Whether the launcher is currently visible.
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Replace the offered app list (e.g. after a new app is installed), keeping
    /// the open/closed state.
    pub fn set_items(&mut self, items: Vec<AppEntry>) {
        self.items = items;
    }

    /// The current presentation mode, if open.
    pub fn mode(&self) -> Option<LauncherMode> {
        self.open
    }

    /// Open (or, if already in `Menu`, close) the dropdown menu.
    pub fn toggle_menu(&mut self) {
        let opening = self.open != Some(LauncherMode::Menu);
        self.open = if opening { Some(LauncherMode::Menu) } else { None };
        self.reset_selection();
        if opening {
            self.rebuild_menu();
            self.path = vec![0];
        }
    }

    /// Build the cascade root: one Submenu per category (sorted, "tetron-wm" first),
    /// apps inside (sorted by name).
    fn rebuild_menu(&mut self) {
        use std::collections::BTreeMap;
        let mut by_cat: BTreeMap<String, Vec<AppEntry>> = BTreeMap::new();
        // Entries in the internal power category are not an app category: they
        // form the bottom "Power" submenu, in the order they were supplied.
        let mut power: Vec<AppEntry> = Vec::new();
        let mut remote_files: Vec<AppEntry> = Vec::new();
        for a in &self.items {
            if a.category.as_deref() == Some(POWER_CATEGORY) {
                power.push(a.clone());
            } else if a.category.as_deref() == Some(REMOTE_FILES_CATEGORY) {
                remote_files.push(a.clone());
            } else {
                by_cat.entry(cat_of(a)).or_default().push(a.clone());
            }
        }
        // "All": every app in one flat list, sorted by name.
        let mut all: Vec<AppEntry> = by_cat.values().flatten().chain(remote_files.iter()).cloned().collect();
        all.sort_by_key(|a| a.name.to_lowercase());
        let rank = |c: &str| if c == "tetron-wm" { 0 } else { 1 };
        let mut cats: Vec<(String, Vec<AppEntry>)> = by_cat.into_iter().collect();
        cats.sort_by(|(a, _), (b, _)| rank(a).cmp(&rank(b)).then_with(|| a.cmp(b)));
        // A bare "Shell" quick-launch is the very first top-level entry (one click,
        // no submenu) — opens the user's $SHELL in a new window.
        let mut root: Vec<MenuEntry> = vec![MenuEntry::Launch(shell_entry())];
        root.extend(cats.into_iter().map(|(label, mut apps)| {
            // "Help" is pinned to the bottom of the tetron-wm section.
            apps.sort_by_key(|x| (x.command == "@help", x.name.to_lowercase()));
            MenuEntry::Submenu {
                label,
                items: apps.into_iter().map(MenuEntry::Launch).collect(),
            }
        }));
        // "All" sits after tetron-wm (which sorts first), before the other categories.
        let all_at = if matches!(root.get(1), Some(MenuEntry::Submenu { label, .. }) if label == "tetron-wm") { 2 } else { 1 };
        root.insert(all_at, MenuEntry::Submenu { label: "All".into(), items: all.into_iter().map(MenuEntry::Launch).collect() });
        // Bottom system section: a divider, then Remote access (sshd, remote files,
        // the systems popover) above Detach / Reload / Quit (each opens a confirm dialog),
        // then a Power submenu (Suspend / Reboot / Power off, each behind its own
        // confirm) when the computer can do any of them. Routed by the session's
        // `launch_entry` via their `@`-sentinel commands; the ids (`@exit`,
        // `@restart`, `@shutdown`) predate the Detach / Reload / Quit labels.
        root.push(MenuEntry::Separator);
        let mut remote: Vec<MenuEntry> = Vec::new();
        // Always listed; where tetronos-ssh-temp is not installed the row says so.
        let state = if !crate::settings::remote_available() { "not installed" } else if crate::settings::remote_on() { "on" } else { "off" };
        remote.push(MenuEntry::Launch(system_entry(&format!("sshd allow remote access ({state})"), "@sshd")));
        if !remote_files.is_empty() {
            remote_files.sort_by_key(|a| a.name.to_lowercase());
            remote.push(MenuEntry::Submenu { label: "Remote files".into(), items: remote_files.into_iter().map(MenuEntry::Launch).collect() });
        }
        remote.push(MenuEntry::Launch(system_entry("Configure Remote Systems", "@systems")));
        root.push(MenuEntry::Submenu { label: "Remote access".into(), items: remote });
        root.push(MenuEntry::Separator);
        root.push(MenuEntry::Launch(system_entry("Detach", "@exit")));
        root.push(MenuEntry::Launch(system_entry("Reload", "@restart")));
        root.push(MenuEntry::Launch(system_entry("Quit", "@shutdown")));
        if !power.is_empty() {
            root.push(MenuEntry::Separator);
            root.push(MenuEntry::Submenu { label: "Power".into(), items: power.into_iter().map(MenuEntry::Launch).collect() });
        }
        self.menu_root = root;
    }

    /// Open (or, if already in `Spotlight`, close) the search overlay.
    pub fn toggle_spotlight(&mut self) {
        self.open = if self.open == Some(LauncherMode::Spotlight) { None } else { Some(LauncherMode::Spotlight) };
        self.reset_selection();
    }

    /// Close the launcher.
    pub fn close(&mut self) {
        self.open = None;
        self.reset_selection();
    }

    fn reset_selection(&mut self) {
        self.query.clear();
        self.selected = 0;
    }

    /// Append a character to the Spotlight query.
    pub fn type_char(&mut self, c: char) {
        if self.open == Some(LauncherMode::Spotlight) {
            self.query.push(c);
            self.selected = 0;
        }
    }

    /// Delete the last character of the Spotlight query.
    pub fn backspace(&mut self) {
        if self.open == Some(LauncherMode::Spotlight) {
            self.query.pop();
            self.selected = 0;
        }
    }

    /// Move the highlight up (saturating). Mode-aware: Menu walks `path`,
    /// Spotlight walks `selected`.
    pub fn move_up(&mut self) {
        if self.open == Some(LauncherMode::Menu) {
            if let Some(next) = self.next_selectable(-1) {
                if let Some(last) = self.path.last_mut() {
                    *last = next;
                }
            }
        } else {
            self.selected = self.selected.saturating_sub(1);
        }
    }

    /// Move the highlight down (clamped). Mode-aware: Menu walks `path`,
    /// Spotlight walks the filtered list.
    pub fn move_down(&mut self) {
        if self.open == Some(LauncherMode::Menu) {
            if let Some(next) = self.next_selectable(1) {
                if let Some(last) = self.path.last_mut() {
                    *last = next;
                }
            }
        } else {
            let n = self.filtered().len();
            if n > 0 && self.selected + 1 < n {
                self.selected += 1;
            }
        }
    }

    /// The current (deepest open) menu level's entries — the slice `*path.last()`
    /// indexes into.
    fn current_entries(&self) -> &[MenuEntry] {
        let mut entries: &[MenuEntry] = &self.menu_root;
        let depth = self.path.len().saturating_sub(1);
        for &sel in self.path.iter().take(depth) {
            match entries.get(sel.min(entries.len().saturating_sub(1))) {
                Some(MenuEntry::Submenu { items, .. }) => entries = items,
                _ => break,
            }
        }
        entries
    }

    /// Next selectable row index stepping by `dir` from the current selection,
    /// skipping separators. `None` when there is none in that direction (keep put).
    fn next_selectable(&self, dir: i32) -> Option<usize> {
        let entries = self.current_entries();
        let n = entries.len() as i32;
        if n == 0 {
            return None;
        }
        let cur = (*self.path.last().unwrap_or(&0)).min(entries.len() - 1) as i32;
        let mut i = cur + dir;
        while (0..n).contains(&i) {
            if !entries[i as usize].is_separator() {
                return Some(i as usize);
            }
            i += dir;
        }
        None
    }

    /// Mouse wheel over a menu panel: move that panel's selection by `lines` rows
    /// (positive = wheel up = toward the first row), skipping separators; the panel
    /// scrolls to keep the selection in view. Deeper panels close. Returns whether
    /// the wheel was over a panel (else the caller scrolls something else).
    pub fn wheel(&mut self, p: Point, lines: i32) -> bool {
        if self.open != Some(LauncherMode::Menu) || lines == 0 {
            return false;
        }
        let geom = self.panel_geometry(self.last_w.get(), self.last_h.get());
        let Some(k) = geom.iter().position(|(_, panel, _)| panel.contains(p)) else { return false };
        let levels = self.levels();
        let Some((entries, sel)) = levels.get(k).map(|(e, s)| (*e, *s)) else { return false };
        let n = entries.len();
        // Scroll the view (wheel up = toward the first row); the highlight only moves when it
        // would leave the view, so the row under the pointer stays pickable.
        let (off, vis) = self.window(k, n, sel, (self.last_h.get() - 3).max(1) as usize);
        let step = lines.unsigned_abs().min(20) as usize;
        let new_off = if lines > 0 { off.saturating_sub(step) } else { (off + step).min(n.saturating_sub(vis)) };
        let mut new_sel = sel;
        if new_sel < new_off {
            new_sel = new_off;
        } else if new_sel >= new_off + vis {
            new_sel = new_off + vis - 1;
        }
        let mut key: Vec<usize> = self.path.iter().take(k).copied().collect();
        {
            let mut st = self.scroll.borrow_mut();
            st[k] = (std::mem::take(&mut key), new_off);
        }
        self.path.truncate(k + 1);
        while self.path.len() < k + 1 {
            self.path.push(0);
        }
        self.path[k] = new_sel;
        true
    }

    /// The entry under screen point `p` (any open panel), or `None`.
    fn row_at(&self, p: Point) -> Option<&MenuEntry> {
        let geom = self.panel_geometry(self.last_w.get(), self.last_h.get());
        let levels = self.levels();
        for (gi, (_k, _panel, rows)) in geom.iter().enumerate() {
            for (i, r) in rows.iter().enumerate() {
                if r.contains(p) {
                    return levels.get(gi).and_then(|(entries, _)| entries.get(i));
                }
            }
        }
        None
    }

    /// The visible panels: for each open level, (entries, selected_row). Includes a
    /// final auto-expanded panel when the deepest selected entry is a Submenu.
    fn levels(&self) -> Vec<(&[MenuEntry], usize)> {
        let mut out: Vec<(&[MenuEntry], usize)> = Vec::new();
        let mut entries: &[MenuEntry] = &self.menu_root;
        for (k, &sel) in self.path.iter().enumerate() {
            let sel = sel.min(entries.len().saturating_sub(1));
            out.push((entries, sel));
            match entries.get(sel) {
                Some(MenuEntry::Submenu { items, .. }) if k + 1 < self.path.len() => entries = items,
                Some(MenuEntry::Submenu { items, .. }) => {
                    // deepest selected is a submenu → auto-expand one panel (row 0)
                    out.push((items.as_slice(), 0));
                    break;
                }
                _ => break,
            }
        }
        out
    }

    fn focused_entry(&self) -> Option<&MenuEntry> {
        let mut entries: &[MenuEntry] = &self.menu_root;
        let mut last = None;
        for &sel in &self.path {
            let sel = sel.min(entries.len().saturating_sub(1));
            last = entries.get(sel);
            match entries.get(sel) {
                Some(MenuEntry::Submenu { items, .. }) => entries = items,
                _ => break,
            }
        }
        last
    }

    /// Descend into the focused submenu (if any, and non-empty).
    pub fn expand(&mut self) {
        if let Some(MenuEntry::Submenu { items, .. }) = self.focused_entry() {
            if !items.is_empty() {
                self.path.push(0);
            }
        }
    }

    /// Collapse one level (never past the root).
    pub fn collapse(&mut self) {
        if self.path.len() > 1 {
            self.path.pop();
        }
    }

    /// Activate the focused entry: descend into a submenu (return None) or launch a
    /// leaf (return the app).
    pub fn activate(&mut self) -> Option<AppEntry> {
        match self.focused_entry().cloned() {
            Some(MenuEntry::Submenu { .. }) => {
                self.expand();
                None
            }
            Some(MenuEntry::Launch(a)) => Some(a),
            Some(MenuEntry::Separator) | None => None,
        }
    }

    // test/inspection helpers
    #[doc(hidden)]
    pub fn path_for_test(&self) -> Vec<usize> {
        self.path.clone()
    }
    #[doc(hidden)]
    pub fn menu_labels(&self) -> Vec<String> {
        self.menu_root.iter().map(|e| e.label().to_string()).collect()
    }
    #[doc(hidden)]
    pub fn focused_label(&self) -> Option<String> {
        self.focused_entry().map(|e| e.label().to_string())
    }

    /// The apps matching the current query (all apps in `Menu` mode), sorted by
    /// category then name so groups are contiguous for header rendering.
    pub fn filtered(&self) -> Vec<AppEntry> {
        let mut v: Vec<AppEntry> = if self.query.is_empty() {
            self.items.clone()
        } else {
            let q = self.query.to_lowercase();
            self.items.iter().filter(|a| a.name.to_lowercase().contains(&q)).cloned().collect()
        };
        v.sort_by(|a, b| {
            let (ca, cb) = (cat_of(a), cat_of(b));
            // Pin the "tetron-wm" section (Store/Settings) to the very top.
            let rank = |c: &str| if c == "tetron-wm" { 0 } else { 1 };
            rank(&ca)
                .cmp(&rank(&cb))
                .then_with(|| ca.cmp(&cb))
                .then_with(|| (a.command == "@help").cmp(&(b.command == "@help")))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        v
    }

    /// The highlighted entry (for Enter in Spotlight).
    pub fn selected_entry(&self) -> Option<AppEntry> {
        self.filtered().into_iter().nth(self.selected)
    }

    /// Render the launcher for a `w × h` screen.
    pub fn render(&self, w: i32, h: i32) -> Rendered {
        self.last_w.set(w);
        self.last_h.set(h);
        match self.open {
            Some(LauncherMode::Menu) => self.render_menu(w, h),
            Some(LauncherMode::Spotlight) => self.render_spotlight(w, h),
            None => Rendered { layers: Vec::new(), items: Vec::new() },
        }
    }

    /// Build the ordered list of rendered rows (category headers + item rows).
    fn rows(&self, filtered: &[AppEntry]) -> Vec<Row> {
        let mut out = Vec::new();
        let mut last: Option<String> = None;
        for (i, e) in filtered.iter().enumerate() {
            let c = cat_of(e);
            if last.as_deref() != Some(c.as_str()) {
                out.push(Row::Header(c.clone()));
                last = Some(c);
            }
            out.push(Row::Item(i));
        }
        out
    }

    /// The scroll window of level `k`'s panel: `(first_visible, visible_count)`. The first
    /// visible row is remembered per level and moves only when the highlight leaves the view
    /// (or the wheel scrolls), so render and hit-test agree and hovering does not shift it.
    fn window(&self, k: usize, n: usize, sel: usize, max: usize) -> (usize, usize) {
        let vis = n.min(max.max(1));
        let key: Vec<usize> = self.path.iter().take(k).copied().collect();
        let mut st = self.scroll.borrow_mut();
        while st.len() <= k {
            st.push((Vec::new(), 0));
        }
        let mut off = if st[k].0 == key { st[k].1 } else { 0 };
        off = off.min(n.saturating_sub(vis));
        if sel < off {
            off = sel;
        } else if sel >= off + vis {
            off = sel + 1 - vis;
        }
        st[k] = (key, off);
        (off, vis)
    }

    /// One rendered panel: its box rect and the per-row rects.
    /// Returns `(level, panel_rect, row_rects)` — the single geometry source shared
    /// by `render_menu`, `hover`, `click`, and `point_in_menu`.
    fn panel_geometry(&self, w: i32, h: i32) -> Vec<(usize, Rect, Vec<Rect>)> {
        let levels = self.levels();
        let mut out = Vec::new();
        let mut x = 0;
        let mut prev_sel_y = 1; // panel 0 top
        for (k, (entries, sel)) in levels.iter().enumerate() {
            // The widest row: its name, plus the CLI tag and the "opens more" marker where it
            // has them (draw_row leaves room for both, so the name is not cut).
            let label_w = entries.iter().map(|e| e.label().chars().count() + e.extra_w()).max().unwrap_or(6) as i32;
            let pw = (label_w + 4).clamp(12, MENU_PANEL_MAX_W).min(w.max(12)); // +marker/padding/border
            // Tall lists scroll: show only what fits below the menubar row.
            let (off, vis) = self.window(k, entries.len(), *sel, (h - 3).max(1) as usize);
            let ph = vis as i32 + 2; // border top/bottom
            let px = if k == 0 { 0 } else { x };
            // clamp horizontally on screen
            let px = px.min((w - pw).max(0));
            let py = if k == 0 { 1 } else { prev_sel_y };
            let py = py.min((h - ph).max(1)).max(1);
            let mut rows = Vec::new();
            for i in 0..entries.len() {
                if i >= off && i < off + vis {
                    rows.push(Rect::new(px + 1, py + 1 + (i - off) as i32, pw - 2, 1));
                } else {
                    rows.push(Rect::new(0, 0, 0, 0)); // scrolled out of view: unhittable
                }
            }
            out.push((k, Rect::new(px, py, pw, ph), rows));
            // next panel starts to the right; anchored at this panel's selected row
            x = px + pw;
            prev_sel_y = py + 1 + (sel.saturating_sub(off) as i32);
        }
        out
    }

    /// Render the cascade: one offset panel per open level, the deepest selected
    /// submenu auto-expanded. Leaf (launchable) rows are collected into
    /// `Rendered.items` for click compatibility.
    fn render_menu(&self, w: i32, h: i32) -> Rendered {
        if self.menu_root.is_empty() {
            let (bw, bh) = (18, 3);
            let mut buf = CellBuffer::new(bw, bh);
            fill_box(&mut buf, bw, bh);
            draw_row(&mut buf, 1, bw - 2, 1, "(no apps)", RowStyle { highlighted: false, marker: false, cli: false });
            return Rendered {
                layers: vec![Layer { z: 5000, origin: Point::new(0, 1), buf, opacity: 1.0, scissor: None }],
                items: Vec::new(),
            };
        }
        let levels = self.levels();
        let geom = self.panel_geometry(w, h);
        let mut layers = Vec::new();
        let mut items: Vec<(AppEntry, Rect)> = Vec::new();
        for ((k, panel, rows), (entries, sel)) in geom.iter().zip(levels.iter()) {
            let mut buf = CellBuffer::new(panel.w, panel.h);
            fill_box(&mut buf, panel.w, panel.h);
            let (off, vis) = self.window(*k, entries.len(), *sel, (h - 3).max(1) as usize);
            for (i, e) in entries.iter().enumerate() {
                if i < off || i >= off + vis {
                    continue;
                }
                let y = 1 + (i - off) as i32;
                if e.is_separator() {
                    // A divider line across the panel's inner width.
                    for x in 1..panel.w - 1 {
                        buf.set(x, y, Cell { ch: '\u{2500}', fg: border(), bg: menu_bg(), attrs: Default::default() });
                    }
                    continue;
                }
                let highlighted = i == *sel;
                let label = e.label();
                let cli = matches!(e, MenuEntry::Launch(a) if a.cli.unwrap_or(false));
                draw_row(&mut buf, 1, panel.w - 2, y, label, RowStyle { highlighted, marker: false, cli });
                if e.has_marker() {
                    // "opens more" marker at the right edge of the row
                    let (fg, bg) = if highlighted { (sel_fg(), sel_bg()) } else { (accent(), menu_bg()) };
                    buf.set(
                        panel.w - 2,
                        y,
                        Cell { ch: '\u{25B8}', fg, bg, attrs: Default::default() },
                    );
                }
                if let MenuEntry::Launch(a) = e {
                    items.push((a.clone(), rows[i]));
                }
            }
            // Scroll hints on the border when rows are hidden.
            let hint_cell = |ch: char| Cell { ch, fg: accent(), bg: menu_bg(), attrs: Default::default() };
            if off > 0 {
                buf.set(panel.w - 2, 0, hint_cell('\u{25B2}'));
            }
            if off + vis < entries.len() {
                buf.set(panel.w - 2, panel.h - 1, hint_cell('\u{25BC}'));
            }
            layers.push(Layer { z: 5000 + *k as i32, origin: Point::new(panel.x, panel.y), buf, opacity: 1.0, scissor: None });
        }
        Rendered { layers, items }
    }

    /// Mouse-move: select the (level,row) under `p`, truncating deeper levels.
    pub fn hover(&mut self, p: Point) {
        if self.open != Some(LauncherMode::Menu) {
            return;
        }
        // A separator is not selectable: leave the highlight where it was.
        if self.row_at(p).map(|e| e.is_separator()).unwrap_or(false) {
            return;
        }
        let geom = self.panel_geometry(self.last_w.get(), self.last_h.get());
        for (k, _panel, rows) in &geom {
            for (i, r) in rows.iter().enumerate() {
                if r.contains(p) {
                    let mut np: Vec<usize> = self.path.iter().take(*k).copied().collect();
                    np.push(i);
                    self.path = np;
                    return;
                }
            }
        }
    }

    /// Mouse-click: hover then activate (descend a submenu, or launch a leaf).
    /// A click *outside* the visible panels returns `None` (the caller closes the
    /// menu) — it must NOT activate the currently-selected row, or clicking away
    /// to dismiss the menu would launch/descend the selection.
    pub fn click(&mut self, p: Point) -> Option<AppEntry> {
        if self.open != Some(LauncherMode::Menu) || !self.point_in_menu(p) {
            return None;
        }
        // A click on a separator is a no-op (don't activate the stale selection).
        if self.row_at(p).map(|e| e.is_separator()).unwrap_or(false) {
            return None;
        }
        self.hover(p);
        self.activate()
    }

    /// Whether `p` is inside any visible panel (so an outside click should close).
    pub fn point_in_menu(&self, p: Point) -> bool {
        self.panel_geometry(self.last_w.get(), self.last_h.get())
            .iter()
            .any(|(_, panel, _)| panel.contains(p))
    }

    #[doc(hidden)]
    pub fn panel_count_for_test(&self, w: i32, h: i32) -> usize {
        self.panel_geometry(w, h).len()
    }
    #[doc(hidden)]
    pub fn panel_rects_for_test(&self, w: i32, h: i32) -> Vec<(usize, usize, Rect)> {
        self.panel_geometry(w, h)
            .into_iter()
            .flat_map(|(k, _p, rows)| rows.into_iter().enumerate().map(move |(i, r)| (k, i, r)))
            .collect()
    }

    fn render_spotlight(&self, w: i32, _h: i32) -> Rendered {
        let all = self.filtered();
        let (off, vis) = list_window(all.len(), self.selected, SPOTLIGHT_ROWS);
        let filtered: Vec<AppEntry> = all[off..off + vis].to_vec();
        let rows = self.rows(&filtered);
        let box_w = 46.min(w - 4).max(24);
        let inner_w = box_w - 2;
        let body = rows.len().max(1) as i32;
        let box_h = body + 4; // top border + query + separator + body + bottom border
        let origin = Point::new((w - box_w) / 2, 3);

        let mut buf = CellBuffer::new(box_w, box_h);
        fill_box(&mut buf, box_w, box_h);

        // Query row + separator.
        buf.write_str(2, 1, &format!("{} {}\u{2588}", crate::glyphs::get(crate::glyphs::Glyph::Prompt), self.query), accent(), menu_bg());
        for x in 1..box_w - 1 {
            buf.set(x, 2, Cell { ch: '\u{2500}', fg: border(), bg: menu_bg(), attrs: Default::default() });
        }

        let mut items = Vec::new();
        if rows.is_empty() {
            buf.write_str(2, 3, "no matches", hint(), menu_bg());
        }
        for (ri, row) in rows.iter().enumerate() {
            let y = 3 + ri as i32;
            match row {
                Row::Header(c) => draw_header(&mut buf, 1, box_w - 2, y, c),
                Row::Item(i) => {
                    let e = &filtered[*i];
                    let style = RowStyle { highlighted: *i + off == self.selected, marker: true, cli: e.cli.unwrap_or(false) };
                    draw_row(&mut buf, 1, box_w - 2, y, &e.name, style);
                    items.push((e.clone(), Rect::new(origin.x + 1, origin.y + y, inner_w, 1)));
                }
            }
        }

        Rendered {
            layers: vec![Layer { z: 5000, origin, buf, opacity: 1.0, scissor: None }],
            items,
        }
    }
}

/// Scroll window for a list of `n` rows with row `sel` highlighted and room for
/// `max` rows: returns `(first_visible, visible_count)`. Stateless — the
/// highlight is kept in view by sliding the window — so render and hit-test
/// agree without extra state.
fn list_window(n: usize, sel: usize, max: usize) -> (usize, usize) {
    let vis = n.min(max.max(1));
    let off = (sel + 1).saturating_sub(vis).min(n - vis);
    (off, vis)
}

/// Rows shown at once in the Spotlight result list.
const SPOTLIGHT_ROWS: usize = 8;

/// A rendered launcher row: a category header or an item (index into `filtered`).
enum Row {
    Header(String),
    Item(usize),
}

/// The category an entry belongs to ("Apps" when unset).
fn cat_of(a: &AppEntry) -> String {
    a.category.clone().unwrap_or_else(|| "Apps".into())
}

/// A quick-launch entry for the user's shell (`$SHELL`, falling back to `sh`).
fn shell_entry() -> AppEntry {
    AppEntry {
        name: "Shell".into(),
        command: std::env::var("SHELL").unwrap_or_else(|_| "sh".into()),
        args: Vec::new(),
        category: None,
        requires_cwd: None,
        cwd: None,
        cli: None,
        warn: None,
    }
}

/// Draw a dimmed, uppercase category header spanning `cw` cells from `x0`.
fn draw_header(buf: &mut CellBuffer, x0: i32, cw: i32, row: i32, cat: &str) {
    for x in x0..x0 + cw {
        buf.set(x, row, Cell { ch: ' ', fg: hint(), bg: menu_bg(), attrs: Default::default() });
    }
    let label: String = cat.to_uppercase().chars().take(cw.max(1) as usize).collect();
    buf.write_str(x0, row, &label, hint(), menu_bg());
}

/// Fill a buffer with the menu background and draw a rounded border.
fn fill_box(buf: &mut CellBuffer, w: i32, h: i32) {
    buf.fill(Cell { ch: ' ', fg: menu_fg(), bg: menu_bg(), attrs: Default::default() });
    let b = |ch: char| Cell { ch, fg: border(), bg: menu_bg(), attrs: Default::default() };
    for x in 0..w {
        buf.set(x, 0, b('\u{2500}'));
        buf.set(x, h - 1, b('\u{2500}'));
    }
    for y in 0..h {
        buf.set(0, y, b('\u{2502}'));
        buf.set(w - 1, y, b('\u{2502}'));
    }
    buf.set(0, 0, b('\u{256D}'));
    buf.set(w - 1, 0, b('\u{256E}'));
    buf.set(0, h - 1, b('\u{2570}'));
    buf.set(w - 1, h - 1, b('\u{256F}'));
}

/// Per-row rendering flags for [`draw_row`], bundled into one argument to keep
/// the function's parameter count clippy-clean.
struct RowStyle {
    /// Whether this row is the current selection (swaps to the highlight colors).
    highlighted: bool,
    /// Whether a leading `▸` marker is drawn when highlighted (Spotlight rows).
    marker: bool,
    /// Whether a right-aligned `CLI` tag is drawn (app flagged as a CLI tool).
    cli: bool,
}

/// Draw one app row spanning `cw` cells from `x0` per `style` (see [`RowStyle`]).
/// The name is truncated to fit the column, leaving room for the `CLI` tag.
fn draw_row(buf: &mut CellBuffer, x0: i32, cw: i32, row: i32, name: &str, style: RowStyle) {
    let RowStyle { highlighted, marker, cli } = style;
    let (fg, bg) = if highlighted { (sel_fg(), sel_bg()) } else { (menu_fg(), menu_bg()) };
    for x in x0..x0 + cw {
        buf.set(x, row, Cell { ch: ' ', fg, bg, attrs: Default::default() });
    }
    let lead = if marker && highlighted { "\u{25B8} " } else { "  " };
    buf.write_str(x0, row, lead, accent(), bg);
    const BADGE: &str = "CLI";
    let badge_w = if cli { BADGE.chars().count() as i32 + 1 } else { 0 };
    let avail = (cw - 2 - badge_w).max(1) as usize;
    let shown: String = if name.chars().count() > avail {
        name.chars().take(avail.saturating_sub(1)).collect::<String>() + "\u{2026}"
    } else {
        name.to_string()
    };
    buf.write_str(x0 + 2, row, &shown, fg, bg);
    if cli {
        buf.write_str(x0 + cw - BADGE.chars().count() as i32, row, BADGE, accent(), bg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(name: &str, cat: &str) -> AppEntry {
        AppEntry { name: name.into(), command: name.into(), args: vec![], category: Some(cat.into()), requires_cwd: None, cwd: None, cli: None, warn: None }
    }

    fn many() -> Vec<AppEntry> {
        vec![
            app("alpha", "Git"), app("beta", "Git"),
            app("gamma", "System"), app("delta", "System"),
            app("epsilon", "Net"), app("zeta", "Net"),
            app("eta", "Games"), app("theta", "Games"),
        ]
    }

    #[test]
    fn a_cli_app_name_is_not_cut_by_the_cli_tag() {
        let mut cli = app("A fairly long program name", "Tools");
        cli.cli = Some(true);
        let mut l = Launcher::new(vec![cli, app("Short", "Tools")]);
        l.toggle_menu();
        let _ = l.render(120, 40);
        let rects = l.panel_rects_for_test(120, 40);
        let tools = rects.iter().find(|(lvl, row, _)| *lvl == 0 && *row == 2).map(|(_, _, r)| *r).unwrap();
        l.hover(Point::new(tools.x + 1, tools.y)); // hovering the category opens its panel
        let r = l.render(120, 40);
        let text: String = r
            .layers
            .iter()
            .flat_map(|ly| {
                let b = &ly.buf;
                (0..b.height()).flat_map(move |y| (0..b.width()).map(move |x| b.get(x, y).map(|c| c.ch).unwrap_or(' ')))
            })
            .collect();
        assert!(text.contains("A fairly long program name"), "the whole name is drawn: {text}");
        assert!(text.contains("CLI"));
        assert!(!text.contains('\u{2026}'), "nothing is cut with an ellipsis");
    }

    #[test]
    fn cascade_renders_root_then_submenu_on_hover() {
        let mut l = Launcher::new(vec![app("Aaa", "Games"), app("Bbb", "Tools")]);
        l.toggle_menu();
        let r = l.render(120, 40);
        assert!(!r.layers.is_empty());
        // root rows: All(0), Shell(1), Games(2), Tools(3). Hover the "Tools" row (row 3).
        let rects = l.panel_rects_for_test(120, 40);
        let tools_row = rects.iter().find(|(lvl, row, _)| *lvl == 0 && *row == 3).map(|(_, _, r)| *r).unwrap();
        let _ = l.render(120, 40); // ensure last_w/last_h match the hover geometry
        l.hover(Point::new(tools_row.x + 1, tools_row.y));
        assert_eq!(l.focused_label(), Some("Tools".to_string()));
        // hovering a category auto-expands a second panel
        assert_eq!(l.panel_count_for_test(120, 40), 2);
    }

    #[test]
    fn click_launches_leaf_and_descends_submenu() {
        let mut l = Launcher::new(vec![app("Aaa", "Games")]);
        l.toggle_menu();
        l.move_down(); l.move_down(); // skip the Shell quick-launch (row 0) and All (row 1); select the Games category
        let _ = l.render(120, 40);
        let rects = l.panel_rects_for_test(120, 40);
        // level 1 row 0 is the leaf "Aaa" (auto-expanded under the Games category)
        let leaf = rects.iter().find(|(lvl, row, _)| *lvl == 1 && *row == 0).map(|(_, _, r)| *r).unwrap();
        let got = l.click(Point::new(leaf.x + 1, leaf.y));
        assert_eq!(got.map(|a| a.name), Some("Aaa".to_string()));
    }

    #[test]
    fn shell_is_the_first_quick_launch_entry_then_all() {
        let mut l = Launcher::new(vec![app("Aaa", "Games")]);
        l.toggle_menu();
        assert_eq!(l.path_for_test(), vec![0]); // first root row selected
        // The first top-level entry is a bare "Shell" leaf, activating it launches it.
        let a = l.activate();
        assert_eq!(a.map(|e| e.name), Some("Shell".to_string()));
        l.move_down();
        assert_eq!(l.focused_label(), Some("All".to_string()));
    }

    #[test]
    fn all_lists_every_app_flat_sorted_by_name_without_power() {
        let power = AppEntry {
            name: "Reboot".into(), command: "@reboot".into(), args: vec![],
            category: Some(POWER_CATEGORY.into()),
            requires_cwd: None, cwd: None, cli: None, warn: None,
        };
        let mut l = Launcher::new(vec![app("Ccc", "Tools"), app("bbb", "Games"), app("Aaa", "Games"), power]);
        l.toggle_menu();
        l.move_down(); // Shell(0) -> All(1)
        l.expand();
        assert_eq!(l.path_for_test(), vec![1, 0]);
        let mut names = vec![l.focused_label().unwrap()];
        for _ in 0..2 { l.move_down(); names.push(l.focused_label().unwrap()); }
        assert_eq!(names, vec!["Aaa", "bbb", "Ccc"]);
        l.move_down();
        assert_eq!(l.focused_label(), Some("Ccc".to_string()), "no Reboot, the list ends");
    }

    #[test]
    fn remote_access_holds_remote_files_and_configure_and_the_category_is_not_a_root_row() {
        let files = AppEntry {
            name: "Files on box".into(), command: "@rfiles".into(), args: vec!["box".into()],
            category: Some(REMOTE_FILES_CATEGORY.into()),
            requires_cwd: None, cwd: None, cli: None, warn: None,
        };
        let mut l = Launcher::new(vec![app("Aaa", "Games"), files]);
        l.toggle_menu();
        let labels = l.menu_labels();
        assert!(labels.contains(&"Remote access".to_string()));
        assert!(!labels.contains(&"Systems".to_string()) && !labels.contains(&"@remote-files".to_string()));
        // Down to Remote access: Shell, All, Games, sep, Remote access.
        while l.focused_label().as_deref() != Some("Remote access") { l.move_down(); }
        l.expand();
        let mut rows = vec![l.focused_label().unwrap()];
        for _ in 0..3 {
            l.move_down();
            rows.push(l.focused_label().unwrap());
        }
        assert!(rows.contains(&"Remote files".to_string()), "{rows:?}");
        assert!(rows.contains(&"Configure Remote Systems".to_string()), "{rows:?}");
    }

    #[test]
    fn wheel_over_the_all_list_scrolls_the_view_and_hover_does_not_shift_it() {
        let apps: Vec<AppEntry> = (0..40).map(|i| app(&format!("app{i:02}"), "Tools")).collect();
        let mut l = Launcher::new(apps);
        l.toggle_menu();
        l.move_down(); // Shell(0) -> All(1)
        l.expand(); // open All
        let _ = l.render(80, 12); // short screen: 9 rows are visible
        let rects = l.panel_rects_for_test(80, 12);
        let r = rects.iter().find(|(lvl, row, _)| *lvl == 1 && *row == 0).map(|(_, _, r)| *r).unwrap();
        assert_eq!(l.focused_label(), Some("app00".to_string()));
        assert!(l.wheel(Point::new(r.x + 1, r.y), -5), "wheel down is used over the panel");
        let _ = l.render(80, 12);
        let after = l.panel_rects_for_test(80, 12);
        // Rows 0..4 scrolled out of view, rows 5..13 are on screen.
        assert_eq!(after.iter().filter(|(lvl, row, r)| *lvl == 1 && *row < 5 && r.w > 0).count(), 0);
        let visible: Vec<_> = after.iter().filter(|(lvl, _, r)| *lvl == 1 && r.w > 0).cloned().collect();
        assert_eq!(visible.len(), 9);
        // Hovering any visible row selects it and leaves the list where it is.
        let target = visible[3].2;
        l.hover(Point::new(target.x + 1, target.y));
        let _ = l.render(80, 12);
        assert_eq!(l.panel_rects_for_test(80, 12), after, "hover must not shift the list");
        assert_eq!(l.focused_label(), Some("app08".to_string()));
        // Wheel up scrolls back; the highlight stays while it is in view.
        assert!(l.wheel(Point::new(r.x + 1, r.y), 2));
        assert_eq!(l.focused_label(), Some("app08".to_string()));
        assert!(!l.wheel(Point::new(79, 11), -1), "outside the panels it is not used");
    }

    #[test]
    fn all_comes_after_tetron_wm_and_before_the_other_categories() {
        let mut l = Launcher::new(vec![app("Aaa", "Games"), app("Store", "tetron-wm")]);
        l.toggle_menu();
        assert_eq!(
            &l.menu_labels()[..4],
            &["Shell".to_string(), "tetron-wm".to_string(), "All".to_string(), "Games".to_string()]
        );
    }

    #[test]
    fn closed_launcher_renders_nothing() {
        let l = Launcher::new(many());
        let r = l.render(120, 40);
        assert!(r.items.is_empty());
        assert!(r.layers.is_empty());
    }

    #[test]
    fn cascade_root_groups_by_category_and_navigates() {
        let mut l = Launcher::new(vec![
            app("Aaa", "Games"), app("Bbb", "Games"), app("Ccc", "Tools"),
        ]);
        l.toggle_menu();
        // root: a "Shell" quick-launch, "All" (every app, flat), the category submenus (sorted), then a
        // separator and the system section (Remote access, then Detach/Reload/Quit).
        assert_eq!(
            l.menu_labels(),
            vec!["Shell", "All", "Games", "Tools", "", "Remote access", "", "Detach", "Reload", "Quit"]
        );
        assert_eq!(l.path_for_test(), vec![0]); // Shell selected first
        l.move_down(); l.move_down(); // select Games (root index 2)
        assert_eq!(l.path_for_test(), vec![2]);
        // descend into Games → its apps
        l.expand();
        assert_eq!(l.path_for_test(), vec![2, 0]);
        assert_eq!(l.focused_label(), Some("Aaa".to_string()));
        l.move_down();
        assert_eq!(l.focused_label(), Some("Bbb".to_string()));
        // activate a leaf returns the app
        assert_eq!(l.activate().map(|a| a.name), Some("Bbb".to_string()));
        // collapse back to root
        l.toggle_menu(); l.toggle_menu(); // reopen fresh
        l.move_down(); l.move_down(); l.expand(); l.collapse();
        assert_eq!(l.path_for_test(), vec![2]);
    }

    #[test]
    fn system_section_skips_separator_and_yields_sentinels() {
        let mut l = Launcher::new(vec![app("Aaa", "Games"), app("Ccc", "Tools")]);
        l.toggle_menu();
        // Shell(0) All(1) Games(2) Tools(3) sep(4) Remote access(5) sep(6) Detach(7) Reload(8) Quit(9)
        l.move_down(); // All(1)
        l.move_down(); // Games(2)
        l.move_down(); // Tools(3)
        assert_eq!(l.path_for_test(), vec![3]);
        l.move_down(); // skip separator(4) → Remote access(5)
        assert_eq!(l.path_for_test(), vec![5]);
        l.expand(); // the sshd row comes first; Configure Remote Systems is last
        while l.focused_label().as_deref() != Some("Configure Remote Systems") { l.move_down(); }
        assert_eq!(l.activate().map(|e| e.command), Some("@systems".to_string()));
        l.collapse();
        l.move_down(); // skip separator(6) → Detach(7)
        assert_eq!(l.path_for_test(), vec![7]);
        assert_eq!(l.activate().map(|e| e.command), Some("@exit".to_string()));
        l.move_down(); // Reload(7)
        assert_eq!(l.activate().map(|e| e.command), Some("@restart".to_string()));
        l.move_down(); // Quit(9)
        assert_eq!(l.activate().map(|e| e.command), Some("@shutdown".to_string()));
        // move_up skips the separators on the way back up too
        l.move_up(); l.move_up(); l.move_up(); // Reload, Detach, skip sep → Remote access(5)
        assert_eq!(l.path_for_test(), vec![5]);
        l.move_up(); // skip separator(4) → Tools(3)
        assert_eq!(l.path_for_test(), vec![3]);
    }

    #[test]
    fn power_entries_form_a_bottom_submenu_after_quit() {
        let power = |name: &str, cmd: &str| AppEntry {
            name: name.into(), command: cmd.into(), args: vec![],
            category: Some(POWER_CATEGORY.into()),
            requires_cwd: None, cwd: None, cli: None, warn: None,
        };
        let mut l = Launcher::new(vec![
            app("Aaa", "Games"),
            power("Suspend", "@suspend"), power("Reboot", "@reboot"), power("Power off", "@poweroff"),
        ]);
        l.toggle_menu();
        assert_eq!(
            l.menu_labels(),
            vec!["Shell", "All", "Games", "", "Remote access", "", "Detach", "Reload", "Quit", "", "Power"],
            "Power is the last row, set apart from Quit, and is not an app category"
        );
        // Walk to Power (index 10), open it, and read its rows in order.
        for _ in 0..9 { l.move_down(); }
        assert_eq!(l.path_for_test(), vec![10]);
        l.expand();
        assert_eq!(l.focused_label(), Some("Suspend".to_string()));
        l.move_down();
        assert_eq!(l.focused_label(), Some("Reboot".to_string()));
        l.move_down();
        assert_eq!(l.activate().map(|e| e.command), Some("@poweroff".to_string()));
    }

    #[test]
    fn no_power_row_when_the_computer_cannot_do_any() {
        let mut l = Launcher::new(vec![app("Aaa", "Games")]);
        l.toggle_menu();
        assert!(!l.menu_labels().contains(&"Power".to_string()));
    }

    #[test]
    fn list_window_keeps_selection_visible() {
        assert_eq!(list_window(3, 2, 8), (0, 3));
        assert_eq!(list_window(20, 0, 8), (0, 8));
        assert_eq!(list_window(20, 7, 8), (0, 8));
        assert_eq!(list_window(20, 8, 8), (1, 8));
        assert_eq!(list_window(20, 19, 8), (12, 8));
    }

    #[test]
    fn tall_menu_panel_fits_screen_and_hides_offscreen_rows() {
        let apps: Vec<AppEntry> = (0..60)
            .map(|i| AppEntry { name: format!("app{i:02}"), command: "true".into(), args: vec![], category: Some("Big".into()), requires_cwd: None, cwd: None, cli: None, warn: None })
            .collect();
        let mut l = Launcher::new(apps);
        l.toggle_menu();
        let h = 20;
        for (_, panel, _) in l.panel_geometry(80, h) {
            assert!(panel.y + panel.h <= h, "panel must fit the screen");
        }
    }

    #[test]
    fn activate_on_category_descends_not_launches() {
        let mut l = Launcher::new(vec![app("Aaa", "Games")]);
        l.toggle_menu();
        l.move_down(); l.move_down(); // skip the Shell quick-launch and All; select the Games category
        assert!(l.activate().is_none()); // category → descend, no launch
        assert_eq!(l.path_for_test(), vec![2, 0]);
        assert_eq!(l.activate().map(|a| a.name), Some("Aaa".to_string())); // now the leaf
    }
}
