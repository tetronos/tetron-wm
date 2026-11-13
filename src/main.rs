//! tetron-wm entry point — a thin dispatcher over the daemon/client split.
//!
//! - `tetron-wm`            ensure the daemon is running, then attach a client.
//! - `tetron-wm attach`     attach to an already-running daemon.
//! - `tetron-wm --help`     list commands, config keys and env vars.
//! - `tetron-wm --daemon`   run the daemon (normally spawned automatically).
//! - `tetron-wm kill`       shut the daemon down (closing all windows).
//! - `tetron-wm reload`     restart the frontend only; apps keep running.
//! - `tetron-wm service …`  install|uninstall|status the per-user apphost service.
//! - `tetron-wm launch …`   open a new app window in the running desktop.
//! - `tetron-wm tile`       tile all windows into the configured grid.
//! - `tetron-wm theme <t>`  switch the theme.
//! - `tetron-wm msg '<j>'`  send a raw ClientMsg (the assistant's escape hatch).
//!
//! The daemon owns the windows and child processes and persists across client
//! detaches, so closing a client (or an SSH disconnect) leaves everything running.

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::time::Duration;
use tetron_wm::protocol::socket_path;

fn main() -> std::io::Result<()> {
    // Before any thread: apps in ~/.local/bin (opencode, uv tools) count as installed and start.
    tetron_wm::catalog::add_user_bin_to_path();
    // Busybox style: invoked as xclip / xsel / wl-copy / wl-paste (through the
    // shim symlinks), act as that tool against the tetron-wm clipboard.
    if let Some(name) = std::env::args().next().as_deref().and_then(|a| std::path::Path::new(a).file_name()).and_then(|n| n.to_str()) {
        if tetron_wm::clipcli::SHIMS.contains(&name) {
            let rest: Vec<String> = std::env::args().skip(1).collect();
            std::process::exit(tetron_wm::clipcli::run_shim(name, &rest));
        }
    }
    // Global `-s NAME` / `--session NAME` (before the subcommand) selects a named
    // session; without it `$TETRON_WM_SESSION`, else `default`.
    let mut args = std::env::args().skip(1).peekable();
    while matches!(args.peek().map(String::as_str), Some("-s" | "--session")) {
        args.next();
        if let Some(name) = parse_session_arg(args.next()) {
            tetron_wm::protocol::set_session(&name);
        }
    }
    let cmd = args.next();
    let rest: Vec<String> = args.collect();
    if cmd.is_some() && rest.iter().any(|a| a == "--help" || a == "-h") && cmd.as_deref() != Some("launch") {
        print!("{}", tetron_wm::cli_help::text());
        return Ok(());
    }
    match cmd.as_deref() {
        Some("--version" | "-V") => {
            println!("tetron-wm {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("--help" | "-h" | "help") => {
            print!("{}", tetron_wm::cli_help::text());
            Ok(())
        }
        Some("--daemon") => tetron_wm::daemon::run(),
        Some("--apphost") => tetron_wm::apphost::server::run(),
        Some("fullscreen-waiter") => {
            let vt = rest.iter().position(|a| a == "--vt").and_then(|i| rest.get(i + 1)).and_then(|v| v.parse().ok());
            tetron_wm::fullscreen::serve(vt.unwrap_or(tetron_wm::fullscreen::DEFAULT_VT))
        }
        Some("clip") => std::process::exit(tetron_wm::clipcli::run_clip(&rest)),
        Some("mcp") => tetron_wm::mcp::run(),
        Some("kill") => kill(),
        Some("kill-app") => kill_app(&rest),
        Some("ps") => ps(),
        Some("attach") => {
            let mut takeover = false;
            let mut it = rest.iter();
            while let Some(a) = it.next() {
                match a.as_str() {
                    "-d" | "--detach" => takeover = true,
                    "-s" | "--session" => {
                        if let Some(name) = parse_session_arg(it.next().cloned()) {
                            tetron_wm::protocol::set_session(&name);
                        }
                    }
                    other => {
                        eprintln!("tetron-wm attach: unknown option '{other}' (try: -d, -s NAME)");
                        std::process::exit(2);
                    }
                }
            }
            attach(false, takeover)
        }
        Some("ls") => ls(),
        Some("config") => match rest.first().map(String::as_str) {
            Some("init") => match tetron_wm::config::init() {
                Ok(tetron_wm::config::InitOutcome::Written(p)) => {
                    println!("tetron-wm: wrote the example config to {}", p.display());
                    Ok(())
                }
                Ok(tetron_wm::config::InitOutcome::Exists(p)) => {
                    println!("tetron-wm: {} already exists; left it unchanged", p.display());
                    Ok(())
                }
                Err(e) => {
                    eprintln!("tetron-wm config init: could not write the config: {e}");
                    std::process::exit(1);
                }
            },
            Some("example") => {
                print!("{}", tetron_wm::config::EXAMPLE);
                Ok(())
            }
            _ => {
                eprintln!("usage: tetron-wm config init | example");
                std::process::exit(2);
            }
        },
        Some("reload") => reload(),
        Some("service") => match rest.first().map(String::as_str) {
            Some("install") => tetron_wm::service::install(),
            Some("uninstall") => tetron_wm::service::uninstall(),
            Some("status") | None => tetron_wm::service::status(),
            Some(other) => {
                eprintln!("tetron-wm service: unknown '{other}' (try: install, uninstall, status)");
                Ok(())
            }
        },
        Some("launch") => {
            let mut rest = rest.iter().cloned();
            let Some(command) = rest.next() else {
                eprintln!("usage: tetron-wm launch <command> [args…]");
                return Ok(());
            };
            let args: Vec<String> = rest.collect();
            let name = command.rsplit('/').next().unwrap_or(&command).to_string();
            ctl(&tetron_wm::session::ClientMsg::Launch { name, command, args })
        }
        Some("screenshot") => {
            let path = match rest.first() {
                Some(p) => std::path::absolute(p)?,
                None => {
                    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                    std::env::current_dir()?.join(format!("tetron-wm-screenshot-{ts}.png"))
                }
            };
            println!("{}", path.display());
            ctl(&tetron_wm::session::ClientMsg::Screenshot(path.to_string_lossy().into_owned()))
        }
        Some("tile") => ctl(&tetron_wm::session::ClientMsg::TileAll),
        Some("screensaver") => screensaver_cmd(&rest),
        Some("theme") => match rest.first().map(String::as_str) {
            // List every theme name (built-ins + file themes).
            Some("--list") => {
                for n in tetron_wm::theme::preset_names() {
                    println!("{n}");
                }
                Ok(())
            }
            // Write a built-in palette out as an editable theme file to start from.
            Some("--dump") => {
                let name = rest.get(1).cloned().unwrap_or_else(|| "midnight".into());
                match tetron_wm::theme::dump_builtin(&name) {
                    Ok(p) => println!("wrote {}", p.display()),
                    Err(e) => eprintln!("tetron-wm theme --dump: {e}"),
                }
                Ok(())
            }
            Some(name) => ctl(&tetron_wm::session::ClientMsg::SetTheme(name.to_string())),
            None => {
                eprintln!(
                    "usage: tetron-wm theme <{}>\n       tetron-wm theme --list | --dump <name>",
                    tetron_wm::theme::preset_names().join("|")
                );
                Ok(())
            }
        },
        Some("msg") => match rest.first() {
            Some(json) => match serde_json::from_str::<tetron_wm::session::ClientMsg>(json) {
                Ok(msg) => ctl(&msg),
                Err(e) => {
                    eprintln!("tetron-wm msg: not a valid ClientMsg: {e}");
                    Ok(())
                }
            },
            None => {
                eprintln!("usage: tetron-wm msg '<ClientMsg JSON>'  e.g.  tetron-wm msg '\"MaximizeFocused\"'");
                Ok(())
            }
        },
        Some(other) => {
            eprintln!(
                "tetron-wm: unknown command '{other}' (try: attach, ls, screensaver, kill, kill-app, ps, reload, launch, tile, screenshot, theme, config, msg, mcp, service, --version, --help, --daemon)"
            );
            Ok(())
        }
        None => attach(true, false),
    }
}

/// `tetron-wm screensaver [on|off|toggle|list] [EFFECT]`: start or stop the
/// on-demand screensaver in the running session. With no mode it starts it.
/// Any key or mouse event in the attached client wakes it again.
fn screensaver_cmd(args: &[String]) -> std::io::Result<()> {
    use tetron_wm::screensaver::{make, SaverMode, EFFECTS};
    let mode = args.first().map(String::as_str).unwrap_or("on");
    if mode == "list" {
        for (name, desc) in EFFECTS {
            println!("{name:<8} {desc}");
        }
        return Ok(());
    }
    let mode = match mode {
        "on" | "start" => SaverMode::On,
        "off" | "stop" => SaverMode::Off,
        "toggle" => SaverMode::Toggle,
        other => {
            eprintln!("tetron-wm screensaver: unknown '{other}' (try: on, off, toggle, list)");
            std::process::exit(2);
        }
    };
    let effect = args.get(1).cloned();
    if let Some(name) = &effect {
        if make(name).is_none() {
            let names: Vec<&str> = EFFECTS.iter().map(|e| e.0).collect();
            eprintln!("tetron-wm screensaver: no effect '{name}' (have: {})", names.join(", "));
            std::process::exit(2);
        }
    }
    ctl(&tetron_wm::session::ClientMsg::Screensaver { mode, effect })
}

/// Send one control message to the running daemon (used by `tetron-wm launch/tile/
/// theme/msg` — and by the desktop assistant to drive the UI).
fn ctl(msg: &tetron_wm::session::ClientMsg) -> std::io::Result<()> {
    if send_control(msg)? {
        println!("tetron-wm: sent");
    } else {
        eprintln!("tetron-wm: no daemon running (start it with `tetron-wm`)");
    }
    Ok(())
}

/// Connect to the daemon and run a client. If `spawn_if_missing`, start a daemon
/// first when none is running. On a `ClientExit::Reload`, waits for the old
/// daemon socket to drop, then loops to spawn/connect a fresh daemon.
/// `takeover` (`attach -d`) evicts any client already attached to the session.
fn attach(spawn_if_missing: bool, takeover: bool) -> std::io::Result<()> {
    loop {
        let path = socket_path();
        if UnixStream::connect(&path).is_err() {
            if !spawn_if_missing {
                eprintln!(
                    "tetron-wm: no daemon running in session '{}' (start it with `tetron-wm`)",
                    tetron_wm::protocol::session_name()
                );
                return Ok(());
            }
            spawn_daemon()?;
            let mut ready = false;
            for _ in 0..100 {
                if UnixStream::connect(&path).is_ok() { ready = true; break; }
                std::thread::sleep(Duration::from_millis(50));
            }
            if !ready {
                eprintln!("tetron-wm: daemon failed to start");
                return Ok(());
            }
        }
        let stream = UnixStream::connect(&path)?;
        match tetron_wm::client::run(stream, takeover)? {
            tetron_wm::client::ClientExit::Detached => return Ok(()),
            tetron_wm::client::ClientExit::Switch(spec) => {
                // Run ssh (and any first-time setup) in the real terminal; when
                // the remote session ends, loop to re-attach to the local
                // daemon — its apps kept running the whole time.
                run_switch(&spec);
                continue;
            }
            tetron_wm::client::ClientExit::Fullscreen(argv) => {
                // fbi on the real console; its own keys (q quits) until it
                // exits, then re-attach: the apps kept running.
                run_fullscreen(&argv);
                continue;
            }
            tetron_wm::client::ClientExit::Reload => {
                // The daemon is restarting; wait briefly for the old socket to
                // drop, then loop to spawn/connect the fresh daemon.
                tetron_wm::dbg_log("client: daemon reload — waiting for old socket to drop, then respawning");
                for _ in 0..100 {
                    if UnixStream::connect(socket_path()).is_err() { break; }
                    std::thread::sleep(Duration::from_millis(20));
                }
                continue;
            }
        }
    }
}

/// Run a full-screen program on the real console with inherited stdio, logging
/// how it ended. fbi exits 0 even when it showed nothing, so its status is
/// only informative.
fn run_fullscreen(argv: &[String]) {
    let Some((program, args)) = argv.split_first() else { return };
    // kmscon: ask the waiter on the spare VT (it switches there and back).
    if program == "@waiter" {
        tetron_wm::fullscreen::service_run(args.first().is_some_and(|a| a == "on"));
        return;
    }
    if program == "@fullscreen" {
        if let Some(kind) = args.first().and_then(|k| tetron_wm::fullscreen::Kind::parse(k)) {
            tetron_wm::fullscreen::show(kind, &args[1..]);
        }
        return;
    }
    tetron_wm::dbg_log(&format!("fullscreen: running {argv:?}"));
    match std::process::Command::new(program).args(args).status() {
        Ok(status) => tetron_wm::dbg_log(&format!("fullscreen: {program} ended with {status}")),
        Err(e) => {
            tetron_wm::log_warn(&format!("fullscreen: could not run {program}: {e}"));
            eprintln!("tetron-wm: could not run {program}: {e}");
        }
    }
}

/// Switch to a remote system: run the generated ssh/setup script with inherited
/// stdio so password and host-key prompts are fully interactive. The setup
/// password (if any) is passed via the `SSHPASS` env var — never on a command
/// line and never written anywhere.
fn run_switch(spec: &tetron_wm::systems::SwitchSpec) {
    println!(
        "tetron-wm: switching to {} ({}{}){}…",
        spec.name,
        spec.host,
        spec.port.map(|p| format!(":{p}")).unwrap_or_default(),
        if spec.setup { " — first-time setup" } else { "" },
    );
    let script = tetron_wm::systems::switch_script(spec);
    // With TETRON_WM_DEBUG set, show exactly what will run (the password is never
    // embedded in the script) and mirror it to the log.
    if std::env::var_os("TETRON_WM_DEBUG").is_some() {
        eprintln!("tetron-wm: switch script:\n{script}");
    }
    tetron_wm::dbg_log(&format!(
        "switch: name={} host={} port={:?} theme={:?} setup={} password={}",
        spec.name, spec.host, spec.port, spec.theme, spec.setup,
        if spec.password.is_some() { "yes (via SSHPASS)" } else { "no" },
    ));
    let mut cmd = std::process::Command::new("sh");
    cmd.arg("-c").arg(&script);
    if let Some(pw) = &spec.password {
        cmd.env("SSHPASS", pw);
    }
    match cmd.status() {
        Ok(status) if status.success() => {
            tetron_wm::dbg_log("switch: remote session ended cleanly");
            println!("tetron-wm: remote session ended — back to this machine.");
        }
        Ok(status) => {
            tetron_wm::dbg_log(&format!("switch: ended with {status}"));
            eprintln!("tetron-wm: switch to {} ended with {status} — back to this machine.", spec.name);
            eprintln!("tetron-wm: (re-run with TETRON_WM_DEBUG=1 to see the exact script; log: {})", tetron_wm::logfile::display_path());
        }
        Err(e) => {
            tetron_wm::log_warn(&format!("switch: could not run sh/ssh: {e}"));
            eprintln!("tetron-wm: could not run ssh: {e}");
        }
    }
    // Give the user a beat to read any setup/ssh output before tetron-wm's
    // alternate screen swallows it on re-attach.
    if spec.setup {
        println!("tetron-wm: re-attaching to the local session in 3s… (Ctrl-C to stay in the shell)");
        std::thread::sleep(Duration::from_secs(3));
    }
}

/// Spawn the daemon detached into its own process group so it survives the
/// client exiting (and SSH disconnects).
fn spawn_daemon() -> std::io::Result<()> {
    let exe = tetron_wm::self_exe()?;
    tetron_wm::dbg_log(&format!("daemon: spawning {} --daemon", exe.display()));
    std::process::Command::new(exe)
        .arg("--daemon")
        .env("TETRON_WM_SESSION", tetron_wm::protocol::session_name())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn()?;
    Ok(())
}

fn send_control(msg: &tetron_wm::session::ClientMsg) -> std::io::Result<bool> {
    tetron_wm::protocol::send_control(msg)
}

/// Validate the NAME of `-s NAME`; exits with status 2 on a missing or invalid one.
fn parse_session_arg(name: Option<String>) -> Option<String> {
    match name {
        Some(n) if tetron_wm::protocol::valid_session_name(&n) => Some(n),
        Some(n) => {
            eprintln!("tetron-wm: invalid session name '{n}' (1-32 chars of A-Z a-z 0-9 _ -)");
            std::process::exit(2);
        }
        None => {
            eprintln!("tetron-wm: -s needs a session name");
            std::process::exit(2);
        }
    }
}

/// What `tetron-wm ls` found for one session.
enum SessionState {
    /// Daemon answers: attached or not, with its window count (`None` for an
    /// older daemon that does not report status).
    Running(Option<tetron_wm::protocol::StatusReply>),
    /// Daemon is down but the apphost answers: apps are alive and the next
    /// `tetron-wm` restores their windows.
    Recoverable,
    /// Sockets left on disk, nothing answers (crash / reboot).
    Stale,
}

/// Probe session `name`. The control socket is used for the daemon (a bare
/// connect/close is ignored; the main socket would count as a poke).
fn probe_session(name: &str) -> SessionState {
    use std::io::BufRead;
    use tetron_wm::protocol::{session_dir, StatusReply};
    let dir = session_dir(name);
    if let Ok(mut s) = UnixStream::connect(dir.join("daemon-ctl.sock")) {
        let _ = s.set_read_timeout(Some(Duration::from_secs(1)));
        let mut buf = serde_json::to_vec(&tetron_wm::session::ClientMsg::Status).unwrap_or_default();
        buf.push(b'\n');
        let _ = s.write_all(&buf);
        let _ = s.shutdown(std::net::Shutdown::Write);
        let mut line = String::new();
        let reply = std::io::BufReader::new(s).read_line(&mut line).ok().filter(|n| *n > 0);
        return SessionState::Running(reply.and_then(|_| serde_json::from_str::<StatusReply>(line.trim()).ok()));
    }
    if UnixStream::connect(dir.join("apphost.sock")).is_ok() {
        return SessionState::Recoverable;
    }
    SessionState::Stale
}

/// `tetron-wm ls` — list sessions with their state and window count.
fn ls() -> std::io::Result<()> {
    let sessions = tetron_wm::protocol::list_sessions();
    if sessions.is_empty() {
        println!("(no sessions)");
        return Ok(());
    }
    println!("{:<32}  {:<11}  WINDOWS", "SESSION", "STATE");
    for name in sessions {
        let (state, windows) = match probe_session(&name) {
            SessionState::Running(Some(st)) => (if st.attached { "attached" } else { "detached" }, st.windows.to_string()),
            SessionState::Running(None) => ("running", "-".into()),
            SessionState::Recoverable => ("recoverable", "-".into()),
            SessionState::Stale => ("stale", "-".into()),
        };
        println!("{name:<32}  {state:<11}  {windows}");
    }
    Ok(())
}

/// Tell a running daemon to reload its frontend (apps keep running via the
/// apphost). An attached client reconnects on its own.
fn reload() -> std::io::Result<()> {
    if send_control(&tetron_wm::session::ClientMsg::Reload)? {
        println!("tetron-wm: reload requested");
    } else {
        println!("tetron-wm: no daemon running");
    }
    Ok(())
}

/// Tell a running daemon to shut down, and stop the apphost.
fn kill() -> std::io::Result<()> {
    if send_control(&tetron_wm::session::ClientMsg::Shutdown)? {
        println!("tetron-wm: shutdown requested");
    } else {
        println!("tetron-wm: no daemon running");
    }
    // Also stop the apphost directly. When a daemon is attached it shuts the
    // apphost down in-band; this covers the case where the apphost is running
    // with no daemon (e.g. the per-user service apphost on its own).
    if let Ok(mut s) = UnixStream::connect(tetron_wm::protocol::apphost_socket_path()) {
        let req = tetron_wm::apphost::proto::HostReq::Shutdown;
        if let Ok(mut buf) = serde_json::to_vec(&req) {
            buf.push(b'\n');
            let _ = tetron_wm::protocol::send_and_drain(&mut s, &buf);
        }
    }
    Ok(())
}

/// Format a `secs` duration as a short human-readable age ("12s", "3m", "1h12m",
/// "2d04h"). Used by `tetron-wm ps` and the in-app activity monitor.
fn format_age(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 60 * 60 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else if secs < 24 * 60 * 60 {
        format!("{}h{:02}m", secs / 3600, (secs / 60) % 60)
    } else {
        format!("{}d{:02}h", secs / 86400, (secs / 3600) % 24)
    }
}

fn format_cmdline(cmd: &str, args: &[String]) -> String {
    if args.is_empty() {
        cmd.to_string()
    } else {
        format!("{} {}", cmd, args.join(" "))
    }
}

/// Connect to the apphost, send one `ListApps` request, and return the
/// resulting row set.
///
/// Drains the `Roster` that the apphost sends on every connection (before it
/// will answer commands), inspects the apphost's declared protocol version,
/// and drains any non-AppList events that arrive between the request and
/// reply. Bounded to a small event count so a pre-v2 apphost (which silently
/// ignores `ListApps`) cannot hang the CLI indefinitely.
fn fetch_app_list() -> std::io::Result<Vec<tetron_wm::apphost::AppListEntry>> {
    use tetron_wm::apphost::proto::{send, HostEvt, HostReq};
    // The on-wire variant that introduced ListApps. Kept inline (instead of
    // imported from `PROTO_VERSION`) so the CLI's behavior against a pre-v2
    // apphost is independent of the running binary's PROTO_VERSION (which
    // changes as the apphost grows new fields).
    const LIST_APPS_MIN_PROTO: u32 = 2;
    // Preferred: the control socket, answered even while the daemon holds the
    // main socket (the apphost serves one frontend connection at a time).
    if let Ok(s) = UnixStream::connect(tetron_wm::protocol::apphost_ctl_socket_path()) {
        s.set_read_timeout(Some(Duration::from_secs(6)))?;
        send(&mut s.try_clone()?, &HostReq::ListApps)?;
        let mut r = std::io::BufReader::new(s);
        return match tetron_wm::apphost::proto::recv::<HostEvt, _>(&mut r) {
            Ok(Some(HostEvt::AppList { apps })) => Ok(apps),
            other => {
                eprintln!("tetron-wm: apphost control socket did not answer ({other:?})");
                std::process::exit(1);
            }
        };
    }
    // Fallback: an apphost without the control socket only answers here when
    // no daemon is connected, so time out instead of waiting in its backlog.
    let path = tetron_wm::protocol::apphost_socket_path();
    let s = UnixStream::connect(&path).unwrap_or_else(|_| {
        eprintln!("tetron-wm: no apphost running (start it with `tetron-wm`)");
        std::process::exit(1);
    });
    s.set_read_timeout(Some(Duration::from_secs(3)))?;
    legacy_app_list(s, LIST_APPS_MIN_PROTO).inspect_err(|e| {
        if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) {
            eprintln!(
                "tetron-wm: the app server is busy serving the desktop and predates 'ps' support \
                 while attached; detach or restart the app server, then retry"
            );
            std::process::exit(1);
        }
    })
}

