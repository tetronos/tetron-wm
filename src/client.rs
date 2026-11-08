//! The thin client: sets up the terminal, renders frames received from the
//! daemon, and forwards input. Holds no session state — it routes keyboard input
//! using the [`Flags`] the daemon sends each frame.

/// How a client session ended.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientExit {
    /// The user detached / shut down — stop.
    Detached,
    /// The daemon asked the client to reconnect (frontend reload).
    Reload,
    /// The user picked a system in the power menu: `main` should run the ssh
    /// switch (and optional first-time setup) in the real terminal, then
    /// re-attach locally when the remote session ends.
    Switch(crate::systems::SwitchSpec),
    /// "View full screen": `main` runs this program (fbi and the images) on
    /// the real console, then re-attaches when it exits.
    Fullscreen(Vec<String>),
}

use crate::geometry::{Point, SnapZone};
use crate::protocol::{Flags, FrameMsg};
use crate::session::ClientMsg;
use crate::terminal::{frame_to_ansi, Terminal};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Attach to the daemon over `stream` and run until the user detaches.
pub fn run(stream: UnixStream, takeover: bool) -> std::io::Result<ClientExit> {
    let term = Terminal::enter()?;
    let caps = term.caps;
    let (w, h) = Terminal::size()?;

    let mut out_stream = stream.try_clone()?;
    send(&mut out_stream, &ClientMsg::Resize { w, h, takeover })?;
    // Tell the daemon what this display can show, so it adapts the theme and
    // wallpaper (an older daemon ignores the message), and that this client
    // reads packed cells (an older daemon ignores the field and sends `changes`).
    send(&mut out_stream, &ClientMsg::Caps(crate::display::ClientCaps { packed_cells: true, ..caps.client_caps() }))?;

    // A per-system theme rides over ssh as TETRON_WM_THEME: apply it to this
    // daemon's config as soon as we attach.
    if let Ok(theme) = std::env::var("TETRON_WM_THEME") {
        if !theme.is_empty() {
            send(&mut out_stream, &ClientMsg::SetTheme(theme))?;
        }
    }

    let cfg = crate::config::Config::load();
    let (keymap, warns) = crate::keymap::Keymap::new(&cfg.keys, cfg.leader.as_deref());
    for w in warns {
        crate::dbg_log(&format!("keymap: {w}"));
    }

    let flags = Arc::new(Mutex::new(Flags::default()));
    let detached = Arc::new(AtomicBool::new(false));
    let reload = Arc::new(AtomicBool::new(false));
    let switch: Arc<Mutex<Option<crate::systems::SwitchSpec>>> = Arc::new(Mutex::new(None));
    let fullscreen: Arc<Mutex<Option<Vec<String>>>> = Arc::new(Mutex::new(None));
    // A message from the daemon to print once the terminal is restored.
    let notice: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    // Reader thread: socket frames → ANSI → stdout.
    {
        let flags = flags.clone();
        let detached = detached.clone();
        let reload = reload.clone();
        let switch = switch.clone();
        let fullscreen = fullscreen.clone();
        let notice = notice.clone();
        let reader_stream = stream.try_clone()?;
        std::thread::spawn(move || {
            let mut r = BufReader::new(reader_stream);
            let mut line = String::new();
            let mut out = std::io::stdout();
            // Image ids already transmitted, and the placement geometry currently
            // displayed for each (so we only re-place on move/resize).
            let mut transmitted: std::collections::HashSet<u64> = std::collections::HashSet::new();
            let mut active: std::collections::HashMap<u32, Placed> = std::collections::HashMap::new();
            // The depth this terminal was detected with; the user's choice
            // (Settings / `colors`) arrives in the flags and replaces it.
            let detected = caps.depth;
            let mut caps = caps;
            loop {
                line.clear();
                match r.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if let Ok(mut msg) = serde_json::from_str::<FrameMsg>(line.trim()) {
                            *flags.lock().unwrap() = msg.flags;
                            caps.depth = msg.flags.colors.unwrap_or(detected);
                            // The whole frame goes out in one write.
                            let mut frame = String::new();
                            if let Some(text) = msg.notice.take() {
                                *notice.lock().unwrap() = Some(text);
                            }
                            if let Some(text) = msg.clipboard.take() {
                                // OSC 52: set the host terminal's clipboard
                                // (Ghostty/Kitty/WezTerm; forwarded over ssh).
                                let b64 = crate::kitty::b64(text.as_bytes());
                                frame.push_str(&format!("\x1b]52;c;{b64}\x07"));
                            }
                            if let Some(spec) = msg.switch_to.take() {
                                crate::dbg_log(&format!("client: switch requested → {} ({})", spec.name, spec.host));
                                *switch.lock().unwrap() = Some(spec);
                            }
                            if let Some(argv) = msg.fullscreen.take() {
                                crate::dbg_log(&format!("client: full screen requested: {argv:?}"));
                                *fullscreen.lock().unwrap() = Some(argv);
                            }
                            if msg.clear {
                                // Re-baseline (attach / resize): erase every cell and
                                // delete all image placements *and* their data. A cell
                                // repaint alone can never remove an image — terminals
                                // composite graphics over text — and after a resize the
                                // emulator's screen/image state can diverge from our
                                // incremental model, leaving orphaned icons on screen.
                                frame.push_str("\x1b[0m\x1b[2J\x1b_Ga=d,d=A\x1b\\");
                                transmitted.clear();
                                active.clear();
                            }
                            crate::protocol::unpack_spans(&msg.spans, &mut msg.changes);
                            frame.push_str(&frame_to_ansi(&msg.changes, &caps));
                            if caps.kitty_graphics {
                                frame.push_str(&reconcile_images(&msg, &mut transmitted, &mut active));
                            }
                            let _ = out.write_all(frame.as_bytes());
                            let _ = out.flush();
                            if msg.flags.reload {
                                reload.store(true, Ordering::SeqCst);
                                break;
                            }
                            if msg.flags.detach {
                                break;
                            }
                        }
                    }
                }
            }
            detached.store(true, Ordering::SeqCst);
        });
    }

    // On a bare Linux console, gpm provides the mouse (the VT emits no xterm
    // mouse sequences). No-op on GUI terminals / non-Linux.
    crate::gpm::start(flags.clone(), out_stream.try_clone()?);

    let mut leader = false;
    // Set after leader Tab / Shift+Tab: further bare Tab / Shift+Tab keep
    // stepping focus without the leader, Enter / Esc end the cycle (consumed),
    // and any other key ends it and is handled normally.
    let mut cycling = false;
    let mut last_click: Option<(Point, std::time::Instant)> = None;
    let mut grab: Option<Grab> = None;
    loop {
        if detached.load(Ordering::SeqCst) {
            break;
        }
        if event::poll(Duration::from_millis(16))? {
            let f = *flags.lock().unwrap();
            match event::read()? {
                Event::Key(k) if k.kind != KeyEventKind::Release => {
                    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
                    let ctrl_alt = ctrl && k.modifiers.contains(KeyModifiers::ALT);
                    let is_leader = keymap.is_leader(k.code, k.modifiers);
                    // Every key ends a focus cycle unless its branch re-arms it.
                    let was_cycling = std::mem::take(&mut cycling);
                    let shift = k.modifiers.contains(KeyModifiers::SHIFT);
                    // kmscon and the Linux VT send the Backspace key as Ctrl-H
                    // (0x08), not DEL (0x7f)/KeyCode::Backspace, so none of
                    // tetron-wm's text fields would erase (they match Backspace).
                    // Normalize it to Backspace for all of tetron-wm's own key
                    // routing below; the app-passthrough path keeps the raw key
                    // (raw_code/raw_mods) so Ctrl-H still reaches apps (readline,
                    // emacs). Terminals that send DEL already arrive as Backspace.
                    let raw_code = k.code;
                    let raw_mods = k.modifiers;
                    let mut k = k;
                    if matches!(k.code, KeyCode::Char('h')) && k.modifiers == KeyModifiers::CONTROL {
                        k.code = KeyCode::Backspace;
                        k.modifiers = KeyModifiers::NONE;
                    }

                    // Global clipboard shortcuts (classic terminal bindings),
                    // handled before any mode routing so they work everywhere:
                    //   Shift+Insert  -> paste the primary (mouse) selection
                    //   Ctrl+Shift+C  -> copy the selection (clipboard + host OSC 52)
                    //   Ctrl+Shift+V  -> paste the clipboard
                    if shift && k.code == KeyCode::Insert {
                        send(&mut out_stream, &ClientMsg::PastePrimary)?;
                    } else if ctrl && shift && matches!(k.code, KeyCode::Char('c') | KeyCode::Char('C')) {
                        send(&mut out_stream, &ClientMsg::CopySelection)?;
                    } else if ctrl && shift && matches!(k.code, KeyCode::Char('v') | KeyCode::Char('V')) {
                        send(&mut out_stream, &ClientMsg::PasteClipboard)?;
                    } else if shift && k.code == KeyCode::PageUp {
                        send(&mut out_stream, &ClientMsg::ScrollFocused(1))?;
                    } else if shift && k.code == KeyCode::PageDown {
                        send(&mut out_stream, &ClientMsg::ScrollFocused(-1))?;
                    } else if f.confirm_close {
                        // The confirm-close dialog is modal: Enter/y confirm, Esc/n cancel.
                        leader = false;
                        match k.code {
                            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                                send(&mut out_stream, &ClientMsg::ConfirmCloseYes)?
                            }
                            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                                send(&mut out_stream, &ClientMsg::ConfirmCloseNo)?
                            }
                            _ => {}
                        }
                    } else if f.launch_warn {
                        // The launch-warning dialog is modal: Enter/y launches, Esc/n cancels.
                        leader = false;
                        match k.code {
                            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                                send(&mut out_stream, &ClientMsg::LaunchWarnYes)?
                            }
                            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                                send(&mut out_stream, &ClientMsg::LaunchWarnNo)?
                            }
                            _ => {}
                        }
                    } else if f.power_editing {
                        // The Add Remote form is modal: forward typing + field nav.
                        leader = false;
                        match k.code {
                            KeyCode::Esc => send(&mut out_stream, &ClientMsg::PowerFormCancel)?,
                            KeyCode::Enter => send(&mut out_stream, &ClientMsg::PowerFormCommit)?,
                            KeyCode::Tab | KeyCode::Down => send(&mut out_stream, &ClientMsg::PowerFormNext)?,
                            KeyCode::BackTab | KeyCode::Up => send(&mut out_stream, &ClientMsg::PowerFormPrev)?,
                            KeyCode::Left => send(&mut out_stream, &ClientMsg::PowerFormLeft)?,
                            KeyCode::Right => send(&mut out_stream, &ClientMsg::PowerFormRight)?,
                            KeyCode::Backspace => send(&mut out_stream, &ClientMsg::PowerFormBackspace)?,
                            KeyCode::Char(c) if !ctrl => send(&mut out_stream, &ClientMsg::PowerFormChar(c))?,
                            _ => {}
                        }
                    } else if f.clipmenu_open {
                        // Clipboard history popover: typing filters; Alt+c/p/d, Insert, Delete act.
                        use crate::clipmenu::ClipKey as K;
                        let alt = k.modifiers.contains(KeyModifiers::ALT) && !ctrl;
                        // Action letters: Shift+Alt+letter (GUI shortcuts rarely take
                        // it; plain Alt+letter works too), or the leader then the
                        // letter, which no GUI ever sees.
                        let action_letter = |c: char| match c.to_ascii_lowercase() {
                            'c' => Some(K::Copy),
                            'p' => Some(K::Pin),
                            'd' => Some(K::Delete),
                            _ => None,
                        };
                        let key = if leader {
                            // Leader pending inside the popover: next key is an action.
                            leader = false;
                            match k.code {
                                KeyCode::Char(c) => action_letter(c),
                                _ => None,
                            }
                        } else if is_leader {
                            leader = true;
                            None
                        } else {
                            // Ctrl+C, Ctrl+D, Ctrl+P and the other terminal control
                            // keys, and F-keys, are never reused here.
                            match (k.code, ctrl) {
                                (KeyCode::Up, _) => Some(K::Up),
                                (KeyCode::Down, _) => Some(K::Down),
                                (KeyCode::PageUp, _) => Some(K::PageUp),
                                (KeyCode::PageDown, _) => Some(K::PageDown),
                                (KeyCode::Home, _) => Some(K::Home),
                                (KeyCode::End, _) => Some(K::End),
                                (KeyCode::Enter, _) => Some(K::Enter),
                                (KeyCode::Esc, _) => Some(K::Esc),
                                (KeyCode::Backspace, _) => Some(K::Backspace),
                                (KeyCode::Tab, _) => Some(K::Pause),
                                (KeyCode::Insert, _) => Some(K::Pin),
                                (KeyCode::Delete, _) => Some(K::Delete),
                                (KeyCode::Char(c), false) if alt => action_letter(c),
                                (KeyCode::Char(c), false) => Some(K::Char(c)),
                                _ => None,
                            }
                        };
                        if let Some(key) = key {
                            send(&mut out_stream, &ClientMsg::ClipMenuKey(key))?;
                        }
                    } else if f.window_picker {
                        // Window picker is modal: arrows / Tab / j k move, Enter or
                        // a digit picks, Esc / q closes, other keys are ignored.
                        leader = false;
                        let msg = match k.code {
                            KeyCode::Up | KeyCode::BackTab | KeyCode::Char('k') => Some(ClientMsg::WindowPickerStep(-1)),
                            KeyCode::Down | KeyCode::Tab | KeyCode::Char('j') => Some(ClientMsg::WindowPickerStep(1)),
                            KeyCode::Enter => Some(ClientMsg::WindowPickerEnter),
                            KeyCode::Esc | KeyCode::Char('q') => Some(ClientMsg::WindowPickerEsc),
                            KeyCode::Char(c @ '1'..='9') => Some(ClientMsg::WindowPickerNumber(c as u8 - b'0')),
                            _ => None,
                        };
                        if let Some(msg) = msg {
                            send(&mut out_stream, &msg)?;
                        }
                    } else if f.tray_info {
                        // Info menu (click or leader i): arrows / Tab move, Enter
                        // opens the row, Esc or the leader closes.
                        leader = false;
                        use crate::tray::TrayKey as T;
                        let key = match k.code {
                            KeyCode::Up | KeyCode::BackTab => Some(T::Up),
                            KeyCode::Down | KeyCode::Tab => Some(T::Down),
                            KeyCode::Enter => Some(T::Enter),
                            KeyCode::Esc => Some(T::Esc),
                            _ if is_leader => Some(T::Esc),
                            _ => None,
                        };
                        if let Some(key) = key {
                            send(&mut out_stream, &ClientMsg::TrayKey(key))?;
                        }
                    } else if f.tray_volume {
                        // Volume popover (opened by click or leader =): Left / Right
                        // level, Up / Down rows, Enter, m / Space mute, Esc. The
                        // leader closes it; other keys are ignored while it is open.
                        leader = false;
                        use crate::tray::TrayKey as T;
                        let key = match k.code {
                            KeyCode::Left | KeyCode::Char('-') => Some(T::Left),
                            KeyCode::Right | KeyCode::Char('=') | KeyCode::Char('+') => Some(T::Right),
                            KeyCode::Up => Some(T::Up),
                            KeyCode::Down => Some(T::Down),
                            KeyCode::Enter => Some(T::Enter),
                            KeyCode::Char('m') | KeyCode::Char('M') | KeyCode::Char(' ') => Some(T::Mute),
                            KeyCode::Esc => Some(T::Esc),
                            _ if is_leader => Some(T::Esc),
                            _ => None,
                        };
                        if let Some(key) = key {
                            send(&mut out_stream, &ClientMsg::TrayKey(key))?;
                        }
                    } else if f.tray_brightness {
                        // Brightness popover (click or leader -): Left / Right
                        // level, Enter or Esc closes; the leader closes it too.
                        leader = false;
                        use crate::tray::TrayKey as T;
                        let key = match k.code {
                            KeyCode::Left | KeyCode::Char('-') => Some(T::Left),
                            KeyCode::Right | KeyCode::Char('=') | KeyCode::Char('+') => Some(T::Right),
                            KeyCode::Enter => Some(T::Enter),
                            KeyCode::Esc => Some(T::Esc),
                            _ if is_leader => Some(T::Esc),
                            _ => None,
                        };
                        if let Some(key) = key {
                            send(&mut out_stream, &ClientMsg::TrayKey(key))?;
                        }
                    } else if f.move_mode {
                        // Keyboard move mode is modal: arrows step 1 cell (Shift: 5),
                        // r toggles resize, Enter keeps, Esc undoes. Other keys
                        // are swallowed so a stray key never reaches the app.
                        leader = false;
                        let n = if shift { 5 } else { 1 };
                        let msg = match k.code {
                            KeyCode::Left => Some(ClientMsg::MoveModeStep { dx: -n, dy: 0 }),
                            KeyCode::Right => Some(ClientMsg::MoveModeStep { dx: n, dy: 0 }),
                            KeyCode::Up => Some(ClientMsg::MoveModeStep { dx: 0, dy: -n }),
                            KeyCode::Down => Some(ClientMsg::MoveModeStep { dx: 0, dy: n }),
                            KeyCode::Char('r') | KeyCode::Char('R') => Some(ClientMsg::MoveModeToggleResize),
                            KeyCode::Enter => Some(ClientMsg::MoveModeEnd { cancel: false }),
                            KeyCode::Esc => Some(ClientMsg::MoveModeEnd { cancel: true }),
                            _ => None,
                        };
                        if let Some(msg) = msg {
                            send(&mut out_stream, &msg)?;
                        }
                    } else if was_cycling && !is_leader && matches!(k.code, KeyCode::Tab | KeyCode::BackTab | KeyCode::Enter | KeyCode::Esc) {
                        // Sticky focus cycle (see `cycling`).
                        match k.code {
                            KeyCode::Tab => { send(&mut out_stream, &ClientMsg::FocusNext)?; cycling = true; }
                            KeyCode::BackTab => { send(&mut out_stream, &ClientMsg::FocusPrev)?; cycling = true; }
                            _ => {}
                        }
                    } else if f.power_dialog {
                        // Power confirm / forget dialog: Enter/y confirms, Esc/n cancels.
                        leader = false;
                        match k.code {
                            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                                send(&mut out_stream, &ClientMsg::PowerKeyConfirm)?
                            }
                            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                                send(&mut out_stream, &ClientMsg::PowerKeyCancel)?
                            }
                            _ => {}
                        }
                    } else if leader {
                        leader = false;
                        match (keymap.action(k.code), k.code) {
                            (Some("spotlight"), _) => send(&mut out_stream, &ClientMsg::ToggleSpotlight)?,
                            (Some("launcher"), _) => send(&mut out_stream, &ClientMsg::ToggleMenu)?,
                            (Some("maximize"), _) => send(&mut out_stream, &ClientMsg::MaximizeFocused)?,
                            (Some("minimize"), _) => send(&mut out_stream, &ClientMsg::MinimizeFocused)?,
                            (Some("focus_next"), _) => { send(&mut out_stream, &ClientMsg::FocusNext)?; cycling = true; }
                            (Some("focus_prev"), _) => { send(&mut out_stream, &ClientMsg::FocusPrev)?; cycling = true; }
                            (Some("windows"), _) => send(&mut out_stream, &ClientMsg::WindowPickerOpen)?,
                            (Some("move"), _) => send(&mut out_stream, &ClientMsg::MoveModeStart)?,
                            (Some("volume"), _) => send(&mut out_stream, &ClientMsg::VolumeOpen)?,
                            (Some("brightness"), _) => send(&mut out_stream, &ClientMsg::BrightnessOpen)?,
                            (Some("info"), _) => send(&mut out_stream, &ClientMsg::InfoOpen)?,
                            (Some("close"), _) => send(&mut out_stream, &ClientMsg::CloseFocused)?,
                            (Some("snap_left"), _) => send(&mut out_stream, &ClientMsg::SnapFocused(SnapZone::Left))?,
                            (Some("snap_right"), _) => send(&mut out_stream, &ClientMsg::SnapFocused(SnapZone::Right))?,
                            (Some("store"), _) => send(&mut out_stream, &ClientMsg::OpenStore)?,
                            (Some("settings"), _) => send(&mut out_stream, &ClientMsg::OpenSettings)?,
                            (Some("activity"), _) => send(&mut out_stream, &ClientMsg::OpenActivity)?,
                            (Some("clipboard"), _) => send(&mut out_stream, &ClientMsg::ClipMenuToggle)?,
                            (Some("select"), _) => send(&mut out_stream, &ClientMsg::ToggleSelectMode)?,
                            (Some("copy"), _) => send(&mut out_stream, &ClientMsg::CopySelection)?,
                            (Some("paste"), _) => send(&mut out_stream, &ClientMsg::PasteClipboard)?,
                            (Some("help"), _) => send(&mut out_stream, &ClientMsg::ToggleHelp)?,
                            (Some("rename"), _) => send(&mut out_stream, &ClientMsg::RenameFocused)?,
                            (Some("tile"), _) => send(&mut out_stream, &ClientMsg::TileAll)?,
                            (Some("context_menu"), _) => send(&mut out_stream, &ClientMsg::ContextMenuOpen)?,
                            (Some("screensaver"), _) => send(&mut out_stream, &ClientMsg::Screensaver { mode: crate::screensaver::SaverMode::Toggle, effect: None })?,
                            (Some("screenshot"), _) => send(&mut out_stream, &ClientMsg::Screenshot(String::new()))?,
                            (Some("auto_tile"), _) => send(&mut out_stream, &ClientMsg::ToggleAutoTile)?,
                            (Some("detach"), _) => break,                       // detach (apps persist)
                            (Some("shutdown"), _) => { send(&mut out_stream, &ClientMsg::Shutdown)?; break; }
                            (_, KeyCode::Char(c @ '1'..='9')) => send(&mut out_stream, &ClientMsg::SendToCell(c as u8 - b'0'))?,
                            _ => {}
                        }
                    } else if is_leader {
                        // Before the Logs / Help / launcher / dir-picker branches:
                        // those used to swallow the leader, so no leader shortcut
                        // worked while one of them had focus.
                        leader = true;
                    } else if f.desktop_menu {
                        // The desktop right-click menu: arrows move, Enter acts,
                        // Esc closes; other keys are ignored while it is open.
                        use crate::session::MenuKey as M;
                        let key = match k.code {
                            KeyCode::Up => Some(M::Up),
                            KeyCode::Down | KeyCode::Tab => Some(M::Down),
                            KeyCode::BackTab => Some(M::Up),
                            KeyCode::Enter => Some(M::Enter),
                            KeyCode::Esc => Some(M::Esc),
                            _ => None,
                        };
                        if let Some(key) = key {
                            send(&mut out_stream, &ClientMsg::DesktopMenuKey(key))?;
                        }
                    } else if f.logs_focused {
                        match k.code {
                            KeyCode::Esc => send(&mut out_stream, &ClientMsg::LogsClose)?,
                            KeyCode::Up => send(&mut out_stream, &ClientMsg::LogsUp)?,
                            KeyCode::Down => send(&mut out_stream, &ClientMsg::LogsDown)?,
                            KeyCode::PageUp => send(&mut out_stream, &ClientMsg::LogsPageUp)?,
                            KeyCode::PageDown => send(&mut out_stream, &ClientMsg::LogsPageDown)?,
                            KeyCode::Char('c') | KeyCode::Char('C') => send(&mut out_stream, &ClientMsg::LogsCopy)?,
                            KeyCode::Char('r') | KeyCode::Char('R') => send(&mut out_stream, &ClientMsg::LogsRefresh)?,
                            _ => {}
                        }
                    } else if f.help_focused {
                        // The help window is focused: navigation keys scroll it,
                        // Esc / q / Enter / ? close it, every other key is ignored
                        // (it is a reference, a stray key must not dismiss it).
                        if let Some(msg) = help_key(k.code) {
                            send(&mut out_stream, &msg)?;
                        }
                    } else if f.launcher_open {
                        match k.code {
                            KeyCode::Esc => send(&mut out_stream, &ClientMsg::LauncherEsc)?,
                            KeyCode::Enter => send(&mut out_stream, &ClientMsg::LauncherEnter)?,
                            KeyCode::Up => send(&mut out_stream, &ClientMsg::LauncherUp)?,
                            KeyCode::Down => send(&mut out_stream, &ClientMsg::LauncherDown)?,
                            KeyCode::Left => send(&mut out_stream, &ClientMsg::LauncherLeft)?,
                            KeyCode::Right => send(&mut out_stream, &ClientMsg::LauncherRight)?,
                            KeyCode::Backspace => send(&mut out_stream, &ClientMsg::LauncherBackspace)?,
                            KeyCode::Char(c) if f.spotlight_open && !ctrl => send(&mut out_stream, &ClientMsg::LauncherChar(c))?,
                            _ => {}
                        }
                    } else if f.dirpicker_open && f.dirpicker_creating {
                        // Typing a new folder name.
                        match k.code {
                            KeyCode::Esc => send(&mut out_stream, &ClientMsg::DirPickerCancel)?,
                            KeyCode::Enter => send(&mut out_stream, &ClientMsg::DirPickerConfirm)?,
                            KeyCode::Backspace => send(&mut out_stream, &ClientMsg::DirPickerBackspace)?,
                            KeyCode::Char(c) if !ctrl => send(&mut out_stream, &ClientMsg::DirPickerChar(c))?,
                            _ => {}
                        }
                    } else if f.dirpicker_open {
                        match k.code {
                            KeyCode::Esc => send(&mut out_stream, &ClientMsg::DirPickerCancel)?,
                            KeyCode::Enter => send(&mut out_stream, &ClientMsg::DirPickerConfirm)?,
                            KeyCode::Up => send(&mut out_stream, &ClientMsg::DirPickerUp)?,
                            KeyCode::Down => send(&mut out_stream, &ClientMsg::DirPickerDown)?,
                            KeyCode::Right => send(&mut out_stream, &ClientMsg::DirPickerExpand)?,
                            KeyCode::Left => send(&mut out_stream, &ClientMsg::DirPickerCollapse)?,
                            KeyCode::Char('.') => send(&mut out_stream, &ClientMsg::DirPickerToggleHidden)?,
                            KeyCode::Char('n') | KeyCode::Char('N') => send(&mut out_stream, &ClientMsg::DirPickerNewFolder)?,
                            _ => {}
                        }
                    } else if f.store_focused {
                        match k.code {
                            KeyCode::Esc => send(&mut out_stream, &ClientMsg::StoreClose)?,
                            KeyCode::Enter => send(&mut out_stream, &ClientMsg::StoreActivate)?,
                            KeyCode::F(5) => send(&mut out_stream, &ClientMsg::StoreUpdateList)?,
                            KeyCode::Up => send(&mut out_stream, &ClientMsg::StoreUp)?,
                            KeyCode::Down => send(&mut out_stream, &ClientMsg::StoreDown)?,
                            KeyCode::Left => send(&mut out_stream, &ClientMsg::StorePrevCategory)?,
                            KeyCode::Right => send(&mut out_stream, &ClientMsg::StoreNextCategory)?,
                            KeyCode::Backspace => send(&mut out_stream, &ClientMsg::StoreBackspace)?,
                            KeyCode::Char(c) if !ctrl => send(&mut out_stream, &ClientMsg::StoreChar(c))?,
                            _ => {}
                        }
                    } else if f.settings_focused && f.settings_editing {
                        // Apps add form: forward typed characters into the field.
                        match k.code {
                            KeyCode::Esc => send(&mut out_stream, &ClientMsg::SettingsCancelEdit)?,
                            KeyCode::Enter => send(&mut out_stream, &ClientMsg::SettingsToggle)?,
                            KeyCode::Up => send(&mut out_stream, &ClientMsg::SettingsUp)?,
                            KeyCode::Down => send(&mut out_stream, &ClientMsg::SettingsDown)?,
                            KeyCode::Backspace => send(&mut out_stream, &ClientMsg::SettingsBackspace)?,
                            KeyCode::Char(c) if !ctrl => send(&mut out_stream, &ClientMsg::SettingsChar(c))?,
                            _ => {}
                        }
                    } else if f.settings_focused {
                        match k.code {
                            KeyCode::Esc => send(&mut out_stream, &ClientMsg::SettingsClose)?,
                            KeyCode::Up => send(&mut out_stream, &ClientMsg::SettingsUp)?,
                            KeyCode::Down => send(&mut out_stream, &ClientMsg::SettingsDown)?,
                            KeyCode::Left => send(&mut out_stream, &ClientMsg::SettingsLeft)?,
                            KeyCode::Right => send(&mut out_stream, &ClientMsg::SettingsRight)?,
                            // Tab / Shift+Tab move focus between the section list and
                            // the rows (like mc's panels); Up / Down then move inside
                            // the focused pane. PageDown / PageUp switch section
                            // directly from either pane.
                            KeyCode::Tab | KeyCode::BackTab => send(&mut out_stream, &ClientMsg::SettingsTogglePane)?,
                            KeyCode::PageDown => send(&mut out_stream, &ClientMsg::SettingsNextSection)?,
                            KeyCode::PageUp => send(&mut out_stream, &ClientMsg::SettingsPrevSection)?,
                            KeyCode::Enter | KeyCode::Char(' ') => send(&mut out_stream, &ClientMsg::SettingsToggle)?,
                            _ => {}
                        }
                    } else if f.activity_focused && f.activity_confirming {
                        // Kill-confirm overlay: Enter / y confirm, Esc / n cancel.
                        match k.code {
                            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => send(&mut out_stream, &ClientMsg::ActivityCancelKill)?,
                            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => send(&mut out_stream, &ClientMsg::ActivityConfirmKill)?,
                            _ => {}
                        }
                    } else if f.activity_focused {
                        match k.code {
                            KeyCode::Esc => send(&mut out_stream, &ClientMsg::ActivityClose)?,
                            KeyCode::Up => send(&mut out_stream, &ClientMsg::ActivityUp)?,
                            KeyCode::Down => send(&mut out_stream, &ClientMsg::ActivityDown)?,
                            KeyCode::Enter | KeyCode::Char('k') => send(&mut out_stream, &ClientMsg::ActivityKill)?,
                            KeyCode::Char('K') => send(&mut out_stream, &ClientMsg::ActivityKillDead)?,
                            KeyCode::Char('r') | KeyCode::Char('R') => send(&mut out_stream, &ClientMsg::ActivityRefresh)?,
                            _ => {}
                        }
                    } else if f.filemanager_focused && f.filemanager_editing {
                        // New-folder / rename overlay: forward typed characters.
                        match k.code {
                            KeyCode::Esc => send(&mut out_stream, &ClientMsg::FileManagerCancel)?,
                            KeyCode::Enter => send(&mut out_stream, &ClientMsg::FileManagerCommit)?,
                            KeyCode::Backspace => send(&mut out_stream, &ClientMsg::FileManagerBackspace)?,
                            KeyCode::Char(c) if !ctrl => send(&mut out_stream, &ClientMsg::FileManagerChar(c))?,
                            _ => {}
                        }
                    } else if f.filemanager_focused && f.filemanager_context {
                        // Context menu open: Up/Down move its highlight, Enter
                        // performs the highlighted action, Esc dismisses it —
                        // none of these should reach normal file navigation.
                        match k.code {
                            KeyCode::Esc => send(&mut out_stream, &ClientMsg::FileManagerCancel)?,
                            KeyCode::Up => send(&mut out_stream, &ClientMsg::FileManagerUp)?,
                            KeyCode::Down => send(&mut out_stream, &ClientMsg::FileManagerDown)?,
                            KeyCode::Enter => send(&mut out_stream, &ClientMsg::FileManagerCommit)?,
                            _ => {}
                        }
                    } else if f.filemanager_focused {
                        match (k.code, ctrl) {
                            (KeyCode::Esc, _) => send(&mut out_stream, &ClientMsg::FileManagerClose)?,
                            (KeyCode::Up, _) => send(&mut out_stream, &ClientMsg::FileManagerUp)?,
                            (KeyCode::Down, _) => send(&mut out_stream, &ClientMsg::FileManagerDown)?,
                            (KeyCode::Left, _) => send(&mut out_stream, &ClientMsg::FileManagerLeft)?,
                            (KeyCode::Right, _) => send(&mut out_stream, &ClientMsg::FileManagerRight)?,
                            (KeyCode::Enter, _) => send(&mut out_stream, &ClientMsg::FileManagerActivate)?,
                            (KeyCode::Backspace, _) => send(&mut out_stream, &ClientMsg::FileManagerParent)?,
                            (KeyCode::Char('c'), true) => send(&mut out_stream, &ClientMsg::FileManagerCopy)?,
                            (KeyCode::Char('x'), true) => send(&mut out_stream, &ClientMsg::FileManagerCut)?,
                            (KeyCode::Char('v'), true) => send(&mut out_stream, &ClientMsg::FileManagerPaste)?,
                            (KeyCode::Char('n'), true) => send(&mut out_stream, &ClientMsg::FileManagerNewFolder)?,
                            (KeyCode::Delete, _) => send(&mut out_stream, &ClientMsg::FileManagerDelete)?,
                            (KeyCode::F(2), _) => send(&mut out_stream, &ClientMsg::FileManagerRename)?,
                            (KeyCode::Char('1'), false) => send(&mut out_stream, &ClientMsg::FileManagerViewIcon)?,
                            (KeyCode::Char('2'), false) => send(&mut out_stream, &ClientMsg::FileManagerViewList)?,
                            (KeyCode::Char('3'), false) => send(&mut out_stream, &ClientMsg::FileManagerViewColumns)?,
                            (KeyCode::Char(' '), false) => send(&mut out_stream, &ClientMsg::FileManagerTogglePreview)?,
                            (KeyCode::Char('.'), false) => send(&mut out_stream, &ClientMsg::FileManagerToggleHidden)?,
                            (KeyCode::Char('t'), true) => send(&mut out_stream, &ClientMsg::FileManagerNewTab)?,
                            (KeyCode::Char('w'), true) => send(&mut out_stream, &ClientMsg::FileManagerCloseTab)?,
                            (KeyCode::Tab, false) => send(&mut out_stream, &ClientMsg::FileManagerNextTab)?,
                            _ => {}
                        }
                    } else if f.desktop_editing {
                        // Desktop rename / new-folder overlay: forward typed chars.
                        match k.code {
                            KeyCode::Esc => send(&mut out_stream, &ClientMsg::DesktopCancel)?,
                            KeyCode::Enter => send(&mut out_stream, &ClientMsg::DesktopCommit)?,
                            KeyCode::Backspace => send(&mut out_stream, &ClientMsg::DesktopBackspace)?,
                            KeyCode::Char(c) if !ctrl => send(&mut out_stream, &ClientMsg::DesktopChar(c))?,
                            _ => {}
                        }
                    } else if f.renaming {
                        // Window rename overlay: forward typed chars to the rename buffer.
                        match k.code {
                            KeyCode::Esc => send(&mut out_stream, &ClientMsg::RenameCancel)?,
                            KeyCode::Enter => send(&mut out_stream, &ClientMsg::RenameCommit)?,
                            KeyCode::Backspace => send(&mut out_stream, &ClientMsg::RenameBackspace)?,
                            KeyCode::Char(c) if !ctrl => send(&mut out_stream, &ClientMsg::RenameChar(c))?,
                            _ => {}
                        }
                    } else if let Some(key) = image_key(k.code).filter(|_| f.image_focused && !ctrl && !ctrl_alt) {
                        // The focused image viewer: after every overlay and
                        // text field, so those keep their keys.
                        send(&mut out_stream, &ClientMsg::ImageKey(key))?;
                    } else if ctrl_alt {
                        match k.code {
                            KeyCode::Char('q') => break,
                            KeyCode::Up => send(&mut out_stream, &ClientMsg::MaximizeFocused)?,
                            KeyCode::Down => send(&mut out_stream, &ClientMsg::MinimizeFocused)?,
                            KeyCode::Left => send(&mut out_stream, &ClientMsg::SnapFocused(SnapZone::Left))?,
                            KeyCode::Right => send(&mut out_stream, &ClientMsg::SnapFocused(SnapZone::Right))?,
                            _ => send(&mut out_stream, &ClientMsg::Key(encode_key(raw_code, raw_mods, f.app_cursor)))?,
                        }
                    } else if f.no_window && !ctrl && matches!(k.code, KeyCode::Enter | KeyCode::Char(' ')) {
                        // Empty desktop (nothing to receive input): Enter / Space open
                        // the launcher. Esc never opens anything.
                        send(&mut out_stream, &ClientMsg::ToggleMenu)?;
                    } else {
                        send(&mut out_stream, &ClientMsg::Key(encode_key(raw_code, raw_mods, f.app_cursor)))?;
                    }
                }
                Event::Mouse(me) => {
                    // A click ends a keyboard focus cycle (Tab goes to the app again).
                    if matches!(me.kind, crossterm::event::MouseEventKind::Down(_)) {
                        cycling = false;
                    }
                    let p = Point::new(me.column as i32, me.row as i32);
                    let mods = crate::mouse::MouseMods {
                        shift: me.modifiers.contains(KeyModifiers::SHIFT),
                        ctrl: me.modifiers.contains(KeyModifiers::CONTROL),
                        alt: me.modifiers.contains(KeyModifiers::ALT),
                    };
                    if let Some(ev) = to_mouse_input(&me, p, mods) {
                        route_mouse(&mut out_stream, &f, ev, &mut last_click, &mut grab)?;
                    }
                }
                Event::Resize(nc, nr) => send(&mut out_stream, &ClientMsg::Resize { w: nc as i32, h: nr as i32, takeover: false })?,
                _ => {}
            }
        }
    }

    // Detach: dropping `term` restores the screen; dropping the socket signals
    // the daemon, which keeps the session alive.
    drop(term);
    if let Some(text) = notice.lock().unwrap().take() {
        eprintln!("tetron-wm: {text}");
    }
    if let Some(argv) = fullscreen.lock().unwrap().take() {
        return Ok(ClientExit::Fullscreen(argv));
    }
    if let Some(spec) = switch.lock().unwrap().take() {
        return Ok(ClientExit::Switch(spec));
    }
    if reload.load(Ordering::SeqCst) {
        Ok(ClientExit::Reload)
    } else {
        Ok(ClientExit::Detached)
    }
}

