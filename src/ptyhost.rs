use crate::buffer::CellBuffer;
use crate::cell::{Cell, CellAttrs, Rgba};
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::Config;
use alacritty_terminal::vte::ansi::{Color as AColor, NamedColor, Processor, StdSyncHandler};
use alacritty_terminal::Term;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

// A cell's "default" foreground/background (and the fallback for unresolved
// named colors) comes from the active desktop theme — `text` and `window_bg` —
// snapshotted once per frame at the render loop below, so an app's
// default-background cells blend into the window and follow theme switches
// (e.g. the light theme's black-on-white terminals).

/// Forwards the embedded terminal's replies back to the child over the PTY.
///
/// Apps that probe the terminal — e.g. `tetris` moving the cursor to `999;999`
/// and sending `ESC[6n` to learn the screen size, or a primary device-attributes
/// query — expect an answer on their stdin. The emulator generates those replies
/// as [`Event::PtyWrite`]; without forwarding them the app sees silence (tetris
/// concluded "terminal too small" and exited). Tetron-wm still polls the grid via
/// [`AppInstance::snapshot`]; this only handles the write-back replies.
#[derive(Clone)]
struct PtyResponder {
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    /// Bell rings since last drain (surfaced as dock/tray notifications).
    bells: Arc<std::sync::atomic::AtomicU32>,
    /// Last OSC-52 clipboard store from the app (forwarded to the host terminal).
    clip: Arc<Mutex<Option<String>>>,
    /// OSC-52 clipboard READ queries from the app, awaiting an answer.
    loads: Arc<Mutex<Vec<crate::apphost::proto::ClipReq>>>,
}
impl EventListener for PtyResponder {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(text) => {
                if let Ok(mut w) = self.writer.lock() {
                    let _ = w.write_all(text.as_bytes());
                    let _ = w.flush();
                }
            }
            Event::ColorRequest(index, fmt) => {
                // An app asked for a colour (OSC 10/11/12 or OSC 4): answer with
                // the theme's, so apps that pick a light or dark scheme from the
                // terminal background (vim `background`, editors, prompts) match
                // the desktop theme. Unanswered, they time out and guess.
                let rgb = query_color(index, &crate::theme::current());
                let reply = fmt(alacritty_terminal::vte::ansi::Rgb { r: rgb.r, g: rgb.g, b: rgb.b });
                if let Ok(mut w) = self.writer.lock() {
                    let _ = w.write_all(reply.as_bytes());
                    let _ = w.flush();
                }
            }
            Event::Bell => {
                self.bells.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            Event::ClipboardStore(_, text) => {
                if let Ok(mut c) = self.clip.lock() {
                    *c = Some(text);
                }
            }
            Event::ClipboardLoad(ty, fmt) => {
                // The emulator hands us a formatter for the reply; render it with
                // empty text to learn which terminator the app used, and let the
                // frontend (which owns the clipboard) answer.
                use alacritty_terminal::term::ClipboardType;
                let kind = match ty {
                    ClipboardType::Clipboard => 'c',
                    ClipboardType::Selection => 'p',
                };
                let bel = fmt("").ends_with('\x07');
                if let Ok(mut l) = self.loads.lock() {
                    if l.len() < 16 {
                        l.push(crate::apphost::proto::ClipReq { kind, bel });
                    }
                }
            }
            _ => {}
        }
    }
}

/// Hosts a child process running inside a pseudo-terminal.
///
/// The child's output is parsed by a full [`alacritty_terminal`] emulator on a
/// dedicated reader thread (chosen over a minimal parser because the desktop must
/// faithfully render demanding TUIs such as `btop`). [`snapshot`](Self::snapshot)
/// converts the current emulator grid into a Tetron-wm [`CellBuffer`].
pub struct AppInstance {
    term: Arc<Mutex<Term<PtyResponder>>>,
    master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    /// Kitty-graphics state captured from the child's output by the reader thread.
    graphics: Arc<Mutex<crate::kittygfx::GraphicsState>>,
    /// Bell rings + OSC-52 clipboard stores captured by the emulator's events.
    bells: Arc<std::sync::atomic::AtomicU32>,
    clip: Arc<Mutex<Option<String>>>,
    loads: Arc<Mutex<Vec<crate::apphost::proto::ClipReq>>>,
    /// Set when anything a frame carries may have changed (output parsed,
    /// resize, scroll); cleared by [`take_dirty`](Self::take_dirty).
    dirty: Arc<AtomicBool>,
    cols: u16,
    rows: u16,
    /// Wall-clock instant the child was spawned. Used by the activity monitor to
    /// show how long each app has been running.
    spawned_at: Instant,
}