/// `ListApps` over the main apphost socket (apphosts without a control socket).
fn legacy_app_list(s: UnixStream, min_proto: u32) -> std::io::Result<Vec<tetron_wm::apphost::AppListEntry>> {
    use tetron_wm::apphost::proto::{send, HostEvt, HostReq};
    // The apphost sends a Roster on every accepted connection before it
    // processes any commands. Read it first so we can:
    //   1. Bail out cleanly against a pre-v2 apphost (one that doesn't
    //      understand ListApps and would otherwise hang the drain loop).
    //   2. Get an honest proto version to report.
    // A pre-v2 apphost that predates the `proto` field reports `0`.
    let mut r = std::io::BufReader::new(s.try_clone().unwrap());
    let proto: u32 = match tetron_wm::apphost::proto::recv::<HostEvt, _>(&mut r)? {
        Some(HostEvt::Roster { proto, .. }) => proto,
        Some(_) => {
            // First event isn't a Roster? Unexpected but the apphost might
            // still be a newer version that just reorders. Continue with
            // the request and see what happens.
            0
        }
        None => {
            eprintln!("tetron-wm: apphost closed before sending roster");
            std::process::exit(1);
        }
    };
    if proto < min_proto && proto != 0 {
        // A peer that explicitly declared proto=1 doesn't speak ListApps.
        // (proto=0 means the field is missing, i.e. the apphost predates
        // the `proto` field — likely the very first apphost release; we
        // still attempt the request and let the bound save us.)
        eprintln!(
            "tetron-wm: apphost speaks protocol v{proto}; 'ps' / 'kill-app' require v{min_proto}+ \
             (update the apphost binary to enable)"
        );
        std::process::exit(1);
    }
    send(&mut r.get_mut(), &HostReq::ListApps)?;
    // Bound the drain: a v3 apphost returns AppList within 1-2 non-AppList
    // events (Roster already consumed + a Frame or two). 64 is a generous
    // ceiling that still aborts in well under a second on a misbehaving peer.
    const MAX_DRAIN: usize = 64;
    for _ in 0..MAX_DRAIN {
        let evt: HostEvt = match tetron_wm::apphost::proto::recv(&mut r)? {
            Some(e) => e,
            None => {
                eprintln!("tetron-wm: apphost closed before replying");
                std::process::exit(1);
            }
        };
        if let HostEvt::AppList { apps } = evt {
            return Ok(apps);
        }
        // Drop any other event (Frame events for live apps, Gone for exited
        // ones) and keep reading until the AppList arrives.
    }
    eprintln!(
        "tetron-wm: apphost did not reply with an AppList after {MAX_DRAIN} events \
         (pre-v2 apphost? update the apphost binary to enable 'ps' / 'kill-app')"
    );
    std::process::exit(1);
}

