//! The native file-manager widget: directory state, selection, navigation,
//! overlays, and operation dispatch. All disk access goes through `FsOps`.

use crate::fileops::{Entry, FsOps, StdFs};
use crate::geometry::{Point, Rect};
use crate::openwith::{resolve, OpenAction};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Icon,
    List,
    Columns,
}

/// What the widget asks the session to do (the session owns windows/PTYs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileManagerAction {
    /// Open the builtin image viewer for this path.
    OpenImage(PathBuf),
    /// Launch a TUI app with these args (the file path already appended).
    RunApp { command: String, args: Vec<String> },
    /// Right-click actions the session carries out.
    Fullscreen(PathBuf),
    SetWallpaper(PathBuf),
    CopyPath(PathBuf),
    TerminalAt(PathBuf),
    PlayFullscreen(PathBuf),
    /// Extract (`list` false) or list an archive, in a window.
    Archive { path: PathBuf, list: bool },
    /// Copy to the Systems entry `system` with scp, in a window.
    SendTo { system: String, path: PathBuf },
    /// Cast the file to a Chromecast with catt, in a window.
    Cast(PathBuf),
    /// Compress or encrypt the entry beside it, in a window.
    Pack { path: PathBuf, pack: crate::contextmenu::Pack },
}

/// Modal overlays drawn on top of the listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Overlay {
    NewFolder { name: String },
    Rename { idx: usize, name: String },
    /// Move the selection to the Trash, or (`empty`) delete what the Trash holds.
    ConfirmDelete { count: usize, empty: bool },
    /// Context menu (the shared [`crate::contextmenu::ContextMenu`]) for entry
    /// `idx`, anchored at a content-local point: the click point for a
    /// right-click, or the entry's tile/row for the keyboard. "Open with…"
    /// swaps its items for the apps that can open the entry.
    Context { idx: usize, menu: crate::contextmenu::ContextMenu },
    Error { message: String },
    GetInfo { idx: usize },
}

#[derive(Clone, Debug)]
struct Clipboard {
    paths: Vec<PathBuf>,
    cut: bool,
}

/// Per-folder state for one open tab.
struct Tab {
    cwd: PathBuf,
    entries: Vec<Entry>,
    cursor: usize,
    selection: BTreeSet<usize>,
    view: ViewMode,
    show_hidden: bool,
    history: Vec<PathBuf>,
    hpos: usize,
    scroll: i32,
    /// Entry index → loaded thumbnail ImageId (filled by the session).
    thumbs: std::collections::HashMap<usize, u64>,
    /// Whether the right-hand preview pane is open.
    preview: bool,
}

impl Tab {
    fn new(cwd: PathBuf) -> Tab {
        Tab {
            cwd: cwd.clone(),
            entries: Vec::new(),
            cursor: 0,
            selection: BTreeSet::new(),
            view: ViewMode::List,
            show_hidden: false,
            history: vec![cwd],
            hpos: 0,
            scroll: 0,
            thumbs: std::collections::HashMap::new(),
            preview: false,
        }
    }
}

/// The file manager. Generic over the filesystem backend for testability.
/// Per-folder state lives on the active `Tab`; widget-global state stays here.
pub struct FileManager<F: FsOps = StdFs> {
    fs: F,
    /// The sidebar's Code entry (`assistant_code_dir`); `~/code` when unset.
    code_dir: Option<PathBuf>,
    tabs: Vec<Tab>,
    active: usize,
    clipboard: Option<Clipboard>,
    overlay: Option<Overlay>,
    handlers: BTreeMap<String, String>,
    status: String,
    action: Option<FileManagerAction>,
    /// Tiles per row in the last Icon render; navigation uses it. Updated by render.
    cols_per_row: std::cell::Cell<i32>,
    /// Entry rows visible in the content area in the last render (view-dependent:
    /// list/columns = text rows, icon = tile rows). `move_cursor` uses it to keep
    /// the cursor on screen by updating the active tab's `scroll` offset.
    vis_rows: std::cell::Cell<i32>,
    /// The last preview body, keyed by (path, size, mtime, line budget) from the listing. Render asks for
    /// it every frame; the cache keeps a PDF preview from running pdftotext (and
    /// a text preview from reading the file) once per frame.
    preview_cache: std::cell::RefCell<Option<Box<PreviewKey>>>,
}

/// A cached preview: path, size, mtime, line budget, body.
type PreviewKey = (PathBuf, u64, Option<std::time::SystemTime>, usize, Vec<String>);

impl FileManager<StdFs> {
    /// Open a file manager rooted at `cwd` with the given `[default_apps]` map.
    pub fn new(cwd: PathBuf, handlers: BTreeMap<String, String>) -> Self {
        Self::with_fs(StdFs, cwd, handlers)
    }
}

/// The boxed backend the session uses, so one window type can browse either the
/// local disk or a saved remote system (see [`crate::fileops::SshFs`]).
pub type DynFs = Box<dyn FsOps + Send>;
pub type DynFileManager = FileManager<DynFs>;

impl FileManager<DynFs> {
    /// A boxed-backend file manager over the local disk.
    pub fn new_local(cwd: PathBuf, handlers: BTreeMap<String, String>) -> Self {
        Self::with_fs(Box::new(StdFs), cwd, handlers)
    }

    /// A boxed-backend file manager browsing `target` over ssh, rooted at `cwd`
    /// (normally the remote home from [`crate::fileops::SshFs::remote_home`]).
    pub fn new_remote(target: String, port: Option<u16>, cwd: PathBuf, handlers: BTreeMap<String, String>) -> Self {
        Self::with_fs(Box::new(crate::fileops::SshFs::new(target, port)), cwd, handlers)
    }
}

impl<F: FsOps> FileManager<F> {
    pub fn with_fs(fs: F, cwd: PathBuf, handlers: BTreeMap<String, String>) -> Self {
        let mut me = Self {
            fs,
            code_dir: None,
            tabs: vec![Tab::new(cwd)],
            active: 0,
            clipboard: None,
            overlay: None,
            handlers,
            status: String::new(),
            action: None,
            cols_per_row: std::cell::Cell::new(1),
            vis_rows: std::cell::Cell::new(1),
            preview_cache: std::cell::RefCell::new(None),
        };
        me.reload();
        me
    }

    fn tab(&self) -> &Tab { &self.tabs[self.active] }
    fn tab_mut(&mut self) -> &mut Tab { &mut self.tabs[self.active] }

    pub fn cwd(&self) -> &Path { &self.tab().cwd }
    pub fn entries(&self) -> &[Entry] { &self.tab().entries }
    pub fn cursor(&self) -> usize { self.tab().cursor }
    pub fn view(&self) -> ViewMode { self.tab().view }
    pub fn status(&self) -> &str { &self.status }
    pub fn set_status(&mut self, s: String) { self.status = s; }
    pub fn overlay(&self) -> Option<&Overlay> { self.overlay.as_ref() }
    pub fn is_editing(&self) -> bool {
        matches!(self.overlay, Some(Overlay::NewFolder { .. }) | Some(Overlay::Rename { .. }))
    }
    pub fn selection_indices(&self) -> Vec<usize> { self.tab().selection.iter().copied().collect() }

    /// Take a pending action requested by the user (cleared on read).
    pub fn take_action(&mut self) -> Option<FileManagerAction> { self.action.take() }

    // ---- tabs --------------------------------------------------------------

    pub fn tab_count(&self) -> usize { self.tabs.len() }
    pub fn active_tab(&self) -> usize { self.active }

    /// Open a new tab rooted at the active tab's current folder and focus it.
    pub fn new_tab(&mut self) {
        let cwd = self.tab().cwd.clone();
        let mut t = Tab::new(cwd);
        t.entries = self.fs.list(&t.cwd, t.show_hidden).unwrap_or_default();
        self.tabs.push(t);
        self.active = self.tabs.len() - 1;
    }

    /// Close the active tab (no-op when only one remains).
    pub fn close_tab(&mut self) {
        if self.tabs.len() > 1 {
            self.tabs.remove(self.active);
            self.active = self.active.min(self.tabs.len() - 1);
        }
    }

