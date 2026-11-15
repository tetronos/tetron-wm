//! Desktop cast: an optional addon seam. While a cast runs, the composited cell
//! buffer is written as JSON lines (protocol v1, see docs) to the stdin of
//! `casttui --cells`, which serves the page, controls the Chromecast and holds
//! every network facing part. tetron-wm itself opens no port. Without casttui
//! installed nothing here runs and the cast UI stays hidden.
//!
//! Events: `hello` (cols, rows), `full` / `diff` (`spans`, the packed form of
//! `protocol::Span`), `cursor` (null: the compositor draws the cursor into the
//! cells), `status` (`detached` | `ok` | `stopped`). The URL, which holds a
//! secret token and is only ever printed by `--no-cast`, is never logged.

use crate::buffer::CellBuffer;
use crate::cell::Cell;
use crate::compositor::CellChange;
use crate::protocol::pack_changes;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// Seconds without an attached terminal before the cast ends by itself.
const DETACH_GRACE: u64 = 60;
/// How long the sink program may take to report that the cast is up.
const START_WAIT: Duration = Duration::from_secs(30);

/// One published frame.
#[derive(Clone)]
pub struct Snapshot {
    pub gen: u64,
    pub buf: Arc<CellBuffer>,
}

/// Cells of `new` that differ from `prev`; every cell when there is no `prev`
/// or the size changed.
pub fn diff_buffers(prev: Option<&CellBuffer>, new: &CellBuffer) -> Vec<CellChange> {
    let same_size = prev.is_some_and(|p| p.width() == new.width() && p.height() == new.height());
    let mut out = Vec::new();
    for y in 0..new.height() {
        for x in 0..new.width() {
            let c: Cell = *new.get(x, y).unwrap();
            if !same_size || prev.and_then(|p| p.get(x, y)) != Some(&c) {
                out.push(CellChange { x, y, cell: c });
            }
        }
    }
    out
}

fn line(v: &serde_json::Value) -> String { format!("{v}\n") }

/// The lines that bring a sink that last received `prev` to `snap`.
pub fn events_for(prev: Option<&Snapshot>, snap: &Snapshot) -> Vec<String> {
    let mut out = Vec::new();
    let resized = prev.is_none_or(|p| p.buf.width() != snap.buf.width() || p.buf.height() != snap.buf.height());
    if resized {
        out.push(line(&serde_json::json!({"t":"hello","v":1,"cols":snap.buf.width(),"rows":snap.buf.height()})));
    }
    let changes = diff_buffers(if resized { None } else { prev.map(|p| &*p.buf) }, &snap.buf);
    if !changes.is_empty() || resized {
        let t = if resized { "full" } else { "diff" };
        let spans = serde_json::to_value(pack_changes(&changes)).unwrap_or_default();
        out.push(line(&serde_json::json!({"t":t,"spans":spans})));
    }
    if resized {
        out.push(line(&serde_json::json!({"t":"cursor","x":null,"y":null})));
    }
    out
}

/// State shared by the render loop (publisher) and the writer thread.
struct Sink {
    slot: Mutex<Option<Snapshot>>,
    cv: Condvar,
    stop: AtomicBool,
    detached: AtomicBool,
    done: AtomicBool,
}

static CURRENT: Mutex<Option<Arc<Sink>>> = Mutex::new(None);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static WANT_FULL: AtomicBool = AtomicBool::new(true);

/// Whether a cast is running: one atomic load, so idle sessions pay nothing.
pub fn active() -> bool { ACTIVE.load(Ordering::Relaxed) }

/// True once after a cast starts: the render loop must publish a frame even if
/// the screen is unchanged.
pub fn take_want_full() -> bool { active() && WANT_FULL.swap(false, Ordering::Relaxed) }

fn current() -> Option<Arc<Sink>> { CURRENT.lock().unwrap_or_else(|e| e.into_inner()).clone() }