impl AppInstance {
    /// Spawn `cmd` with `args` inside a PTY of size `cols × rows`.
    ///
    /// Returns `Err` if the PTY or the child process could not be created.
    pub fn spawn(
        cmd: &str,
        args: &[String],
        cols: i32,
        rows: i32,
        cwd: Option<&std::path::Path>,
    ) -> std::io::Result<AppInstance> {
        let (cols, rows) = (cols.max(1) as u16, rows.max(1) as u16);
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| std::io::Error::other(e.to_string()))?;

        // Resolve a bare command name to an absolute path ourselves. portable-pty's
        // own PATH search is unreliable here (a binary on $PATH still failed to
        // launch by bare name), so we hand it the resolved path.
        let program = resolve_program(cmd);
        let mut builder = CommandBuilder::new(&program);
        for a in args {
            builder.arg(a);
        }
        // Inherit the parent environment so apps on the user's PATH (e.g.
        // Homebrew binaries in /opt/homebrew/bin) launch and get HOME/LANG.
        for (key, val) in std::env::vars() {
            builder.env(key, val);
        }
        // Clipboard shims (xclip/xsel/wl-copy/wl-paste) first on PATH when enabled,
        // so apps without a GUI clipboard use ours.
        let shim = crate::clipcli::shim_env();
        if let Some(dir) = &shim.dir {
            let old = std::env::var("PATH").unwrap_or_default();
            builder.env("PATH", format!("{}:{old}", dir.display()));
        }
        if shim.fake_display {
            builder.env("WAYLAND_DISPLAY", "tetron-wm");
        }
        // Pin TERM/COLORTERM to what the embedded emulator implements, letting
        // apps emit 24-bit color (captured here, re-emitted per the real terminal).
        // TERM stays `xterm-256color` (universally present in terminfo) — using
        // `xterm-kitty` would break ncurses apps on hosts lacking that entry. Kitty
        // graphics support is instead advertised at runtime: the reader thread's tap
        // answers the graphics query (`a=q`) with OK, which graphics-capable apps
        // (yazi, timg) probe for and treat as the authoritative capability signal.
        // Apps belong to this session: `tetron-wm launch/clip/...` run inside a
        // window must reach it, not the default session.
        builder.env("TETRON_WM_SESSION", crate::protocol::session_name());
        builder.env("TERM", "xterm-256color");
        builder.env("COLORTERM", "truecolor");
        // Start in the requested working directory, else the user's home.
        match cwd {
            Some(d) => builder.cwd(d),
            None => {
                if let Some(home) = dirs::home_dir() {
                    builder.cwd(home);
                }
            }
        }