/// What a key does while the help overlay is open: scroll it, close it
/// explicitly, or nothing (`None`): a stray key must not dismiss the reference.
fn help_key(code: KeyCode) -> Option<ClientMsg> {
    Some(match code {
        KeyCode::Up | KeyCode::Char('k') => ClientMsg::HelpScroll(-1),
        KeyCode::Down | KeyCode::Char('j') => ClientMsg::HelpScroll(1),
        KeyCode::PageUp => ClientMsg::HelpScroll(-10),
        KeyCode::PageDown | KeyCode::Char(' ') => ClientMsg::HelpScroll(10),
        KeyCode::Home => ClientMsg::HelpScroll(i32::MIN),
        KeyCode::End => ClientMsg::HelpScroll(i32::MAX),
        KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q' | 'Q' | '?') => ClientMsg::ToggleHelp,
        _ => return None,
    })
}

/// The image viewer's keys. Plain keys only (no terminal control keys); others
/// fall through to the normal routing.
fn image_key(code: KeyCode) -> Option<crate::imageview::ImageKey> {
    use crate::imageview::ImageKey as K;
    Some(match code {
        KeyCode::Char('+') | KeyCode::Char('=') => K::ZoomIn,
        KeyCode::Char('-') | KeyCode::Char('_') => K::ZoomOut,
        KeyCode::Char('0') => K::Fit,
        KeyCode::Left => K::Left,
        KeyCode::Right => K::Right,
        KeyCode::Up => K::Up,
        KeyCode::Down => K::Down,
        KeyCode::Char('n') | KeyCode::Char(' ') | KeyCode::PageDown => K::Next,
        KeyCode::Char('p') | KeyCode::Backspace | KeyCode::PageUp => K::Prev,
        KeyCode::Char('m') => K::Mode,
        KeyCode::Char('i') => K::Info,
        KeyCode::Char('f') => K::Fullscreen,
        KeyCode::Esc | KeyCode::Char('q') => K::Close,
        _ => return None,
    })
}