/// Publish the composited screen (a clone of it): an `Arc` swap and a wake up,
/// never a write to the sink. The cursor is already drawn into the cells.
pub fn publish_frame(buf: &CellBuffer) {
    if let Some(s) = current() {
        let mut g = s.slot.lock().unwrap_or_else(|e| e.into_inner());
        let gen = g.as_ref().map_or(1, |x| x.gen + 1);
        *g = Some(Snapshot { gen, buf: Arc::new(buf.clone()) });
        s.cv.notify_all();
    }
}

/// Mark the session attached or detached (viewers show an overlay when detached).
pub fn set_detached(d: bool) {
    if let Some(s) = current() {
        if s.detached.swap(d, Ordering::Relaxed) != d { s.cv.notify_all(); }
    }
}

/// Whether casting can be offered: a configured program, or casttui installed.
pub fn available(cfg: &crate::config::Config) -> bool {
    !cfg.cast_command.trim().is_empty() || crate::catalog::is_installed("casttui")
}

/// The program and arguments for a cast: `casttui --cells [--no-cast] [-d DEVICE]`.
pub fn command_line(cfg: &crate::config::Config, device: Option<&str>, browser: bool) -> (String, Vec<String>) {
    let program = match cfg.cast_command.trim() { "" => "casttui", p => p }.to_string();
    let mut args = vec!["--cells".to_string()];
    if browser {
        args.push("--no-cast".into());
    } else {
        let d = device.map(str::trim).filter(|d| !d.is_empty()).unwrap_or(cfg.cast_device.trim());
        if !d.is_empty() { args.extend(["-d".to_string(), d.to_string()]); }
    }
    (program, args)
}

/// Start the sink program and begin streaming. Returns the program's first
/// output line: `casting cells on DEVICE`, or with `browser` the URL (a secret).
pub fn start(cfg: &crate::config::Config, device: Option<&str>, browser: bool) -> Result<String, String> {
    if active() { return Err("already casting (tetron-wm cast off stops it)".into()); }
    if !available(cfg) {
        return Err("casting needs casttui (sudo apt install casttui)".into());
    }
    let (program, args) = command_line(cfg, device, browser);
    crate::dbg_log(&format!("cast: starting {program} {}", args.join(" ")));
    let mut child = Command::new(&program)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run {program}: {e}"))?;
    let stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr = child.stderr.take().ok_or("no stderr")?;
    let sink = Arc::new(Sink {
        slot: Mutex::new(None),
        cv: Condvar::new(),
        stop: AtomicBool::new(false),
        detached: AtomicBool::new(false),
        done: AtomicBool::new(false),
    });
    let last_err = Arc::new(Mutex::new(String::new()));
    let le = last_err.clone();
    std::thread::spawn(move || {
        for l in BufReader::new(stderr).lines().map_while(Result::ok) {
            if !l.trim().is_empty() { *le.lock().unwrap_or_else(|e| e.into_inner()) = l; }
        }
    });
    // First stdout line to the caller; the rest is logged (never a URL).
    let (tx, rx) = mpsc::channel::<String>();
    let sk = sink.clone();
    std::thread::spawn(move || {
        let mut first = true;
        for l in BufReader::new(stdout).lines().map_while(Result::ok) {
            if first { first = false; let _ = tx.send(l); }
            else if !l.starts_with("http") { crate::dbg_log(&format!("cast: {l}")); }
        }
        // The sink program ended (TV switched, casttui --stop, crash): end the cast.
        sk.stop.store(true, Ordering::Relaxed);
        sk.cv.notify_all();
    });
    let first = match rx.recv_timeout(START_WAIT) {
        Ok(l) => l,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            std::thread::sleep(Duration::from_millis(100)); // let the stderr reader finish
            let e = last_err.lock().unwrap_or_else(|e| e.into_inner()).clone();
            return Err(if e.is_empty() { format!("{program} did not start the cast") } else { e.trim_start_matches("casttui: ").to_string() });
        }
    };
    *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = Some(sink.clone());
    WANT_FULL.store(true, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
    let gap = Duration::from_millis(1000 / u64::from(cfg.cast_fps.clamp(1, 60)));
    std::thread::spawn(move || writer(sink, stdin, child, gap));
    Ok(first)
}

