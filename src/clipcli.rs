//! Command-line access to the tetron-wm clipboard: `tetron-wm clip ...` and
//! busybox-style shims that make `xclip`, `xsel`, `wl-copy` and `wl-paste` talk
//! to it when there is no X11/Wayland (see `clipboard_shims`). All requests go
//! over the daemon control socket, which answers with one JSON line.

use crate::clipboard::{ClipCmd, ClipReply};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

/// Ask the daemon. `Err` text is ready to print.
pub fn request(cmd: ClipCmd) -> Result<ClipReply, String> {
    let mut s = UnixStream::connect(crate::protocol::daemon_ctl_path())
        .map_err(|_| "tetron-wm daemon is not running (start it with `tetron-wm`)".to_string())?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(3)));
    let mut line = serde_json::to_vec(&crate::session::ClientMsg::Clip(cmd)).map_err(|e| e.to_string())?;
    line.push(b'\n');
    s.write_all(&line).map_err(|e| e.to_string())?;
    let mut reply = String::new();
    BufReader::new(s).read_line(&mut reply).map_err(|e| e.to_string())?;
    if reply.trim().is_empty() {
        return Err("no reply from the daemon (it may predate the clipboard; run `tetron-wm reload`)".into());
    }
    let r: ClipReply = serde_json::from_str(reply.trim()).map_err(|e| e.to_string())?;
    if r.ok { Ok(r) } else { Err(r.error) }
}

fn read_stdin() -> String {
    let mut buf = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

fn stdin_is_tty() -> bool {
    // SAFETY-free check without libc: /dev/stdin points at a tty device.
    std::fs::read_link("/proc/self/fd/0").map(|p| p.to_string_lossy().starts_with("/dev/pts") || p.to_string_lossy().starts_with("/dev/tty")).unwrap_or(false)
}

fn out(text: &str) {
    let mut o = std::io::stdout();
    let _ = o.write_all(text.as_bytes());
    let _ = o.flush();
}

// ── `tetron-wm clip` ────────────────────────────────────────────────────────

const CLIP_USAGE: &str = "usage: tetron-wm clip <command>\n\
  copy [--primary] [text...]   set the clipboard from text or stdin\n\
  copy-file PATH               set the clipboard from a file\n\
  paste [--primary] [-n N]     print the clipboard (or history entry N, 0 = newest)\n\
  list [--json]                show the history\n\
  clear                        empty the history\n\
  pin N | delete N             pin / delete history entry N\n\
  setup-mc                     make mcedit use this clipboard (~/.config/mc/ini)";

fn preview(text: &str, n: usize) -> String {
    let one: String = text.chars().map(|c| if c == '\n' { crate::glyphs::ch(crate::glyphs::Glyph::Newline) } else { c }).collect();
    if one.chars().count() > n { format!("{}…", one.chars().take(n).collect::<String>()) } else { one }
}

/// `tetron-wm clip ...`; returns the process exit code.
pub fn run_clip(args: &[String]) -> i32 {
    if args.first().map(String::as_str) == Some("setup-mc") {
        return setup_mc();
    }
    let primary = args.iter().any(|a| a == "--primary" || a == "-p");
    let rest: Vec<&String> = args.iter().skip(1).filter(|a| *a != "--primary" && *a != "-p").collect();
    let cmd = match args.first().map(String::as_str) {
        // mc runs `<clipboard_store> <tmpfile>`: copy from the named file.
        Some("copy-file") => match rest.first() {
            Some(path) => match std::fs::read_to_string(path.as_str()) {
                Ok(text) => ClipCmd::Copy { text, primary },
                Err(e) => {
                    eprintln!("tetron-wm clip copy-file: {path}: {e}");
                    return 1;
                }
            },
            None => {
                eprintln!("tetron-wm clip copy-file: needs a file");
                return 2;
            }
        },
        Some("copy") => {
            let text = if rest.is_empty() { read_stdin() } else { rest.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" ") };
            ClipCmd::Copy { text, primary }
        }
        Some("paste") => {
            let mut index = None;
            let mut it = rest.iter();
            while let Some(a) = it.next() {
                if a.as_str() == "-n" {
                    index = it.next().and_then(|v| v.parse().ok());
                    if index.is_none() {
                        eprintln!("tetron-wm clip paste: -n needs a number");
                        return 2;
                    }
                }
            }
            ClipCmd::Paste { primary, index }
        }
        Some("list") => ClipCmd::List,
        Some("clear") => ClipCmd::Clear,
        Some(c @ ("pin" | "delete")) => match rest.first().and_then(|v| v.parse::<usize>().ok()) {
            Some(index) if c == "pin" => ClipCmd::Pin { index },
            Some(index) => ClipCmd::Delete { index },
            None => {
                eprintln!("tetron-wm clip {c}: needs an entry number (see `tetron-wm clip list`)");
                return 2;
            }
        },
        _ => {
            eprintln!("{CLIP_USAGE}");
            return 2;
        }
    };
    let is_list = matches!(cmd, ClipCmd::List);
    match request(cmd) {
        Ok(r) if is_list => {
            if args.iter().any(|a| a == "--json") {
                println!("{}", serde_json::to_string_pretty(&r.entries).unwrap_or_default());
            } else {
                for (i, e) in r.entries.iter().enumerate() {
                    println!("{i:>3} {} {:<4} {}", if e.pinned { '*' } else { ' ' }, e.source.tag(), preview(&e.text, 70));
                }
            }
            0
        }
        Ok(r) => {
            out(&r.text);
            0
        }
        Err(e) => {
            eprintln!("tetron-wm clip: {e}");
            1
        }
    }
}