/// A placement the terminal holds: image id, cell origin, size and source crop.
type Placed = (u64, i32, i32, u16, u16, Option<[u32; 4]>);

/// Build the Kitty graphics escapes for a frame: transmit not-yet-seen image
/// blobs, (re)place images whose geometry changed, and delete placements that are
/// now hidden or gone. The cursor is saved/restored so cell rendering is intact.
fn reconcile_images(
    msg: &FrameMsg,
    transmitted: &mut std::collections::HashSet<u64>,
    active: &mut std::collections::HashMap<u32, Placed>,
) -> String {
    use std::fmt::Write;
    let mut g = String::new();
    for blob in &msg.image_data {
        if transmitted.insert(blob.id) {
            g.push_str(&crate::kitty::transmit_b64(blob.id, &blob.png_base64));
        }
    }
    // Key placements by screen position, not image id, so one image (e.g. a shared
    // file-type icon) can appear at many spots. `active` maps placement-key →
    // (image_id, x, y, cols, rows).
    let mut now: std::collections::HashMap<u32, Placed> = std::collections::HashMap::new();
    for p in &msg.images {
        // A negative origin can't be addressed: CUP parameters are 1-based, and
        // a 0/negative parameter aborts the escape, dropping the image at the
        // current cursor position — i.e. painted at an arbitrary screen spot.
        if p.visible && p.rect.x >= 0 && p.rect.y >= 0 {
            now.insert(place_key(p.rect.x, p.rect.y), (p.id, p.rect.x, p.rect.y, p.cols, p.rows, p.src));
        }
    }
    let mut ops = String::new();
    // Remove placements that are gone, or whose image changed at the same slot.
    for (&pk, &(img, ..)) in active.iter() {
        let keep = matches!(now.get(&pk), Some(&(new_img, ..)) if new_img == img);
        if !keep {
            ops.push_str(&crate::kitty::delete(img, pk));
        }
    }
    // (Re)place new or moved/changed placements.
    for (&pk, &(img, x, y, c, r, src)) in now.iter() {
        if active.get(&pk) != Some(&(img, x, y, c, r, src)) {
            let _ = write!(ops, "\x1b[{};{}H", y + 1, x + 1);
            ops.push_str(&crate::kitty::place_src(img, pk, c, r, src));
        }
    }
    if !ops.is_empty() {
        g.push_str("\x1b[s"); // save cursor
        g.push_str(&ops);
        g.push_str("\x1b[u"); // restore cursor
    }
    *active = now;
    g
}