    /// Cycle focus to the next tab.
    pub fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + 1) % self.tabs.len();
        }
    }

    /// Re-list the current directory, clamping cursor/selection.
    pub fn reload(&mut self) {
        let (cwd, show) = { let t = self.tab(); (t.cwd.clone(), t.show_hidden) };
        let listed = self.fs.list(&cwd, show);
        match listed {
            Ok(es) => {
                self.tab_mut().entries = es;
                self.status.clear();
            }
            Err(e) => {
                self.tab_mut().entries.clear();
                self.status = format!("Cannot read {}: {}", cwd.display(), e);
            }
        }
        let t = self.tab_mut();
        t.cursor = t.cursor.min(t.entries.len().saturating_sub(1));
        t.selection.clear();
        t.scroll = 0;
        t.thumbs.clear();
    }

    pub fn toggle_hidden(&mut self) {
        self.tab_mut().show_hidden = !self.tab().show_hidden;
        self.reload();
    }

    pub fn set_view(&mut self, v: ViewMode) { self.tab_mut().view = v; }

    /// Cycle Icon -> List -> Columns -> Icon.
    pub fn cycle_view(&mut self) {
        let next = match self.tab().view {
            ViewMode::Icon => ViewMode::List,
            ViewMode::List => ViewMode::Columns,
            ViewMode::Columns => ViewMode::Icon,
        };
        self.tab_mut().view = next;
    }

    pub fn preview_open(&self) -> bool { self.tab().preview }
    pub fn toggle_preview(&mut self) { self.tab_mut().preview = !self.tab().preview; }

    /// Width (cells) reserved for the preview pane when it is open.
    fn preview_reserve(&self, w: i32) -> i32 {
        if self.tab().preview { (w / 3).clamp(20, 48) } else { 0 }
    }

    /// The preview body for the focused entry (≤ `max` lines).
    pub fn preview_lines(&self, max: usize) -> Vec<String> {
        let t = self.tab();
        let Some(e) = t.entries.get(t.cursor) else { return vec![]; };
        use crate::openwith::Role::*;
        if matches!(e.role, Text | Code | Pdf) {
            if let Some((p, size, modified, m, lines)) = self.preview_cache.borrow().as_deref() {
                if *p == e.path && *size == e.size && *modified == e.modified && *m == max {
                    return lines.clone();
                }
            }
            let lines = if e.role == Pdf { pdf_preview(&e.path, max) } else { read_head(&e.path, max) };
            *self.preview_cache.borrow_mut() = Some(Box::new((e.path.clone(), e.size, e.modified, max, lines.clone())));
            return lines;
        }
        match e.role {
            Directory => vec![format!("{} \u{2014} folder", e.name)],
            _ => {
                let mut v = vec![
                    format!("Name: {}", e.name),
                    format!("Kind: {}", e.role.label()),
                    format!("Size: {} bytes", e.size),
                ];
                v.truncate(max);
                v
            }
        }
    }

    /// Image entries (index, path) in the current view that should have a thumbnail.
    pub fn thumbnail_requests(&self) -> Vec<(usize, std::path::PathBuf)> {
        self.tab()
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.role == crate::openwith::Role::Image)
            .map(|(i, e)| (i, e.path.clone()))
            .collect()
    }

    /// Record a loaded thumbnail id for entry `idx`.
    pub fn set_thumb(&mut self, idx: usize, id: u64) {
        self.tab_mut().thumbs.insert(idx, id);
    }

    /// Non-image entries (index, role) that should show a role-icon tile —
    /// the same `icons::role_icon_png` tiles the desktop uses, one PNG shared
    /// by every entry of that role. Image entries use `thumbnail_requests`
    /// (their own file's contents) instead.
    pub fn role_icon_requests(&self) -> Vec<(usize, crate::openwith::Role)> {
        self.tab()
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.role != crate::openwith::Role::Image)
            .map(|(i, e)| (i, e.role))
            .collect()
    }

    /// Placements for loaded thumbnails, in the Icon view's tile grid, offset into
    /// `content` (the window content rect, cells). Only on-screen tiles are emitted.
    pub fn thumbnail_placements(
        &self,
        content: crate::geometry::Rect,
        visible: bool,
    ) -> Vec<crate::protocol::ImagePlacement> {
        // Thumbnails only render in Icon view (List shows glyphs).
        if self.tab().view != ViewMode::Icon {
            return Vec::new();
        }
        let top = self.content_top();
        let area_x = SIDEBAR_W;
        let area_w = (content.w - SIDEBAR_W - self.preview_reserve(content.w)).max(1);
        let cols = icon_cols(area_w);
        let visible_rows = icon_visible_rows(top, content.h);
        let off = self.scroll_rows();
        let eff_top = top - off * TILE_H; // match render's scrolled grid origin
        let mut out = Vec::new();
        for (&idx, &id) in &self.tab().thumbs {
            if idx >= self.tab().entries.len() {
                continue;
            }
            let row = idx as i32 / cols - off;
            if row < 0 || row >= visible_rows {
                continue; // outside the viewport
            }
            let tile = icon_tile_rect(content.x + area_x, content.y + eff_top, cols, idx);
            let rect = icon_image_rect(tile);
            out.push(crate::protocol::ImagePlacement {
                id,
                rect,
                cols: rect.w.max(1) as u16,
                rows: rect.h.max(1) as u16,
                visible,
                src: None,
            });
        }
        out.sort_by_key(|p| p.id); // deterministic order
        out
    }

    /// Move the cursor by (dx tiles, dy rows). In List view any nonzero delta is
    /// collapsed to a single ±1 step.
    pub fn move_cursor(&mut self, dx: i32, dy: i32) {
        if self.tab().entries.is_empty() || self.dialog_open() { return; }
        let n = self.tab().entries.len() as i32;
        let cur = self.tab().cursor as i32;
        let next = match self.tab().view {
            ViewMode::List | ViewMode::Columns => cur + dx.signum() + dy.signum(),
            ViewMode::Icon => {
                let cols = self.cols_per_row.get().max(1);
                cur + dx.signum() + dy.signum() * cols
            }
        };
        self.tab_mut().cursor = next.clamp(0, n - 1) as usize;
        self.follow_scroll();
    }

    /// Entries drawn per content row for the current view (1 for list/columns,
    /// `cols_per_row` for icon). Used to map a cursor index to its screen row.
    fn per_row(&self) -> i32 {
        match self.tab().view {
            ViewMode::Icon => self.cols_per_row.get().max(1),
            _ => 1,
        }
    }

    /// The scroll offset (in content rows) to apply when drawing/hit-testing the
    /// current tab, clamped to a valid range for the current geometry. Read by
    /// render/hit_test/thumbnail_placements so they agree even if the stored
    /// `scroll` is momentarily stale (e.g. right after a resize or view toggle).
    fn scroll_rows(&self) -> i32 {
        let per_row = self.per_row();
        let vis = self.vis_rows.get().max(1);
        let n = self.tab().entries.len() as i32;
        let total_rows = (n + per_row - 1) / per_row; // ceil
        let max_off = (total_rows - vis).max(0);
        self.tab().scroll.clamp(0, max_off)
    }

    /// After the cursor moves, scroll the view the minimum amount needed to keep
    /// the cursor's row on screen. Without this the cursor walks off the bottom
    /// of the viewport invisibly and the list never scrolls (the original bug).
    fn follow_scroll(&mut self) {
        let per_row = self.per_row();
        let vis = self.vis_rows.get().max(1);
        let n = self.tab().entries.len() as i32;
        let total_rows = (n + per_row - 1) / per_row;
        let max_off = (total_rows - vis).max(0);
        let cursor_row = self.tab().cursor as i32 / per_row;
        let t = self.tab_mut();
        let mut off = t.scroll;
        if cursor_row < off {
            off = cursor_row;
        } else if cursor_row >= off + vis {
            off = cursor_row - vis + 1;
        }
        t.scroll = off.clamp(0, max_off);
    }

    /// Select entry `idx`. `ctrl` toggles it into the set; `shift` selects the
    /// range from the current cursor; neither makes it the sole selection.
    /// Put the cursor on the entry at `path` (if listed) and show its Get Info box.
    pub fn show_info_for(&mut self, path: &std::path::Path) -> bool {
        let Some(i) = self.tab().entries.iter().position(|e| e.path == path) else { return false };
        self.select_at(i, false, false);
        self.begin_get_info();
        true
    }

    pub fn select_at(&mut self, idx: usize, ctrl: bool, shift: bool) {
        let cursor = self.tab().cursor;
        if idx >= self.tab().entries.len() { return; }
        let t = self.tab_mut();
        if shift {
            let (lo, hi) = if idx >= cursor { (cursor, idx) } else { (idx, cursor) };
            t.selection = (lo..=hi).collect();
        } else if ctrl {
            if !t.selection.remove(&idx) { t.selection.insert(idx); }
        } else {
            t.selection.clear();
            t.selection.insert(idx);
        }
        t.cursor = idx;
    }

    /// Change directory, pushing onto history (truncating any forward entries).
    fn navigate_to(&mut self, dir: PathBuf) {
        let t = self.tab_mut();
        t.cwd = dir.clone();
        t.history.truncate(t.hpos + 1);
        t.history.push(dir);
        t.hpos = t.history.len() - 1;
        t.cursor = 0;
        self.reload();
    }

    /// Enter the focused entry: navigate into a directory, or request an open
    /// action for a file (via `openwith::resolve`).
    pub fn activate(&mut self) {
        // Enter on a dialog answers it; it never opens the entry underneath.
        if self.dialog_open() {
            self.dialog_commit();
            return;
        }
        let cursor = self.tab().cursor;
        let Some(entry) = self.tab().entries.get(cursor) else { return; };
        let path = entry.path.clone();
        let is_dir = entry.is_dir;
        match resolve(&path, is_dir, &self.handlers) {
            OpenAction::Navigate => self.navigate_to(path),
            OpenAction::Builtin("@image") => {
                self.action = Some(FileManagerAction::OpenImage(path));
            }
            OpenAction::Builtin(_) => {}
            OpenAction::RunApp { command, args } => {
                self.action = Some(FileManagerAction::RunApp { command, args });
            }
            OpenAction::OpenWithMenu => {
                // No default app for this type: offer the ones that can open it.
                let at = self.entry_anchor(cursor);
                let items = crate::contextmenu::open_with_actions(&path);
                if !items.is_empty() {
                    self.overlay = Some(Overlay::Context { idx: cursor, menu: crate::contextmenu::ContextMenu::new(at, items) });
                }
            }
        }
    }

    pub fn go_parent(&mut self) {
        if self.dialog_open() { return; }
        if let Some(parent) = self.tab().cwd.parent() {
            self.navigate_to(parent.to_path_buf());
        }
    }

    pub fn go_back(&mut self) {
        let t = self.tab_mut();
        if t.hpos > 0 {
            t.hpos -= 1;
            t.cwd = t.history[t.hpos].clone();
            t.cursor = 0;
            self.reload();
        }
    }

    pub fn go_forward(&mut self) {
        let t = self.tab_mut();
        if t.hpos + 1 < t.history.len() {
            t.hpos += 1;
            t.cwd = t.history[t.hpos].clone();
            t.cursor = 0;
            self.reload();
        }
    }

    // ---- overlay lifecycle -------------------------------------------------

    pub fn begin_new_folder(&mut self) { self.overlay = Some(Overlay::NewFolder { name: String::new() }); }

    pub fn begin_rename(&mut self) {
        let cursor = self.tab().cursor;
        if let Some(e) = self.tab().entries.get(cursor) {
            self.overlay = Some(Overlay::Rename { idx: cursor, name: e.name.clone() });
        }
    }

    pub fn begin_delete(&mut self) {
        let count = if self.tab().selection.is_empty() { 1 } else { self.tab().selection.len() };
        self.overlay = Some(Overlay::ConfirmDelete { count, empty: false });
    }

    /// Open the context menu for the focused entry (keyboard-invoked),
    /// anchored at its tile/row so it appears right where the entry is.
    pub fn begin_context(&mut self) {
        let idx = self.tab().cursor;
        if idx >= self.tab().entries.len() {
            return;
        }
        let at = self.entry_anchor(idx);
        self.open_menu(idx, at);
    }

    /// Open the context menu for entry `idx`, anchored at the exact
    /// content-local point `at` — used by the mouse right-click path, which
    /// already knows where the user clicked (mirrors the desktop's
    /// `right_click`).
    pub fn begin_context_at(&mut self, idx: usize, at: Point) {
        if idx >= self.tab().entries.len() {
            return;
        }
        self.select_at(idx, false, false);
        self.open_menu(idx, at);
    }

    /// The menu of the empty area of the listing (Paste, New Folder, …),
    /// anchored at the click.
    pub fn begin_background_context(&mut self, at: Point) {
        let can = crate::contextmenu::Abilities::current();
        let items = crate::contextmenu::actions_for(
            crate::contextmenu::Surface::Files,
            crate::contextmenu::Target::Background,
            if self.fs.is_local() { can } else { can.remote() },
        );
        // An index past the entries marks the background: no entry is the target.
        let idx = self.tab().entries.len();
        self.overlay = Some(Overlay::Context { idx, menu: crate::contextmenu::ContextMenu::new(at, items) });
    }

    /// Ask before emptying the Trash (nothing to ask when it is empty).
    pub fn begin_empty_trash(&mut self) {
        let count = crate::fileops::trash_count();
        self.overlay = (count > 0).then_some(Overlay::ConfirmDelete { count, empty: true });
    }

    fn open_menu(&mut self, idx: usize, at: Point) {
        let (is_dir, role) = self.tab().entries.get(idx).map_or((false, crate::openwith::Role::Other), |e| (e.is_dir, e.role));
        let can = crate::contextmenu::Abilities::current();
        let items = crate::contextmenu::actions_for(
            crate::contextmenu::Surface::Files,
            crate::contextmenu::Target::Entry { is_dir, pinned: false, role },
            if self.fs.is_local() { can } else { can.remote() },
        );
        self.overlay = Some(Overlay::Context { idx, menu: crate::contextmenu::ContextMenu::new(at, items) });
    }

    /// The content-local point to anchor a keyboard-invoked context menu for
    /// `idx`, matching render's layout for the active view.
    fn entry_anchor(&self, idx: usize) -> Point {
        let t = self.tab();
        let top = self.content_top();
        // Scrolled like render and hit-test, so the menu sits at the entry.
        let off = self.scroll_rows();
        match t.view {
            // Just below the row, so the entry stays visible above the menu.
            ViewMode::List | ViewMode::Columns => Point::new(SIDEBAR_W, top + idx as i32 - off + 1),
            ViewMode::Icon => {
                let cols = self.cols_per_row.get().max(1);
                let tile = icon_tile_rect(SIDEBAR_W, top - off * TILE_H, cols, idx);
                Point::new(tile.x, tile.y)
            }
        }
    }

    /// Whether the context menu overlay is open.
    pub fn context_menu_open(&self) -> bool {
        matches!(self.overlay, Some(Overlay::Context { .. }))
    }

    /// Move the context-menu highlight up (wrapping).
    pub fn context_menu_up(&mut self) {
        if let Some(Overlay::Context { menu, .. }) = &mut self.overlay {
            menu.up();
        }
    }

    /// Move the context-menu highlight down (wrapping).
    pub fn context_menu_down(&mut self) {
        if let Some(Overlay::Context { menu, .. }) = &mut self.overlay {
            menu.down();
        }
    }

    /// Highlight the menu row under the content-local point `p` (mouse hover).
    pub fn context_menu_hover(&mut self, p: Point, w: i32, h: i32) {
        if let Some(Overlay::Context { menu, .. }) = &mut self.overlay {
            menu.hover(p, crate::geometry::Rect::new(0, 0, w, h));
        }
    }

    /// Perform the highlighted context-menu action.
    pub fn context_menu_commit(&mut self) {
        if let Some(Overlay::Context { idx, menu }) = self.overlay.take() {
            if let Some(action) = menu.selected().cloned() {
                self.run_context_action(idx, menu, action);
            }
        }
    }

    /// Handle a content-local click while the context menu is open: a hit on
    /// a row performs that action, a miss closes the menu. Returns `true`
    /// either way (the overlay always consumes the click while it's open),
    /// or `false` when no menu is open (so the caller falls back to normal
    /// FM click handling).
    pub fn context_menu_click(&mut self, p: Point, w: i32, h: i32) -> bool {
        let Some(Overlay::Context { idx, menu }) = self.overlay.take() else {
            return false;
        };
        if let Some(action) = menu.item_at(p, crate::geometry::Rect::new(0, 0, w, h)).cloned() {
            self.run_context_action(idx, menu, action);
        }
        true
    }

    /// Dispatch a context-menu action, reusing the FM's existing operations.
    /// "Open with…" keeps the menu open with the app choices instead.
    fn run_context_action(&mut self, idx: usize, mut menu: crate::contextmenu::ContextMenu, action: crate::contextmenu::MenuAction) {
        use crate::contextmenu::MenuAction as A;
        self.select_at(idx, false, false);
        let path = self.tab().entries.get(idx).map(|e| e.path.clone());
        match action {
            A::Open => self.activate(),
            A::OpenWith => {
                let items = path.as_deref().map(crate::contextmenu::open_with_actions).unwrap_or_default();
                if !items.is_empty() {
                    menu.replace(items);
                    self.overlay = Some(Overlay::Context { idx, menu });
                }
            }
            A::OpenWithApp(cmd) => {
                if let Some(path) = path {
                    self.action = match crate::openwith::open_with(&cmd, &path) {
                        OpenAction::Builtin("@image") => Some(FileManagerAction::OpenImage(path)),
                        OpenAction::RunApp { command, args } => Some(FileManagerAction::RunApp { command, args }),
                        _ => None,
                    };
                }
            }
            A::Rename => self.begin_rename(),
            A::Copy => self.copy_selection(),
            A::Cut => self.cut_selection(),
            A::Paste => self.paste(),
            A::PasteInto => {
                if let Some(dir) = path {
                    self.paste_to(&dir);
                }
            }
            A::EmptyTrash => self.begin_empty_trash(),
            A::Trash => self.begin_delete(),
            A::GetInfo => self.begin_get_info(),
            A::ViewFullscreen => self.action = path.map(FileManagerAction::Fullscreen),
            A::SetWallpaper => self.action = path.map(FileManagerAction::SetWallpaper),
            A::CopyPath => self.action = path.map(FileManagerAction::CopyPath),
            A::OpenTerminalHere => self.action = Some(FileManagerAction::TerminalAt(path.unwrap_or_else(|| self.tab().cwd.clone()))),
            A::PlayFullscreen => self.action = path.map(FileManagerAction::PlayFullscreen),
            A::ExtractHere | A::ListContents => {
                let list = action == A::ListContents;
                self.action = path.map(|path| FileManagerAction::Archive { path, list });
            }
            A::SendTo => {
                let items = crate::contextmenu::send_to_actions();
                if !items.is_empty() {
                    menu.replace(items);
                    self.overlay = Some(Overlay::Context { idx, menu });
                }
            }
            A::SendToSystem(system) => self.action = path.map(|path| FileManagerAction::SendTo { system, path }),
            A::Compress | A::Encrypt => {
                let items = crate::contextmenu::pack_actions(action == A::Encrypt);
                if !items.is_empty() {
                    menu.replace(items);
                    self.overlay = Some(Overlay::Context { idx, menu });
                }
            }
            A::PackAs(pack) => self.action = path.map(|path| FileManagerAction::Pack { path, pack }),
            A::CastTo => self.action = path.map(FileManagerAction::Cast),
            A::NewFolder => self.begin_new_folder(),
            A::Unpin | A::CleanUp => {}
            A::Minimize | A::Maximize | A::MoveWindow | A::RenameWindow | A::CloseWindow
            | A::StoreInstall(_) | A::OpenLink(..) | A::CopyLink(..) => {}
        }
    }

    pub fn begin_get_info(&mut self) {
        self.overlay = Some(Overlay::GetInfo { idx: self.tab().cursor });
    }

    pub fn cancel_overlay(&mut self) { self.overlay = None; }

    pub fn overlay_char(&mut self, c: char) {
        match &mut self.overlay {
            Some(Overlay::NewFolder { name }) | Some(Overlay::Rename { name, .. }) => name.push(c),
            _ => {}
        }
    }

    pub fn overlay_backspace(&mut self) {
        match &mut self.overlay {
            Some(Overlay::NewFolder { name }) | Some(Overlay::Rename { name, .. }) => { name.pop(); }
            _ => {}
        }
    }

    /// Commit a NewFolder or Rename overlay.
    pub fn overlay_commit(&mut self) {
        match self.overlay.take() {
            Some(Overlay::NewFolder { name }) if !name.trim().is_empty() => {
                let cwd = self.tab().cwd.clone();
                if let Err(e) = self.fs.mkdir(&cwd, name.trim()) {
                    self.status = format!("New folder failed: {e}");
                }
                self.reload();
            }
            Some(Overlay::Rename { idx, name }) if !name.trim().is_empty() => {
                let path = self.tab().entries.get(idx).map(|e| e.path.clone());
                if let Some(path) = path {
                    if let Err(e) = self.fs.rename(&path, name.trim()) {
                        self.status = format!("Rename failed: {e}");
                    }
                }
                self.reload();
            }
            _ => {}
        }
    }

    // ---- clipboard ---------------------------------------------------------

    pub fn selected_paths(&self) -> Vec<PathBuf> {
        let t = self.tab();
        if t.selection.is_empty() {
            t.entries.get(t.cursor).map(|e| vec![e.path.clone()]).unwrap_or_default()
        } else {
            t.selection.iter().filter_map(|&i| t.entries.get(i)).map(|e| e.path.clone()).collect()
        }
    }

    // On this machine the clipboard is shared with the desktop and the other
    // windows (fileops::clip_set); a remote folder keeps its own.
    pub fn copy_selection(&mut self) { self.clip(false); }

    pub fn cut_selection(&mut self) { self.clip(true); }

    fn clip(&mut self, cut: bool) {
        let paths = self.selected_paths();
        if paths.is_empty() { return; }
        let n = paths.len();
        if self.fs.is_local() {
            crate::fileops::clip_set(paths, cut);
        } else {
            self.clipboard = Some(Clipboard { paths, cut });
        }
        let what = if n == 1 { "1 item".to_string() } else { format!("{n} items") };
        self.status = format!("{} {what}; Paste with Ctrl+V or the right-click menu", if cut { "Cut" } else { "Copied" });
    }

    /// Paste into the folder shown.
    pub fn paste(&mut self) {
        let cwd = self.tab().cwd.clone();
        self.paste_to(&cwd);
    }

    /// Paste into `dir`. Name clashes get " copy", " copy 2", …
    pub fn paste_to(&mut self, dir: &Path) {
        let (done, err) = if self.fs.is_local() {
            crate::fileops::paste_clip_into(dir)
        } else {
            let Some(cb) = self.clipboard.clone() else { return; };
            let fs: &dyn crate::fileops::FsOps = &self.fs;
            let clip = crate::fileops::FileClip { paths: cb.paths, cut: cb.cut };
            let r = crate::fileops::paste_into(fs, &clip, dir);
            if cb.cut { self.clipboard = None; }
            r
        };
        self.status = match err {
            Some(e) => format!("Paste failed: {e}"),
            None if done == 0 => String::new(),
            None => format!("Pasted {}", if done == 1 { "1 item".to_string() } else { format!("{done} items") }),
        };
        self.reload();
    }

    // ---- delete ------------------------------------------------------------

    /// A dialog without a text field is open (trash confirmation, error, info):
    /// Enter and clicks answer it instead of reaching the listing.
    pub fn dialog_open(&self) -> bool {
        matches!(self.overlay, Some(Overlay::ConfirmDelete { .. }) | Some(Overlay::Error { .. }) | Some(Overlay::GetInfo { .. }))
    }

    /// Enter on a dialog: trash for the confirmation, close for the others.
    pub fn dialog_commit(&mut self) {
        if matches!(self.overlay, Some(Overlay::ConfirmDelete { .. })) {
            self.confirm_delete();
        } else {
            self.overlay = None;
        }
    }

    /// A click while a dialog is open: the trash confirmation's buttons act,
    /// a click outside the box cancels; any click closes an error or info box.
    /// Always consumed (the listing underneath never sees it).
    pub fn dialog_click(&mut self, p: Point, w: i32, h: i32) -> bool {
        if let Some(Overlay::ConfirmDelete { count, empty }) = self.overlay {
            let (trash, cancel) = confirm_buttons(w, h, count, empty);
            if trash.contains(p) {
                self.confirm_delete();
            } else if cancel.contains(p) || !confirm_box(w, h, count, empty).contains(p) {
                self.overlay = None;
            }
        } else {
            self.overlay = None;
        }
        true
    }

    /// The trash confirmation's (Move to Trash, Cancel) buttons in content
    /// coordinates, when it is open.
    pub fn confirm_buttons(&self, w: i32, h: i32) -> Option<(Rect, Rect)> {
        match self.overlay {
            Some(Overlay::ConfirmDelete { count, empty }) => Some(confirm_buttons(w, h, count, empty)),
            _ => None,
        }
    }

    pub fn confirm_delete(&mut self) {
        let Some(Overlay::ConfirmDelete { empty, .. }) = self.overlay else { return; };
        self.overlay = None;
        if empty {
            self.status = match crate::fileops::empty_trash() {
                Ok(n) => format!("Trash emptied ({n} item{})", if n == 1 { "" } else { "s" }),
                Err(e) => format!("Empty Trash failed: {e}"),
            };
            self.reload();
            return;
        }
        for src in self.selected_paths() {
            if let Err(e) = self.fs.trash(&src) { self.status = format!("Trash failed: {e}"); }
        }
        self.reload();
    }
}