/// Write events to the sink until told to stop, the sink ends, or the terminal
/// stays detached for DETACH_GRACE seconds; then close its stdin (casttui quits
/// the TV app and exits) and reap it.
fn writer(sink: Arc<Sink>, mut stdin: std::process::ChildStdin, mut child: std::process::Child, gap: Duration) {
    let mut sent: Option<Snapshot> = None;
    let mut shown_detached = false;
    let mut detached_since = Instant::now();
    let mut last_send = Instant::now() - gap;
    'run: loop {
        let snap = {
            let mut g = sink.slot.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if sink.stop.load(Ordering::Relaxed) { break 'run; }
                let det = sink.detached.load(Ordering::Relaxed);
                if !det { detached_since = Instant::now(); }
                if det && detached_since.elapsed().as_secs() >= DETACH_GRACE {
                    crate::dbg_log("cast: no terminal attached, stopping");
                    break 'run;
                }
                if det != shown_detached {
                    shown_detached = det;
                    let st = line(&serde_json::json!({"t":"status","state": if det { "detached" } else { "ok" }}));
                    if stdin.write_all(st.as_bytes()).and_then(|()| stdin.flush()).is_err() { break 'run; }
                }
                if let Some(cur) = g.as_ref() {
                    if sent.as_ref().is_none_or(|p| p.gen != cur.gen) { break g.clone(); }
                }
                g = sink.cv.wait_timeout(g, Duration::from_secs(1)).unwrap_or_else(|e| e.into_inner()).0;
            }
        };
        let Some(snap) = snap else { continue };
        let since = last_send.elapsed();
        if since < gap { std::thread::sleep(gap - since); }
        // Coalesce: diff against the newest snapshot, not the one that woke us.
        let snap = sink.slot.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or(snap);
        for ev in events_for(sent.as_ref(), &snap) {
            if stdin.write_all(ev.as_bytes()).is_err() { break 'run; }
        }
        if stdin.flush().is_err() { break 'run; }
        sent = Some(snap);
        last_send = Instant::now();
    }
    ACTIVE.store(false, Ordering::Relaxed);
    {
        let mut cur = CURRENT.lock().unwrap_or_else(|e| e.into_inner());
        if cur.as_ref().is_some_and(|c| Arc::ptr_eq(c, &sink)) { *cur = None; }
    }
    let _ = stdin.write_all(line(&serde_json::json!({"t":"status","state":"stopped"})).as_bytes());
    drop(stdin); // EOF: casttui ends the cast on the TV and exits
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(12) {
        if matches!(child.try_wait(), Ok(Some(_))) { break; }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
    crate::dbg_log("cast: stopped");
    sink.done.store(true, Ordering::Relaxed);
}

/// Stop the cast and wait (up to 15 s) for the TV side to be quit.
pub fn stop() -> bool {
    let Some(s) = current() else { return false };
    s.stop.store(true, Ordering::Relaxed);
    s.cv.notify_all();
    let t0 = Instant::now();
    while !s.done.load(Ordering::Relaxed) && t0.elapsed() < Duration::from_secs(15) {
        std::thread::sleep(Duration::from_millis(50));
    }
    true
}

/// One line for `tetron-wm cast status`.
pub fn status() -> String { if active() { "casting".into() } else { "off".into() } }

/// Start the TV cast, or stop it while one runs (leader key, menu item).
pub fn toggle(cfg: &crate::config::Config) -> Result<String, String> {
    if active() { stop(); Ok("stopped".into()) } else { start(cfg, None, false) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn snap(gen: u64, b: CellBuffer) -> Snapshot { Snapshot { gen, buf: Arc::new(b) } }

    fn script(dir: &std::path::Path, body: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(dir).unwrap();
        let p = dir.join("sink.sh");
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn diff_finds_only_changes() {
        let a = CellBuffer::new(4, 2);
        let mut b = a.clone();
        assert!(diff_buffers(Some(&a), &b).is_empty());
        b.set(1, 1, Cell { ch: 'x', ..Cell::default() });
        let d = diff_buffers(Some(&a), &b);
        assert_eq!((d.len(), d[0].x, d[0].y), (1, 1, 1));
        assert_eq!(diff_buffers(None, &b).len(), 8, "no previous buffer means every cell");
        assert_eq!(diff_buffers(Some(&CellBuffer::new(2, 2)), &b).len(), 8, "a resize means every cell");
    }

    #[test]
    fn events_hello_full_cursor_then_diff() {
        let mut buf = CellBuffer::new(3, 1);
        let s1 = snap(1, buf.clone());
        let ev = events_for(None, &s1);
        assert_eq!(ev.len(), 3);
        assert!(ev[0].contains("\"hello\"") && ev[0].contains("\"cols\":3") && ev[0].ends_with('\n'));
        assert!(ev[1].contains("\"full\""));
        assert!(ev[2].contains("\"cursor\""));
        buf.set(2, 0, Cell { ch: 'q', ..Cell::default() });
        let s2 = snap(2, buf);
        let ev = events_for(Some(&s1), &s2);
        assert_eq!(ev.len(), 1, "only the diff");
        assert!(ev[0].contains("\"diff\"") && ev[0].contains('q'));
        assert!(events_for(Some(&s2), &s2).is_empty(), "nothing changed, nothing sent");
        assert_eq!(events_for(Some(&s2), &snap(3, CellBuffer::new(5, 2))).len(), 3, "a resize starts over");
    }

    #[test]
    fn command_line_for_tv_and_browser() {
        let mut c = Config { cast_device: "TV".into(), ..Config::default() };
        assert_eq!(command_line(&c, None, false), ("casttui".into(), vec!["--cells".into(), "-d".into(), "TV".into()]));
        assert_eq!(command_line(&c, Some(" 1.2.3.4 "), false).1, ["--cells", "-d", "1.2.3.4"]);
        assert_eq!(command_line(&c, None, true).1, ["--cells", "--no-cast"], "the browser view needs no device");
        c.cast_command = "/opt/x/casttui".into();
        c.cast_device.clear();
        assert_eq!(command_line(&c, None, false), ("/opt/x/casttui".into(), vec!["--cells".into()]));
    }

    /// One test owns the global cast state: a failing sink first, then a fake sink
    /// that reports it is up, records what it is sent, and must see EOF on stop.
    #[test]
    fn a_failing_sink_reports_its_error_and_a_working_one_is_streamed_to_and_closed() {
        let base = std::env::temp_dir().join(format!("tetron-wm-cast-test-{}", std::process::id()));
        let mut cfg = Config { cast_fps: 60, ..Config::default() };

        cfg.cast_command = script(&base.join("fail"), "echo 'casttui: no Chromecast found' >&2\nexit 1");
        let e = start(&cfg, None, false).unwrap_err();
        assert!(e.contains("no Chromecast found") && !active(), "{e}");

        let out = base.join("out.txt");
        cfg.cast_command = script(&base.join("ok"), &format!("echo 'casting cells on FAKE'\ncat > '{}'", out.display()));
        assert_eq!(status(), "off");
        assert_eq!(start(&cfg, None, false).unwrap(), "casting cells on FAKE");
        assert!(active() && take_want_full() && !take_want_full(), "one forced full frame per start");
        assert!(start(&cfg, None, false).is_err(), "a second start is refused");
        let mut b = CellBuffer::new(4, 2);
        publish_frame(&b);
        b.set(0, 0, Cell { ch: 'Z', ..Cell::default() });
        publish_frame(&b);
        std::thread::sleep(Duration::from_millis(400));
        set_detached(true);
        std::thread::sleep(Duration::from_millis(1300));
        assert!(stop() && !stop());
        assert!(!active() && status() == "off");
        let got = std::fs::read_to_string(&out).unwrap();
        assert!(got.contains("\"hello\"") && got.contains("\"full\"") && got.contains('Z'), "{got}");
        assert!(got.contains("\"detached\"") && got.contains("\"stopped\""), "{got}");
        let _ = std::fs::remove_dir_all(&base);
    }
}