        let child = pair
            .slave
            .spawn_command(builder)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        drop(pair.slave);

        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let writer: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(
            pair.master
                .take_writer()
                .map_err(|e| std::io::Error::other(e.to_string()))?,
        ));

        // The emulator's reply events (DSR/DA responses) are written back to the
        // child over the same PTY writer.
        let bells = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let clip: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let loads: Arc<Mutex<Vec<crate::apphost::proto::ClipReq>>> = Arc::new(Mutex::new(Vec::new()));
        // OSC 52 in both directions: stores feed the clipboard history and
        // reads (`ESC ] 52 ; c ; ? ST`) are answered by the frontend.
        // Scrollback is read per spawn, so a config change applies to new windows.
        let history = crate::config::Config::load().scrollback.min(crate::config::MAX_SCROLLBACK);
        let term_cfg = Config {
            osc52: alacritty_terminal::term::Osc52::CopyPaste,
            scrolling_history: history,
            ..Config::default()
        };
        let term = Arc::new(Mutex::new(Term::new(
            term_cfg,
            &TermSize::new(cols as usize, rows as usize),
            PtyResponder { writer: writer.clone(), bells: bells.clone(), clip: clip.clone(), loads: loads.clone() },
        )));

        // Captured Kitty-graphics state, shared with the reader thread.
        let graphics = Arc::new(Mutex::new(crate::kittygfx::GraphicsState::new()));

        // Reader thread: pump PTY bytes through a graphics tap, then the emulator.
        // The tap pulls Kitty-graphics APC sequences out of the stream (so the
        // emulator never sees them) and records image transmissions/placements at
        // the cursor. The `Processor` and `GraphicsTap` persist across reads so
        // partial escape/APC sequences are handled.
        let tclone = term.clone();
        let gclone = graphics.clone();
        let wclone = writer.clone();
        let dirty = Arc::new(AtomicBool::new(true));
        let dclone = dirty.clone();
        std::thread::spawn(move || {
            let mut parser = Processor::<StdSyncHandler>::new();
            let mut tap = crate::kittygfx::GraphicsTap::new();
            let mut split = crate::kittygfx::Split::default();
            let mut buf = [0u8; 8192];
            loop {
                let n = match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                // A misbehaving or crashing child (e.g. carbonyl aborting on
                // resize) can emit malformed/pathological escape sequences that
                // panic the emulator parser. That panic MUST NOT propagate: it
                // runs while holding the `term` lock, so an unguarded panic here
                // poisons the lock and the next `snapshot()` in the daemon then
                // panics too, taking the whole WM down. Contain it to this pane:
                // log it and stop reading (the dead pane is reaped normally).
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    // Plain output (the bulk of it) goes to the emulator as read;
                    // only chunks touching a graphics APC are copied by the tap.
                    let chunk = &buf[..n];
                    let text = if tap.passes_through(chunk) {
                        split.commands.clear();
                        chunk
                    } else {
                        tap.feed_into(chunk, &mut split);
                        &split.passthrough[..]
                    };
                    if let Ok(mut t) = tclone.lock() {
                        if split.commands.is_empty() {
                            parser.advance(&mut *t, text);
                        } else if let Ok(mut g) = gclone.lock() {
                            // Each command applies at the cursor where it sat in
                            // the stream, not where the whole read left it.
                            let mut done = 0;
                            for (cmd, &at) in split.commands.iter().zip(&split.at) {
                                parser.advance(&mut *t, &text[done..at]);
                                done = at;
                                let (col, row) = cursor_cell(&t);
                                g.apply(cmd, col, row);
                            }
                            parser.advance(&mut *t, &text[done..]);
                            // Answer any `a=q` support queries on the PTY so apps
                            // proceed to actually transmit graphics.
                            if !g.queries.is_empty() {
                                if let Ok(mut w) = wclone.lock() {
                                    for q in g.queries.drain(..) {
                                        let _ = w.write_all(&q);
                                    }
                                    let _ = w.flush();
                                }
                            }
                        } else {
                            parser.advance(&mut *t, text);
                        }
                    }
                }));
                dclone.store(true, Ordering::Release);
                notify_output();
                if outcome.is_err() {
                    crate::dbg_log(
                        "ptyhost: reader thread caught a panic parsing child output; \
                         stopping this pane's reader (WM unaffected)",
                    );
                    break;
                }
            }
            // EOF: the child exited, wake the host so it reports it promptly.
            dclone.store(true, Ordering::Release);
            notify_output();
        });

        Ok(AppInstance { term, master: pair.master, writer, child, graphics, bells, clip, loads, dirty, cols, rows, spawned_at: Instant::now() })
    }

    /// Whether the app may have changed since the last call (cleared). Starts
    /// true. Clear it BEFORE reading state, so output parsed meanwhile marks
    /// the app dirty again instead of being lost.
    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::Acquire)
    }

    /// Mark the app changed by something other than its output (a theme change
    /// recolours the snapshot, a test injects graphics).
    pub fn mark_dirty(&self) {
        self.dirty.store(true, Ordering::Release);
    }

    /// Bell rings since the last call (drained).
    pub fn take_bells(&self) -> u32 {
        self.bells.swap(0, std::sync::atomic::Ordering::Relaxed)
    }

    /// The app's latest OSC-52 clipboard store, if any (drained).
    pub fn take_clipboard(&self) -> Option<String> {
        self.clip.lock().ok().and_then(|mut c| c.take())
    }

    /// OSC-52 clipboard READ queries the app made since the last call (drained).
    pub fn take_clip_requests(&self) -> Vec<crate::apphost::proto::ClipReq> {
        self.loads.lock().map(|mut l| std::mem::take(&mut *l)).unwrap_or_default()
    }

    /// Convert the current emulator grid into a Tetron-wm [`CellBuffer`].
    pub fn snapshot(&self) -> CellBuffer {
        use alacritty_terminal::grid::Dimensions;
        // Recover a poisoned lock instead of unwrapping: if some other code path
        // ever panicked while holding `term`, we render the (stale) grid
        // best-effort rather than letting the poison cascade and kill the WM.
        let t = self.term.lock().unwrap_or_else(|e| e.into_inner());
        let grid = t.grid();
        // `Grid`'s `Index<Line>` reads raw storage WITHOUT applying the display
        // offset, so we must subtract it ourselves: at offset 0 the visible row
        // y is storage Line(y) (the active screen); scrolled back by N, row y is
        // Line(y - N), i.e. N lines up into history. Without this, scrolling
        // changed the offset but the snapshot never moved (no visible scroll).
        let off = grid.display_offset() as i32;
        // Clamp every index to the grid's ACTUAL geometry. Our cached
        // `self.cols/self.rows` can momentarily disagree with the grid during a
        // resize, and indexing a Line/Column out of range panics inside
        // alacritty — which would take the daemon down. Valid line range is
        // -(history) ..= screen_lines-1; valid columns are 0..grid.columns().
        let gcols = grid.columns() as i32;
        let gscreen = grid.screen_lines() as i32;
        let ghist = grid.total_lines() as i32 - gscreen; // scrollback depth
        let mut buf = CellBuffer::new(self.cols as i32, self.rows as i32);
        let th = crate::theme::current(); // default fg/bg follow the active theme
        // Palette entries the app redefined (OSC 4, e.g. pyradio's themes,
        // which curses sets with init_color); OSC 104 clears them again.
        let set = palette_overrides(t.colors());
        for y in 0..self.rows as i32 {
            let line = y - off;
            // Outside the grid's line range -> leave the row blank.
            if line < -ghist || line >= gscreen {
                continue;
            }
            for x in 0..self.cols as usize {
                // Past the grid's last column -> leave the rest of the row blank.
                if x as i32 >= gcols {
                    break;
                }
                let cell = &grid[Line(line)][Column(x)];
                let ch = if cell.c == '\0' { ' ' } else { cell.c };
                let flags = cell.flags;
                buf.set(
                    x as i32,
                    y,
                    Cell {
                        ch,
                        fg: resolve_color(cell.fg, th.text, &th.ansi, &set),
                        bg: resolve_color(cell.bg, th.window_bg, &th.ansi, &set),
                        attrs: CellAttrs {
                            bold: flags.contains(Flags::BOLD),
                            italic: flags.contains(Flags::ITALIC),
                            underline: flags.contains(Flags::UNDERLINE),
                            inverse: flags.contains(Flags::INVERSE),
                        },
                    },
                );
            }
        }
        buf
    }

    /// Resize both the PTY (sends `SIGWINCH`) and the emulator grid.
    pub fn resize(&mut self, cols: i32, rows: i32) {
        self.cols = cols.max(1) as u16;
        self.rows = rows.max(1) as u16;
        let _ = self.master.resize(PtySize {
            rows: self.rows,
            cols: self.cols,
            pixel_width: 0,
            pixel_height: 0,
        });
        if let Ok(mut t) = self.term.lock() {
            t.resize(TermSize::new(self.cols as usize, self.rows as usize));
        }
        self.mark_dirty();
    }

    /// Forward raw input bytes to the child. Any keystroke snaps the view back
    /// to the live bottom of the buffer (like a real terminal), so typing while
    /// scrolled back doesn't leave you reading stale output.
    pub fn write_input(&mut self, bytes: &[u8]) {
        if !bytes.is_empty() {
            if let Ok(mut t) = self.term.lock() {
                t.scroll_display(alacritty_terminal::grid::Scroll::Bottom);
            }
            self.mark_dirty();
        }
        if let Ok(mut w) = self.writer.lock() {
            let _ = w.write_all(bytes);
            let _ = w.flush();
        }
    }

    /// Scroll the scrollback view by `lines` (positive = back into history).
    /// The next [`snapshot`](Self::snapshot) reflects it automatically, since
    /// the grid index honours the display offset. Returns the resulting offset
    /// (0 = pinned to the live bottom) so callers can show a scrollbar.
    pub fn scroll(&mut self, lines: i32) -> usize {
        self.mark_dirty();
        if let Ok(mut t) = self.term.lock() {
            t.scroll_display(alacritty_terminal::grid::Scroll::Delta(lines));
            t.grid().display_offset()
        } else {
            0
        }
    }

    /// Current scrollback position and total history depth, for the scrollbar:
    /// `(display_offset, history_size)`. Offset 0 means pinned to the bottom.
    pub fn scroll_state(&self) -> (usize, usize) {
        use alacritty_terminal::grid::Dimensions;
        match self.term.lock() {
            Ok(t) => {
                let g = t.grid();
                // history = total buffered lines − the visible screen.
                let history = g.total_lines().saturating_sub(g.screen_lines());
                (g.display_offset(), history)
            }
            Err(_) => (0, 0),
        }
    }

    /// The app's current terminal mouse mode (what it asked the terminal for).
    pub fn mouse_mode(&self) -> crate::mouse::AppMouse {
        use alacritty_terminal::term::TermMode;
        let guard = self.term.lock().unwrap_or_else(|e| e.into_inner());
        let mode = guard.mode();
        crate::mouse::AppMouse {
            report_click: mode.contains(TermMode::MOUSE_REPORT_CLICK),
            report_drag: mode.contains(TermMode::MOUSE_DRAG),
            report_motion: mode.contains(TermMode::MOUSE_MOTION),
            sgr: mode.contains(TermMode::SGR_MOUSE),
            utf8: mode.contains(TermMode::UTF8_MOUSE),
            alternate_scroll: mode.contains(TermMode::ALTERNATE_SCROLL),
            alt_screen: mode.contains(TermMode::ALT_SCREEN),
            app_cursor: mode.contains(TermMode::APP_CURSOR),
            bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
        }
    }

    /// The emulator's text cursor as a visible `(col, row)`, or `None` when the
    /// app hid it (DECTCEM) or it is scrolled out of view. Row is in the same
    /// coordinate space as [`snapshot`](Self::snapshot): a visible row `y` shows
    /// grid `Line(y - display_offset)`, so the active-screen cursor line `L`
    /// appears at `y = L + display_offset` (and is hidden once scrolled back).
    pub fn cursor(&self) -> Option<(u16, u16)> {
        use alacritty_terminal::term::TermMode;
        let t = self.term.lock().unwrap_or_else(|e| e.into_inner());
        if !t.mode().contains(TermMode::SHOW_CURSOR) {
            return None;
        }
        let grid = t.grid();
        let off = grid.display_offset() as i32;
        let p = grid.cursor.point;
        let x = p.column.0 as i32;
        let y = p.line.0 + off;
        if x < 0 || x >= self.cols as i32 || y < 0 || y >= self.rows as i32 {
            return None;
        }
        Some((x as u16, y as u16))
    }

    /// Kill the child process.
    pub fn kill(&mut self) {
        let _ = self.child.kill();
    }

    /// Whether the child process is still running.
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// The child process's OS pid, if available. `None` when the platform can't
    /// report one (or the child has already been reaped).
    pub fn process_id(&self) -> Option<u32> {
        self.child.process_id()
    }

    /// The wall-clock instant this instance was spawned.
    pub fn spawned_at(&self) -> Instant {
        self.spawned_at
    }

    /// Lock and return this app's captured Kitty-graphics state (placements +
    /// decoded images), for the session to turn into image placements.
    pub fn graphics(&self) -> std::sync::MutexGuard<'_, crate::kittygfx::GraphicsState> {
        self.graphics.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Process-wide "some app produced output" signal, so the apphost loop sleeps