// ---- Dialog geometry: render and hit-test share these, so they can not drift.

const TRASH_BUTTON: &str = "[ Move to Trash ]";
const EMPTY_BUTTON: &str = "[ Empty Trash ]";
const CANCEL_BUTTON: &str = "[ Cancel ]";

/// A centred dialog box: the title row, a gap, then `lines`.
fn box_rect(w: i32, h: i32, title: &str, lines: &[String]) -> Rect {
    let widest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    let bw = (title.chars().count().max(widest) as i32 + 4).min(w - 2).max(4);
    let bh = lines.len() as i32 + 2;
    Rect::new((w - bw) / 2, (h - bh) / 2, bw, bh)
}

/// The action button of the confirmation: Move to Trash, or Empty Trash.
fn confirm_button(empty: bool) -> &'static str {
    if empty { EMPTY_BUTTON } else { TRASH_BUTTON }
}

/// The trash confirmation: the question, a blank row, the button row.
/// `empty`: emptying the Trash, which can not be undone.
fn confirm_text(count: usize, empty: bool) -> (&'static str, Vec<String>) {
    let what = if count == 1 { "1 item".to_string() } else { format!("{count} items") };
    let buttons = format!("{}  {CANCEL_BUTTON}", confirm_button(empty));
    if empty {
        ("Empty Trash", vec![format!("Delete the {what} in the Trash for good?"), "This can not be undone.".into(), String::new(), buttons])
    } else {
        ("Move to Trash", vec![format!("Move {what} to the Trash?"), String::new(), buttons])
    }
}