// ── Midnight Commander (mcedit) setup ───────────────────────────────────────

/// mc keeps its editor clipboard in a private file unless `clipboard_store` and
/// `clipboard_paste` (in the `[Misc]` section of `~/.config/mc/ini`) name
/// commands. Returns the ini text with both pointing at `tetron-wm clip`, or
/// `None` when they are already set to something.
pub fn mc_ini_with_clip(ini: &str, exe: &str) -> Option<String> {
    let has = |key: &str| ini.lines().any(|l| l.trim_start().starts_with(&format!("{key}=")) && l.split_once('=').is_some_and(|(_, v)| !v.trim().is_empty()));
    if has("clipboard_store") || has("clipboard_paste") {
        return None;
    }
    // mc runs these through a shell: quote an executable path with odd characters.
    let exe = if exe.chars().all(|c| c.is_ascii_alphanumeric() || "_./-".contains(c)) {
        exe.to_string()
    } else {
        crate::systems::sh_quote(exe)
    };
    let add = format!("clipboard_store={exe} clip copy-file\nclipboard_paste={exe} clip paste\n");
    let mut out = String::new();
    let mut done = false;
    for line in ini.lines() {
        // Drop any empty values we are about to replace.
        let t = line.trim_start();
        if t.starts_with("clipboard_store=") || t.starts_with("clipboard_paste=") {
            continue;
        }
        out.push_str(line);
        out.push('\n');
        if !done && line.trim() == "[Misc]" {
            out.push_str(&add);
            done = true;
        }
    }
    if !done {
        if !out.is_empty() && !out.ends_with("\n\n") {
            out.push('\n');
        }
        out.push_str("[Misc]\n");
        out.push_str(&add);
    }
    Some(out)
}

/// `tetron-wm clip setup-mc`: point mcedit's clipboard at tetron-wm.
fn setup_mc() -> i32 {
    let Some(home) = dirs::home_dir() else {
        eprintln!("tetron-wm clip setup-mc: no home directory");
        return 1;
    };
    let path = home.join(".config/mc/ini");
    // Written to mc.ini and used later: a stable path (WMNIX-003).
    let exe = crate::paths::stable_self_path().to_str().map(String::from).unwrap_or_else(|| "tetron-wm".into());
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    match mc_ini_with_clip(&old, &exe) {
        None => {
            println!("mc: clipboard_store / clipboard_paste already set in {} (left alone)", path.display());
            0
        }
        Some(new) => {
            if path.exists() {
                let _ = std::fs::copy(&path, path.with_extension("bak-tetron-wm"));
            }
            if let Some(d) = path.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            match std::fs::write(&path, new) {
                Ok(()) => {
                    println!("mc: mcedit copy/paste now use the tetron-wm clipboard ({})", path.display());
                    0
                }
                Err(e) => {
                    eprintln!("tetron-wm clip setup-mc: {e}");
                    1
                }
            }
        }
    }
}

// ── xclip / xsel / wl-copy / wl-paste shims ─────────────────────────────────

