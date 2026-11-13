//! Session core — the `ClientMsg`-in / `Frame`-out boundary.
//!
//! [`SessionCore`] is the integration layer that owns:
//! - the [`WindowManager`] (window geometry, z-order, focus), and
//! - the [`LocalAppHost`] (PTY-backed child processes).
//!
//! All external control flows through [`ClientMsg`] variants; the core
//! produces a [`Frame`] (ordered compositor layers + cursor position) via
//! [`SessionCore::build_frame`].  No I/O, no terminal types, no renderer
//! details cross this boundary — it is the seam that a future daemon will
//! expose on a socket.

use crate::chrome::{
    render_menubar, menubar_new_shell_region, menubar_taskbar_regions, menubar_taskbar_overflow, menubar_assistant_region, menubar_brand_region, menubar_mode_region, DockItem, DockKind,
};
use crate::powermenu::{PowerClick, PowerMenu, PowerOutcome};
use crate::confirmclose::ConfirmClose;
use crate::launchwarn::LaunchWarn;
use crate::compositor::Layer;
use crate::config::{AppEntry, Config};
use crate::geometry::{Point, Rect, SnapZone};
use crate::input::{route_mouse, MouseKind, Hit, Action};

/// An in-progress or finished mouse text-selection inside an app (terminal)
/// window's content area, used for select + middle-click paste — the standard
/// X11 "primary selection" behaviour that Linux users expect in a terminal.
///
/// `anchor` and `head` are content-relative cell coordinates (column, row);
/// `cr` is the window's content rect captured at press time so a drag maps to
/// the same grid even if geometry shifts, and `aid` is the app whose grid the
/// text is read from. `dragging` is true only between press and release.
#[derive(Clone, Copy)]
struct TextSel {
    win: WindowId,
    aid: AppId,
    cr: Rect,
    anchor: (i32, i32),
    head: (i32, i32),
    dragging: bool,
}

/// Order the two endpoints of a selection top-to-bottom then left-to-right, so
/// extraction/highlighting can walk rows in a single forward direction
/// regardless of which way the user dragged.
fn ordered(a: (i32, i32), b: (i32, i32)) -> ((i32, i32), (i32, i32)) {
    // Compare by (row, col): a selection that starts lower on the screen, or on
    // the same row further right, is the "end".
    if (a.1, a.0) <= (b.1, b.0) { (a, b) } else { (b, a) }
}

/// Extract the selected text from a window's rendered grid as a linear
/// (xterm-style) selection: full intermediate rows, partial first/last rows,
/// trailing blanks trimmed per line, rows joined with newlines. Returns the
/// plain text to place in the primary buffer.
fn extract_text(buf: &crate::buffer::CellBuffer, a: (i32, i32), b: (i32, i32)) -> String {
    let (s, e) = ordered(a, b);
    let w = buf.width();
    let mut out = String::new();
    for row in s.1..=e.1 {
        // First row starts at the anchor column; last row ends at the head
        // column; middle rows span the full width.
        let c0 = if row == s.1 { s.0 } else { 0 };
        let c1 = if row == e.1 { e.0 } else { w - 1 };
        let mut line = String::new();
        for col in c0..=c1 {
            if let Some(cell) = buf.get(col, row) {
                line.push(cell.ch);
            }
        }
        // Trim trailing padding so selecting a short line does not grab the
        // blank cells that fill the rest of the terminal row.
        out.push_str(line.trim_end());
        if row != e.1 {
            out.push('\n');
        }
    }
    out
}

/// Paint the selection highlight onto a window's content buffer in place, so the
/// composited frame shows what is selected. Uses the theme's accent as the
/// selection background with the contrasting close-foreground, matching the
/// calendar "today" highlight.
fn highlight_selection(buf: &mut crate::buffer::CellBuffer, a: (i32, i32), b: (i32, i32)) {
    let t = crate::theme::current();
    let (s, e) = ordered(a, b);
    let w = buf.width();
    for row in s.1..=e.1 {
        let c0 = if row == s.1 { s.0 } else { 0 };
        let c1 = if row == e.1 { e.0 } else { w - 1 };
        for col in c0..=c1 {
            if let Some(cell) = buf.get(col, row) {
                let mut c = *cell;
                c.bg = t.accent;
                c.fg = t.close_fg;
                buf.set(col, row, c);
            }
        }
    }
}

/// Paint a text cursor at `(x, y)` by inverting the cell there (swap fg/bg),
/// so the character under the caret stays legible. A no-op if out of bounds.
fn draw_cursor_cell(buf: &mut crate::buffer::CellBuffer, x: i32, y: i32) {
    if let Some(cell) = buf.get(x, y) {
        let mut c = *cell;
        std::mem::swap(&mut c.fg, &mut c.bg);
        buf.set(x, y, c);
    }
}
use crate::apphost::{AppHost, AppId, LocalAppHost};
use crate::launcher::Launcher;
use crate::settings::Settings;
use crate::activity::Activity;
use crate::store::{self, Store};
use crate::window::WindowId;
use crate::wm::{WindowManager, render_window};
use std::collections::HashMap;

/// A running screensaver: its effect, when it started, when it last painted,
/// and the last painted frame (shown by `build_frame` until the next repaint).
struct Saver {
    effect: Box<dyn crate::screensaver::Effect>,
    since: std::time::Instant,
    last: Option<std::time::Instant>,
    buf: Option<crate::buffer::CellBuffer>,
}

/// Opaque per-app window state the frontend stashes in the apphost so a fresh
/// frontend (after `reload` or a crash) can rebuild the window in place.
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Debug)]
struct WinMeta {
    rect: crate::geometry::Rect,
    title: String,
    z: i32,
    minimized: bool,
    /// Immutable grouping key (the launch name, set at spawn time). If absent in
    /// older stored meta, the title is used as a fallback.
    #[serde(default)]
    app_key: String,
}

/// A file-manager window's backend identity: `None` browses the local disk,
/// `Some((ssh_target, port))` a saved remote system.
type FmBackend = Option<(String, Option<u16>)>;

/// The content hosted by a window: a PTY-backed app or a native Tetron-wm widget.
///
/// This is the [`WindowContent`](../../docs) seam — the window manager, chrome,
/// and input routing operate on windows uniformly, while the content type varies.
enum WinContent {
    /// A PTY-backed child process, identified by its `AppId` in the session's `LocalAppHost`.
    App(AppId),
    /// The native app store browser.
    Store(Store),
    /// The native settings panel (boxed: by far the largest variant, and there
    /// is at most one settings window).
    Settings(Box<Settings>),
    /// The built-in image viewer (Kitty graphics, chafa cells or half blocks).
    ImageView(Box<crate::imageview::ImageView>),
    /// The native file manager (local disk or a remote system over ssh).
    FileManager(crate::filemanager::DynFileManager),
    /// The native log viewer (tail of the log + clipboard copy).
    Logs(crate::logsview::LogsView),
    /// The activity monitor — a live table of hosted apps with kill-app controls.
    Activity(Activity),
    /// The keyboard-shortcut cheatsheet (a normal window: stays open as a reference).
    Help(crate::help::HelpView),
}

impl WinContent {
    fn render(&self, host: &dyn AppHost, w: i32, h: i32) -> crate::buffer::CellBuffer {
        match self {
            WinContent::App(id) => host.snapshot(*id).unwrap_or_else(|| crate::buffer::CellBuffer::new(w, h)),
            WinContent::Store(s) => s.render(w, h),
            WinContent::Settings(s) => s.render(w, h),
            WinContent::ImageView(v) => v.render(w, h),
            WinContent::FileManager(f) => f.render(w, h),
            WinContent::Logs(l) => l.render(w, h),
            WinContent::Activity(a) => a.render(w, h),
            WinContent::Help(v) => v.render(w, h),
        }
    }
}

// ── Public message type ───────────────────────────────────────────────────────

/// Keys for an open menu (the desktop right-click menu).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MenuKey {
    Up,
    Down,
    Enter,
    Esc,
}

/// All input the front-end (or a future daemon client) can send to the core.
///
/// This enum is intentionally minimal — exactly the surface needed for Slice 1.
/// Additional variants (e.g. scroll, touch, IPC commands) belong in later
/// slices once the socket transport is defined.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum ClientMsg {
    /// Spawn a new PTY-backed application window.
    Launch {
        /// Short human-readable name shown in the titlebar and dock.
        name: String,
        /// Executable to run (passed to the PTY host verbatim).
        command: String,
        /// Additional arguments for the child process.
        args: Vec<String>,
    },
    /// Left-button press at screen coordinates `p`.
    MouseDown(Point),
    /// Left-button drag (button still held) at screen coordinates `p`.
    MouseDrag(Point),
    /// Left-button release at screen coordinates `p`.
    MouseUp(Point),
    /// Pointer motion with NO button held (hover). Kept distinct from `MouseDrag`
    /// so a bare move can never *extend* a text selection, and in fact *ends* a
    /// selection drag if one is still active — self-healing a missed release so
    /// the selection can't get "stuck on" and grow as the mouse moves untouched.
    MouseMove(Point),
    /// Middle-button press at `p`: paste the primary (mouse-selected) text into
    /// the terminal app under the pointer (classic X11 primary-selection paste).
    MousePaste(Point),
    /// Shift+Insert: paste the primary (mouse) selection into the focused app —
    /// the keyboard equivalent of middle-click, for devices without a middle
    /// button (e.g. a buttonless clickpad).
    PastePrimary,
    /// Ctrl+Shift+C: copy the current selection to the clipboard (internal, for
    /// Ctrl+Shift+V) and push it to the host terminal clipboard via OSC 52.
    CopySelection,
    /// Ctrl+Shift+V: paste the clipboard into the focused app.
    PasteClipboard,
    /// Shift+PageUp / Shift+PageDown: scroll the focused app's scrollback by one
    /// page. Sign is direction: +1 = back into history, -1 = toward the live
    /// bottom. Device-independent scroll, since a touchpad under kmscon does not
    /// emit a wheel (two-finger scroll is a gesture kmscon does not translate).
    ScrollFocused(i32),
    /// Raw input bytes to forward to the focused app.
    Key(Vec<u8>),
    /// Terminal was resized to `w` × `h` cells. Also the first message a client
    /// sends on attach, where `takeover` asks the daemon to evict any client
    /// already attached (`attach -d`) instead of being refused.
    Resize {
        w: i32,
        h: i32,
        #[serde(default)]
        takeover: bool,
    },
    /// Toggle maximize / restore on the focused window (keyboard command).
    MaximizeFocused,
    /// Minimize the focused window to the dock (keyboard command).
    MinimizeFocused,
    /// Snap the focused window to a screen half (keyboard command).
    SnapFocused(SnapZone),
    /// Tile all windows into the configured grid (one-shot).
    TileAll,
    /// Save a PNG of the next composited frame to this absolute path (daemon-side).
    Screenshot(String),
    /// Toggle auto-tile mode.
    ToggleAutoTile,
    /// Send the focused window to grid cell N (1-based, row-major).
    SendToCell(u8),
    /// Open/close the launcher dropdown menu (keyboard command).
    ToggleMenu,
    /// Open/close the Spotlight search overlay (keyboard command).
    ToggleSpotlight,
    /// Type a character into the Spotlight query.
    LauncherChar(char),
    /// Delete the last character of the Spotlight query.
    LauncherBackspace,
    /// Move the launcher highlight up.
    LauncherUp,
    /// Move the launcher highlight down.
    LauncherDown,
    /// Collapse the cascade one level (Menu mode, ←).
    LauncherLeft,
    /// Descend into the focused submenu (Menu mode, →).
    LauncherRight,
    /// Launch the highlighted entry (Enter).
    LauncherEnter,
    /// Dismiss the launcher (Escape).
    LauncherEsc,
    /// Open the store window (or focus it if already open).
    OpenStore,
    /// Store: move selection up / down.
    StoreUp,
    StoreDown,
    /// Store: previous / next category.
    StorePrevCategory,
    StoreNextCategory,
    /// Store: type into / edit the search query.
    StoreChar(char),
    StoreBackspace,
    /// Store: install or launch the selected app (Enter).
    StoreActivate,
    /// Store: close the store window (Escape).
    StoreClose,
    /// Store: Update list (F5, or the row 0 button), WMCAT-004.
    StoreUpdateList,
    /// Open the settings window (or focus it if already open).
    OpenSettings,
    /// Settings: move selection up / down.
    SettingsUp,
    SettingsDown,
    /// Settings: previous / next section.
    SettingsPrevSection,
    SettingsNextSection,
    /// Settings: move keyboard focus between the section list and the rows (Tab).
    SettingsTogglePane,
    /// Settings: decrease / increase / toggle the selected setting.
    SettingsLeft,
    SettingsRight,
    SettingsToggle,
    /// Settings (Apps add form): type a character into the focused field.
    SettingsChar(char),
    /// Settings (Apps add form): delete the last character of the focused field.
    SettingsBackspace,
    /// Settings (Apps add form): abandon the form without saving (Escape).
    SettingsCancelEdit,
    /// Settings: close the settings window (Escape).
    SettingsClose,
    /// Open the file-manager window (or focus it if already open).
    OpenFileManager,
    /// File manager: move the cursor.
    FileManagerUp,
    FileManagerDown,
    FileManagerLeft,
    FileManagerRight,
    /// File manager: enter the focused entry (navigate / open).
    FileManagerActivate,
    /// File manager: history back.
    FileManagerBack,
    /// File manager: navigate to the parent directory.
    FileManagerParent,
    /// File manager: cycle Icon / List / Columns view.
    FileManagerToggleView,
    /// File manager: set the view directly.
    FileManagerViewIcon,
    FileManagerViewList,
    FileManagerViewColumns,
    /// File manager: toggle the preview pane.
    FileManagerTogglePreview,
    /// File manager: toggle hidden (dot) entries.
    FileManagerToggleHidden,
    /// File manager: begin the new-folder overlay.
    FileManagerNewFolder,
    /// File manager: begin the rename overlay.
    FileManagerRename,
    /// File manager: begin the delete confirmation.
    FileManagerDelete,
    /// File manager: copy / cut the selection to the clipboard.
    FileManagerCopy,
    FileManagerCut,
    /// File manager: paste the clipboard into the current directory.
    FileManagerPaste,
    /// File manager: type / delete a character in an overlay text field.
    FileManagerChar(char),
    FileManagerBackspace,
    /// File manager: commit the active overlay (Enter).
    FileManagerCommit,
    /// File manager: cancel the active overlay (Escape).
    FileManagerCancel,
    /// File manager: close the window (Escape with no overlay).
    FileManagerClose,
    /// File manager: open a new tab on the current folder.
    FileManagerNewTab,
    /// File manager: close the active tab.
    FileManagerCloseTab,
    /// File manager: focus the next tab.
    FileManagerNextTab,
    /// Open the activity-monitor window (or focus it if already open).
    OpenActivity,
    /// The ✦ Assistant: open it, or show / hide it like the menubar button.
    ToggleAssistant,
    /// Activity monitor: move selection up / down.
    ActivityUp,
    ActivityDown,
    /// Activity monitor: request to kill the selected app (Enter / `k`).
    /// Returns the requested kill via the in-app confirm overlay when the app
    /// is alive, or kills immediately when it's already dead.
    ActivityKill,
    /// Activity monitor: confirm a pending kill (`Enter` / `y` from confirm).
    ActivityConfirmKill,
    /// Activity monitor: cancel a pending kill (`Esc` / `n` from confirm).
    ActivityCancelKill,
    /// Activity monitor: kill every row currently in the `dead` state.
    ActivityKillDead,
    /// Activity monitor: force a refresh of the row list (`r`).
    ActivityRefresh,
    /// Activity monitor: close the panel (`Esc` when no kill is pending).
    ActivityClose,
    /// A clipboard request (`tetron-wm clip`, the xclip/xsel shims). Served by
    /// the daemon's control thread, which writes the reply; applying it here is
    /// the in-process equivalent (the reply is dropped).
    Clip(crate::clipboard::ClipCmd),
    /// Open or close the clipboard history popover.
    ClipMenuToggle,
    /// Toggle one-shot select mode (mouse drags select text instead of
    /// reaching the app).
    ToggleSelectMode,
    /// A key for the open clipboard history popover.
    ClipMenuKey(crate::clipmenu::ClipKey),
    /// Toggle the keyboard-shortcut help overlay.
    ToggleHelp,
    /// Scroll the help overlay by this many rows (negative = up; `i32::MIN`/`MAX` = top/bottom).
    HelpScroll(i32),
    /// Open an image file in a native image-viewer window.
    OpenImage(String),
    /// Working-directory picker: navigation, expand/collapse, confirm, cancel.
    DirPickerUp,
    DirPickerDown,
    DirPickerExpand,
    DirPickerCollapse,
    DirPickerConfirm,
    DirPickerCancel,
    DirPickerToggleHidden,
    /// Picker: start the new-folder name input.
    DirPickerNewFolder,
    /// Picker: type / delete a character of the new-folder name.
    DirPickerChar(char),
    DirPickerBackspace,
    /// Left-button double-click at screen coordinates `p` (desktop: open icon).
    MouseDouble(Point),
    /// Right-button press at screen coordinates `p` (desktop: context menu).
    MouseRightDown(Point),
    /// Desktop overlay (rename / new-folder): type a character.
    DesktopChar(char),
    /// Desktop overlay: delete the last character.
    DesktopBackspace,
    /// Desktop overlay: commit (Enter).
    DesktopCommit,
    /// Desktop overlay: cancel (Escape).
    DesktopCancel,
    /// Focus and raise the next window in taskbar order, wrapping (leader Tab).
    /// Cycles visible windows; when every window is minimized it restores them.
    FocusNext,
    /// Focus and raise the previous window in taskbar order (leader Shift+Tab).
    FocusPrev,
    /// Open the window picker, a numbered list of every window (leader f).
    WindowPickerOpen,
    /// Window picker: move the highlight by this many rows (wraps).
    WindowPickerStep(i32),
    /// Window picker: focus the highlighted window (Enter).
    WindowPickerEnter,
    /// Window picker: focus row N, 1-based (digit keys).
    WindowPickerNumber(u8),
    /// Window picker: close without changing focus (Esc).
    WindowPickerEsc,
    /// Open the Info menu from the keyboard (leader `i`).
    InfoOpen,
    /// Open (or close) the menubar volume popover from the keyboard (leader =).
    VolumeOpen,
    /// Open (or close) the menubar brightness popover from the keyboard
    /// (leader -). Nothing opens on a machine without a backlight.
    BrightnessOpen,
    /// A key for the open volume popover (Left / Right level, Up / Down rows,
    /// Enter, m / Space mute, Esc).
    TrayKey(crate::tray::TrayKey),
    /// Start the keyboard move mode on the focused window (leader w).
    MoveModeStart,
    /// Move mode: move (or, in resize mode, grow/shrink) by `dx`, `dy` cells.
    MoveModeStep { dx: i32, dy: i32 },
    /// Move mode: switch between moving and resizing (r).
    MoveModeToggleResize,
    /// Move mode: finish. `cancel` (Esc) puts the window back where it started;
    /// otherwise (Enter) the new geometry stays.
    MoveModeEnd { cancel: bool },
    /// Begin renaming the currently focused window (keyboard shortcut / double-click).
    RenameFocused,
    /// Type a character into the window rename buffer.
    RenameChar(char),
    /// Delete the last character from the window rename buffer.
    RenameBackspace,
    /// Commit the window rename (sets the new title; empty buffer = cancel).
    RenameCommit,
    /// Cancel the window rename without changing the title.
    RenameCancel,
    /// Confirm the close-window dialog (Enter / y): close the pending window.
    /// Leader + Esc: close the focused window (apps ask first).
    CloseFocused,
    ConfirmCloseYes,
    /// Dismiss the close-window dialog (Esc / n) without closing.
    ConfirmCloseNo,
    /// Confirm the launch-warning dialog (Enter / y): launch the pending entry.
    LaunchWarnYes,
    /// Dismiss the launch-warning dialog (Esc / n) without launching.
    LaunchWarnNo,
    /// Shut down the daemon entirely (kills all apps). Sent by `tetron-wm kill`.
    Shutdown,
    /// Restart the frontend only, keeping the apphost (and apps) alive.
    Reload,
    /// Start, stop or toggle the screensaver, optionally naming the effect
    /// (default: the `screensaver` config key). Sent by the leader key and by
    /// `tetron-wm screensaver`.
    Screensaver {
        mode: crate::screensaver::SaverMode,
        #[serde(default)]
        effect: Option<String>,
    },
    /// Ask the daemon for its [`crate::protocol::StatusReply`] (`tetron-wm ls`).
    /// Answered by the control thread; never reaches `apply`'s state.
    Status,
    /// A raw mouse event destined for the focused app's PTY (passthrough).
    MouseInput(crate::mouse::MouseInput),
    /// Scroll the scrollback of the PTY window under `p` by `lines`
    /// (+ = back into history). Sent by the wheel when no overlay claims it.
    ScrollAt { p: Point, lines: i32 },
    /// Power-menu Add Remote form: type a character into the focused field.
    PowerFormChar(char),
    /// Power-menu form: delete the last character of the focused field.
    PowerFormBackspace,
    /// Power-menu form: focus the next / previous field (Tab / Shift-Tab, ↓ / ↑).
    PowerFormNext,
    PowerFormPrev,
    /// Power-menu form: cycle the theme field (← / →).
    PowerFormLeft,
    PowerFormRight,
    /// Power-menu form: Enter — advance, submitting from the last field.
    PowerFormCommit,
    /// Power-menu form: Esc — back to the Systems submenu.
    PowerFormCancel,
    /// Keyboard confirm (Enter/y) of the open power confirm/forget dialog.
    PowerKeyConfirm,
    /// Keyboard cancel (Esc/n) of the power menu.
    PowerKeyCancel,
    /// Apply (and persist) a theme by name. Sent by the client on attach when
    /// `TETRON_WM_THEME` is set — how a per-system theme rides over ssh.
    SetTheme(String),
    /// What the client's display can show (colour depth, bare VT, graphics).
    /// Sent once right after the attach `Resize`; an older daemon drops it as
    /// an unknown variant, which is harmless. See `crate::display`.
    Caps(crate::display::ClientCaps),
    /// Logs viewer: scroll one line / one page.
    LogsUp,
    LogsDown,
    LogsPageUp,
    LogsPageDown,
    /// Logs viewer: copy the log tail to the host clipboard (OSC 52).
    LogsCopy,
    /// Logs viewer: re-read the log file.
    LogsRefresh,
    /// Logs viewer: close the window.
    LogsClose,
    /// A key for the focused image viewer.
    ImageKey(crate::imageview::ImageKey),
    /// A key for the open desktop right-click menu.
    DesktopMenuKey(MenuKey),
    /// Open the right-click menu from the keyboard (leader `.`): on the
    /// focused file manager's entry, else on the desktop.
    ContextMenuOpen,
}

// ── Output frame type ─────────────────────────────────────────────────────────

/// One rendered desktop frame, ready for the compositor.
///
/// The layers are already in z-ordered form (chrome on top of windows on top of
/// desktop background).  The cursor position, if `Some`, should be rendered by
/// the compositor as an inverse-video overlay.
pub struct Frame {
    /// Compositor layers ordered bottom-to-top.
    pub layers: Vec<Layer>,
    /// Screen-space cursor position, or `None` if the cursor is hidden.
    pub cursor: Option<Point>,
    /// Image placements (Kitty graphics) for the visible ImageView windows.
    pub images: Vec<crate::protocol::ImagePlacement>,
}

// ── Session core ──────────────────────────────────────────────────────────────

/// Keyboard move mode state (leader w): the window, where it started (for Esc),
/// and whether arrows currently resize instead of move.
struct MoveMode {
    id: WindowId,
    start: Rect,
    start_state: crate::window::WindowState,
    resize: bool,
}

/// Owns the window manager and all running app instances.
///
/// `SessionCore` is the clean `ClientMsg`-in / `Frame`-out boundary.
/// All internal state (window geometry, PTY handles, drag tracking) is
/// fully encapsulated; callers interact only through [`apply`](Self::apply)
/// and [`build_frame`](Self::build_frame).
///
/// A future daemon will serialise `ClientMsg` over a socket and deliver the
/// resulting `Frame` layers to a remote renderer — keeping this struct on the
/// server side.
pub struct SessionCore {
    wm: WindowManager,
    contents: HashMap<WindowId, WinContent>,
    apphost: Box<dyn AppHost>,
    /// Active or last mouse text-selection in an app window (for middle paste).
    selection: Option<TextSel>,
    /// The history file path the daemon registered (so Settings can switch
    /// persistence on later); `None` in tests, which never touch the real file.
    clip_persist_path: Option<std::path::PathBuf>,
    /// One-shot "select mode": the mouse is never passed into apps until one
    /// selection is made, so text can be selected out of mouse-capturing apps
    /// (w3m, carbonyl, vim) without relying on Shift, which many terminals
    /// intercept. Toggled by leader + `x`.
    select_mode: bool,
    /// App-managed state (icon positions, recent directories), persisted to
    /// `state_path` when the daemon attached one.
    state: crate::state::State,
    state_path: Option<std::path::PathBuf>,
    /// The clipboard history popover (menubar segment / leader + C).
    clipmenu: crate::clipmenu::ClipMenu,
    /// Clipboard, primary selection and history, shared with the daemon's
    /// control thread (see `clipboard.rs`).
    clip: crate::clipboard::SharedClip,
    /// The store window's id, if open (so it can be re-focused, not re-opened).
    store_win: Option<WindowId>,
    /// The settings window's id, if open.
    settings_win: Option<WindowId>,
    /// The file-manager window's id, if open.
    filemanager_win: Option<WindowId>,
    /// Remote file-manager windows: window → (system name, ssh target, port).
    remote_fms: HashMap<WindowId, (String, String, Option<u16>)>,
    /// Cross-window file clipboard for system↔system transfers: the source
    /// window's ssh backend (`None` = local disk) and the copied paths.
    transfer: Option<(FmBackend, Vec<std::path::PathBuf>)>,
    /// Post-update safety dialog: the running apphost predates this binary's
    /// minimum-compatible protocol, so a full app-server restart (which closes
    /// the user's apps) is needed for everything to work. The dialog gives
    /// them a chance to save work first.
    compat_dialog: bool,
    /// Set once the user was warned (so Settings can show the restart row even
    /// after the dialog is dismissed).
    apphost_outdated: bool,
    /// The logs-viewer window's id, if open.
    logs_win: Option<WindowId>,
    /// The activity-monitor window's id, if open.
    activity_win: Option<WindowId>,
    /// Text to put on the HOST terminal's clipboard (OSC 52), shipped to the
    /// client in the next frame: log copies and app OSC-52 stores.
    pending_clipboard: Option<String>,
    pending_screenshot: Option<String>,
    /// Dock-ordered list of (id, display-name) pairs.
    titles: Vec<(WindowId, String)>,
    /// Immutable grouping key per window (set at spawn/open; never mutated).
    app_keys: HashMap<WindowId, String>,
    /// The app_key of the dock group whose chooser popup is open (`None` = closed).
    dock_popup: Option<String>,
    /// Dock right-click context menu: `Some((target window, anchor x of the
    /// clicked pill))` while open, `None` when closed.
    dock_ctx: Option<(WindowId, i32)>,
    /// The title bar right-click menu: its window and the shared menu widget.
    win_menu: Option<(WindowId, crate::contextmenu::ContextMenu)>,
    /// Short host name (computed once at startup); used in the AI briefing.
    host_label: String,
    /// Active window rename: `Some((id, buffer))` while the user is typing a new
    /// name. `None` when no rename is in progress.
    rename: Option<(WindowId, String)>,
    cfg: Config,
    w: i32,
    h: i32,
    drag: Option<Hit>,
    /// A mouse drag panning a zoomed image viewer: its window and the last
    /// pointer cell.
    image_pan: Option<(WindowId, Point)>,
    /// Pointer position where the current drag was pressed, and whether it has
    /// passed the drag threshold. A plain click (or sub-threshold jitter) on a
    /// titlebar must NOT move/untile the window — only a real drag does.
    drag_start: Option<Point>,
    drag_armed: bool,
    cursor: Point,
    /// Set when the user clicks the menubar quit button; polled by the loop.
    /// In daemon mode this means "detach", not "shut down".
    quit: bool,
    /// A full-screen program (fbi + images) for the client to run on the real
    /// console; rides on the detach like `switch_to`.
    fullscreen: Option<Vec<String>>,
    /// A waiter service switch was handed to the client: re-read its state on reattach.
    refresh_waiter: bool,
    /// The display facts the client reported, before the `colors` setting is
    /// applied (so switching the setting back to auto restores them).
    client_caps: crate::display::ClientCaps,
    /// The colour depth changed: the daemon re-baselines the client's screen.
    repaint: bool,
    /// Set by `ClientMsg::Shutdown` — the daemon should exit and kill all apps.
    shutdown: bool,
    /// Set by `ClientMsg::Reload` — the daemon should restart the frontend only,
    /// keeping the apphost (and all apps) alive.
    reload: bool,
    /// The app launcher (menubar dropdown + Spotlight overlay).
    launcher: Launcher,
    /// The top-right power menu (Exit / Restart / Shutdown + confirm dialogs).
    power_menu: PowerMenu,
    /// A suspend / reboot / power-off command chain running on a worker thread:
    /// the action and the slot its [`crate::powerctl::Report`] lands in.
    power_job: Option<(crate::powerctl::Kind, std::sync::Arc<std::sync::Mutex<Option<crate::powerctl::Report>>>)>,
    /// Settings > Updates check worker, read by [`Self::poll_update_check`].
    update_check: Option<std::sync::mpsc::Receiver<String>>,
    /// Settings > Get more themes worker, read by [`Self::poll_themes`].
    theme_job: Option<std::sync::mpsc::Receiver<crate::themefetch::Msg>>,
    /// Settings > General > Apps download size worker, read by [`Self::poll_apps_download`].
    apps_download_job: Option<std::sync::mpsc::Receiver<Option<crate::packaging::AptEstimate>>>,
    /// A running Store Update list fetch (WMCAT-004).
    list_job: Option<std::sync::mpsc::Receiver<String>>,
    /// Modal "are you sure?" shown when closing an app window (which kills it).
    confirm_close: ConfirmClose,
    /// Keyboard window picker (leader f): modal numbered window list.
    win_picker: crate::winpicker::WinPicker,
    /// Keyboard move / resize mode (leader w), `None` when off.
    move_mode: Option<MoveMode>,
    /// Modal "are you sure?" shown before launching an app entry flagged `warn`.
    launch_warn: LaunchWarn,
    /// What the UI shows, for the poller (None outside the daemon).
    interest: Option<std::sync::Arc<crate::system::Interest>>,
    /// Shared host-state snapshot refreshed by the daemon's `SystemPoller`.
    tray_state: std::sync::Arc<std::sync::RwLock<crate::system::SystemState>>,
    /// The menubar status tray (open popover state + hit-testing).
    tray: crate::tray::Tray,
    /// The OS backend that applies tray control intents (volume/wifi/bluetooth).
    backend: Box<dyn crate::system::Backend>,
    /// Target-cell highlight shown while dragging a window near an edge.
    drag_preview: Option<Rect>,
    /// The working-directory picker, open while a flagged launch awaits a dir.
    dirpicker: Option<crate::dirpicker::DirPicker>,
    /// The running screensaver, if any.
    saver: Option<Saver>,
    /// The keyboard-shortcut help window, while one is open.
    help_win: Option<WindowId>,
    /// Whether the full-screen "simple" view mode is active.
    simple: bool,
    /// Decoded-image cache for ImageView windows (the native image layer).
    images: crate::imagestore::ImageStore,
    /// The wallpaper-level desktop icons (merged `~/Desktop` + pins).
    desktop: crate::desktop::DesktopIcons,
    /// PROTOTYPE image wallpaper (cells via chafa), built from the
    /// `TETRON_WM_WALLPAPER=<image>` env var. `None` = normal solid/icon desktop.
    wallpaper: Option<crate::buffer::CellBuffer>,
    /// When [`poll_desktop_dir`](Self::poll_desktop_dir) last actually stat'd
    /// the desktop folder (throttle so every tick doesn't hit the filesystem).
    desktop_last_poll: std::time::Instant,
    /// The desktop folder's modified-time as of the last poll, so a changed
    /// mtime (folder/file created or removed outside tetron-wm) is detected.
    desktop_last_mtime: Option<std::time::SystemTime>,
    /// Maps a hosted app's Kitty image id `(window, kitty_id)` to the `ImageStore`
    /// id its PNG was loaded under (populated by [`refresh_app_graphics`]).
    app_image_ids: HashMap<(WindowId, u32), u64>,
    /// Background loader for file-manager / desktop thumbnails (off the desktop
    /// loop so a slow/offloaded image never freezes the UI).
    thumb_loader: crate::thumbnail::ThumbLoader,
    /// Maps a source image path to its loaded thumbnail `ImageStore` id.
    thumb_ids: HashMap<std::path::PathBuf, u64>,
    /// Pre-generated large file-type icons (one per role) → `ImageStore` id.
    role_icon_ids: HashMap<crate::openwith::Role, u64>,
    /// Last `WinMeta` blob pushed to the apphost per app window (change-gate so
    /// we only send `set_meta` when a window actually moved/retitled/minimized).
    last_meta: HashMap<WindowId, Vec<u8>>,
    /// Saved remote systems shown in the power menu's Systems submenu.
    systems: Vec<crate::systems::RemoteSystem>,
    /// Set when the user picked a system to switch to: shipped to the client in
    /// the next frame (alongside `quit`), which exits and ssh-es over.
    switch_to: Option<crate::systems::SwitchSpec>,
}

impl SessionCore {
    /// Create a new session for a terminal of size `w` × `h` cells.
    ///
    /// The work area is set to exclude the single-row menubar at the top (which
    /// also carries the taskbar), i.e. `Rect::new(0, 1, w, h - 1)`.
    pub fn new(w: i32, h: i32, cfg: Config) -> Self {
        Self::with_apphost(w, h, cfg, Box::new(LocalAppHost::new()))
    }

    /// Construct a session backed by a specific [`AppHost`]. The daemon injects
    /// a `RemoteAppHost` here (Phase 2b); tests and in-process use get the
    /// default `LocalAppHost` via [`new`](Self::new).
    pub fn with_apphost(w: i32, h: i32, cfg: Config, apphost: Box<dyn AppHost>) -> Self {
        let work = Rect::new(0, 1, w, h - 1);
        let systems = crate::systems::load();
        let launcher = Launcher::new(Self::build_launcher_apps(&cfg, &systems));
        let desktop_dir = dirs::home_dir().map(|h| h.join("Desktop")).unwrap_or_default();
        let wallpaper = Self::build_wallpaper(w, h, &cfg);
        crate::fullscreen::set_enabled(cfg.fullscreen);
        crate::logfile::set_level(cfg.log_level);
        let mut core = Self {
            wm: WindowManager::new(work),
            contents: HashMap::new(),
            apphost,
            selection: None,
            select_mode: false,
            state: crate::state::State::default(),
            state_path: None,
            clipmenu: crate::clipmenu::ClipMenu::default(),
            clip_persist_path: None,
            clip: crate::clipboard::new_shared(Self::clip_limits(&cfg), cfg.clipboard_history),
            store_win: None,
            settings_win: None,
            filemanager_win: None,
            remote_fms: HashMap::new(),
            transfer: None,
            compat_dialog: false,
            apphost_outdated: false,
            logs_win: None,
            activity_win: None,
            pending_clipboard: None,
            pending_screenshot: None,
            titles: Vec::new(),
            app_keys: HashMap::new(),
            dock_popup: None,
            dock_ctx: None,
            win_menu: None,
            host_label: Self::host_short_label(),
            rename: None,
            cfg,
            w,
            h,
            drag: None,
            image_pan: None,
            drag_start: None,
            drag_armed: false,
            cursor: Point::new(w / 2, h / 2),
            quit: false,
            shutdown: false,
            reload: false,
            launcher,
            power_menu: PowerMenu::new(),
            power_job: None,
            theme_job: None,
            apps_download_job: None,
            list_job: None,
            update_check: None,
            confirm_close: ConfirmClose::new(),
            win_picker: crate::winpicker::WinPicker::new(),
            move_mode: None,
            launch_warn: LaunchWarn::new(),
            interest: None,
            tray_state: std::sync::Arc::new(std::sync::RwLock::new(crate::system::SystemState::default())),
            tray: crate::tray::Tray::new(),
            backend: crate::system::backend(),
            drag_preview: None,
            dirpicker: None,
            help_win: None,
            saver: None,
            simple: false,
            images: crate::imagestore::ImageStore::new(),
            desktop: crate::desktop::DesktopIcons::new(desktop_dir),
            wallpaper,
            desktop_last_poll: std::time::Instant::now(),
            desktop_last_mtime: None,
            app_image_ids: HashMap::new(),
            thumb_loader: crate::thumbnail::ThumbLoader::new(),
            thumb_ids: HashMap::new(),
            role_icon_ids: HashMap::new(),
            last_meta: HashMap::new(),
            systems,
            switch_to: None,
            fullscreen: None,
            refresh_waiter: false,
            client_caps: crate::display::ClientCaps::default(),
            repaint: false,
        };
        core.tray.set_tools(core.cfg.tray_tools.clone());
        // A long-lived apphost may predate a config edit made while the UI was down.
        core.push_terminal_colors();
        core.generate_role_icons();
        core.reload_desktop();
        core
    }

    /// Generate the large file-type icons once and cache them in the image store.
    fn generate_role_icons(&mut self) {
        use crate::openwith::Role::*;
        // Match the icon-image cell box aspect (cells are ~8×16px), at 2× for crispness.
        let w = ((crate::desktop::ICON_W - 2).max(2) * 16) as u32;
        let h = ((crate::desktop::ICON_H - 1).max(1) * 32) as u32;
        for role in [
            Image, Audio, Video, Text, Code, Archive, Pdf, Directory, Executable, Other,
        ] {
            if let Some(png) = crate::icons::role_icon_png(role, w, h) {
                let id = self.images.store_png(png, w, h);
                self.role_icon_ids.insert(role, id);
            }
        }
    }

    /// Rebuild the desktop icons from the configured pins + saved positions, lay
    /// them out for the current screen, and refresh any image thumbnails.
    fn reload_desktop(&mut self) {
        // layout() must run first: reload() assigns icons to free grid cells, which
        // needs the real column/row count (else everything piles onto cell (0,0)).
        self.desktop.layout(self.w, self.h);
        self.desktop.reload(&self.cfg.desktop_pins, &self.state.desktop_positions);
        let n = self.desktop.icons().len();
        crate::log_changed("desktop reload", &n.to_string(), &format!("desktop reload: {n} icons"));
        self.refresh_desktop_thumbnails();
    }

    /// Throttled mtime watch for `~/Desktop`: at most every
    /// [`DESKTOP_POLL_INTERVAL`], stat the desktop folder and reload the
    /// desktop when its modified-time changed since the last observed scan —
    /// picks up a folder/file created via the file manager or a terminal
    /// without an FS-watcher dependency. Called once per daemon tick; cheap
    /// (a single `stat`, gated by an `Instant` so most ticks do nothing).
    pub fn poll_desktop_dir(&mut self) {
        let now = std::time::Instant::now();
        if !desktop_poll_due(self.desktop_last_poll, now) {
            return;
        }
        self.desktop_last_poll = now;
        let current = std::fs::metadata(self.desktop.dir()).and_then(|m| m.modified()).ok();
        if desktop_mtime_changed(self.desktop_last_mtime, current) {
            crate::dbg_log("desktop: folder mtime changed, auto-reloading");
            self.reload_desktop();
        }
        self.desktop_last_mtime = current;
    }

    /// Assign already-loaded thumbnails to the desktop's image icons and queue any
    /// not-yet-loaded ones on the background loader. Never blocks on file I/O.
    fn refresh_desktop_thumbnails(&mut self) {
        for (idx, path) in self.desktop.thumbnail_requests() {
            if let Some(&id) = self.thumb_ids.get(&path) {
                self.desktop.set_thumb(idx, id);
            } else {
                // Icon tile is 21 cells wide, 4 tall; cells are ~8x16px.
                self.thumb_loader.request(path, 13 * 8, 4 * 16);
            }
        }
    }

    /// Drain finished thumbnails into the image store, then (re)assign them to the
    /// desktop and the open file manager. Called once per frame; cheap (no I/O).
    /// Drive the image viewers: collect finished decodes and renders, start
    /// the renders their current size, zoom and client need, and keep each
    /// window title in step with the shown file.
    pub fn pump_images(&mut self) {
        let caps = crate::display::get();
        let focused = self.wm.focused();
        let simple = self.simple.then(|| self.simple_content_rect());
        let views: Vec<(WindowId, crate::geometry::Rect, bool)> = self
            .wm
            .z_ordered()
            .iter()
            .filter(|w| !w.minimized && matches!(self.contents.get(&w.id), Some(WinContent::ImageView(_))))
            .map(|w| match simple {
                Some(r) if focused == Some(w.id) => (w.id, r, true),
                _ => (w.id, w.content_rect(), self.fully_unobstructed(w)),
            })
            .collect();
        for (id, r, unobstructed) in views {
            let Some(WinContent::ImageView(v)) = self.contents.get_mut(&id) else { continue };
            v.poll(r.w, r.h, caps, unobstructed, &mut self.images);
            let title = v.title();
            if let Some(t) = self.titles.iter_mut().find(|(w, _)| *w == id) {
                if t.1 != title {
                    t.1 = title.clone();
                    self.wm.rename_window(id, title);
                }
            }
        }
    }

    pub fn pump_thumbnails(&mut self) {
        for r in self.thumb_loader.drain() {
            let id = self.images.store_png(r.png, r.w, r.h);
            self.thumb_ids.insert(r.path, id);
        }
        self.refresh_desktop_thumbnails();
        self.refresh_fm_thumbnails();
    }

    // ── Clipboard history ───────────────────────────────────────────────────

    fn clip_limits(cfg: &Config) -> crate::clipboard::ClipLimits {
        crate::clipboard::ClipLimits {
            max_entries: cfg.clipboard_history_size.max(1),
            max_item_bytes: cfg.clipboard_item_max_kb.max(1) * 1024,
            ..Default::default()
        }
    }

    /// Lower-cased title + command of the focused app window, for the
    /// `clipboard_ignore_apps` match.
    fn focused_app_label(&self) -> String {
        let Some(id) = self.wm.focused() else { return String::new() };
        let mut s = self.titles.iter().find(|(w, _)| *w == id).map(|(_, t)| t.clone()).unwrap_or_default();
        if let Some(WinContent::App(aid)) = self.contents.get(&id) {
            if let Some((cmd, args)) = self.apphost.launch_cmd(*aid) {
                s.push(' ');
                s.push_str(&cmd);
                s.push(' ');
                s.push_str(&args.join(" "));
            }
        }
        s.to_lowercase()
    }

    /// Whether the clipboard history popover is showing (the client routes keys to it).
    pub fn clipmenu_open(&self) -> bool { self.clipmenu.is_open() }

    /// Open or close the clipboard history popover, anchored under its menubar
    /// segment (or the right edge if the segment is hidden).
    fn toggle_clipmenu(&mut self) {
        if !self.cfg.clipboard_history {
            return;
        }
        if !self.clipmenu.is_open() {
            self.launcher.close();
            self.tray.close();
            self.power_menu.close();
        }
        let anchor = self
            .tray_segments_now()
            .iter()
            .find(|s| s.kind == crate::tray::SegmentKind::Clipboard)
            .map(|s| s.rect.x)
            .unwrap_or((self.w - 64).max(0));
        self.clipmenu.toggle(anchor);
    }

    /// Whether one-shot select mode is on (menubar indicator, tests).
    pub fn select_mode(&self) -> bool { self.select_mode }

    /// Centre of the open clipboard popover's [Select] button (integration tests).
    #[doc(hidden)]
    pub fn clipmenu_select_button_for_test(&self) -> Option<Point> {
        if !self.clipmenu.is_open() {
            return None;
        }
        let hist = self.clip_history();
        Some(self.clipmenu.layout(self.w, self.h, &hist).select_btn.center())
    }

    /// Menubar tray segments including the clipboard indicator.
    fn clip_badge(&self) -> Option<(usize, bool)> {
        if !self.cfg.clipboard_history {
            return None;
        }
        let s = crate::clipboard::lock(&self.clip);
        Some((s.history.len(), s.history.paused()))
    }

    /// Carry out what the user chose in the clipboard popover.
    fn apply_clip_action(&mut self, a: crate::clipmenu::ClipAction) {
        use crate::clipmenu::ClipAction as A;
        let entry_text = |s: &Self, i: usize| crate::clipboard::lock(&s.clip).history.get(i).map(|e| e.text.clone());
        match a {
            A::Paste(i) | A::CopyOnly(i) => {
                if let Some(text) = entry_text(self, i) {
                    self.set_clipboard(&text);
                    self.pending_clipboard = Some(text.clone());
                    // Re-record so the chosen entry moves to the front.
                    self.clip_record(&text, crate::clipboard::ClipSource::Copy);
                    self.clipmenu.close();
                    if matches!(a, A::Paste(_)) {
                        self.paste_to_focused(&text);
                    }
                }
            }
            A::TogglePin(i) => crate::clipboard::lock(&self.clip).history.toggle_pin(i),
            A::Delete(i) => crate::clipboard::lock(&self.clip).history.delete(i),
            A::TogglePause => {
                let mut s = crate::clipboard::lock(&self.clip);
                let p = !s.history.paused();
                s.history.set_paused(p);
            }
            A::Clear => crate::clipboard::lock(&self.clip).history.clear(),
            A::Select => {
                // Same as Ctrl+Space x: close the popover so the drag lands on the app.
                self.select_mode = true;
                self.clipmenu.close();
            }
            A::Close => self.clipmenu.close(),
        }
        let hist = self.clip_history();
        self.clipmenu.history_changed(&hist);
        crate::clipboard::lock(&self.clip).save_if_due(false);
    }

    /// The mouse-selection buffer (pasted by middle click / Shift+Insert).
    fn primary_text(&self) -> String { crate::clipboard::lock(&self.clip).primary.clone() }

    /// The explicit clipboard (set by Ctrl+Shift+C, an app's OSC 52 store or the
    /// CLI; pasted by Ctrl+Shift+V and answered to OSC 52 reads).
    fn clipboard_text(&self) -> String { crate::clipboard::lock(&self.clip).clipboard.clone() }

    fn set_primary(&mut self, t: &str) { crate::clipboard::lock(&self.clip).primary = t.to_string(); }

    fn set_clipboard(&mut self, t: &str) { crate::clipboard::lock(&self.clip).clipboard = t.to_string(); }

    /// Record a copy in the history, honouring the config switches. Does not
    /// touch the live clipboard.
    fn clip_record(&mut self, text: &str, src: crate::clipboard::ClipSource) {
        use crate::clipboard::ClipSource;
        if !self.cfg.clipboard_history || (src == ClipSource::Selection && !self.cfg.clipboard_record_selection) {
            return;
        }
        if !self.cfg.clipboard_ignore_apps.is_empty() {
            let label = self.focused_app_label();
            if self.cfg.clipboard_ignore_apps.iter().any(|a| !a.is_empty() && label.contains(&a.to_lowercase())) {
                return;
            }
        }
        crate::clipboard::lock(&self.clip).history.record(text, src);
    }

    /// The history (read-only), for the UI and tests.
    pub fn clip_history(&self) -> crate::clipboard::ClipHistory { crate::clipboard::lock(&self.clip).history.clone() }

    /// A handle to the shared clipboard state, for the daemon's control thread.
    pub fn clip_handle(&self) -> crate::clipboard::SharedClip { self.clip.clone() }

    /// Turn on disk persistence: load the file now and save changes later.
    /// Called by the daemon only, so tests never touch the real history file.
    pub fn enable_clipboard_persistence(&mut self, path: std::path::PathBuf) {
        self.clip_persist_path = Some(path.clone());
        if !self.cfg.clipboard_history || !self.cfg.clipboard_persist {
            return;
        }
        self.clip_persist_path = Some(path.clone());
        crate::clipboard::lock(&self.clip).enable_persistence(path);
    }

    /// Apply changed `clipboard_*` settings live: limits, the on/off switch, and
    /// turning disk persistence on (write the history) or off (delete the file).
    fn apply_clipboard_config(&mut self) {
        let limits = Self::clip_limits(&self.cfg);
        let mut s = crate::clipboard::lock(&self.clip);
        s.enabled = self.cfg.clipboard_history;
        s.history.set_limits(limits);
        let want = self.cfg.clipboard_history && self.cfg.clipboard_persist;
        match (want, s.path.is_some(), self.clip_persist_path.clone()) {
            (true, false, Some(p)) => s.persist_to(p),
            (false, true, _) => s.stop_persisting(),
            _ => {}
        }
        s.save_if_due(true);
        drop(s);
        if !self.cfg.clipboard_history {
            self.clipmenu.close();
        }
    }

    /// The live config (integration tests).
    #[doc(hidden)]
    pub fn config_for_test(&self) -> &Config { &self.cfg }

    /// Replace the live config and re-apply the clipboard settings, as the
    /// Settings panel does on every change (integration tests).
    #[doc(hidden)]
    pub fn set_config_for_test(&mut self, cfg: Config) {
        self.cfg = cfg;
        self.tray.set_tools(self.cfg.tray_tools.clone());
        self.apply_clipboard_config();
    }

    /// Flush the history to disk now (daemon exit).
    pub fn flush_clipboard(&mut self) {
        crate::clipboard::lock(&self.clip).save_if_due(true);
    }

    /// Drain app events captured by the PTY emulators: bell rings become dock/
    /// tray notifications (for unfocused or minimized windows), and OSC-52
    /// clipboard stores are forwarded to the host terminal. Called once per
    /// daemon tick; cheap (drains in-memory counters).
    pub fn pump_app_events(&mut self) {
        let focused = self.wm.focused();
        let minimized: std::collections::HashSet<WindowId> =
            self.wm.z_ordered().iter().filter(|w| w.minimized).map(|w| w.id).collect();
        let now = self.tray_state.read().map(|st| st.clock.time.clone()).unwrap_or_default();
        let wins: Vec<(WindowId, AppId, String)> = self
            .titles
            .iter()
            .filter_map(|(id, title)| match self.contents.get(id) {
                Some(WinContent::App(aid)) => Some((*id, *aid, title.clone())),
                _ => None,
            })
            .collect();
        for (id, aid, title) in wins {
            let bells = self.apphost.take_bells(aid);
            if bells > 0 && (focused != Some(id) || minimized.contains(&id)) {
                crate::dbg_log(&format!("notify: '{title}' rang the bell x{bells}"));
                self.tray.notify(id.0, title.clone(), now.clone());
            }
            if let Some(text) = self.apphost.take_clipboard(aid) {
                crate::dbg_log(&format!("clipboard: forwarding {} bytes from '{title}' (OSC 52)", text.len()));
                self.set_clipboard(&text);
                self.clip_record(&text, crate::clipboard::ClipSource::App);
                self.pending_clipboard = Some(text);
            }
            // OSC 52 READ queries (`ESC ] 52 ; c ; ? ST`): apps such as Fresh and
            // vim's osc52 package paste this way. Answer from our clipboard,
            // subject to `clipboard_osc52_read`.
            for req in self.apphost.take_clip_requests(aid) {
                let allowed = match self.cfg.clipboard_osc52_read.as_str() {
                    "always" => true,
                    "never" => false,
                    _ => focused == Some(id),
                };
                if !allowed {
                    crate::dbg_log(&format!("clipboard: OSC 52 read from '{title}' denied ({})", self.cfg.clipboard_osc52_read));
                    continue;
                }
                let text = if req.kind == 'p' { self.primary_text() } else { self.clipboard_text() };
                let reply = crate::clipboard::osc52_reply(req.kind, &text, req.bel);
                crate::dbg_log(&format!("clipboard: OSC 52 read from '{title}' answered ({} bytes)", text.len()));
                self.apphost.input(aid, reply.as_bytes());
            }
        }
        if self.cfg.clipboard_ttl_secs > 0 {
            crate::clipboard::lock(&self.clip).history.expire(crate::clipboard::now_ms(), self.cfg.clipboard_ttl_secs);
        }
        // Text set through the `clip` CLI / shims goes out to the host terminal too.
        if let Some(t) = crate::clipboard::lock(&self.clip).host_out.take() {
            self.pending_clipboard = Some(t);
        }
        crate::clipboard::lock(&self.clip).save_if_due(false);
    }

    /// Point the desktop at a specific directory and reload (integration tests).
    #[doc(hidden)]
    pub fn set_desktop_dir_for_test(&mut self, dir: std::path::PathBuf) {
        self.desktop = crate::desktop::DesktopIcons::new(dir);
        self.reload_desktop();
        // Re-baseline the mtime watch against the new folder so a stale mtime
        // from the previous desktop dir doesn't trigger a spurious reload.
        self.desktop_last_mtime = None;
        self.desktop_last_poll = std::time::Instant::now();
    }

    /// The number of currently-selected desktop icons (integration tests).
    #[doc(hidden)]
    pub fn desktop_selection_len_for_test(&self) -> usize {
        self.desktop.selection().len()
    }

    /// The screen tile rect of desktop icon `idx` (integration tests).
    #[doc(hidden)]
    pub fn desktop_icon_tile_for_test(&self, idx: usize) -> crate::geometry::Rect {
        self.desktop.tile_rect(self.desktop.icons()[idx].cell)
    }

    /// Begin the desktop new-folder overlay directly (integration tests).
    #[doc(hidden)]
    pub fn begin_desktop_new_folder_for_test(&mut self) {
        self.desktop.begin_new_folder();
    }

    /// Whether the desktop has a rename / new-folder overlay open (so the client
    /// forwards typed characters as desktop overlay input).
    pub fn desktop_editing(&self) -> bool {
        self.desktop.is_editing()
    }

    /// Whether a window rename is in progress (so the client forwards typed
    /// characters to the rename buffer rather than the focused app).
    pub fn renaming(&self) -> bool {
        self.rename.is_some()
    }

    /// The menubar power-button label: the machine's host name + a ▾ chevron
    /// (the short host name, domain stripped, capped so it can't crowd the bar).
    fn host_short_label() -> String {
        let host = sysinfo::System::host_name().unwrap_or_else(|| "tetron-wm".into());
        let short: String = host.split('.').next().unwrap_or(&host).chars().take(20).collect();
        if short.is_empty() { "tetron-wm".to_string() } else { short }
    }

    /// True when no non-minimized window's rect contains `p` (a click here falls
    /// through to the desktop).
    fn window_at_is_none(&self, p: Point) -> bool {
        !self.wm.z_ordered().iter().any(|w| !w.minimized && w.rect.contains(p))
    }

    /// Return the id of the topmost non-minimized window whose **titlebar row**
    /// contains `p`, provided `p` is NOT on a control button (min/max/close).
    /// Used by the double-click handler to start a rename.
    /// The topmost window whose **title text** is at `p`. Rename triggers only on
    /// the title text (not the whole titlebar), so a double-click on empty
    /// titlebar space doesn't accidentally start a rename. The text is drawn at
    /// column 2 of the titlebar, truncated to the same limit `render_window` uses.
    fn topmost_window_title_text_at(&self, p: Point) -> Option<WindowId> {
        self.wm
            .z_ordered()
            .iter()
            .filter(|w| {
                if w.minimized || w.rect.y != p.y {
                    return false;
                }
                let r = w.rect;
                let title_limit = if r.w >= 9 { (r.w - 10).max(0) } else { (r.w - 4).max(0) } as usize;
                let len = w.title.chars().count().min(title_limit) as i32;
                let start = r.x + 2;
                p.x >= start && p.x < start + len
            })
            .map(|w| w.id)
            .next_back()
    }

    /// The topmost non-minimized window whose **content area** contains `p`, with
    /// that content rect. `z_ordered` is bottom-to-top, so the last match is on top.
    fn topmost_window_content_at(&self, p: Point) -> Option<(WindowId, Rect)> {
        self.wm
            .z_ordered()
            .iter()
            .filter(|w| !w.minimized && w.content_rect().contains(p))
            .map(|w| (w.id, w.content_rect()))
            .next_back()
    }

    /// The App window whose **content** a point lands in, respecting occlusion:
    /// it is the *topmost* window overall (by full frame, so a higher window's
    /// titlebar/border blocks windows beneath) and only when `p` is in that
    /// window's own content area and it hosts an app. Returns None when the top
    /// window under `p` is chrome, a non-app widget, or nothing — so callers do
    /// NOT leak a click through to a lower window. Used for text selection and
    /// middle-click paste, which must never click through a titlebar.
    fn topmost_app_content_at(&self, p: Point) -> Option<(WindowId, AppId, Rect)> {
        // z_ordered() is bottom-to-top; the last (topmost) window whose full
        // frame contains the point is the one the user is actually clicking.
        let top = self
            .wm
            .z_ordered()
            .into_iter()
            .rev()
            .find(|w| !w.minimized && w.rect.contains(p))?;
        if !top.content_rect().contains(p) {
            return None; // the point is on this window's chrome, not content
        }
        match self.contents.get(&top.id) {
            Some(WinContent::App(aid)) => Some((top.id, *aid, top.content_rect())),
            _ => None,
        }
    }

    /// Fire any pending Updates-section action (Check / Install), from either
    /// the keyboard or the mouse path.
    fn drain_settings_action(&mut self) {
        match self.focused_settings_mut().and_then(|s| s.take_action()) {
            Some(crate::settings::SettingsAction::ClearClipboard) => {
                crate::clipboard::lock(&self.clip).history.clear();
                crate::clipboard::lock(&self.clip).save_if_due(true);
                let hist = self.clip_history();
                self.clipmenu.history_changed(&hist);
            }
            Some(crate::settings::SettingsAction::CheckUpdates) => {
                // curl can take seconds (or its full time limit offline): run it
                // on a worker, never on the render loop; poll_update_check
                // delivers the result.
                if self.update_check.is_none() {
                    let branch = self.cfg.update_branch.clone();
                    let (tx, rx) = std::sync::mpsc::channel();
                    std::thread::spawn(move || {
                        let mut msg = check_for_updates(&branch);
                        if crate::packaging::running_is_diverted() {
                            msg.push_str(" (own build)");
                        } else if crate::packaging::running_is_packaged() {
                            msg.push_str(" (installed by apt)");
                        }
                        crate::dbg_log(&format!("update: check (branch {branch}, running v{}) -> {msg}", crate::VERSION));
                        let _ = tx.send(msg);
                    });
                    self.update_check = Some(rx);
                }
                if let Some(s) = self.focused_settings_mut() {
                    s.set_update_status("Checking…".into());
                }
            }
            Some(crate::settings::SettingsAction::PreviewColors) => {
                let staged = self.focused_settings_mut().map(|s| {
                    let (bg, fg) = s.staged_colors();
                    (bg.map(String::from), fg.map(String::from))
                });
                if let Some((bg, fg)) = staged {
                    self.send_terminal_colors(bg.as_deref(), fg.as_deref());
                }
            }
            Some(crate::settings::SettingsAction::RevertColors) => {
                self.push_terminal_colors();
            }
            // Sound section: the same intents the tray popover uses, so the
            // menubar and Settings can never disagree.
            Some(crate::settings::SettingsAction::VolumeDown) => self.apply_intent(crate::system::ControlIntent::VolumeDown),
            Some(crate::settings::SettingsAction::BrightnessSet(p)) => self.apply_intent(crate::system::ControlIntent::BrightnessSet(p)),
            Some(crate::settings::SettingsAction::VolumeUp) => self.apply_intent(crate::system::ControlIntent::VolumeUp),
            Some(crate::settings::SettingsAction::VolumeSet(l)) => self.apply_intent(crate::system::ControlIntent::VolumeSet(l)),
            Some(crate::settings::SettingsAction::ToggleMute) => self.apply_intent(crate::system::ControlIntent::ToggleMute),
            Some(crate::settings::SettingsAction::SetOutput(id)) => {
                self.apply_intent(crate::system::ControlIntent::SetOutput(id));
                self.refresh_sound_outputs();
            }
            Some(crate::settings::SettingsAction::OpenMixer) => {
                if let Some((_, intent)) = crate::tray::volume_mixer_now(&self.cfg.tray_tools) {
                    self.apply_intent(intent);
                }
            }
            Some(crate::settings::SettingsAction::RefreshSound) => self.refresh_sound_outputs(),
            Some(crate::settings::SettingsAction::OpenStoreFor(app)) => self.open_store_for(&app),
            Some(crate::settings::SettingsAction::WaiterService(on)) => {
                self.refresh_waiter = true;
                self.hand_over(vec!["@waiter".to_string(), if on { "on" } else { "off" }.to_string()]);
            }
            Some(crate::settings::SettingsAction::RestartApphost) => {
                self.restart_apphost();
            }
            Some(crate::settings::SettingsAction::GetThemes) => {
                if self.theme_job.is_none() {
                    crate::dbg_log("themes: get more themes requested");
                    self.theme_job = Some(crate::themefetch::spawn());
                }
            }
            Some(crate::settings::SettingsAction::InstallApps(size, scripts)) => {
                // Settings > General > Apps (WMINST-007, WMINST-011): the suite
                // installer for that size in a window, so sudo asks where the
                // user can answer.
                crate::dbg_log(&format!("apps: install the {} size with the suite installer (script apps: {scripts})", size.name()));
                let raw = "https://raw.githubusercontent.com/tetronos/tetron-wm";
                // The list Settings counted from, so the script installs the same one.
                let list = crate::packaging::write_apps_list();
                let cmd = crate::packaging::apps_install_command(raw, size, list.as_deref(), scripts);
                // "install:" titles rescan $PATH when the window closes (reap_dead).
                self.launch(format!("install: {} apps", size.name()), "sh".into(), vec!["-lc".into(), cmd]);
            }
            Some(crate::settings::SettingsAction::RemoveApps(size)) => {
                let pkgs = crate::packaging::remove_debs_here(size);
                if !pkgs.is_empty() {
                    crate::dbg_log(&format!("apps: remove set {} ({})", size.name(), pkgs.join(" ")));
                    let cmd = crate::packaging::apps_remove_command(&pkgs);
                    self.launch(format!("install: remove {} set", size.name()), "sh".into(), vec!["-lc".into(), cmd]);
                }
            }
            Some(crate::settings::SettingsAction::AppsDownloadSize) => {
                let pkgs = self.focused_settings_mut().map(|s| s.apps_download_pkgs()).unwrap_or_default();
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let _ = tx.send(crate::packaging::apt_estimate(&pkgs));
                });
                self.apps_download_job = Some(rx);
            }
            Some(crate::settings::SettingsAction::OpenConfigFile) => self.open_config_file(),
            Some(crate::settings::SettingsAction::CreateConfigFile) => {
                match crate::config::init() {
                    Ok(o) => crate::dbg_log(&format!("config: create from example -> {o:?}")),
                    Err(e) => crate::log_warn(&format!("config: create from example failed: {e}")),
                }
                self.reload_to_general();
            }
            Some(crate::settings::SettingsAction::ResetConfigFile) => {
                match crate::config::file_path().and_then(|p| crate::config::reset_at(&p)) {
                    Ok(bak) => crate::dbg_log(&format!("config: reset to the example, old file kept as {bak:?}")),
                    Err(e) => crate::log_warn(&format!("config: reset failed: {e}")),
                }
                self.reload_to_general();
            }
            Some(crate::settings::SettingsAction::ReloadConfig) => {
                crate::dbg_log("config: reload requested from Settings");
                self.reload_to_general();
            }
            Some(crate::settings::SettingsAction::InstallUpdate)
                if crate::packaging::running_is_packaged() && sanitize_branch(&self.cfg.update_branch) != "main" =>
            {
                // A source build would add a second copy beside the packaged one.
                crate::dbg_log("update: refused, dev channel on an apt-installed tetron-wm");
                if let Some(s) = self.focused_settings_mut() {
                    s.set_update_status("Installed by apt: dev builds from source. Use Channel main.".into());
                }
            }
            Some(crate::settings::SettingsAction::InstallUpdate) => {
                // Remember to reopen Settings on the Updates screen after the
                // reload, so the user isn't dumped back on the bare desktop.
                write_reopen_hint(Ui_SETTINGS_UPDATES);
                let cmd = if crate::packaging::running_is_diverted() {
                    own_build_update_command()
                } else {
                    update_command(&self.cfg.update_branch, crate::packaging::running_is_packaged())
                };
                crate::dbg_log(&format!(
                    "update: install requested (branch {}, running v{}, exe {:?}); running in a window",
                    self.cfg.update_branch,
                    crate::VERSION,
                    std::env::current_exe().ok()
                ));
                self.launch("update tetron-wm".into(), "sh".into(), vec!["-lc".into(), cmd]);
            }
            None => {}
        }
    }

    /// Reload the desktop (the config is read fresh) and reopen Settings on
    /// General afterwards.
    fn reload_to_general(&mut self) {
        write_reopen_hint(Ui_SETTINGS_GENERAL);
        self.reload = true;
    }

    /// Settings > General > Open in editor: the Default Apps `editor` with the
    /// config path, `vi` when that is not a command.
    fn open_config_file(&mut self) {
        let Ok(path) = crate::config::file_path() else { return };
        let editor = self.cfg.default_apps.get("editor").cloned().unwrap_or_default();
        let (command, args) = match crate::openwith::open_with(&editor, &path) {
            crate::openwith::OpenAction::RunApp { command, args } => (command, args),
            _ => ("vi".to_string(), vec![path.to_string_lossy().into_owned()]),
        };
        crate::dbg_log(&format!("config: open {} with {command}", path.display()));
        self.open_file_in("config.toml".into(), command, args, path.parent().map(|p| p.to_path_buf()));
    }

    /// Hand a finished update check to every open Settings panel (call once
    /// per daemon tick).
    pub fn poll_update_check(&mut self) {
        let Some(rx) = self.update_check.as_ref() else { return };
        let msg = match rx.try_recv() {
            Ok(m) => m,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => "Couldn't check".into(),
        };
        self.update_check = None;
        for c in self.contents.values_mut() {
            if let WinContent::Settings(s) = c {
                s.set_update_status(msg.clone());
            }
        }
    }

    /// Store > Update list: fetch the catalog on a worker thread (one at a time).
    fn start_list_update(&mut self) {
        if self.list_job.is_some() {
            return;
        }
        self.list_job = Some(crate::listfetch::spawn());
        for c in self.contents.values_mut() {
            if let WinContent::Store(s) = c {
                s.update_started();
            }
        }
    }

    /// Hand a finished Update list to every open Store and rebuild the launcher
    /// from the catalog now in use (call once per daemon tick).
    pub fn poll_lists(&mut self) {
        let Some(rx) = self.list_job.as_ref() else { return };
        let msg = match rx.try_recv() {
            Ok(m) => m,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => "Update list stopped".into(),
        };
        self.list_job = None;
        for c in self.contents.values_mut() {
            if let WinContent::Store(s) = c {
                s.update_done(&msg);
            }
        }
        self.refresh_installed_apps();
    }

    /// Hand the Apps confirmation's download size to the open Settings panels
    /// (call once per daemon tick).
    pub fn poll_apps_download(&mut self) {
        let Some(rx) = self.apps_download_job.as_ref() else { return };
        let bytes = match rx.try_recv() {
            Ok(b) => b,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => None,
        };
        self.apps_download_job = None;
        for c in self.contents.values_mut() {
            if let WinContent::Settings(s) = c {
                s.set_apps_download(bytes);
            }
        }
    }

    /// Hand theme download progress to every open Settings panel (call once
    /// per daemon tick).
    pub fn poll_themes(&mut self) {
        let Some(rx) = self.theme_job.as_ref() else { return };
        let mut last = None;
        let mut done = false;
        loop {
            match rx.try_recv() {
                Ok(crate::themefetch::Msg::Progress(s)) => last = Some(s),
                Ok(crate::themefetch::Msg::Done(s)) => {
                    last = Some(s);
                    done = true;
                    break;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    last.get_or_insert_with(|| "Download stopped".into());
                    done = true;
                    break;
                }
            }
        }
        if done {
            self.theme_job = None;
        }
        let Some(msg) = last else { return };
        for c in self.contents.values_mut() {
            if let WinContent::Settings(s) = c {
                s.set_themes_status(msg.clone(), done);
            }
        }
    }

    /// Re-read the audio outputs into the focused Settings panel (Sound section).
    fn refresh_sound_outputs(&mut self) {
        let outs = crate::system::audio_outputs();
        crate::dbg_log(&format!("sound: {} outputs, default {:?}", outs.len(), outs.iter().find(|o| o.default).map(|o| &o.name)));
        if let Some(s) = self.focused_settings_mut() {
            s.set_outputs(outs);
        }
    }

    /// Called by the daemon after connecting to the apphost: if the running app
    /// server predates this binary's minimum-compatible protocol AND owns live
    /// apps, raise the safety dialog instead of silently misbehaving — the user
    /// gets a chance to save and close work before restarting the app server.
    pub fn check_apphost_compat(&mut self) {
        let proto = self.apphost.proto_version();
        let min = crate::apphost::proto::MIN_COMPAT;
        if needs_apphost_restart(proto, min, self.host_app_count()) {
            crate::dbg_log(&format!(
                "compat: apphost proto {proto} < required {min} with {} app(s) — raising safety dialog",
                self.host_app_count()
            ));
            self.compat_dialog = true;
            self.apphost_outdated = true;
        }
    }

    /// Restart the app server (CLOSES every running app — only ever offered via
    /// the safety dialog / Settings row, after the user was warned): stop the
    /// apphost and reload the frontend; the fresh daemon spawns a new-binary
    /// apphost.
    fn restart_apphost(&mut self) {
        crate::dbg_log("compat: user confirmed app-server restart (apps will close)");
        write_reopen_hint(Ui_SETTINGS_UPDATES);
        self.apphost.shutdown_host();
        self.compat_dialog = false;
        self.apphost_outdated = false;
        self.reload = true;
    }

    /// Whether the post-update compat dialog is showing (modal).
    pub fn compat_dialog_open(&self) -> bool {
        self.compat_dialog
    }

    /// Render the dock context menu (empty when closed). Uses the same
    /// geometry fns as the hit-testing so they can never drift.
    /// Where the title bar menu may go: below the menubar, on screen.
    fn win_menu_bounds(&self) -> Rect {
        Rect::new(0, 1, self.w.max(1), (self.h - 1).max(1))
    }

    /// Render the title bar menu (empty when closed), above the windows.
    fn render_win_menu(&self) -> Vec<crate::compositor::Layer> {
        let Some((_, menu)) = &self.win_menu else { return Vec::new() };
        let b = self.win_menu_bounds();
        let r = menu.rect(b);
        let mut buf = crate::buffer::CellBuffer::new(self.w, self.h);
        buf.fill(crate::cell::Cell { ch: ' ', fg: crate::cell::Rgba::TRANSPARENT, bg: crate::cell::Rgba::TRANSPARENT, attrs: Default::default() });
        menu.render(&mut buf, b);
        // Only the box itself: a layer of the box's size, cut from the buffer.
        let mut boxed = crate::buffer::CellBuffer::new(r.w, r.h);
        for y in 0..r.h {
            for x in 0..r.w {
                if let Some(c) = buf.get(r.x + x, r.y + y) {
                    boxed.set(x, y, *c);
                }
            }
        }
        vec![crate::compositor::Layer { z: 5300, origin: Point::new(r.x, r.y), buf: boxed, opacity: 1.0, scissor: None }]
    }

    /// Perform a title bar menu action on window `id`.
    fn run_window_action(&mut self, id: WindowId, action: crate::contextmenu::MenuAction) {
        use crate::contextmenu::MenuAction as A;
        match action {
            A::Minimize => self.wm.minimize(id),
            A::Maximize => {
                self.wm.unminimize(id);
                self.wm.maximize_toggle(id);
                self.sync_app_size(id);
            }
            A::MoveWindow => {
                self.focus_window(id);
                self.move_mode_start();
            }
            A::RenameWindow => {
                self.focus_window(id);
                self.rename = Some((id, String::new()));
            }
            A::CloseWindow => self.request_close(id),
            _ => {}
        }
    }

    fn render_dock_ctx(&self) -> Vec<crate::compositor::Layer> {
        let Some((_, ax)) = self.dock_ctx else { return Vec::new() };
        let t = crate::theme::current();
        let d = dock_ctx_rect(ax, self.w, self.h);
        let mut buf = crate::buffer::CellBuffer::new(d.w, d.h);
        buf.fill(crate::cell::Cell { ch: ' ', fg: t.text, bg: t.window_bg, attrs: Default::default() });
        let b = |ch: char| crate::cell::Cell { ch, fg: t.border, bg: t.window_bg, attrs: Default::default() };
        for x in 0..d.w {
            buf.set(x, 0, b('\u{2500}'));
            buf.set(x, d.h - 1, b('\u{2500}'));
        }
        for y in 0..d.h {
            buf.set(0, y, b('\u{2502}'));
            buf.set(d.w - 1, y, b('\u{2502}'));
        }
        buf.set(0, 0, b('\u{256D}'));
        buf.set(d.w - 1, 0, b('\u{256E}'));
        buf.set(0, d.h - 1, b('\u{2570}'));
        buf.set(d.w - 1, d.h - 1, b('\u{256F}'));
        for (i, label) in DOCK_CTX_ROWS.iter().enumerate() {
            let fg = if i == 2 { t.close_fg } else { t.text };
            buf.write_str(1, 1 + i as i32, &format!(" {label}"), fg, t.window_bg);
        }
        vec![crate::compositor::Layer { z: 5300, origin: Point::new(d.x, d.y), buf, opacity: 1.0, scissor: None }]
    }

    /// Render the post-update safety dialog (empty when closed).
    fn render_compat_dialog(&self) -> Vec<crate::compositor::Layer> {
        if !self.compat_dialog {
            return Vec::new();
        }
        let t = crate::theme::current();
        let d = compat_dialog_rect(self.w, self.h);
        let mut buf = crate::buffer::CellBuffer::new(d.w, d.h);
        buf.fill(crate::cell::Cell { ch: ' ', fg: t.text, bg: t.window_bg, attrs: Default::default() });
        for x in 0..d.w {
            buf.set(x, 0, crate::cell::Cell { ch: ' ', fg: t.title_fg, bg: t.title_focus, attrs: Default::default() });
        }
        buf.write_str(2, 0, " tetron-wm update ", t.title_fg, t.title_focus);
        let b = |ch: char| crate::cell::Cell { ch, fg: t.border, bg: t.window_bg, attrs: Default::default() };
        for y in 1..d.h {
            buf.set(0, y, b('\u{2502}'));
            buf.set(d.w - 1, y, b('\u{2502}'));
        }
        for x in 0..d.w {
            buf.set(x, d.h - 1, b('\u{2500}'));
        }
        buf.set(0, d.h - 1, b('\u{2570}'));
        buf.set(d.w - 1, d.h - 1, b('\u{256F}'));
        let n = self.host_app_count();
        buf.write_str(2, 2, "This update needs to restart the app server.", t.text, t.window_bg);
        buf.write_str(2, 3, &format!("Your {n} running app(s) will CLOSE when it restarts."), t.close_fg, t.window_bg);
        buf.write_str(2, 5, "Save your work first, then restart - or keep apps", t.dim, t.window_bg);
        buf.write_str(2, 6, "running for now (restart later in Settings > Updates).", t.dim, t.window_bg);
        let (keep, restart) = compat_dialog_buttons(self.w, self.h);
        let keep_s = format!("{:^width$}", "Keep apps", width = keep.w as usize);
        buf.write_str(keep.x - d.x, keep.y - d.y, &keep_s, t.text, t.active_bg);
        let restart_s = format!("{:^width$}", "Restart app server", width = restart.w as usize);
        buf.write_str(restart.x - d.x, restart.y - d.y, &restart_s, t.window_bg, t.close_fg);
        vec![crate::compositor::Layer { z: 6500, origin: Point::new(d.x, d.y), buf, opacity: 1.0, scissor: None }]
    }

    /// On startup (fresh or post-reload), honour a one-shot reopen hint left by
    /// the in-app updater so Settings comes back on the Updates screen.
    pub fn reopen_ui_from_hint(&mut self) {
        let hint = take_reopen_hint();
        if hint == Some(Ui_SETTINGS_UPDATES) || hint == Some(Ui_SETTINGS_GENERAL) {
            self.open_settings();
            if let Some(s) = self.focused_settings_mut() {
                if hint == Some(Ui_SETTINGS_GENERAL) {
                    s.show_general_section();
                } else {
                    s.show_updates_section();
                }
            }
            self.sync_settings();
        }
    }

    /// Scroll the scrollback of the PTY app window under `p`. No-op when the
    /// pointer isn't over an app window (native widgets handle their own scroll).
    /// The intent for a wheel event over the Volume or Brightness tray segment
    /// (menubar row): up = louder or brighter, ignoring `natural_scroll`.
    /// `None` when the pointer is not on one of them, so the caller falls
    /// through to PTY scrollback.
    fn volume_scroll_intent(&self, p: Point, lines: i32) -> Option<crate::system::ControlIntent> {
        use crate::system::ControlIntent;
        use crate::tray::SegmentKind;
        if p.y != 0 {
            return None;
        }
        let kind = self.tray_segments_now().into_iter().find(|s| s.rect.contains(p)).map(|s| s.kind)?;
        let up = lines > 0;
        match kind {
            SegmentKind::Volume => Some(if up { ControlIntent::VolumeUp } else { ControlIntent::VolumeDown }),
            SegmentKind::Brightness => Some(if up { ControlIntent::BrightnessUp } else { ControlIntent::BrightnessDown }),
            _ => None,
        }
    }

    fn scroll_app_at(&mut self, p: Point, lines: i32) {
        // "Natural" scrolling inverts the wheel: down goes back into history.
        let lines = if self.cfg.natural_scroll { -lines } else { lines };
        let Some((id, _)) = self.topmost_window_content_at(p) else { return };
        match self.contents.get(&id) {
            Some(WinContent::App(aid)) => {
                let aid = *aid;
                self.apphost.scroll(aid, lines);
            }
            // The native file manager has no history to scroll; move the
            // selection like the arrow keys instead, which drags the view with
            // it (see filemanager::follow_scroll). Positive `lines` = toward the
            // top, matching the app-pane convention above.
            Some(WinContent::FileManager(_)) => {
                if let Some(WinContent::FileManager(f)) = self.contents.get_mut(&id) {
                    let dir = if lines > 0 { -1 } else { 1 };
                    for _ in 0..lines.unsigned_abs().min(100) {
                        f.move_cursor(0, dir);
                    }
                }
            }
            _ => {}
        }
    }

    /// Attach the app-managed state loaded by the daemon (and where to persist it)
    /// and rebuild the desktop from its saved icon positions. Sessions without an
    /// attached state (tests, in-process use) keep it in memory only.
    pub fn attach_state(&mut self, state: crate::state::State, path: std::path::PathBuf) {
        self.state = state;
        self.state_path = Some(path);
        self.reload_desktop();
    }

    /// The app-managed state (integration tests).
    #[doc(hidden)]
    pub fn state_for_test(&self) -> &crate::state::State { &self.state }

    fn save_state(&self) {
        if let Some(p) = &self.state_path {
            if let Err(e) = self.state.save(p) {
                crate::log_warn(&format!("state: save failed: {e}"));
            }
        }
    }

    /// Persist the desktop's current icon positions to the state file.
    fn persist_desktop_positions(&mut self) {
        self.state.desktop_positions = self.desktop.positions();
        self.save_state();
    }

    /// Open an image file in a new ImageView window.
    fn open_image(&mut self, path: String) {
        crate::dbg_log(&format!("open_image: {}", path));
        // Decoding happens on the viewer's own thread, never on the desktop loop.
        let method = crate::imageview::Method::parse(&self.cfg.image_viewer);
        let view = crate::imageview::ImageView::new(expand_tilde(&path), method);
        let w = 90.min((self.w - 4).max(20));
        let h = 32.min((self.h - 4).max(8));
        let rect = Rect::new((self.w - w) / 2, 2, w, h);
        let label = view.title();
        let id_win = self.wm.add_window(label.clone(), rect);
        self.app_keys.insert(id_win, label.clone());
        self.contents.insert(id_win, WinContent::ImageView(Box::new(view)));
        self.titles.push((id_win, label));
    }

    /// Whether the working-directory picker overlay is open.
    pub fn dirpicker_open(&self) -> bool {
        self.dirpicker.is_some()
    }

    /// Whether `win`'s content rect is fully unobstructed by any higher window
    /// (used to decide if an image placement is visible).
    fn fully_unobstructed(&self, win: &crate::window::Window) -> bool {
        let cr = win.content_rect();
        !self.wm.z_ordered().iter().any(|o| {
            !o.minimized && o.z > win.z && o.rect.intersect(cr).is_some()
        })
    }

    /// PNG bytes for an image id currently held in the store (for the daemon's
    /// blob bookkeeping).
    pub fn image_png(&self, id: u64) -> Option<Vec<u8>> {
        self.images.png_bytes(id).map(|b| b.to_vec())
    }

    /// Load any not-yet-loaded PNGs that hosted apps have transmitted into the
    /// shared `ImageStore`, mapping each `(window, kitty_id)` to its store id.
    ///
    /// Called by the daemon once per frame, before [`build_frame`](Self::build_frame).
    /// `build_frame` itself is `&self` and only reads the resulting map, so the
    /// `&mut self` loading happens here.
    pub fn refresh_app_graphics(&mut self) {
        // First pass: collect the (window, kitty_id, png) we still need to load,
        // cloning the PNG bytes so the app's graphics lock is released before we
        // mutably borrow `self.images`.
        let mut needed: Vec<(WindowId, u32, Vec<u8>)> = Vec::new();
        for (id, content) in &self.contents {
            if let WinContent::App(aid) = content {
                for pl in self.apphost.placements(*aid) {
                    if self.app_image_ids.contains_key(&(*id, pl.image_id)) {
                        continue;
                    }
                    if let Some(png) = self.apphost.image_png(*aid, pl.image_id) {
                        needed.push((*id, pl.image_id, png));
                    }
                }
            }
        }
        // Second pass: load + record (no app graphics borrow held).
        let bound_w = (self.w.max(1) as u32) * 8;
        let bound_h = (self.h.max(1) as u32) * 16;
        for (win, kitty_id, png) in needed {
            if let Some(img) = self.images.load_bytes(&png, bound_w, bound_h) {
                self.app_image_ids.insert((win, kitty_id), img);
            }
        }
    }

    /// Push each app window's current geometry/state to the apphost as opaque
    /// `meta`, but only when it changed. Called once per frame by the daemon.
    /// For the in-process `LocalAppHost` this just updates a local map (cheap);
    /// for `RemoteAppHost` it sends `SetMeta` so a restarted frontend can restore.
    pub fn sync_app_meta(&mut self) {
        let mut updates: Vec<(AppId, WindowId, Vec<u8>)> = Vec::new();
        for w in self.wm.z_ordered() {
            if let Some(WinContent::App(aid)) = self.contents.get(&w.id) {
                let title = self
                    .titles
                    .iter()
                    .find(|(i, _)| *i == w.id)
                    .map(|(_, t)| t.clone())
                    .unwrap_or_default();
                let app_key = self.app_keys.get(&w.id).cloned().unwrap_or_default();
                let meta = WinMeta { rect: w.rect, title, z: w.z, minimized: w.minimized, app_key };
                let bytes = serde_json::to_vec(&meta).unwrap_or_default();
                if self.last_meta.get(&w.id) != Some(&bytes) {
                    updates.push((*aid, w.id, bytes));
                }
            }
        }
        for (aid, win, bytes) in updates {
            self.apphost.set_meta(aid, bytes.clone());
            self.last_meta.insert(win, bytes);
        }
    }

    /// Number of entries in the last-meta cache (for tests).
    #[doc(hidden)]
    pub fn app_meta_count_for_test(&self) -> usize {
        self.last_meta.len()
    }

    /// Inject a placement + image directly into the most-recently-launched app's
    /// graphics state, then refresh the image map so a subsequent `build_frame`
    /// emits it (integration tests).
    #[doc(hidden)]
    pub fn inject_app_graphics_for_test(&mut self, png: &[u8]) {
        let app_id = self
            .titles
            .iter()
            .rev()
            .map(|(id, _)| *id)
            .find_map(|id| match self.contents.get(&id) {
                Some(WinContent::App(aid)) => Some(*aid),
                _ => None,
            });
        if let Some(aid) = app_id {
            self.apphost.inject_test_image(aid, png);
        }
        self.refresh_app_graphics();
    }

    /// Rebuild a window for every app the apphost already owns (after a frontend
    /// reload or crash). Returns the number of windows restored. Apps with no
    /// stored `meta` are skipped (they will still be reaped/closed normally if
    /// the user never had a window for them).
    pub fn restore_windows_from_host(&mut self) -> usize {
        // Snapshot ids first so we don't borrow the host across mutations.
        let ids: Vec<AppId> = self.apphost.list();
        let mut restored = 0;
        for aid in ids {
            // Skip if we already have a window bound to this app.
            if self.contents.values().any(|c| matches!(c, WinContent::App(a) if *a == aid)) {
                continue;
            }
            let Some(bytes) = self.apphost.meta(aid) else { continue };
            let Ok(meta) = serde_json::from_slice::<WinMeta>(&bytes) else { continue };
            let id = self.wm.add_window(meta.title.clone(), meta.rect);
            if meta.minimized {
                self.wm.minimize(id);
            }
            // Use stored app_key, falling back to title for old meta blobs.
            let key = if meta.app_key.is_empty() { meta.title.clone() } else { meta.app_key.clone() };
            self.app_keys.insert(id, key);
            self.contents.insert(id, WinContent::App(aid));
            self.titles.push((id, meta.title));
            self.last_meta.insert(id, bytes);
            restored += 1;
        }
        if restored > 0 {
            self.auto_tile_if_enabled();
        }
        restored
    }

    /// Drop all window bookkeeping while leaving the apphost's apps alive — used
    /// to simulate a fresh frontend in restore tests.
    #[doc(hidden)]
    pub fn forget_windows_for_test(&mut self) {
        let ids: Vec<WindowId> = self.contents.keys().copied().collect();
        for id in ids {
            self.wm.close(id);
        }
        self.contents.clear();
        self.titles.clear();
        self.app_keys.clear();
        self.last_meta.clear();
    }

    /// Whether the picker's new-folder name input is active.
    /// The directory the picker's selection is on (None when it is closed).
    pub fn dirpicker_selected(&self) -> Option<std::path::PathBuf> {
        self.dirpicker.as_ref().map(|d| d.selected_path())
    }

    pub fn dirpicker_creating(&self) -> bool {
        self.dirpicker.as_ref().map(|d| d.is_creating()).unwrap_or(false)
    }

    /// Whether the keyboard-shortcut help window is open.
    pub fn help_open(&self) -> bool {
        self.help_win.map(|id| self.contents.contains_key(&id)).unwrap_or(false)
    }

    /// Whether the full-screen "simple" view mode is active.
    pub fn simple_mode(&self) -> bool { self.simple }

    /// The work-area rect a full-screen app fills in simple mode (everything
    /// below the top menubar/taskbar row).
    fn simple_content_rect(&self) -> crate::geometry::Rect {
        crate::geometry::Rect::new(0, 1, self.w.max(1), (self.h - 1).max(1))
    }

    /// Toggle between desktop and simple view. Resizes the focused app so it
    /// fills the screen (entering simple) or returns to its window size (leaving).
    pub fn toggle_simple(&mut self) {
        self.simple = !self.simple;
        // Re-sync every app window: in simple mode only the focused one becomes
        // full-screen (handled by sync_app_size); the rest go to their window size.
        let ids: Vec<WindowId> = self.contents.keys().copied().collect();
        for id in ids {
            self.sync_app_size(id);
        }
    }

    /// The launcher button's text: the configured label, normalised to 6 cells,
    /// with `@session` resolved to this session's name.
    fn launcher_label_now(&self) -> String {
        crate::chrome::launcher_label(&self.cfg.launcher_label, &crate::protocol::session_name())
    }

    /// The current tray layout (bar segments + Info rows) from the live snapshot.
    fn tray_layout_now(&self) -> crate::tray::TrayLayout {
        let st = self.tray_state.read().unwrap();
        let placement = crate::tray::Placement::from_config(&self.cfg.tray_placement);
        crate::tray::tray_layout(&st, self.w, 0, self.tray.notif_count(), self.clip_badge(), self.select_mode, &placement)
    }

    /// The current tray segments, laid out from the live snapshot.
    fn tray_segments_now(&self) -> Vec<crate::tray::Segment> {
        self.tray_layout_now().segments
    }

    /// Render the open tray popover (with the Info menu's session facts).
    fn tray_render_now(&self) -> crate::tray::Rendered {
        let info = crate::tray::InfoView {
            session: crate::protocol::session_name(),
            windows: self.window_count(),
            version: env!("CARGO_PKG_VERSION"),
            rows: self.tray_layout_now().in_info,
        };
        let st = self.tray_state.read().unwrap();
        self.tray.render_with(self.w, self.h, &st, &info)
    }

    /// Apply a tray control intent: optimistically update the cached snapshot so
    /// the UI responds immediately, then run the (timeout-guarded) backend call.
    fn apply_intent(&mut self, intent: crate::system::ControlIntent) {
        use crate::system::ControlIntent as I;
        // Calendar navigation and notifications are session/tray UI state, not
        // OS controls.
        match intent {
            I::CalendarPrev => return self.tray.calendar_step(-1),
            I::CalendarNext => return self.tray.calendar_step(1),
            I::NotifFocus(raw) => {
                let id = WindowId(raw);
                self.tray.clear_notifs_for(raw);
                self.tray.close();
                if self.contents.contains_key(&id) {
                    self.wm.unminimize(id);
                    if self.simple { self.sync_app_size(id); }
                }
                return;
            }
            I::NotifClear => {
                self.tray.clear_notifs();
                self.tray.close();
                return;
            }
            I::TetronSetActive { ref network, active } => {
                // Send to the tetron daemon (fire-and-forget; the next poll
                // reflects the real result), and optimistically flip local state
                // so the popover updates immediately. `ref network` avoids moving
                // out of `intent`, which the OS-control match below still reads.
                crate::system::tetron::set_active(network.clone(), active);
                if let Ok(mut s) = self.tray_state.write() {
                    match network {
                        None => {
                            s.tetron.active = active;
                            for n in &mut s.tetron.networks {
                                n.active = active;
                            }
                        }
                        Some(name) => {
                            if let Some(n) = s.tetron.networks.iter_mut().find(|n| &n.name == name) {
                                n.active = active;
                            }
                        }
                    }
                }
                return;
            }
            I::OpenTool { title, command, args } => {
                // A popover "Open <tool>" row: run it in a window.
                self.tray.close();
                self.launch_in(title, command, args, None);
                return;
            }
            I::OpenSegment(key) => {
                if let Some(kind) = crate::tray::SegmentKind::from_key(&key) {
                    self.tray.open_nested(kind);
                }
                return;
            }
            I::OpenSessions => {
                self.tray.close();
                // A live view of every session: re-run `tetron-wm ls` each second.
                // Absolute path (like the updater) so a bare `tetron-wm` missing
                // from the shell's PATH cannot break it.
                let exe = crate::self_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "tetron-wm".into());
                let exe = crate::systems::sh_quote(&exe);
                let script = format!("while :; do clear; {exe} ls; sleep 1; done");
                self.launch_in("sessions".into(), "sh".into(), vec!["-c".into(), script], None);
                return;
            }
            I::OpenLogs => {
                self.tray.close();
                self.open_logs();
                return;
            }
            I::OpenHelp => {
                self.tray.close();
                self.open_help();
                return;
            }
            I::StartScreensaver => {
                self.tray.close();
                self.screensaver_on(None);
                return;
            }
            I::OpenSettings => {
                self.tray.close();
                self.open_settings();
                return;
            }
            I::OpenStoreSearch(query) => {
                self.tray.close();
                self.open_store();
                if let Some(store) = self.store_win.and_then(|id| match self.contents.get_mut(&id) {
                    Some(WinContent::Store(st)) => Some(st),
                    _ => None,
                }) {
                    for c in query.chars() {
                        store.type_char(c);
                    }
                }
                return;
            }
            I::CopyText(text) => {
                self.tray.close();
                self.set_clipboard(&text);
                self.clip_record(&text, crate::clipboard::ClipSource::Copy);
                return;
            }
            I::TetronOpenTerminal => {
                // Prefer the native tetron-tui panel when installed; otherwise
                // fall back to showing status and dropping to an interactive
                // shell so the whole tetron CLI (invite/join/admin/…) is to hand.
                // A login shell (`-lc`) sources the profile so ~/.local/bin is
                // on PATH. `exec` replaces the shell so the window tracks the
                // app/session lifetime.
                self.tray.close();
                self.launch_in(
                    "Tetron".to_string(),
                    "sh".to_string(),
                    vec![
                        "-lc".to_string(),
                        "if command -v tetron-tui >/dev/null 2>&1; then exec tetron-tui; fi; \
                         tetron status; exec \"${SHELL:-sh}\" -i"
                            .to_string(),
                    ],
                    None,
                );
                return;
            }
            _ => {}
        }
        if let Ok(mut s) = self.tray_state.write() {
            match &intent {
                I::VolumeUp => s.volume.level = s.volume.level.saturating_add(5).min(100),
                I::VolumeDown => s.volume.level = s.volume.level.saturating_sub(5),
                I::VolumeSet(l) => s.volume.level = (*l).min(100),
                I::ToggleMute => s.volume.muted = !s.volume.muted,
                I::BrightnessUp => s.brightness = s.brightness.map(|p| crate::brightness::step(p, true)),
                I::BrightnessDown => s.brightness = s.brightness.map(|p| crate::brightness::step(p, false)),
                I::BrightnessSet(p) => s.brightness = s.brightness.map(|_| (*p).clamp(crate::brightness::MIN_PCT, 100)),
                I::WifiSetEnabled(on) => { if let Some(w) = s.wifi.as_mut() { w.enabled = *on; } }
                I::BtSetEnabled(on) => s.bluetooth.enabled = *on,
                _ => {}
            }
        }
        self.backend.apply(&intent);
    }

    /// Attach the daemon's [`crate::system::Interest`]: the session publishes
    /// which polled parts the UI shows each frame.
    pub fn attach_interest(&mut self, interest: std::sync::Arc<crate::system::Interest>) {
        self.interest = Some(interest);
    }

    /// Tell the poller which shell-backed data is on screen: Wi-Fi/net when the
    /// segment is on the bar or its popover (or the Info menu listing it) is
    /// open; Bluetooth when its segment is on the bar or its popover is open.
    fn publish_interest(&self) {
        use crate::tray::SegmentKind as K;
        let Some(i) = &self.interest else { return };
        let l = self.tray_layout_now();
        let open = self.tray.open();
        let on_bar = |k: K| l.segments.iter().any(|s| s.kind == k);
        let listed = |k: K| l.in_info.iter().any(|r| r.kind == k);
        // A segment only exists once its data has been read, so until it does
        // (first frames, or a link that came back) keep reading: otherwise a
        // segment that is not yet on the bar could never appear.
        let net = on_bar(K::Wifi)
            || open == Some(K::Wifi)
            || (open == Some(K::Info) && listed(K::Wifi))
            || !(on_bar(K::Wifi) || listed(K::Wifi));
        let bt = on_bar(K::Bluetooth)
            || open == Some(K::Bluetooth)
            || !(on_bar(K::Bluetooth) || listed(K::Bluetooth));
        i.set(net, bt);
    }

    /// Attach the daemon's shared system snapshot (written by the `SystemPoller`)
    /// so the menubar tray reflects live host state.
    pub fn attach_tray_state(
        &mut self,
        state: std::sync::Arc<std::sync::RwLock<crate::system::SystemState>>,
    ) {
        self.tray_state = state;
    }

    /// Whether `tetron-wm kill` requested a full daemon shutdown.
    pub fn shutdown_requested(&self) -> bool { self.shutdown }

    /// Whether a frontend-only reload was requested (apps stay alive).
    pub fn reload_requested(&self) -> bool { self.reload }

    /// Number of apps the host currently owns (regardless of whether the
    /// frontend has a window for each). The daemon uses this to decide whether
    /// this is a fresh start (auto-launch configured apps) or a reload/recovery
    /// against an apphost that already has apps (restore, don't re-launch).
    pub fn host_app_count(&self) -> usize {
        self.apphost.list().len()
    }

    /// Whether every app window has its grid available yet. After a reload the
    /// daemon waits on this so the first painted frame already shows app content
    /// (the apphost streams frames asynchronously into the host cache, so a
    /// freshly-restored window is briefly blank until its first frame lands).
    /// `true` when there are no app windows.
    pub fn app_windows_ready(&self) -> bool {
        self.contents.values().all(|c| match c {
            WinContent::App(aid) => self.apphost.snapshot(*aid).is_some(),
            _ => true,
        })
    }

    /// Clear the detach (quit) flag — called by the daemon after a client detaches
    /// so the next client doesn't immediately detach again.
    pub fn clear_quit(&mut self) {
        self.quit = false;
        self.switch_to = None;
        self.fullscreen = None;
        if std::mem::take(&mut self.refresh_waiter) {
            let st = crate::fullscreen::service_state();
            if let Some(s) = self.focused_settings_mut() {
                s.set_waiter(st);
            }
        }
    }

    /// The colour depth in use: `TETRON_WM_COLORS` (the client says it was
    /// forced), else the `colors` setting, else what the client detected.
    fn effective_caps(&self) -> crate::display::ClientCaps {
        let c = self.client_caps;
        match crate::display::ColorDepth::parse(&self.cfg.colors) {
            Some(colors) if !c.colors_forced => crate::display::ClientCaps { colors, ..c },
            _ => c,
        }
    }

    /// The depth the client must render with, when the setting overrides its
    /// own detection (sent in every frame's flags).
    pub fn colors_override(&self) -> Option<crate::display::ColorDepth> {
        let e = self.effective_caps().colors;
        (e != self.client_caps.colors).then_some(e)
    }

    /// Put the effective display facts in force. The theme adapts on the next
    /// frame by itself (`theme::current` reads the depth); the wallpaper bakes
    /// in chafa's colour mode and symbol set, so it is rebuilt, and
    /// `icon_style = "auto"` follows the display too (ascii on a VT).
    fn apply_display(&mut self) {
        if crate::display::set(self.effective_caps()) {
            crate::iconset::refresh();
            self.wallpaper = Self::build_wallpaper(self.w, self.h, &self.cfg);
            self.repaint = true;
        }
    }

    /// Whether the client's screen must be re-baselined (the depth changed, so
    /// every cell quantizes differently, not only the ones the diff sees).
    pub fn take_repaint(&mut self) -> bool {
        std::mem::take(&mut self.repaint)
    }

    /// The pending full-screen program, shipped with the detach.
    pub fn fullscreen_spec(&self) -> Option<Vec<String>> {
        self.fullscreen.clone()
    }

    /// Show `files` full screen, first one first: images (or one PDF). The
    /// client detaches, `fullscreen::show` runs the route this machine has
    /// (fbi, fim, fbgs or mpv, straight on a plain console, through the waiter
    /// under kmscon) and the client re-attaches when it ends.
    fn start_fullscreen(&mut self, files: Vec<std::path::PathBuf>) {
        let Some(first) = files.first() else { return };
        if crate::imageview::is_pdf(first) {
            self.fullscreen_show(crate::fullscreen::Kind::Pdf, vec![first.clone()]);
        } else {
            self.fullscreen_show(crate::fullscreen::Kind::Images, files);
        }
    }

    /// Hand `files` to the client to show full screen, when a route exists.
    fn fullscreen_show(&mut self, kind: crate::fullscreen::Kind, files: Vec<std::path::PathBuf>) {
        let caps = crate::display::get();
        if !crate::fullscreen::offered(kind, &caps, &crate::catalog::is_installed) {
            crate::dbg_log(&format!("fullscreen: no route for {} (client {caps:?})", kind.name()));
            return;
        }
        let mut argv = vec!["@fullscreen".to_string(), kind.name().to_string()];
        argv.extend(files.iter().map(|p| p.to_string_lossy().into_owned()));
        crate::dbg_log(&format!("fullscreen: handing {} file(s) ({}) to the client", files.len(), kind.name()));
        self.hand_over(argv);
    }

    /// Detach the client so it runs `argv` on the real console, then reattaches.
    fn hand_over(&mut self, argv: Vec<String>) {
        self.fullscreen = Some(argv);
        self.quit = true;
    }

    /// Right-click "Play full screen": the route this machine has (mpv over
    /// DRM, else cvlc on the framebuffer). mpv's own keys work (q quits).
    fn fullscreen_video(&mut self, path: &std::path::Path) {
        self.fullscreen_show(crate::fullscreen::Kind::Video, vec![path.to_path_buf()]);
    }

    /// Run `script` in a new window in `dir`, keeping the window open with the
    /// outcome until Enter, so a failure can be read.
    fn run_in_window(&mut self, name: String, script: String, dir: std::path::PathBuf) {
        crate::dbg_log(&format!("run in window: {script}"));
        let wrapped = format!(
            "{script}; s=$?; echo; if [ $s -eq 0 ]; then echo Done.; else echo \"Failed (exit $s).\"; fi; printf 'Press Enter to close. '; read _"
        );
        self.launch_in(name, "sh".into(), vec!["-c".into(), wrapped], Some(dir));
    }

    /// Right-click "Extract here" / "List contents" on an archive.
    fn archive(&mut self, path: &std::path::Path, list: bool) {
        let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let Some(cmd) = crate::contextmenu::archive_command(path, &dir, list) else { return };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if list && !crate::contextmenu::archive_prompts(path) {
            crate::dbg_log(&format!("archive list: {cmd}"));
            self.launch_in(name, "sh".into(), vec!["-c".into(), format!("{cmd} 2>&1 | less")], Some(dir));
        } else {
            // A password prompt needs the tty to itself (no `less` reading it too);
            // the window keeps the listing in its scrollback.
            self.run_in_window(format!("{} {name}", if list { "list" } else { "extract" }), cmd, dir);
        }
    }

    /// Right-click "Compress…" / "Encrypt…" > a format: pack the entry into a
    /// new archive beside it, in a window so the tool can ask for the password.
    fn pack(&mut self, path: &std::path::Path, pack: crate::contextmenu::Pack) {
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else { return };
        let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        let out = crate::contextmenu::unique_with_ext(&dir, &name, pack.ext());
        let out = out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let cmd = crate::contextmenu::PackTools::current().command(pack, &name, &out);
        crate::dbg_log(&format!("pack {pack:?}: in {} run {cmd}", dir.display()));
        self.run_in_window(format!("{} {name}", if pack.encrypted() { "encrypt" } else { "compress" }), cmd, dir);
    }

    /// Right-click "Send to…" a Systems entry: scp to its home directory, in a
    /// window so password and host-key prompts work.
    fn send_to(&mut self, system: &str, path: &std::path::Path) {
        let Some(sys) = self.systems.iter().find(|s| s.name == system).cloned() else { return };
        let q = |s: &str| crate::systems::sh_quote(s);
        let port = sys.port.map(|p| format!("-P {p} ")).unwrap_or_default();
        let r = if path.is_dir() { "-r " } else { "" };
        let cmd = format!("scp {port}{r}{} {}", q(&path.to_string_lossy()), q(&format!("{}:", sys.host)));
        let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        self.run_in_window(format!("send to {system}"), cmd, dir);
    }

    /// Right-click "Cast to Chromecast": catt serves the file and stays up until
    /// playback ends, so it runs in a window (closing the window stops the cast).
    fn cast_file(&mut self, path: &std::path::Path) {
        let cmd = crate::contextmenu::cast_command(path, &self.cfg.cast_device);
        crate::dbg_log(&format!("cast: {cmd}"));
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        self.run_in_window(format!("cast {name}"), cmd, dir);
    }

    /// Desktop "Get Info": open Files at the item's folder with its info box.
    fn desktop_get_info(&mut self, path: &std::path::Path) {
        let Some(dir) = path.parent() else { return };
        self.open_filemanager_root(dir.to_path_buf());
        if let Some(f) = self.focused_filemanager_mut() {
            f.show_info_for(path);
        }
    }

    /// Right-click "View full screen" on an image: it and the folder's others.
    fn fullscreen_path(&mut self, path: &std::path::Path) {
        if crate::imageview::is_pdf(path) {
            self.start_fullscreen(vec![path.to_path_buf()]);
            return;
        }
        let files = crate::imageview::starting_at(&crate::imageview::siblings(path), path);
        self.start_fullscreen(files);
    }

    /// Right-click "Set as wallpaper": save it to the config and redraw.
    fn set_wallpaper_path(&mut self, path: &std::path::Path) {
        crate::dbg_log(&format!("wallpaper: set to {} from the menu", path.display()));
        self.cfg.wallpaper = Some(path.to_string_lossy().into_owned());
        self.cfg.wallpaper_enabled = true;
        if let Err(e) = self.cfg.save() {
            crate::log_warn(&format!("config: save failed: {e}"));
        }
        self.wallpaper = Self::build_wallpaper(self.w, self.h, &self.cfg);
    }

    /// Right-click "Copy path": into the clipboard (and history), and out to
    /// the host terminal via OSC 52.
    fn copy_path(&mut self, path: &std::path::Path) {
        let text = path.to_string_lossy().into_owned();
        self.set_clipboard(&text);
        self.pending_clipboard = Some(text.clone());
        self.clip_record(&text, crate::clipboard::ClipSource::Copy);
    }

    /// Right-click "Open terminal here": a shell in the folder (a file's own folder).
    fn terminal_at(&mut self, path: &std::path::Path) {
        let dir = if path.is_dir() { path.to_path_buf() } else { path.parent().map(|p| p.to_path_buf()).unwrap_or_default() };
        let shell = std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "bash".into());
        let name = std::path::Path::new(&shell).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| shell.clone());
        self.launch_in(name, shell, Vec::new(), Some(dir));
    }

    /// The pending system switch, if the user picked one in the Systems menu.
    /// Shipped to the client in the frame that also carries the detach flag.
    pub fn switch_spec(&self) -> Option<crate::systems::SwitchSpec> {
        self.switch_to.clone()
    }

    /// Whether the power menu's Add Remote form is open (the client forwards
    /// typed characters / field navigation to it).
    pub fn power_form_editing(&self) -> bool {
        self.power_menu.form_open()
    }

    /// Carry out a confirmed power-menu outcome (button click or form submit).
    /// Open the confirm dialog for `action`, naming this computer and how many
    /// running apps it would end.
    fn confirm_power(&mut self, action: crate::powermenu::PowerAction) {
        if !self.cfg.power_menu {
            crate::dbg_log("power: refused, power_menu = false");
            return;
        }
        self.launcher.close();
        let apps = self.contents.values().filter(|c| matches!(c, WinContent::App(_))).count();
        self.power_menu.set_context(crate::powerctl::hostname(), apps);
        self.power_menu.confirm_action(action);
    }

    /// Run a confirmed suspend / reboot / power off on a worker thread (the
    /// commands can take seconds, a suspend blocks until resume) so the render
    /// loop never stalls. The clipboard history is flushed first on reboot and
    /// power off; apps are left alone, so a refused command loses nothing.
    fn start_power(&mut self, kind: crate::powerctl::Kind) {
        if !self.cfg.power_menu {
            crate::dbg_log(&format!("power: {} refused, power_menu = false", kind.key()));
            return;
        }
        if self.power_job.is_some() {
            crate::dbg_log(&format!("power: {} ignored, another power action is running", kind.key()));
            return;
        }
        crate::dbg_log(&format!("power: {} confirmed", kind.key()));
        if kind != crate::powerctl::Kind::Suspend {
            self.flush_clipboard();
        }
        let slot = std::sync::Arc::new(std::sync::Mutex::new(None));
        let out = std::sync::Arc::clone(&slot);
        let cfg = self.cfg.power_commands.clone();
        std::thread::spawn(move || {
            let report = crate::powerctl::run(kind, &cfg, &crate::catalog::is_installed);
            *out.lock().unwrap_or_else(|e| e.into_inner()) = Some(report);
        });
        self.power_job = Some((kind, slot));
    }

    /// Collect a finished power action (call once per daemon tick). A refused or
    /// impossible one opens a window saying what was tried, instead of failing
    /// silently; an accepted one needs nothing (the system is acting on it).
    pub fn poll_power(&mut self) {
        let Some((kind, slot)) = self.power_job.as_ref() else { return };
        let Some(report) = slot.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
        let kind = *kind;
        self.power_job = None;
        if report.started.is_none() {
            let text = crate::powerctl::failure_text(kind, &report);
            let script = format!(
                "printf '%s' {}; printf '\nPress Enter to close. '; read _k",
                crate::systems::sh_quote(&text)
            );
            self.launch_in(kind.label().to_string(), "sh".into(), vec!["-c".into(), script], None);
        }
    }

    /// A screenshot that could not be written opens a window saying why, so
    /// leader s never fails silently (the reason used to reach only the log).
    pub fn report_screenshot_failure(&mut self, path: &str, err: &str) {
        let text = format!("Screenshot not saved.\n\n  {path}\n    {err}\n");
        let script = format!("printf '%s' {}; printf '\nPress Enter to close. '; read _k", crate::systems::sh_quote(&text));
        self.launch_in("Screenshot".into(), "sh".into(), vec!["-c".into(), script], None);
    }

    fn apply_power_outcome(&mut self, outcome: PowerOutcome) {
        match outcome {
            PowerOutcome::Detach => self.quit = true,
            PowerOutcome::Reload => self.reload = true,
            PowerOutcome::Shutdown => self.shutdown = true,
            PowerOutcome::Power(kind) => self.start_power(kind),
            PowerOutcome::Switch(spec) => {
                crate::dbg_log(&format!("systems: switch to '{}' ({})", spec.name, spec.host));
                self.switch_to = Some(spec);
                self.quit = true; // detach locally; apps keep running
            }
            PowerOutcome::AddAndConnect { system, password } => {
                let mut spec = crate::systems::SwitchSpec::connect(&system);
                spec.setup = true;
                spec.password = password; // setup-only; never persisted
                crate::dbg_log(&format!(
                    "systems: add '{}' ({}) theme={:?} → saving + first-time setup",
                    system.name, system.host, system.theme
                ));
                self.systems.retain(|s| s.name != system.name);
                self.systems.push(system);
                if let Err(e) = crate::systems::save(&self.systems) {
                    crate::log_warn(&format!("systems: save failed: {e}"));
                }
                self.switch_to = Some(spec);
                self.quit = true;
            }
            PowerOutcome::Forget { name, revoke } => {
                crate::dbg_log(&format!("systems: forget '{name}' (revoke={})", revoke.is_some()));
                self.systems.retain(|s| s.name != name);
                if let Err(e) = crate::systems::save(&self.systems) {
                    crate::log_warn(&format!("systems: save failed: {e}"));
                }
                // Optionally strip our key from the remote — best-effort, on a
                // background thread so an unreachable host can't stall the UI.
                if let Some(sys) = revoke {
                    Self::revoke_remote_key(sys);
                }
            }
        }
    }

    /// Strip this machine's SSH key from a forgotten system's remote
    /// `authorized_keys` (the inverse of the key copy that adding it performed).
    /// Runs on a detached thread: ssh can take seconds or hang on an unreachable
    /// host, and the render loop must never wait on the network. Best-effort —
    /// every outcome is logged for the in-app Logs viewer, nothing is surfaced.
    fn revoke_remote_key(sys: crate::systems::RemoteSystem) {
        std::thread::spawn(move || {
            crate::dbg_log(&format!("systems: revoking key on '{}' ({})", sys.name, sys.host));
            let script = crate::systems::revoke_script(&sys.host, sys.port);
            match crate::system::run_capped("sh", &["-c", &script], 15) {
                Some(out) => {
                    let tail = out.trim();
                    crate::dbg_log(&format!(
                        "systems: revoke on '{}' done{}",
                        sys.name,
                        if tail.is_empty() { String::new() } else { format!(" — {tail}") }
                    ));
                }
                None => crate::dbg_log(&format!(
                    "systems: revoke on '{}' did not complete (host unreachable, auth refused, \
or a remote-side error — its authorized_keys was left untouched)",
                    sys.name
                )),
            }
        });
    }

    /// Build the launcher's app list: the configured apps (with categories filled
    /// in from the catalog where missing), plus any known TUIs detected on `$PATH`
    /// that aren't already listed.
    fn build_launcher_apps(cfg: &Config, systems: &[crate::systems::RemoteSystem]) -> Vec<AppEntry> {
        // Pinned tetron-wm actions first (open the store / settings windows).
        let mut apps = vec![
            AppEntry { name: "Store".into(), command: "@store".into(), args: vec![], category: Some("tetron-wm".into()), requires_cwd: None, cwd: None, cli: None, warn: None },
            AppEntry { name: "Settings".into(), command: "@settings".into(), args: vec![], category: Some("tetron-wm".into()), requires_cwd: None, cwd: None, cli: None, warn: None },
            AppEntry { name: "Files".into(), command: "@files".into(), args: vec![], category: Some("tetron-wm".into()), requires_cwd: None, cwd: None, cli: None, warn: None },
            AppEntry { name: "Logs".into(), command: "@logs".into(), args: vec![], category: Some("tetron-wm".into()), requires_cwd: None, cwd: None, cli: None, warn: None },
            AppEntry { name: "Activity".into(), command: "@activity".into(), args: vec![], category: Some("tetron-wm".into()), requires_cwd: None, cwd: None, cli: None, warn: None },
            AppEntry { name: "Clipboard".into(), command: "@clipboard".into(), args: vec![], category: Some("tetron-wm".into()), requires_cwd: None, cwd: None, cli: None, warn: None },
            AppEntry { name: "Screenshot".into(), command: "@screenshot".into(), args: vec![], category: Some("tetron-wm".into()), requires_cwd: None, cwd: None, cli: None, warn: None },
            AppEntry { name: "Screensaver".into(), command: "@screensaver".into(), args: vec![], category: Some("tetron-wm".into()), requires_cwd: None, cwd: None, cli: None, warn: None },
            AppEntry { name: "Help".into(), command: "@help".into(), args: vec![], category: Some("tetron-wm".into()), requires_cwd: None, cwd: None, cli: None, warn: None },
        ];
        // Suspend / Reboot / Power off: the launcher's bottom "Power" submenu,
        // only for actions this computer can actually carry out.
        for kind in crate::powerctl::Kind::ALL {
            if cfg.power_menu && crate::powerctl::available(kind, &cfg.power_commands, &crate::catalog::is_installed) {
                apps.push(AppEntry {
                    name: kind.label().into(),
                    command: kind.command().into(),
                    args: vec![],
                    category: Some(crate::launcher::POWER_CATEGORY.into()),
                    requires_cwd: None,
                    cwd: None,
                    cli: None,
                    warn: None,
                });
            }
        }
        // One remote file browser per saved system (Systems category).
        for sys in systems {
            apps.push(AppEntry {
                name: format!("Files on {}", sys.name),
                command: "@rfiles".into(),
                args: vec![sys.name.clone()],
                category: Some("Systems".into()),
                requires_cwd: None,
                cwd: None,
                cli: None,
                warn: None,
            });
        }
        apps.extend(cfg.launcher_apps());
        for a in &mut apps {
            if a.category.is_none() {
                a.category = crate::catalog::category_for(&a.name)
                    .or_else(|| crate::catalog::category_for(&a.command));
            }
            // Backfill the cwd-prompt flag from the catalog (by name, then binary)
            // when a config entry doesn't set it explicitly, so a known cwd-app
            // gets its working-directory picker without the user remembering the flag.
            if a.requires_cwd.is_none() {
                a.requires_cwd = crate::catalog::requires_cwd_for(&a.name)
                    .or_else(|| crate::catalog::requires_cwd_for(&a.command));
            }
            // Backfill the CLI flag from the catalog the same way, so a config
            // entry that just names a known CLI tool still gets the badge and
            // the `sh -lc` launch wrapper without repeating the flag by hand.
            if a.cli.is_none() {
                a.cli = Some(
                    crate::catalog::is_cli(&a.name) || crate::catalog::is_cli(&a.command),
                );
            }
        }
        for detected in crate::catalog::detect_installed() {
            if !apps.iter().any(|a| a.name.eq_ignore_ascii_case(&detected.name)) {
                apps.push(detected);
            }
        }
        apps
    }

    /// Re-scan `$PATH` and rebuild the launcher's app list when it is about to
    /// open, so a newly-installed app appears without a daemon restart.
    fn refresh_launcher_if_closed(&mut self) {
        if !self.launcher.is_open() {
            crate::catalog::refresh_installed();
            self.launcher.set_items(Self::build_launcher_apps(&self.cfg, &self.systems));
        }
    }

    /// Whether the launcher (menu or Spotlight) is currently open.
    pub fn launcher_open(&self) -> bool {
        self.launcher.is_open()
    }

    /// Whether the launcher is open (integration tests).
    #[doc(hidden)]
    pub fn launcher_open_for_test(&self) -> bool {
        self.launcher.is_open()
    }

    /// Whether the Spotlight overlay specifically is open (the loop routes typed
    /// characters to the query only in this mode).
    pub fn spotlight_open(&self) -> bool {
        self.launcher.mode() == Some(crate::launcher::LauncherMode::Spotlight)
    }

    /// Whether the user has requested quit (Exit from the power menu).
    /// The render loop polls this each tick and exits when it returns `true`.
    pub fn quit_requested(&self) -> bool { self.quit }

    /// Whether the top-right power menu (dropdown or confirm dialog) is open.
    pub fn power_menu_open(&self) -> bool { self.power_menu.is_open() }
    pub fn power_dialog_open(&self) -> bool { self.power_menu.dialog_open() }

    /// Whether the confirm-close dialog is showing (the client routes Enter/Esc
    /// and y/n to it while open).
    pub fn confirm_close_open(&self) -> bool { self.confirm_close.is_open() }

    /// Whether the window picker is showing (the client routes its keys to it).
    pub fn window_picker_open(&self) -> bool { self.win_picker.is_open() }

    /// Whether the volume popover is open (the client routes its keys to it,
    /// whether a click or leader `=` opened it).
    pub fn volume_popover_open(&self) -> bool {
        self.tray.open() == Some(crate::tray::SegmentKind::Volume)
    }

    /// Leader `=`: toggle the volume popover under its menubar segment, closing
    /// any other popover or menu first so only one thing has the keys.
    /// Leader `i`: open (or close) the Info menu under its menubar segment.
    fn info_open(&mut self) {
        if self.info_popover_open() {
            self.tray.close();
            return;
        }
        self.launcher.close();
        self.power_menu.close();
        let x = self
            .tray_segments_now()
            .iter()
            .find(|s| s.kind == crate::tray::SegmentKind::Info)
            .map(|s| s.rect.x)
            .unwrap_or(self.w);
        self.tray.open_at(crate::tray::SegmentKind::Info, x);
    }

    /// Whether the Info menu is open (the client routes arrows to it).
    pub fn info_popover_open(&self) -> bool {
        self.tray.open() == Some(crate::tray::SegmentKind::Info)
    }

    /// Whether the brightness popover is open (the client routes its keys to it).
    pub fn brightness_popover_open(&self) -> bool {
        self.tray.open() == Some(crate::tray::SegmentKind::Brightness)
    }

    /// Leader `-`: toggle the brightness popover under its segment. Without a
    /// backlight there is no segment and nothing opens.
    fn brightness_open(&mut self) {
        if self.brightness_popover_open() {
            self.tray.close();
            return;
        }
        let Some(x) = self
            .tray_segments_now()
            .iter()
            .find(|s| s.kind == crate::tray::SegmentKind::Brightness)
            .map(|s| s.rect.x)
        else {
            return;
        };
        self.launcher.close();
        self.power_menu.close();
        self.tray.open_at(crate::tray::SegmentKind::Brightness, x);
    }

    fn volume_open(&mut self) {
        if self.volume_popover_open() {
            self.tray.close();
            return;
        }
        self.launcher.close();
        self.power_menu.close();
        let x = self
            .tray_segments_now()
            .iter()
            .find(|s| s.kind == crate::tray::SegmentKind::Volume)
            .map(|s| s.rect.x)
            .unwrap_or(self.w);
        self.tray.open_at(crate::tray::SegmentKind::Volume, x);
    }

    /// Whether the keyboard move mode is active on a window that still exists.
    pub fn move_mode_open(&self) -> bool {
        self.move_mode.as_ref().is_some_and(|m| self.wm.get(m.id).is_some())
    }

    /// Whether the move mode is resizing rather than moving (for tests).
    pub fn move_mode_resizing(&self) -> bool {
        self.move_mode.as_ref().is_some_and(|m| m.resize)
    }

    /// Leader w: start moving the focused window from the keyboard. The start
    /// geometry is kept so Esc can put it back. Minimized or no window: no-op.
    fn move_mode_start(&mut self) {
        let Some(id) = self.wm.focused() else { return };
        let Some(w) = self.wm.get(id) else { return };
        if w.minimized {
            return;
        }
        self.move_mode = Some(MoveMode { id, start: w.rect, start_state: w.state, resize: false });
    }

    /// One arrow press in move mode. Moving goes through `move_to` (clamped so
    /// the titlebar stays reachable, like a mouse drag); resizing moves the
    /// right / bottom edge and resizes the app's PTY to match.
    fn move_mode_step(&mut self, dx: i32, dy: i32) {
        let Some(m) = self.move_mode.as_ref() else { return };
        let (id, resize) = (m.id, m.resize);
        let Some(r) = self.wm.get(id).map(|w| w.rect) else {
            self.move_mode = None;
            return;
        };
        if resize {
            self.wm.resize_to(id, r.w + dx, r.h + dy);
            self.sync_app_size(id);
        } else {
            self.wm.move_to(id, r.x + dx, r.y + dy);
        }
    }

    /// Enter keeps the new geometry; Esc restores the start rect and state
    /// (a maximized or snapped window comes back maximized or snapped).
    fn move_mode_end(&mut self, cancel: bool) {
        let Some(m) = self.move_mode.take() else { return };
        if cancel && self.wm.get(m.id).is_some() {
            self.wm.set_geometry(m.id, m.start, m.start_state);
            self.sync_app_size(m.id);
        }
    }

    /// Window ids in taskbar order (the order `titles` keeps: creation order),
    /// limited to windows the window manager still knows about.
    fn window_order(&self) -> Vec<WindowId> {
        self.titles.iter().map(|(id, _)| *id).filter(|id| self.wm.get(*id).is_some()).collect()
    }

    /// Focus, raise and (if minimized) restore window `id`. The one place the
    /// keyboard focus commands land, so they behave like a taskbar click.
    fn focus_window(&mut self, id: WindowId) {
        self.wm.unminimize(id);
        if self.simple {
            self.sync_app_size(id);
        }
    }

    /// Leader Tab / Shift+Tab: step focus through the windows in taskbar order,
    /// wrapping. Minimized windows are skipped (stepping past them would pop each
    /// one back up); the picker reaches those. If every window is minimized the
    /// step runs over all of them instead, so the key still does something.
    fn focus_step(&mut self, forward: bool) {
        let all = self.window_order();
        let visible: Vec<WindowId> =
            all.iter().copied().filter(|id| self.wm.get(*id).is_some_and(|w| !w.minimized)).collect();
        let ring = if visible.is_empty() { all } else { visible };
        if ring.is_empty() {
            return;
        }
        let n = ring.len();
        let next = match self.wm.focused().and_then(|f| ring.iter().position(|id| *id == f)) {
            Some(i) if forward => ring[(i + 1) % n],
            Some(i) => ring[(i + n - 1) % n],
            // Focus is on nothing in the ring (empty desktop): start at an end.
            None if forward => ring[0],
            None => ring[n - 1],
        };
        self.focus_window(next);
    }

    /// Open the window picker over every window in taskbar order, with the
    /// focused window highlighted.
    fn open_window_picker(&mut self) {
        let order = self.window_order();
        let focused = self.wm.focused();
        let sel = order.iter().position(|id| Some(*id) == focused).unwrap_or(0);
        let rows = order
            .iter()
            .filter_map(|id| {
                let w = self.wm.get(*id)?;
                let label = self.titles.iter().find(|(i, _)| i == id).map(|(_, l)| l.clone()).unwrap_or_else(|| w.title.clone());
                Some(crate::winpicker::PickRow { id: *id, label, minimized: w.minimized })
            })
            .collect();
        self.win_picker.open(rows, sel);
    }

    /// Whether the launch-warning dialog is showing (the client routes
    /// Enter/Esc and y/n to it while open).
    pub fn launch_warn_open(&self) -> bool { self.launch_warn.is_open() }

    /// Return the number of live windows (app instances spawned successfully).
    pub fn window_count(&self) -> usize { self.contents.len() }

    /// Test: force-open `kind`'s tray popover and return the hit rect for its
    /// `TetronOpenTerminal` row, if present (used to drive a click at that cell).
    pub fn tray_openterm_rect_for_test(
        &mut self,
        kind: crate::tray::SegmentKind,
    ) -> Option<crate::geometry::Rect> {
        self.tray.force_open(kind);
        let st = self.tray_state.read().unwrap();
        self.tray
            .render(self.w, self.h, &st)
            .hits
            .into_iter()
            .find(|h| matches!(h.intent, crate::system::ControlIntent::TetronOpenTerminal))
            .map(|h| h.rect)
    }

    /// Return the currently focused [`WindowId`], if any.
    pub fn focused(&self) -> Option<WindowId> { self.wm.focused() }

    /// Test helper: the focused window's outer rect (titlebar + borders).
    #[doc(hidden)]
    pub fn focused_window_rect_for_test(&self) -> Option<crate::geometry::Rect> {
        self.wm.focused().and_then(|id| self.wm.get(id)).map(|w| w.rect)
    }

    /// Return the screen-space hit regions for every taskbar pill.
    ///
    /// Each tuple is `(pill_index, Rect)` where the rect is a 1-row slice on the
    /// top menubar row.  Used by callers that need to detect taskbar clicks
    /// without going through the full mouse-routing path.
    pub fn dock_regions(&self) -> Vec<(usize, Rect)> {
        let items = self.dock_items();
        let segs = self.tray_segments_now();
        menubar_taskbar_regions(self.w, &items, &segs)
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    /// Build the current list of dock pills, grouping windows by app_key.
    fn dock_items(&self) -> Vec<DockItem> {
        let focused = self.wm.focused();
        let mut order: Vec<String> = Vec::new();
        let mut groups: std::collections::HashMap<String, Vec<WindowId>> = std::collections::HashMap::new();
        for (id, _) in &self.titles {
            let key = self.app_keys.get(id).cloned().unwrap_or_else(|| {
                self.titles.iter().find(|(i, _)| i == id).map(|(_, t)| t.clone()).unwrap_or_default()
            });
            if !groups.contains_key(&key) { order.push(key.clone()); }
            groups.entry(key).or_default().push(*id);
        }
        order.into_iter().map(|key| {
            let wins = groups.remove(&key).unwrap_or_default();
            let badge = crate::badge::badge_for(&key, &self.cfg.dock_badges);
            let is_focused = wins.iter().any(|w| Some(*w) == focused);
            let attention = wins.iter().any(|w| self.tray.has_notif_for(w.0));
            if wins.len() == 1 {
                let id = wins[0];
                let label = self.titles.iter().find(|(i, _)| *i == id).map(|(_, t)| t.clone()).unwrap_or_else(|| key.clone());
                DockItem { kind: DockKind::Single(id), label, count: 1, badge_letter: badge.letter, badge_color: badge.color, focused: is_focused, attention }
            } else {
                let count = wins.len();
                DockItem { kind: DockKind::Group(key.clone(), wins), label: key, count, badge_letter: badge.letter, badge_color: badge.color, focused: is_focused, attention }
            }
        }).collect()
    }

    /// Test helper: whether the dock popup (group chooser or …+N list) is open.
    #[doc(hidden)]
    pub fn dock_popup_open_for_test(&self) -> bool {
        self.dock_popup.is_some()
    }

    /// Test helper: number of dock pills (after grouping).
    #[doc(hidden)]
    pub fn dock_pill_count_for_test(&self) -> usize {
        self.dock_items().len()
    }

    /// Test helper: the full dock-item list (after grouping).
    #[doc(hidden)]
    pub fn dock_items_for_test(&self) -> Vec<DockItem> {
        self.dock_items()
    }

    /// Test helper: the focused window's current dock/title label.
    #[doc(hidden)]
    pub fn focused_label_for_test(&self) -> String {
        self.wm
            .focused()
            .and_then(|id| self.titles.iter().find(|(i, _)| *i == id))
            .map(|(_, t)| t.clone())
            .unwrap_or_default()
    }

    /// Test helper: the focused window's actual spawned command + args, as
    /// recorded by the (in-process) apphost — lets launch tests assert on the
    /// real `sh -lc …` wrapper rewrite for CLI-flagged apps.
    #[doc(hidden)]
    pub fn focused_app_launch_cmd_for_test(&self) -> Option<(String, Vec<String>)> {
        let id = self.wm.focused()?;
        match self.contents.get(&id)? {
            WinContent::App(aid) => Some(self.apphost_launch_cmd(*aid)),
            _ => None,
        }
    }

    /// Rows for the "…+N" overflow list: every window of every taskbar pill that
    /// is hidden behind the marker (badge, label per window).
    fn overflow_popup_rows(&self) -> Vec<(WindowId, char, crate::cell::Rgba, String)> {
        let items = self.dock_items();
        let segs = self.tray_segments_now();
        let Some((_, hidden)) = menubar_taskbar_overflow(self.w, &items, &segs) else { return Vec::new() };
        let mut rows = Vec::new();
        for i in hidden {
            let it = &items[i];
            let wins: Vec<WindowId> = match &it.kind {
                DockKind::Single(id) => vec![*id],
                DockKind::Group(_, wins) => wins.clone(),
            };
            for id in wins {
                let label = self.titles.iter().find(|(w, _)| *w == id).map(|(_, t)| t.clone()).unwrap_or_else(|| it.label.clone());
                rows.push((id, it.badge_letter, it.badge_color, label));
            }
        }
        rows
    }

    /// Collect the rows for the open dock-group popup (badge, label per window).
    fn dock_popup_rows(&self) -> Vec<(WindowId, char, crate::cell::Rgba, String)> {
        let Some(ref key) = self.dock_popup else { return Vec::new() };
        if key == OVERFLOW_POPUP {
            return self.overflow_popup_rows();
        }
        let focused = self.wm.focused();
        let badge = crate::badge::badge_for(key, &self.cfg.dock_badges);
        self.titles
            .iter()
            .filter(|(id, _)| self.app_keys.get(id).map(|k| k == key).unwrap_or(false))
            .map(|(id, label)| {
                let _ = focused;
                (*id, badge.letter, badge.color, label.clone())
            })
            .collect()
    }

    /// Compute the popup position and call `render_dock_popup`, returning layers
    /// + row rects for hit-testing. Used in both `handle_mouse` and `build_frame`.
    fn render_popup_for_hit_test(
        &self,
        rows: &[(WindowId, char, crate::cell::Rgba, String)],
    ) -> (Vec<crate::compositor::Layer>, Vec<(WindowId, Rect)>) {
        let Some(ref key) = self.dock_popup else {
            return (Vec::new(), Vec::new());
        };
        // Find the pill's x position so the popup anchors to it
        let items = self.dock_items();
        let segs = self.tray_segments_now();
        if key == OVERFLOW_POPUP {
            let x = menubar_taskbar_overflow(self.w, &items, &segs).map(|(r, _)| r.x).unwrap_or(0);
            return crate::chrome::render_dock_popup(self.w, self.h, x, 2, rows, self.wm.focused());
        }
        let regions = menubar_taskbar_regions(self.w, &items, &segs);
        let pill_x = items
            .iter()
            .enumerate()
            .find(|(_, it)| match &it.kind {
                DockKind::Group(k, _) => k == key,
                DockKind::Single(_) => false,
            })
            .and_then(|(i, it)| {
                regions.iter().find(|(idx, _)| *idx == i).map(|(_, r)| (r.x, it.count))
            });
        let (px, pill_w) = pill_x.map(|(x, c)| (x, c as i32)).unwrap_or((0, 2));
        let focused = self.wm.focused();
        crate::chrome::render_dock_popup(self.w, self.h, px, pill_w, rows, focused)
    }

    // ── Public apply ─────────────────────────────────────────────────────────

    /// Apply a single client message, mutating internal state.
    ///
    /// This is the **only** way external code drives the session.  The method
    /// dispatches to private sub-handlers so that none of the internal
    /// machinery (WM, PTY handles, drag state) leaks through the public API.
    pub fn apply(&mut self, msg: ClientMsg) {
        if !matches!(
            msg,
            ClientMsg::MouseDown(_)
                | ClientMsg::MouseUp(_)
                | ClientMsg::MouseDrag(_)
                | ClientMsg::MouseMove(_)
                | ClientMsg::MouseInput(_)
                | ClientMsg::Resize { .. }
                | ClientMsg::Key(_)
                | ClientMsg::RenameChar(_)
                | ClientMsg::RenameBackspace
        ) {
            // Every message is `debug`: the events they cause log themselves
            // at `info` (launch_in, close window, settings changes).
            crate::log_debug(&format!("apply {:?}", msg));
        }
        match msg {
            ClientMsg::Launch { name, command, args } => {
                // The `tetron-wm launch` escape hatch (CLI / assistant / dock pins).
                // A bare launch of a catalog-flagged CLI tool would open a window
                // that prints usage and instantly dies, so give it the same
                // help-then-shell wrapper as the launcher menu. Explicit args mean
                // an intentional invocation (`tetron-wm launch gum choose a b`) —
                // run those as given.
                // A built-in (`@files`, `@settings`, …) opens like the launcher
                // entry; spawned as a program it was a window that died at once.
                if command.starts_with('@') {
                    match (command.as_str(), args.first()) {
                        ("@files", Some(dir)) => self.open_filemanager_root(std::path::PathBuf::from(dir)),
                        _ => self.launch_entry(AppEntry { name, command, args, ..Default::default() }),
                    }
                    return;
                }
                let (command, args) = if args.is_empty()
                    && (crate::catalog::is_cli(&command) || crate::catalog::is_cli(&name))
                {
                    cli_wrap(&command, &args)
                } else {
                    (command, args)
                };
                self.launch(name, command, args);
            }
            ClientMsg::MouseDown(p) => {
                self.cursor = p;
                // A click ends the keyboard move mode, keeping the new geometry.
                self.move_mode_end(false);
                // A press inside a terminal window's content starts a text
                // selection (consumed here); clicks on chrome/desktop fall
                // through to normal window routing.
                if self.mouse_select(MouseKind::Down, p) {
                    return;
                }
                self.handle_mouse(MouseKind::Down, p);
                self.image_pan = self.zoomed_image_at(p).map(|id| (id, p));
            }
            ClientMsg::MouseDrag(p) => {
                // A press on a zoomed image pans it while the button is held.
                if let Some((id, last)) = self.image_pan {
                    if let Some(WinContent::ImageView(v)) = self.contents.get_mut(&id) {
                        v.drag_by(p.x - last.x, p.y - last.y);
                    }
                    self.image_pan = Some((id, p));
                    self.cursor = p;
                    return;
                }
                // An active text selection owns drags (extend the selection).
                if self.selection.map(|s| s.dragging).unwrap_or(false) {
                    self.cursor = p;
                    self.mouse_select(MouseKind::Drag, p);
                    return;
                }
                // While dragging a window, drop a "teleport" report — a motion
                // event that jumps more than half the screen vertically in one
                // step. You can't teleport mid-drag, so this is a spurious/garbage
                // terminal mouse report that would fling the window off-screen.
                // (Only applied during an active drag — a plain click on a bottom-
                // row control like the dock "+" must always go through.)
                if self.drag.is_some() && self.is_spurious_jump(p) {
                    return; // keep the last good cursor; ignore this event
                }
                self.cursor = p;
                self.handle_mouse(MouseKind::Drag, p);
            }
            ClientMsg::MouseUp(p) => {
                if self.image_pan.take().is_some() {
                    self.cursor = p;
                    return;
                }
                // Finish a text selection (capture its text into the primary
                // buffer) before any window-drag release handling.
                if self.selection.map(|s| s.dragging).unwrap_or(false) {
                    self.cursor = p;
                    self.mouse_select(MouseKind::Up, p);
                    return;
                }
                // End a drag at the last good position if the release coordinate
                // is itself a spurious teleport (don't snap the window to it).
                let p = if self.drag.is_some() && self.is_spurious_jump(p) { self.cursor } else { p };
                self.cursor = p;
                self.handle_mouse(MouseKind::Up, p);
            }
            ClientMsg::MouseMove(p) => {
                // A no-button move means the release was lost: end an image pan.
                self.image_pan = None;
                // An open right-click menu highlights the row under the pointer.
                if let Some((_, m)) = &mut self.win_menu {
                    let b = Rect::new(0, 1, self.w.max(1), (self.h - 1).max(1));
                    m.hover(p, b);
                } else if self.desktop.menu_open() {
                    self.desktop.menu_hover(p);
                } else if let Some((id, cr)) = self.topmost_window_content_at(p) {
                    if let Some(WinContent::FileManager(f)) = self.contents.get_mut(&id) {
                        if f.context_menu_open() {
                            f.context_menu_hover(Point::new(p.x - cr.x, p.y - cr.y), cr.w, cr.h);
                        }
                    }
                }
                // A no-button move ends any active text selection (self-heals a
                // missed release), then behaves like the previous bare-move path
                // (hover / drag-preview via handle_mouse).
                if self.selection.map(|s| s.dragging).unwrap_or(false) {
                    self.cursor = p;
                    self.mouse_select(MouseKind::Up, p);
                    return;
                }
                // A window drag is still active but this is a no-button move: a
                // real drag only ever arrives as Drag events (button held), so the
                // release was lost. End the drag here instead of letting the window
                // keep following the pointer ("stuck drag"), mirroring the
                // text-selection self-heal above.
                if self.drag.is_some() {
                    self.cursor = p;
                    self.handle_mouse(MouseKind::Up, p);
                    return;
                }
                self.cursor = p;
                self.handle_mouse(MouseKind::Drag, p);
            }
            ClientMsg::MousePaste(p) => {
                // Middle-click: write the primary selection to the terminal app
                // under the pointer, via the same PTY-input path as keystrokes.
                let primary = self.primary_text();
                if !primary.is_empty() {
                    // Occlusion-aware so a middle-click on a titlebar does not
                    // paste into a window hidden beneath it.
                    if let Some((_id, aid, _cr)) = self.topmost_app_content_at(p) {
                        let bytes = self.paste_bytes(aid, &primary);
                        self.apphost.input(aid, &bytes);
                    }
                }
            }
            ClientMsg::PastePrimary => {
                // Shift+Insert: paste the mouse (primary) selection into focus.
                let text = self.primary_text();
                self.paste_to_focused(&text);
            }
            ClientMsg::CopySelection => {
                // Ctrl+Shift+C: promote the current mouse selection to the
                // clipboard (for Ctrl+Shift+V) and out to the host via OSC 52.
                let text = self.primary_text();
                if !text.is_empty() {
                    self.set_clipboard(&text);
                    self.pending_clipboard = Some(text.clone());
                    self.clip_record(&text, crate::clipboard::ClipSource::Copy);
                }
            }
            ClientMsg::PasteClipboard => {
                // Ctrl+Shift+V: paste the clipboard into the focused app.
                let text = self.clipboard_text();
                self.paste_to_focused(&text);
            }
            ClientMsg::ScrollFocused(dir) => {
                // Keyboard scrollback for the focused terminal, one page at a
                // time (the focused window's content height, minus a line of
                // overlap). Uses the same apphost scroll path as the wheel.
                if let Some(id) = self.wm.focused() {
                    if let Some(WinContent::App(aid)) = self.contents.get(&id) {
                        let aid = *aid;
                        let page = self
                            .wm
                            .z_ordered()
                            .into_iter()
                            .find(|w| w.id == id)
                            .map(|w| (w.content_rect().h - 1).max(1))
                            .unwrap_or(10);
                        self.apphost.scroll(aid, dir * page);
                    }
                }
            }
            ClientMsg::Key(bytes) => {
                if let Some(id) = self.wm.focused() {
                    if let Some(WinContent::App(aid)) = self.contents.get(&id) {
                        let aid = *aid;
                        self.apphost.input(aid, &bytes);
                    }
                }
            }
            ClientMsg::Resize { w, h, .. } => {
                self.w = w;
                self.h = h;
                self.wm.set_work_area(Rect::new(0, 1, w, h - 1));
                // Rebuild the wallpaper at the new size (a tty never resizes, so
                // this only fires on a GUI terminal / SSH).
                self.wallpaper = Self::build_wallpaper(w, h, &self.cfg);
                // Re-fit the desktop grid so icons stay anchored top-right.
                self.desktop.layout(w, h);
                // Re-fit any maximized window and its app to the new work area.
                if let Some(id) = self.wm.focused() {
                    self.sync_app_size(id);
                }
                self.auto_tile_if_enabled();
                if self.simple {
                    if let Some(fid) = self.wm.focused() {
                        self.sync_app_size(fid);
                    }
                }
            }
            ClientMsg::MaximizeFocused => {
                if let Some(id) = self.wm.focused() {
                    self.wm.maximize_toggle(id);
                    self.sync_app_size(id);
                }
            }
            ClientMsg::MinimizeFocused => {
                if let Some(id) = self.wm.focused() {
                    self.wm.minimize(id);
                }
            }
            ClientMsg::SnapFocused(zone) => {
                if let Some(id) = self.wm.focused() {
                    self.wm.snap(id, zone);
                    self.sync_app_size(id);
                }
            }
            ClientMsg::Screenshot(path) => self.request_screenshot(path),
            ClientMsg::TileAll => {
                let grid = self.grid();
                self.wm.tile_all(grid, self.cfg.tile_gap);
                self.sync_all_app_sizes();
            }
            ClientMsg::ToggleAutoTile => {
                self.cfg.auto_tile = !self.cfg.auto_tile;
                let _ = self.cfg.save();
                if self.cfg.auto_tile {
                    let grid = self.grid();
                    self.wm.tile_all(grid, self.cfg.tile_gap);
                    self.sync_all_app_sizes();
                }
            }
            ClientMsg::SendToCell(n) => {
                let grid = self.grid();
                if n >= 1 && n <= grid.cells() {
                    if let Some(id) = self.wm.focused() {
                        let (row, col) = grid.row_col(n - 1);
                        self.wm.send_to_cell(id, grid, row, col, self.cfg.tile_gap);
                        self.sync_app_size(id);
                    }
                }
            }
            ClientMsg::ToggleMenu => {
                self.refresh_launcher_if_closed();
                self.launcher.toggle_menu();
            }
            ClientMsg::ToggleSpotlight => {
                self.refresh_launcher_if_closed();
                self.launcher.toggle_spotlight();
            }
            ClientMsg::LauncherChar(c) => self.launcher.type_char(c),
            ClientMsg::LauncherBackspace => self.launcher.backspace(),
            ClientMsg::LauncherUp => self.launcher.move_up(),
            ClientMsg::LauncherDown => self.launcher.move_down(),
            ClientMsg::LauncherLeft => self.launcher.collapse(),
            ClientMsg::LauncherRight => self.launcher.expand(),
            ClientMsg::LauncherEnter => {
                match self.launcher.mode() {
                    Some(crate::launcher::LauncherMode::Menu) => {
                        if let Some(app) = self.launcher.activate() {
                            self.launcher.close();
                            self.launch_entry(app);
                        }
                    }
                    Some(crate::launcher::LauncherMode::Spotlight) => {
                        if let Some(app) = self.launcher.selected_entry() {
                            self.launcher.close();
                            self.launch_entry(app);
                        }
                    }
                    None => {}
                }
            }
            ClientMsg::LauncherEsc => self.launcher.close(),
            ClientMsg::OpenStore => self.open_store(),
            ClientMsg::StoreUp => { if let Some(s) = self.focused_store_mut() { s.move_up(); } }
            ClientMsg::StoreDown => { if let Some(s) = self.focused_store_mut() { s.move_down(); } }
            ClientMsg::StorePrevCategory => { if let Some(s) = self.focused_store_mut() { s.prev_category(); } }
            ClientMsg::StoreNextCategory => { if let Some(s) = self.focused_store_mut() { s.next_category(); } }
            ClientMsg::StoreChar(c) => { if let Some(s) = self.focused_store_mut() { s.type_char(c); } }
            ClientMsg::StoreBackspace => { if let Some(s) = self.focused_store_mut() { s.backspace(); } }
            ClientMsg::StoreActivate => self.store_activate(),
            ClientMsg::StoreUpdateList => self.start_list_update(),
            ClientMsg::StoreClose => {
                if let Some(id) = self.wm.focused() {
                    if matches!(self.contents.get(&id), Some(WinContent::Store(_))) {
                        self.close(id);
                    }
                }
            }
            ClientMsg::OpenSettings => self.open_settings(),
            // Up / Down in the section list change section, which can arm the
            // Sound refresh, so both drain.
            ClientMsg::SettingsUp => { if let Some(s) = self.focused_settings_mut() { s.move_up(); } self.drain_settings_action(); }
            ClientMsg::SettingsDown => { if let Some(s) = self.focused_settings_mut() { s.move_down(); } self.drain_settings_action(); }
            ClientMsg::SettingsPrevSection => { if let Some(s) = self.focused_settings_mut() { s.prev_section(); } self.drain_settings_action(); }
            ClientMsg::SettingsNextSection => { if let Some(s) = self.focused_settings_mut() { s.next_section(); } self.drain_settings_action(); }
            ClientMsg::SettingsTogglePane => { if let Some(s) = self.focused_settings_mut() { s.toggle_pane(); } }
            ClientMsg::SettingsLeft => { if let Some(s) = self.focused_settings_mut() { s.left(); } self.drain_settings_action(); self.sync_settings(); }
            ClientMsg::SettingsRight => { if let Some(s) = self.focused_settings_mut() { s.right(); } self.drain_settings_action(); self.sync_settings(); }
            ClientMsg::SettingsToggle => {
                if let Some(s) = self.focused_settings_mut() {
                    s.toggle();
                }
                self.drain_settings_action();
                self.sync_settings();
            }
            ClientMsg::SettingsChar(c) => { if let Some(s) = self.focused_settings_mut() { s.type_char(c); } }
            ClientMsg::SettingsBackspace => { if let Some(s) = self.focused_settings_mut() { s.backspace(); } }
            ClientMsg::SettingsCancelEdit => { if let Some(s) = self.focused_settings_mut() { s.cancel_edit(); } }
            ClientMsg::SettingsClose => {
                if let Some(id) = self.wm.focused() {
                    if matches!(self.contents.get(&id), Some(WinContent::Settings(_))) {
                        self.close(id);
                    }
                }
            }
            ClientMsg::OpenFileManager => self.open_filemanager(),
            ClientMsg::FileManagerUp => {
                if let Some(f) = self.focused_filemanager_mut() {
                    // While the context menu is open, Up/Down move its
                    // highlight instead of the file cursor underneath.
                    if f.context_menu_open() { f.context_menu_up(); } else { f.move_cursor(0, -1); }
                }
            }
            ClientMsg::FileManagerDown => {
                if let Some(f) = self.focused_filemanager_mut() {
                    if f.context_menu_open() { f.context_menu_down(); } else { f.move_cursor(0, 1); }
                }
            }
            ClientMsg::FileManagerLeft => { if let Some(f) = self.focused_filemanager_mut() { f.move_cursor(-1, 0); } }
            ClientMsg::FileManagerRight => { if let Some(f) = self.focused_filemanager_mut() { f.move_cursor(1, 0); } }
            ClientMsg::FileManagerActivate => {
                if let Some(f) = self.focused_filemanager_mut() { f.activate(); }
                self.drain_fm_action();
            }
            ClientMsg::FileManagerBack => { if let Some(f) = self.focused_filemanager_mut() { f.go_back(); } }
            ClientMsg::FileManagerParent => { if let Some(f) = self.focused_filemanager_mut() { f.go_parent(); } }
            ClientMsg::FileManagerToggleView => {
                if let Some(f) = self.focused_filemanager_mut() { f.cycle_view(); }
            }
            ClientMsg::FileManagerViewIcon => {
                if let Some(f) = self.focused_filemanager_mut() {
                    f.set_view(crate::filemanager::ViewMode::Icon);
                }
            }
            ClientMsg::FileManagerViewList => {
                if let Some(f) = self.focused_filemanager_mut() {
                    f.set_view(crate::filemanager::ViewMode::List);
                }
            }
            ClientMsg::FileManagerViewColumns => {
                if let Some(f) = self.focused_filemanager_mut() {
                    f.set_view(crate::filemanager::ViewMode::Columns);
                }
            }
            ClientMsg::FileManagerTogglePreview => {
                if let Some(f) = self.focused_filemanager_mut() { f.toggle_preview(); }
            }
            ClientMsg::FileManagerToggleHidden => { if let Some(f) = self.focused_filemanager_mut() { f.toggle_hidden(); } }
            ClientMsg::FileManagerNewFolder => { if let Some(f) = self.focused_filemanager_mut() { f.begin_new_folder(); } }
            ClientMsg::FileManagerRename => { if let Some(f) = self.focused_filemanager_mut() { f.begin_rename(); } }
            ClientMsg::FileManagerDelete => { if let Some(f) = self.focused_filemanager_mut() { f.begin_delete(); } }
            ClientMsg::FileManagerCopy => {
                // Record the copy in the session-level transfer clipboard too, so
                // pasting into a window on a DIFFERENT system scp's the files.
                let backend = self.focused_fm_backend();
                if let Some(f) = self.focused_filemanager_mut() {
                    f.copy_selection();
                    let paths = f.selected_paths();
                    if !paths.is_empty() {
                        self.transfer = Some((backend, paths));
                    }
                }
            }
            ClientMsg::FileManagerCut => { if let Some(f) = self.focused_filemanager_mut() { f.cut_selection(); } }
            ClientMsg::FileManagerPaste => self.fm_paste(),
            ClientMsg::FileManagerChar(c) => { if let Some(f) = self.focused_filemanager_mut() { f.overlay_char(c); } }
            ClientMsg::FileManagerBackspace => { if let Some(f) = self.focused_filemanager_mut() { f.overlay_backspace(); } }
            ClientMsg::FileManagerCommit => {
                // Commit an edit overlay, a delete confirmation, or (Enter on
                // an open context menu) the highlighted menu action.
                if let Some(f) = self.focused_filemanager_mut() {
                    match f.overlay() {
                        Some(crate::filemanager::Overlay::ConfirmDelete { .. }) => f.confirm_delete(),
                        Some(crate::filemanager::Overlay::Context { .. }) => f.context_menu_commit(),
                        _ => f.overlay_commit(),
                    }
                }
                // The "Open" menu action can queue an OpenImage/RunApp action,
                // same as FileManagerActivate.
                self.drain_fm_action();
            }
            ClientMsg::FileManagerCancel => { if let Some(f) = self.focused_filemanager_mut() { f.cancel_overlay(); } }
            ClientMsg::FileManagerClose => {
                // Esc with a dialog open answers it (Cancel); only a bare Esc closes Files.
                if let Some(f) = self.focused_filemanager_mut() {
                    if f.overlay().is_some() {
                        f.cancel_overlay();
                        return;
                    }
                }
                if let Some(id) = self.wm.focused() {
                    if matches!(self.contents.get(&id), Some(WinContent::FileManager(_))) {
                        self.close(id);
                    }
                }
            }
            ClientMsg::FileManagerNewTab => { if let Some(f) = self.focused_filemanager_mut() { f.new_tab(); } }
            ClientMsg::FileManagerCloseTab => { if let Some(f) = self.focused_filemanager_mut() { f.close_tab(); } }
            ClientMsg::FileManagerNextTab => { if let Some(f) = self.focused_filemanager_mut() { f.next_tab(); } }
            ClientMsg::OpenActivity => self.open_activity(),
            ClientMsg::ToggleAssistant => self.toggle_assistant(),
            ClientMsg::ActivityUp => { if let Some(a) = self.focused_activity_mut() { a.move_up(); } }
            ClientMsg::ActivityDown => { if let Some(a) = self.focused_activity_mut() { a.move_down(); } }
            ClientMsg::ActivityKill => {
                if let Some(a) = self.focused_activity_mut() {
                    if let Some(app_id) = a.request_kill_selected() {
                        self.apphost.kill(AppId(app_id));
                    }
                }
            }
            ClientMsg::ActivityConfirmKill => {
                if let Some(a) = self.focused_activity_mut() {
                    if let Some(app_id) = a.request_kill_selected() {
                        self.apphost.kill(AppId(app_id));
                    }
                }
            }
            ClientMsg::ActivityCancelKill => {
                if let Some(a) = self.focused_activity_mut() { a.cancel_kill(); }
            }
            ClientMsg::ActivityKillDead => {
                if let Some(a) = self.focused_activity_mut() {
                    for app_id in a.kill_dead() {
                        self.apphost.kill(AppId(app_id));
                    }
                }
            }
            ClientMsg::ActivityRefresh => self.refresh_activity(),
            ClientMsg::ActivityClose => {
                if let Some(id) = self.activity_win.take() {
                    if matches!(self.contents.get(&id), Some(WinContent::Activity(_))) {
                        self.close(id);
                    }
                }
            }
            ClientMsg::Clip(cmd) => {
                crate::clipboard::handle_cmd(&self.clip, cmd);
            }
            ClientMsg::ClipMenuToggle => self.toggle_clipmenu(),
            ClientMsg::ToggleSelectMode => self.select_mode = !self.select_mode,
            ClientMsg::ClipMenuKey(k) => {
                let hist = self.clip_history();
                if let Some(a) = self.clipmenu.key(k, &hist) {
                    self.apply_clip_action(a);
                }
            }
            ClientMsg::ToggleHelp => {
                // Open the help window; from inside it (focused), close it.
                if self.focused_is_help() {
                    if let Some(id) = self.wm.focused() {
                        self.close(id);
                    }
                } else {
                    self.open_help();
                }
            }
            ClientMsg::HelpScroll(d) => {
                if let Some(h) = self.focused_help() {
                    h.scroll_by(d);
                }
            }
            ClientMsg::OpenImage(p) => self.open_image(p),
            ClientMsg::DirPickerUp => { if let Some(d) = self.dirpicker.as_mut() { d.move_up(); } }
            ClientMsg::DirPickerDown => { if let Some(d) = self.dirpicker.as_mut() { d.move_down(); } }
            ClientMsg::DirPickerExpand => { if let Some(d) = self.dirpicker.as_mut() { d.expand(); } }
            ClientMsg::DirPickerCollapse => { if let Some(d) = self.dirpicker.as_mut() { d.collapse(); } }
            ClientMsg::DirPickerToggleHidden => { if let Some(d) = self.dirpicker.as_mut() { d.toggle_hidden(); } }
            ClientMsg::DirPickerNewFolder => { if let Some(d) = self.dirpicker.as_mut() { d.begin_create(); } }
            ClientMsg::DirPickerChar(c) => { if let Some(d) = self.dirpicker.as_mut() { d.create_type(c); } }
            ClientMsg::DirPickerBackspace => { if let Some(d) = self.dirpicker.as_mut() { d.create_backspace(); } }
            ClientMsg::DirPickerCancel => {
                // Cancel the new-folder input first; only then close the picker.
                match self.dirpicker.as_mut() {
                    Some(d) if d.is_creating() => d.cancel_create(),
                    _ => self.dirpicker = None,
                }
            }
            ClientMsg::DirPickerConfirm => {
                // Enter commits the new-folder name, else launches in the dir.
                if self.dirpicker.as_ref().map(|d| d.is_creating()).unwrap_or(false) {
                    if let Some(d) = self.dirpicker.as_mut() { d.commit_create(); }
                } else {
                    self.confirm_dirpicker();
                }
            }
            ClientMsg::MouseRightDown(p) => {
                self.cursor = p;
                // Right-clicking a dock pill opens its context menu (minimise /
                // maximise / close / reset size). Groups target the focused
                // window of the group, else the first. Only when no other
                // menu/modal is already showing: its click-capture block runs
                // before the other modal routing, so opening it underneath a
                // higher-z overlay would let a hidden menu silently eat the
                // next click. Same overlay set as `app_mouse_area`.
                let overlay_open = self.launcher.is_open()
                    || self.dirpicker.is_some()
                    || self.power_menu.is_open()
                    || self.confirm_close.is_open()
                    || self.win_picker.is_open()
                    || self.launch_warn.is_open()
                    || self.compat_dialog
                    || self.rename.is_some()
                    || self.tray.open().is_some()
                    || self.clipmenu.is_open()
                    || self.win_menu.is_some()
                    || self.desktop.overlay_rect().is_some();
                if !overlay_open && p.y == 0 {
                    let items = self.dock_items();
                    let segs = self.tray_segments_now();
                    let hit = menubar_taskbar_regions(self.w, &items, &segs)
                        .into_iter()
                        .find(|(_, r)| r.contains(p));
                    if let Some((idx, r)) = hit {
                        let focused = self.wm.focused();
                        let target = match &items[idx].kind {
                            DockKind::Single(id) => Some(*id),
                            DockKind::Group(_, wins) => wins
                                .iter()
                                .copied()
                                .find(|w| Some(*w) == focused)
                                .or_else(|| wins.first().copied()),
                        };
                        if let Some(id) = target {
                            self.dock_popup = None;
                            self.dock_ctx = Some((id, r.x));
                        }
                        return;
                    }
                }
                // Right-clicking a window's title bar opens its window menu.
                if !overlay_open {
                    if let Some(id) = self.wm.topmost_at(p) {
                        if self.wm.get(id).is_some_and(|w| !w.minimized && w.titlebar_rect().contains(p)) {
                            self.wm.raise(id);
                            self.win_menu = Some((id, crate::contextmenu::ContextMenu::new(p, crate::contextmenu::window_actions())));
                            return;
                        }
                    }
                }
                // Right-clicking inside a file-manager window's content area
                // opens its context menu (rename / delete / …) on the entry
                // under the cursor. Same overlay guard as the dock branch
                // above; a non-entry hit (toolbar/sidebar/empty area) does
                // nothing (v1) rather than falling through to the desktop.
                if !overlay_open {
                    if let Some((id, cr)) = self.topmost_window_content_at(p) {
                        if matches!(self.contents.get(&id), Some(WinContent::FileManager(_))) {
                            self.wm.raise(id);
                            let local = Point::new(p.x - cr.x, p.y - cr.y);
                            if let Some(WinContent::FileManager(f)) = self.contents.get_mut(&id) {
                                match f.hit_test(local, cr.w, cr.h) {
                                    // Anchor the menu at the exact click point (mirrors the
                                    // desktop's right_click), not the tile origin.
                                    Some(crate::filemanager::Target::Entry(i)) => f.begin_context_at(i, local),
                                    // The empty part of the listing: Paste, New Folder, ….
                                    None if local.x >= crate::filemanager::SIDEBAR_W && local.y >= f.listing_top() => f.begin_background_context(local),
                                    _ => {}
                                }
                            }
                            return;
                        }
                    }
                }
                self.handle_desktop_right(p);
            }
            ClientMsg::MouseDouble(p) => {
                self.cursor = p;
                // If the launcher is open, route the double-click to it (a second
                // fast click on the brand dismisses the menu rather than falling
                // through to titlebar/desktop handling — and must not launch the
                // selected row).
                if self.launcher.is_open() {
                    if let Some(app) = self.launcher.click(p) {
                        self.launcher.close();
                        self.launch_entry(app);
                    } else {
                        self.launcher.close();
                    }
                    return;
                }
                // Double-click on a window's title TEXT (not empty titlebar space
                // or a control button) starts a rename of that window. Check this
                // before desktop/content.
                if let Some(id) = self.topmost_window_title_text_at(p) {
                    // Clear any in-progress move/resize from the double-click's own
                    // button presses so it can't linger behind the rename.
                    self.drag = None;
                    self.drag_armed = false;
                    // Start with an empty buffer — type the new name fresh; an
                    // empty commit (or Esc) keeps the current name.
                    self.rename = Some((id, String::new()));
                } else if self.cfg.desktop_enabled && self.window_at_is_none(p) {
                    self.desktop.double_click(p);
                    self.drain_desktop_action();
                } else if let Some((id, cr)) = self.topmost_window_content_at(p) {
                    // Double-click inside a file-manager window opens the entry
                    // under the cursor (navigate into a folder / open a file).
                    let local = Point::new(p.x - cr.x, p.y - cr.y);
                    let mut acted = false;
                    if let Some(WinContent::FileManager(f)) = self.contents.get_mut(&id) {
                        acted = f.double_click(local, cr.w, cr.h);
                    }
                    if acted {
                        self.drain_fm_action();
                    }
                }
            }
            ClientMsg::DesktopChar(c) => self.desktop.overlay_char(c),
            ClientMsg::DesktopBackspace => self.desktop.overlay_backspace(),
            ClientMsg::DesktopCommit => self.desktop_commit(),
            ClientMsg::DesktopCancel => self.desktop.cancel_overlay(),
            ClientMsg::VolumeOpen => self.volume_open(),
            ClientMsg::BrightnessOpen => self.brightness_open(),
            ClientMsg::InfoOpen => self.info_open(),
            ClientMsg::TrayKey(key) if self.info_popover_open() => {
                // Enter applies the selected row's action, exactly what a click does.
                let rendered = self.tray_render_now();
                if let Some(i) = self.tray.info_key(key, rendered.hits.len()) {
                    if let Some(hit) = rendered.hits.get(i) {
                        self.apply_intent(hit.intent.clone());
                    }
                }
            }
            ClientMsg::TrayKey(key) => {
                let intent = match self.tray.open() {
                    Some(crate::tray::SegmentKind::Volume) => self.tray.volume_key(key),
                    Some(crate::tray::SegmentKind::Brightness) => self.tray.brightness_key(key),
                    _ => None,
                };
                if let Some(intent) = intent {
                    self.apply_intent(intent);
                }
            }
            ClientMsg::MoveModeStart => self.move_mode_start(),
            ClientMsg::MoveModeStep { dx, dy } => self.move_mode_step(dx, dy),
            ClientMsg::MoveModeToggleResize => {
                if let Some(m) = self.move_mode.as_mut() {
                    m.resize = !m.resize;
                }
            }
            ClientMsg::MoveModeEnd { cancel } => self.move_mode_end(cancel),
            ClientMsg::FocusNext => self.focus_step(true),
            ClientMsg::FocusPrev => self.focus_step(false),
            ClientMsg::WindowPickerOpen => self.open_window_picker(),
            ClientMsg::WindowPickerStep(d) => self.win_picker.step(d),
            ClientMsg::WindowPickerEnter => {
                if let Some(id) = self.win_picker.confirm() {
                    self.focus_window(id);
                }
            }
            ClientMsg::WindowPickerNumber(n) => {
                if let Some(id) = self.win_picker.pick_number(n) {
                    self.focus_window(id);
                }
            }
            ClientMsg::WindowPickerEsc => self.win_picker.close(),
            ClientMsg::RenameFocused => {
                if let Some(id) = self.wm.focused() {
                    self.rename = Some((id, String::new()));
                }
            }
            ClientMsg::RenameChar(c) => {
                if !c.is_control() {
                    if let Some((_, buf)) = &mut self.rename {
                        buf.push(c);
                    }
                }
            }
            ClientMsg::RenameBackspace => {
                if let Some((_, buf)) = &mut self.rename {
                    buf.pop();
                }
            }
            ClientMsg::RenameCommit => {
                if let Some((id, buf)) = self.rename.take() {
                    if !buf.is_empty() {
                        // Update the dock label.
                        if let Some((_, label)) = self.titles.iter_mut().find(|(i, _)| *i == id) {
                            *label = buf.clone();
                        }
                        // Update the window titlebar title.
                        self.wm.rename_window(id, buf);
                    }
                }
            }
            ClientMsg::RenameCancel => {
                self.rename = None;
            }
            ClientMsg::CloseFocused => {
                if let Some(id) = self.wm.focused() {
                    self.request_close(id);
                }
            }
            ClientMsg::ConfirmCloseYes => {
                if let Some(id) = self.confirm_close.confirm() {
                    self.close(id);
                }
            }
            ClientMsg::ConfirmCloseNo => self.confirm_close.cancel(),
            ClientMsg::LaunchWarnYes => {
                if let Some(l) = self.launch_warn.confirm() {
                    self.launch_resolved(l.name, l.command, l.args, l.cli, l.requires_cwd, l.cwd);
                }
            }
            ClientMsg::LaunchWarnNo => self.launch_warn.cancel(),
            ClientMsg::Shutdown => self.shutdown = true,
            ClientMsg::Reload => self.reload = true,
            ClientMsg::Status => {} // answered by the daemon's control thread
            ClientMsg::Screensaver { mode, effect } => {
                use crate::screensaver::SaverMode;
                match (mode, self.saver.is_some()) {
                    (SaverMode::Off, _) | (SaverMode::Toggle, true) => self.screensaver_off(),
                    (SaverMode::On, _) | (SaverMode::Toggle, false) => self.screensaver_on(effect.as_deref()),
                }
            }
            ClientMsg::MouseInput(m) => self.forward_mouse_to_app(m),
            ClientMsg::ScrollAt { p, lines } if self.clipmenu.is_open() => {
                let _ = p;
                let hist = self.clip_history();
                self.clipmenu.scroll(-lines, &hist); // wheel up (+) moves up the list
            }
            ClientMsg::ScrollAt { lines, .. } if self.dirpicker.is_some() => {
                // Wheel over the working-directory picker moves the selection
                // (the view follows it); positive lines = wheel up.
                if let Some(d) = self.dirpicker.as_mut() {
                    for _ in 0..lines.unsigned_abs().min(20) {
                        if lines > 0 { d.move_up() } else { d.move_down() }
                    }
                }
            }
            ClientMsg::ScrollAt { p, lines } => {
                // Wheel over the Volume or Brightness tray segment, or over the
                // volume row of Settings → Sound, adjusts that level; otherwise it
                // scrolls the window under the pointer.
                if let Some(intent) = self.volume_scroll_intent(p, lines) {
                    self.apply_intent(intent);
                } else if self.settings_wheel(p, lines) {
                    self.drain_settings_action();
                } else {
                    self.scroll_app_at(p, lines);
                }
            }
            ClientMsg::PowerFormChar(c) => self.power_menu.form_char(c),
            ClientMsg::PowerFormBackspace => self.power_menu.form_backspace(),
            ClientMsg::PowerFormNext => self.power_menu.form_next(),
            ClientMsg::PowerFormPrev => self.power_menu.form_prev(),
            ClientMsg::PowerFormLeft => self.power_menu.form_left(),
            ClientMsg::PowerFormRight => self.power_menu.form_right(),
            ClientMsg::PowerFormCommit => {
                if let PowerClick::Act(outcome) = self.power_menu.form_commit() {
                    self.apply_power_outcome(outcome);
                }
            }
            ClientMsg::PowerFormCancel => self.power_menu.form_cancel(),
            ClientMsg::PowerKeyConfirm => {
                if let PowerClick::Act(outcome) = self.power_menu.key_confirm() {
                    self.apply_power_outcome(outcome);
                }
            }
            ClientMsg::PowerKeyCancel => self.power_menu.key_cancel(),
            ClientMsg::LogsUp => { if let Some(l) = self.focused_logs_mut() { l.scroll_by(-1); } }
            ClientMsg::LogsDown => { if let Some(l) = self.focused_logs_mut() { l.scroll_by(1); } }
            ClientMsg::LogsPageUp => {
                if let Some(l) = self.focused_logs_mut() {
                    let n = l.page_size() as i32;
                    l.scroll_by(-n);
                }
            }
            ClientMsg::LogsPageDown => {
                if let Some(l) = self.focused_logs_mut() {
                    let n = l.page_size() as i32;
                    l.scroll_by(n);
                }
            }
            ClientMsg::LogsCopy => {
                if let Some(l) = self.focused_logs_mut() {
                    let payload = l.copy_payload();
                    self.pending_clipboard = Some(payload);
                }
            }
            ClientMsg::LogsRefresh => { if let Some(l) = self.focused_logs_mut() { l.reload(); } }
            ClientMsg::DesktopMenuKey(k) if self.win_menu.is_some() => match k {
                MenuKey::Up => { if let Some((_, m)) = &mut self.win_menu { m.up(); } }
                MenuKey::Down => { if let Some((_, m)) = &mut self.win_menu { m.down(); } }
                MenuKey::Esc => self.win_menu = None,
                MenuKey::Enter => {
                    if let Some((id, m)) = self.win_menu.take() {
                        if let Some(action) = m.selected().cloned() {
                            self.run_window_action(id, action);
                        }
                    }
                }
            },
            ClientMsg::DesktopMenuKey(k) => match k {
                MenuKey::Up => self.desktop.menu_up(),
                MenuKey::Down => self.desktop.menu_down(),
                MenuKey::Esc => self.desktop.cancel_overlay(),
                MenuKey::Enter => {
                    if let Some(action) = self.desktop.menu_selected() {
                        self.run_desktop_menu_action(action);
                    }
                }
            },
            ClientMsg::ContextMenuOpen => {
                if let Some(f) = self.focused_filemanager_mut() {
                    f.begin_context();
                } else if self.cfg.desktop_enabled && self.wm.focused().is_none() {
                    self.desktop.open_menu_keyboard();
                }
            }
            ClientMsg::ImageKey(k) => {
                if let Some(id) = self.wm.focused() {
                    if let Some(WinContent::ImageView(v)) = self.contents.get_mut(&id) {
                        if k == crate::imageview::ImageKey::Fullscreen {
                            let files = v.fullscreen_files();
                            self.start_fullscreen(files);
                        } else if v.key(k) {
                            self.close(id);
                        }
                    }
                }
            }
            ClientMsg::LogsClose => {
                if let Some(id) = self.wm.focused() {
                    if matches!(self.contents.get(&id), Some(WinContent::Logs(_))) {
                        self.close(id);
                    }
                }
            }
            ClientMsg::Caps(caps) => {
                self.client_caps = caps;
                self.apply_display();
            }
            ClientMsg::SetTheme(name) => {
                crate::dbg_log(&format!("theme: set '{name}' (client attach / TETRON_WM_THEME)"));
                crate::theme::set(&name);
                // A client sends this on every attach; only a real change is saved.
                let changed = self.cfg.theme != name;
                // `default` shows the theme's own picture, so a switch redraws it.
                let new_picture = crate::wallpaper::is_default(self.cfg.wallpaper.as_deref())
                    && crate::wallpaper::theme_key(&self.cfg.theme) != crate::wallpaper::theme_key(&name);
                self.cfg.theme = name;
                if new_picture {
                    self.wallpaper = Self::build_wallpaper(self.w, self.h, &self.cfg);
                }
                self.push_terminal_colors();
                if changed {
                    if let Err(e) = self.cfg.save() {
                        crate::log_warn(&format!("theme: config save failed: {e}"));
                    }
                }
            }
        }
        // Refresh thumbnails after any message that may have changed the focused
        // file manager's listing (cheap: ImageStore loads are content-hash cached).
        if self.focused_is_filemanager() {
            self.refresh_fm_thumbnails();
        }
    }

    /// Resolve the open picker: launch its pending app in the chosen directory
    /// and record the directory in the recent list.
    fn confirm_dirpicker(&mut self) {
        let Some(picker) = self.dirpicker.take() else { return };
        let (pending, path) = picker.confirm();
        // Record in the MRU (most-recent first, deduped, capped at 10).
        let p = path.to_string_lossy().to_string();
        self.state.recent_dirs.retain(|d| d != &p);
        self.state.recent_dirs.insert(0, p);
        self.state.recent_dirs.truncate(10);
        self.save_state();
        self.launch_in(pending.name, pending.command, pending.args, Some(path));
    }

    /// Launch an app, first opening the working-directory picker when it is
    /// flagged `requires_cwd` and has no fixed directory.
    fn launch_maybe_cwd(&mut self, name: String, command: String, args: Vec<String>, requires_cwd: bool, fixed: Option<String>) {
        if let Some(dir) = fixed {
            self.launch_in(name, command, args, Some(expand_tilde(&dir)));
        } else if requires_cwd {
            self.dirpicker = Some(crate::dirpicker::DirPicker::new(
                self.picker_root(),
                crate::dirpicker::PendingLaunch { name, command, args },
            ));
        } else {
            self.launch(name, command, args);
        }
    }

    /// The directory the picker opens at: the configured project dir (tilde
    /// expanded) or the user's home.
    fn picker_root(&self) -> std::path::PathBuf {
        self.cfg
            .default_project_dir
            .as_deref()
            .map(expand_tilde)
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| std::path::PathBuf::from("/"))
    }

    /// Open the settings window, or focus it if it's already open.
    fn open_settings(&mut self) {
        if let Some(id) = self.settings_win {
            if self.contents.contains_key(&id) {
                self.wm.unminimize(id);
                return;
            }
        }
        let w = 84.min((self.w - 4).max(40));
        let h = 23.min((self.h - 4).max(10)); // Appearance has rows and hints to y 20
        let rect = Rect::new((self.w - w) / 2, 3, w, h);
        let id = self.wm.add_window("Settings".into(), rect);
        self.app_keys.insert(id, "Settings".into());
        let mut panel = Settings::new(self.cfg.clone());
        panel.set_apphost_outdated(self.apphost_outdated);
        panel.set_colors_live(self.apphost.proto_version() >= 4);
        panel.set_sound_source(self.tray_state.clone());
        self.contents.insert(id, WinContent::Settings(Box::new(panel)));
        self.titles.push((id, "Settings".into()));
        self.settings_win = Some(id);
    }

    /// Wheel at screen point `p`: if the topmost window there is the Settings
    /// panel and the point is its Sound volume row, it arms a volume action and
    /// takes focus so the drain finds it.
    fn settings_wheel(&mut self, p: Point, lines: i32) -> bool {
        let Some(id) = self.wm.topmost_at(p) else { return false };
        let Some(cr) = self.wm.get(id).map(|w| w.content_rect()) else { return false };
        if !cr.contains(p) {
            return false;
        }
        let local = Point::new(p.x - cr.x, p.y - cr.y);
        let used = match self.contents.get_mut(&id) {
            Some(WinContent::Settings(s)) => s.wheel_at(local, lines),
            _ => false,
        };
        if used {
            self.wm.raise(id);
        }
        used
    }

    /// `true` when the focused window hosts the settings panel.
    pub fn focused_is_settings(&self) -> bool {
        self.wm
            .focused()
            .and_then(|id| self.contents.get(&id))
            .map(|c| matches!(c, WinContent::Settings(_)))
            .unwrap_or(false)
    }

    /// `true` when the focused settings panel is in a text-entry field (Apps add
    /// form), so the client should forward typed characters rather than treat
    /// them as navigation.
    pub fn settings_editing(&self) -> bool {
        matches!(
            self.wm.focused().and_then(|id| self.contents.get(&id)),
            Some(WinContent::Settings(s)) if s.is_editing()
        )
    }

    fn focused_settings_mut(&mut self) -> Option<&mut Settings> {
        let id = self.wm.focused()?;
        match self.contents.get_mut(&id)? {
            WinContent::Settings(s) => Some(s),
            _ => None,
        }
    }

    /// Copy the focused settings panel's edited config into the live config and
    /// persist it to disk. Changes (snapping, shadows) then take effect next frame.
    fn sync_settings(&mut self) {
        let cfg = match self.wm.focused().and_then(|id| self.contents.get(&id)) {
            Some(WinContent::Settings(s)) => Some(s.config().clone()),
            _ => None,
        };
        if let Some(cfg) = cfg {
            // Rebuilding the launcher rescans $PATH, so only do it when the
            // custom-app list actually changed (not on every shadow/theme tweak).
            let launcher_changed = cfg.launcher != self.cfg.launcher;
            let clip_changed = cfg.clipboard_history != self.cfg.clipboard_history
                || cfg.clipboard_persist != self.cfg.clipboard_persist
                || cfg.clipboard_history_size != self.cfg.clipboard_history_size
                || cfg.clipboard_item_max_kb != self.cfg.clipboard_item_max_kb;
            let colors_changed_depth = cfg.colors != self.cfg.colors;
            // `default` shows the theme's own picture, so a theme change redraws it.
            let wallpaper_changed = cfg.wallpaper_enabled != self.cfg.wallpaper_enabled
                || cfg.wallpaper != self.cfg.wallpaper
                || (crate::wallpaper::is_default(cfg.wallpaper.as_deref())
                    && crate::wallpaper::theme_key(&cfg.theme) != crate::wallpaper::theme_key(&self.cfg.theme));
            let colors_changed = cfg.theme != self.cfg.theme
                || cfg.terminal_bg != self.cfg.terminal_bg
                || cfg.terminal_fg != self.cfg.terminal_fg;
            let changed = cfg != self.cfg;
            self.cfg = cfg;
            crate::fullscreen::set_enabled(self.cfg.fullscreen);
            crate::logfile::set_level(self.cfg.log_level);
            self.tray.set_tools(self.cfg.tray_tools.clone());
            crate::theme::set(&self.cfg.theme);
            crate::iconset::set(self.cfg.icon_style);
            crate::openwith::set_extra(&self.cfg.open_with);
            if changed {
                if let Err(e) = self.cfg.save() {
                    crate::log_warn(&format!("config: save failed: {e}"));
                }
            }
            if launcher_changed {
                self.launcher = Launcher::new(Self::build_launcher_apps(&self.cfg, &self.systems));
            }
            if clip_changed {
                self.apply_clipboard_config();
            }
            if wallpaper_changed {
                self.wallpaper = Self::build_wallpaper(self.w, self.h, &self.cfg);
            }
            if colors_changed_depth {
                crate::dbg_log(&format!("display: colors setting -> {}", self.cfg.colors));
                self.apply_display();
            }
            if colors_changed {
                self.push_terminal_colors();
            }
        }
    }

    /// Tell the apphost to re-apply the theme and terminal color overrides so
    /// running apps repaint without a restart.
    fn push_terminal_colors(&mut self) {
        let (bg, fg) = (self.cfg.terminal_bg.clone(), self.cfg.terminal_fg.clone());
        self.send_terminal_colors(bg.as_deref(), fg.as_deref());
    }

    /// Resolve the colors here (the daemon knows every theme file; the apphost
    /// scanned its themes dir once at start) and send them, so a theme the
    /// apphost never saw still recolors correctly. An apphost that predates this
    /// still works: it applies the name, then overrides bg/fg with these values.
    fn send_terminal_colors(&mut self, bg: Option<&str>, fg: Option<&str>) {
        let (rbg, rfg) = crate::theme::resolve_terminal_colors(&self.cfg.theme, bg, fg);
        let ansi = crate::theme::resolve_ansi(&self.cfg.theme);
        self.apphost.set_colors(&self.cfg.theme, Some(&rbg), Some(&rfg), Some(&ansi));
    }

    /// Open the store window, or focus it if it's already open.
    /// Open the Store filtered to one app (Settings > Assistant > Agent on
    /// an agent that is not installed).
    fn open_store_for(&mut self, app: &str) {
        self.open_store();
        if let Some(WinContent::Store(st)) = self.store_win.and_then(|id| self.contents.get_mut(&id)) {
            st.set_query(app);
        }
    }

    fn open_store(&mut self) {
        if let Some(id) = self.store_win {
            if self.contents.contains_key(&id) {
                self.wm.unminimize(id);
                return;
            }
        }
        let w = 84.min((self.w - 4).max(40));
        let h = 28.min((self.h - 4).max(12));
        let rect = Rect::new((self.w - w) / 2, 2, w, h);
        let id = self.wm.add_window("Store".into(), rect);
        self.app_keys.insert(id, "Store".into());
        self.contents.insert(id, WinContent::Store(Store::new()));
        self.titles.push((id, "Store".into()));
        self.store_win = Some(id);
    }

    /// `true` when the focused window hosts the store browser (the front-end
    /// routes keyboard input to the store in this case).
    pub fn focused_is_store(&self) -> bool {
        self.wm
            .focused()
            .and_then(|id| self.contents.get(&id))
            .map(|c| matches!(c, WinContent::Store(_)))
            .unwrap_or(false)
    }

    fn focused_store_mut(&mut self) -> Option<&mut Store> {
        let id = self.wm.focused()?;
        match self.contents.get_mut(&id)? {
            WinContent::Store(s) => Some(s),
            _ => None,
        }
    }

    /// Assign already-loaded thumbnails to the focused file manager's image
    /// entries and queue any not-yet-loaded ones on the background loader. Never
    /// blocks on file I/O (a slow/offloaded image can no longer freeze the loop).
    /// Non-image entries get the same pre-generated per-role tile the desktop
    /// uses (`self.role_icon_ids`) — no disk I/O, no per-entry PNG generation.
    fn refresh_fm_thumbnails(&mut self) {
        let (reqs, role_reqs) = match self.focused_filemanager_mut() {
            Some(f) => (f.thumbnail_requests(), f.role_icon_requests()),
            None => return,
        };
        let mut ready: Vec<(usize, u64)> = Vec::new();
        for (idx, path) in reqs {
            if let Some(&id) = self.thumb_ids.get(&path) {
                ready.push((idx, id));
            } else {
                // Icon tile's image area is (TILE_W - 2) cells wide by
                // (TILE_H - 1) tall; cells are ~8x16px. Sized to match the
                // enlarged Icon-view tile (was a 1-row-tall request before
                // the icon tile grew to mirror the desktop's).
                let iw = (crate::filemanager::TILE_W - 2).max(2) * 8;
                let ih = (crate::filemanager::TILE_H - 1).max(1) * 16;
                self.thumb_loader.request(path, iw as u32, ih as u32);
            }
        }
        for (idx, role) in role_reqs {
            if let Some(&id) = self.role_icon_ids.get(&role) {
                ready.push((idx, id));
            }
        }
        if let Some(f) = self.focused_filemanager_mut() {
            for (idx, id) in ready {
                f.set_thumb(idx, id);
            }
        }
    }

    /// The assistant panel's window, if one exists (found by app key so it
    /// survives frontend reloads, where the window is restored from the apphost
    /// roster).
    fn assistant_window(&self) -> Option<WindowId> {
        self.app_keys.iter().find(|(_, k)| k.as_str() == "Assistant").map(|(id, _)| *id)
    }

    /// The ✦ menubar button: toggle the assistant chat panel. Focused → hide
    /// (minimize; the agent keeps running); hidden/blurred → show + focus;
    /// absent → spawn the configured agent CLI in a right-docked panel.
    fn toggle_assistant(&mut self) {
        if let Some(id) = self.assistant_window() {
            let minimized = self.wm.get(id).map(|w| w.minimized).unwrap_or(false);
            if self.wm.focused() == Some(id) && !minimized {
                self.wm.minimize(id);
            } else {
                self.wm.unminimize(id);
                if self.simple {
                    self.sync_app_size(id);
                }
            }
            return;
        }
        self.open_assistant();
    }

    /// Spawn the assistant agent (`assistant_command`, opencode by default) in
    /// a persistent right-side panel. The agent is a normal apphost app
    /// (survives detach/reload); its briefing is stamped as `AGENTS.md` in a
    /// dedicated working directory, which the agent reads on startup.
    fn open_assistant(&mut self) {
        let Some((agent, command, mut args)) = crate::assistant::resolve(&self.cfg) else {
            let a = crate::assistant::selected(&self.cfg);
            crate::dbg_log(&format!(
                "assistant: '{}' ({}) not installed; opening the Store (or set assistant_command in config.toml)",
                a.label, a.command
            ));
            self.open_store();
            return;
        };
        let Some(dir) = crate::assistant::workdir() else { return };
        let host = self.host_label.clone();
        let code = crate::assistant::code_dir(&self.cfg);
        if let Err(e) = crate::assistant::write_briefing(&dir, &host, &self.systems, &code) {
            crate::log_warn(&format!("assistant: briefing write failed: {e}"));
        }
        crate::assistant::link_context(&dir, &agent);
        let all = crate::assistant::agents(&self.cfg.assistant_agent);
        let bin = crate::assistant::self_bin();
        args.extend(crate::assistant::wire_mcp(&dir, &agent, &all, self.cfg.assistant_mcp, &bin));
        crate::dbg_log(&format!(
            "assistant: starting {} as '{command}' {:?} ({}, mcp {}) in {}",
            agent.label,
            args,
            self.cfg.assistant_mode,
            if self.cfg.assistant_mcp { "on" } else { "off" },
            dir.display()
        ));
        // "panel": right-docked, full work height, 2/5 width (clamped readable).
        // "window": a regular floating window — the popped-out mode. Either way
        // it's a normal WM window, so the user can drag/resize/maximize it.
        let rect = if self.cfg.assistant_mode == "window" {
            let w = 84.min((self.w - 4).max(30));
            let h = 30.min((self.h - 4).max(8));
            Rect::new((self.w - w) / 2, ((self.h - h) / 2).max(1), w, h)
        } else {
            let panel_w = (self.w * 2 / 5).clamp(34.min(self.w), 70);
            Rect::new((self.w - panel_w).max(0), 1, panel_w, (self.h - 2).max(4))
        };
        let id = self.wm.add_window("Assistant".into(), rect);
        let content = self.wm.get(id).unwrap().content_rect();
        match self.apphost.spawn(&command, &args, Some(&dir), content.w.max(1), content.h.max(1)) {
            Ok(app_id) => {
                self.app_keys.insert(id, "Assistant".into());
                self.contents.insert(id, WinContent::App(app_id));
                self.titles.push((id, "Assistant".into()));
                if self.simple {
                    self.sync_app_size(id);
                }
            }
            Err(e) => {
                crate::log_warn(&format!("assistant: spawn '{command}' failed: {e}"));
                self.wm.close(id);
            }
        }
    }

    /// Open (or re-focus) the Logs viewer window.
    fn open_logs(&mut self) {
        if let Some(id) = self.logs_win {
            if self.contents.contains_key(&id) {
                self.wm.unminimize(id);
                if let Some(WinContent::Logs(l)) = self.contents.get_mut(&id) {
                    l.reload();
                }
                return;
            }
        }
        let w = 100.min((self.w - 4).max(40));
        let h = 28.min((self.h - 4).max(10));
        let rect = Rect::new((self.w - w) / 2, 1, w, h);
        let id = self.wm.add_window("Logs".into(), rect);
        self.app_keys.insert(id, "Logs".into());
        self.contents.insert(id, WinContent::Logs(crate::logsview::LogsView::new()));
        self.titles.push((id, "Logs".into()));
        self.logs_win = Some(id);
    }

    /// The zoomed image viewer whose image area is under `p` (topmost window),
    /// so a press there starts a pan instead of doing nothing.
    fn zoomed_image_at(&self, p: Point) -> Option<WindowId> {
        let id = self.wm.topmost_at(p)?;
        let win = self.wm.get(id)?;
        let content = if self.simple && self.wm.focused() == Some(id) { self.simple_content_rect() } else { win.content_rect() };
        match self.contents.get(&id) {
            Some(WinContent::ImageView(v)) if !win.minimized && v.zoomed() && content.contains(p) => Some(id),
            _ => None,
        }
    }

    /// Whether the desktop right-click menu is open (keyboard routing).
    pub fn desktop_menu_open(&self) -> bool {
        self.desktop.menu_open() || self.win_menu.is_some()
    }

    /// `true` when the focused window is an image viewer (keyboard routing).
    pub fn focused_is_image(&self) -> bool {
        matches!(
            self.wm.focused().and_then(|id| self.contents.get(&id)),
            Some(WinContent::ImageView(_))
        )
    }

    /// `true` when the focused window hosts the logs viewer (keyboard routing).
    pub fn focused_is_logs(&self) -> bool {
        matches!(
            self.wm.focused().and_then(|id| self.contents.get(&id)),
            Some(WinContent::Logs(_))
        )
    }

    fn focused_logs_mut(&mut self) -> Option<&mut crate::logsview::LogsView> {
        let id = self.wm.focused()?;
        match self.contents.get_mut(&id)? {
            WinContent::Logs(l) => Some(l),
            _ => None,
        }
    }

    /// Start the screensaver with `effect` (default: the `screensaver` config
    /// key; an unknown name falls back to black and is logged). Any client input
    /// ends it (see `daemon.rs`).
    pub fn screensaver_on(&mut self, effect: Option<&str>) {
        let name = effect.unwrap_or(&self.cfg.screensaver).to_string();
        let eff = crate::screensaver::make(&name).unwrap_or_else(|| {
            crate::dbg_log(&format!("screensaver: unknown effect '{name}', using black"));
            crate::screensaver::make(crate::screensaver::DEFAULT_EFFECT).expect("black exists")
        });
        crate::dbg_log(&format!("screensaver: on ({})", eff.name()));
        // Menus and popovers would still be open when it wakes; close them so the
        // desktop comes back clean.
        self.launcher.close();
        self.tray.close();
        self.saver = Some(Saver { effect: eff, since: std::time::Instant::now(), last: None, buf: None });
        self.tick_screensaver();
    }

    /// Stop the screensaver (a no-op when it is not running).
    pub fn screensaver_off(&mut self) {
        if self.saver.take().is_some() {
            crate::dbg_log("screensaver: off");
        }
    }

    /// Whether the screensaver is covering the screen.
    pub fn screensaver_active(&self) -> bool {
        self.saver.is_some()
    }

    /// Repaint the screensaver when it is due (its effect's frame rate, or the
    /// screen size changed). Called once per daemon tick; cheap when nothing is due.
    pub fn tick_screensaver(&mut self) {
        let (w, h) = (self.w, self.h);
        let Some(s) = self.saver.as_mut() else { return };
        let fps = s.effect.fps();
        let size_ok = s.buf.as_ref().map(|b| b.width() == w && b.height() == h).unwrap_or(false);
        let due = !size_ok
            || (fps > 0 && s.last.map(|t| t.elapsed() >= std::time::Duration::from_millis(1000 / fps as u64)).unwrap_or(true));
        if due {
            let ctx = crate::screensaver::Ctx { elapsed: s.since.elapsed(), now: crate::screensaver::local_time() };
            s.buf = Some(s.effect.frame(&ctx, w, h));
            s.last = Some(std::time::Instant::now());
        }
    }

    /// The frame to show while the screensaver is up: one top layer, no cursor,
    /// no image placements (so the client also removes any Kitty images).
    fn saver_frame(&self) -> Option<Frame> {
        let buf = self.saver.as_ref()?.buf.as_ref()?;
        Some(Frame {
            layers: vec![Layer { z: 10_000, origin: Point::new(0, 0), buf: buf.clone(), opacity: 1.0, scissor: None }],
            cursor: None,
            images: Vec::new(),
        })
    }

    /// Open (or re-focus) the keyboard-shortcut help window.
    fn open_help(&mut self) {
        if let Some(id) = self.help_win {
            if self.contents.contains_key(&id) {
                self.wm.unminimize(id);
                if self.simple { self.sync_app_size(id); }
                return;
            }
        }
        let (pw, ph) = crate::help::preferred_size(self.w, self.h);
        let rect = Rect::new(((self.w - pw) / 2).max(0), 1.max((self.h - ph) / 3), pw, ph);
        let id = self.wm.add_window("Shortcuts".into(), rect);
        self.app_keys.insert(id, "Shortcuts".into());
        self.contents.insert(id, WinContent::Help(crate::help::HelpView::new()));
        self.titles.push((id, "Shortcuts".into()));
        self.help_win = Some(id);
    }

    /// `true` when the focused window is the help window (keyboard routing).
    pub fn focused_is_help(&self) -> bool {
        matches!(
            self.wm.focused().and_then(|id| self.contents.get(&id)),
            Some(WinContent::Help(_))
        )
    }

    fn focused_help(&mut self) -> Option<&crate::help::HelpView> {
        let id = self.wm.focused()?;
        match self.contents.get(&id)? {
            WinContent::Help(h) => Some(h),
            _ => None,
        }
    }

    /// Text the client should put on the HOST terminal clipboard via OSC 52
    /// (one-shot; cleared on take).
    pub fn take_clipboard(&mut self) -> Option<String> {
        self.pending_clipboard.take()
    }

    /// Queue a screenshot; an empty path means the default timestamped file in
    /// the configured directory.
    fn request_screenshot(&mut self, path: String) {
        let path = if path.is_empty() {
            let home = dirs::home_dir().unwrap_or_default();
            let dir = match self.cfg.screenshot_dir.trim() {
                "" => Some(home.join("Pictures")).filter(|p| p.is_dir()).unwrap_or(home),
                d => match d.strip_prefix("~/") {
                    Some(rest) => home.join(rest),
                    None => std::path::PathBuf::from(d),
                },
            };
            let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            dir.join(format!("tetron-wm-screenshot-{ts}.png")).to_string_lossy().into_owned()
        } else {
            path
        };
        self.pending_screenshot = Some(path);
    }

    /// Path of a requested screenshot, if any (one-shot; cleared on take).
    pub fn take_screenshot(&mut self) -> Option<String> {
        self.pending_screenshot.take()
    }

    fn open_filemanager(&mut self) {
        let root = self.picker_root();
        self.open_filemanager_root(root);
    }

    /// Open (or re-focus) the file manager rooted at `root`, then load thumbnails.
    fn open_filemanager_root(&mut self, root: std::path::PathBuf) {
        crate::dbg_log(&format!("open_filemanager_root: {}", root.display()));
        if let Some(id) = self.filemanager_win {
            if self.contents.contains_key(&id) {
                self.wm.unminimize(id);
                return;
            }
        }
        let w = 90.min((self.w - 4).max(40));
        let h = 30.min((self.h - 4).max(12));
        let rect = Rect::new((self.w - w) / 2, 2, w, h);
        let id = self.wm.add_window("Files".into(), rect);
        self.app_keys.insert(id, "Files".into());
        self.contents.insert(
            id,
            WinContent::FileManager(crate::filemanager::DynFileManager::new_local(root, self.cfg.default_apps.clone())),
        );
        self.titles.push((id, "Files".into()));
        self.filemanager_win = Some(id);
        self.refresh_fm_thumbnails();
    }

    /// Open the file manager rooted at an arbitrary directory (test support).
    #[doc(hidden)]
    pub fn open_filemanager_at(&mut self, dir: std::path::PathBuf) {
        // Replace any existing FM so the new root takes effect.
        if let Some(id) = self.filemanager_win.take() {
            self.contents.remove(&id);
            self.titles.retain(|(tid, _)| *tid != id);
            self.wm.close(id);
        }
        self.open_filemanager_root(dir);
    }

    /// Open (or re-focus) a remote file browser for the saved system `name`.
    /// Resolving the remote home blocks briefly (one capped ssh call); failures
    /// open nothing and log the reason.
    fn open_remote_files(&mut self, name: &str) {
        // Re-focus an existing browser for this system.
        if let Some((&id, _)) = self.remote_fms.iter().find(|(_, (n, _, _))| n == name) {
            if self.contents.contains_key(&id) {
                self.wm.unminimize(id);
                return;
            }
        }
        let Some(sys) = self.systems.iter().find(|s| s.name == name).cloned() else {
            crate::dbg_log(&format!("rfiles: unknown system '{name}'"));
            return;
        };
        crate::dbg_log(&format!("rfiles: opening {} ({})", sys.name, sys.host));
        let fs = crate::fileops::SshFs::new(sys.host.clone(), sys.port);
        let Some(home) = fs.remote_home() else {
            crate::dbg_log(&format!(
                "rfiles: {} unreachable (no key auth / offline) — set it up via Systems → Add Remote",
                sys.host
            ));
            return;
        };
        let w = 90.min((self.w - 4).max(40));
        let h = 30.min((self.h - 4).max(12));
        let rect = Rect::new((self.w - w) / 2, 2, w, h);
        let title = format!("Files on {}", sys.name);
        let id = self.wm.add_window(title.clone(), rect);
        self.app_keys.insert(id, title.clone());
        self.contents.insert(
            id,
            WinContent::FileManager(crate::filemanager::DynFileManager::new_remote(
                sys.host.clone(),
                sys.port,
                home,
                self.cfg.default_apps.clone(),
            )),
        );
        self.titles.push((id, title));
        self.remote_fms.insert(id, (sys.name.clone(), sys.host, sys.port));
    }

    /// The ssh backend of the focused file-manager window (`None` = local).
    fn focused_fm_backend(&self) -> FmBackend {
        let id = self.wm.focused()?;
        self.remote_fms.get(&id).map(|(_, target, port)| (target.clone(), *port))
    }

    /// `true` when the focused window hosts the file manager.
    pub fn focused_is_filemanager(&self) -> bool {
        matches!(
            self.wm.focused().and_then(|id| self.contents.get(&id)),
            Some(WinContent::FileManager(_))
        )
    }

    /// `true` when the focused file manager has a text overlay open (new-folder /
    /// rename), so the client forwards typed characters as overlay input.
    pub fn filemanager_editing(&self) -> bool {
        matches!(
            self.wm.focused().and_then(|id| self.contents.get(&id)),
            Some(WinContent::FileManager(f)) if f.is_editing()
        )
    }

    /// Whether the focused file manager has its context (right-click) menu
    /// open; the client redirects Up/Down/Enter/Esc to the menu instead of
    /// normal navigation while this is true.
    pub fn filemanager_context(&self) -> bool {
        matches!(
            self.wm.focused().and_then(|id| self.contents.get(&id)),
            Some(WinContent::FileManager(f))
                if matches!(f.overlay(), Some(crate::filemanager::Overlay::Context { .. }))
        )
    }

    /// Alias kept for integration-test readability.
    #[doc(hidden)]
    pub fn focused_fm_context_open_for_test(&self) -> bool {
        self.filemanager_context()
    }

    fn focused_filemanager_mut(&mut self) -> Option<&mut crate::filemanager::DynFileManager> {
        let id = self.wm.focused()?;
        match self.contents.get_mut(&id)? {
            WinContent::FileManager(f) => Some(f),
            _ => None,
        }
    }

    /// Paste into the focused file manager. Same backend → the manager's own
    /// clipboard; different backend (local↔remote) → background `scp -r`.
    fn fm_paste(&mut self) {
        let dest_backend = self.focused_fm_backend();
        let cross = match (&self.transfer, &dest_backend) {
            (Some((src, _)), dst) => src != dst,
            (None, _) => false,
        };
        if !cross {
            if let Some(f) = self.focused_filemanager_mut() {
                f.paste();
            }
            return;
        }
        let Some((source, paths)) = self.transfer.clone() else { return };
        let Some(f) = self.focused_filemanager_mut() else { return };
        let cwd = f.cwd().to_path_buf();
        // Build one scp invocation per path; remote↔remote uses scp -3 (via us).
        let mut cmds: Vec<Vec<String>> = Vec::new();
        for p in &paths {
            let mut args: Vec<String> = vec!["-r".into()];
            match (&source, &dest_backend) {
                (Some((st, sp)), None) => {
                    if let Some(port) = sp { args.extend(["-P".into(), port.to_string()]); }
                    args.push(format!("{}:{}", st, p.display()));
                    args.push(format!("{}/", cwd.display()));
                }
                (None, Some((dt, dp))) => {
                    if let Some(port) = dp { args.extend(["-P".into(), port.to_string()]); }
                    args.push(p.display().to_string());
                    args.push(format!("{}:{}/", dt, cwd.display()));
                }
                (Some((st, _)), Some((dt, _))) => {
                    args.insert(0, "-3".into());
                    args.push(format!("{}:{}", st, p.display()));
                    args.push(format!("{}:{}/", dt, cwd.display()));
                }
                (None, None) => continue, // handled by the non-cross path
            }
            cmds.push(args);
        }
        f.set_status(format!("transferring {} item(s) in the background (scp)…", cmds.len()));
        crate::dbg_log(&format!("scp transfer: {} item(s) → {}", cmds.len(), cwd.display()));
        std::thread::spawn(move || {
            for args in cmds {
                crate::dbg_log(&format!("scp {}", args.join(" ")));
                match std::process::Command::new("scp")
                    .args(&args)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                {
                    Ok(st) if st.success() => crate::dbg_log("scp: ok"),
                    Ok(st) => crate::dbg_log(&format!("scp: FAILED ({st}) — args: {}", args.join(" "))),
                    Err(e) => crate::log_warn(&format!("scp: could not run: {e}")),
                }
            }
        });
    }

    /// Open (or re-focus) the activity-monitor window, then immediately refresh
    /// its row list so the user sees live data on first render.
    fn open_activity(&mut self) {
        if let Some(id) = self.activity_win {
            if self.contents.contains_key(&id) {
                self.wm.unminimize(id);
                self.refresh_activity();
                return;
            }
        }
        let w = 84.min((self.w - 4).max(40));
        let h = 22.min((self.h - 4).max(10));
        let rect = Rect::new((self.w - w) / 2, 2, w, h);
        let id = self.wm.add_window("Activity Monitor".into(), rect);
        self.app_keys.insert(id, "Activity".into());
        self.contents.insert(id, WinContent::Activity(Activity::new()));
        self.titles.push((id, "Activity Monitor".into()));
        self.activity_win = Some(id);
        self.refresh_activity();
    }

    /// `true` when the focused window hosts the activity monitor.
    pub fn focused_is_activity(&self) -> bool {
        matches!(
            self.wm.focused().and_then(|id| self.contents.get(&id)),
            Some(WinContent::Activity(_))
        )
    }

    /// `true` when the activity monitor's kill-confirm overlay is up; the
    /// client uses this to route Enter / Esc / y / n to the panel.
    pub fn activity_confirming(&self) -> bool {
        self.focused_activity()
            .map(|a| a.has_pending_kill())
            .unwrap_or(false)
    }

    fn focused_activity_mut(&mut self) -> Option<&mut Activity> {
        let id = self.wm.focused()?;
        match self.contents.get_mut(&id)? {
            WinContent::Activity(a) => Some(a),
            _ => None,
        }
    }

    fn focused_activity(&self) -> Option<&Activity> {
        let id = self.wm.focused()?;
        match self.contents.get(&id)? {
            WinContent::Activity(a) => Some(a),
            _ => None,
        }
    }

    /// Rebuild the activity monitor's row list from the current `AppHost`.
    /// Called every frame so the table stays live — but only does any work when
    /// the panel is actually *visible*. Probing every hosted app each frame is
    /// costly (`apphost_dims` clones a whole app grid via `snapshot`), so we must
    /// not pay it when the rows aren't on screen: otherwise every render —
    /// including each step of a window drag — stalls behind N full grid copies,
    /// which is what makes dragging feel sticky/jerky. A minimized panel (skipped
    /// by `build_frame`), or a non-focused one in simple mode (hidden by
    /// `build_frame_simple`), counts as not visible.
    pub fn refresh_activity(&mut self) {
        let win = match self.activity_win {
            Some(id)
                if matches!(self.contents.get(&id), Some(WinContent::Activity(_)))
                    && self.wm.get(id).is_some_and(|w| {
                        !w.minimized && (!self.simple || self.wm.focused() == Some(id))
                    }) =>
            {
                id
            }
            _ => return,
        };
        let entries: Vec<crate::apphost::AppListEntry> = self
            .apphost
            .list()
            .into_iter()
            .map(|id| {
                let alive = self.apphost.is_alive(id);
                let pid = self.apphost.pid(id);
                let age_secs = self
                    .apphost
                    .spawn_time(id)
                    .map(|t| t.elapsed().as_secs())
                    .unwrap_or(0);
                let (cols, rows) = self.apphost_dims(id);
                let (cmd, args) = self.apphost_launch_cmd(id);
                crate::apphost::AppListEntry {
                    app: id.0,
                    cmd,
                    args,
                    pid,
                    cols,
                    rows,
                    age_secs,
                    alive,
                }
            })
            .collect();
        if let Some(WinContent::Activity(a)) = self.contents.get_mut(&win) {
            a.set_rows(entries);
        }
    }

    /// Recover the launch command + args for `id` via the `AppHost` trait's
    /// own `launch_cmd` (both `LocalAppHost` and `RemoteAppHost` track it);
    /// empty strings when the host has no record (e.g. an unknown id).
    fn apphost_launch_cmd(&self, id: AppId) -> (String, Vec<String>) {
        if let Some((cmd, args)) = self.apphost.launch_cmd(id) {
            (cmd, args)
        } else {
            (String::new(), Vec::new())
        }
    }

    fn apphost_dims(&self, id: AppId) -> (i32, i32) {
        if let Some(snap) = self.apphost.snapshot(id) {
            (snap.width(), snap.height())
        } else {
            (0, 0)
        }
    }

    /// The cwd of the focused file manager (for launching an app in-place).
    fn focused_fm_cwd(&self) -> Option<std::path::PathBuf> {
        match self.wm.focused().and_then(|id| self.contents.get(&id)) {
            Some(WinContent::FileManager(f)) => Some(f.cwd().to_path_buf()),
            _ => None,
        }
    }

    /// Turn a pending [`FileManagerAction`] from the focused file manager into a
    /// real effect: open the builtin image viewer, or launch a TUI app in the
    /// file manager's current directory.
    fn drain_fm_action(&mut self) {
        // Compute everything that borrows `self` immutably first, so the
        // subsequent `&mut self` calls (open_image / launch_in) don't conflict.
        let action = self.focused_filemanager_mut().and_then(|f| f.take_action());
        let cwd = self.focused_fm_cwd();
        match action {
            Some(crate::filemanager::FileManagerAction::OpenImage(path)) => {
                crate::dbg_log(&format!("fm action: OpenImage {}", path.display()));
                self.open_image(path.to_string_lossy().to_string());
            }
            Some(crate::filemanager::FileManagerAction::RunApp { command, args }) => {
                crate::dbg_log(&format!("fm action: RunApp {}", command));
                let name = args
                    .last()
                    .and_then(|a| a.rsplit('/').next())
                    .unwrap_or(&command)
                    .to_string();
                self.open_file_in(name, command, args, cwd);
            }
            Some(crate::filemanager::FileManagerAction::Fullscreen(p)) => self.fullscreen_path(&p),
            Some(crate::filemanager::FileManagerAction::SetWallpaper(p)) => self.set_wallpaper_path(&p),
            Some(crate::filemanager::FileManagerAction::CopyPath(p)) => self.copy_path(&p),
            Some(crate::filemanager::FileManagerAction::TerminalAt(p)) => self.terminal_at(&p),
            Some(crate::filemanager::FileManagerAction::PlayFullscreen(p)) => self.fullscreen_video(&p),
            Some(crate::filemanager::FileManagerAction::Archive { path, list }) => self.archive(&path, list),
            Some(crate::filemanager::FileManagerAction::SendTo { system, path }) => self.send_to(&system, &path),
            Some(crate::filemanager::FileManagerAction::Cast(path)) => self.cast_file(&path),
            Some(crate::filemanager::FileManagerAction::Pack { path, pack }) => self.pack(&path, pack),
            None => {}
        }
    }

    /// Turn a pending [`DesktopAction`] from the desktop model into a real effect:
    /// open a folder in Files, open an image, launch an app for a file, run a pin,
    /// or unpin a shortcut. Mirrors [`drain_fm_action`](Self::drain_fm_action).
    fn drain_desktop_action(&mut self) {
        // Take the action first so the desktop borrow is dropped before the
        // subsequent `&mut self` effect calls.
        let action = self.desktop.take_action();
        match action {
            Some(crate::desktop::DesktopAction::Open(path)) => {
                crate::dbg_log(&format!("desktop action: Open {}", path.display()));
                let is_dir = path.is_dir();
                match crate::openwith::resolve(&path, is_dir, &self.cfg.default_apps) {
                    crate::openwith::OpenAction::Navigate => self.open_filemanager_root(path),
                    crate::openwith::OpenAction::Builtin("@image") => {
                        self.open_image(path.to_string_lossy().to_string());
                    }
                    crate::openwith::OpenAction::Builtin(_) => {}
                    crate::openwith::OpenAction::RunApp { command, args } => {
                        let name = args
                            .last()
                            .and_then(|a| a.rsplit('/').next())
                            .unwrap_or(&command)
                            .to_string();
                        let cwd = path.parent().map(|p| p.to_path_buf());
                        self.open_file_in(name, command, args, cwd);
                    }
                    crate::openwith::OpenAction::OpenWithMenu => {}
                }
            }
            Some(crate::desktop::DesktopAction::Run { command, args }) => {
                crate::dbg_log(&format!("desktop action: Run {}", command));
                self.launch_entry(AppEntry {
                    name: command.clone(),
                    command,
                    args,
                    category: None,
                    requires_cwd: None,
                    cwd: None,
                    cli: None,
                    warn: None,
                });
            }
            Some(crate::desktop::DesktopAction::Unpin(cmd)) => {
                crate::dbg_log(&format!("desktop action: Unpin {}", cmd));
                self.cfg.desktop_pins.retain(|p| p.command != cmd);
                let _ = self.cfg.save();
                self.reload_desktop();
            }
            None => {}
        }
    }

    /// Right-click on the desktop: open a context (icon) or empty-desktop menu,
    /// but only for clicks that fall through to the desktop (no window hit).
    fn handle_desktop_right(&mut self, p: Point) {
        if self.cfg.desktop_enabled && self.window_at_is_none(p) {
            self.desktop.right_click(p);
        }
    }

    /// Commit the desktop's rename / new-folder overlay, reloading on success.
    fn desktop_commit(&mut self) {
        if self.desktop.overlay_commit() {
            self.reload_desktop();
        }
    }

    /// Enter on a store row: launch the app if installed, else install it (the
    /// install command runs visibly in a new shell window).
    fn store_activate(&mut self) {
        let app = self.focused_store_mut().and_then(|s| s.selected_app());
        let Some(app) = app else { return };
        if crate::catalog::is_installed(&app.bin) {
            // Coding agents (flagged, or in the AI category) prompt for a dir.
            let requires_cwd = crate::catalog::recipe(&app.name).map(|r| r.requires_cwd).unwrap_or(false)
                || app.category == "AI";
            // CLI-flagged apps get the same `sh -lc … --help; exec $SHELL` wrapper
            // as launcher-driven launches (see `cli_wrap` / `launch_entry`).
            let (command, args) = if app.cli { cli_wrap(&app.bin, &[]) } else { (app.bin.clone(), Vec::new()) };
            self.launch_maybe_cwd(app.name.clone(), command, args, requires_cwd, None);
        } else {
            // Run the install visibly, then drop into a shell so the output (and
            // any errors) stay on screen. Closing the window triggers a $PATH
            // re-scan + launcher rebuild (see `reap_dead`).
            let cmd = store::install_command(app);
            let wrapped = format!(
                "{cmd}; echo; echo '── install finished — close this window (x) to refresh ──'; exec \"$SHELL\""
            );
            self.launch(format!("install: {}", app.name), "sh".into(), vec!["-lc".into(), wrapped]);
        }
    }

    /// Activate a launcher entry: open the store/settings for the pinned tetron-wm
    /// actions, otherwise spawn the app (prompting for a working directory when
    /// the entry is flagged `requires_cwd` and has no fixed `cwd`).
    fn launch_entry(&mut self, e: AppEntry) {
        match e.command.as_str() {
            "@store" => self.open_store(),
            "@settings" => self.open_settings(),
            "@files" => self.open_filemanager(),
            "@logs" => self.open_logs(),
            "@rfiles" => {
                if let Some(name) = e.args.first().cloned() {
                    self.open_remote_files(&name);
                }
            }
            "@activity" => self.open_activity(),
            "@clipboard" => {
                self.launcher.close();
                if !self.clipmenu.is_open() {
                    self.toggle_clipmenu();
                }
            }
            "@screenshot" => { self.launcher.close(); self.request_screenshot(String::new()); }
            "@help" => { self.launcher.close(); self.open_help(); }
            "@screensaver" => { self.launcher.close(); self.screensaver_on(None); }
            "@image" => { if let Some(p) = e.args.first().cloned() { self.open_image(p); } }
            // System actions from the launcher's bottom section — delegate to the
            // existing power-menu flows (confirm dialog / systems popover).
            "@systems" => { self.launcher.close(); self.power_menu.open_systems(); }
            "@exit" => self.confirm_power(crate::powermenu::PowerAction::Exit),
            "@restart" => self.confirm_power(crate::powermenu::PowerAction::Restart),
            "@shutdown" => self.confirm_power(crate::powermenu::PowerAction::Shutdown),
            "@suspend" => self.confirm_power(crate::powermenu::PowerAction::Suspend),
            "@reboot" => self.confirm_power(crate::powermenu::PowerAction::Reboot),
            "@poweroff" => self.confirm_power(crate::powermenu::PowerAction::PowerOff),
            _ => {
                let cli = e.cli.unwrap_or(false);
                let requires_cwd = e.requires_cwd.unwrap_or(false);
                if let Some(message) = e.warn {
                    // A warn-flagged entry (e.g. the Claude Code "skip
                    // permissions" variant) opens the modal first instead of
                    // launching; `LaunchWarnYes` replays the exact same launch
                    // via `launch_resolved`.
                    self.launch_warn.open(
                        crate::launchwarn::PendingLaunch {
                            name: e.name,
                            command: e.command,
                            args: e.args,
                            cli,
                            requires_cwd,
                            cwd: e.cwd,
                        },
                        message,
                    );
                } else {
                    self.launch_resolved(e.name, e.command, e.args, cli, requires_cwd, e.cwd);
                }
            }
        }
    }

    /// The tail of `launch_entry`'s default arm: apply the CLI
    /// help-then-shell wrapper (if flagged) and hand off to
    /// `launch_maybe_cwd`. Shared by the direct launch path and the
    /// launch-warning dialog's confirm action, so a warned launch proceeds
    /// exactly as it would have without the prompt.
    fn launch_resolved(&mut self, name: String, command: String, args: Vec<String>, cli: bool, requires_cwd: bool, cwd: Option<String>) {
        // CLI-flagged apps launch through the `sh -lc … --help; exec $SHELL`
        // wrapper instead of the bare binary (see `cli_wrap`); the window
        // title still shows the app name, and `requires_cwd`/`cwd` keep
        // working since the shell itself starts in the picked directory.
        let (command, args) = if cli { cli_wrap(&command, &args) } else { (command, args) };
        self.launch_maybe_cwd(name, command, args, requires_cwd, cwd)
    }

    /// Spawn a new PTY-backed window.
    ///
    /// If spawning fails, the window is removed and no dock entry
    /// is added (silently drops the launch request — the caller can surface an
    /// error later via a `CoreMsg` notification once that protocol exists).
    fn launch(&mut self, name: String, command: String, args: Vec<String>) {
        self.launch_in(name, command, args, None);
    }

    /// Open a new shell window (the dock "+" button / quick-launch). Uses `$SHELL`
    /// (falling back to `sh`); the window groups under the "Shell" app key.
    fn open_shell(&mut self) {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".into());
        self.launch("Shell".into(), shell, Vec::new());
    }

    /// Spawn a new PTY-backed window, starting the child in `cwd` (or the user's
    /// home when `None`).
    /// Open a file with its handler. A file for fresh goes into a fresh window
    /// that is already open, as a new tab, when that fresh can be reached
    /// (fresh 0.5 or later registers each window as `local-<pid>-…`); otherwise
    /// the handler starts in a new window.
    fn open_file_in(&mut self, name: String, command: String, args: Vec<String>, cwd: Option<std::path::PathBuf>) {
        if is_fresh(&command) {
            if let Some(file) = args.last() {
                if self.open_in_running_fresh(&command, file) {
                    return;
                }
            }
        }
        self.launch_in(name, command, args, cwd);
    }

    /// Send `file` to a fresh window tetron-wm hosts (most recently raised
    /// first) and focus it. `false` when there is none, or fresh is too old to
    /// list its instances: `--cmd daemon list` then fails and nothing else runs
    /// (older fresh would start a background server on `open-file`).
    fn open_in_running_fresh(&mut self, program: &str, file: &str) -> bool {
        let wins: Vec<(WindowId, u32)> = self
            .wm
            .z_ordered()
            .iter()
            .rev()
            .filter_map(|w| match self.contents.get(&w.id) {
                Some(WinContent::App(aid)) => {
                    let (cmd, _) = self.apphost.launch_cmd(*aid)?;
                    if !is_fresh(&cmd) || !self.apphost.is_alive(*aid) {
                        return None;
                    }
                    Some((w.id, self.apphost.pid(*aid)?))
                }
                _ => None,
            })
            .collect();
        if wins.is_empty() {
            return false;
        }
        let Some(list) = crate::system::run_capped(program, &["--cmd", "daemon", "list"], 2) else {
            crate::dbg_log("open with fresh: no instance list (fresh older than 0.5?), new window");
            return false;
        };
        for (id, pid) in wins {
            let Some(name) = fresh_instance_name(&list, pid) else { continue };
            // fresh reports on stderr, so success is the exit status.
            match crate::system::run_capped_status(program, &["--cmd", "daemon", "open-file", name, file], 3) {
                crate::system::Capped::Done => {
                    crate::dbg_log(&format!("open with fresh: {file} as a tab in window {id:?} ({name})"));
                    self.focus_window(id);
                    return true;
                }
                other => crate::log_warn(&format!("open with fresh: open-file into {name} failed: {other:?}")),
            }
        }
        false
    }

    fn launch_in(&mut self, name: String, command: String, args: Vec<String>, cwd: Option<std::path::PathBuf>) {
        // A bare launch of an app that exits without a URL (w3m) gets the start page.
        let args = if args.is_empty() && !self.cfg.web_home.is_empty() && crate::catalog::needs_url(&command) {
            vec![self.cfg.web_home.clone()]
        } else {
            args
        };
        crate::dbg_log(&format!("launch_in {} cmd={}", name, command));
        // Cascade new windows with a generous offset so each one is clearly
        // visible (not buried under the previous window), clamped so the whole
        // window stays on-screen within the work area.
        let n = self.titles.len() as i32;
        // Default large enough that demanding apps (e.g. btop needs 80×24
        // content → 82×26 outer) fit without complaint, clamped to the screen.
        let win_w = 84.min((self.w - 4).max(20));
        let win_h = 30.min((self.h - 4).max(6));
        let max_x = (self.w - win_w - 1).max(0);
        let max_y = (self.h - win_h).max(1); // keep on-screen (no bottom dock in v2)
        let x = (2 + n * 6).min(max_x);
        let y = (2 + n * 3).min(max_y);
        let rect = Rect::new(x, y, win_w, win_h);
        let id = self.wm.add_window(name.clone(), rect);
        // Optionally open maximized (fills the work area) before sizing the PTY.
        if self.cfg.launch_maximized {
            self.wm.maximize_toggle(id);
        }
        let content = self.wm.get(id).unwrap().content_rect();
        match self.apphost.spawn(&command, &args, cwd.as_deref(), content.w.max(1), content.h.max(1)) {
            Ok(app_id) => {
                self.app_keys.insert(id, name.clone());
                self.contents.insert(id, WinContent::App(app_id));
                self.titles.push((id, name));
                self.auto_tile_if_enabled();
                if self.simple {
                    self.sync_app_size(id);
                }
            }
            Err(_) => {
                self.wm.close(id);
            }
        }
    }

    /// Whether `p` is a spurious terminal mouse report: a big (> half-screen)
    /// *vertical* jump from the last cursor position that also lands on the top or
    /// bottom screen edge. The observed garbage teleports to the extreme row (e.g.
    /// a same-ms row 1 → row 71 "drag" that flings the window off the bottom), so
    /// we require BOTH the magnitude and an edge landing — a fast legitimate drag
    /// that ends mid-screen is *not* dropped (that over-broad magnitude-only test
    /// made fast vertical drags freeze and biased dragging toward one direction).
    /// Vertical only: horizontal jumps are legit (far-left brand → far-right power
    /// button) and off-screen horizontal motion is clamped by `wm::move_to`.
    fn is_spurious_jump(&self, p: Point) -> bool {
        let big = (p.y - self.cursor.y).abs() > self.h / 2;
        let to_edge = p.y <= 0 || p.y >= self.h - 1;
        big && to_edge
    }

    /// Route a mouse event through dock hit-testing then the WM input router.
    /// True while any modal overlay/menu owns the pointer. Selection must not
    /// start under these (a click should dismiss the overlay instead), so
    /// `mouse_select` defers to `handle_mouse` when one is open.
    fn overlay_active(&self) -> bool {
        self.clipmenu.is_open()
            || self.compat_dialog
            || self.dock_ctx.is_some()
            || self.win_menu.is_some()
            || self.dirpicker.is_some()
            || self.power_menu.is_open()
            || self.confirm_close.is_open()
            || self.win_picker.is_open()
            || self.launch_warn.is_open()
            || self.launcher.is_open()
            || self.tray.open().is_some()
            || self.dock_popup.is_some()
    }

    /// Mouse text-selection for terminal (app) windows: select by left-drag over
    /// a window's content, for middle-click paste. Returns true when the event
    /// is consumed as a selection interaction, so normal window routing is
    /// skipped. Engages only inside an App window's content area with no overlay
    /// open; chrome, borders, desktop and widget windows fall through unchanged.
    fn mouse_select(&mut self, kind: MouseKind, p: Point) -> bool {
        match kind {
            MouseKind::Down => {
                // A press elsewhere (or under an overlay) ends any shown
                // selection and does not start a new one.
                if self.overlay_active() {
                    return false;
                }
                // Occlusion-aware: only the topmost window's own content starts a
                // selection; a click on a titlebar/border (chrome) returns None
                // and must NOT fall through to a window underneath.
                let Some((id, aid, cr)) = self.topmost_app_content_at(p) else {
                    self.selection = None;
                    return false;
                };
                // Focus/raise the window we are selecting in, then anchor the
                // selection at the pressed cell (content-relative).
                self.wm.raise(id);
                let cell = (p.x - cr.x, p.y - cr.y);
                self.selection = Some(TextSel {
                    win: id,
                    aid,
                    cr,
                    anchor: cell,
                    head: cell,
                    dragging: true,
                });
                true
            }
            MouseKind::Drag => {
                let Some(sel) = self.selection.as_mut() else { return false };
                if !sel.dragging {
                    return false;
                }
                // Clamp the moving end to the content rect captured at press.
                let cr = sel.cr;
                sel.head = (
                    (p.x - cr.x).clamp(0, (cr.w - 1).max(0)),
                    (p.y - cr.y).clamp(0, (cr.h - 1).max(0)),
                );
                true
            }
            MouseKind::Up => {
                let Some(mut sel) = self.selection else { return false };
                if !sel.dragging {
                    return false;
                }
                sel.dragging = false;
                self.selection = Some(sel); // keep the highlight shown after release
                self.select_mode = false; // one-shot: back to normal mouse routing
                // Read the selected text from the app's current grid snapshot and
                // stash it as the primary selection for the next middle click.
                if let Some(buf) = self.apphost.snapshot(sel.aid) {
                    let text = extract_text(&buf, sel.anchor, sel.head);
                    if !text.is_empty() {
                        self.set_primary(&text);
                        self.clip_record(&text, crate::clipboard::ClipSource::Selection);
                    }
                }
                true
            }
            _ => false,
        }
    }

    /// Write `text` to the focused app's PTY (the keyboard-paste target), via the
    /// same input path as keystrokes. No-op if nothing is focused or the text is
    /// empty. Used by Shift+Insert (primary) and Ctrl+Shift+V (clipboard).
    fn paste_to_focused(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        // An open Settings text field (wallpaper path, colors, Apps form)
        // takes the paste; otherwise it goes to the focused app as before.
        if self.focused_settings_mut().is_some_and(|s| s.paste(text)) {
            return;
        }
        if let Some(id) = self.wm.focused() {
            if let Some(WinContent::App(aid)) = self.contents.get(&id) {
                let aid = *aid;
                let bytes = self.paste_bytes(aid, text);
                self.apphost.input(aid, &bytes);
            }
        }
    }

    /// The bytes to send for pasting `text` into app `aid`: wrapped in
    /// bracketed-paste markers when the app asked for them (so a shell or editor
    /// treats multi-line text as one paste instead of running each line). Any
    /// embedded end marker is removed so pasted text cannot break out.
    fn paste_bytes(&self, aid: AppId, text: &str) -> Vec<u8> {
        if self.apphost.mouse_mode(aid).bracketed_paste {
            let clean = text.replace("\x1b[201~", "");
            let mut v = b"\x1b[200~".to_vec();
            v.extend_from_slice(clean.as_bytes());
            v.extend_from_slice(b"\x1b[201~");
            v
        } else {
            text.as_bytes().to_vec()
        }
    }

    fn handle_mouse(&mut self, kind: MouseKind, p: Point) {
        // The clipboard history popover is modal while open: a click picks a row
        // or a button, anywhere else closes it.
        if kind == MouseKind::Down && self.clipmenu.is_open() {
            let hist = self.clip_history();
            if let Some(a) = self.clipmenu.click(p, self.w, self.h, &hist) {
                self.apply_clip_action(a);
            }
            return;
        }

        // The post-update compat dialog is the topmost modal of all: nothing
        // else reacts until the user picks "keep apps" or "restart app server".
        if kind == MouseKind::Down && self.compat_dialog {
            let (keep, restart) = compat_dialog_buttons(self.w, self.h);
            if restart.contains(p) {
                self.restart_apphost();
            } else if keep.contains(p) || !compat_dialog_rect(self.w, self.h).contains(p) {
                // "Keep apps" (or a click outside): dismiss; the restart stays
                // available in Settings → Updates once work is saved.
                crate::dbg_log("compat: user chose to keep apps for now");
                self.compat_dialog = false;
            }
            return;
        }

        // The dock context menu captures the next click: a row applies its
        // action; anywhere else dismisses (consuming the click, like the
        // other transient menus).
        // The title bar menu captures the next click the same way.
        if kind == MouseKind::Down && self.win_menu.is_some() {
            if let Some((id, menu)) = self.win_menu.take() {
                if let Some(action) = menu.item_at(p, self.win_menu_bounds()).cloned() {
                    self.run_window_action(id, action);
                }
            }
            return;
        }
        if kind == MouseKind::Down && self.dock_ctx.is_some() {
            if let Some((id, ax)) = self.dock_ctx.take() {
                let row = (0..DOCK_CTX_ROWS.len())
                    .find(|&i| dock_ctx_row_rect(ax, self.w, self.h, i).contains(p));
                match row {
                    Some(0) => self.wm.minimize(id),
                    Some(1) => {
                        self.wm.unminimize(id);
                        self.wm.maximize_toggle(id);
                        self.sync_app_size(id);
                    }
                    Some(2) => self.request_close(id),
                    Some(3) => {
                        // Reset: centre at half the work area's width/height —
                        // the rescue hatch for a mis-sized or stranded window.
                        self.wm.unminimize(id);
                        let work = self.wm.work_area();
                        let (nw, nh) = ((work.w / 2).max(20), (work.h / 2).max(5));
                        self.wm.move_to(id, work.x + (work.w - nw) / 2, work.y + (work.h - nh) / 2);
                        self.wm.resize_to(id, nw, nh);
                        self.sync_app_size(id);
                    }
                    _ => {}
                }
            }
            return;
        }

        // The power menu (dropdown / confirm dialog) is modal while open: route
        // the click to it and act on a confirmed choice.
        if kind == MouseKind::Down && self.power_menu.is_open() {
            match self.power_menu.on_click(p, self.w, self.h, &self.systems) {
                PowerClick::Act(outcome) => self.apply_power_outcome(outcome),
                PowerClick::Consumed => {}
            }
            return;
        }

        // The window picker is modal while open: a row focuses that window,
        // anywhere else dismisses it.
        if kind == MouseKind::Down && self.win_picker.is_open() {
            if let Some(id) = self.win_picker.on_click(p, self.w, self.h) {
                self.focus_window(id);
            }
            return;
        }

        // The confirm-close dialog is modal while open: route the click to it and
        // close the window if confirmed.
        if kind == MouseKind::Down && self.confirm_close.is_open() {
            if let Some(id) = self.confirm_close.on_click(p, self.w, self.h) {
                self.close(id);
            }
            return;
        }

        // The launch-warning dialog is modal while open: route the click to it
        // and run the pending launch if confirmed.
        if kind == MouseKind::Down && self.launch_warn.is_open() {
            if let Some(l) = self.launch_warn.on_click(p, self.w, self.h) {
                self.launch_resolved(l.name, l.command, l.args, l.cli, l.requires_cwd, l.cwd);
            }
            return;
        }

        // The working-directory picker captures clicks while open: a click on a
        // row selects + expands it; a click outside the box cancels.
        if kind == MouseKind::Down && self.dirpicker.is_some() {
            let (w, h) = (self.w, self.h);
            let hit = self.dirpicker.as_ref().and_then(|d| d.row_at(p, w, h));
            match hit {
                Some(i) => {
                    if let Some(d) = self.dirpicker.as_mut() {
                        d.select(i);
                        d.expand();
                    }
                }
                None => self.dirpicker = None,
            }
            return;
        }

        // An open launcher captures all clicks/moves: in Menu mode a move flies
        // out the hovered submenu and a click descends/launches; in Spotlight a
        // click launches the hit row. Nothing leaks through to window routing.
        if self.launcher.is_open() {
            match (self.launcher.mode(), kind) {
                (Some(crate::launcher::LauncherMode::Menu), MouseKind::Drag) => {
                    self.launcher.hover(p);
                    return;
                }
                (Some(crate::launcher::LauncherMode::Menu), MouseKind::Down) => {
                    if let Some(app) = self.launcher.click(p) {
                        self.launcher.close();
                        self.launch_entry(app);
                    } else if !self.launcher.point_in_menu(p) {
                        self.launcher.close();
                    }
                    return;
                }
                (Some(crate::launcher::LauncherMode::Menu), _) => return,
                (Some(crate::launcher::LauncherMode::Spotlight), MouseKind::Down) => {
                    let rendered = self.launcher.render(self.w, self.h);
                    let hit = rendered
                        .items
                        .into_iter()
                        .find(|(_, r)| r.contains(p))
                        .map(|(entry, _)| entry);
                    self.launcher.close();
                    if let Some(entry) = hit {
                        self.launch_entry(entry);
                    }
                    return;
                }
                _ => {}
            }
        }

        // An open tray popover captures the next click: apply its intent, or
        // dismiss when the click misses every hot-zone.
        if kind == MouseKind::Down && self.tray.open().is_some() {
            let rendered = self.tray_render_now();
            if let Some(intent) = self.tray.on_popover_click(p, &rendered) {
                self.apply_intent(intent);
                return;
            }
            self.tray.close();
            return;
        }

        // Menubar brand opens the launcher dropdown; quit button, tray segments,
        // and dock clicks are checked before normal window routing.
        if kind == MouseKind::Down {
            if menubar_mode_region().contains(p) {
                self.launcher.close();
                self.power_menu.close();
                self.toggle_simple();
                return;
            }
            if menubar_assistant_region().contains(p) {
                self.launcher.close();
                self.power_menu.close();
                self.toggle_assistant();
                return;
            }
            if menubar_brand_region().contains(p) {
                self.power_menu.close();
                self.launcher.toggle_menu();
                return;
            }
            let segs = self.tray_segments_now();
            // A click on the SELECT marker cancels select mode.
            if segs.iter().any(|s| s.kind == crate::tray::SegmentKind::Select && s.rect.contains(p)) {
                self.select_mode = false;
                return;
            }
            if segs.iter().any(|s| s.kind == crate::tray::SegmentKind::Clipboard && s.rect.contains(p)) {
                self.toggle_clipmenu();
                return;
            }
            if self.tray.on_menubar_click(p, &segs) {
                return;
            }

            // If a dock group popup is open, hit-test it first (modal for
            // clicks while it is showing). A click on a row focuses that
            // window; a click anywhere else dismisses the popup.
            if self.dock_popup.is_some() {
                let popup_rows = self.dock_popup_rows();
                let (_layers, row_rects) = self.render_popup_for_hit_test(&popup_rows);
                for (win_id, row_rect) in &row_rects {
                    if row_rect.contains(p) {
                        let id = *win_id;
                        self.dock_popup = None;
                        self.wm.unminimize(id);
                        if self.simple { self.sync_app_size(id); }
                        return;
                    }
                }
                // Click outside popup → dismiss it, then continue normal routing
                self.dock_popup = None;
                return;
            }

            // The menubar "+" button opens a new shell window.
            if menubar_new_shell_region().contains(p) {
                self.dock_popup = None;
                self.open_shell();
                return;
            }

            let items = self.dock_items();
            // The "…+N" marker opens the list of hidden windows.
            if let Some((r, _)) = menubar_taskbar_overflow(self.w, &items, &self.tray_segments_now()) {
                if r.contains(p) {
                    self.dock_popup = Some(OVERFLOW_POPUP.to_string());
                    return;
                }
            }
            for (idx, r) in self.dock_regions() {
                if r.contains(p) {
                    match &items[idx].kind {
                        DockKind::Single(id) => {
                            let id = *id;
                            self.dock_popup = None;
                            self.wm.unminimize(id);
                            if self.simple { self.sync_app_size(id); }
                        }
                        DockKind::Group(key, _) => {
                            let key = key.clone();
                            self.dock_popup = if self.dock_popup.as_deref() == Some(key.as_str()) {
                                None
                            } else {
                                Some(key)
                            };
                        }
                    }
                    return;
                }
            }
        }
        // An open desktop menu floats above the windows: a left press first tries
        // a menu item, else dismisses the menu, before any window routing.
        if kind == MouseKind::Down && self.cfg.desktop_enabled && self.desktop.overlay().is_some() {
            self.handle_desktop_menu_click(p);
            return;
        }

        // A desktop drag in progress captures motion + release (the icon drag is
        // tracked inside the desktop model, separate from window `self.drag`).
        if self.desktop.dragging() {
            match kind {
                MouseKind::Drag => {
                    self.desktop.drag_to(p);
                    return;
                }
                MouseKind::Up => {
                    if self.desktop.end_drag(p) {
                        self.persist_desktop_positions();
                    }
                    return;
                }
                MouseKind::Down | MouseKind::Move => {}
            }
        }

        // Minimized windows are hidden, so they never receive mouse events.
        let windows: Vec<_> = self
            .wm
            .z_ordered()
            .into_iter()
            .filter(|w| !w.minimized)
            .cloned()
            .collect();
        let action = route_mouse(kind, p, &windows, self.drag);
        // A left press that hits no window and no chrome falls through to the
        // desktop: begin a drag on an icon (which also selects it), else clear
        // the selection. A plain press-without-move still leaves the icon
        // selected; a drag moves + persists.
        if self.cfg.desktop_enabled
            && matches!(action, Action::None)
            && self.drag.is_none()
            && kind == MouseKind::Down
        {
            match self.desktop.icon_at(p) {
                Some(_) => self.desktop.begin_drag(p),
                None => self.desktop.click(p, false),
            }
            return;
        }
        self.exec(action, p);
    }

    /// A click while the desktop menu is open: act on the item under `p`, or
    /// dismiss the menu if the click missed it.
    fn handle_desktop_menu_click(&mut self, p: Point) {
        match self.desktop.menu_item_at(p) {
            Some(action) => self.run_desktop_menu_action(action),
            None => self.desktop.cancel_overlay(),
        }
    }

    /// Perform a desktop menu action (click or Enter). "Open with…" keeps the
    /// menu open with the app choices instead.
    fn run_desktop_menu_action(&mut self, action: crate::contextmenu::MenuAction) {
        use crate::contextmenu::MenuAction as A;
        let idx = self.desktop.context_idx();
        let path = idx.and_then(|i| self.desktop.icon_path(i));
        match action {
            A::Open => {
                self.desktop.cancel_overlay();
                if let Some(i) = idx {
                    let r = self.desktop.tile_rect(self.desktop.icons()[i].cell);
                    self.desktop.double_click(Point::new(r.x + 1, r.y));
                    self.drain_desktop_action();
                }
            }
            A::OpenWith => {
                let items = path.as_deref().map(crate::contextmenu::open_with_actions).unwrap_or_default();
                if items.is_empty() {
                    self.desktop.cancel_overlay();
                } else {
                    self.desktop.menu_replace(items);
                }
            }
            A::OpenWithApp(cmd) => {
                self.desktop.cancel_overlay();
                if let Some(path) = path {
                    crate::dbg_log(&format!("desktop: open {} with {cmd}", path.display()));
                    match crate::openwith::open_with(&cmd, &path) {
                        crate::openwith::OpenAction::Builtin("@image") => self.open_image(path.to_string_lossy().to_string()),
                        crate::openwith::OpenAction::RunApp { command, args } => {
                            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| command.clone());
                            self.open_file_in(name, command, args, path.parent().map(|p| p.to_path_buf()));
                        }
                        _ => {}
                    }
                }
            }
            A::Rename => {
                if let Some(i) = idx {
                    self.desktop.begin_rename(i);
                }
            }
            A::Trash => {
                if self.desktop.trash_selection() {
                    self.reload_desktop();
                }
            }
            A::Unpin => {
                let cmd = idx.and_then(|i| self.desktop.icon_command(i));
                self.desktop.cancel_overlay();
                if let Some(cmd) = cmd {
                    self.cfg.desktop_pins.retain(|p| p.command != cmd);
                    let _ = self.cfg.save();
                    self.reload_desktop();
                }
            }
            A::NewFolder => self.desktop.begin_new_folder(),
            A::CleanUp => {
                self.desktop.cancel_overlay();
                self.desktop.clean_up();
                self.persist_desktop_positions();
            }
            A::ViewFullscreen | A::SetWallpaper | A::CopyPath | A::OpenTerminalHere => {
                self.desktop.cancel_overlay();
                let target = path.clone().unwrap_or_else(|| self.desktop.dir().to_path_buf());
                match action {
                    A::ViewFullscreen => self.fullscreen_path(&target),
                    A::SetWallpaper => self.set_wallpaper_path(&target),
                    A::CopyPath => self.copy_path(&target),
                    _ => self.terminal_at(&target),
                }
            }
            A::PlayFullscreen | A::ExtractHere | A::ListContents | A::SendToSystem(_) | A::CastTo | A::GetInfo | A::PackAs(_) => {
                self.desktop.cancel_overlay();
                if let Some(p) = path {
                    match action {
                        A::PlayFullscreen => self.fullscreen_video(&p),
                        A::CastTo => self.cast_file(&p),
                        A::ExtractHere => self.archive(&p, false),
                        A::ListContents => self.archive(&p, true),
                        A::SendToSystem(name) => self.send_to(&name, &p),
                        A::PackAs(pack) => self.pack(&p, pack),
                        _ => self.desktop_get_info(&p),
                    }
                }
            }
            A::SendTo | A::Compress | A::Encrypt => {
                let items = match action {
                    A::SendTo => crate::contextmenu::send_to_actions(),
                    _ => crate::contextmenu::pack_actions(action == A::Encrypt),
                };
                if items.is_empty() {
                    self.desktop.cancel_overlay();
                } else {
                    self.desktop.menu_replace(items);
                }
            }
            A::Copy | A::Cut => {
                self.desktop.cancel_overlay();
                if let Some(p) = path {
                    crate::fileops::clip_set(vec![p], action == A::Cut);
                }
            }
            A::Paste | A::PasteInto => {
                self.desktop.cancel_overlay();
                let dir = if action == A::PasteInto { path } else { Some(self.desktop.dir().to_path_buf()) };
                if let Some(dir) = dir {
                    let (done, err) = crate::fileops::paste_clip_into(&dir);
                    crate::dbg_log(&format!("desktop: pasted {done} into {}{}", dir.display(), err.map(|e| format!(", WARN {e}")).unwrap_or_default()));
                    self.reload_desktop();
                }
            }
            // The confirmation is the file manager's: open it there.
            A::EmptyTrash => {
                self.desktop.cancel_overlay();
                self.open_filemanager();
                if let Some(f) = self.focused_filemanager_mut() {
                    f.begin_empty_trash();
                }
            }
            A::Minimize | A::Maximize | A::MoveWindow | A::RenameWindow | A::CloseWindow => self.desktop.cancel_overlay(),
        }
    }

    /// Execute a resolved [`Action`] against the window manager and app state.
    fn exec(&mut self, action: Action, p: Point) {
        match action {
            Action::BeginMove(id) => {
                self.wm.raise(id);
                // Guard against a window that vanished between hit-test and here
                // (its app may have just died); drop the move instead of panicking.
                let Some(r) = self.wm.get(id).map(|w| w.rect) else { return };
                self.drag = Some(Hit::Moving {
                    id,
                    grab_dx: p.x - r.x,
                    grab_dy: p.y - r.y,
                });
                self.drag_start = Some(p);
                self.drag_armed = false;
            }
            Action::BeginResize(id) => {
                self.wm.raise(id);
                self.drag = Some(Hit::Resizing { id });
                self.drag_start = Some(p);
                self.drag_armed = false;
            }
            Action::MoveTo { id, x, y } => {
                // Ignore sub-threshold motion so a plain click (or jitter) on a
                // titlebar never untiles/moves the window — only a real drag does.
                if !self.drag_armed {
                    let moved = self.drag_start
                        .map(|s| (p.x - s.x).abs() + (p.y - s.y).abs() >= DRAG_THRESHOLD)
                        .unwrap_or(true);
                    if !moved {
                        return;
                    }
                    self.drag_armed = true;
                }
                self.wm.move_to(id, x, y);
                // Show a target-cell highlight when the pointer nears an edge.
                let work = Rect::new(0, 1, self.w, self.h - 1);
                self.drag_preview = if self.cfg.snapping_enabled && near_edge(p, work, self.cfg.snap_threshold) {
                    let grid = self.grid();
                    let (row, col) = grid.cell_at(work, p);
                    Some(grid.cell_rect(work, row, col, self.cfg.tile_gap))
                } else {
                    None
                };
            }
            Action::ResizeTo { id, w, h } => {
                // Same drag threshold: a plain click on a window edge must not
                // resize/untile it; only a real drag does.
                if !self.drag_armed {
                    let moved = self.drag_start
                        .map(|s| (p.x - s.x).abs() + (p.y - s.y).abs() >= DRAG_THRESHOLD)
                        .unwrap_or(true);
                    if !moved {
                        return;
                    }
                    self.drag_armed = true;
                }
                // The window may have been closed mid-drag (e.g. its app crashed
                // between mouse-down and this motion event). A stale id is not a
                // bug — just drop the resize rather than unwrap a None.
                let Some(r) = self.wm.get(id).map(|w| w.rect) else { return };
                self.wm.resize_to(id, w - r.x + 1, h - r.y + 1);
                self.sync_app_size(id);
            }
            Action::Close(id) => self.request_close(id),
            Action::Minimize(id) => self.wm.minimize(id),
            Action::ToggleMaximize(id) => {
                self.wm.maximize_toggle(id);
                self.sync_app_size(id);
            }
            Action::FocusAndForward { id, local } => {
                self.wm.raise(id);
                // The store handles content clicks (category/row/install); PTY apps
                // are keyboard-first for now, so raising is enough for them.
                let cr = self.wm.get(id).map(|w| w.content_rect());
                let mut store_click = crate::store::StoreClick::None;
                let mut settings_changed = false;
                let mut fm_clicked = false;
                let mut activity_kill: Option<u64> = None;
                if let Some(cr) = cr {
                    match self.contents.get_mut(&id) {
                        Some(WinContent::Store(s)) => store_click = s.handle_click(local, cr.w, cr.h),
                        Some(WinContent::Settings(s)) => settings_changed = s.handle_click(local, cr.w, cr.h),
                        // The mouse path carries no modifiers, so a click is a plain
                        // single-select / toolbar-nav (Ctrl/Shift-select and
                        // double-click-to-open are keyboard-driven for v1).
                        // While the context menu is open it captures every click
                        // (item → act, elsewhere → dismiss) before normal FM
                        // click handling ever sees it.
                        Some(WinContent::FileManager(f)) => {
                            fm_clicked = if f.context_menu_open() {
                                f.context_menu_click(local, cr.w, cr.h)
                            } else {
                                f.handle_click(local, cr.w, cr.h, false, false)
                            };
                        }
                        Some(WinContent::Activity(a)) => activity_kill = a.handle_click(local, cr.w),
                        _ => {}
                    }
                }
                if let Some(app_id) = activity_kill {
                    self.apphost.kill(AppId(app_id));
                }
                if store_click == crate::store::StoreClick::UpdateList {
                    self.start_list_update();
                }
                if store_click == crate::store::StoreClick::Activate {
                    self.store_activate();
                }
                if settings_changed {
                    // A click can have armed an Updates action (Check / Install);
                    // fire it on the mouse path too, not just the keyboard one.
                    self.drain_settings_action();
                    self.sync_settings();
                }
                if fm_clicked {
                    self.drain_fm_action();
                }
            }
            Action::EndDrag => {
                // Only consider drop-to-snap if the drag actually moved (armed);
                // a plain titlebar click leaves a tiled window exactly as it was.
                if let (Some(Hit::Moving { id, .. }), true) = (self.drag, self.drag_armed) {
                    let work = Rect::new(0, 1, self.w, self.h - 1);
                    if self.cfg.snapping_enabled && near_edge(p, work, self.cfg.snap_threshold) {
                        let grid = self.grid();
                        let (row, col) = grid.cell_at(work, p);
                        // In auto-tile mode, dropping onto an occupied cell swaps
                        // the two windows; otherwise place into the target cell.
                        match self.window_in_cell(grid, row, col, id) {
                            Some(other) if self.cfg.auto_tile => {
                                self.wm.swap_cells(id, other);
                                self.sync_app_size(id);
                                self.sync_app_size(other);
                            }
                            _ => {
                                self.wm.send_to_cell(id, grid, row, col, self.cfg.tile_gap);
                                self.sync_app_size(id);
                            }
                        }
                    }
                }
                self.drag = None;
                self.drag_start = None;
                self.drag_armed = false;
                self.drag_preview = None;
            }
            Action::None => {}
        }
    }

    /// The configured tiling grid (clamped to 1..=6 on each axis).
    fn grid(&self) -> crate::geometry::Grid {
        crate::geometry::Grid {
            rows: self.cfg.grid_rows.clamp(1, 6),
            cols: self.cfg.grid_cols.clamp(1, 6),
        }
    }

    /// The id of a non-`except` window currently tiled in cell `(row, col)`.
    fn window_in_cell(&self, _grid: crate::geometry::Grid, row: u8, col: u8, except: WindowId) -> Option<WindowId> {
        self.wm
            .z_ordered()
            .into_iter()
            .find(|w| w.id != except && w.state == crate::window::WindowState::Tiled { row, col })
            .map(|w| w.id)
    }

    /// Re-tile all windows into the grid when auto-tile is on (called after a
    /// window opens, closes, or the screen resizes).
    fn auto_tile_if_enabled(&mut self) {
        if self.cfg.auto_tile {
            let grid = self.grid();
            self.wm.tile_all(grid, self.cfg.tile_gap);
            self.sync_all_app_sizes();
        }
    }

    /// Resize every window's hosted app to its current content rect (after a
    /// bulk re-tile).
    fn sync_all_app_sizes(&mut self) {
        let ids: Vec<WindowId> = self.wm.z_ordered().iter().map(|w| w.id).collect();
        for id in ids {
            self.sync_app_size(id);
        }
    }

    /// Tell the app instance for `id` to resize to match the window's current
    /// content rect (or the full work area when in simple mode and focused).
    fn sync_app_size(&mut self, id: WindowId) {
        let target = if self.simple && self.wm.focused() == Some(id) {
            self.simple_content_rect()
        } else if let Some(w) = self.wm.get(id) {
            w.content_rect()
        } else {
            return;
        };
        if let Some(WinContent::App(aid)) = self.contents.get(&id) {
            let aid = *aid;
            self.apphost.resize(aid, target.w.max(1), target.h.max(1));
        }
    }

    /// Kill a window's content, remove its dock entry, and close the WM window.
    /// Handle a close request from the titlebar ✕. App windows (whose close kills
    /// a running process) raise a modal confirm dialog first; built-in panels
    /// (Store / Settings / File Manager) close immediately.
    fn request_close(&mut self, id: WindowId) {
        if matches!(self.contents.get(&id), Some(WinContent::App(_))) {
            let title = self
                .titles
                .iter()
                .find(|(i, _)| *i == id)
                .map(|(_, t)| t.clone())
                .unwrap_or_default();
            self.confirm_close.open(id, title);
        } else {
            self.close(id);
        }
    }

    fn close(&mut self, id: WindowId) {
        if self.move_mode.as_ref().is_some_and(|m| m.id == id) {
            self.move_mode = None;
        }
        // Closing Settings with an unsaved color preview puts the saved colors back.
        let revert_preview = matches!(self.contents.get(&id), Some(WinContent::Settings(s)) if s.previewing());
        // An install window is kept alive (it `exec`s a shell after the install so
        // the output stays readable), so it never trips `reap_dead`; instead we
        // refresh here when the user closes it, so the new app appears immediately.
        let was_install = self
            .titles
            .iter()
            .any(|(i, t)| *i == id && t.starts_with("install:"));
        crate::dbg_log(&format!(
            "close window {:?}{}",
            id,
            if was_install { " (install)" } else { "" }
        ));
        if let Some(WinContent::App(aid)) = self.contents.remove(&id) {
            self.apphost.kill(aid);
            self.apphost.remove(aid);
        }
        if self.store_win == Some(id) {
            self.store_win = None;
        }
        if self.settings_win == Some(id) {
            self.settings_win = None;
        }
        if self.filemanager_win == Some(id) {
            self.filemanager_win = None;
        }
        if self.logs_win == Some(id) {
            self.logs_win = None;
        }
        if self.activity_win == Some(id) {
            self.activity_win = None;
        }
        self.remote_fms.remove(&id);
        self.titles.retain(|(i, _)| *i != id);
        self.app_keys.remove(&id);
        // Close popup if its group no longer exists after this window closes.
        if let Some(ref key) = self.dock_popup.clone() {
            let still_has_group = key == OVERFLOW_POPUP || self.app_keys.values().any(|k| k == key);
            if !still_has_group {
                self.dock_popup = None;
            }
        }
        self.wm.close(id);
        if revert_preview {
            self.push_terminal_colors();
        }
        if was_install {
            self.refresh_installed_apps();
        }
        self.auto_tile_if_enabled();
    }

    /// Re-scan `$PATH` for newly-installed binaries and rebuild the launcher so a
    /// just-installed app appears without a daemon restart. Shared by `close`
    /// (install window dismissed) and `reap_dead` (install process exited).
    fn refresh_installed_apps(&mut self) {
        crate::catalog::refresh_installed();
        self.launcher = Launcher::new(Self::build_launcher_apps(&self.cfg, &self.systems));
    }

    // ── Frame builder ─────────────────────────────────────────────────────────

    /// Render the rename text-entry overlay over a window's titlebar row.
    ///
    /// Returns a single 1-row [`Layer`] at high z (2000) drawn over the title
    /// area of the window, showing the current buffer and a cursor block.
    /// The move-mode hint drawn over the window's title (same area as the rename
    /// overlay), in reverse colours so the window being moved stands out.
    fn render_move_hint(&self, win: &crate::window::Window, resize: bool) -> Vec<Layer> {
        let t = crate::theme::current();
        let r = win.rect;
        let btn_reserve = if r.w >= 9 { 9 } else { 4 };
        let title_w = (r.w - 2 - btn_reserve).max(1);
        let text = if resize {
            " RESIZE  arrows, Shift x5, r move, Enter done, Esc undo "
        } else {
            " MOVE  arrows, Shift x5, r resize, Enter done, Esc undo "
        };
        let mut row = crate::buffer::CellBuffer::new(title_w, 1);
        row.fill(crate::cell::Cell { ch: ' ', fg: t.title_focus, bg: t.title_fg, attrs: Default::default() });
        let shown: String = text.chars().take(title_w as usize).collect();
        row.write_str(0, 0, &shown, t.title_focus, t.title_fg);
        vec![Layer { z: 2000, origin: Point::new(r.x + 2, r.y), buf: row, opacity: 1.0, scissor: None }]
    }

    fn render_rename_overlay(&self, win: &crate::window::Window, buf: &str) -> Vec<Layer> {
        let t = crate::theme::current();
        let r = win.rect;
        // Title area: skip the 2-cell left indent; leave room for control buttons.
        let btn_reserve = if r.w >= 9 { 9 } else { 4 };
        let title_start = r.x + 2;
        let title_w = (r.w - 2 - btn_reserve).max(1);
        let mut row = crate::buffer::CellBuffer::new(title_w, 1);
        // Fill background.
        row.fill(crate::cell::Cell { ch: ' ', fg: t.title_fg, bg: t.title_focus, attrs: Default::default() });
        // Draw the buffer text (truncated to fit).
        let display: String = buf.chars().take(title_w as usize).collect();
        for (i, ch) in display.chars().enumerate() {
            row.set(i as i32, 0, crate::cell::Cell { ch, fg: t.title_fg, bg: t.title_focus, attrs: Default::default() });
        }
        // Draw cursor block after the text.
        let cursor_x = display.chars().count().min((title_w - 1) as usize) as i32;
        row.set(cursor_x, 0, crate::cell::Cell { ch: ' ', fg: t.title_focus, bg: t.title_fg, attrs: Default::default() });
        vec![Layer { z: 2000, origin: Point::new(title_start, r.y), buf: row, opacity: 1.0, scissor: None }]
    }

    /// Build a complete [`Frame`] from the current session state.
    ///
    /// The frame contains (bottom to top):
    /// 1. Window shadow + body layers for every open window (z-ordered).
    /// 2. The menubar layer (z = 1000).
    /// 3. The dock layer (z = 1000).
    ///
    /// Build the chafa cell wallpaper sized to the `w`×`h` grid, or `None` for a
    /// solid desktop. The source is `TETRON_WM_WALLPAPER` (dev override) if set,
    /// else the `wallpaper` config key when `wallpaper_enabled` is on. The config
    /// path is resolved via [`wallpaper::resolve_path`] (absolute / `~` / relative
    /// to the config dir). A missing file or missing `chafa` returns `None`
    /// (desktop stays solid) and logs why.
    fn build_wallpaper(w: i32, h: i32, cfg: &Config) -> Option<crate::buffer::CellBuffer> {
        // Black and white: an image becomes noise behind the icons once its
        // colours are gone, so keep the plain desktop.
        if crate::display::depth() == crate::display::ColorDepth::Mono {
            return None;
        }
        // TETRON_WM_WALLPAPER overrides the config (and its on/off switch).
        let env = std::env::var("TETRON_WM_WALLPAPER").ok().filter(|p| !p.is_empty());
        if env.is_none() && !cfg.wallpaper_enabled {
            return None;
        }
        // `default` shows the theme's own picture (a file next to its theme
        // file, else the one bundled with it); anything else is the user's pick.
        let themed = env.is_none() && crate::wallpaper::is_default(cfg.wallpaper.as_deref());
        let theme_spec = if themed { crate::theme::wallpaper_for(&cfg.theme) } else { None };
        let spec = theme_spec.as_deref().or(env.as_deref()).or(cfg.wallpaper.as_deref());
        if let crate::wallpaper::Source::Solid(c) = crate::wallpaper::draw_for(spec, themed, crate::display::depth())? {
            return Some(crate::wallpaper::solid(c, w, h));
        }
        // `default`, empty or unset draw the built-in image, and so does a
        // configured file that is missing: the desktop only goes plain when
        // the wallpaper is switched off (or chafa is missing).
        let choice = crate::wallpaper::choose(spec, &cfg.theme);
        if let Some(m) = &choice.missing {
            crate::log_warn(&format!("wallpaper: file not found: {}, using the built-in image", m.display()));
        }
        let Some(path) = choice.path else {
            crate::log_warn("wallpaper: could not write the built-in image to the data dir");
            return None;
        };
        let s = path.to_string_lossy();
        match crate::wallpaper::from_image(&s, w, h) {
            Some(buf) => {
                crate::log_changed("wallpaper", &format!("{} {w}x{h}", path.display()), &format!("wallpaper: loaded {} ({w}x{h})", path.display()));
                Some(buf)
            }
            None => {
                crate::log_warn(&format!("wallpaper: chafa failed/missing for {} — solid fallback (install chafa)", path.display()));
                None
            }
        }
    }

    /// The cursor is set to the last known mouse position.
    pub fn build_frame(&self) -> Frame {
        self.publish_interest();
        if let Some(f) = self.saver_frame() {
            return f;
        }
        if self.simple {
            return self.build_frame_simple();
        }
        let mut layers: Vec<Layer> = Vec::new();
        let focused = self.wm.focused();

        // PROTOTYPE: the chafa wallpaper is the opaque base at z=0 (pushed first).
        if let Some(wp) = &self.wallpaper {
            layers.push(Layer { z: 0, origin: Point::new(0, 0), buf: wp.clone(), opacity: 1.0, scissor: None });
        }
        // The desktop icon layer sits at z=0, beneath every window (z≥1). Its
        // fill is TRANSPARENT (desktop.rs), so with a wallpaper pushed first the
        // icons/labels blend on top of it; without one, the compositor's
        // desktop_bg base shows through as before.
        if self.cfg.desktop_enabled {
            let buf = self.desktop.render(self.w, self.h, self.wallpaper.is_some(), self.cfg.desktop_scrim_color());
            layers.push(Layer { z: 0, origin: Point::new(0, 0), buf, opacity: 1.0, scissor: None });
        }

        for w in self.wm.z_ordered() {
            if w.minimized {
                continue; // hidden to the dock
            }
            let cr = w.content_rect();
            let mut content = self.contents.get(&w.id)
                .map(|c| c.render(self.apphost.as_ref(), cr.w, cr.h))
                .unwrap_or_else(|| crate::buffer::CellBuffer::new(cr.w, cr.h));
            // Paint the mouse text-selection highlight onto this window's grid.
            if let Some(sel) = self.selection {
                if sel.win == w.id {
                    highlight_selection(&mut content, sel.anchor, sel.head);
                }
            }
            // Draw the focused terminal's text cursor (the frame's `cursor` field
            // is the mouse pointer, so the caret must be painted into the grid).
            if Some(w.id) == focused {
                if let Some(WinContent::App(aid)) = self.contents.get(&w.id) {
                    if let Some((cx, cy)) = self.apphost.cursor(*aid) {
                        draw_cursor_cell(&mut content, cx as i32, cy as i32);
                    }
                }
            }
            layers.extend(render_window(w, &content, Some(w.id) == focused, self.cfg.window_shadows));
        }

        // Drag-to-cell preview: a translucent highlight of the target cell,
        // above the windows but below the chrome.
        if let Some(r) = self.drag_preview {
            let t = crate::theme::current();
            let mut buf = crate::buffer::CellBuffer::new(r.w, r.h);
            let mut tint = t.accent;
            tint.a = 70; // translucent so the windows below show through
            buf.fill(crate::cell::Cell { ch: ' ', fg: crate::cell::Rgba::TRANSPARENT, bg: tint, attrs: Default::default() });
            layers.push(Layer { z: 900, origin: Point::new(r.x, r.y), buf, opacity: 1.0, scissor: None });
        }

        // Keyboard move mode: the key hint replaces the window's title.
        if let Some(m) = &self.move_mode {
            if let Some(win) = self.wm.get(m.id) {
                layers.extend(self.render_move_hint(win, m.resize));
            }
        }

        // Window rename overlay: draw the edit buffer over the target window's
        // titlebar row at high z (above the window chrome, below floating menus).
        if let Some((ren_id, ren_buf)) = &self.rename {
            if let Some(win) = self.wm.get(*ren_id) {
                layers.extend(self.render_rename_overlay(win, ren_buf));
            }
        }

        let segs = self.tray_segments_now();
        layers.push(render_menubar(self.w, &self.dock_items(), &segs, false, &self.launcher_label_now()));
        layers.extend(self.render_dock_ctx());
        layers.extend(self.render_win_menu());

        // The desktop context / rename menu floats above the windows (but below
        // the launcher / help overlays) on its own high-z layer.
        if self.cfg.desktop_enabled {
            if let Some(buf) = self.desktop.overlay_buffer(self.w, self.h) {
                layers.push(Layer { z: 850, origin: Point::new(0, 0), buf, opacity: 1.0, scissor: None });
            }
        }

        // From here on come the floating, panel-sized overlays (launcher, tray,
        // dir-picker, help, power menu). Record where they start so we can collect
        // their rects and suppress only the desktop icons they actually cover —
        // terminals draw the icon graphics over text, so an icon under a menu
        // would otherwise hide it. (Panel-sized layers give precise rects; the
        // full-screen desktop-overlay layer above is handled via overlay_rect().)
        let overlay_start = layers.len();

        // Dock group chooser popup (above dock, below the floating menus).
        if self.dock_popup.is_some() {
            let rows = self.dock_popup_rows();
            if !rows.is_empty() {
                let (popup_layers, _) = self.render_popup_for_hit_test(&rows);
                layers.extend(popup_layers);
            }
        }

        // Launcher (dropdown / Spotlight) renders above all chrome.
        layers.extend(self.launcher.render(self.w, self.h).layers);

        // An open tray popover renders above everything else.
        {
            layers.extend(self.tray_render_now().layers);
            if self.clipmenu.is_open() {
                let hist = self.clip_history();
                let paused = hist.paused();
                layers.extend(self.clipmenu.render(self.w, self.h, &hist, paused));
            }
        }

        // The working-directory picker renders on top of the whole desktop.
        if let Some(d) = &self.dirpicker {
            layers.extend(d.render(self.w, self.h));
        }

        // The power menu (dropdown + confirm dialog) renders above all chrome.
        {
            let online = self.tray_state.read().map(|st| st.remotes_online.clone()).unwrap_or_default();
            layers.extend(self.power_menu.render(self.w, self.h, &self.systems, &online));
        }

        // The confirm-close / compat-restart / launch-warning dialogs stack on
        // top of everything else, in that order (launch-warning is the topmost
        // modal of all, by z).
        layers.extend(self.win_picker.render(self.w, self.h));
        layers.extend(self.confirm_close.render(self.w, self.h));
        layers.extend(self.render_compat_dialog());
        layers.extend(self.launch_warn.render(self.w, self.h));

        // Screen rects of the floating overlays now showing (empty when none open).
        let overlay_rects: Vec<crate::geometry::Rect> = layers[overlay_start..]
            .iter()
            .map(|l| crate::geometry::Rect::new(l.origin.x, l.origin.y, l.buf.width(), l.buf.height()))
            .collect();

        // Image placements for ImageView windows (visible only when their content
        // rect is fully unobstructed; the placeholder cells show otherwise).
        let mut images = Vec::new();
        for w in self.wm.z_ordered() {
            if w.minimized {
                continue;
            }
            let place = match self.contents.get(&w.id) {
                Some(WinContent::ImageView(v)) => v.placement(w.content_rect()),
                _ => None,
            };
            if let Some((id, rect, src)) = place {
                images.push(crate::protocol::ImagePlacement {
                    id,
                    rect,
                    cols: rect.w.max(1) as u16,
                    rows: rect.h.max(1) as u16,
                    visible: self.fully_unobstructed(w),
                    src: Some(src),
                });
            }
        }

        // Thumbnail placements for visible file-manager windows (Icon view).
        for w in self.wm.z_ordered() {
            if w.minimized {
                continue;
            }
            if let Some(WinContent::FileManager(f)) = self.contents.get(&w.id) {
                let cr = w.content_rect();
                let vis = self.fully_unobstructed(w);
                images.extend(f.thumbnail_placements(cr, vis));
            }
        }

        // Image placements for desktop icons (photo thumbnails or generated
        // file-type icons) not covered by a window. (Menu/overlay overlap is
        // handled uniformly for ALL images by the suppression pass below.)
        if self.cfg.desktop_enabled {
            let occluded = |r: crate::geometry::Rect| {
                self.wm
                    .z_ordered()
                    .iter()
                    .any(|w| !w.minimized && w.rect.intersect(r).is_some())
            };
            images.extend(self.desktop.icon_placements(&self.role_icon_ids, |r| !occluded(r)));
        }

        // Image placements transmitted by hosted apps (A2 Kitty-graphics passthrough),
        // offset to the window's content rect and clipped to it. The PNGs were loaded
        // into `self.images` by `refresh_app_graphics` (called before this frame).
        for w in self.wm.z_ordered() {
            if w.minimized {
                continue;
            }
            let aid = match self.contents.get(&w.id) {
                Some(WinContent::App(aid)) => *aid,
                _ => continue,
            };
            let placements = self.apphost.placements(aid);
            if placements.is_empty() {
                continue;
            }
            let cr = w.content_rect();
            let vis = self.fully_unobstructed(w);
            for pl in &placements {
                if let Some(&img) = self.app_image_ids.get(&(w.id, pl.image_id)) {
                    let x = cr.x + pl.col as i32;
                    let y = cr.y + pl.row as i32;
                    if x >= cr.x + cr.w || y >= cr.y + cr.h {
                        continue;
                    }
                    let cols = pl.cols.min((cr.x + cr.w - x).max(1) as u16);
                    let rows = pl.rows.min((cr.y + cr.h - y).max(1) as u16);
                    images.push(crate::protocol::ImagePlacement {
                        id: img,
                        rect: crate::geometry::Rect::new(x, y, cols as i32, rows as i32),
                        cols,
                        rows,
                        visible: vis,
                        src: None,
                    });
                }
            }
        }

        // Terminals composite images OVER text, so any image sitting under a
        // floating menu/overlay would hide it. Drop every image (desktop icon,
        // FM thumbnail, image viewer, or hosted-app graphic) that intersects an
        // open overlay's rect — the launcher/power-menu/help/tray panels and the
        // desktop context menu. Only icons actually under a menu are removed, so
        // a top-left launcher leaves the top-right icons as image tiles.
        let mut menu_rects = overlay_rects;
        if let Some(dm) = self.desktop.overlay_rect() {
            menu_rects.push(dm);
        }
        if !menu_rects.is_empty() {
            images.retain(|p| menu_rects.iter().all(|o| o.intersect(p.rect).is_none()));
        }
        sanitize_images(&mut images, self.w, self.h);

        Frame { layers, cursor: Some(self.cursor), images }
    }

    /// Build the frame for simple (full-screen single-app) view: the focused
    /// window's content fills the work area with no decorations; the menubar and
    /// dock stay; the desktop and other windows are hidden.
    fn build_frame_simple(&self) -> Frame {
        self.publish_interest();
        if let Some(f) = self.saver_frame() {
            return f;
        }
        use crate::geometry::Point;
        let t = crate::theme::current();
        let wa = self.simple_content_rect();
        let mut layers: Vec<Layer> = Vec::new();
        let focused = self.wm.focused();

        // Focused window full-screen (no chrome), or a hint when nothing is open.
        if let Some(fid) = focused {
            if let Some(content) = self.contents.get(&fid) {
                let mut buf = content.render(self.apphost.as_ref(), wa.w, wa.h);
                if let WinContent::App(aid) = content {
                    if let Some((cx, cy)) = self.apphost.cursor(*aid) {
                        draw_cursor_cell(&mut buf, cx as i32, cy as i32);
                    }
                }
                layers.push(Layer { z: 1, origin: Point::new(wa.x, wa.y), buf, opacity: 1.0, scissor: None });
            }
        } else {
            let mut buf = crate::buffer::CellBuffer::new(wa.w, wa.h);
            buf.fill(crate::cell::Cell { ch: ' ', fg: t.dim, bg: t.desktop_bg, attrs: Default::default() });
            let hint = "Press Go to launch an app";
            let hx = ((wa.w - hint.chars().count() as i32) / 2).max(0);
            let hy = wa.h / 2;
            buf.write_str(hx, hy, hint, t.dim, t.desktop_bg);
            layers.push(Layer { z: 1, origin: Point::new(wa.x, wa.y), buf, opacity: 1.0, scissor: None });
        }

        // Chrome: menubar (simple glyph) with the taskbar pills.
        let segs = self.tray_segments_now();
        layers.push(render_menubar(self.w, &self.dock_items(), &segs, true, &self.launcher_label_now()));
        layers.extend(self.render_dock_ctx());
        layers.extend(self.render_win_menu());

        // Overlays that must still work in simple mode.
        let overlay_start = layers.len();
        // Taskbar group chooser popup (drops down from its pill).
        if self.dock_popup.is_some() {
            let rows = self.dock_popup_rows();
            if !rows.is_empty() {
                let (popup_layers, _) = self.render_popup_for_hit_test(&rows);
                layers.extend(popup_layers);
            }
        }
        layers.extend(self.launcher.render(self.w, self.h).layers);
        {
            layers.extend(self.tray_render_now().layers);
            if self.clipmenu.is_open() {
                let hist = self.clip_history();
                let paused = hist.paused();
                layers.extend(self.clipmenu.render(self.w, self.h, &hist, paused));
            }
        }
        {
            let online = self.tray_state.read().map(|st| st.remotes_online.clone()).unwrap_or_default();
            layers.extend(self.power_menu.render(self.w, self.h, &self.systems, &online));
        }
        layers.extend(self.win_picker.render(self.w, self.h));
        layers.extend(self.confirm_close.render(self.w, self.h));
        layers.extend(self.render_compat_dialog());

        // Images for the focused window only, mapped into the work area.
        let mut images = Vec::new();
        if let Some(fid) = focused {
            match self.contents.get(&fid) {
                Some(WinContent::App(aid)) => {
                    let aid = *aid;
                    for pl in self.apphost.placements(aid) {
                        if let Some(&img) = self.app_image_ids.get(&(fid, pl.image_id)) {
                            let x = wa.x + pl.col as i32;
                            let y = wa.y + pl.row as i32;
                            if x >= wa.x + wa.w || y >= wa.y + wa.h {
                                continue;
                            }
                            let cols = pl.cols.min((wa.x + wa.w - x).max(1) as u16);
                            let rows = pl.rows.min((wa.y + wa.h - y).max(1) as u16);
                            images.push(crate::protocol::ImagePlacement {
                                id: img,
                                rect: crate::geometry::Rect::new(x, y, cols as i32, rows as i32),
                                cols,
                                rows,
                                visible: true,
                                src: None,
                            });
                        }
                    }
                }
                Some(WinContent::ImageView(v)) => {
                    if let Some((id, rect, src)) = v.placement(wa) {
                        images.push(crate::protocol::ImagePlacement {
                            id,
                            rect,
                            cols: rect.w.max(1) as u16,
                            rows: rect.h.max(1) as u16,
                            visible: true,
                            src: Some(src),
                        });
                    }
                }
                Some(WinContent::FileManager(f)) => {
                    images.extend(f.thumbnail_placements(wa, true));
                }
                _ => {}
            }
        }

        // Drop any image under an open overlay (launcher / tray / help / power
        // menu / confirm-close dialog), since terminals composite graphics over
        // text and would otherwise bleed through the dialog.
        let overlay_rects: Vec<crate::geometry::Rect> = layers[overlay_start..]
            .iter()
            .map(|l| crate::geometry::Rect::new(l.origin.x, l.origin.y, l.buf.width(), l.buf.height()))
            .collect();
        images.retain(|p| overlay_rects.iter().all(|o| o.intersect(p.rect).is_none()));
        sanitize_images(&mut images, self.w, self.h);

        Frame { layers, cursor: Some(self.cursor), images }
    }

    // ── Lifecycle ─────────────────────────────────────────────────────────────

    /// Remove windows whose PTY child has exited.
    ///
    /// Call this once per render loop tick to keep the session consistent with
    /// process state.
    pub fn reap_dead(&mut self) {
        let app_windows: Vec<(WindowId, AppId)> = self
            .contents
            .iter()
            .filter_map(|(id, c)| match c {
                WinContent::App(aid) => Some((*id, *aid)),
                _ => None,
            })
            .collect();
        let dead: Vec<WindowId> = app_windows
            .into_iter()
            .filter(|(_, aid)| !self.apphost.is_alive(*aid))
            .map(|(id, _)| id)
            .collect();
        // If an install window finished, re-scan $PATH and rebuild the launcher so
        // the newly-installed app shows up (and the store sees it as installed)
        // without a daemon restart.
        let install_finished = dead.iter().any(|id| {
            self.titles.iter().any(|(i, t)| i == id && t.starts_with("install:"))
        });
        for id in dead {
            self.close(id);
        }
        if install_finished {
            self.refresh_installed_apps();
            // Settings > General > Apps shows the size that is now installed.
            for c in self.contents.values_mut() {
                if let WinContent::Settings(s) = c {
                    s.refresh_apps();
                }
            }
        }
    }

    /// The focused app's content rect IFF that app currently captures the
    /// pointer (wants mouse, or alt-scroll). Used by the client to route
    /// in-app mouse events. `None` → all mouse stays on the chrome/WM path.
    /// Whether the focused window is an app that has application cursor-key mode
    /// (DECCKM) enabled. The client uses this to encode arrows as `ESC O A..D`
    /// so apps that bind only the application form (e.g. midnight commander)
    /// get working arrow keys. `false` when no app is focused.
    pub fn app_cursor_keys(&self) -> bool {
        let Some(fid) = self.wm.focused() else { return false };
        let aid = match self.contents.get(&fid) {
            Some(WinContent::App(aid)) => *aid,
            _ => return false,
        };
        self.apphost.mouse_mode(aid).app_cursor
    }

    pub fn app_mouse_area(&self) -> Option<crate::geometry::Rect> {
        // Any open overlay/menu (launcher, power menu, help, dir-picker, tray
        // popover, desktop menu) owns the mouse via the normal chrome path. Never
        // route clicks into the app while one is showing — otherwise clicking an
        // app in the Go launcher (drawn over a focused mouse-app) is swallowed.
        if self.select_mode
            || self.launcher.is_open()
            || self.clipmenu.is_open()
            || self.dirpicker.is_some()
            || self.power_menu.is_open()
            || self.confirm_close.is_open()
            || self.win_picker.is_open()
            || self.launch_warn.is_open()
            || self.tray.open().is_some()
            || self.desktop.overlay_rect().is_some()
            || self.dock_popup.is_some()
        {
            return None;
        }
        let fid = self.wm.focused()?;
        let aid = match self.contents.get(&fid)? {
            WinContent::App(aid) => *aid,
            _ => return None,
        };
        if !self.apphost.mouse_mode(aid).captures_pointer() {
            return None;
        }
        if self.simple {
            Some(self.simple_content_rect())
        } else {
            let w = self.wm.get(fid)?;
            if w.minimized { return None; }
            Some(w.content_rect())
        }
    }

    /// Forward a passthrough mouse event to the focused app's PTY.
    fn forward_mouse_to_app(&mut self, m: crate::mouse::MouseInput) {
        // The pointer we draw must follow the mouse here too: on a bare VT (gpm)
        // it is the only pointer on screen, and without this it stayed behind
        // at the edge of any focused mouse-capturing app (mc, fresh, htop).
        self.cursor = Point::new(m.col, m.row);
        let Some(area) = self.app_mouse_area() else { return };
        if m.col < area.x || m.col >= area.x + area.w || m.row < area.y || m.row >= area.y + area.h {
            return;
        }
        let Some(fid) = self.wm.focused() else { return };
        let aid = match self.contents.get(&fid) { Some(WinContent::App(aid)) => *aid, _ => return };
        let mode = self.apphost.mouse_mode(aid);
        let local = crate::mouse::MouseInput { col: m.col - area.x, row: m.row - area.y, ..m };
        if let Some(bytes) = crate::mouse::encode(&local, &mode) {
            self.apphost.input(aid, &bytes);
        }
    }

    /// Kill all running apps and clear the session.
    ///
    /// Must be called before dropping the session to ensure no child processes
    /// are orphaned.
    pub fn shutdown(&mut self) {
        for aid in self.apphost.list() {
            self.apphost.kill(aid);
        }
        self.contents.clear();
        self.apphost.shutdown_host();
    }
}

/// Rewrite a CLI-flagged app's command/args into the `sh -lc` wrapper: run the
/// bare binary with `--help` so its usage is visible immediately, then drop
/// into the user's shell (tool still on `$PATH`, ready to invoke properly).
/// `requires_cwd`/`cwd` are untouched by this — the shell still starts wherever
/// the caller picked.
fn cli_wrap(command: &str, args: &[String]) -> (String, Vec<String>) {
    // Quote every word so an arg with spaces/quotes (from user config) can't
    // splice into the script — same rule as every other shell command we build.
    let mut invocation = crate::systems::sh_quote(command);
    for a in args {
        invocation.push(' ');
        invocation.push_str(&crate::systems::sh_quote(a));
    }
    let script = format!("{invocation} --help; exec \"${{SHELL:-sh}}\"");
    ("sh".into(), vec!["-lc".into(), script])
}

/// Expand a leading `~` to the user's home directory.
fn expand_tilde(s: &str) -> std::path::PathBuf {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    } else if s == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    std::path::PathBuf::from(s)
}

/// Manhattan distance (in cells) the pointer must travel from the press point
/// before a titlebar/edge drag starts moving/resizing a window. Below this, the
/// press is treated as a plain click (so it can't untile a window, and lets
/// double-click-to-rename work without nudging the window).
const DRAG_THRESHOLD: i32 = 3;

/// True when `p` is within `threshold` cells of any edge of `work` — the band in
/// which a drag engages grid-cell snapping (interior drags stay floating).
fn near_edge(p: Point, work: Rect, threshold: i32) -> bool {
    p.x - work.x < threshold
        || work.right() - p.x < threshold
        || p.y - work.y < threshold
        || work.bottom() - p.y < threshold
}

/// Drop image placements the client can't address on a `w`×`h` screen: a rect
/// with a negative origin would emit a CUP with a 0/negative parameter, which
/// terminals reject — the image would then be placed at whatever cell the
/// cursor happens to be on (an icon painted over an arbitrary spot, typically
/// seen right after a resize). Origins past the right/bottom edge are dropped
/// too; they would be entirely invisible anyway.
/// The name fresh gives the instance with process `pid` in its
/// `--cmd daemon list` output (`local-<pid>-<hash>`), if it is listed.
fn fresh_instance_name(list: &str, pid: u32) -> Option<&str> {
    let prefix = format!("local-{pid}-");
    list.split_whitespace().find(|t| t.starts_with(&prefix))
}

/// Whether `command` runs the fresh editor (bare name or a path to it).
fn is_fresh(command: &str) -> bool {
    std::path::Path::new(command).file_name().is_some_and(|n| n == "fresh")
}

fn sanitize_images(images: &mut Vec<crate::protocol::ImagePlacement>, w: i32, h: i32) {
    images.retain(|p| p.rect.x >= 0 && p.rect.y >= 0 && p.rect.x < w && p.rect.y < h);
}

/// Check the upstream repository for a newer commit than this build.
///
/// Uses `curl` against the GitHub API with a hard timeout so the call can never
/// hang the desktop. Returns a short human-readable status string.
/// One-shot UI reopen hint codes, persisted across a frontend reload.
#[allow(non_upper_case_globals)]
const Ui_SETTINGS_UPDATES: u8 = 1;
/// Reopen Settings on General (after a config reload, reset or create).
#[allow(non_upper_case_globals)]
const Ui_SETTINGS_GENERAL: u8 = 2;

/// `dock_popup` key meaning "the taskbar's …+N overflow list" rather than a
/// window group. Cannot collide with an app key (starts with a control char).
const OVERFLOW_POPUP: &str = "\u{1}overflow";

/// Path of the reopen-hint file (per-user socket dir, so it's reload-scoped).
fn reopen_hint_path() -> std::path::PathBuf {
    crate::protocol::socket_dir().join("reopen-hint")
}

/// Persist a one-shot UI reopen hint for the next frontend start.
fn write_reopen_hint(code: u8) {
    let _ = std::fs::create_dir_all(crate::protocol::socket_dir());
    let _ = std::fs::write(reopen_hint_path(), [code]);
}

/// Read and consume the reopen hint (deleted after reading).
fn take_reopen_hint() -> Option<u8> {
    let p = reopen_hint_path();
    let code = std::fs::read(&p).ok().and_then(|b| b.first().copied());
    let _ = std::fs::remove_file(&p);
    code
}

/// Accept only well-formed git branch names; anything else falls back to
/// `main`. The branch comes from `config.update_branch`, which a hand-edited
/// config (or a typo) could set to junk — and it's interpolated into both a
/// shell command and a URL below, so a stray quote/space would otherwise
/// produce a broken updater run rather than a clean fallback.
fn sanitize_branch(branch: &str) -> String {
    let ok = !branch.is_empty()
        && branch.len() <= 100
        && branch
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '/' | '.'))
        && !branch.starts_with('-')
        && !branch.contains("..");
    if ok { branch.to_string() } else { "main".to_string() }
}

/// The shell command that updates tetron-wm and reloads. On `main` it prefers the
/// prebuilt release binary (a fast download via `install.sh`, jumping straight
/// to the latest release), falling back to a source build; on any other branch
/// (the dev channel) it builds that branch from git. On success it reloads and
/// EXITS so the updater window auto-closes; only a failure drops to a shell.
///
/// `packaged` (apt owns the running binary, WMDEB-005): `install.sh` upgrades
/// through the release `.deb` and apt, with no source fallback (a cargo build
/// would add a second, shadowing copy), and the dev channel refuses. On a raw
/// install, `install.sh` exiting 3 (it found the package and refused) also
/// skips the source fallback.
fn update_command(branch: &str, packaged: bool) -> String {
    let branch = sanitize_branch(branch);
    let branch = branch.as_str();
    let repo = crate::REPO_URL;
    let raw = "https://raw.githubusercontent.com/tetronos/tetron-wm";
    // Keep the new binary where the running one lives (cargo bin, /usr/local/bin,
    // or a user dir). install.sh sudo-escalates when that dir needs root.
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .map(|d| d.display().to_string())
        .unwrap_or_else(|| "/usr/local/bin".into());
    let root_flag = std::env::current_exe().map(|p| cargo_root_flag(&p)).unwrap_or_default();
    let dir = crate::systems::sh_quote(&exe_dir);
    // Reload via the freshly-installed binary's ABSOLUTE path, not a bare
    // `tetron-wm`. The updater runs in a non-interactive `sh -lc`, whose PATH may
    // not include the install dir (~/.local/bin is added by interactive zsh
    // config, not a login `sh`). If `tetron-wm reload` isn't found, the install
    // still succeeds but the daemon never restarts onto the new binary — so
    // the running version never changes and the update appears to "keep
    // failing". install.sh lands the binary in `exe_dir` (TETRON_WM_BIN_DIR), and
    // the cargo `--root` fallback targets the same dir, so `{exe_dir}/tetron-wm` is
    // the new binary in both paths.
    let reload = crate::systems::sh_quote(&format!("{exe_dir}/tetron-wm"));
    // Append each step to the log (same file as dbg_log) so a failed
    // update is visible in the log the user pastes — the install runs in a
    // window whose output is otherwise lost.
    let log = crate::logfile::shell_path();
    // uv and the uv tools first (WMINST-009); their result never stops the update.
    let tools = crate::packaging::tool_updates_snippet();
    if packaged && branch != "main" {
        return format!(
            "clear; echo 'tetron-wm was installed by apt. The {branch} channel builds from source, which would add'; \
echo 'a second copy beside the packaged one, so it is not used here. Switch Channel to main.'; \
echo \"update: refused, {branch} channel on an apt-installed tetron-wm\" >> {log}; exec \"$SHELL\""
        );
    }
    if packaged {
        return format!(
            "clear; {tools}echo 'Updating tetron-wm with apt (latest release .deb)…'; echo; \
echo \"update: apt upgrade via install.sh (packaged)\" >> {log}; \
if sh -c 'curl -fsSL {raw}/main/install.sh | sh'; then \
echo \"update: install.sh ok; reloading via {reload}\" >> {log}; echo; echo 'Reloading…'; {reload} reload; exit 0; \
else echo \"update: FAILED (install.sh, packaged)\" >> {log}; echo 'Update failed — tetron-wm not reloaded.'; exec \"$SHELL\"; fi"
        );
    }
    if branch == "main" {
        format!(
            "clear; {tools}echo 'Updating tetron-wm (latest release)…'; echo; \
echo \"update: install -> {dir}\" >> {log}; \
if TETRON_WM_BIN_DIR={dir} sh -c 'curl -fsSL {raw}/main/install.sh | sh'; then \
echo \"update: install.sh ok; reloading via {reload}\" >> {log}; echo; echo 'Reloading…'; {reload} reload; exit 0; \
else rc=$?; if [ \"$rc\" = 3 ]; then \
echo \"update: install.sh found the apt package and changed nothing; no source fallback\" >> {log}; echo 'Update stopped — tetron-wm not reloaded.'; exec \"$SHELL\"; \
elif cargo install --git {repo}{root_flag} --force; then \
echo \"update: cargo fallback ok; reloading via {reload}\" >> {log}; echo; echo 'Reloading…'; {reload} reload; exit 0; \
else echo \"update: FAILED (install.sh and cargo both failed)\" >> {log}; echo 'Update failed — tetron-wm not reloaded.'; exec \"$SHELL\"; fi; fi",
        )
    } else {
        format!(
            "clear; {tools}echo 'Updating tetron-wm (branch {branch})…'; echo; \
echo \"update: cargo install (branch {branch})\" >> {log}; \
if cargo install --git {repo} --branch {branch}{root_flag} --force; then \
echo \"update: cargo ok; reloading via {reload}\" >> {log}; echo; echo 'Reloading…'; {reload} reload; exit 0; \
else echo \"update: FAILED (cargo branch {branch})\" >> {log}; echo 'Update failed — tetron-wm not reloaded.'; exec \"$SHELL\"; fi",
        )
    }
}

/// Settings > Updates on an own build (the package's binary diverted,
/// WMINST-013): installs nothing, since apt would only update the diverted
/// file and a release binary would replace the user's build. Shows how to
/// rebuild from the clone or go back to the apt package, then a shell.
fn own_build_update_command() -> String {
    let log = crate::logfile::shell_path();
    let raw = "https://raw.githubusercontent.com/tetronos/tetron-wm/main/install.sh";
    format!(
        "clear; echo 'tetron-wm here is your own build: the tetron-wm package stays installed and its'; \
echo 'binary is set aside (dpkg-divert), so Update does not replace your build.'; echo; \
echo 'Rebuild from your clone:'; echo '  cd ~/code/tetron-wm && git pull && TETRON_WM_FROM_CLONE=$PWD sh install.sh && tetron-wm reload'; echo; \
echo 'Back to the apt package (apt upgrade updates it again):'; echo '  curl -fsSL {raw} | TETRON_WM_METHOD=deb sh && /usr/bin/tetron-wm reload'; echo; \
echo \"update: not run, own build (diverted package)\" >> {log}; exec \"$SHELL\""
    )
}

/// The ` --root <dir>` flag that steers `cargo install` to the running
/// binary's own `bin/` dir (its parent's parent — cargo appends `/bin` to the
/// root), or an empty string when the binary isn't in a `bin/` dir (leaving
/// cargo's default `~/.cargo/bin`). Co-locating the source build with the
/// binary the user actually runs avoids landing a shadowed copy in a different
/// dir, which would make the in-app update silently appear to do nothing.
fn cargo_root_flag(exe: &std::path::Path) -> String {
    match exe
        .parent()
        .filter(|d| d.file_name() == Some(std::ffi::OsStr::new("bin")))
        .and_then(|d| d.parent())
    {
        Some(root) => format!(" --root {}", crate::systems::sh_quote(&root.display().to_string())),
        None => String::new(),
    }
}

/// Whether a too-old apphost with live apps warrants the safety dialog: an
/// incompatible protocol is only worth interrupting the user for when there
/// are actually apps that would die in a restart.
fn needs_apphost_restart(host_proto: u32, min_compat: u32, app_count: usize) -> bool {
    host_proto < min_compat && app_count > 0
}

/// Minimum spacing between desktop-folder `stat` calls in
/// [`SessionCore::poll_desktop_dir`] — cheap, but no reason to hit the
/// filesystem every tick.
const DESKTOP_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// Whether enough time has passed since the last desktop-folder poll to do
/// another one. Pure so the throttle is testable without real sleeps.
fn desktop_poll_due(last_poll: std::time::Instant, now: std::time::Instant) -> bool {
    now.duration_since(last_poll) >= DESKTOP_POLL_INTERVAL
}

/// Whether the desktop folder's modified-time changed since the last observed
/// poll (so it should be rescanned). The very first poll (`last == None`)
/// never counts as a change — it just establishes the baseline, since the
/// desktop was already scanned at startup / by its own actions.
fn desktop_mtime_changed(last: Option<std::time::SystemTime>, current: Option<std::time::SystemTime>) -> bool {
    matches!((last, current), (Some(a), Some(b)) if a != b)
}

/// Rows of the dock right-click context menu (row 2, "Close", renders in the
/// close colour and routes through the confirm-close dialog for apps).
const DOCK_CTX_ROWS: [&str; 4] = ["Minimise", "Maximise", "Close", "Reset size"];

/// The taskbar pill context-menu box, dropping down from the menubar row,
/// anchored to the clicked pill's x (shared by render + hit-testing so they can
/// never drift).
#[doc(hidden)]
pub fn dock_ctx_rect(anchor_x: i32, w: i32, _h: i32) -> Rect {
    let box_w = 14;
    let box_h = DOCK_CTX_ROWS.len() as i32 + 2; // border rows
    let x = anchor_x.clamp(0, (w - box_w).max(0));
    // Drops down just below the menubar/taskbar row (row 0).
    Rect::new(x, 1, box_w, box_h)
}

/// Screen rect of dock context-menu row `i` (the clickable row).
#[doc(hidden)]
pub fn dock_ctx_row_rect(anchor_x: i32, w: i32, h: i32, i: usize) -> Rect {
    let d = dock_ctx_rect(anchor_x, w, h);
    Rect::new(d.x + 1, d.y + 1 + i as i32, d.w - 2, 1)
}

/// The post-update compat dialog box (shared by render + hit-testing).
fn compat_dialog_rect(w: i32, h: i32) -> Rect {
    let box_w = 58.min((w - 2).max(24));
    let box_h = 9.min((h - 1).max(7));
    Rect::new((w - box_w) / 2, ((h - box_h) / 2).max(0), box_w, box_h)
}

/// `(keep-apps, restart-app-server)` button rects of the compat dialog.
fn compat_dialog_buttons(w: i32, h: i32) -> (Rect, Rect) {
    let d = compat_dialog_rect(w, h);
    let by = d.y + d.h - 2;
    let keep = Rect::new(d.x + 2, by, 16, 1);
    let restart_w = 24;
    let restart = Rect::new(d.x + d.w - 2 - restart_w, by, restart_w, 1);
    (keep, restart)
}

/// Latest release tag via the web redirect, dodging the rate-limited REST API.
/// `github.com/OWNER/REPO/releases/latest` 302s to `.../releases/tag/vX.Y.Z`;
/// we read the tag straight out of the redirect's `Location` header. install.sh
/// resolves it the same way, for the same reason: the api.github.com endpoint is
/// rate-limited to 60 req/hour/IP and its 403 was surfacing as a false
/// "Couldn't check (offline?)" (and wedging the updater's install path).
fn latest_release_tag(repo: &str) -> Option<String> {
    let url = format!("https://github.com/{repo}/releases/latest");
    let out = std::process::Command::new("curl")
        .args(["-fsSI", "--max-time", "6", &url])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let headers = String::from_utf8_lossy(&out.stdout);
    let location = headers
        .lines()
        .rfind(|l| l.to_ascii_lowercase().starts_with("location:"))
        .and_then(|l| l.split_once(':').map(|(_, v)| v.trim()))?;
    if !location.contains("/releases/tag/") {
        return None;
    }
    let tag = location.rsplit('/').next()?.trim();
    (!tag.is_empty()).then(|| tag.to_string())
}

/// Check whether a newer build is available, comparing against whatever the
/// update channel would actually install, and reporting it in that channel's
/// own terms:
/// - **main** installs the latest prebuilt *release* (via `install.sh`), so we
///   compare the installed semver against the latest release tag and report
///   versions (`v0.2.0 → v0.2.1`). Comparing against the `main` branch tip is a
///   bug: any commit landed on `main` after the last release shows a permanent
///   "update available" that re-installing the same release can never clear,
///   i.e. an update loop.
/// - **dev** builds the branch tip from source — there is no version there, so
///   we compare the installed commit against the branch tip and report short
///   commit hashes.
fn check_for_updates(branch: &str) -> String {
    let branch = sanitize_branch(branch);
    let branch = branch.as_str();
    let repo = crate::REPO_URL.trim_start_matches("https://github.com/");
    let get_json = |path: &str| -> Option<serde_json::Value> {
        let url = format!("https://api.github.com/repos/{repo}/{path}");
        std::process::Command::new("curl")
            .args(["-fsS", "--max-time", "6", "-H", "User-Agent: tetron-wm", &url])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok())
    };
    let str_field = |v: &serde_json::Value, k: &str| {
        v.get(k).and_then(|s| s.as_str()).map(str::to_string)
    };

    if branch == "main" {
        // Release channel: compare versions, show versions. Resolve the tag via
        // the web redirect first (not rate-limited), falling back to the REST API
        // only when the redirect can't be parsed.
        let tag = match latest_release_tag(repo)
            .or_else(|| get_json("releases/latest").and_then(|v| str_field(&v, "tag_name")))
        {
            Some(t) => t,
            None => return "Couldn't check (offline?)".to_string(),
        };
        let latest = tag.trim_start_matches('v');
        let cur = crate::VERSION; // CARGO_PKG_VERSION, e.g. "0.2.1"
        // Only offer the update when the release is *strictly newer*: a plain
        // string compare would prompt a downgrade when the local build is ahead
        // of the latest tag — e.g. mid-release-cut, or a dev/source build whose
        // version outruns the published release. A release tag we can't parse is
        // reported as a check failure rather than silently treated as 0.0.0.
        match semver_tuple(latest) {
            Some(l) if Some(l) > semver_tuple(cur) => {
                format!("Update available: v{cur} → v{latest}")
            }
            Some(_) => format!("Up to date (v{cur})"),
            None => format!("Couldn't check (unexpected release tag '{tag}')"),
        }
    } else {
        // Dev channel: compare commits, show short hashes.
        let short = |s: &str| s.chars().take(7).collect::<String>();
        let latest = get_json(&format!("commits/{branch}")).and_then(|v| str_field(&v, "sha"));
        match latest {
            Some(sha) => {
                let cur = crate::GIT_SHA;
                if cur == "unknown" {
                    format!("Latest is {} on {branch} — reinstall to update", short(&sha))
                } else if sha.starts_with(cur) || cur.starts_with(&short(&sha)) {
                    format!("Up to date ({}) on {branch}", short(cur))
                } else {
                    format!("Update available on {branch}: {} → {}", short(cur), short(&sha))
                }
            }
            None => "Couldn't check (offline?)".to_string(),
        }
    }
}

/// Parse a `MAJOR.MINOR.PATCH` version (a leading `v` is tolerated) into a
/// comparable tuple, or `None` if it isn't exactly that. This is a numeric
/// `major.minor.patch` comparison, *not* full semver: any `-pre`/`+build`
/// suffix is dropped before parsing (tetron-wm only ever ships plain release
/// versions, so pre-release precedence never comes up). Anything that isn't
/// three numeric components — too few, too many, or non-numeric — is `None`, so
/// a malformed release tag surfaces as a check failure instead of silently
/// comparing as `0.0.0`.
fn semver_tuple(v: &str) -> Option<(u64, u64, u64)> {
    let core = v.trim().trim_start_matches('v').split(['-', '+']).next().unwrap_or("");
    let mut it = core.split('.');
    let major = it.next()?.trim().parse::<u64>().ok()?;
    let minor = it.next()?.trim().parse::<u64>().ok()?;
    let patch = it.next()?.trim().parse::<u64>().ok()?;
    if it.next().is_some() {
        return None; // a fourth component → not a MAJOR.MINOR.PATCH version
    }
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A config whose power chains only run harmless commands. Tests must NEVER
    /// let the real `systemctl poweroff` chain run.
    fn safe_power_cfg(reboot: &str, poweroff: &str) -> crate::config::Config {
        use crate::traytools::ToolSpec;
        let mut cfg = crate::config::Config::default();
        cfg.power_commands.insert("suspend".into(), ToolSpec::Many(Vec::new()));
        cfg.power_commands.insert("reboot".into(), ToolSpec::One(reboot.into()));
        cfg.power_commands.insert("poweroff".into(), ToolSpec::One(poweroff.into()));
        cfg
    }

    fn entry_for(cmd: &str) -> crate::config::AppEntry {
        crate::config::AppEntry {
            name: cmd.into(), command: cmd.into(), args: Vec::new(), category: None,
            requires_cwd: None, cwd: None, cli: None, warn: None,
        }
    }

    /// An hour of ordinary use (a refresh a second: menus, focus, resizes,
    /// desktop and wallpaper reloads) logs a handful of lines at `info`, not
    /// one per message. The level is the default `info` for every test thread.
    #[test]
    fn an_hour_of_use_logs_events_not_repeats() {
        let mut core = SessionCore::new(120, 30, Config { apps: vec![], ..Config::default() });
        let before = crate::logfile::lines_passed();
        for i in 0..3600 {
            core.apply(ClientMsg::ToggleSpotlight);
            core.apply(ClientMsg::ToggleSpotlight);
            core.apply(ClientMsg::FocusNext);
            core.apply(ClientMsg::InfoOpen);
            core.apply(ClientMsg::InfoOpen);
            if i % 60 == 0 {
                core.apply(ClientMsg::Resize { w: 120, h: 30, takeover: false });
            }
            core.reload_desktop();
        }
        let lines = crate::logfile::lines_passed() - before;
        assert!(lines < 40, "{lines} lines logged at info for an hour of use");
    }

    #[test]
    fn leader_i_opens_info_and_enter_follows_the_selected_row() {
        let mut core = SessionCore::new(120, 30, Config { apps: vec![], desktop_enabled: false, ..Config::default() });
        core.apply(ClientMsg::InfoOpen);
        assert!(core.info_popover_open());
        let rows = core.tray_render_now().hits;
        let logs = rows.iter().position(|h| h.intent == crate::system::ControlIntent::OpenLogs).expect("Logs row");
        for _ in 0..logs {
            core.apply(ClientMsg::TrayKey(crate::tray::TrayKey::Down));
        }
        assert_eq!(core.tray.info_row(), logs);
        core.apply(ClientMsg::TrayKey(crate::tray::TrayKey::Enter));
        assert!(core.focused_is_logs(), "Enter opened the Logs window");
        assert!(!core.info_popover_open());
        // Leader i toggles; Esc closes.
        core.apply(ClientMsg::InfoOpen);
        core.apply(ClientMsg::TrayKey(crate::tray::TrayKey::Esc));
        assert!(!core.info_popover_open());
        core.shutdown();
    }

    #[test]
    fn title_bar_right_click_menu_acts_on_that_window() {
        let mut core = SessionCore::new(100, 30, Config { apps: vec![], desktop_enabled: false, ..Config::default() });
        core.apply(ClientMsg::OpenFileManager);
        let id = core.wm.focused().expect("files window");
        let r = core.wm.get(id).unwrap().rect;
        core.apply(ClientMsg::MouseRightDown(Point::new(r.x + 3, r.y)));
        assert!(core.desktop_menu_open(), "menu open, so the client routes arrows to it");
        assert!(core.win_menu.as_ref().is_some_and(|(w, m)| *w == id && m.items == crate::contextmenu::window_actions()));
        assert!(!core.build_frame().layers.iter().all(|l| l.z != 5300), "the menu is drawn");
        // Esc closes; reopen and Enter on the first item (Minimize).
        core.apply(ClientMsg::DesktopMenuKey(MenuKey::Esc));
        assert!(core.win_menu.is_none());
        core.apply(ClientMsg::MouseRightDown(Point::new(r.x + 3, r.y)));
        core.apply(ClientMsg::DesktopMenuKey(MenuKey::Down));
        core.apply(ClientMsg::DesktopMenuKey(MenuKey::Up));
        core.apply(ClientMsg::DesktopMenuKey(MenuKey::Enter));
        assert!(core.win_menu.is_none());
        assert!(core.wm.get(id).unwrap().minimized, "Minimize applied to that window");
        // A right-click in the content does not open the window menu.
        core.wm.unminimize(id);
        core.apply(ClientMsg::MouseRightDown(Point::new(r.x + 3, r.y + 5)));
        assert!(core.win_menu.is_none());
        core.shutdown();
    }

    #[test]
    fn fresh_instance_name_matches_the_window_pid_only() {
        let list = "Cleaned up 1 stale daemon(s).\nRunning daemons:\n  local-1646983-a62c05d247\n  local-16469-0000\n  /tmp/d  (name: tmp_d)\n";
        assert_eq!(fresh_instance_name(list, 1646983), Some("local-1646983-a62c05d247"));
        assert_eq!(fresh_instance_name(list, 16469), Some("local-16469-0000"), "not a prefix of a longer pid");
        assert_eq!(fresh_instance_name(list, 42), None);
        assert_eq!(fresh_instance_name("Unknown command: daemon list", 42), None);
        assert!(is_fresh("fresh") && is_fresh("/usr/bin/fresh") && !is_fresh("freshclam"));
    }

    #[test]
    fn power_menu_false_removes_every_power_entry() {
        let mut cfg = safe_power_cfg("true", "true");
        cfg.power_menu = false;
        let apps = SessionCore::build_launcher_apps(&cfg, &[]);
        assert!(!apps.iter().any(|a| a.category.as_deref() == Some(crate::launcher::POWER_CATEGORY)));
    }

    #[test]
    fn power_launcher_entries_exist_only_for_available_actions() {
        let cfg = safe_power_cfg("true", "true");
        let apps = SessionCore::build_launcher_apps(&cfg, &[]);
        let power: Vec<&str> = apps
            .iter()
            .filter(|a| a.category.as_deref() == Some(crate::launcher::POWER_CATEGORY))
            .map(|a| a.name.as_str())
            .collect();
        // Suspend is disabled by an empty list; Reboot and Power off exist, in order.
        assert_eq!(power, vec!["Reboot", "Power off"]);
        let cmds: Vec<&str> = apps.iter().filter(|a| a.command.starts_with("@po") || a.command == "@reboot").map(|a| a.command.as_str()).collect();
        assert_eq!(cmds, vec!["@reboot", "@poweroff"]);
    }

    #[test]
    fn confirmed_power_action_that_fails_opens_a_window_saying_why() {
        let mut core = SessionCore::new(100, 30, safe_power_cfg("sh -c 'echo denied by policy >&2; exit 1'", "true"));
        core.launch_entry(entry_for("@reboot"));
        assert!(core.power_menu.dialog_open(), "Reboot asks first");
        assert_eq!(core.window_count(), 0);
        core.apply(ClientMsg::PowerKeyConfirm);
        assert!(core.power_job.is_some(), "the chain runs on a worker thread");
        // The worker finishes within a moment; the failure then shows up as a window.
        let start = std::time::Instant::now();
        while core.window_count() == 0 && start.elapsed() < std::time::Duration::from_secs(5) {
            core.poll_power();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(core.window_count(), 1, "a failed Reboot is reported, not silent");
        assert_eq!(core.focused_label_for_test(), "Reboot");
        assert!(core.power_job.is_none());
        core.shutdown();
    }

    #[test]
    fn confirmed_power_action_that_succeeds_opens_nothing() {
        let mut core = SessionCore::new(100, 30, safe_power_cfg("true", "true"));
        core.launch_entry(entry_for("@poweroff"));
        core.apply(ClientMsg::PowerKeyConfirm);
        let start = std::time::Instant::now();
        while core.power_job.is_some() && start.elapsed() < std::time::Duration::from_secs(5) {
            core.poll_power();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(core.power_job.is_none(), "the job finished");
        assert_eq!(core.window_count(), 0, "an accepted power-off needs no window");
        core.shutdown();
    }

    #[test]
    fn cancelling_a_power_dialog_runs_nothing() {
        let mut core = SessionCore::new(100, 30, safe_power_cfg("true", "true"));
        core.launch_entry(entry_for("@poweroff"));
        core.apply(ClientMsg::PowerKeyCancel);
        assert!(!core.power_menu.is_open() && core.power_job.is_none());
        core.shutdown();
    }

    #[test]
    fn launcher_system_actions_open_power_flows() {
        let entry = |cmd: &str| crate::config::AppEntry {
            name: cmd.into(),
            command: cmd.into(),
            args: Vec::new(),
            category: None,
            requires_cwd: None,
            cwd: None,
            cli: None,
            warn: None,
        };
        let mut core = SessionCore::new(80, 24, crate::config::Config::default());
        assert!(!core.power_menu.is_open());
        core.launch_entry(entry("@exit"));
        assert!(core.power_menu.is_open(), "@exit should open the power confirm dialog");
        core.power_menu.close();
        core.launch_entry(entry("@systems"));
        assert!(core.power_menu.is_open(), "@systems should open the systems popover");
        core.launch_entry(entry("@help"));
        assert!(core.help_open(), "@help should open the shortcuts window");
    }

    #[test]
    fn help_window_scrolls_clamps_and_toggles() {
        let mut core = SessionCore::new(80, 12, crate::config::Config::default());
        core.apply(ClientMsg::ToggleHelp);
        assert!(core.help_open() && core.focused_is_help());
        // A short screen needs scrolling; the view learns its height on render.
        core.build_frame();
        core.apply(ClientMsg::HelpScroll(i32::MAX));
        let end = core.focused_help().unwrap().scroll();
        assert!(end > 0, "short window must scroll");
        core.apply(ClientMsg::HelpScroll(1));
        assert_eq!(core.focused_help().unwrap().scroll(), end, "clamped at the end");
        core.apply(ClientMsg::HelpScroll(i32::MIN));
        assert_eq!(core.focused_help().unwrap().scroll(), 0);
        // Toggling again from inside closes it.
        core.apply(ClientMsg::ToggleHelp);
        assert!(!core.help_open());
    }

    #[test]
    fn help_is_last_in_tetron_wm_launcher_section() {
        let apps = SessionCore::build_launcher_apps(&crate::config::Config::default(), &[]);
        let mut l = crate::launcher::Launcher::new(apps);
        let f = l.filtered();
        let tw: Vec<&str> = f.iter().filter(|a| a.category.as_deref() == Some("tetron-wm")).map(|a| a.command.as_str()).collect();
        assert_eq!(tw.last(), Some(&"@help"));
        l.toggle_menu();
    }

    #[test]
    fn wheel_over_volume_segment_yields_volume_intent() {
        use crate::system::ControlIntent;
        let core = SessionCore::new(120, 30, crate::config::Config::default());
        let seg = core
            .tray_segments_now()
            .into_iter()
            .find(|s| s.kind == crate::tray::SegmentKind::Volume)
            .expect("a volume segment is always present");
        let on = Point::new(seg.rect.x, 0);
        assert_eq!(core.volume_scroll_intent(on, 3), Some(ControlIntent::VolumeUp));
        assert_eq!(core.volume_scroll_intent(on, -3), Some(ControlIntent::VolumeDown));
        // Off the segment (wrong row, or a different column) is not consumed.
        assert_eq!(core.volume_scroll_intent(Point::new(seg.rect.x, 1), 3), None);
        assert_eq!(core.volume_scroll_intent(Point::new(0, 0), 3), None);
    }

    #[test]
    fn wheel_over_brightness_segment_steps_brightness() {
        use crate::system::ControlIntent;
        let core = SessionCore::new(120, 30, crate::config::Config::default());
        core.tray_state.write().unwrap().brightness = Some(50);
        let seg = core
            .tray_segments_now()
            .into_iter()
            .find(|s| s.kind == crate::tray::SegmentKind::Brightness)
            .expect("a brightness segment with a backlight");
        let on = Point::new(seg.rect.x, 0);
        assert_eq!(core.volume_scroll_intent(on, 1), Some(ControlIntent::BrightnessUp));
        assert_eq!(core.volume_scroll_intent(on, -1), Some(ControlIntent::BrightnessDown));
        assert_eq!(core.volume_scroll_intent(Point::new(seg.rect.x, 1), 1), None);
    }

    #[test]
    fn compat_dialog_arms_only_for_old_hosts_with_apps() {
        // Dormant today: MIN_COMPAT is 0, nothing is older than that.
        assert!(!needs_apphost_restart(0, crate::apphost::proto::MIN_COMPAT, 5));
        // When a future release bumps MIN_COMPAT, old hosts with live apps arm
        // the dialog — but an empty apphost just restarts silently.
        assert!(needs_apphost_restart(0, 1, 3));
        assert!(!needs_apphost_restart(0, 1, 0), "no apps → nothing to protect");
        assert!(!needs_apphost_restart(1, 1, 3), "compatible host → no dialog");
        // The current binary pair is always compatible with itself (a const
        // block so a bad MIN_COMPAT bump fails the BUILD, not just this test).
        const {
            // MIN_COMPAT is 0 today, so this reads as "always true" to clippy —
            // but it's a deliberate build-time guard for future bumps.
            #[allow(clippy::absurd_extreme_comparisons)]
            {
                assert!(crate::apphost::proto::PROTO_VERSION >= crate::apphost::proto::MIN_COMPAT);
            }
        }
    }

    #[test]
    fn desktop_poll_is_throttled() {
        let t0 = std::time::Instant::now();
        assert!(!desktop_poll_due(t0, t0), "no time has passed yet");
        assert!(!desktop_poll_due(t0, t0 + std::time::Duration::from_secs(1)), "under the interval");
        assert!(desktop_poll_due(t0, t0 + DESKTOP_POLL_INTERVAL), "interval elapsed");
        assert!(desktop_poll_due(t0, t0 + std::time::Duration::from_secs(60)), "well past the interval");
    }

    #[test]
    fn desktop_mtime_change_detection() {
        let a = std::time::SystemTime::UNIX_EPOCH;
        let b = a + std::time::Duration::from_secs(1);
        assert!(!desktop_mtime_changed(None, None), "nothing observed yet");
        assert!(!desktop_mtime_changed(None, Some(a)), "first poll only sets the baseline");
        assert!(!desktop_mtime_changed(Some(a), Some(a)), "unchanged mtime");
        assert!(desktop_mtime_changed(Some(a), Some(b)), "mtime moved forward");
        assert!(!desktop_mtime_changed(Some(a), None), "folder vanished: no reload, just note it");
    }

    #[test]
    fn main_update_prefers_prebuilt_release() {
        let cmd = update_command("main", false);
        assert!(cmd.contains("install.sh"), "fast path is the prebuilt release: {cmd}");
        assert!(cmd.contains("cargo install --git"), "with a source fallback");
        assert!(cmd.contains("/tetron-wm' reload"), "reloads via the installed binary's absolute path: {cmd}");
        assert!(!cmd.contains("; tetron-wm reload"), "must not rely on PATH-resolving a bare `tetron-wm`: {cmd}");
        assert!(cmd.contains(&format!(">> {}", crate::logfile::shell_path())), "logs each step to the debug log");
        assert!(cmd.contains("exit 0"), "exits so the updater window auto-closes");
        assert!(!cmd.contains("--branch"), "main needs no branch flag");
    }

    #[test]
    fn packaged_update_goes_through_apt_and_never_builds_from_source() {
        let cmd = update_command("main", true);
        assert!(cmd.contains("install.sh"), "the release .deb through install.sh: {cmd}");
        assert!(!cmd.contains("cargo"), "no source build beside the package: {cmd}");
        assert!(!cmd.contains("TETRON_WM_BIN_DIR"), "apt decides where the binary goes: {cmd}");
        assert!(cmd.contains("/tetron-wm' reload"), "reloads the installed binary: {cmd}");
        let own = own_build_update_command();
        assert!(!own.contains("apt-get") && !own.contains("| sh;") && own.contains("TETRON_WM_FROM_CLONE") && own.contains("TETRON_WM_METHOD=deb"), "{own}");
        let dev = update_command("dev", true);
        assert!(!dev.contains("cargo") && !dev.contains("install.sh"), "dev refuses on a packaged install: {dev}");
        assert!(dev.contains("installed by apt"), "and says why: {dev}");
    }

    #[test]
    fn updates_also_update_uv_and_its_tools_but_a_refusal_does_not() {
        for (branch, packaged) in [("main", false), ("main", true), ("dev", false)] {
            let cmd = update_command(branch, packaged);
            assert!(cmd.contains("uv tool upgrade --all"), "({branch}, {packaged}): {cmd}");
            assert!(cmd.find("uv tool upgrade").unwrap() < cmd.find("Updating tetron-wm").unwrap(), "before tetron-wm");
        }
        assert!(!update_command("dev", true).contains("uv tool"), "the refused dev channel installs nothing");
    }

    #[test]
    fn every_update_command_is_valid_sh() {
        for (branch, packaged) in [("main", false), ("main", true), ("dev", false), ("dev", true)] {
            let cmd = update_command(branch, packaged);
            let ok = std::process::Command::new("sh").args(["-n", "-c", &cmd]).status().unwrap().success();
            assert!(ok, "sh -n rejects ({branch}, packaged {packaged}): {cmd}");
        }
    }

    #[test]
    fn raw_update_skips_the_source_fallback_when_install_sh_refuses() {
        let cmd = update_command("main", false);
        let refuse = cmd.find("= 3").expect("checks install.sh's refusal status");
        let cargo = cmd.find("cargo install").expect("source fallback still there");
        assert!(refuse < cargo, "the refusal check comes before the fallback: {cmd}");
    }

    #[test]
    fn cargo_root_targets_the_running_bin_dir() {
        use std::path::Path;
        // A binary in a `bin/` dir → cargo is steered to the parent (it appends
        // `/bin`), so the source build lands next to the running binary.
        assert_eq!(cargo_root_flag(Path::new("/home/jay/.local/bin/tetron-wm")), " --root '/home/jay/.local'");
        assert_eq!(cargo_root_flag(Path::new("/home/jay/.cargo/bin/tetron-wm")), " --root '/home/jay/.cargo'");
        // Not in a `bin/` dir → no `--root` (leave cargo's default).
        assert_eq!(cargo_root_flag(Path::new("/opt/tetron-wm/tetron-wm")), "");
        assert_eq!(cargo_root_flag(Path::new("tetron-wm")), "");
    }

    #[test]
    fn dev_channel_builds_from_branch() {
        let cmd = update_command("dev", false);
        assert!(cmd.contains("cargo install --git"), "dev builds from source");
        assert!(cmd.contains("--branch dev"), "on the dev branch: {cmd}");
        assert!(!cmd.contains("install.sh"), "no prebuilt release off main");
    }

    #[test]
    fn junk_branch_falls_back_to_main() {
        // A hand-edited / typo'd config branch must never leak shell syntax or a
        // broken ref into the updater command or the check URL.
        for junk in ["x; rm -rf ~", "a b", "evil'", "--force", "..", ""] {
            assert_eq!(sanitize_branch(junk), "main", "rejected: {junk:?}");
            let cmd = update_command(junk, false);
            assert!(cmd.contains("install.sh"), "junk → the safe main path: {cmd}");
            assert!(!cmd.contains("rm -rf ~"), "no injected payload survives: {cmd}");
        }
        // Legitimate branch names are preserved.
        for ok in ["dev", "main", "feature/x", "release-1.2", "v0.2.0"] {
            assert_eq!(sanitize_branch(ok), ok);
        }
    }

    #[test]
    fn semver_tuple_orders_versions_and_never_downgrades() {
        // Newer release than installed → an update is offered.
        assert!(semver_tuple("0.2.2") > semver_tuple("0.2.1"));
        assert!(semver_tuple("0.3.0") > semver_tuple("0.2.9"));
        assert!(semver_tuple("1.0.0") > semver_tuple("0.9.9"));
        // Installed >= latest release → no downgrade prompt.
        assert!(semver_tuple("0.2.0") <= semver_tuple("0.2.1"), "local ahead of release");
        assert!(semver_tuple("0.2.1") <= semver_tuple("0.2.1"), "equal = up to date");
        // Leading v and (ignored) pre-release/build suffixes parse to the core.
        assert_eq!(semver_tuple("v0.2.1"), Some((0, 2, 1)));
        assert_eq!(semver_tuple("0.2.1-rc1"), Some((0, 2, 1)));
        // Anything that isn't exactly MAJOR.MINOR.PATCH is None (→ "couldn't
        // check"), not a silent 0.0.0 / truncation.
        assert_eq!(semver_tuple("0.2"), None, "too few components");
        assert_eq!(semver_tuple("0.2.1.4"), None, "too many components");
        assert_eq!(semver_tuple("not-a-version"), None);
        assert_eq!(semver_tuple(""), None);
        assert_eq!(semver_tuple("v"), None);
    }

    fn placement(x: i32, y: i32) -> crate::protocol::ImagePlacement {
        crate::protocol::ImagePlacement {
            id: 1,
            rect: Rect::new(x, y, 4, 3),
            cols: 4,
            rows: 3,
            visible: true,
            src: None,
        }
    }

    #[test]
    fn sanitize_drops_unaddressable_and_offscreen_placements() {
        let mut images = vec![
            placement(0, 0),     // kept: top-left corner
            placement(-2, 5),    // dropped: negative x (CUP param would be ≤ 0)
            placement(5, -1),    // dropped: negative y
            placement(100, 5),   // dropped: origin past the right edge
            placement(5, 30),    // dropped: origin past the bottom edge
            placement(99, 29),   // kept: last addressable cell
        ];
        sanitize_images(&mut images, 100, 30);
        let origins: Vec<(i32, i32)> = images.iter().map(|p| (p.rect.x, p.rect.y)).collect();
        assert_eq!(origins, vec![(0, 0), (99, 29)]);
    }
}