fn confirm_box(w: i32, h: i32, count: usize, empty: bool) -> Rect {
    let (title, lines) = confirm_text(count, empty);
    box_rect(w, h, title, &lines)
}

/// The (action, Cancel) buttons on the confirmation's last line.
fn confirm_buttons(w: i32, h: i32, count: usize, empty: bool) -> (Rect, Rect) {
    let (title, lines) = confirm_text(count, empty);
    let b = box_rect(w, h, title, &lines);
    let y = b.y + 2 + lines.len() as i32 - 1;
    let tw = confirm_button(empty).chars().count() as i32;
    let trash = Rect::new(b.x + 2, y, tw, 1);
    let cancel = Rect::new(b.x + 2 + tw + 2, y, CANCEL_BUTTON.chars().count() as i32, 1);
    (trash, cancel)
}

fn read_head(path: &std::path::Path, max: usize) -> Vec<String> {
    use std::io::Read;
    // Read only the head: a multi-gigabyte log must not be loaded to show a page.
    let mut head = Vec::new();
    let ok = std::fs::File::open(path).and_then(|f| f.take(PREVIEW_BYTES).read_to_end(&mut head)).is_ok();
    match ok.then(|| String::from_utf8_lossy(&head).into_owned()).filter(|s| !s.contains('\0')) {
        Some(s) => s.lines().take(max).map(|l| l.chars().take(200).collect()).collect(),
        None => vec!["(binary or unreadable)".into()],
    }
}

