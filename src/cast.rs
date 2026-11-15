//! Cast viewer: a tiny read only HTTP server that streams the composited cell
//! buffer to a web page (for a Chromecast or any browser) as Server-Sent Events.
//! Std only. GET only, an unguessable token in the path, LAN address only.
//! The token and the full URL are never logged.

use crate::buffer::CellBuffer;
use crate::cell::Cell;
use crate::compositor::CellChange;
use crate::protocol::{pack_changes, Span};
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const PAGE: &str = include_str!("../assets/cast.html");
const MAX_HEADER: usize = 8 * 1024;
const MAX_VIEWERS: usize = 4;
const HEARTBEAT: Duration = Duration::from_secs(15);

/// One published frame.
#[derive(Clone)]
pub struct Snapshot {
    pub gen: u64,
    pub buf: Arc<CellBuffer>,
    pub cursor: Option<(i32, i32)>,
}

/// State shared by the publisher (render loop) and the viewer threads.
pub struct Shared {
    slot: Mutex<Option<Snapshot>>,
    cv: Condvar,
    stop: AtomicBool,
    viewers: AtomicUsize,
    detached: AtomicBool,
}

impl Shared {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { slot: Mutex::new(None), cv: Condvar::new(), stop: AtomicBool::new(false), viewers: AtomicUsize::new(0), detached: AtomicBool::new(false) })
    }
    /// Publish a frame: an `Arc` swap and a wake up, never a socket write.
    pub fn publish(&self, buf: Arc<CellBuffer>, cursor: Option<(i32, i32)>) {
        let mut g = self.slot.lock().unwrap();
        let gen = g.as_ref().map_or(1, |s| s.gen + 1);
        *g = Some(Snapshot { gen, buf, cursor });
        self.cv.notify_all();
    }
    /// The session has no attached client: viewers show an overlay instead of a frozen frame.
    pub fn set_detached(&self, d: bool) {
        if self.detached.swap(d, Ordering::Relaxed) != d { self.cv.notify_all(); }
    }
    pub fn viewers(&self) -> usize { self.viewers.load(Ordering::Relaxed) }
    pub fn stop(&self) { self.stop.store(true, Ordering::Relaxed); self.cv.notify_all(); }
    fn stopped(&self) -> bool { self.stop.load(Ordering::Relaxed) }
}

/// 128 random bits as 32 hex digits, from the kernel.
pub fn new_token() -> std::io::Result<String> {
    let mut b = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// Compare without stopping at the first difference.
pub fn token_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
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

/// One SSE event carrying a JSON object.
pub fn sse(v: &serde_json::Value) -> String { format!("data: {v}\n\n") }

fn spans_json(s: &[Span]) -> serde_json::Value { serde_json::to_value(s).unwrap_or_default() }

/// The events to send a viewer that last received `prev` so that it shows `snap`.
pub fn events_for(prev: Option<&Snapshot>, snap: &Snapshot) -> Vec<String> {
    let mut out = Vec::new();
    let resized = prev.is_none_or(|p| p.buf.width() != snap.buf.width() || p.buf.height() != snap.buf.height());
    if resized {
        out.push(sse(&serde_json::json!({"t":"hello","v":1,"cols":snap.buf.width(),"rows":snap.buf.height()})));
    }
    let changes = diff_buffers(if resized { None } else { prev.map(|p| &*p.buf) }, &snap.buf);
    if !changes.is_empty() || resized {
        let t = if resized { "full" } else { "diff" };
        out.push(sse(&serde_json::json!({"t":t,"spans":spans_json(&pack_changes(&changes))})));
    }
    if resized || prev.map(|p| p.cursor) != Some(snap.cursor) {
        let (x, y) = snap.cursor.map_or((None, None), |(x, y)| (Some(x), Some(y)));
        out.push(sse(&serde_json::json!({"t":"cursor","x":x,"y":y})));
    }
    out
}

#[derive(Debug, PartialEq)]
pub enum Route { Page, Events }

/// Route a request. `Err` is the status code to answer with: 405 for a method
/// other than GET, 400 for a malformed line, 404 for anything else (no body).
pub fn route(request_line: &str, token: &str) -> Result<Route, u16> {
    let mut it = request_line.split(' ');
    let (Some(method), Some(target), Some(ver), None) = (it.next(), it.next(), it.next(), it.next()) else { return Err(400) };
    if !ver.starts_with("HTTP/1.") { return Err(400); }
    if method != "GET" { return Err(405); }
    let path = target.split(['?', '#']).next().unwrap_or("");
    let Some(rest) = path.strip_prefix('/') else { return Err(404) };
    let (tok, tail) = rest.split_once('/').unwrap_or((rest, "\u{0}"));
    if !token_eq(tok, token) { return Err(404); }
    match tail { "" => Ok(Route::Page), "events" => Ok(Route::Events), _ => Err(404) }
}

fn status_text(code: u16) -> &'static str {
    match code { 400 => "Bad Request", 405 => "Method Not Allowed", 503 => "Service Unavailable", _ => "Not Found" }
}

