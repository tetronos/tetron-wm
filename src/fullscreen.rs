//! Full screen video under kmscon, with no root.
//!
//! kmscon holds DRM master on its VT and gives the client a pty, so a player
//! started from there can neither draw nor read keys. A *waiter* runs as the
//! user on a spare VT in its own logind session (a systemd unit the suite
//! installer sets up): the client sends it a file path, the waiter switches
//! the seat to its VT through logind (polkit allows an active session), runs
//! mpv there with the VT as its terminal, and switches back when mpv exits.
//! Nothing here needs root; the request socket is owner-only and the peer uid
//! is checked as well.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

static ENABLED: AtomicBool = AtomicBool::new(true);

/// Apply the `fullscreen` config switch (daemon side).
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

/// The packaged waiter unit (tetron-wm-desktop ships it in
/// /usr/lib/systemd/system), one instance per user: `User=%i`, VT 9.
pub const TEMPLATE: &str = "tetron-wm-fullscreen@.service";
/// The unit older suite installers wrote, with the user written in. Still
/// used where it exists, so a machine set up that way keeps working.
pub const LEGACY_UNIT: &str = "tetron-wm-fullscreen.service";

/// The template instance for `user`.
pub fn instance(user: &str) -> String {
    format!("tetron-wm-fullscreen@{user}.service")
}

/// The name of the user running this process (from the password database,
/// not `$USER`, which a `su` or a service may leave stale).
pub fn user_name() -> Option<String> {
    // SAFETY: getpwuid returns a pointer into static storage or null; the
    // name is copied out before any other passwd call can run here.
    unsafe {
        let pw = libc::getpwuid(libc::getuid());
        if pw.is_null() || (*pw).pw_name.is_null() {
            return None;
        }
        std::ffi::CStr::from_ptr((*pw).pw_name).to_str().ok().map(str::to_string)
    }
}

/// Where the waiter service stands (Settings shows and switches it).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Service {
    /// No unit installed (tetron-wm-desktop ships the template;
    /// `install-tetron-wm-suite.sh --fullscreen` sets it up without the deb).
    #[default]
    Missing,
    Stopped,
    Running,
}

/// Read `systemctl show -p LoadState -p ActiveState` output.
pub fn parse_service(show: &str) -> Service {
    let get = |k: &str| show.lines().find_map(|l| l.strip_prefix(k)).unwrap_or("");
    if get("LoadState=") != "loaded" {
        Service::Missing
    } else if get("ActiveState=") == "active" {
        Service::Running
    } else {
        Service::Stopped
    }
}

/// From `systemctl show` of the legacy unit then the instance (two blocks in
/// that order), the unit in use and its state: the legacy unit when it is
/// installed, otherwise the instance (`Missing` when no template is
/// installed either).
pub fn pick_unit(show: &str, inst: &str) -> (Service, String) {
    let mut blocks = show.split("\n\n");
    let legacy = parse_service(blocks.next().unwrap_or(""));
    if legacy != Service::Missing {
        return (legacy, LEGACY_UNIT.to_string());
    }
    (parse_service(blocks.next().unwrap_or("")), inst.to_string())
}

/// The waiter unit for this user and its state (a short, capped shell-out;
/// call it on open, not per frame).
pub fn service_unit() -> (Service, String) {
    let inst = instance(&user_name().unwrap_or_default());
    crate::system::run_capped("systemctl", &["show", "-p", "LoadState", "-p", "ActiveState", LEGACY_UNIT, &inst], 3)
        .map(|s| pick_unit(&s, &inst))
        .unwrap_or((Service::Missing, inst))
}

/// The state of [`service_unit`].
pub fn service_state() -> Service {
    service_unit().0
}

/// Client side: enable and start (`on`) or disable and stop the unit, with
/// sudo on the real terminal so the password prompt works. Waits for Enter so
/// the result can be read before the screen comes back.
pub fn service_run(on: bool) -> i32 {
    let verb = if on { "enable" } else { "disable" };
    // SAFETY: getuid never fails.
    let root = unsafe { libc::getuid() } == 0;
    let (program, mut args) = if root { ("systemctl", vec![]) } else { ("sudo", vec!["systemctl"]) };
    let unit = service_unit().1;
    args.extend([verb, "--now", unit.as_str()]);
    crate::dbg_log(&format!("fullscreen waiter service: {program} {args:?}"));
    let code = match std::process::Command::new(program).args(&args).status() {
        Ok(s) if s.success() => {
            println!("tetron-wm: full screen helper {}.", if on { "enabled and started" } else { "disabled and stopped" });
            0
        }
        Ok(s) => {
            eprintln!("tetron-wm: {program} {verb} failed ({s})");
            1
        }
        Err(e) => {
            eprintln!("tetron-wm: could not run {program}: {e}");
            1
        }
    };
    crate::dbg_log(&format!("fullscreen waiter service: {verb} ended with {code}"));
    print!("Press Enter to return to tetron-wm. ");
    let _ = std::io::stdout().flush();
    let _ = std::io::stdin().read_line(&mut String::new());
    code
}