fn ps() -> std::io::Result<()> {
    let apps = fetch_app_list()?;
    if apps.is_empty() {
        println!("(no apps running)");
        return Ok(());
    }
    println!(
        "{:>5}  {:>7}  {:<32}  {:>10}  {:>7}  STATE",
        "APPID", "PID", "CMD", "COLSxROWS", "AGE"
    );
    for a in &apps {
        let pid = a.pid.map(|p| p.to_string()).unwrap_or_else(|| "—".into());
        let cmdline = format_cmdline(&a.cmd, &a.args);
        let cmdline = if cmdline.chars().count() > 60 {
            let mut s: String = cmdline.chars().take(59).collect();
            s.push('…');
            s
        } else {
            cmdline
        };
        let state = if a.alive { "alive" } else { "dead" };
        println!(
            "{:>5}  {:>7}  {:<32}  {:>4}x{:<4}  {:>7}  {}",
            a.app,
            pid,
            cmdline,
            a.cols,
            a.rows,
            format_age(a.age_secs),
            state,
        );
    }
    Ok(())
}

/// `tetron-wm kill-app <id|all>` — send `HostReq::Kill` for one (or all dead)
/// hosted apps. `<id>` may be the apphost's numeric AppId. `all` is a safe
/// cleanup target: it kills only apps in the `dead` state (already exited);
/// live apps need an explicit id. Errors clearly when the apphost isn't
/// running.
fn kill_app(args: &[String]) -> std::io::Result<()> {
    use tetron_wm::apphost::proto::{send, HostReq};
    let target = match args.first().map(String::as_str) {
        Some(t) => t,
        None => {
            eprintln!("usage: tetron-wm kill-app <id|all>");
            std::process::exit(2);
        }
    };
    let apps = fetch_app_list()?;
    let to_kill: Vec<u64> = if target == "all" {
        // `all` is a safe-by-default cleanup: it only reaps apps that have
        // already exited (the apphost still holds the bookkeeping entry until
        // `HostReq::Kill` removes it). Killing a live app needs its id, so a
        // user can't blow away their session with a single typo.
        apps.iter().filter(|a| !a.alive).map(|a| a.app).collect()
    } else {
        let id: u64 = match target.parse() {
            Ok(n) => n,
            Err(_) => {
                eprintln!("tetron-wm kill-app: '{target}' is not a numeric id or 'all'");
                std::process::exit(2);
            }
        };
        if !apps.iter().any(|a| a.app == id) {
            let known: Vec<String> = apps.iter().map(|a| a.app.to_string()).collect();
            eprintln!(
                "tetron-wm kill-app: no such app (have: {})",
                if known.is_empty() { "(none)".into() } else { known.join(", ") }
            );
            std::process::exit(1);
        }
        vec![id]
    };
    if to_kill.is_empty() {
        println!("tetron-wm kill-app: no dead apps to reap");
        return Ok(());
    }
    // Reconnect for each Kill — the previous connection's reader consumed
    // the ListApps reply (and the side-reader can't share a socket safely),
    // so the cleanest path is one short-lived connection per Kill. The control
    // socket is answered while a daemon is attached; the main one is not.
    let ctl_path = tetron_wm::protocol::apphost_ctl_socket_path();
    let path = if ctl_path.exists() { ctl_path } else { tetron_wm::protocol::apphost_socket_path() };
    for id in &to_kill {
        let mut s = match UnixStream::connect(&path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("tetron-wm kill-app: connect failed: {e}");
                std::process::exit(1);
            }
        };
        send(&mut s, &HostReq::Kill { app: *id })?;
    }
    if to_kill.len() == 1 {
        println!("tetron-wm kill-app: sent kill to app {}", to_kill[0]);
    } else {
        println!("tetron-wm kill-app: sent kill to {} app(s)", to_kill.len());
    }
    Ok(())
}