/// until there is work instead of polling. The flag keeps a reader from paying
/// for a notify while one is already pending.
static OUTPUT: (Mutex<()>, Condvar) = (Mutex::new(()), Condvar::new());
static OUTPUT_PENDING: AtomicBool = AtomicBool::new(false);

/// Wake [`wait_output`]. Called by reader threads after parsing a chunk; also
/// usable by other event sources (frontend commands) to cut a wait short.
pub fn notify_output() {
    if !OUTPUT_PENDING.swap(true, Ordering::AcqRel) {
        let _guard = OUTPUT.0.lock().unwrap_or_else(|e| e.into_inner());
        OUTPUT.1.notify_all();
    }
}

/// Block until [`notify_output`] fires or `timeout` passes; true when woken.
/// Clears the signal, so the next wait blocks until new output.
pub fn wait_output(timeout: Duration) -> bool {
    let guard = OUTPUT.0.lock().unwrap_or_else(|e| e.into_inner());
    let (_guard, _) = OUTPUT
        .1
        .wait_timeout_while(guard, timeout, |_| !OUTPUT_PENDING.load(Ordering::Acquire))
        .unwrap_or_else(|e| e.into_inner());
    OUTPUT_PENDING.swap(false, Ordering::AcqRel)
}

/// The current cursor cell `(col, row)` of the embedded emulator — where a
/// Kitty `a=T` transmit-and-display places its image.
fn cursor_cell(term: &Term<PtyResponder>) -> (u16, u16) {
    let p = term.grid().cursor.point;
    (p.column.0 as u16, p.line.0.max(0) as u16)
}