fn reject(s: &mut TcpStream, code: u16) {
    let _ = write!(s, "HTTP/1.1 {code} {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", status_text(code));
}

/// Read the request head (up to the blank line) with a size cap; the first line.
fn read_request_line(s: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::new();
    let mut b = [0u8; 512];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = s.read(&mut b).ok()?;
        if n == 0 { return None; }
        buf.extend_from_slice(&b[..n]);
        if buf.len() > MAX_HEADER { return None; }
    }
    let head = String::from_utf8_lossy(&buf).into_owned();
    head.lines().next().map(str::to_string)
}

/// A running viewer server.
pub struct Viewer {
    pub shared: Arc<Shared>,
    pub port: u16,
    pub token: String,
}

impl Viewer {
    /// Listen on `ip:port` (port 0 picks a free one). When `allow` is set, other
    /// peers are refused, except loopback.
    pub fn start(ip: IpAddr, port: u16, allow: Option<IpAddr>, fps: u32) -> std::io::Result<Viewer> {
        let token = new_token()?;
        let l = TcpListener::bind(SocketAddr::new(ip, port))?;
        l.set_nonblocking(true)?;
        let port = l.local_addr()?.port();
        let shared = Shared::new();
        let (sh, tok) = (shared.clone(), token.clone());
        let gap = Duration::from_millis(1000 / u64::from(fps.clamp(1, 60)));
        std::thread::spawn(move || accept_loop(l, sh, tok, allow, gap));
        Ok(Viewer { shared, port, token })
    }
    pub fn url(&self, host: &str) -> String { format!("http://{host}:{}/{}/", self.port, self.token) }
    pub fn stop(&self) { self.shared.stop(); }
}

