//! Wire protocol between the tetron-wm daemon and a thin client over a local Unix
//! socket. Messages are newline-delimited JSON.
//!
//! - **Client → daemon:** [`crate::session::ClientMsg`] (input, resize, shutdown).
//! - **Daemon → client:** [`FrameMsg`] — a frame diff plus the UI-state flags the
//!   client needs to route keyboard input (which overlay is open, etc.).

use crate::compositor::CellChange;
use crate::geometry::{Point, Rect};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Daemon → client frame: changed cells, cursor position, routing flags, and the
/// image layer (placements + any first-time image data).
#[derive(Serialize, Deserialize)]
pub struct FrameMsg {
    /// Changed cells, one JSON object each. Empty when the client asked for
    /// `spans` (`ClientCaps::packed_cells`); older clients only read this.
    #[serde(default)]
    pub changes: Vec<CellChange>,
    /// The same changes packed as same-style runs (see [`Span`]), sent only to
    /// a client that advertised `packed_cells`. Applied after `changes`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spans: Vec<Span>,
    pub cursor: Option<Point>,
    pub flags: Flags,
    /// Image placements for this frame (where each visible image goes).
    #[serde(default)]
    pub images: Vec<ImagePlacement>,
    /// PNG bytes for images not yet sent to this client (base64), sent once.
    #[serde(default)]
    pub image_data: Vec<ImageBlob>,
    /// The screen was re-baselined (attach or resize): before applying this
    /// frame the client must erase all cells, delete every image placement
    /// (and its transmitted data), and forget its cached image state. A resize
    /// invalidates everything incremental — the emulator may have reflowed
    /// cells and kept placements the diff/delete stream no longer knows about.
    #[serde(default)]
    pub clear: bool,
    /// Set when the user picked a system in the power menu: the client should
    /// exit and hand this to `main`, which runs `ssh -t` in the real terminal
    /// (and the optional first-time setup). `None` on every normal frame.
    #[serde(default)]
    pub switch_to: Option<crate::systems::SwitchSpec>,
    /// Text the client should place on the HOST terminal's clipboard via
    /// OSC 52 (Logs-viewer copy; apps' own OSC 52 stores). One frame only.
    #[serde(default)]
    pub clipboard: Option<String>,
    /// A message for the user from the daemon, shown by the client on the real
    /// terminal after it exits (e.g. "session in use", "detached by another
    /// client"). Set on a final frame only.
    #[serde(default)]
    pub notice: Option<String>,
    /// A full-screen program (fbi with the images to show) the client should
    /// run on the real console: it exits like a detach, `main` runs the argv,
    /// then reattaches. Only sent to a client that reported `fb_console`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fullscreen: Option<Vec<String>>,
}

/// A run of changed cells on one row, left to right from `x`, that share
/// colours and attributes: `[x, y, fg, bg, attrs, text]` on the wire, with
/// colours as `0xRRGGBBAA` and attrs as bits (bold 1, italic 2, underline 4,
/// inverse 8). About a tenth the JSON of one `CellChange` object per cell.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Span(pub i32, pub i32, pub u32, pub u32, pub u8, pub String);

fn pack_rgba(c: crate::cell::Rgba) -> u32 { u32::from_be_bytes([c.r, c.g, c.b, c.a]) }

fn unpack_rgba(v: u32) -> crate::cell::Rgba {
    let [r, g, b, a] = v.to_be_bytes();
    crate::cell::Rgba { r, g, b, a }
}

fn pack_attrs(a: crate::cell::CellAttrs) -> u8 {
    a.bold as u8 | (a.italic as u8) << 1 | (a.underline as u8) << 2 | (a.inverse as u8) << 3
}

fn unpack_attrs(v: u8) -> crate::cell::CellAttrs {
    crate::cell::CellAttrs { bold: v & 1 != 0, italic: v & 2 != 0, underline: v & 4 != 0, inverse: v & 8 != 0 }
}