/// A stable per-position placement id (Kitty `p=`), unique per cell coordinate.
fn place_key(x: i32, y: i32) -> u32 {
    (((x.max(0) as u32) & 0xffff) << 16) | ((y.max(0) as u32) & 0xffff)
}

/// Which target captured the left button at press time. The whole drag is routed
/// there until release — a standard pointer grab. Without it, dragging a
/// mouse-reporting app's titlebar downward "falls into" the app: the cursor crosses
/// the (one-frame-stale) `app_area` before the window follows, so a per-event
/// positional router would start sending the drag to the app instead of the WM.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Grab {
    App,
    Chrome,
}

/// Route one mouse event: into the focused app (passthrough) when the pointer is
/// in `f.app_area`, otherwise via the existing chrome/WM variants. A left-button
/// press grabs its target (`grab`) so the rest of the drag follows it regardless of
/// position. Shared by the crossterm path and the gpm reader so both behave
/// identically.
pub(crate) fn route_mouse(
    out: &mut UnixStream,
    f: &Flags,
    ev: crate::mouse::MouseInput,
    last_click: &mut Option<(Point, std::time::Instant)>,
    grab: &mut Option<Grab>,
) -> std::io::Result<()> {
    use crate::mouse::{MouseAction as A, MouseButton as B};
    let p = Point::new(ev.col, ev.row);
    // Whether this position would passthrough to the app (Shift forces tetron-wm's
    // own text-selection/paste over the app — the classic xterm override, so you
    // can still copy from vim/htop).
    let in_app = f.app_area.map(|r| r.contains(p)).unwrap_or(false) && !ev.mods.shift;

    // A left press establishes a pointer grab on whatever is under it; the matching
    // release clears it. A left drag/release in between follows that grab, ignoring
    // the current position — so a titlebar drag never falls into a mouse-reporting
    // app, and an app-content drag that wanders onto chrome still reaches the app.
    if ev.button == B::Left && ev.action == A::Down {
        *grab = Some(if in_app { Grab::App } else { Grab::Chrome });
    }
    // While a grab is held, a button-less Move is a drag continuation. Some
    // terminals (kmscon's experimental mouse) report motion during a drag as a
    // bare `Moved` with no button bit, so crossterm never yields a Drag event —
    // we track the held button ourselves via `grab` and treat the move as a drag
    // so titlebar move / edge resize (and in-app drags) work there too.
    if let Some(g) = *grab {
        let is_left_up = ev.button == B::Left && ev.action == A::Up;
        let is_left_drag = ev.button == B::Left && ev.action == A::Drag;
        let is_bare_move = ev.action == A::Move;
        if is_left_up || is_left_drag || is_bare_move {
            let r = match g {
                // Give the app a real Left drag, not a bare move, so mouse apps
                // (btop, vim, lazygit) see button-held motion.
                Grab::App if is_bare_move => send(
                    out,
                    &ClientMsg::MouseInput(crate::mouse::MouseInput {
                        button: B::Left,
                        action: A::Drag,
                        ..ev
                    }),
                ),
                Grab::App => send(out, &ClientMsg::MouseInput(ev)),
                Grab::Chrome if is_left_up => send(out, &ClientMsg::MouseUp(p)),
                Grab::Chrome => send(out, &ClientMsg::MouseDrag(p)),
            };
            if is_left_up {
                *grab = None;
            }
            return r;
        }
    }

    // When the focused app is grabbing the mouse, pass events straight through.
    if in_app {
        return send(out, &ClientMsg::MouseInput(ev));
    }
    match (ev.button, ev.action) {
        (B::Left, A::Down) => {
            let now = std::time::Instant::now();
            let dbl = last_click
                .map(|(lp, lt)| lp == p && now.duration_since(lt) < std::time::Duration::from_millis(400))
                .unwrap_or(false);
            if dbl {
                send(out, &ClientMsg::MouseDouble(p))?;
                *last_click = None;
            } else {
                send(out, &ClientMsg::MouseDown(p))?;
                *last_click = Some((p, now));
            }
        }
        (B::Right, A::Down) => send(out, &ClientMsg::MouseRightDown(p))?,
        (B::Left, A::Drag) => send(out, &ClientMsg::MouseDrag(p))?,
        (B::Left, A::Up) => send(out, &ClientMsg::MouseUp(p))?,
        // Middle-button press: primary-selection paste into the app under `p`.
        (B::Middle, A::Down) => send(out, &ClientMsg::MousePaste(p))?,
        (_, A::Move) => send(out, &ClientMsg::MouseMove(p))?,
        // Menubar row is chrome: always a positional scroll so the daemon can
        // route it (e.g. volume over the tray), never the focused widget's scroll.
        (_, A::ScrollUp) if p.y == 0 => send(out, &ClientMsg::ScrollAt { p, lines: 3 })?,
        (_, A::ScrollDown) if p.y == 0 => send(out, &ClientMsg::ScrollAt { p, lines: -3 })?,
        (_, A::ScrollUp) if f.store_focused => send(out, &ClientMsg::StoreUp)?,
        (_, A::ScrollDown) if f.store_focused => send(out, &ClientMsg::StoreDown)?,
        (_, A::ScrollUp) if f.filemanager_focused => send(out, &ClientMsg::FileManagerUp)?,
        (_, A::ScrollDown) if f.filemanager_focused => send(out, &ClientMsg::FileManagerDown)?,
        (_, A::ScrollUp) if f.help_focused => send(out, &ClientMsg::HelpScroll(-3))?,
        (_, A::ScrollDown) if f.help_focused => send(out, &ClientMsg::HelpScroll(3))?,
        (_, A::ScrollUp) if f.image_focused => send(out, &ClientMsg::ImageKey(crate::imageview::ImageKey::ZoomIn))?,
        (_, A::ScrollDown) if f.image_focused => send(out, &ClientMsg::ImageKey(crate::imageview::ImageKey::ZoomOut))?,
        (_, A::ScrollUp) if f.logs_focused => send(out, &ClientMsg::LogsUp)?,
        (_, A::ScrollDown) if f.logs_focused => send(out, &ClientMsg::LogsDown)?,
        // Anywhere else, the wheel scrolls the scrollback of the PTY window
        // under the pointer (a no-op if that's not a non-mouse app window).
        (_, A::ScrollUp) => send(out, &ClientMsg::ScrollAt { p, lines: 3 })?,
        (_, A::ScrollDown) => send(out, &ClientMsg::ScrollAt { p, lines: -3 })?,
        _ => {}
    }
    Ok(())
}