/// Bytes of a file the preview pane reads.
const PREVIEW_BYTES: u64 = 64 * 1024;

/// Text of the first page of a PDF, for the preview pane. Capped like every
/// other spawn: a broken or huge PDF must not stall the desktop.
fn pdf_preview(path: &std::path::Path, max: usize) -> Vec<String> {
    let file = path.to_string_lossy();
    for tool in ["pdftotext", "mutool"] {
        if !crate::catalog::is_installed(tool) {
            continue;
        }
        let args: Vec<&str> =
            if tool == "pdftotext" { vec!["-l", "1", &file, "-"] } else { vec!["draw", "-F", "txt", &file, "1"] };
        match crate::system::run_capped(tool, &args, 3) {
            Some(text) => {
                let lines: Vec<String> = text.lines().take(max).map(|l| l.chars().take(200).collect()).collect();
                if !lines.is_empty() {
                    return lines;
                }
            }
            None => crate::log_warn(&format!("preview: {tool} failed or timed out on {file}")),
        }
    }
    vec!["PDF (install pdftotext or mutool for a text preview)".into()]
}

use crate::buffer::CellBuffer;
use crate::cell::{Cell, Rgba};

// File-manager colors follow the active desktop theme (were hardcoded midnight,
// which left the Files app dark under every theme). Named `pal_*` so they do not
// collide with the many local `bg`/`fg` bindings in the render code.
fn pal_bg() -> Rgba { crate::theme::current().window_bg }
fn pal_fg() -> Rgba { crate::theme::current().text }
fn pal_dim() -> Rgba { crate::theme::current().dim }
fn pal_sel() -> Rgba { crate::theme::current().active_bg }
fn pal_accent() -> Rgba { crate::theme::current().accent }

pub const SIDEBAR_W: i32 = 16; // left shortcuts column
const TOOLBAR_Y: i32 = 0;  // breadcrumb/toolbar row
const LIST_TOP: i32 = 2;   // first entry row (below toolbar + spacer)
/// Icon-grid tile size — mirrors the desktop's icon tile
/// (`desktop::ICON_W`/`ICON_H`) so the FM's Icon view and the desktop icons
/// scale and look the same: the top `TILE_H - 1` rows hold the (image) icon,
/// the last row holds the centered label. Kept in lockstep with the desktop
/// constants deliberately (not re-exported from there) since the FM grid is
/// laid out row-major by entry index, unlike the desktop's draggable cells.
pub(crate) const TILE_W: i32 = 14;
pub(crate) const TILE_H: i32 = 6;

/// Tiles per row for an Icon-view grid `area_w` cells wide.
fn icon_cols(area_w: i32) -> i32 {
    (area_w / TILE_W).max(1)
}

/// How many full tile rows fit between `top` and the bottom status row, for a
/// content area `h` cells tall. Entries beyond this must not be
/// drawn/hit-tested/placed: there is no Icon-view scroll offset (yet) to page
/// past them, and a partially clipped tile would look broken.
fn icon_visible_rows(top: i32, h: i32) -> i32 {
    ((h - 1 - top) / TILE_H).max(0)
}

/// The Icon-view tile rect for entry index `i` in a `cols`-wide grid whose
/// origin is `(area_x, top)`. Shared by render/hit_test/thumbnail_placements
/// so they can never drift (the repo's render-and-hit-test-share-geometry
/// convention).
fn icon_tile_rect(area_x: i32, top: i32, cols: i32, i: usize) -> crate::geometry::Rect {
    let i = i as i32;
    let col = i % cols;
    let row = i / cols;
    crate::geometry::Rect::new(area_x + col * TILE_W, top + row * TILE_H, TILE_W, TILE_H)
}

/// The icon-image sub-rect within a tile — centered horizontally, the top
/// `TILE_H - 1` rows (the last row is the label). Mirrors
/// `desktop::icon_image_rect`.
fn icon_image_rect(tile: crate::geometry::Rect) -> crate::geometry::Rect {
    let iw = (TILE_W - 2).max(2);
    let ih = (TILE_H - 1).max(1);
    crate::geometry::Rect::new(tile.x + (TILE_W - iw) / 2, tile.y, iw, ih)
}

/// A click target inside the file-manager content area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Entry(usize),
    Sidebar(usize),
    Back,
    Forward,
    Up,
    ToggleView,
    Crumb(usize),
}

impl<F: FsOps> FileManager<F> {
    /// Point the sidebar's Code entry at `dir` (WMAST-005).
    pub fn set_code_dir(&mut self, dir: PathBuf) {
        self.code_dir = Some(dir);
    }

    /// Sidebar shortcut destinations (label, path), home-relative.
    fn sidebar(&self) -> Vec<(String, PathBuf)> {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
        [
            ("Home", home.clone()),
            ("Code", self.code_dir.clone().unwrap_or_else(|| home.join("code"))),
            ("Desktop", home.join("Desktop")),
            ("Documents", home.join("Documents")),
            ("Downloads", home.join("Downloads")),
            ("Pictures", home.join("Pictures")),
        ]
        .iter()
        .map(|(l, p)| (l.to_string(), p.clone()))
        .collect()
    }

    /// The file-type glyph for the list view and the no-graphics fallback, in
    /// the configured icon style (ascii / nerd / emoji -- see `iconset`).
    fn glyph(entry: &Entry) -> char {
        crate::iconset::glyph(entry.role)
    }

    /// First content row, shifted down by one when the tab strip is shown.
    /// The first row of the listing (below toolbar, crumbs and headers).
    pub fn listing_top(&self) -> i32 {
        self.content_top()
    }