fn accept_loop(l: TcpListener, sh: Arc<Shared>, token: String, allow: Option<IpAddr>, gap: Duration) {
    while !sh.stopped() {
        match l.accept() {
            Ok((s, peer)) => {
                let ok = allow.is_none_or(|a| peer.ip() == a || peer.ip().is_loopback());
                if !ok {
                    crate::dbg_log(&format!("cast: refused {}", peer.ip()));
                    continue;
                }
                let (sh, token) = (sh.clone(), token.clone());
                std::thread::spawn(move || serve_conn(s, peer, sh, token, gap));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => break,
        }
    }
}

fn serve_conn(mut s: TcpStream, peer: SocketAddr, sh: Arc<Shared>, token: String, gap: Duration) {
    let _ = s.set_nonblocking(false);
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    let Some(line) = read_request_line(&mut s) else { return };
    match route(&line, &token) {
        Err(code) => reject(&mut s, code),
        Ok(Route::Page) => {
            let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{PAGE}", PAGE.len());
        }
        Ok(Route::Events) => {
            if sh.viewers.fetch_add(1, Ordering::Relaxed) >= MAX_VIEWERS {
                sh.viewers.fetch_sub(1, Ordering::Relaxed);
                return reject(&mut s, 503);
            }
            crate::dbg_log(&format!("cast: viewer connected {}", peer.ip()));
            stream_events(&mut s, &sh, gap);
            sh.viewers.fetch_sub(1, Ordering::Relaxed);
            crate::dbg_log(&format!("cast: viewer left {}", peer.ip()));
        }
    }
}

fn stream_events(s: &mut TcpStream, sh: &Shared, gap: Duration) {
    let _ = s.set_read_timeout(None);
    let _ = s.set_write_timeout(Some(Duration::from_secs(10)));
    if write!(s, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n").is_err() { return; }
    let mut sent: Option<Snapshot> = None;
    let mut shown_detached = false;
    let mut last_send = Instant::now();
    let mut last_beat = Instant::now();
    loop {
        let snap = {
            let mut g = sh.slot.lock().unwrap();
            loop {
                if sh.stopped() { drop(g); let _ = s.write_all(sse(&serde_json::json!({"t":"status","state":"stopped"})).as_bytes()); return; }
                let det = sh.detached.load(Ordering::Relaxed);
                if det != shown_detached {
                    shown_detached = det;
                    let st = sse(&serde_json::json!({"t":"status","state": if det { "detached" } else { "ok" }}));
                    if s.write_all(st.as_bytes()).is_err() { return; }
                }
                if let Some(cur) = g.as_ref() {
                    if sent.as_ref().is_none_or(|p| p.gen != cur.gen) { break g.clone(); }
                }
                let (ng, to) = sh.cv.wait_timeout(g, HEARTBEAT).unwrap();
                g = ng;
                if to.timed_out() && last_beat.elapsed() >= HEARTBEAT {
                    if s.write_all(b":\n\n").is_err() { return; }
                    last_beat = Instant::now();
                }
            }
        };
        let Some(snap) = snap else { continue };
        // Coalesce: wait out the frame gap, then diff against the newest snapshot.
        let since = last_send.elapsed();
        if since < gap { std::thread::sleep(gap - since); }
        let snap = sh.slot.lock().unwrap().clone().unwrap_or(snap);
        for ev in events_for(sent.as_ref(), &snap) {
            if s.write_all(ev.as_bytes()).is_err() { return; }
        }
        let _ = s.flush();
        sent = Some(snap);
        last_send = Instant::now();
        last_beat = Instant::now();
    }
}

// ---- the one cast of this daemon -------------------------------------------

static CURRENT: Mutex<Option<Arc<Viewer>>> = Mutex::new(None);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static WANT_FULL: AtomicBool = AtomicBool::new(true);

/// Whether a cast is running: one atomic load, so idle sessions pay nothing.
pub fn active() -> bool { ACTIVE.load(Ordering::Relaxed) }

/// True once after a cast starts: the render loop must publish a frame even if
/// the screen is unchanged.
pub fn take_want_full() -> bool { active() && WANT_FULL.swap(false, Ordering::Relaxed) }

/// Publish the composited screen (a clone of it) to the viewers. The cursor is
/// already drawn into the cells by the compositor.
pub fn publish_frame(buf: &CellBuffer) {
    if let Some(v) = CURRENT.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        v.shared.publish(Arc::new(buf.clone()), None);
    }
}

/// Mark the session attached or detached.
pub fn set_detached(d: bool) {
    if let Some(v) = CURRENT.lock().unwrap_or_else(|e| e.into_inner()).as_ref() { v.shared.set_detached(d); }
}

/// The local address that routes toward `device` (a UDP connect sends no packet),
/// or toward the default route when `device` is not an IP.
pub fn lan_ip(device: Option<&str>) -> IpAddr {
    let target: IpAddr = device.and_then(|d| d.trim().parse().ok()).unwrap_or_else(|| "192.0.2.1".parse().unwrap());
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| { s.connect(SocketAddr::new(target, 9))?; s.local_addr() })
        .map(|a| a.ip())
        .unwrap_or_else(|_| "127.0.0.1".parse().unwrap())
}

/// Start the viewer (or return the running one's URL). Returns the URL, which
/// holds the secret token.
pub fn start(device: Option<&str>, port: u16, fps: u32, idle_stop: u64) -> Result<String, String> {
    let mut cur = CURRENT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(v) = cur.as_ref() { return Ok(v.url(&lan_ip(device).to_string())); }
    let ip = lan_ip(device);
    let allow = device.and_then(|d| d.trim().parse::<IpAddr>().ok());
    let v = Arc::new(Viewer::start(ip, port, allow, fps).map_err(|e| format!("cannot start the viewer: {e}"))?);
    let url = v.url(&ip.to_string());
    crate::dbg_log(&format!("cast: serving on {ip}:{}", v.port));
    *cur = Some(v.clone());
    WANT_FULL.store(true, Ordering::Relaxed);
    ACTIVE.store(true, Ordering::Relaxed);
    std::thread::spawn(move || watch(v, idle_stop));
    Ok(url)
}