/// Resolve a bare command name (no `/`) to an absolute path by searching `$PATH`.
/// Returns the input unchanged if it's already a path or can't be found (so the
/// error surfaces from the spawn rather than here).
fn resolve_program(cmd: &str) -> String {
    if cmd.contains('/') {
        return cmd.to_string();
    }
    if let Some(path) = std::env::var_os("PATH") {
        use std::os::unix::fs::PermissionsExt;
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(cmd);
            if let Ok(meta) = std::fs::metadata(&candidate) {
                if meta.is_file() && meta.permissions().mode() & 0o111 != 0 {
                    return candidate.to_string_lossy().into_owned();
                }
            }
        }
    }
    cmd.to_string()
}

/// The colour reported for a colour query: palette entries 0-255 as drawn,
/// then the terminal's default foreground (256), background (257) and cursor
/// (258) from the theme, the same defaults `snapshot` paints with.
fn query_color(index: usize, th: &crate::theme::Theme) -> Rgba {
    match index {
        0..=255 => idx_to_rgb(index as u8, &th.ansi),
        257 => th.window_bg,
        _ => th.text,
    }
}

/// The palette entries 0-255 the app set with OSC 4 (None: as drawn by default).
type Overrides = [Option<Rgba>; 256];

fn palette_overrides(colors: &alacritty_terminal::term::color::Colors) -> Overrides {
    std::array::from_fn(|i| colors[i].map(|c| Rgba::rgb(c.r, c.g, c.b)))
}

