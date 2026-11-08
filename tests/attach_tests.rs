//! End-to-end `attach -d` takeover and named sessions: real daemons in a temp
//! `XDG_RUNTIME_DIR`, driven by raw socket clients (no terminal needed).

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_tetron-wm");

struct Env {
    root: PathBuf,
}

impl Env {
    fn new(tag: &str) -> Env {
        let root = std::env::temp_dir().join(format!("twm-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Env { root }
    }

    fn cmd(&self, session: &str) -> Command {
        let mut c = Command::new(BIN);
        c.env("XDG_RUNTIME_DIR", &self.root)
            .env("HOME", &self.root)
            .env("XDG_CONFIG_HOME", self.root.join("cfg"))
            .env("XDG_DATA_HOME", self.root.join("data"))
            .env("TETRON_WM_SESSION", session)
            .stdin(Stdio::null());
        c
    }

    fn sock(&self, session: &str) -> PathBuf {
        let base = self.root.join(format!("tetron-wm-{}", std::env::var("USER").unwrap_or_else(|_| "user".into())));
        if session == "default" {
            base.join("daemon.sock")
        } else {
            base.join("sessions").join(session).join("daemon.sock")
        }
    }

    fn start(&self, session: &str) {
        // The daemon runs until `kill`; keep the handle so the child is reaped.
        let child = self.cmd(session).arg("--daemon").stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        std::mem::forget(child);
        let sock = self.sock(session);
        let t = Instant::now();
        while UnixStream::connect(&sock).is_err() {
            assert!(t.elapsed() < Duration::from_secs(10), "daemon {session} did not start");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn ls(&self) -> String {
        let out = self.cmd("default").arg("ls").output().unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn kill(&self, session: &str) {
        let _ = self.cmd(session).arg("kill").output();
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// The STATE column of `name` in `ls` output ("" if not listed).
fn state_of(ls: &str, name: &str) -> String {
    ls.lines()
        .find(|l| l.split_whitespace().next() == Some(name))
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap_or("")
        .to_string()
}

fn attach(sock: &Path, takeover: bool) -> BufReader<UnixStream> {
    let mut s = UnixStream::connect(sock).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    writeln!(s, r#"{{"Resize":{{"w":80,"h":24,"takeover":{takeover}}}}}"#).unwrap();
    BufReader::new(s)
}

/// Read frames until one carries a `notice`, or EOF / timeout. Returns the notice.
fn read_notice(r: &mut BufReader<UnixStream>) -> Option<String> {
    let mut line = String::new();
    loop {
        line.clear();
        match r.read_line(&mut line) {
            Ok(0) | Err(_) => return None,
            Ok(_) => {
                let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
                if let Some(n) = v["notice"].as_str() {
                    return Some(n.to_string());
                }
            }
        }
    }
}

fn expect_frame(r: &mut BufReader<UnixStream>) {
    let mut line = String::new();
    assert!(r.read_line(&mut line).unwrap() > 0, "expected a frame");
    let v: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    assert!(v.get("flags").is_some());
}

#[test]
fn second_attach_refused_and_dash_d_takes_over() {
    let env = Env::new("take");
    env.start("t1");
    let sock = env.sock("t1");

    let mut a = attach(&sock, false);
    expect_frame(&mut a);

    // Plain attach while A is attached: refused with a hint, A unaffected.
    let mut b = attach(&sock, false);
    let n = read_notice(&mut b).expect("refusal notice");
    assert!(n.contains("attach -d"), "{n}");
    expect_frame(&mut a);

    // attach -d: A is evicted with a notice, C is served.
    let mut c = attach(&sock, true);
    let n = read_notice(&mut a).expect("eviction notice");
    assert!(n.contains("another client"), "{n}");
    expect_frame(&mut c);

    env.kill("t1");
}

#[test]
fn named_sessions_are_independent_and_listed() {
    let env = Env::new("named");
    env.start("s1");
    env.start("s2");
    assert_ne!(env.sock("s1"), env.sock("s2"));

    // Both can hold a client at once (different daemons).
    let mut a = attach(&env.sock("s1"), false);
    let mut b = attach(&env.sock("s2"), false);
    expect_frame(&mut a);
    expect_frame(&mut b);

    let text = env.ls();
    assert!(state_of(&text, "s1") == "attached" && state_of(&text, "s2") == "attached", "{text}");

    // Killing s1 leaves s2 alive.
    env.kill("s1");
    std::thread::sleep(Duration::from_millis(500));
    assert!(UnixStream::connect(env.sock("s1")).is_err());
    assert!(UnixStream::connect(env.sock("s2")).is_ok());
    env.kill("s2");
}

#[test]
fn invalid_session_name_is_rejected() {
    let env = Env::new("bad");
    let out = env.cmd("default").args(["-s", "a/b", "ls"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn ls_shows_detached_after_client_leaves_recoverable_after_daemon_exit_and_nothing_after_kill() {
    let env = Env::new("states");
    env.start("w");
    let mut a = attach(&env.sock("w"), false);
    expect_frame(&mut a);
    assert_eq!(state_of(&env.ls(), "w"), "attached");

    // Launcher > Exit equivalent: the client goes away, the session lives on.
    drop(a);
    let t = Instant::now();
    while state_of(&env.ls(), "w") != "detached" {
        assert!(t.elapsed() < Duration::from_secs(5), "never detached: {}", env.ls());
        std::thread::sleep(Duration::from_millis(100));
    }
    let text = env.ls();
    assert!(text.lines().any(|l| l.starts_with('w') && l.trim_end().ends_with('0')), "{text}");

    // Daemon gone, apphost alive: recoverable.
    let _ = env.cmd("w").arg("reload").output();
    let t = Instant::now();
    while state_of(&env.ls(), "w") != "recoverable" {
        assert!(t.elapsed() < Duration::from_secs(5), "never recoverable: {}", env.ls());
        std::thread::sleep(Duration::from_millis(100));
    }

    // Clean stop removes the sockets: not listed any more.
    env.kill("w");
    let t = Instant::now();
    while !state_of(&env.ls(), "w").is_empty() {
        assert!(t.elapsed() < Duration::from_secs(5), "still listed: {}", env.ls());
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn screensaver_command_lists_effects_and_rejects_bad_input() {
    let env = Env::new("saverargs");
    let out = env.cmd("default").args(["screensaver", "list"]).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success() && text.contains("black") && text.contains("clock"), "{text}");
    let bad_effect = env.cmd("default").args(["screensaver", "on", "nope"]).output().unwrap();
    assert_eq!(bad_effect.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&bad_effect.stderr).contains("black, clock"));
    let bad_mode = env.cmd("default").args(["screensaver", "sideways"]).output().unwrap();
    assert_eq!(bad_mode.status.code(), Some(2));
}

#[test]
fn screensaver_starts_over_the_control_socket_and_a_key_wakes_it() {
    let env = Env::new("saverwake");
    env.start("sv");
    let sock = env.sock("sv");
    let mut client = attach(&sock, false);
    expect_frame(&mut client);
    // Like the real client, keep reading frames: a full repaint is far bigger than
    // the socket buffer, so a client that stops reading stalls the daemon.
    let mut writer = client.get_ref().try_clone().unwrap();
    client.get_ref().set_read_timeout(None).unwrap();
    std::thread::spawn(move || {
        let mut line = String::new();
        while matches!(client.read_line(&mut line), Ok(n) if n > 0) {
            line.clear();
        }
    });

    let out = env.cmd("sv").args(["screensaver", "on", "clock"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let log_path = env.root.join(".local/state/tetron-wm/debug.log");
    let wait_for = |needle: &str| {
        let t = Instant::now();
        loop {
            let log = std::fs::read_to_string(&log_path).unwrap_or_default();
            if log.contains(needle) {
                return;
            }
            assert!(t.elapsed() < Duration::from_secs(5), "log never said {needle:?}:\n{log}");
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    wait_for("screensaver: on (clock)");

    // A key from the attached client ends it.
    writeln!(writer, r#"{{"Key":[97]}}"#).unwrap();
    wait_for("screensaver: off");
    env.kill("sv");
}