/// Map a crossterm [`event::MouseEvent`] to a [`crate::mouse::MouseInput`] for
/// passthrough to the focused PTY app. Returns `None` only if the event kind is
/// somehow unrecognised (should not happen in practice).
fn to_mouse_input(
    me: &event::MouseEvent,
    p: Point,
    mods: crate::mouse::MouseMods,
) -> Option<crate::mouse::MouseInput> {
    use crate::mouse::{MouseAction, MouseButton, MouseInput};
    use crossterm::event::{MouseButton as XB, MouseEventKind as K};
    let (button, action) = match me.kind {
        K::Down(XB::Left) => (MouseButton::Left, MouseAction::Down),
        K::Down(XB::Middle) => (MouseButton::Middle, MouseAction::Down),
        K::Down(XB::Right) => (MouseButton::Right, MouseAction::Down),
        K::Up(XB::Left) => (MouseButton::Left, MouseAction::Up),
        K::Up(XB::Middle) => (MouseButton::Middle, MouseAction::Up),
        K::Up(XB::Right) => (MouseButton::Right, MouseAction::Up),
        K::Drag(XB::Left) => (MouseButton::Left, MouseAction::Drag),
        K::Drag(XB::Middle) => (MouseButton::Middle, MouseAction::Drag),
        K::Drag(XB::Right) => (MouseButton::Right, MouseAction::Drag),
        K::Moved => (MouseButton::None, MouseAction::Move),
        K::ScrollUp => (MouseButton::None, MouseAction::ScrollUp),
        K::ScrollDown => (MouseButton::None, MouseAction::ScrollDown),
        K::ScrollLeft => (MouseButton::None, MouseAction::ScrollLeft),
        K::ScrollRight => (MouseButton::None, MouseAction::ScrollRight),
    };
    Some(MouseInput { col: p.x, row: p.y, button, action, mods })
}