#[derive(Debug, PartialEq)]
pub enum Action {
    /// Set the clipboard. `text: None` = read stdin (or `files`).
    Copy { primary: bool, text: Option<String>, files: Vec<String>, filter: bool, strip_nl: bool, append: bool },
    Paste { primary: bool, strip_nl: bool },
    Clear { primary: bool },
    /// `wl-paste --list-types` / `xclip -t TARGETS`.
    ListTypes,
    Print(String),
    Error(String),
}

/// The shim names recognised through `argv[0]`.
pub const SHIMS: [&str; 4] = ["xclip", "xsel", "wl-copy", "wl-paste"];

fn text_target(t: &str) -> bool {
    let t = t.to_ascii_lowercase();
    t.starts_with("text/") || matches!(t.as_str(), "utf8_string" | "string" | "text" | "compound_text")
}

fn xclip_selection(v: &str) -> bool {
    // true = primary-like. xclip default is primary; "clipboard" is the other one.
    !v.to_ascii_lowercase().starts_with('c')
}

pub fn parse_xclip(args: &[String]) -> Action {
    let (mut primary, mut paste, mut filter, mut strip_nl) = (true, false, false, false);
    let mut files = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-i" | "-in" => paste = false,
            "-o" | "-out" => paste = true,
            "-f" | "-filter" => filter = true,
            "-r" | "-rmlastnl" => strip_nl = true,
            "-quiet" | "-silent" | "-verbose" | "-noutf8" => {}
            "-version" => return Action::Print("xclip version 0.13 (tetron-wm shim)\n".into()),
            "-h" | "-help" => return Action::Print("Usage: xclip [-i|-o] [-selection primary|clipboard] [-f] [-r] [file...]\n(tetron-wm shim)\n".into()),
            "-l" | "-loops" | "-d" | "-display" => {
                it.next();
            }
            "-t" | "-target" => match it.next() {
                Some(t) if t == "TARGETS" => return Action::ListTypes,
                Some(t) if text_target(t) => {}
                Some(t) => return Action::Error(format!("Error: target {t} not available (text only)")),
                None => return Action::Error("Error: -t needs an argument".into()),
            },
            s if s.starts_with("-sel") => match it.next() {
                Some(v) => primary = xclip_selection(v),
                None => return Action::Error("Error: -selection needs an argument".into()),
            },
            s if s.starts_with('-') && s.len() > 1 => return Action::Error(format!("xclip: unrecognised option {s}")),
            f => files.push(f.to_string()),
        }
    }
    if paste {
        Action::Paste { primary, strip_nl: false }
    } else {
        Action::Copy { primary, text: None, files, filter, strip_nl, append: false }
    }
}

pub fn parse_xsel(args: &[String]) -> Action {
    let (mut primary, mut op, mut append) = (true, ' ', false);
    for a in args {
        if let Some(long) = a.strip_prefix("--") {
            match long {
                "input" => op = 'i',
                "output" => op = 'o',
                "append" => { op = 'i'; append = true }
                "clear" | "delete" => op = 'c',
                "primary" | "secondary" => primary = true,
                "clipboard" => primary = false,
                "keep" | "nodetach" | "follow" | "exchange" => {}
                "version" => return Action::Print("xsel version 1.2.0 (tetron-wm shim)\n".into()),
                "help" => return Action::Print("Usage: xsel [-i|-o|-c] [-p|-b]\n(tetron-wm shim)\n".into()),
                other => return Action::Error(format!("xsel: unrecognised option --{other}")),
            }
        } else if let Some(shorts) = a.strip_prefix('-') {
            for c in shorts.chars() {
                match c {
                    'i' => op = 'i',
                    'o' => op = 'o',
                    'a' => { op = 'i'; append = true }
                    'c' | 'd' => op = 'c',
                    'p' | 's' => primary = true,
                    'b' => primary = false,
                    'k' | 'n' | 'x' | 'f' => {}
                    _ => return Action::Error(format!("xsel: unrecognised option -{c}")),
                }
            }
        }
    }
    let op = if op == ' ' { if stdin_is_tty() { 'o' } else { 'i' } } else { op };
    match op {
        'o' => Action::Paste { primary, strip_nl: false },
        'c' => Action::Clear { primary },
        _ => Action::Copy { primary, text: None, files: vec![], filter: false, strip_nl: false, append },
    }
}