/// Pack `changes` (in any order) into [`Span`]s: a run continues while the next
/// change is the next column on the same row with the same style.
pub fn pack_changes(changes: &[CellChange]) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    let mut end = (i32::MIN, i32::MIN); // (row, column after the open span)
    for c in changes {
        let (fg, bg, at) = (pack_rgba(c.cell.fg), pack_rgba(c.cell.bg), pack_attrs(c.cell.attrs));
        match out.last_mut() {
            Some(s) if end == (c.y, c.x) && s.2 == fg && s.3 == bg && s.4 == at => s.5.push(c.cell.ch),
            _ => out.push(Span(c.x, c.y, fg, bg, at, c.cell.ch.to_string())),
        }
        end = (c.y, c.x + 1);
    }
    out
}

/// Expand `spans` back into one [`CellChange`] per cell, appended to `out`.
pub fn unpack_spans(spans: &[Span], out: &mut Vec<CellChange>) {
    for Span(x, y, fg, bg, at, text) in spans {
        let (fg, bg, attrs) = (unpack_rgba(*fg), unpack_rgba(*bg), unpack_attrs(*at));
        for (i, ch) in text.chars().enumerate() {
            out.push(CellChange { x: x + i as i32, y: *y, cell: crate::cell::Cell { ch, fg, bg, attrs } });
        }
    }
}

/// A request to place image `id` at `rect` (screen cells). `visible=false` tells
/// the client to remove the placement (occluded or window closed).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ImagePlacement {
    pub id: u64,
    pub rect: Rect,
    pub cols: u16,
    pub rows: u16,
    pub visible: bool,
    /// Source crop in image pixels (x, y, w, h) for a zoomed or panned view;
    /// `None` shows the whole image. Older clients ignore it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src: Option<[u32; 4]>,
}

/// PNG bytes for image `id`, base64-encoded, sent once per attach.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ImageBlob {
    pub id: u64,
    pub png_base64: String,
}