/// The default spare VT (`tetron-wm fullscreen-waiter --vt N` overrides it).
pub const DEFAULT_VT: u32 = 9;

/// What to show. The waiter picks the program itself from this, so a request
/// can never name a command to run.
#[derive(Serialize, Deserialize, Debug, Default, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// One video, played with mpv.
    #[default]
    Video,
    /// Images, shown with fbi (or fim), first one first.
    Images,
    /// One PDF, shown with fbgs.
    Pdf,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Kind> {
        match s {
            "video" => Some(Kind::Video),
            "images" => Some(Kind::Images),
            "pdf" => Some(Kind::Pdf),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Kind::Video => "video",
            Kind::Images => "images",
            Kind::Pdf => "pdf",
        }
    }
}

/// The most files one request may carry (a folder of images).
const MAX_FILES: usize = 2000;

/// One request: what to show, the absolute files, and the VT to come back to.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub struct Request {
    #[serde(default)]
    pub kind: Kind,
    pub files: Vec<String>,
    pub return_vt: u32,
}

/// The owner-only socket the waiter listens on.
pub fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        // SAFETY: getuid never fails.
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::getuid() })));
    dir.join("tetron-wm-fullscreen.sock")
}

/// Whether a waiter answers on its socket (a bare connect, which the waiter
/// treats as a ping).
pub fn waiter_available() -> bool {
    UnixStream::connect(socket_path()).is_ok()
}

/// Check a requested file. Only an absolute path to a regular file is played,
/// returned with symlinks resolved, so it can never be read as an option and a
/// directory, device or fifo is refused.
pub fn validate(path: &str) -> Result<PathBuf, String> {
    if path.contains('\0') {
        return Err("path has a NUL byte".into());
    }
    let p = Path::new(path);
    if !p.is_absolute() {
        return Err("path is not absolute".into());
    }
    let real = std::fs::canonicalize(p).map_err(|e| format!("cannot open the file: {e}"))?;
    let meta = std::fs::metadata(&real).map_err(|e| format!("cannot read the file: {e}"))?;
    if !meta.is_file() {
        return Err("not a regular file".into());
    }
    Ok(real)
}