/// Resolve an alacritty cell color into our [`Rgba`]: a palette entry the app
/// redefined (`set`, OSC 4) wins over the theme's table and the xterm palette.
fn resolve_color(c: AColor, default: Rgba, ansi: &[Rgba; 16], set: &Overrides) -> Rgba {
    match c {
        AColor::Spec(rgb) => Rgba::rgb(rgb.r, rgb.g, rgb.b),
        AColor::Indexed(i) => set[i as usize].unwrap_or_else(|| idx_to_rgb(i, ansi)),
        AColor::Named(n) => match set.get(n as usize).copied().flatten() {
            Some(c) => c,
            None => named_to_rgb(n, default, ansi),
        },
    }
}

/// Map a named terminal color to RGB; `default` covers the terminal's own
/// default fg/bg and any names we don't special-case (cursor, dim variants).
fn named_to_rgb(n: NamedColor, default: Rgba, ansi: &[Rgba; 16]) -> Rgba {
    use NamedColor::*;
    match n {
        Foreground => default,
        Background => default,
        Black => ansi[0],
        Red => ansi[1],
        Green => ansi[2],
        Yellow => ansi[3],
        Blue => ansi[4],
        Magenta => ansi[5],
        Cyan => ansi[6],
        White => ansi[7],
        BrightBlack => ansi[8],
        BrightRed => ansi[9],
        BrightGreen => ansi[10],
        BrightYellow => ansi[11],
        BrightBlue => ansi[12],
        BrightMagenta => ansi[13],
        BrightCyan => ansi[14],
        BrightWhite => ansi[15],
        _ => default,
    }
}