/// UI-state flags the client uses to decide where keyboard input goes. They lag
/// the daemon by one frame, which is imperceptible for typing.
///
/// `#[serde(default)]` makes the struct tolerant of version skew: a client built
/// against a newer `Flags` can still parse frames from an older daemon (missing
/// fields default to `false`) instead of dropping every frame and rendering a
/// blank desktop.
#[derive(Serialize, Deserialize, Clone, Copy, Default)]
#[serde(default)]
pub struct Flags {
    pub launcher_open: bool,
    pub spotlight_open: bool,
    pub store_focused: bool,
    pub settings_focused: bool,
    /// The settings panel is in a text-entry field (Apps section add/edit form),
    /// so the client forwards typed characters instead of treating them as
    /// navigation commands.
    pub settings_editing: bool,
    /// The working-directory picker overlay is open (the client routes navigation
    /// keys to it).
    pub dirpicker_open: bool,
    /// The picker's new-folder name input is active (client forwards typed chars).
    pub dirpicker_creating: bool,
    /// The keyboard-shortcut help overlay is showing (any key dismisses it).
    /// Legacy (the help overlay is now a window); always false, kept so old
    /// clients still parse the frame.
    pub help_open: bool,
    /// The help window is focused; the client routes scroll / close keys to it.
    pub help_focused: bool,
    /// The file-manager window is focused; the client routes navigation keys to it.
    pub filemanager_focused: bool,
    /// The file manager has a text overlay open (new-folder / rename); forward chars.
    pub filemanager_editing: bool,
    /// The file manager's context (right-click) menu is open; the client
    /// routes Up/Down/Enter/Esc to the menu instead of file navigation.
    pub filemanager_context: bool,
    /// The desktop has a rename/new-folder overlay open; forward typed chars.
    pub desktop_editing: bool,
    /// The desktop right-click menu is open; the client routes arrows, Enter
    /// and Esc to it.
    pub desktop_menu: bool,
    /// The daemon asked the client to detach (e.g. the Quit button was clicked).
    pub detach: bool,
    /// The daemon is reloading the frontend; the client should reconnect (not
    /// fully detach). Apps stay alive in the apphost.
    pub reload: bool,
    /// The focused app's content rect, set only when that app wants mouse. The
    /// client routes events inside it as `ClientMsg::MouseInput` (passthrough);
    /// `None` keeps all mouse on the normal chrome/WM path.
    pub app_area: Option<Rect>,
    /// The focused app has application cursor-key mode (DECCKM) enabled, so the
    /// client must encode arrows as `ESC O A..D` rather than `ESC [ A..D`.
    /// Apps that read xterm terminfo (e.g. midnight commander) bind only the
    /// application form, so without this their arrow keys do nothing.
    pub app_cursor: bool,
    /// A window rename is in progress; the client should forward typed
    /// characters to the rename buffer rather than the focused app.
    pub renaming: bool,
    /// The confirm-close dialog is open; the client routes Enter/Esc and y/n to
    /// it (confirm / cancel) instead of the focused app.
    pub confirm_close: bool,
    /// The launch-warning dialog is open (launching an app entry flagged
    /// `warn`); the client routes Enter/Esc and y/n to it (launch / cancel).
    pub launch_warn: bool,
    /// The power menu's "Add Remote" form is open; the client forwards typed
    /// characters and field navigation to it.
    pub power_editing: bool,
    /// A power-menu confirm/forget dialog is open; the client routes Enter/y
    /// (confirm) and Esc/n (cancel) to it.
    pub power_dialog: bool,
    /// No window is focused (empty desktop): bare Enter/Space open the launcher.
    pub no_window: bool,
    /// The window picker (leader f) is open; the client routes arrows, Tab,
    /// digits, Enter and Esc to it.
    pub window_picker: bool,
    /// The keyboard move / resize mode (leader w) is active; the client routes
    /// arrows, r, Enter and Esc to it and swallows other keys.
    pub move_mode: bool,
    /// The menubar volume popover is open (by click or leader =); the client
    /// routes arrows, Enter, m / Space and Esc to it.
    pub tray_volume: bool,
    /// The menubar brightness popover is open (by click or leader -); the
    /// client routes Left / Right, Enter and Esc to it.
    #[serde(default)]
    pub tray_brightness: bool,
    /// The menubar Info menu is open (by click or leader i); the client routes
    /// arrows, Tab, Enter and Esc to it.
    pub tray_info: bool,
    /// The clipboard history popover is open; the client routes keys to it.
    pub clipmenu_open: bool,
    /// The logs-viewer window is focused; the client routes scroll/copy keys to it.
    pub logs_focused: bool,
    /// An image viewer is focused; the client routes zoom, pan and next/previous
    /// keys and the wheel to it.
    pub image_focused: bool,
    /// The colour depth the user chose (Settings / `colors`), when it differs
    /// from detection: the client renders with it. `None` = its own detection.
    pub colors: Option<crate::display::ColorDepth>,
    /// The activity monitor is the focused window; the client routes
    /// navigation / kill keys to it.
    pub activity_focused: bool,
    /// The activity monitor is showing a kill-confirm overlay; the client
    /// forwards Enter / y to confirm and Esc / n to cancel.
    pub activity_confirming: bool,
}

/// Daemon answer to [`crate::session::ClientMsg::Status`] on the control socket
/// (one JSON line). Every field is `#[serde(default)]` so version skew degrades.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct StatusReply {
    /// A client is attached right now.
    pub attached: bool,
    /// Open windows in the session.
    pub windows: usize,
    /// Window titles, front-to-back order as the taskbar lists them.
    pub titles: Vec<String>,
}

/// Live status published by the render loop for the control thread to read (the
/// control thread has no access to `SessionCore`).
#[derive(Default)]
pub struct DaemonStatus {
    pub attached: std::sync::atomic::AtomicBool,
    pub windows: std::sync::atomic::AtomicUsize,
    /// Window titles as of the last publish (`tetron-wm mcp` lists them).
    pub titles: std::sync::Mutex<Vec<String>>,
    /// Bumped on every attach, so background pollers that were paused while
    /// detached can wake at once instead of waiting out their interval.
    pub attach_gen: std::sync::atomic::AtomicU64,
}