pub fn parse_wl_copy(args: &[String]) -> Action {
    let (mut primary, mut clear, mut trim) = (false, false, false);
    let mut text: Vec<String> = Vec::new();
    let mut it = args.iter();
    let mut rest_is_text = false;
    while let Some(a) = it.next() {
        if rest_is_text {
            text.push(a.clone());
            continue;
        }
        match a.as_str() {
            "--" => rest_is_text = true,
            "-p" | "--primary" => primary = true,
            "-c" | "--clear" => clear = true,
            "-n" | "--trim-newline" => trim = true,
            "-o" | "--paste-once" | "-f" | "--foreground" => {}
            "-s" | "--seat" => {
                it.next();
            }
            "-t" | "--type" => match it.next() {
                Some(t) if text_target(t) => {}
                Some(t) => return Action::Error(format!("wl-copy: unsupported type {t} (text only)")),
                None => return Action::Error("wl-copy: --type needs an argument".into()),
            },
            "-v" | "--version" => return Action::Print("wl-clipboard 2.2.1 (tetron-wm shim)\n".into()),
            "-h" | "--help" => return Action::Print("Usage: wl-copy [-p] [-c] [-n] [text...]\n(tetron-wm shim)\n".into()),
            s if s.starts_with('-') && s.len() > 1 => return Action::Error(format!("wl-copy: unrecognised option {s}")),
            t => text.push(t.to_string()),
        }
    }
    if clear {
        return Action::Clear { primary };
    }
    let given = if text.is_empty() { None } else { Some(text.join(" ")) };
    Action::Copy { primary, text: given, files: vec![], filter: false, strip_nl: trim, append: false }
}

pub fn parse_wl_paste(args: &[String]) -> Action {
    let (mut primary, mut no_nl, mut list) = (false, false, false);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-p" | "--primary" => primary = true,
            "-n" | "--no-newline" => no_nl = true,
            "-l" | "--list-types" => list = true,
            "-w" | "--watch" => return Action::Error("wl-paste --watch is not supported by the tetron-wm shim".into()),
            "-s" | "--seat" => {
                it.next();
            }
            "-t" | "--type" => match it.next() {
                Some(t) if text_target(t) => {}
                Some(t) => return Action::Error(format!("wl-paste: unsupported type {t} (text only)")),
                None => return Action::Error("wl-paste: --type needs an argument".into()),
            },
            "-v" | "--version" => return Action::Print("wl-clipboard 2.2.1 (tetron-wm shim)\n".into()),
            "-h" | "--help" => return Action::Print("Usage: wl-paste [-p] [-n] [-l]\n(tetron-wm shim)\n".into()),
            s if s.starts_with('-') && s.len() > 1 => return Action::Error(format!("wl-paste: unrecognised option {s}")),
            _ => {}
        }
    }
    if list { Action::ListTypes } else { Action::Paste { primary, strip_nl: no_nl } }
}

/// Run a shim; returns the process exit code.
pub fn run_shim(tool: &str, args: &[String]) -> i32 {
    let action = match tool {
        "xclip" => parse_xclip(args),
        "xsel" => parse_xsel(args),
        "wl-copy" => parse_wl_copy(args),
        _ => parse_wl_paste(args),
    };
    match action {
        Action::Print(s) => {
            out(&s);
            0
        }
        Action::Error(e) => {
            eprintln!("{e}");
            1
        }
        Action::ListTypes => {
            out("text/plain;charset=utf-8\nUTF8_STRING\nSTRING\ntext/plain\n");
            0
        }
        Action::Clear { primary } => finish(tool, request(ClipCmd::Copy { text: String::new(), primary })),
        Action::Paste { primary, strip_nl } => match request(ClipCmd::Paste { primary, index: None }) {
            Ok(r) if r.text.is_empty() => {
                eprintln!("{}", if tool == "wl-paste" { "Nothing is copied" } else { "Error: target STRING not available" });
                1
            }
            Ok(r) => {
                out(if strip_nl { r.text.strip_suffix('\n').unwrap_or(&r.text) } else { &r.text });
                0
            }
            Err(e) => {
                eprintln!("{tool}: {e}");
                1
            }
        },
        Action::Copy { primary, text, files, filter, strip_nl, append } => {
            let mut body = match text {
                Some(t) => t,
                None if !files.is_empty() => files.iter().filter_map(|f| std::fs::read_to_string(f).ok()).collect::<String>(),
                None => read_stdin(),
            };
            if filter {
                out(&body);
            }
            if strip_nl {
                if let Some(s) = body.strip_suffix('\n') {
                    body = s.to_string();
                }
            }
            if append {
                if let Ok(r) = request(ClipCmd::Paste { primary, index: None }) {
                    body = format!("{}{}", r.text, body);
                }
            }
            finish(tool, request(ClipCmd::Copy { text: body, primary }))
        }
    }
}