/// Convert an xterm 256-color index to RGB (16 base + 6×6×6 cube + grayscale ramp).
fn idx_to_rgb(i: u8, ansi: &[Rgba; 16]) -> Rgba {
    if (i as usize) < 16 {
        return ansi[i as usize];
    }
    if i >= 232 {
        let v = 8 + (i - 232) * 10;
        return Rgba::rgb(v, v, v);
    }
    let i = i - 16;
    let (r, g, b) = (i / 36, (i % 36) / 6, i % 6);
    let s = |n: u8| if n == 0 { 0 } else { 55 + n * 40 };
    Rgba::rgb(s(r), s(g), s(b))
}

#[cfg(test)]
mod tests {

    use super::*;
    use alacritty_terminal::vte::ansi::Processor;

    #[test]
    fn colour_queries_report_the_theme_defaults() {
        let th = crate::theme::Theme::midnight();
        assert_eq!(query_color(257, &th), th.window_bg);
        assert_eq!(query_color(256, &th), th.text);
        assert_eq!(query_color(1, &th), idx_to_rgb(1, &th.ansi));
    }

    #[test]
    fn themed_ansi_palette_colours_indexed_and_named_cells() {
        let mut ansi = crate::theme::DEFAULT_ANSI;
        ansi[1] = Rgba::rgb(1, 2, 3);
        let d = Rgba::rgb(9, 9, 9);
        let none = [None; 256];
        assert_eq!(resolve_color(AColor::Named(NamedColor::Red), d, &ansi, &none), Rgba::rgb(1, 2, 3));
        assert_eq!(resolve_color(AColor::Indexed(1), d, &ansi, &none), Rgba::rgb(1, 2, 3));
        // The 6x6x6 cube and grey ramp are not part of the table.
        assert_eq!(resolve_color(AColor::Indexed(196), d, &ansi, &none), Rgba::rgb(255, 0, 0));
        let mut th = crate::theme::Theme::midnight();
        th.ansi = ansi;
        assert_eq!(query_color(1, &th), Rgba::rgb(1, 2, 3));
    }

    /// pyradio's themes: curses init_color sends OSC 4, which redefines a
    /// palette entry; cells in that colour must show the new value, and OSC 104
    /// puts the palette back.
    #[test]
    fn osc4_redefined_palette_entries_colour_the_cells() {
        #[derive(Clone)]
        struct Quiet;
        impl EventListener for Quiet {}
        let mut term = Term::new(Config::default(), &TermSize::new(10, 2), Quiet);
        let mut p: Processor<StdSyncHandler> = Processor::new();
        p.advance(&mut term, b"\x1b]4;20;rgb:12/34/56\x1b\\\x1b]4;3;rgb:aa/bb/cc\x07\x1b[38;5;20mA\x1b[33mB");
        let set = palette_overrides(term.colors());
        let th = crate::theme::Theme::midnight();
        let g = term.grid();
        let fg = |x: usize| resolve_color(g[Line(0)][Column(x)].fg, th.text, &th.ansi, &set);
        assert_eq!(fg(0), Rgba::rgb(0x12, 0x34, 0x56), "indexed colour 20 as redefined");
        assert_eq!(fg(1), Rgba::rgb(0xaa, 0xbb, 0xcc), "named yellow (3) as redefined");
        p.advance(&mut term, b"\x1b]104\x07");
        let set = palette_overrides(term.colors());
        assert_eq!(set[20], None, "OSC 104 resets the palette");
    }

    /// An OSC 11 background query reaches `PtyResponder` as `ColorRequest(257)`
    /// and the reply carries the theme's window background.
    #[test]
    fn emulator_background_query_is_answered_with_the_theme() {
        #[derive(Clone)]
        struct Colors(Arc<Mutex<Vec<String>>>);
        impl EventListener for Colors {
            fn send_event(&self, e: Event) {
                if let Event::ColorRequest(i, fmt) = e {
                    let c = query_color(i, &crate::theme::Theme::midnight());
                    self.0.lock().unwrap().push(fmt(alacritty_terminal::vte::ansi::Rgb { r: c.r, g: c.g, b: c.b }));
                }
            }
        }
        let rec = Colors(Arc::new(Mutex::new(Vec::new())));
        let mut term = Term::new(Config::default(), &TermSize::new(80, 24), rec.clone());
        let mut parser = Processor::<StdSyncHandler>::new();
        parser.advance(&mut term, b"\x1b]11;?\x07");
        let out = rec.0.lock().unwrap();
        // midnight window_bg (17,20,29) as 16-bit-per-channel X11 rgb.
        assert!(out.iter().any(|s| s.contains("11;rgb:1111/1414/1d1d")), "{:?}", *out);
    }

