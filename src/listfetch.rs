//! Store > Update list (WMCAT-004): download the catalog from the versioned
//! lists location for this tetron-wm's core minor and, when it passes the
//! catalog checks, use and keep it ([`crate::catalog::install`]). Runs only on
//! that press, on a worker thread through [`crate::system::run_capped`].

use std::sync::mpsc;

/// The published lists, one directory per core minor (`<base>/0.15/`).
/// `TETRON_WM_LISTS_URL` overrides it (tests serve a local copy).
pub const LISTS_BASE: &str = "https://tetronos.github.io/apt/lists";

/// Cap for the download (seconds).
const FETCH_SECS: u64 = 30;

/// `major.minor` of a version string ("0.15.3" gives "0.15").
pub fn minor_of(version: &str) -> String {
    version.split('.').take(2).collect::<Vec<_>>().join(".")
}

/// The catalog URL for this build: `<base>/<major.minor>/catalog.json`.
pub fn catalog_url(base: &str) -> String {
    format!("{}/{}/catalog.json", base.trim_end_matches('/'), minor_of(env!("CARGO_PKG_VERSION")))
}

fn base() -> String {
    std::env::var("TETRON_WM_LISTS_URL").ok().filter(|v| !v.is_empty()).unwrap_or_else(|| LISTS_BASE.into())
}

/// The line the Store shows for a fetch result.
pub fn outcome(fetched: Option<String>, install: impl FnOnce(&str) -> Result<usize, String>) -> String {
    match fetched {
        None => "Update list: download failed; the list in use stays".into(),
        Some(body) => match install(&body) {
            Ok(n) => format!("List updated: {n} apps"),
            Err(e) => format!("Update list: not used ({e}); the list in use stays"),
        },
    }
}

/// Download, check and install; returns the line to show.
pub fn run() -> String {
    let url = catalog_url(&base());
    crate::dbg_log(&format!("catalog: update list from {url}"));
    let got = crate::system::run_capped("curl", &["-fsSL", "--max-time", "25", "-H", "User-Agent: tetron-wm", &url], FETCH_SECS);
    let msg = outcome(got, crate::catalog::install);
    crate::dbg_log(&format!("catalog: {msg}"));
    msg
}

/// Start [`run`] on a worker thread; the receiver gets its one line.
pub fn spawn() -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(run());
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_is_keyed_by_the_core_minor_never_a_branch() {
        assert_eq!(minor_of("0.15.3"), "0.15");
        assert_eq!(minor_of("1.2"), "1.2");
        let u = catalog_url("https://example.org/lists/");
        assert_eq!(u, format!("https://example.org/lists/{}/catalog.json", minor_of(env!("CARGO_PKG_VERSION"))));
        assert!(!catalog_url(LISTS_BASE).contains("/main/"));
    }

    #[test]
    fn outcome_keeps_the_list_in_use_on_any_failure() {
        assert!(outcome(None, |_| Ok(1)).contains("download failed"));
        let bad = outcome(Some("{}".into()), crate::catalog::parse_count);
        assert!(bad.contains("not used") && bad.contains("the list in use stays"), "{bad}");
        assert_eq!(outcome(Some("x".into()), |_| Ok(688)), "List updated: 688 apps");
    }
}