fn finish(tool: &str, r: Result<ClipReply, String>) -> i32 {
    match r {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("{tool}: {e}");
            1
        }
    }
}

// ── Shim directory (PATH injection for launched apps) ───────────────────────

/// `~/.local/share/tetron-wm/shims`.
pub fn shims_dir() -> Option<PathBuf> {
    crate::clipboard::history_path()?.parent().map(|p| p.join("shims"))
}

/// Whether launched apps should get the shims, per `clipboard_shims`.
pub fn shims_wanted(setting: &str, display_present: bool) -> bool {
    match setting {
        "on" => true,
        "off" => false,
        _ => !display_present,
    }
}

/// Create (or refresh) the symlinks to `exe` and return the directory.
pub fn ensure_shims(exe: &std::path::Path) -> Option<PathBuf> {
    let dir = shims_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    for name in SHIMS {
        let link = dir.join(name);
        if std::fs::read_link(&link).map(|t| t == exe).unwrap_or(false) {
            continue;
        }
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(exe, &link).ok()?;
    }
    Some(dir)
}

/// What to add to a launched app's environment for the clipboard shims.
#[derive(Clone, Debug, Default)]
pub struct ShimEnv {
    /// Directory to prepend to `PATH` (shims enabled).
    pub dir: Option<PathBuf>,
    /// Also set a dummy `WAYLAND_DISPLAY` (only when the process has no display).
    pub fake_display: bool,
}

/// Whether a dummy display variable should be set.
pub fn fake_display_wanted(enabled: bool, display_present: bool) -> bool {
    enabled && !display_present
}

