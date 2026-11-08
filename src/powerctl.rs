//! Power actions for the computer itself: suspend, reboot, power off.
//!
//! The init system varies (systemd, OpenRC on Alpine with or without elogind, a
//! plain busybox), so each action is an ordered chain of candidate commands and
//! the first one that SUCCEEDS wins, not merely the first that is installed: a
//! refused `systemctl` (polkit) must fall through to the next candidate.
//! `[power_commands]` in the config replaces a chain. Nothing here shells out
//! except through [`crate::system::run_capped_status`] (hard timeout).

use crate::system::{run_capped_status, Capped};
use crate::traytools::TrayTools;

/// A computer power action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Suspend,
    Reboot,
    PowerOff,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Suspend, Kind::Reboot, Kind::PowerOff];

    /// The `[power_commands]` key.
    pub fn key(self) -> &'static str {
        match self {
            Kind::Suspend => "suspend",
            Kind::Reboot => "reboot",
            Kind::PowerOff => "poweroff",
        }
    }

    /// Menu / dialog label.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Suspend => "Suspend",
            Kind::Reboot => "Reboot",
            Kind::PowerOff => "Power off",
        }
    }

    /// The launcher `@`-sentinel that opens this action's confirm dialog.
    pub fn command(self) -> &'static str {
        match self {
            Kind::Suspend => "@suspend",
            Kind::Reboot => "@reboot",
            Kind::PowerOff => "@poweroff",
        }
    }
}