/// Stop the cast: viewers get a "cast stopped" overlay and the listener closes.
pub fn stop() -> bool {
    let v = CURRENT.lock().unwrap_or_else(|e| e.into_inner()).take();
    ACTIVE.store(false, Ordering::Relaxed);
    match v { Some(v) => { v.stop(); crate::dbg_log("cast: stopped"); true } None => false }
}

/// One line for `tetron-wm cast status`.
pub fn status() -> String {
    match CURRENT.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        Some(v) => format!("casting, {} viewer(s)", v.shared.viewers()),
        None => "off".into(),
    }
}

/// Ends the cast after `idle_stop` seconds with no viewer (0 = never) or 60 s detached.
fn watch(v: Arc<Viewer>, idle_stop: u64) {
    let (mut no_viewer, mut detached) = (Instant::now(), Instant::now());
    loop {
        std::thread::sleep(Duration::from_secs(1));
        if v.shared.stopped() { return; }
        if v.shared.viewers() > 0 { no_viewer = Instant::now(); }
        if !v.shared.detached.load(Ordering::Relaxed) { detached = Instant::now(); }
        if (idle_stop > 0 && no_viewer.elapsed().as_secs() >= idle_stop) || detached.elapsed().as_secs() >= 60 {
            crate::dbg_log("cast: idle, stopping");
            let mine = CURRENT.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|c| Arc::ptr_eq(c, &v));
            if mine { stop(); } else { v.stop(); }
            return;
        }
    }
}

