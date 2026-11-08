//! Settings > Appearance > Get more themes: download the theme files that are
//! in the repository's `themes/` directory on `main` and missing from the
//! download dir ([`crate::theme::download_dir`]).
//!
//! The directory itself is the list: one GitHub contents API request returns
//! every file with its `download_url`, so a theme pushed to `main` is offered at
//! once and no index has to be kept. Only theme files are fetched (see
//! [`wanted`]); a README or script in `themes/` never is. Everything runs on a
//! worker thread through [`crate::system::run_capped`], never the render loop.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

/// Extensions fetched: the palette and the pictures it can name.
const EXTS: &[&str] = &["toml", "jpg", "jpeg", "png", "webp"];

/// Cap for the listing request (seconds).
const LIST_SECS: u64 = 15;
/// Cap for one file. No size cap: pictures for large screens can be big.
const FILE_SECS: u64 = 300;

/// One file in the `themes/` listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub url: String,
}

/// A message from the worker: progress text, then one final `Done`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Msg {
    Progress(String),
    Done(String),
}

/// Whether `name` is a theme file worth fetching: a plain file name (no path,
/// not hidden) with one of [`EXTS`], any case.
pub fn wanted(name: &str) -> bool {
    if name.is_empty() || name.starts_with('.') || name.contains(['/', '\\']) {
        return false;
    }
    match name.rsplit_once('.') {
        Some((_, ext)) => EXTS.iter().any(|e| e.eq_ignore_ascii_case(ext)),
        None => false,
    }
}

/// Parse a GitHub contents API listing into the wanted files. `Err` when the
/// body is not a listing (an error object, a truncated download).
pub fn parse_listing(body: &str) -> Result<Vec<Entry>, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| format!("bad listing: {e}"))?;
    let arr = v.as_array().ok_or_else(|| {
        let msg = v.get("message").and_then(|m| m.as_str()).unwrap_or("not a directory listing");
        format!("bad listing: {msg}")
    })?;
    Ok(arr
        .iter()
        .filter(|f| f.get("type").and_then(|t| t.as_str()) == Some("file"))
        .filter_map(|f| {
            let name = f.get("name")?.as_str()?;
            let url = f.get("download_url")?.as_str()?;
            (wanted(name) && url.starts_with("https://")).then(|| Entry { name: name.into(), url: url.into() })
        })
        .collect())
}

/// The entries not already present in `dir`.
pub fn missing(entries: Vec<Entry>, dir: &Path) -> Vec<Entry> {
    entries.into_iter().filter(|e| !dir.join(&e.name).exists()).collect()
}

/// Split curl output made with `-w '\n%{http_code}'` into (status, body).
fn split_status(out: &str) -> (u16, &str) {
    match out.rsplit_once('\n') {
        Some((body, code)) => (code.trim().parse().unwrap_or(0), body),
        None => (out.trim().parse().unwrap_or(0), ""),
    }
}

/// The message for a listing that did not come back with 200.
pub fn listing_error(status: u16) -> String {
    match status {
        0 => "Could not reach GitHub (offline?)".into(),
        403 | 429 => "GitHub rate limit, try again later".into(),
        404 => "No themes directory found upstream".into(),
        s => format!("GitHub answered HTTP {s}"),
    }
}

/// The contents API URL of `themes/` on `main` for `repo` (`owner/name`).
pub fn listing_url(repo: &str) -> String {
    format!("https://api.github.com/repos/{repo}/contents/themes?ref=main")
}

/// Fetch the listing and every missing file into `dir`, reporting through `tx`.
/// Each file is written to `<name>.part` and renamed, so a cut download never
/// leaves a half file under the real name. Returns the final message.
pub fn run(repo: &str, dir: &Path, tx: &mpsc::Sender<Msg>) -> String {
    let say = |s: String| {
        crate::dbg_log(&format!("themes: {s}"));
        let _ = tx.send(Msg::Progress(s));
    };
    say("Listing themes on GitHub".into());
    let url = listing_url(repo);
    let Some(out) = crate::system::run_capped(
        "curl",
        &["-sS", "-L", "--max-time", "12", "-H", "User-Agent: tetron-wm", "-H", "Accept: application/vnd.github+json", "-w", "\n%{http_code}", &url],
        LIST_SECS,
    ) else {
        return listing_error(0);
    };
    let (status, body) = split_status(&out);
    if status != 200 {
        return listing_error(status);
    }
    let all = match parse_listing(body) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let total = all.len();
    let todo = missing(all, dir);
    if todo.is_empty() {
        return format!("Up to date: all {total} theme files present");
    }
    if let Err(e) = std::fs::create_dir_all(dir) {
        return format!("Could not create {}: {e}", dir.display());
    }
    let (n, mut got, mut failed) = (todo.len(), 0, Vec::new());
    for (i, e) in todo.iter().enumerate() {
        say(format!("Downloading {} of {n}: {}", i + 1, e.name));
        let fin = dir.join(&e.name);
        let part = part_path(&fin);
        let part_s = part.to_string_lossy();
        let ok = crate::system::run_capped(
            "curl",
            &["-fsSL", "--max-time", &FILE_SECS.to_string(), "-H", "User-Agent: tetron-wm", "-o", &part_s, &e.url],
            FILE_SECS + 5,
        )
        .is_some()
            && std::fs::rename(&part, &fin).is_ok();
        if ok {
            got += 1;
        } else {
            let _ = std::fs::remove_file(&part);
            crate::log_warn(&format!("themes: {} failed from {}", e.name, e.url));
            failed.push(e.name.clone());
        }
    }
    if failed.is_empty() {
        format!("Got {got} new files")
    } else {
        format!("Got {got} of {n}; failed: {}", failed.join(", "))
    }
}