/// The mpv command line: no user config or scripts, no youtube-dl hook, no
/// following references in a playlist, and `--` so the file is never an option.
pub fn mpv_args(file: &Path) -> Vec<String> {
    let mut v: Vec<String> = [
        "--no-config", "--load-scripts=no", "--ytdl=no", "--access-references=no",
        "--vo=drm", "--fs", "--audio-fallback-to-null=yes", "--msg-level=all=error", "--",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    v.push(file.to_string_lossy().into_owned());
    v
}

/// Images with fbi (or fim): autozoom, no status line, `--` before the files.
fn fbi_args(program: &str, files: &[PathBuf]) -> Vec<String> {
    let mut v: Vec<String> = if program == "fim" { vec!["-a".into()] } else { vec!["-a".into(), "--noverbose".into()] };
    v.push("--".into());
    v.extend(files.iter().map(|p| p.to_string_lossy().into_owned()));
    v
}

/// mpv keys for browsing images, matching the built-in viewer (n / p, arrows,
/// + - 0 zoom, q or Esc to leave). Written next to the socket for `--input-conf`.
const IMAGE_KEYS: &str = "RIGHT playlist-next\nLEFT playlist-prev\nn playlist-next\np playlist-prev\nSPACE playlist-next\nBS playlist-prev\n+ add video-zoom 0.1\n- add video-zoom -0.1\n0 set video-zoom 0\nq quit\nESC quit\n";

/// Images with mpv over DRM, for a machine with no framebuffer device (fbi
/// needs `/dev/fb0`, which some GPU drivers, NVIDIA among them, do not make).
fn mpv_image_args(files: &[PathBuf], keys: &Path) -> Vec<String> {
    let mut v: Vec<String> = [
        "--no-config", "--load-scripts=no", "--ytdl=no", "--access-references=no", "--vo=drm", "--fs",
        "--msg-level=all=error", "--image-display-duration=inf", "--loop-playlist=inf",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    v.push(format!("--input-conf={}", keys.display()));
    v.push("--".into());
    v.extend(files.iter().map(|p| p.to_string_lossy().into_owned()));
    v
}

/// What the machine can drive, probed where the program will run. Fallbacks
/// follow from this and from which programs are installed, never from a fixed
/// guess: a GPU driver with no framebuffer (NVIDIA) still has DRM for mpv.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hw {
    /// A writable framebuffer device (`/dev/fb0`): fbi, fim, fbgs, `cvlc --vout fb`.
    pub fb: bool,
    /// A DRM card this user can open: `mpv --vo=drm`.
    pub drm: bool,
}

impl Hw {
    pub fn detect() -> Hw {
        use std::os::unix::ffi::OsStrExt;
        // SAFETY: a NUL-terminated path literal.
        let fb = unsafe { libc::access(c"/dev/fb0".as_ptr(), libc::W_OK) } == 0;
        let drm = std::fs::read_dir("/dev/dri")
            .map(|d| {
                d.flatten().any(|e| {
                    e.file_name().to_string_lossy().starts_with("card")
                        && std::ffi::CString::new(e.path().as_os_str().as_bytes())
                            // SAFETY: `c` is a valid NUL-terminated path.
                            .is_ok_and(|c| unsafe { libc::access(c.as_ptr(), libc::R_OK | libc::W_OK) } == 0)
                })
            })
            .unwrap_or(false);
        Hw { fb, drm }
    }
}

/// How to show a request on this machine.
#[derive(Debug, PartialEq, Eq)]
pub enum Plan {
    /// Run one program.
    Run { program: String, args: Vec<String> },
    /// Render a PDF's pages to images with pdftoppm, then page through them with mpv.
    PdfPages { pdf: PathBuf },
}

/// Pick the route for a request, best first, from the programs `have` says are
/// installed and what `hw` can drive. Files are already validated (absolute,
/// regular). `keys` is the mpv key file for image browsing.
///
/// - video: mpv over DRM, else cvlc on the framebuffer;
/// - images: fbi, else fim (framebuffer), else mpv over DRM;
/// - PDF: fbgs with gs (framebuffer), else pdftoppm pages through mpv over DRM.
///
/// `waiter`: the request runs on the spare VT while kmscon keeps the DRM
/// device open. Many GPUs then never scan out the framebuffer emulation, so
/// fbi and fbgs draw into a buffer nobody sees (the picture flashes only as
/// they quit); images and PDFs go through mpv over DRM first there.
pub fn plan(kind: Kind, files: &[PathBuf], have: &dyn Fn(&str) -> bool, hw: Hw, keys: &Path, waiter: bool) -> Result<Plan, String> {
    let run = |program: &str, args: Vec<String>| Ok(Plan::Run { program: program.into(), args });
    let first = files.first().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let drm_first = waiter && hw.drm && have("mpv");
    match kind {
        Kind::Images if !files.is_empty() && drm_first => run("mpv", mpv_image_args(files, keys)),
        Kind::Pdf if files.len() == 1 && drm_first && have("pdftoppm") => Ok(Plan::PdfPages { pdf: files[0].clone() }),
        Kind::Video if files.len() == 1 => {
            if hw.drm && have("mpv") {
                run("mpv", mpv_args(&files[0]))
            } else if hw.fb && have("cvlc") {
                run("cvlc", vec!["--play-and-exit".into(), "--vout".into(), "fb".into(), first])
            } else {
                Err("video needs mpv with a DRM display, or cvlc with a framebuffer (/dev/fb0)".into())
            }
        }
        Kind::Images if !files.is_empty() => {
            if hw.fb && have("fbi") {
                run("fbi", fbi_args("fbi", files))
            } else if hw.fb && have("fim") {
                run("fim", fbi_args("fim", files))
            } else if hw.drm && have("mpv") {
                run("mpv", mpv_image_args(files, keys))
            } else {
                Err("images need fbi or fim with a framebuffer (/dev/fb0), or mpv with a DRM display".into())
            }
        }
        // The path is absolute, so it can not be read as an option.
        Kind::Pdf if files.len() == 1 => {
            if hw.fb && have("fbgs") && have("gs") {
                run("fbgs", vec![first])
            } else if hw.drm && have("mpv") && have("pdftoppm") {
                Ok(Plan::PdfPages { pdf: files[0].clone() })
            } else {
                Err("PDFs need fbgs and gs with a framebuffer (/dev/fb0), or pdftoppm and mpv with a DRM display".into())
            }
        }
        _ => Err(format!("a {} request needs a different number of files", kind.name())),
    }
}

/// Whether any route exists for `kind` (what the menu checks).
pub fn possible(kind: Kind, have: &dyn Fn(&str) -> bool, hw: Hw) -> bool {
    plan(kind, &[PathBuf::from("/x")], have, hw, Path::new("/x"), false).is_ok()
}

/// Whether the attached client can show `kind` full screen: it is on a plain
/// console (direct) or a waiter answers (kmscon and others), and a route exists.
pub fn offered(kind: Kind, caps: &crate::display::ClientCaps, have: &dyn Fn(&str) -> bool) -> bool {
    ENABLED.load(Ordering::Relaxed) && (caps.vt_console || caps.fullscreen_waiter) && possible(kind, have, Hw { fb: caps.fb_device, drm: caps.drm_device })
}

/// Where the mpv key file for image browsing lives.
fn keys_path() -> PathBuf {
    socket_path().with_file_name("tetron-wm-fullscreen-keys.conf")
}

fn run_program(program: &str, args: &[String]) -> Result<(), String> {
    match std::process::Command::new(program).args(args).status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("{program} ended with {s}")),
        Err(e) => Err(format!("could not run {program}: {e}")),
    }
}