    fn content_top(&self) -> i32 {
        if self.tabs.len() > 1 { LIST_TOP + 1 } else { LIST_TOP }
    }

    pub fn render(&self, w: i32, h: i32) -> CellBuffer {
        let t = self.tab();
        let top = self.content_top();
        let mut buf = CellBuffer::new(w, h);
        buf.fill(Cell { ch: ' ', fg: pal_fg(), bg: pal_bg(), attrs: Default::default() });

        // Toolbar: back/forward/up + breadcrumb + view toggle.
        buf.write_str(0, TOOLBAR_Y, "\u{25C2} \u{25B8} \u{25B2}", pal_accent(), pal_bg()); // ◂ ▸ ▲
        let crumb = t.cwd.to_string_lossy().to_string();
        buf.write_str(8, TOOLBAR_Y, &crumb, pal_fg(), pal_bg());
        let toggle = match t.view {
            ViewMode::Icon => "[grid]",
            ViewMode::List => "[list]",
            ViewMode::Columns => "[cols]",
        };
        buf.write_str((w - toggle.len() as i32 - 1).max(0), TOOLBAR_Y, toggle, pal_dim(), pal_bg());

        // Tab strip (only with more than one tab) on the row below the toolbar.
        if self.tabs.len() > 1 {
            let mut x = 0;
            for (i, tb) in self.tabs.iter().enumerate() {
                let name = tb
                    .cwd
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "/".to_string());
                let label = format!(" {name} ");
                let active = i == self.active;
                let (fg, bg) = if active { (pal_accent(), pal_sel()) } else { (pal_dim(), pal_bg()) };
                buf.write_str(x, TOOLBAR_Y + 1, &label, fg, bg);
                x += label.chars().count() as i32 + 1;
                if x >= w { break; }
            }
        }

        // Sidebar.
        for (i, (label, _)) in self.sidebar().iter().enumerate() {
            buf.write_str(0, top + i as i32, label, pal_dim(), pal_bg());
        }

        let area_x = SIDEBAR_W;
        let preview_w = self.preview_reserve(w);
        let area_right = (w - preview_w).max(area_x + 1);
        let area_w = (area_right - SIDEBAR_W).max(1);

        // Cache the view's visible entry-row count so navigation (`move_cursor`)
        // can keep the cursor on screen via the `scroll` offset.
        let list_rows = (h - 1 - top).max(1);
        self.vis_rows.set(match t.view {
            ViewMode::Icon => icon_visible_rows(top, h).max(1),
            _ => list_rows,
        });

        match t.view {
            ViewMode::List => {
                let off = self.scroll_rows();
                for (i, e) in t.entries.iter().enumerate() {
                    let row = i as i32 - off;
                    if row < 0 { continue; } // scrolled above the viewport
                    let y = top + row;
                    if y >= h - 1 { break; }
                    let selected = t.selection.contains(&i);
                    let focused = i == t.cursor;
                    let bg = if selected || focused { pal_sel() } else { pal_bg() };
                    for x in area_x..area_right { buf.set(x, y, Cell { ch: ' ', fg: pal_fg(), bg, attrs: Default::default() }); }
                    let mark = if e.is_dir { crate::iconset::glyph(crate::openwith::Role::Directory) } else { Self::glyph(e) };
                    buf.write_str(area_x, y, &format!("{mark} {}", e.name), if focused { pal_accent() } else { pal_fg() }, bg);
                }
            }
            ViewMode::Columns => {
                self.render_columns(&mut buf, area_x, area_right, h);
            }
            ViewMode::Icon => {
                let cols = icon_cols(area_w);
                self.cols_per_row.set(cols);
                let visible_rows = icon_visible_rows(top, h);
                let off = self.scroll_rows();
                // Shift the grid origin up by `off` tile-rows so scrolled-in rows
                // land in the viewport; `icon_tile_rect` then places every tile
                // relative to this origin (render/hit_test/thumbnails share it).
                let eff_top = top - off * TILE_H;
                for (i, e) in t.entries.iter().enumerate() {
                    let row = i as i32 / cols - off;
                    if row < 0 { continue; } // scrolled above the viewport
                    if row >= visible_rows { break; } // below the viewport: don't draw a clipped tile
                    let tile = icon_tile_rect(area_x, eff_top, cols, i);
                    let ir = icon_image_rect(tile);
                    let selected = t.selection.contains(&i);
                    let focused = i == t.cursor;
                    // Big centered glyph — the text-glyph fallback for
                    // non-graphics terminals; a loaded thumbnail/role-icon
                    // image covers the same rect on Kitty-graphics terminals
                    // (see `thumbnail_placements`).
                    // Sit the glyph at the bottom of the icon rect, just above the
                    // label, so the single-char fallback isn't stranded high in a
                    // tile sized for a large image. Still inside the rect, so a
                    // loaded thumbnail/role-icon image covers it on graphics terminals.
                    buf.set(
                        ir.x + ir.w / 2,
                        ir.y + ir.h - 1,
                        Cell { ch: Self::glyph(e), fg: pal_fg(), bg: pal_bg(), attrs: Default::default() },
                    );
                    // Label: centered on the tile's last row. Selection/focus
                    // highlight lives here only, like the desktop.
                    let label_bg = if selected || focused { pal_sel() } else { pal_bg() };
                    let label_y = tile.y + TILE_H - 1;
                    let name: String = e.name.chars().take(TILE_W as usize).collect();
                    let len = name.chars().count() as i32;
                    let lx = tile.x + (TILE_W - len).max(0) / 2;
                    for x in tile.x..(tile.x + TILE_W).min(area_right) {
                        buf.set(x, label_y, Cell { ch: ' ', fg: pal_fg(), bg: label_bg, attrs: Default::default() });
                    }
                    buf.write_str(lx, label_y, &name, if focused { pal_accent() } else { pal_fg() }, label_bg);
                }
            }
        }

        // Preview pane (Icon / List): a vertical separator then the entry's body.
        if preview_w > 0 && t.view != ViewMode::Columns {
            let sep_x = area_right;
            for y in top..h - 1 {
                buf.set(sep_x, y, Cell { ch: '\u{2502}', fg: pal_dim(), bg: pal_bg(), attrs: Default::default() });
            }
            let pane_x = sep_x + 2;
            let max = (h - top - 1).max(0) as usize;
            for (i, line) in self.preview_lines(max).iter().enumerate() {
                let y = top + i as i32;
                if y >= h - 1 { break; }
                let text: String = line.chars().take((preview_w - 2).max(1) as usize).collect();
                buf.write_str(pane_x, y, &text, pal_fg(), pal_bg());
            }
        }

        // Status line.
        if !self.status.is_empty() {
            buf.write_str(0, h - 1, &self.status, pal_accent(), pal_bg());
        } else {
            let info = format!("{} items", t.entries.len());
            buf.write_str(0, h - 1, &info, pal_dim(), pal_bg());
        }