impl DaemonStatus {
    pub fn snapshot(&self) -> StatusReply {
        use std::sync::atomic::Ordering::Relaxed;
        let titles = self.titles.lock().map(|t| t.clone()).unwrap_or_default();
        StatusReply { attached: self.attached.load(Relaxed), windows: self.windows.load(Relaxed), titles }
    }

    /// Publish the window count and titles (the titles only when they changed).
    pub fn publish(&self, windows: usize, titles: impl FnOnce() -> Vec<String>) {
        self.windows.store(windows, std::sync::atomic::Ordering::Relaxed);
        let t = titles();
        if let Ok(mut cur) = self.titles.lock() {
            if *cur != t {
                *cur = t;
            }
        }
    }
}

/// Name of the default session. Its sockets live directly in the per-user
/// directory (the pre-named-sessions layout), so a daemon started by an older
/// binary stays reachable across an update.
pub const DEFAULT_SESSION: &str = "default";

static SESSION_OVERRIDE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Whether `name` is a valid session name: 1 to 32 chars of `[A-Za-z0-9_-]`.
pub fn valid_session_name(name: &str) -> bool {
    (1..=32).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Select the session for this process (`-s NAME`). First call wins.
pub fn set_session(name: &str) {
    let _ = SESSION_OVERRIDE.set(name.to_string());
}

/// The active session name: `-s NAME`, else `$TETRON_WM_SESSION`, else `default`.
/// App windows inherit `TETRON_WM_SESSION`, so `tetron-wm launch` run inside a
/// window talks to the session that window belongs to. An invalid name falls
/// back to the default.
pub fn session_name() -> String {
    SESSION_OVERRIDE
        .get()
        .cloned()
        .or_else(|| std::env::var("TETRON_WM_SESSION").ok())
        .filter(|n| valid_session_name(n))
        .unwrap_or_else(|| DEFAULT_SESSION.to_string())
}

/// Per-user root directory holding every session. Created mode `0700`.
pub fn socket_root() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
    base.join(format!("tetron-wm-{user}"))
}

/// Directory of session `name`: the root itself for `default`, else
/// `<root>/sessions/<name>`.
pub fn session_dir(name: &str) -> PathBuf {
    if name == DEFAULT_SESSION {
        socket_root()
    } else {
        socket_root().join("sessions").join(name)
    }
}

/// Directory that holds the active session's sockets. Created mode `0700` (see
/// [`ensure_socket_dir`]) so other local users cannot reach the sockets inside.
pub fn socket_dir() -> PathBuf {
    session_dir(&session_name())
}

/// Create the active session directory (and its parents) with mode `0700`.
pub fn ensure_socket_dir() -> std::io::Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let dir = socket_dir();
    let root = socket_root();
    std::fs::create_dir_all(&dir)?;
    let mut d = dir.as_path();
    loop {
        std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700))?;
        if d == root {
            break;
        }
        match d.parent() {
            Some(p) => d = p,
            None => break,
        }
    }
    Ok(dir)
}

/// Whether `dir` holds any session socket. A cleanly stopped session removes
/// its sockets (the empty directory remains), so it is not listed; a crashed
/// one leaves them behind and shows as stale.
fn has_sockets(dir: &Path) -> bool {
    ["daemon.sock", "daemon-ctl.sock", "apphost.sock"].iter().any(|f| dir.join(f).exists())
}

/// Sessions with sockets on disk: `default` (if its legacy sockets exist)
/// plus every named session, sorted.
pub fn list_sessions() -> Vec<String> {
    let mut out = Vec::new();
    if has_sockets(&socket_root()) {
        out.push(DEFAULT_SESSION.to_string());
    }
    if let Ok(rd) = std::fs::read_dir(socket_root().join("sessions")) {
        let mut named: Vec<String> = rd
            .flatten()
            .filter(|e| has_sockets(&e.path()))
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| valid_session_name(n))
            .collect();
        named.sort();
        out.extend(named);
    }
    out
}