/// Run a plan with inherited stdio (the program owns the screen and keys).
fn run_plan(plan: &Plan, keys: &Path) -> Result<(), String> {
    let write_keys = || std::fs::write(keys, IMAGE_KEYS).map_err(|e| format!("cannot write the key bindings: {e}"));
    match plan {
        Plan::Run { program, args } => {
            if args.iter().any(|a| a.starts_with("--input-conf=")) {
                write_keys()?;
            }
            run_program(program, args)
        }
        Plan::PdfPages { pdf } => {
            write_keys()?;
            let dir = keys.with_file_name(format!("tetron-wm-pdf-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).map_err(|e| format!("cannot make a page folder: {e}"))?;
            let result = (|| {
                // 110 dpi, the first 200 pages: enough to read, quick to render.
                let root = dir.join("page");
                run_program("pdftoppm", &["-r".into(), "110".into(), "-l".into(), "200".into(), "-png".into(), pdf.to_string_lossy().into_owned(), root.to_string_lossy().into_owned()])?;
                let mut pages: Vec<PathBuf> = std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).collect();
                pages.sort();
                if pages.is_empty() {
                    return Err("pdftoppm made no pages".to_string());
                }
                run_program("mpv", &mpv_image_args(&pages, keys))
            })();
            let _ = std::fs::remove_dir_all(&dir);
            result
        }
    }
}

/// The VT that is showing now, from `/sys/class/tty/tty0/active` ("tty3").
pub fn active_vt() -> Option<u32> {
    let s = std::fs::read_to_string("/sys/class/tty/tty0/active").ok()?;
    s.trim().strip_prefix("tty")?.parse().ok()
}

/// Switch the seat to VT `n` through logind (no root: polkit allows an active
/// or inactive local session).
fn switch_vt(n: u32) -> Result<(), String> {
    let n = n.to_string();
    let args = [
        "--system", "call", "org.freedesktop.login1", "/org/freedesktop/login1/seat/seat0",
        "org.freedesktop.login1.Seat", "SwitchTo", "u", n.as_str(),
    ];
    match crate::system::run_capped_status("busctl", &args, 5) {
        crate::system::Capped::Done => Ok(()),
        crate::system::Capped::Failed(e) => Err(format!("switching to tty{n}: {e}")),
        crate::system::Capped::StillRunning => Err(format!("switching to tty{n} timed out")),
        crate::system::Capped::Missing(e) => Err(format!("busctl is not available: {e}")),
    }
}

/// Wait up to `secs` for VT `n` to be the active one.
fn wait_active(n: u32, secs: u64) -> bool {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    while std::time::Instant::now() < end {
        if active_vt() == Some(n) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    active_vt() == Some(n)
}

/// The uid on the other end of a unix socket (`SO_PEERCRED`).
#[cfg(any(target_os = "linux", target_os = "android"))]
fn peer_uid(s: &UnixStream) -> Option<u32> {
    use std::os::fd::AsRawFd;
    let mut cred = libc::ucred { pid: 0, uid: 0, gid: 0 };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `cred` and `len` are valid for SO_PEERCRED, which fills a ucred.
    let rc = unsafe { libc::getsockopt(s.as_raw_fd(), libc::SOL_SOCKET, libc::SO_PEERCRED, (&mut cred as *mut libc::ucred).cast(), &mut len) };
    (rc == 0).then_some(cred.uid)
}

/// The uid on the other end of a unix socket (`getpeereid`; macOS has no `SO_PEERCRED`).
#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn peer_uid(s: &UnixStream) -> Option<u32> {
    use std::os::fd::AsRawFd;
    let (mut uid, mut gid): (libc::uid_t, libc::gid_t) = (0, 0);
    // SAFETY: `uid` and `gid` are valid out-pointers for getpeereid.
    let rc = unsafe { libc::getpeereid(s.as_raw_fd(), &mut uid, &mut gid) };
    (rc == 0).then_some(uid)
}

/// Waiter: serve play requests on this VT until killed. One at a time.
pub fn serve(vt: u32) -> std::io::Result<()> {
    serve_on(&socket_path(), vt)
}

fn serve_on(sock: &Path, vt: u32) -> std::io::Result<()> {
    if UnixStream::connect(sock).is_ok() {
        eprintln!("tetron-wm fullscreen-waiter: one is already running ({})", sock.display());
        return Ok(());
    }
    let _ = std::fs::remove_file(sock);
    // SAFETY: umask only changes this process's file creation mask.
    let old = unsafe { libc::umask(0o177) };
    let bound = UnixListener::bind(sock);
    // SAFETY: restores the mask saved above.
    unsafe { libc::umask(old) };
    let listener = bound?;
    crate::dbg_log(&format!("fullscreen waiter: listening on {} for tty{vt}", sock.display()));
    // SAFETY: getuid never fails.
    let me = unsafe { libc::getuid() };
    for conn in listener.incoming() {
        let Ok(mut conn) = conn else { continue };
        if peer_uid(&conn) != Some(me) {
            crate::log_warn("fullscreen waiter: refused a connection from another user");
            continue;
        }
        let mut line = String::new();
        // 1 MiB holds a folder's worth of paths; a longer line is not a request.
        let read = BufReader::new((&conn).take(1 << 20)).read_line(&mut line);
        if read.map(|n| n == 0).unwrap_or(true) {
            continue; // a ping
        }
        let reply = match serde_json::from_str::<Request>(&line) {
            Err(e) => Err(format!("bad request: {e}")),
            Ok(req) => handle(vt, &req),
        };
        let text = match &reply {
            Ok(()) => "ok\n".to_string(),
            Err(e) => format!("error: {e}\n"),
        };
        if let Err(e) = &reply {
            crate::dbg_log(&format!("fullscreen waiter: {e}"));
        }
        let _ = conn.write_all(text.as_bytes());
    }
    Ok(())
}

/// Show one request: validate, plan from what this machine has, switch here,
/// run it, switch back.
fn handle(vt: u32, req: &Request) -> Result<(), String> {
    if req.files.is_empty() || req.files.len() > MAX_FILES {
        return Err(format!("a request needs 1 to {MAX_FILES} files"));
    }
    let files = req.files.iter().map(|f| validate(f)).collect::<Result<Vec<_>, _>>()?;
    let keys = keys_path();
    let hw = Hw::detect();
    let plan = plan(req.kind, &files, &crate::catalog::is_installed, hw, &keys, true)?;
    crate::dbg_log(&format!("fullscreen waiter: {} {} file(s), first {}, hw {hw:?}, plan {} (back to tty{})", req.kind.name(), files.len(), files[0].display(), plan_name(&plan), req.return_vt));
    switch_vt(vt)?;
    if !wait_active(vt, 3) {
        let _ = switch_vt(req.return_vt);
        return Err(format!("tty{vt} did not become active"));
    }
    // Let kmscon release the display before the program asks for it.
    std::thread::sleep(std::time::Duration::from_millis(250));
    // Clear the VT and hide its cursor so neither shows around the picture.
    print!("\x1b[2J\x1b[H\x1b[?25l");
    let _ = std::io::stdout().flush();
    let result = run_plan(&plan, &keys);
    print!("\x1b[?25h");
    let _ = std::io::stdout().flush();
    crate::dbg_log(&format!("fullscreen waiter: {} ended with {result:?}", plan_name(&plan)));
    switch_vt(req.return_vt)?;
    wait_active(req.return_vt, 3);
    result
}

fn plan_name(plan: &Plan) -> &str {
    match plan {
        Plan::Run { program, .. } => program,
        Plan::PdfPages { .. } => "pdftoppm+mpv",
    }
}

/// Show `files` full screen from the client: straight on a plain console (the
/// program takes the screen), through the waiter anywhere else (kmscon).
/// Returns the process exit code for `run_fullscreen`.
pub fn show(kind: Kind, files: &[String]) -> i32 {
    if crate::terminal::on_vt() {
        direct(kind, files)
    } else {
        play(kind, files)
    }
}

fn direct(kind: Kind, files: &[String]) -> i32 {
    let fail = |msg: &str| {
        crate::dbg_log(&format!("fullscreen: {msg}"));
        eprintln!("tetron-wm: {msg}");
        1
    };
    let mut abs = Vec::with_capacity(files.len());
    for f in files {
        match std::path::absolute(f).map_err(|e| e.to_string()).and_then(|p| validate(&p.to_string_lossy())) {
            Ok(p) => abs.push(p),
            Err(e) => return fail(&format!("{f}: {e}")),
        }
    }
    let keys = keys_path();
    let hw = Hw::detect();
    match plan(kind, &abs, &crate::catalog::is_installed, hw, &keys, false) {
        Err(e) => fail(&e),
        Ok(plan) => {
            crate::dbg_log(&format!("fullscreen: {} {} file(s), hw {hw:?}, plan {}", kind.name(), abs.len(), plan_name(&plan)));
            match run_plan(&plan, &keys) {
                Ok(()) => 0,
                Err(e) => fail(&e),
            }
        }
    }
}

/// Client side: ask the waiter to show `files` and wait until it is done.
/// Returns the process exit code for `run_fullscreen`.
pub fn play(kind: Kind, files: &[String]) -> i32 {
    let fail = |msg: &str| {
        crate::dbg_log(&format!("fullscreen play: {msg}"));
        eprintln!("tetron-wm: {msg}");
        1
    };
    let Some(return_vt) = active_vt() else { return fail("cannot tell which VT is showing") };
    let mut abs = Vec::with_capacity(files.len());
    for f in files {
        match std::path::absolute(f) {
            Ok(p) => abs.push(p.to_string_lossy().into_owned()),
            Err(e) => return fail(&format!("bad path: {e}")),
        }
    }
    let mut conn = match UnixStream::connect(socket_path()) {
        Ok(c) => c,
        Err(e) => return fail(&format!("the full screen waiter is not running ({e})")),
    };
    let req = serde_json::to_string(&Request { kind, files: abs, return_vt }).unwrap_or_default();
    if conn.write_all(format!("{req}\n").as_bytes()).is_err() {
        return fail("could not send the request");
    }
    let mut reply = String::new();
    let _ = BufReader::new(&conn).read_line(&mut reply);
    match reply.trim() {
        "ok" => 0,
        "" => fail("the waiter closed the connection"),
        other => fail(other.strip_prefix("error: ").unwrap_or(other)),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn service_state_follows_systemctl_show() {
        assert_eq!(parse_service("LoadState=not-found\nActiveState=inactive\n"), Service::Missing);
        assert_eq!(parse_service(""), Service::Missing);
        assert_eq!(parse_service("LoadState=loaded\nActiveState=inactive\n"), Service::Stopped);
        assert_eq!(parse_service("LoadState=loaded\nActiveState=active\n"), Service::Running);
    }

    #[test]
    fn the_legacy_unit_wins_where_it_is_installed_else_the_instance() {
        let inst = instance("ann");
        assert_eq!(inst, "tetron-wm-fullscreen@ann.service");
        let legacy_on = "LoadState=loaded\nActiveState=active\n\nLoadState=loaded\nActiveState=inactive\n";
        assert_eq!(pick_unit(legacy_on, &inst), (Service::Running, LEGACY_UNIT.to_string()));
        let packaged = "LoadState=not-found\nActiveState=inactive\n\nLoadState=loaded\nActiveState=inactive\n";
        assert_eq!(pick_unit(packaged, &inst), (Service::Stopped, inst.clone()));
        let none = "LoadState=not-found\nActiveState=inactive\n\nLoadState=not-found\nActiveState=inactive\n";
        assert_eq!(pick_unit(none, &inst), (Service::Missing, inst.clone()));
        assert!(user_name().is_some_and(|n| !n.is_empty()));
    }

    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tetron-wm-fs-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn validate_accepts_only_absolute_regular_files() {
        let d = tmp("validate");
        let f = d.join("clip.mp4");
        std::fs::write(&f, b"x").unwrap();
        assert_eq!(validate(f.to_str().unwrap()).unwrap(), std::fs::canonicalize(&f).unwrap());
        assert!(validate("clip.mp4").is_err(), "relative");
        assert!(validate("-x").is_err(), "leading dash is relative");
        assert!(validate(d.to_str().unwrap()).is_err(), "directory");
        assert!(validate("/nonexistent/clip.mp4").is_err(), "missing");
        assert!(validate("/dev/null").is_err(), "device");
        assert!(validate("/tmp/a\0b").is_err(), "NUL");
    }

    #[test]
    fn validate_resolves_symlinks() {
        let d = tmp("symlink");
        let f = d.join("real.mp4");
        std::fs::write(&f, b"x").unwrap();
        let l = d.join("link.mp4");
        std::os::unix::fs::symlink(&f, &l).unwrap();
        assert_eq!(validate(l.to_str().unwrap()).unwrap(), std::fs::canonicalize(&f).unwrap());
    }

    #[test]
    fn mpv_gets_hardening_flags_and_the_file_after_double_dash() {
        let args = mpv_args(Path::new("/v/-weird.mp4"));
        assert_eq!(args.last().unwrap(), "/v/-weird.mp4");
        assert_eq!(args[args.len() - 2], "--");
        for flag in ["--no-config", "--load-scripts=no", "--ytdl=no", "--access-references=no", "--vo=drm"] {
            assert!(args.iter().any(|a| a == flag), "{flag}");
        }
    }

    #[test]
    fn waiter_answers_bad_requests_with_errors_and_ignores_pings() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp("serve");
        let sock = d.join("w.sock");
        let s2 = sock.clone();
        std::thread::spawn(move || {
            let _ = serve_on(&s2, 9);
        });
        for _ in 0..100 {
            if UnixStream::connect(&sock).is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(std::fs::metadata(&sock).unwrap().permissions().mode() & 0o777, 0o600, "owner only");
        let ask = |line: &str| {
            let mut c = UnixStream::connect(&sock).unwrap();
            c.write_all(line.as_bytes()).unwrap();
            let mut reply = String::new();
            BufReader::new(&c).read_line(&mut reply).unwrap();
            reply
        };
        assert!(ask("not json\n").starts_with("error: bad request"));
        assert!(ask("{\"files\":[\"relative.mp4\"],\"return_vt\":1}\n").starts_with("error: path is not absolute"));
        assert!(ask("{\"files\":[\"/nonexistent/x.mp4\"],\"return_vt\":1}\n").starts_with("error: cannot open"));
        assert!(ask("{\"files\":[],\"return_vt\":1}\n").starts_with("error: a request needs 1 to"));
        // A ping (connect and close) must not wedge the waiter.
        drop(UnixStream::connect(&sock).unwrap());
        assert!(ask("{\"kind\":\"images\",\"files\":[\"/dev/null\"],\"return_vt\":1}\n").starts_with("error: not a regular file"));
    }

    fn f(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }
    fn prog(plan: Result<Plan, String>) -> String {
        match plan {
            Ok(Plan::Run { program, .. }) => program,
            Ok(Plan::PdfPages { .. }) => "pdfpages".into(),
            Err(e) => format!("err: {e}"),
        }
    }

    #[test]
    fn plan_follows_the_hardware_and_the_installed_programs() {
        let keys = Path::new("/run/k.conf");
        let all = |_: &str| true;
        let only = |names: &'static [&'static str]| move |b: &str| names.contains(&b);
        let both = Hw { fb: true, drm: true };
        let fb_only = Hw { fb: true, drm: false };
        let drm_only = Hw { fb: false, drm: true };
        let nothing = Hw::default();
        // Video: mpv over DRM, else cvlc on the framebuffer.
        assert_eq!(prog(plan(Kind::Video, &f(&["/v/a.mp4"]), &all, both, keys, false)), "mpv");
        assert_eq!(prog(plan(Kind::Video, &f(&["/v/a.mp4"]), &all, fb_only, keys, false)), "cvlc");
        assert!(prog(plan(Kind::Video, &f(&["/v/a.mp4"]), &only(&["mpv"]), fb_only, keys, false)).starts_with("err"), "mpv needs DRM");
        assert!(prog(plan(Kind::Video, &f(&["/a", "/b"]), &all, both, keys, false)).starts_with("err"), "one video only");
        assert!(prog(plan(Kind::Video, &f(&["/a"]), &all, nothing, keys, false)).starts_with("err"));
        // Images: fbi, fim, then mpv over DRM.
        let imgs = f(&["/i/1.png", "/i/2.png"]);
        assert_eq!(prog(plan(Kind::Images, &imgs, &all, both, keys, false)), "fbi");
        assert_eq!(prog(plan(Kind::Images, &imgs, &only(&["fim", "mpv"]), both, keys, false)), "fim");
        assert_eq!(prog(plan(Kind::Images, &imgs, &all, drm_only, keys, false)), "mpv");
        assert!(prog(plan(Kind::Images, &imgs, &only(&["fbi"]), drm_only, keys, false)).starts_with("err"), "fbi needs a framebuffer");
        assert!(prog(plan(Kind::Images, &imgs, &all, nothing, keys, false)).starts_with("err"));
        match plan(Kind::Images, &imgs, &all, both, keys, false).unwrap() {
            Plan::Run { args, .. } => assert_eq!(args, ["-a", "--noverbose", "--", "/i/1.png", "/i/2.png"]),
            other => panic!("{other:?}"),
        }
        match plan(Kind::Images, &imgs, &all, drm_only, keys, false).unwrap() {
            Plan::Run { args, .. } => {
                assert!(args.contains(&"--vo=drm".to_string()) && args.contains(&"--input-conf=/run/k.conf".to_string()));
                assert_eq!(&args[args.len() - 3..], ["--", "/i/1.png", "/i/2.png"]);
            }
            other => panic!("{other:?}"),
        }
        // PDFs: fbgs with gs, else pdftoppm pages through mpv over DRM.
        let pdf = f(&["/d/x.pdf"]);
        assert_eq!(prog(plan(Kind::Pdf, &pdf, &all, both, keys, false)), "fbgs");
        assert_eq!(prog(plan(Kind::Pdf, &pdf, &all, drm_only, keys, false)), "pdfpages");
        assert!(prog(plan(Kind::Pdf, &pdf, &only(&["fbgs"]), both, keys, false)).starts_with("err"), "fbgs needs gs");
        assert!(prog(plan(Kind::Pdf, &pdf, &only(&["mpv"]), drm_only, keys, false)).starts_with("err"), "needs pdftoppm");
        assert!(prog(plan(Kind::Pdf, &pdf, &all, nothing, keys, false)).starts_with("err"));
        assert!(prog(plan(Kind::Images, &[], &all, both, keys, false)).starts_with("err"));
        // The waiter (kmscon holds DRM): images and PDFs through mpv over DRM
        // first; fbi and fbgs only without mpv or DRM; video unchanged.
        assert_eq!(prog(plan(Kind::Images, &imgs, &all, both, keys, true)), "mpv");
        assert_eq!(prog(plan(Kind::Pdf, &pdf, &all, both, keys, true)), "pdfpages");
        assert_eq!(prog(plan(Kind::Images, &imgs, &only(&["fbi"]), both, keys, true)), "fbi", "no mpv: fbi still");
        assert_eq!(prog(plan(Kind::Images, &imgs, &all, fb_only, keys, true)), "fbi", "no DRM: fbi still");
        assert_eq!(prog(plan(Kind::Pdf, &pdf, &only(&["mpv", "fbgs", "gs"]), both, keys, true)), "fbgs", "no pdftoppm: fbgs");
        assert_eq!(prog(plan(Kind::Video, &f(&["/v/a.mp4"]), &all, both, keys, true)), "mpv");
    }

    #[test]
    fn offered_needs_a_way_to_the_screen_and_a_route() {
        use crate::display::ClientCaps;
        let all = |_: &str| true;
        let drm = ClientCaps { drm_device: true, ..Default::default() };
        assert!(!offered(Kind::Video, &drm, &all), "no console and no waiter");
        assert!(offered(Kind::Video, &ClientCaps { vt_console: true, ..drm }, &all), "plain console");
        assert!(offered(Kind::Video, &ClientCaps { fullscreen_waiter: true, ..drm }, &all), "kmscon with the waiter");
        assert!(!offered(Kind::Video, &ClientCaps { vt_console: true, ..Default::default() }, &all), "no display device");
        let only_mpv = |b: &str| b == "mpv";
        assert!(!offered(Kind::Pdf, &ClientCaps { vt_console: true, fb_device: true, ..Default::default() }, &only_mpv), "fbgs missing");
    }

    #[test]
    fn kind_names_round_trip() {
        for k in [Kind::Video, Kind::Images, Kind::Pdf] {
            assert_eq!(Kind::parse(k.name()), Some(k));
        }
        assert_eq!(Kind::parse("rm"), None);
    }

    #[test]
    fn request_round_trips() {
        let r = Request { kind: Kind::Images, files: vec!["/v/a b.png".into(), "/v/c.png".into()], return_vt: 1 };
        let back: Request = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert_eq!(back, r);
    }
}