/// An animated test grid for `tetron-wm cast --demo`: a moving gradient, box
/// and block glyphs and some text, so the page can be checked without a session.
pub fn demo_buffer(tick: u32, w: i32, h: i32) -> CellBuffer {
    use crate::cell::Rgba;
    let mut b = CellBuffer::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let v = ((x as u32 * 4 + y as u32 * 8 + tick * 6) % 255) as u8;
            let ch = char::from_u32([0x2580, 0x2584, 0x2588, 0x2596, 0x259a, 0x2591][((x + y) as usize + tick as usize / 4) % 6]).unwrap_or('#');
            b.set(x, y, Cell { ch, fg: Rgba::rgb(v, 255 - v, 160), bg: Rgba::rgb(20, v / 4, 60), attrs: Default::default() });
        }
    }
    let (fg, bg) = (Rgba::rgb(255, 255, 255), Rgba::rgb(0, 0, 0));
    if w > 30 && h > 6 {
        b.write_str(2, 1, &format!("{}{}{}", "\u{250c}", "\u{2500}".repeat(16), "\u{2510}"), fg, bg);
        b.write_str(2, 2, "\u{2502} tetron-wm demo \u{2502}", fg, bg);
        b.write_str(2, 3, &format!("\u{2502} tick {tick:<9}\u{2502}"), fg, bg);
        b.write_str(2, 4, &format!("{}{}{}", "\u{2514}", "\u{2500}".repeat(16), "\u{2518}"), fg, bg);
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn token_is_32_hex_and_unique() {
        let (a, b) = (new_token().unwrap(), new_token().unwrap());
        assert_eq!(a.len(), 32);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn token_compare() {
        assert!(token_eq(T, T));
        assert!(!token_eq(T, "0123456789abcdef0123456789abcdee"));
        assert!(!token_eq(T, "short"));
    }

    #[test]
    fn routes() {
        assert_eq!(route(&format!("GET /{T}/ HTTP/1.1"), T), Ok(Route::Page));
        assert_eq!(route(&format!("GET /{T}/?x=1 HTTP/1.1"), T), Ok(Route::Page));
        assert_eq!(route(&format!("GET /{T}/events HTTP/1.1"), T), Ok(Route::Events));
        assert_eq!(route(&format!("GET /{T} HTTP/1.1"), T), Err(404), "relative events URL needs the slash");
        assert_eq!(route("GET /wrong/ HTTP/1.1", T), Err(404));
        assert_eq!(route(&format!("GET /{T}/other HTTP/1.1"), T), Err(404));
        assert_eq!(route(&format!("POST /{T}/ HTTP/1.1"), T), Err(405));
        assert_eq!(route("GET", T), Err(400));
        assert_eq!(route("GET / FTP/1.0", T), Err(400));
        assert_eq!(route(&format!("GET /{T}/ HTTP/1.1 extra"), T), Err(400));
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
        let s1 = Snapshot { gen: 1, buf: Arc::new(buf.clone()), cursor: Some((0, 0)) };
        let ev = events_for(None, &s1);
        assert_eq!(ev.len(), 3);
        assert!(ev[0].contains("\"hello\"") && ev[0].contains("\"cols\":3"));
        assert!(ev[1].contains("\"full\""));
        assert!(ev[2].contains("\"cursor\""));
        buf.set(2, 0, Cell { ch: 'q', ..Cell::default() });
        let s2 = Snapshot { gen: 2, buf: Arc::new(buf), cursor: Some((0, 0)) };
        let ev = events_for(Some(&s1), &s2);
        assert_eq!(ev.len(), 1, "same cursor: only the diff");
        assert!(ev[0].contains("\"diff\"") && ev[0].contains('q'));
        assert!(events_for(Some(&s2), &s2).is_empty(), "nothing changed, nothing sent");
    }

    fn get(addr: SocketAddr, req: &str) -> String {
        let mut s = TcpStream::connect(addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        s.write_all(req.as_bytes()).unwrap();
        let mut out = Vec::new();
        let mut b = [0u8; 4096];
        loop {
            match s.read(&mut b) { Ok(0) | Err(_) => break, Ok(n) => out.extend_from_slice(&b[..n]) }
            if String::from_utf8_lossy(&out).contains("\"cursor\"") { break; }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    #[test]
    fn server_serves_page_stream_and_rejects() {
        let v = Viewer::start("127.0.0.1".parse().unwrap(), 0, None, 60).unwrap();
        let addr: SocketAddr = format!("127.0.0.1:{}", v.port).parse().unwrap();
        let t = v.token.clone();
        let page = get(addr, &format!("GET /{t}/ HTTP/1.1\r\nHost: x\r\n\r\n"));
        assert!(page.starts_with("HTTP/1.1 200") && page.contains("EventSource"));
        assert!(get(addr, "GET /nope/ HTTP/1.1\r\n\r\n").starts_with("HTTP/1.1 404"));
        assert!(get(addr, &format!("POST /{t}/ HTTP/1.1\r\n\r\n")).starts_with("HTTP/1.1 405"));
        v.shared.publish(Arc::new(demo_buffer(0, 20, 5)), Some((1, 1)));
        let ev = get(addr, &format!("GET /{t}/events HTTP/1.1\r\n\r\n"));
        assert!(ev.contains("text/event-stream") && ev.contains("\"hello\"") && ev.contains("\"full\""), "{ev}");
        v.stop();
    }

    #[test]
    fn manager_start_status_stop_cycle() {
        assert_eq!(status(), "off");
        assert!(!active());
        let url = start(Some("127.0.0.1"), 0, 15, 0).unwrap();
        assert!(url.starts_with("http://127.0.0.1:") && url.ends_with('/'));
        assert!(active() && take_want_full() && !take_want_full(), "one forced full frame per start");
        assert_eq!(start(Some("127.0.0.1"), 0, 15, 0).unwrap(), url, "a second start returns the same URL");
        assert!(status().starts_with("casting"));
        assert!(stop() && !stop());
        assert!(!active());
        assert_eq!(status(), "off");
    }

    #[test]
    fn viewer_cap_is_enforced() {
        let v = Viewer::start("127.0.0.1".parse().unwrap(), 0, None, 60).unwrap();
        v.shared.publish(Arc::new(CellBuffer::new(2, 2)), None);
        let addr: SocketAddr = format!("127.0.0.1:{}", v.port).parse().unwrap();
        let req = format!("GET /{}/events HTTP/1.1\r\n\r\n", v.token);
        let mut held = Vec::new();
        for _ in 0..MAX_VIEWERS {
            let mut s = TcpStream::connect(addr).unwrap();
            s.write_all(req.as_bytes()).unwrap();
            held.push(s);
        }
        let t0 = Instant::now();
        while v.shared.viewers() < MAX_VIEWERS && t0.elapsed() < Duration::from_secs(3) { std::thread::sleep(Duration::from_millis(20)); }
        assert!(get(addr, &req).starts_with("HTTP/1.1 503"));
        v.stop();
    }
}