/// Built-in candidates, in preference order. Linux: systemd, then elogind (also
/// `loginctl`), then privileged busybox/`doas`/`sudo` fallbacks, then the bare
/// command (works when running as root). macOS: AppleScript / `pmset`.
fn defaults(kind: Kind) -> Vec<String> {
    if cfg!(target_os = "macos") {
        return match kind {
            Kind::Suspend => vec!["pmset sleepnow".into()],
            Kind::Reboot => vec![r#"osascript -e 'tell app "System Events" to restart'"#.into()],
            Kind::PowerOff => vec![r#"osascript -e 'tell app "System Events" to shut down'"#.into()],
        };
    }
    let bare = match kind {
        Kind::Suspend => "zzz",
        Kind::Reboot => "reboot",
        Kind::PowerOff => "poweroff",
    };
    let sub = match kind {
        Kind::Suspend => "suspend",
        Kind::Reboot => "reboot",
        Kind::PowerOff => "poweroff",
    };
    vec![
        format!("systemctl {sub}"),
        format!("loginctl {sub}"),
        format!("sudo -n {bare}"),
        format!("doas -n {bare}"),
        bare.to_string(),
    ]
}

/// Split a command line into words, honouring single and double quotes.
pub fn split_words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for c in line.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '\'') | (None, '"') => {
                quote = Some(c);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if started || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The program a candidate really runs: `sudo -n poweroff` and `doas -n poweroff`
/// run `poweroff`, so that is the binary that has to exist (as well as sudo).
fn target_program(argv: &[String]) -> Option<&str> {
    let first = argv.first()?.as_str();
    if matches!(first, "sudo" | "doas") {
        return argv.iter().skip(1).find(|a| !a.starts_with('-')).map(String::as_str);
    }
    Some(first)
}

/// The candidates for `kind` whose programs `installed` accepts, as argv lists.
/// A `[power_commands]` entry replaces the built-in chain (an empty entry
/// disables the action).
pub fn candidates(kind: Kind, cfg: &TrayTools, installed: &dyn Fn(&str) -> bool) -> Vec<Vec<String>> {
    let lines: Vec<String> = match cfg.get(kind.key()) {
        Some(spec) => spec.lines(),
        None => defaults(kind),
    };
    lines
        .iter()
        .map(|l| split_words(l))
        .filter(|argv| !argv.is_empty())
        .filter(|argv| {
            installed(&argv[0]) && target_program(argv).map(installed).unwrap_or(false)
        })
        .collect()
}

/// Whether anything can carry out `kind` here (used to hide menu rows).
pub fn available(kind: Kind, cfg: &TrayTools, installed: &dyn Fn(&str) -> bool) -> bool {
    !candidates(kind, cfg, installed).is_empty()
}

/// What happened when a power action was attempted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    /// The command line that was accepted, if any.
    pub started: Option<String>,
    /// Every candidate tried before it (or all of them, on failure) and why it failed.
    pub tried: Vec<(String, String)>,
}

/// Seconds a candidate gets before it counts as "accepted and waiting" (a
/// suspend only returns after the machine has resumed).
const CAP_SECS: u64 = 6;

/// Run `kind`: try each candidate in order, stopping at the first that exits 0 or
/// is still running when the cap hits (the system accepted it). Every attempt is
/// logged. Blocks up to `CAP_SECS` per candidate, so call it off the render loop.
pub fn run(kind: Kind, cfg: &TrayTools, installed: &dyn Fn(&str) -> bool) -> Report {
    run_with_cap(kind, cfg, installed, CAP_SECS)
}

/// [`run`] with an explicit per-candidate cap (tests use a short one).
pub fn run_with_cap(kind: Kind, cfg: &TrayTools, installed: &dyn Fn(&str) -> bool, cap_secs: u64) -> Report {
    let mut report = Report { started: None, tried: Vec::new() };
    let cands = candidates(kind, cfg, installed);
    if cands.is_empty() {
        crate::dbg_log(&format!("power: {} — no candidate command is available", kind.key()));
        report.tried.push(("(none)".into(), "no usable command found".into()));
        return report;
    }
    for argv in cands {
        let line = argv.join(" ");
        crate::dbg_log(&format!("power: {} — trying `{line}`", kind.key()));
        let args: Vec<&str> = argv[1..].iter().map(String::as_str).collect();
        match run_capped_status(&argv[0], &args, cap_secs) {
            Capped::Done | Capped::StillRunning => {
                crate::dbg_log(&format!("power: {} — `{line}` accepted", kind.key()));
                report.started = Some(line);
                return report;
            }
            Capped::Failed(why) | Capped::Missing(why) => {
                crate::log_warn(&format!("power: {} — `{line}` failed: {why}", kind.key()));
                report.tried.push((line, why));
            }
        }
    }
    report
}

/// The machine's host name, for confirm dialogs.
pub fn hostname() -> String {
    let mut buf = [0u8; 256];
    // SAFETY: `buf` is a valid writable buffer of the length passed.
    let ok = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0;
    if !ok {
        return "this computer".into();
    }
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    let name = String::from_utf8_lossy(&buf[..end]).trim().to_string();
    if name.is_empty() { "this computer".into() } else { name }
}

/// Text for a window explaining why a power action did not start, with what was
/// tried, so the user can fix permissions or set `[power_commands]`.
pub fn failure_text(kind: Kind, report: &Report) -> String {
    let mut s = format!("{} did not start.\n\n", kind.label());
    for (cmd, why) in &report.tried {
        s += &format!("  {cmd}\n    {why}\n");
    }
    s += &format!(
        "\nNeed a different command? Set [power_commands] {} = \"...\" in config.toml.\n",
        kind.key()
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traytools::ToolSpec;

    fn have(list: &'static [&'static str]) -> impl Fn(&str) -> bool {
        move |b: &str| list.contains(&b)
    }

    #[test]
    fn split_words_honours_quotes() {
        assert_eq!(split_words("systemctl poweroff"), vec!["systemctl", "poweroff"]);
        assert_eq!(split_words("  a   b "), vec!["a", "b"]);
        assert_eq!(
            split_words(r#"osascript -e 'tell app "System Events" to shut down'"#),
            vec!["osascript", "-e", r#"tell app "System Events" to shut down"#]
        );
        assert_eq!(split_words("x ''"), vec!["x", ""]);
        assert!(split_words("   ").is_empty());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_chain_order_and_privileged_fallbacks() {
        let none = TrayTools::new();
        let all = candidates(Kind::PowerOff, &none, &|_| true);
        let lines: Vec<String> = all.iter().map(|a| a.join(" ")).collect();
        assert_eq!(lines, vec!["systemctl poweroff", "loginctl poweroff", "sudo -n poweroff", "doas -n poweroff", "poweroff"]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn candidates_need_the_real_target_program_too() {
        let none = TrayTools::new();
        // Alpine without elogind: only busybox poweroff and doas exist.
        let got = candidates(Kind::PowerOff, &none, &have(&["doas", "poweroff"]));
        let lines: Vec<String> = got.iter().map(|a| a.join(" ")).collect();
        assert_eq!(lines, vec!["doas -n poweroff", "poweroff"]);
        // sudo present but the target missing: not a candidate.
        assert!(candidates(Kind::Reboot, &none, &have(&["sudo"])).is_empty());
        // Nothing at all: the action is unavailable.
        assert!(!available(Kind::Suspend, &none, &have(&[])));
        assert!(available(Kind::Reboot, &none, &have(&["systemctl"])));
    }

    #[test]
    fn config_replaces_the_chain_and_empty_disables() {
        let mut cfg = TrayTools::new();
        cfg.insert("poweroff".into(), ToolSpec::One("my-poweroff --now".into()));
        let got = candidates(Kind::PowerOff, &cfg, &|_| true);
        assert_eq!(got, vec![vec!["my-poweroff".to_string(), "--now".to_string()]]);
        cfg.insert("suspend".into(), ToolSpec::Many(Vec::new()));
        assert!(!available(Kind::Suspend, &cfg, &|_| true), "an empty list disables the action");
        // Other actions keep their built-in chain.
        assert!(available(Kind::Reboot, &cfg, &|_| true));
    }

    #[test]
    fn run_falls_through_failures_and_stops_at_the_first_success() {
        let mut cfg = TrayTools::new();
        cfg.insert(
            "reboot".into(),
            ToolSpec::Many(vec!["sh -c 'echo denied >&2; exit 1'".into(), "true".into(), "false".into()]),
        );
        let r = run(Kind::Reboot, &cfg, &|_| true);
        assert_eq!(r.started.as_deref(), Some("true"), "the first success wins and later ones never run");
        assert_eq!(r.tried.len(), 1);
        assert!(r.tried[0].1.contains("denied"), "{:?}", r.tried);
    }

    #[test]
    fn run_reports_every_failure_when_nothing_works() {
        let mut cfg = TrayTools::new();
        cfg.insert("suspend".into(), ToolSpec::Many(vec!["false".into(), "sh -c 'exit 2'".into()]));
        let r = run(Kind::Suspend, &cfg, &|_| true);
        assert!(r.started.is_none());
        assert_eq!(r.tried.len(), 2);
        let text = failure_text(Kind::Suspend, &r);
        assert!(text.contains("Suspend did not start") && text.contains("[power_commands]"), "{text}");
    }

    #[test]
    fn a_command_still_running_at_the_cap_counts_as_accepted() {
        // `sleep` outlives the cap like a real suspend waiting on resume.
        let mut cfg = TrayTools::new();
        cfg.insert("suspend".into(), ToolSpec::One("sleep 8".into()));
        let r = run_with_cap(Kind::Suspend, &cfg, &|_| true, 1);
        assert_eq!(r.started.as_deref(), Some("sleep 8"));
    }

    #[test]
    fn nothing_available_is_reported_not_run() {
        let mut cfg = TrayTools::new();
        cfg.insert("poweroff".into(), ToolSpec::One("tetron-no-such-power-tool".into()));
        let r = run(Kind::PowerOff, &cfg, &|_| false);
        assert!(r.started.is_none() && r.tried[0].0 == "(none)");
    }

    #[test]
    fn hostname_is_not_empty() {
        assert!(!hostname().is_empty());
    }
}