    /// Records the emulator's write-back replies.
    #[derive(Clone)]
    struct Rec(Arc<Mutex<Vec<String>>>);
    impl EventListener for Rec {
        fn send_event(&self, e: Event) {
            if let Event::PtyWrite(s) = e {
                self.0.lock().unwrap().push(s);
            }
        }
    }

    /// A graphics command is placed at the cursor where it sat in the output,
    /// even when the same write then moves the cursor on (w3m does this).
    #[test]
    fn graphics_place_at_the_cursor_of_the_command_not_the_end_of_the_read() {
        // A 1x1 PNG, transmitted and displayed (a=T) at row 3, column 5.
        let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
        let script = format!(r"printf '\033[3;5H\033_Ga=T,f=100,i=7,c=2,r=1;{png}\033\\\r\n\r\nafter'; sleep 5");
        let app = AppInstance::spawn("sh", &["-c".into(), script], 20, 8, None).unwrap();
        let t0 = Instant::now();
        while !app.snapshot().get(0, 4).is_some_and(|c| c.ch == 'a') {
            assert!(t0.elapsed() < Duration::from_secs(5), "no output");
            std::thread::sleep(Duration::from_millis(10));
        }
        let g = app.graphics();
        let p = g.placements.first().expect("placement captured");
        assert_eq!((p.col, p.row), (4, 2), "placed where the cursor was at the command");
    }

    /// Output, resize and scroll mark an app dirty; reading clears it, and a
    /// quiet app stays clean so the apphost can skip its snapshot.
    #[test]
    fn dirty_tracks_output_resize_and_scroll() {
        let mut app = AppInstance::spawn("sh", &["-c".into(), "printf hello; sleep 5".into()], 20, 4, None).unwrap();
        assert!(app.take_dirty(), "a new app starts dirty");
        let t0 = Instant::now();
        while !app.snapshot().get(0, 0).is_some_and(|c| c.ch == 'h') {
            assert!(t0.elapsed() < Duration::from_secs(5), "no output");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(app.take_dirty(), "output marks the app dirty");
        std::thread::sleep(Duration::from_millis(100));
        assert!(!app.take_dirty(), "a quiet app stays clean");
        app.resize(30, 5);
        assert!(app.take_dirty(), "resize marks dirty");
        app.scroll(1);
        assert!(app.take_dirty(), "scroll marks dirty");
        app.kill();
    }

    #[test]
    fn notify_output_wakes_a_waiter() {
        notify_output();
        assert!(wait_output(Duration::from_secs(5)), "a pending notify returns at once");
        let t = std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(50));
            notify_output();
        });
        let t0 = Instant::now();
        // Other tests' readers may notify too; either way the wait ends early.
        assert!(wait_output(Duration::from_secs(5)));
        assert!(t0.elapsed() < Duration::from_secs(4));
        t.join().unwrap();
    }

    #[test]
    fn resolves_bare_command_to_absolute_path() {
        // `sh` is always on $PATH at an absolute location.
        let p = resolve_program("sh");
        assert!(p.contains('/'), "expected an absolute path, got {p}");
        assert!(std::path::Path::new(&p).is_file());
        // A path is returned unchanged.
        assert_eq!(resolve_program("/bin/sh"), "/bin/sh");
    }

    // Apps like tetris move the cursor to 999;999 and send ESC[6n to learn the
    // terminal size from the cursor-position report. The emulator must answer.
    #[test]
    fn emulator_answers_cursor_position_query() {
        let rec = Rec(Arc::new(Mutex::new(Vec::new())));
        let mut term = Term::new(Config::default(), &TermSize::new(80, 24), rec.clone());
        let mut parser = Processor::<StdSyncHandler>::new();
        parser.advance(&mut term, b"\x1b[999;999H\x1b[6n");
        let out = rec.0.lock().unwrap();
        assert!(
            out.iter().any(|s| s.starts_with("\x1b[") && s.ends_with('R')),
            "expected a cursor-position report, got {:?}",
            *out
        );
    }
}