/// `<file>.part` next to `fin`.
fn part_path(fin: &Path) -> PathBuf {
    let mut s = fin.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

/// Start [`run`] on a worker thread into the download dir; read the returned
/// receiver once per tick.
pub fn spawn() -> mpsc::Receiver<Msg> {
    let (tx, rx) = mpsc::channel();
    let repo = crate::REPO_URL.trim_start_matches("https://github.com/").to_string();
    std::thread::spawn(move || {
        let msg = match crate::theme::download_dir() {
            Some(dir) => run(&repo, &dir, &tx),
            None => "No data directory (no HOME and no XDG_DATA_HOME)".into(),
        };
        crate::dbg_log(&format!("themes: {msg}"));
        let _ = tx.send(Msg::Done(msg));
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wanted_takes_theme_files_only() {
        for n in ["nord.toml", "a.jpg", "b.JPEG", "c.png", "d.webp"] {
            assert!(wanted(n), "{n}");
        }
        for n in ["README.md", "make.sh", "noext", ".hidden.toml", "x/y.toml", "", "toml"] {
            assert!(!wanted(n), "{n}");
        }
    }

    #[test]
    fn listing_parses_files_with_a_download_url() {
        let body = r#"[
          {"name":"nord.toml","type":"file","download_url":"https://raw.githubusercontent.com/o/r/main/themes/nord.toml"},
          {"name":"nord.jpg","type":"file","download_url":"https://raw.githubusercontent.com/o/r/main/themes/nord.jpg"},
          {"name":"README.md","type":"file","download_url":"https://raw.githubusercontent.com/o/r/main/themes/README.md"},
          {"name":"sub","type":"dir","download_url":null},
          {"name":"odd.toml","type":"file","download_url":"http://insecure/odd.toml"}
        ]"#;
        let got = parse_listing(body).unwrap();
        assert_eq!(got.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["nord.toml", "nord.jpg"]);
        assert!(got[0].url.ends_with("/themes/nord.toml"));
    }

    #[test]
    fn listing_error_object_is_an_error() {
        let e = parse_listing(r#"{"message":"Not Found"}"#).unwrap_err();
        assert!(e.contains("Not Found"), "{e}");
        assert!(parse_listing("<html>").is_err());
    }

    #[test]
    fn rate_limit_has_its_own_message() {
        assert_eq!(listing_error(403), "GitHub rate limit, try again later");
        assert!(listing_error(0).contains("offline"));
    }

    #[test]
    fn status_split_from_curl_write_out() {
        assert_eq!(split_status("[1]\n200"), (200, "[1]"));
        assert_eq!(split_status("{\"m\":1}\n403"), (403, "{\"m\":1}"));
        assert_eq!(split_status("000"), (0, ""));
    }

    #[test]
    fn missing_skips_files_already_downloaded() {
        let dir = std::env::temp_dir().join(format!("tetron-wm-themefetch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("nord.toml"), "x").unwrap();
        let e = |n: &str| Entry { name: n.into(), url: format!("https://h/{n}") };
        let left = missing(vec![e("nord.toml"), e("nord.jpg")], &dir);
        assert_eq!(left, vec![e("nord.jpg")]);
        assert_eq!(part_path(&dir.join("a.jpg")), dir.join("a.jpg.part"));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Live: one listing request to GitHub plus the downloads. Run by hand with
    /// `cargo test -- --ignored live_fetch` (uses 1 of 60 hourly API requests).
    #[test]
    #[ignore]
    fn live_fetch_downloads_then_is_up_to_date() {
        let dir = std::env::temp_dir().join(format!("tetron-wm-themefetch-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let repo = crate::REPO_URL.trim_start_matches("https://github.com/");
        let (tx, rx) = mpsc::channel();
        let msg = run(repo, &dir, &tx);
        println!("{msg}");
        for m in rx.try_iter() {
            println!("{m:?}");
        }
        assert!(msg.starts_with("Got"), "{msg}");
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().into_string().unwrap()).collect();
        println!("{names:?}");
        assert!(names.iter().any(|n| n.ends_with(".toml")));
        assert!(!names.iter().any(|n| n.ends_with(".part")));
        std::fs::remove_dir_all(&dir).ok();
    }
}