/// The shim environment for apps. Decided once per process (an apphost restart
/// picks up config changes).
pub fn shim_env() -> ShimEnv {
    static ENV: std::sync::OnceLock<ShimEnv> = std::sync::OnceLock::new();
    ENV.get_or_init(|| {
        let cfg = crate::config::Config::load();
        let display = std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty())
            || std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
        if !cfg.clipboard_history || !shims_wanted(&cfg.clipboard_shims, display) {
            return ShimEnv::default();
        }
        // The shim links outlive this process: a stable target (WMNIX-003).
        let dir = crate::self_exe().ok().and_then(|_| ensure_shims(&crate::paths::stable_self_path()));
        if let Some(d) = &dir {
            crate::dbg_log(&format!("clipboard: shims enabled in {}", d.display()));
        }
        ShimEnv { fake_display: dir.is_some() && fake_display_wanted(cfg.clipboard_fake_display, display), dir }
    })
    .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(s: &str) -> Vec<String> { s.split_whitespace().map(String::from).collect() }

    #[test]
    fn xclip_flags() {
        assert_eq!(parse_xclip(&a("-selection clipboard -o")), Action::Paste { primary: false, strip_nl: false });
        assert_eq!(parse_xclip(&a("-sel c -o")), Action::Paste { primary: false, strip_nl: false });
        assert_eq!(parse_xclip(&a("-o")), Action::Paste { primary: true, strip_nl: false });
        assert_eq!(
            parse_xclip(&a("-i -selection clipboard -f -r")),
            Action::Copy { primary: false, text: None, files: vec![], filter: true, strip_nl: true, append: false }
        );
        assert_eq!(parse_xclip(&a("-t TARGETS -o")), Action::ListTypes);
        assert!(matches!(parse_xclip(&a("-t image/png -o")), Action::Error(_)));
        assert!(matches!(parse_xclip(&a("-bogus")), Action::Error(_)));
        assert!(matches!(parse_xclip(&a("-selection")), Action::Error(_)));
        match parse_xclip(&a("-i f1.txt")) {
            Action::Copy { files, .. } => assert_eq!(files, vec!["f1.txt".to_string()]),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn xsel_flags() {
        assert_eq!(parse_xsel(&a("-bo")), Action::Paste { primary: false, strip_nl: false });
        assert_eq!(parse_xsel(&a("--clipboard --output")), Action::Paste { primary: false, strip_nl: false });
        assert_eq!(parse_xsel(&a("-b -i")), Action::Copy { primary: false, text: None, files: vec![], filter: false, strip_nl: false, append: false });
        assert_eq!(parse_xsel(&a("-bc")), Action::Clear { primary: false });
        assert!(matches!(parse_xsel(&a("-ba")), Action::Copy { append: true, primary: false, .. }));
        assert!(matches!(parse_xsel(&a("-Z")), Action::Error(_)));
    }

    #[test]
    fn wl_copy_and_paste_flags() {
        assert_eq!(
            parse_wl_copy(&a("-p hello world")),
            Action::Copy { primary: true, text: Some("hello world".into()), files: vec![], filter: false, strip_nl: false, append: false }
        );
        assert!(matches!(parse_wl_copy(&a("--trim-newline")), Action::Copy { text: None, strip_nl: true, .. }));
        assert_eq!(parse_wl_copy(&a("--clear")), Action::Clear { primary: false });
        assert!(matches!(parse_wl_copy(&a("--type image/png")), Action::Error(_)));
        assert_eq!(parse_wl_copy(&a("-- -n")), Action::Copy { primary: false, text: Some("-n".into()), files: vec![], filter: false, strip_nl: false, append: false });
        assert_eq!(parse_wl_paste(&a("-n")), Action::Paste { primary: false, strip_nl: true });
        assert_eq!(parse_wl_paste(&a("--primary")), Action::Paste { primary: true, strip_nl: false });
        assert_eq!(parse_wl_paste(&a("-l")), Action::ListTypes);
        assert!(matches!(parse_wl_paste(&a("--watch cat")), Action::Error(_)));
    }

    #[test]
    fn mc_ini_gets_clipboard_commands_once() {
        let ini = "[Midnight-Commander]\nverbose=1\n\n[Misc]\ntimeformat_old=%Y\nclipboard_store=\n";
        let out = mc_ini_with_clip(ini, "/usr/bin/tetron-wm").unwrap();
        assert!(out.contains("[Misc]\nclipboard_store=/usr/bin/tetron-wm clip copy-file\nclipboard_paste=/usr/bin/tetron-wm clip paste\n"));
        assert!(out.contains("timeformat_old=%Y"));
        assert_eq!(out.matches("clipboard_store=").count(), 1, "empty value replaced, not duplicated");
        assert_eq!(mc_ini_with_clip(&out, "/x"), None, "second run leaves it alone");
        let fresh = mc_ini_with_clip("", "tetron-wm").unwrap();
        assert!(fresh.starts_with("[Misc]\nclipboard_store=tetron-wm clip copy-file"));
        assert_eq!(mc_ini_with_clip("[Misc]\nclipboard_store=xclip -i\n", "x"), None, "respects a user setting");
        let spaced = mc_ini_with_clip("", "/opt/my dir/tetron-wm").unwrap();
        assert!(spaced.contains("clipboard_store='/opt/my dir/tetron-wm' clip copy-file"));
    }

    #[test]
    fn fake_display_only_without_a_real_one() {
        assert!(fake_display_wanted(true, false));
        assert!(!fake_display_wanted(true, true));
        assert!(!fake_display_wanted(false, false));
    }

    #[test]
    fn shims_policy() {
        assert!(shims_wanted("auto", false));
        assert!(!shims_wanted("auto", true));
        assert!(shims_wanted("on", true));
        assert!(!shims_wanted("off", false));
    }

    #[test]
    fn ensure_shims_makes_and_refreshes_symlinks() {
        let tmp = std::env::temp_dir().join(format!("twm-shims-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::env::set_var("XDG_DATA_HOME", &tmp);
        let dir = ensure_shims(std::path::Path::new("/bin/true")).unwrap();
        for n in SHIMS {
            assert_eq!(std::fs::read_link(dir.join(n)).unwrap(), std::path::Path::new("/bin/true"));
        }
        ensure_shims(std::path::Path::new("/bin/false")).unwrap();
        assert_eq!(std::fs::read_link(dir.join("xclip")).unwrap(), std::path::Path::new("/bin/false"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