        // Overlays render on top (Task 7 adds NewFolder/Rename/Confirm/Context/OpenWith/Error).
        self.render_overlay(&mut buf, w, h);
        buf
    }

    /// Render the Miller-columns layout: parent | current | preview-of-focused.
    fn render_columns(&self, buf: &mut CellBuffer, area_x: i32, area_right: i32, h: i32) {
        let t = self.tab();
        let top = self.content_top();
        let area_w = (area_right - area_x).max(1);
        let col_w = (area_w / 3).max(1);
        let mid_x = area_x + col_w;
        let right_x = area_x + 2 * col_w;

        // Left: parent directory listing, the current folder highlighted.
        if let Some(parent) = t.cwd.parent() {
            if let Ok(es) = self.fs.list(parent, t.show_hidden) {
                for (i, e) in es.iter().enumerate() {
                    let y = top + i as i32;
                    if y >= h - 1 { break; }
                    let here = e.path == t.cwd;
                    let bg = if here { pal_sel() } else { pal_bg() };
                    for x in area_x..mid_x { buf.set(x, y, Cell { ch: ' ', fg: pal_fg(), bg, attrs: Default::default() }); }
                    let name: String = e.name.chars().take((col_w - 1).max(1) as usize).collect();
                    buf.write_str(area_x, y, &name, if here { pal_accent() } else { pal_dim() }, bg);
                }
            }
        }

        // Middle: current entries, cursor highlighted.
        let off = self.scroll_rows();
        for (i, e) in t.entries.iter().enumerate() {
            let row = i as i32 - off;
            if row < 0 { continue; } // scrolled above the viewport
            let y = top + row;
            if y >= h - 1 { break; }
            let selected = t.selection.contains(&i);
            let focused = i == t.cursor;
            let bg = if selected || focused { pal_sel() } else { pal_bg() };
            for x in mid_x..right_x { buf.set(x, y, Cell { ch: ' ', fg: pal_fg(), bg, attrs: Default::default() }); }
            let mark = if e.is_dir { crate::iconset::glyph(crate::openwith::Role::Directory) } else { Self::glyph(e) };
            let label = format!("{mark} {}", e.name);
            let label: String = label.chars().take((col_w - 1).max(1) as usize).collect();
            buf.write_str(mid_x, y, &label, if focused { pal_accent() } else { pal_fg() }, bg);
        }

        // Right: preview of the focused entry.
        for x in right_x - 1..right_x { buf.set(x, top, Cell { ch: ' ', fg: pal_fg(), bg: pal_bg(), attrs: Default::default() }); }
        let max = (h - top - 1).max(0) as usize;
        for (i, line) in self.preview_lines(max).iter().enumerate() {
            let y = top + i as i32;
            if y >= h - 1 { break; }
            let text: String = line.chars().take((area_right - right_x).max(1) as usize).collect();
            buf.write_str(right_x, y, &text, pal_fg(), pal_bg());
        }
    }

    /// Map a content-local click to a target. Mirrors the render layout.
    pub fn hit_test(&self, p: Point, w: i32, h: i32) -> Option<Target> {
        let top = self.content_top();
        let t = self.tab();
        if p.y == TOOLBAR_Y {
            return match p.x {
                0 => Some(Target::Back),
                2 => Some(Target::Forward),
                4 => Some(Target::Up),
                x if x >= w - 7 => Some(Target::ToggleView),
                _ => None,
            };
        }
        if p.x < SIDEBAR_W && p.y >= top {
            let i = (p.y - top) as usize;
            if i < self.sidebar().len() { return Some(Target::Sidebar(i)); }
            return None;
        }
        // Entry area.
        let area_x = SIDEBAR_W;
        let area_right = (w - self.preview_reserve(w)).max(area_x + 1);
        let area_w = (area_right - SIDEBAR_W).max(1);
        // Scrolled rows shift which entry sits under a given screen row; add the
        // offset so clicks hit the same entry the render drew (shared geometry).
        let off = self.scroll_rows();
        match t.view {
            ViewMode::List => {
                let i = (p.y - top + off) as usize;
                if p.y >= top && i < t.entries.len() { Some(Target::Entry(i)) } else { None }
            }
            ViewMode::Columns => {
                // Only the middle column maps to entries; left/right are visual.
                let col_w = (area_w / 3).max(1);
                let mid_x = area_x + col_w;
                if p.y < top || p.x < mid_x || p.x >= mid_x + col_w {
                    return None;
                }
                let i = (p.y - top + off) as usize;
                if i < t.entries.len() { Some(Target::Entry(i)) } else { None }
            }
            ViewMode::Icon => {
                let cols = icon_cols(area_w);
                if p.y < top { return None; }
                let visible_rows = icon_visible_rows(top, h);
                let col = (p.x - area_x) / TILE_W;
                let row = (p.y - top) / TILE_H;
                if col < 0 || col >= cols || row < 0 || row >= visible_rows { return None; }
                let i = ((row + off) * cols + col) as usize;
                if i < t.entries.len() { Some(Target::Entry(i)) } else { None }
            }
        }
    }

    /// Handle a content-local left click. `ctrl`/`shift` modify selection; a click
    /// on a toolbar/sidebar target navigates. Returns true if anything changed.
    pub fn handle_click(&mut self, p: Point, w: i32, h: i32, ctrl: bool, shift: bool) -> bool {
        if self.dialog_open() {
            return self.dialog_click(p, w, h);
        }
        match self.hit_test(p, w, h) {
            Some(Target::Entry(i)) => { self.select_at(i, ctrl, shift); true }
            Some(Target::Back) => { self.go_back(); true }
            Some(Target::Forward) => { self.go_forward(); true }
            Some(Target::Up) => { self.go_parent(); true }
            Some(Target::ToggleView) => { self.cycle_view(); true }
            Some(Target::Sidebar(i)) => {
                if let Some((_, path)) = self.sidebar().get(i) {
                    if path.is_dir() { self.navigate_to(path.clone()); }
                }
                true
            }
            Some(Target::Crumb(_)) | None => false,
        }
    }

    /// Handle a content-local double click: on an entry, select then open it
    /// (navigate into a folder / open a file via the default app); elsewhere fall
    /// back to single-click behaviour (toolbar / sidebar nav). Returns true if the
    /// caller should drain a resulting open action.
    pub fn double_click(&mut self, p: Point, w: i32, h: i32) -> bool {
        if self.dialog_open() {
            return self.dialog_click(p, w, h);
        }
        match self.hit_test(p, w, h) {
            Some(Target::Entry(i)) => {
                self.select_at(i, false, false);
                self.activate();
                true
            }
            _ => self.handle_click(p, w, h, false, false),
        }
    }

    fn render_overlay(&self, buf: &mut CellBuffer, w: i32, h: i32) {
        let Some(ov) = &self.overlay else { return; };
        match ov {
            Overlay::NewFolder { name } => {
                self.draw_box(buf, w, h, "New folder", &[format!("Name: {name}\u{2588}")]);
            }
            Overlay::Rename { name, .. } => {
                self.draw_box(buf, w, h, "Rename", &[format!("Name: {name}\u{2588}")]);
            }
            Overlay::ConfirmDelete { count, empty } => {
                let (title, lines) = confirm_text(*count, *empty);
                self.draw_box(buf, w, h, title, &lines);
                let (trash, cancel) = confirm_buttons(w, h, *count, *empty);
                buf.write_str(trash.x, trash.y, confirm_button(*empty), pal_bg(), pal_accent());
                buf.write_str(cancel.x, cancel.y, CANCEL_BUTTON, pal_fg(), pal_bg());
            }
            Overlay::Context { menu, .. } => {
                menu.render(buf, crate::geometry::Rect::new(0, 0, w, h));
            }
            Overlay::Error { message } => {
                self.draw_box(buf, w, h, "Error", std::slice::from_ref(message));
            }
            Overlay::GetInfo { idx } => {
                let Some(e) = self.tab().entries.get(*idx) else { return; };
                let mut lines = vec![format!("Name: {}", e.name)];
                if let Ok(info) = crate::fileops::info(&e.path) {
                    lines.push(format!("Path: {}", info.path.display()));
                    lines.push(format!("Size: {} bytes", info.size));
                    let kind = if info.is_dir { "Folder" } else { e.role.label() };
                    lines.push(format!("Kind: {kind}"));
                    lines.push(format!(
                        "Permissions: {} ({:o})",
                        crate::fileops::mode_rwx(info.mode),
                        info.mode & 0o777
                    ));
                    if let Some(t) = &info.link_target {
                        lines.push(format!("Symlink \u{2192} {}", t.display()));
                    }
                }
                lines.push("[Esc] close".into());
                self.draw_box(buf, w, h, "Get Info", &lines);
            }
        }
    }

    /// Draw a centered modal box `lines.len()+2` rows tall with a title and body.
    fn draw_box(&self, buf: &mut CellBuffer, w: i32, h: i32, title: &str, lines: &[String]) {
        let Rect { x: bx, y: by, w: bw, h: bh } = box_rect(w, h, title, lines);
        for y in by..by + bh {
            for x in bx..bx + bw {
                buf.set(x, y, Cell { ch: ' ', fg: pal_fg(), bg: pal_sel(), attrs: Default::default() });
            }
        }
        buf.write_str(bx + 2, by, title, pal_accent(), pal_sel());
        for (i, line) in lines.iter().enumerate() {
            buf.write_str(bx + 2, by + 2 + i as i32, line, pal_fg(), pal_sel());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;

    fn tmp(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("tetron-wm-fmu-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn sidebar_lists_code_after_home_at_the_code_dir() {
        let d = tmp("code");
        let mut fm = FileManager::new(d.clone(), BTreeMap::new());
        let home = dirs::home_dir().unwrap();
        assert_eq!(fm.sidebar()[1], ("Code".to_string(), home.join("code")));
        fm.set_code_dir(d.join("src"));
        assert_eq!(fm.sidebar()[1], ("Code".to_string(), d.join("src")));
        assert_eq!(fm.sidebar()[0].0, "Home");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn read_head_reads_only_the_head_and_flags_binary() {
        let d = tmp("head");
        let big = d.join("big.log");
        fs::write(&big, "line\n".repeat(200_000)).unwrap(); // 1 MB
        let lines = read_head(&big, 5);
        assert_eq!(lines, vec!["line"; 5]);
        fs::write(d.join("bin"), [0u8, 1, 2, 0]).unwrap();
        assert_eq!(read_head(&d.join("bin"), 5), vec!["(binary or unreadable)"]);
        assert_eq!(read_head(&d.join("missing"), 5), vec!["(binary or unreadable)"]);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn preview_is_cached_per_file_and_size() {
        let d = tmp("pcache");
        fs::write(d.join("a.txt"), "one\n").unwrap();
        let fm = FileManager::new(d.clone(), BTreeMap::new());
        assert_eq!(fm.preview_lines(5), vec!["one"]);
        // Same listing (size and mtime): served from the cache, the file is not read again.
        fs::write(d.join("a.txt"), "two\n").unwrap();
        assert_eq!(fm.preview_lines(5), vec!["one"]);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn render_returns_sized_buffer() {
        let d = tmp("render");
        fs::write(d.join("a.txt"), b"x").unwrap();
        let fm = FileManager::new(d.clone(), BTreeMap::new());
        let buf = fm.render(80, 24);
        assert_eq!(buf.width(), 80);
        assert_eq!(buf.height(), 24);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn view_toggle_switches_modes() {
        let d = tmp("toggle");
        let mut fm = FileManager::new(d.clone(), BTreeMap::new());
        assert_eq!(fm.view(), ViewMode::List); // List is the default
        fm.cycle_view();
        assert_eq!(fm.view(), ViewMode::Columns);
        fm.cycle_view();
        assert_eq!(fm.view(), ViewMode::Icon);
        fm.cycle_view();
        assert_eq!(fm.view(), ViewMode::List);
        fm.set_view(ViewMode::Icon);
        assert_eq!(fm.view(), ViewMode::Icon);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn click_on_entry_selects_it() {
        let d = tmp("click");
        for n in ["a", "b", "c"] { fs::write(d.join(n), b"").unwrap(); }
        let mut fm = FileManager::new(d.clone(), BTreeMap::new());
        fm.set_view(ViewMode::List); // deterministic 1-per-row layout
        let _ = fm.render(80, 24);   // establish layout rects
        // List rows start at LIST_TOP; second row → index 1. The click x must be
        // inside the entry area (x >= SIDEBAR_W); x < SIDEBAR_W hits the sidebar.
        let target = crate::geometry::Point::new(SIDEBAR_W + 2, LIST_TOP + 1);
        let hit = fm.hit_test(target, 80, 24);
        assert_eq!(hit, Some(Target::Entry(1)));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn double_click_folder_navigates_into_it() {
        let d = tmp("dclick");
        fs::create_dir(d.join("sub")).unwrap();
        fs::write(d.join("sub/inner.txt"), b"x").unwrap();
        let mut fm = FileManager::new(d.clone(), BTreeMap::new());
        fm.set_view(ViewMode::List); // 1-per-row; "sub" is the first (dirs-first) entry
        let _ = fm.render(80, 24);
        let entry0 = crate::geometry::Point::new(SIDEBAR_W + 2, LIST_TOP);
        assert!(fm.double_click(entry0, 80, 24));
        assert_eq!(fm.cwd(), d.join("sub"));
        let _ = fs::remove_dir_all(&d);
    }

    /// hit_test must return exactly the entry whose tile `icon_tile_rect`
    /// (the geometry the Icon-view render uses) places under the click — the
    /// repo's "render and hit-test share the same rect math" convention,
    /// verified against the *enlarged* (desktop-proportioned) tile now that
    /// TILE_H > 1.
    #[test]
    fn icon_hit_test_matches_the_tile_render_computes() {
        let d = tmp("tilegeo");
        for n in ["a", "b", "c", "d", "e"] { fs::write(d.join(n), b"").unwrap(); }
        let mut fm = FileManager::new(d.clone(), BTreeMap::new());
        fm.set_view(ViewMode::Icon);
        let (w, h) = (80, 24);
        let _ = fm.render(w, h); // populates cols_per_row from the real layout
        let cols = fm.cols_per_row.get().max(1);
        let top = fm.content_top();
        assert_eq!(fm.entries().len(), 5);
        for i in 0..fm.entries().len() {
            let tile = icon_tile_rect(SIDEBAR_W, top, cols, i);
            // Any point inside the tile — including its label row, the last
            // of the TILE_H rows — must hit this entry, not a neighbor.
            let p = crate::geometry::Point::new(tile.x + 1, tile.y + TILE_H - 1);
            assert_eq!(fm.hit_test(p, w, h), Some(Target::Entry(i)), "entry {i} tile {tile:?}");
        }
        let _ = fs::remove_dir_all(&d);
    }

    /// The old 1-row-tall thumbnail placement was the bug: with the enlarged
    /// tile, the placed rect must span the multi-row icon area (`TILE_H - 1`
    /// rows), sitting entirely inside the tile it belongs to.
    #[test]
    fn thumbnail_placement_spans_multiple_rows_within_its_tile() {
        let d = tmp("thumbrows");
        fs::write(d.join("pic.png"), b"\x89PNG\r\n\x1a\n").unwrap();
        let mut fm = FileManager::new(d.clone(), BTreeMap::new());
        fm.set_view(ViewMode::Icon); // thumbnails only place in Icon view (default is List)
        let idx = fm.thumbnail_requests()[0].0;
        fm.set_thumb(idx, 999);
        let content = crate::geometry::Rect::new(0, 0, 80, 24);
        let places = fm.thumbnail_placements(content, true);
        assert_eq!(places.len(), 1);
        let r = places[0].rect;
        assert_eq!(r.h, (TILE_H - 1).max(1), "icon area is TILE_H - 1 rows tall, not 1");
        assert!(r.h > 1, "must be multi-row, not the old 1-row blob");

        let top = fm.content_top();
        let area_w = (content.w - SIDEBAR_W).max(1); // preview closed by default
        let cols = icon_cols(area_w);
        let tile = icon_tile_rect(SIDEBAR_W, top, cols, idx);
        assert!(r.x >= tile.x && r.x + r.w <= tile.x + TILE_W, "placement must stay inside the tile horizontally");
        assert!(r.y >= tile.y && r.y + r.h <= tile.y + TILE_H, "placement must stay inside the tile vertically");
        let _ = fs::remove_dir_all(&d);
    }

    /// In a scrolled list the keyboard menu opens just below the cursor's
    /// visible row, not where the row would be unscrolled.
    #[test]
    fn begin_context_follows_the_list_scroll() {
        let d = tmp("ctxscroll");
        for i in 0..60 { fs::write(d.join(format!("f{i:02}")), b"").unwrap(); }
        let mut fm = FileManager::new(d.clone(), BTreeMap::new());
        fm.set_view(ViewMode::List);
        let _ = fm.render(80, 24);
        for _ in 0..40 { fm.move_cursor(0, 1); }
        let _ = fm.render(80, 24);
        let off = fm.scroll_rows();
        assert!(off > 0, "the list scrolled");
        fm.begin_context();
        match fm.overlay() {
            Some(Overlay::Context { menu, .. }) => {
                assert_eq!(menu.anchor.y, fm.content_top() + 40 - off + 1);
                assert!(menu.anchor.y < 24, "on screen");
            }
            other => panic!("expected a Context overlay, got {other:?}"),
        }
        let _ = fs::remove_dir_all(&d);
    }

    /// The keyboard-invoked context menu anchors at the focused entry's tile
    /// (Icon view), matching the same `icon_tile_rect` render/hit_test use —
    /// not some fixed/centered position.
    #[test]
    fn begin_context_anchors_at_the_focused_tile_in_icon_view() {
        let d = tmp("ctxkbd");
        for n in ["a", "b", "c"] { fs::write(d.join(n), b"").unwrap(); }
        let mut fm = FileManager::new(d.clone(), BTreeMap::new());
        fm.set_view(ViewMode::Icon);
        let _ = fm.render(80, 24); // establishes cols_per_row
        fm.move_cursor(1, 0); // cursor -> entry 1
        fm.begin_context();
        let cols = fm.cols_per_row.get().max(1);
        let top = fm.content_top();
        let expected = icon_tile_rect(SIDEBAR_W, top, cols, 1);
        match fm.overlay() {
            Some(Overlay::Context { idx, menu }) => {
                assert_eq!(*idx, 1);
                assert_eq!(menu.anchor.x, expected.x);
                assert_eq!(menu.anchor.y, expected.y);
            }
            other => panic!("expected a Context overlay, got {other:?}"),
        }
        let _ = fs::remove_dir_all(&d);
    }
}