/// Serialize a [`ClientMsg`] as a newline-delimited JSON frame.
fn send(stream: &mut UnixStream, msg: &ClientMsg) -> std::io::Result<()> {
    let mut buf = serde_json::to_vec(msg).map_err(std::io::Error::other)?;
    buf.push(b'\n');
    stream.write_all(&buf)
}

/// Encode a key into the bytes forwarded to the focused PTY app.
fn encode_key(code: KeyCode, mods: KeyModifiers, app_cursor: bool) -> Vec<u8> {
    // Cursor keys (arrows + Home/End) have two encodings: the CSI form
    // `ESC [ x` in normal mode, and the SS3 form `ESC O x` when the focused app
    // enabled application cursor-key mode (DECCKM). Apps that read xterm
    // terminfo bind only the application form (mc's kcuu1 = `\EOA`), so sending
    // the CSI form leaves their arrow keys dead. `app_cursor` is the focused
    // app's DECCKM state, relayed from the emulator via `Flags::app_cursor`.
    let seq = |c: u8| -> Vec<u8> {
        if app_cursor { vec![0x1b, b'O', c] } else { vec![0x1b, b'[', c] }
    };
    match code {
        KeyCode::Char(c) => {
            if mods.contains(KeyModifiers::CONTROL) {
                vec![(c.to_ascii_uppercase() as u8).wrapping_sub(0x40)]
            } else {
                c.to_string().into_bytes()
            }
        }
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => seq(b'A'),
        KeyCode::Down => seq(b'B'),
        KeyCode::Right => seq(b'C'),
        KeyCode::Left => seq(b'D'),
        KeyCode::Home => seq(b'H'),
        KeyCode::End => seq(b'F'),
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Rect;
    use crate::protocol::ImagePlacement;

    #[test]
    fn help_closes_only_on_explicit_keys() {
        for code in [KeyCode::Esc, KeyCode::Enter, KeyCode::Char('q'), KeyCode::Char('Q'), KeyCode::Char('?')] {
            assert!(matches!(help_key(code), Some(ClientMsg::ToggleHelp)), "{code:?} closes");
        }
        for code in [KeyCode::Char('x'), KeyCode::Char('a'), KeyCode::Tab, KeyCode::Backspace, KeyCode::F(1), KeyCode::Left] {
            assert!(help_key(code).is_none(), "{code:?} must not dismiss the reference");
        }
        assert!(matches!(help_key(KeyCode::Down), Some(ClientMsg::HelpScroll(1))));
        assert!(matches!(help_key(KeyCode::PageUp), Some(ClientMsg::HelpScroll(-10))));
    }

    fn frame_with(images: Vec<ImagePlacement>) -> FrameMsg {
        FrameMsg {
            changes: Vec::new(),
            spans: Vec::new(),
            cursor: None,
            flags: Flags::default(),
            images,
            image_data: Vec::new(),
            clear: false,
            switch_to: None,
            clipboard: None,
            notice: None,
            fullscreen: None,
        }
    }

    #[test]
    fn places_visible_placement_with_cup_and_place_ops() {
        let msg = frame_with(vec![ImagePlacement {
            id: 7,
            rect: Rect::new(2, 3, 4, 2),
            cols: 4,
            rows: 2,
            visible: true,
            src: None,
        }]);
        let mut transmitted = std::collections::HashSet::new();
        let mut active = std::collections::HashMap::new();
        let g = reconcile_images(&msg, &mut transmitted, &mut active);
        assert!(g.contains("\x1b[4;3H"), "CUP to 1-based (row 4, col 3): {g:?}");
        assert!(g.contains("a=p"), "place op emitted: {g:?}");
        assert_eq!(active.len(), 1);
    }

    #[test]
    fn skips_placement_with_negative_origin() {
        // A CUP with a 0/negative parameter aborts the escape and the image
        // would land at the current cursor position — never emit one.
        let msg = frame_with(vec![ImagePlacement {
            id: 7,
            rect: Rect::new(-3, 5, 4, 2),
            cols: 4,
            rows: 2,
            visible: true,
            src: None,
        }]);
        let mut transmitted = std::collections::HashSet::new();
        let mut active = std::collections::HashMap::new();
        let g = reconcile_images(&msg, &mut transmitted, &mut active);
        assert!(!g.contains("a=p"), "no place op for an unaddressable rect: {g:?}");
        assert!(active.is_empty());
    }

    #[test]
    fn deletes_placement_that_disappeared() {
        let mut transmitted = std::collections::HashSet::new();
        let mut active = std::collections::HashMap::new();
        active.insert(place_key(2, 3), (7u64, 2, 3, 4u16, 2u16, None));
        let g = reconcile_images(&frame_with(Vec::new()), &mut transmitted, &mut active);
        assert!(g.contains("a=d,d=i,i=7"), "stale placement deleted: {g:?}");
        assert!(active.is_empty());
    }
}

