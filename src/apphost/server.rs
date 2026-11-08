//! The apphost process: owns the live apps behind a socket, pushing per-app
//! frames and accepting commands from the frontend. Started as `tetron-wm --apphost`
//! (normally spawned automatically by the frontend daemon). The apps it owns
//! survive a frontend restart because this process keeps running.

use crate::apphost::proto::{recv, send, AppListEntry, HostEvt, HostReq, ImgBlob, RosterEntry};
use crate::apphost::{AppHost, AppId, LocalAppHost};
use crate::protocol::apphost_socket_path;
use std::collections::{HashMap, HashSet};
use std::fs::Permissions;
use std::io::BufReader;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::mpsc;
use std::time::Duration;

/// Run the apphost event loop until a frontend sends `Shutdown`.
pub fn run() -> std::io::Result<()> {
    // The apphost is a separate process with its own theme global, read only by
    // `ptyhost` for hosted-app terminal cell colors. Initialize it from the same
    // sources the client/daemon use (TETRON_WM_THEME env, else config.theme) so a
    // light desktop gets black-on-white terminals, then apply the independent
    // terminal_bg/terminal_fg overrides. Read at startup; later changes arrive
    // as `HostReq::SetColors` (protocol v4+), no restart needed.
    {
        let cfg = crate::config::Config::load();
        crate::logfile::set_level(cfg.log_level);
        let name = std::env::var("TETRON_WM_THEME")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| cfg.theme.clone());
        crate::theme::set_with_terminal(&name, cfg.terminal_bg.as_deref(), cfg.terminal_fg.as_deref());
    }

    let path = apphost_socket_path();
    // If another apphost is already listening (e.g. the service started it, or the
    // daemon spawned it), exit cleanly rather than rebinding the socket. Combined
    // with "restart on failure only", a redundant service start won't loop.
    if UnixStream::connect(&path).is_ok() {
        crate::dbg_log("apphost: another instance already running; exiting");
        return Ok(());
    }
    crate::protocol::ensure_socket_dir()?;
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, Permissions::from_mode(0o600))?;
    crate::dbg_log("apphost: listening");

    let mut local = LocalAppHost::new();
    let mut shutdown = false;

    // Frontends are served one at a time, so `ps` / `kill-app` use a separate
    // control socket answered from inside the serve loop (and while idle).
    let ctl_path = crate::protocol::apphost_ctl_socket_path();
    let (ctl_tx, ctl_rx) = mpsc::channel::<CtlReq>();
    let _ = std::fs::remove_file(&ctl_path);
    match UnixListener::bind(&ctl_path) {
        Ok(l) => {
            let _ = std::fs::set_permissions(&ctl_path, Permissions::from_mode(0o600));
            std::thread::spawn(move || serve_ctl(l, ctl_tx));
        }
        Err(e) => crate::dbg_log(&format!("apphost: control socket {}: {e}", ctl_path.display())),
    }

    let (fe_tx, fe_rx) = mpsc::channel::<UnixStream>();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            if fe_tx.send(stream).is_err() {
                break;
            }
        }
    });

    // Apps killed over the control socket while no frontend is attached. A
    // frontend's loop drops dead apps itself (and sends `Gone`); with none
    // attached, drop them here once they exit, so `kill-app` clears them.
    let mut reap: HashSet<AppId> = HashSet::new();
    loop {
        reap.extend(answer_ctl(&mut local, &ctl_rx));
        reap.retain(|&id| {
            if local.is_alive(id) {
                return true;
            }
            local.remove(id);
            false
        });
        match fe_rx.recv_timeout(Duration::from_millis(50)) {
            Ok(stream) => {
                serve_frontend(&mut local, stream, &mut shutdown, &ctl_rx);
                if shutdown {
                    break;
                }
                crate::dbg_log("apphost: frontend detached; apps stay alive");
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    // Full shutdown: kill every app, then remove the sockets.
    for id in local.list() {
        local.kill(id);
    }
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&ctl_path);
    crate::dbg_log("apphost: shut down");
    Ok(())
}

/// Minimum time between frame passes.
const FRAME: Duration = Duration::from_millis(16);
/// Longest idle wait, so exited apps are still noticed without output.
const IDLE: Duration = Duration::from_millis(100);

/// A control-socket request and where its reply goes.
type CtlReq = (HostReq, mpsc::Sender<HostEvt>);

/// Accept control connections: read one request, hand it to the main loop,
/// write back its reply. A Kill gets no reply; the CLI does not wait for one.
fn serve_ctl(listener: UnixListener, tx: mpsc::Sender<CtlReq>) {
    for stream in listener.incoming().flatten() {
        let Ok(mut writer) = stream.try_clone() else { continue };
        let mut r = BufReader::new(stream);
        let Ok(Some(req)) = recv::<HostReq, _>(&mut r) else { continue };
        let (rtx, rrx) = mpsc::channel();
        if tx.send((req, rtx)).is_err() {
            return;
        }
        crate::ptyhost::notify_output();
        if let Ok(evt) = rrx.recv_timeout(Duration::from_secs(5)) {
            let _ = send(&mut writer, &evt);
        }
    }
}

/// Answer pending control requests and return the apps killed. Only `ListApps`
/// and `Kill` are accepted; anything else (spawns, input) belongs on a
/// frontend connection.
fn answer_ctl(local: &mut LocalAppHost, rx: &mpsc::Receiver<CtlReq>) -> Vec<AppId> {
    let mut killed = Vec::new();
    while let Ok((req, reply)) = rx.try_recv() {
        match req {
            HostReq::ListApps => {
                let _ = reply.send(HostEvt::AppList { apps: app_list(local) });
            }
            HostReq::Kill { app } => {
                crate::dbg_log(&format!("apphost: ctl kill app {app}"));
                local.kill(AppId(app));
                killed.push(AppId(app));
            }
            other => crate::dbg_log(&format!("apphost: ctl ignored {other:?}")),
        }
    }
    killed
}

/// Every hosted app, for `ps` / `kill-app`.
fn app_list(local: &mut LocalAppHost) -> Vec<AppListEntry> {
    let now = std::time::Instant::now();
    let mut apps = Vec::new();
    for id in local.list() {
        let (cmd, args) = local
            .launch_cmd(id)
            .map(|(c, a)| (c.to_string(), a.to_vec()))
            .unwrap_or_else(|| (String::new(), Vec::new()));
        let (cols, rows) = local.dims(id).unwrap_or((0, 0));
        let age_secs = local
            .spawn_time(id)
            .map(|t| now.duration_since(t).as_secs())
            .unwrap_or(0);
        apps.push(AppListEntry {
            app: id.0,
            cmd,
            args,
            pid: local.pid(id),
            cols,
            rows,
            age_secs,
            alive: local.is_alive(id),
        });
    }
    apps
}

/// Serve a single connected frontend. Per-connection change state is fresh so
/// the frontend always gets full grids on (re)connect.
fn serve_frontend(local: &mut LocalAppHost, stream: UnixStream, shutdown: &mut bool, ctl: &mpsc::Receiver<CtlReq>) {
    let Ok(reader_stream) = stream.try_clone() else { return };
    let mut writer = stream;

    // Frontend → channel on a reader thread.
    let (tx, rx) = mpsc::channel::<HostReq>();
    std::thread::spawn(move || {
        let mut r = BufReader::new(reader_stream);
        while let Ok(Some(req)) = recv::<HostReq, _>(&mut r) {
            if tx.send(req).is_err() {
                break;
            }
            crate::ptyhost::notify_output();
        }
        crate::ptyhost::notify_output();
    });

    // Roster so the frontend can rebuild its windows.
    let roster = HostEvt::Roster {
        apps: local
            .list()
            .into_iter()
            .map(|id| RosterEntry { app: id.0, meta: local.meta(id).unwrap_or_default() })
            .collect(),
        proto: crate::apphost::proto::PROTO_VERSION,
    };
    // Best-effort: a short-lived connection (e.g. `tetron-wm kill` sending Shutdown)
    // may close before we finish writing the roster. Don't abandon the
    // connection on that — we still want to read any pending command (Shutdown).
    let _ = send(&mut writer, &roster);

    // Per-connection change tracking.
    let mut last_grid: HashMap<AppId, crate::buffer::CellBuffer> = HashMap::new();
    let mut last_placements: HashMap<AppId, Vec<crate::kittygfx::Placement>> = HashMap::new();
    let mut last_mouse: HashMap<AppId, crate::mouse::AppMouse> = HashMap::new();
    let mut last_cursor: HashMap<AppId, Option<(u16, u16)>> = HashMap::new();
    let mut sent_images: HashMap<AppId, HashSet<u32>> = HashMap::new();
    // The frontend announced v5 (`HostReq::Hello`): send packed grids.
    let mut packed_ok = false;

    loop {
        // Drain commands.
        loop {
            match rx.try_recv() {
                Ok(HostReq::Spawn { req_id, cmd, args, cwd, cols, rows }) => {
                    let cwd_path = cwd.as_deref().map(std::path::Path::new);
                    let evt = match local.spawn(&cmd, &args, cwd_path, cols, rows) {
                        Ok(id) => HostEvt::Spawned { req_id, app: id.0, pid: local.pid(id) },
                        Err(e) => HostEvt::SpawnFailed { req_id, error: e.to_string() },
                    };
                    if send(&mut writer, &evt).is_err() {
                        return;
                    }
                }
                Ok(HostReq::Input { app, bytes }) => local.input(AppId(app), &bytes),
                Ok(HostReq::Resize { app, cols, rows }) => local.resize(AppId(app), cols, rows),
                Ok(HostReq::Scroll { app, lines }) => local.scroll(AppId(app), lines),
                Ok(HostReq::SetMeta { app, meta }) => local.set_meta(AppId(app), meta),
                Ok(HostReq::Kill { app }) => local.kill(AppId(app)),
                Ok(HostReq::ListApps) => {
                    if send(&mut writer, &HostEvt::AppList { apps: app_list(local) }).is_err() {
                        return;
                    }
                }
                Ok(HostReq::SetColors { theme, terminal_bg, terminal_fg, ansi }) => {
                    crate::dbg_log(&format!("apphost: colors -> theme '{theme}' bg {terminal_bg:?} fg {terminal_fg:?}"));
                    crate::theme::set_with_terminal(&theme, terminal_bg.as_deref(), terminal_fg.as_deref());
                    if let Some(ansi) = &ansi {
                        crate::theme::set_ansi(ansi);
                    }
                    // The new colours change every snapshot.
                    local.mark_all_dirty();
                }
                Ok(HostReq::Hello { proto }) => packed_ok = proto >= 5,
                Ok(HostReq::Shutdown) => {
                    *shutdown = true;
                    return;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return, // frontend gone
            }
        }
        // Killed apps are dropped by the liveness check below.
        answer_ctl(local, ctl);

        // Push frames for every app whose grid / placements / images changed.
        let mut busy = false;
        for id in local.list() {
            let alive = local.is_alive(id);
            if !alive {
                let _ = send(&mut writer, &HostEvt::Gone { app: id.0 });
                local.remove(id);
                last_grid.remove(&id);
                last_placements.remove(&id);
                last_mouse.remove(&id);
                last_cursor.remove(&id);
                sent_images.remove(&id);
                continue;
            }
            // Only apps with new output (or a resize/scroll/theme change) can
            // have a different frame; skip the snapshot and compare otherwise.
            if !local.take_dirty(id) && last_grid.contains_key(&id) {
                continue;
            }
            busy = true;
            let Some(grid) = local.snapshot(id) else { continue };
            let placements = local.placements(id);
            let mouse = local.mouse_mode(id);
            let cursor = local.cursor(id);

            // New image blobs (not yet sent to this frontend).
            let seen = sent_images.entry(id).or_default();
            let mut images = Vec::new();
            for pl in &placements {
                if !seen.contains(&pl.image_id) {
                    if let Some(png) = local.image_png(id, pl.image_id) {
                        images.push(ImgBlob { image_id: pl.image_id, png_b64: crate::kitty::b64(&png) });
                        seen.insert(pl.image_id);
                    }
                }
            }

            let bells = local.take_bells(id);
            let clip = local.take_clipboard(id);
            let clip_reqs = local.take_clip_requests(id);
            let packed = if packed_ok { crate::apphost::proto::pack_grid(&grid, last_grid.get(&id)) } else { None };
            let grid_changed = if packed_ok { packed.is_some() } else { last_grid.get(&id) != Some(&grid) };
            let placements_changed = last_placements.get(&id) != Some(&placements);
            let mouse_changed = last_mouse.get(&id) != Some(&mouse);
            // A bare cursor move (e.g. arrow keys in a shell) changes no cells,
            // so it must be tracked separately or the caret would never update.
            let cursor_changed = last_cursor.get(&id) != Some(&cursor);
            if grid_changed || placements_changed || mouse_changed || cursor_changed || !images.is_empty() || bells > 0 || clip.is_some() || !clip_reqs.is_empty() {
                // A packed frame leaves `grid` empty; no `packed` then means "grid unchanged".
                let (wire_grid, kept) = if packed_ok { (crate::buffer::CellBuffer::new(0, 0), Some(grid)) } else { (grid, None) };
                let evt = HostEvt::Frame { app: id.0, grid: wire_grid, placements, images, alive: true, mouse, cursor, bells, clip, clip_reqs, packed };
                if send(&mut writer, &evt).is_err() {
                    return;
                }
                // Keep the sent grid as the baseline without cloning it.
                if let HostEvt::Frame { grid, placements, .. } = evt {
                    last_grid.insert(id, kept.unwrap_or(grid));
                    last_placements.insert(id, placements);
                }
                last_mouse.insert(id, mouse);
                last_cursor.insert(id, cursor);
            }
        }

        // At most one frame per app per FRAME (bursts like `yes` coalesce);
        // with nothing changing, sleep until output or a command arrives.
        std::thread::sleep(FRAME);
        if !busy {
            crate::ptyhost::wait_output(IDLE);
        }
    }
}

/// Test-only re-export of the per-frontend serve loop (used by remote.rs's
/// loopback test). Not part of the public API.
#[doc(hidden)]
pub fn server_serve_for_test(local: &mut LocalAppHost, stream: UnixStream, shutdown: &mut bool) {
    let (_tx, rx) = mpsc::channel();
    serve_frontend(local, stream, shutdown, &rx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctl_lists_and_kills_apps() {
        let mut local = LocalAppHost::new();
        let id = local.spawn("sleep", &["30".into()], None, 80, 24).unwrap();
        let (tx, rx) = mpsc::channel::<CtlReq>();
        let (rtx, rrx) = mpsc::channel();
        tx.send((HostReq::ListApps, rtx)).unwrap();
        answer_ctl(&mut local, &rx);
        match rrx.try_recv() {
            Ok(HostEvt::AppList { apps }) => {
                assert_eq!(apps.len(), 1);
                assert_eq!(apps[0].app, id.0);
                assert_eq!(apps[0].cmd, "sleep");
            }
            other => panic!("expected AppList, got {other:?}"),
        }
        let (rtx, rrx) = mpsc::channel();
        tx.send((HostReq::Kill { app: id.0 }, rtx)).unwrap();
        answer_ctl(&mut local, &rx);
        assert!(rrx.try_recv().is_err(), "Kill sends no reply");
        for _ in 0..100 {
            if !local.is_alive(id) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("app still alive after ctl Kill");
    }
}