/// Path of the per-user daemon socket.
pub fn socket_path() -> PathBuf {
    socket_dir().join("daemon.sock")
}

/// Path of the daemon's out-of-band control socket. `tetron-wm kill` / `tetron-wm reload`
/// send here so they work even while a client is attached — the daemon serves a
/// single client on `socket_path()` serially, so a control message on the main
/// socket would queue behind the attached client and never be read.
pub fn daemon_ctl_path() -> PathBuf {
    socket_dir().join("daemon-ctl.sock")
}

/// Path to the apphost socket (apps live behind this; survives frontend restarts).
pub fn apphost_socket_path() -> PathBuf {
    socket_dir().join("apphost.sock")
}

/// Path to the apphost's control socket: one request, one reply (`tetron-wm ps`,
/// `kill-app`). Answered while a frontend holds `apphost.sock`, which the
/// apphost serves one connection at a time.
pub fn apphost_ctl_socket_path() -> PathBuf {
    socket_dir().join("apphost-ctl.sock")
}

/// Send one newline-framed message, then keep the connection open reading to EOF
/// so the recipient processes the message before our socket closes.
///
/// Without this, a control command (`kill`/`reload`) that writes-then-exits can
/// race the daemon: the daemon hits a broken pipe on its next frame *write* and
/// bails before its reader thread has delivered our queued message. Holding the
/// connection open (until the daemon acts and closes its end) removes the race.
pub fn send_and_drain(stream: &mut std::os::unix::net::UnixStream, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Read;
    std::io::Write::write_all(stream, bytes)?;
    let _ = stream.shutdown(std::net::Shutdown::Write);
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(3)));
    let mut sink = [0u8; 256];
    loop {
        match stream.read(&mut sink) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
    }
    Ok(())
}

/// Send a control message (Shutdown/Reload) to the daemon. Returns whether a
/// daemon was reachable.
///
/// The daemon serves its single attached client serially, so a control message
/// on the main socket would queue behind that client and never be read. We send
/// to the out-of-band control socket (read by the daemon's control thread even
/// while attached), and also poke the main socket so an *unattached* daemon —
/// blocked in `accept()` — wakes and re-checks the control flag (this also covers
/// an older daemon that predates the control socket).
pub fn send_control(msg: &crate::session::ClientMsg) -> std::io::Result<bool> {
    let mut buf = serde_json::to_vec(msg).map_err(std::io::Error::other)?;
    buf.push(b'\n');
    let mut reached = false;
    if let Ok(mut s) = std::os::unix::net::UnixStream::connect(daemon_ctl_path()) {
        let _ = send_and_drain(&mut s, &buf);
        reached = true;
    }
    if let Ok(mut s) = std::os::unix::net::UnixStream::connect(socket_path()) {
        // Best-effort wake poke: write and close without draining so we don't
        // block when the daemon is busy serving an attached client.
        let _ = std::io::Write::write_all(&mut s, &buf);
        let _ = s.shutdown(std::net::Shutdown::Write);
        reached = true;
    }
    Ok(reached)
}

/// Ask the running daemon for its status over the control socket (`None`: no
/// daemon, or one too old to answer).
pub fn query_status() -> Option<StatusReply> {
    use std::io::BufRead;
    let mut s = std::os::unix::net::UnixStream::connect(daemon_ctl_path()).ok()?;
    let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(2)));
    let mut buf = serde_json::to_vec(&crate::session::ClientMsg::Status).ok()?;
    buf.push(b'\n');
    std::io::Write::write_all(&mut s, &buf).ok()?;
    let _ = s.shutdown(std::net::Shutdown::Write);
    let mut line = String::new();
    std::io::BufReader::new(s).read_line(&mut line).ok().filter(|n| *n > 0)?;
    serde_json::from_str(line.trim()).ok()
}
