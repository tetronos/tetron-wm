//! The Store install command (WMCAT-005, WMCAT-006): one shell script per
//! app, run in a window. The host's package manager first, then the verified
//! upstream recipe (its toolchain offered first), then Homebrew, else a
//! best-effort chain; it ends by naming the homepage and, when the app has
//! prebuilt binaries, its releases page. Pure string generation.

use crate::{CatalogApp, Packages};

/// A package name the Store may put in a shell command: letters, digits and
/// `. _ + - / @` only (the catalog is downloaded; anything else is ignored).
fn pkg_name(n: &str) -> Option<&str> {
    let ok = !n.is_empty() && !n.starts_with('-') && n.chars().all(|c| c.is_ascii_alphanumeric() || "._+-/@".contains(c));
    ok.then_some(n)
}

/// `sudo` unless already root, as `$S` for the package manager branches.
const SUDO_VAR: &str = "S=sudo; [ \"$(id -u)\" -eq 0 ] && S=; ";

/// The host package manager branches for `p` (WMCAT-005), host order: Homebrew
/// on macOS, then apt, dnf, pacman. Each is `(condition, command)`; a condition
/// holds when that manager exists and has the package, so a name missing on
/// this release falls through to the next way.
fn native_branches(p: &Packages) -> Vec<(String, String)> {
    let mut v = Vec::new();
    if let Some(n) = pkg_name(&p.brew) {
        v.push((
            format!("[ \"$(uname -s)\" = Darwin ] && {{ {}; command -v brew >/dev/null 2>&1; }}", crate::toolchain::BREW_ENV),
            format!("brew install {n}"),
        ));
    }
    if let Some(n) = pkg_name(&p.apt) {
        v.push((format!("command -v apt-get >/dev/null 2>&1 && apt-cache show {n} >/dev/null 2>&1"), format!("$S apt-get install -y {n}")));
    }
    if let Some(n) = pkg_name(&p.dnf) {
        v.push((format!("command -v dnf >/dev/null 2>&1 && dnf -q info {n} >/dev/null 2>&1"), format!("$S dnf install -y {n}")));
    }
    if let Some(n) = pkg_name(&p.pacman) {
        v.push((format!("command -v pacman >/dev/null 2>&1 && pacman -Si {n} >/dev/null 2>&1"), format!("$S pacman -S --needed --noconfirm {n}")));
    }
    v
}

/// The install command for an app (run in a shell window), WMCAT-005: the
/// host's own package manager when it has the app (Homebrew on macOS; apt, dnf
/// or pacman on Linux), else the verified upstream recipe (binary, cargo, go,
/// uv, …), else Homebrew on Linux as the last resort, else a best-effort chain.
/// It always ends by naming the homepage so the user can finish by hand.
pub fn install_command(app: &CatalogApp) -> String {
    let done = format!(
        "echo; echo '────────'; echo 'Done. If it succeeded, {name} is now in your launcher.'; {prebuilt}\
echo 'Close this window (x) when finished.'; exec \"$SHELL\"",
        name = app.name,
        prebuilt = prebuilt_lines(app),
    );
    let recipe = app.recipe.as_ref().filter(|r| r.verified && !r.install.is_empty());
    // The upstream way: a verified recipe (its toolchain offered first when
    // missing), or Homebrew by name; None leaves the best-effort chain.
    let brew_by_name = pkg_name(&app.pkg.brew).map(|n| format!("brew install {n}"));
    let upstream = match (recipe, brew_by_name) {
        (Some(r), _) => Some((r.method.clone(), r.install.clone())),
        (None, Some(b)) => Some(("brew".to_string(), b)),
        (None, None) => None,
    };
    let upstream = upstream.map(|(method, cmd)| {
        let pre = crate::toolchain::preamble(&app.name, &method, &app.homepage);
        format!("{pre}{cmd}")
    });
    let native = native_branches(&app.pkg);
    if upstream.is_none() && native.is_empty() {
        return fallback_command(app);
    }
    let mut sh = format!("clear; {SUDO_VAR}echo 'Installing {name} …'; echo; ", name = app.name);
    if native.is_empty() {
        sh.push_str(&format!("{}; ", upstream.unwrap()));
    } else {
        for (i, (cond, cmd)) in native.iter().enumerate() {
            sh.push_str(&format!("{} {cond}; then {cmd}; ", if i == 0 { "if" } else { "elif" }));
        }
        let rest = upstream.unwrap_or_else(|| {
            format!("echo 'No package for {name} here. Get it from:'; echo '  {home}'", name = app.name, home = app.homepage)
        });
        sh.push_str(&format!("else {rest}; fi; "));
    }
    sh.push_str(&done);
    sh
}

/// Shell lines naming the releases page (WMCAT-006), or nothing.
fn prebuilt_lines(app: &CatalogApp) -> String {
    crate::releases_url(app)
        .map(|u| format!("echo 'If it did not install, prebuilt binaries are at:'; echo '  {u}'; "))
        .unwrap_or_default()
}

/// The best-effort chain for an app with no package names and no verified
/// recipe: the host's manager by command name, then `cargo install`, then
/// `go install <repo>@latest`, then Homebrew when installed.
fn fallback_command(app: &CatalogApp) -> String {
    let gopath = app
        .homepage
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    let bin = pkg_name(&app.bin).unwrap_or("");
    let native: String = if bin.is_empty() {
        String::new()
    } else {
        native_branches(&Packages { apt: bin.into(), dnf: bin.into(), pacman: bin.into(), brew: String::new() })
            .into_iter()
            .map(|(c, i)| format!("( {c} && {i} ) || "))
            .collect()
    };
    format!(
        "clear; {SUDO_VAR}echo 'Installing {name} — trying the package manager, then cargo, go and brew…'; echo; \
{native}cargo install {bin} \
|| go install {gopath}@latest \
|| ( {brew_env}; command -v brew >/dev/null 2>&1 && brew install {bin} ) \
|| true; \
echo; echo '────────'; \
echo 'If nothing installed automatically, get {name} from:'; echo '  {home}'; {prebuilt}\
echo; echo 'Close this window (x) when done.'; exec \"$SHELL\"",
        name = app.name,
        prebuilt = prebuilt_lines(app),
        brew_env = crate::toolchain::BREW_ENV,
        home = app.homepage,
    )
}


#[cfg(test)]
mod tests {
    use super::*;

    /// The command parses as POSIX shell (`sh -n`).
    fn parses(c: &str) {
        let ok = std::process::Command::new("sh").args(["-n", "-c", c]).status().expect("sh").success();
        assert!(ok, "not valid sh: {c}");
    }

    fn app(json: &str) -> CatalogApp {
        let base = r#"{"name":"demo","bin":"demo","category":"X","description":"d","homepage":"https://github.com/o/demo""#;
        serde_json::from_str(&format!("{base}{json}}}")).expect("test app")
    }

    /// WMCAT-005: the host's manager first (brew only on macOS), then the
    /// verified upstream recipe; Homebrew on Linux only when nothing else.
    #[test]
    fn host_manager_first_then_upstream_recipe() {
        let a = app(r#","pkg":{"apt":"demo-tui","dnf":"demo","pacman":"demo","brew":"demo"},
            "recipe":{"install":"cargo install demo","method":"cargo","verified":true}"#);
        let c = install_command(&a);
        let (darwin, apt, dnf, pac, up) = (
            c.find("= Darwin").unwrap(),
            c.find("apt-cache show demo-tui").unwrap(),
            c.find("dnf -q info demo").unwrap(),
            c.find("pacman -Si demo").unwrap(),
            c.find("else ").unwrap(),
        );
        parses(&c);
        assert!(darwin < apt && apt < dnf && dnf < pac && pac < up, "{c}");
        assert!(c.contains("$S apt-get install -y demo-tui") && c[up..].contains("cargo install demo"), "{c}");
        assert!(c.contains("S=sudo; [ \"$(id -u)\" -eq 0 ] && S=;"), "root needs no sudo: {c}");
        assert!(!c[up..].contains("brew install"), "Linux reaches no brew when upstream exists: {c}");
    }

    /// A brew-only app on Linux: Homebrew is offered and loaded into the
    /// shell after its installer, so the brew install in the same shell works.
    #[test]
    fn brew_last_resort_loads_brew_into_the_shell() {
        let a = app(r#","recipe":{"install":"brew install o/tap/demo","method":"brew","verified":true}"#);
        let c = install_command(&a);
        parses(&c);
        assert!(c.contains("Homebrew/install") && c.contains("brew install o/tap/demo"), "{c}");
        let boot = c.find("Homebrew/install").unwrap();
        assert!(c[boot..].contains("shellenv"), "brew put on PATH after its installer: {c}");
        assert!(c.find("shellenv").unwrap() < c.find("command -v brew").unwrap(), "an installed brew off PATH is used first: {c}");
    }

    /// Package names from the downloaded catalog never reach the shell unless
    /// they are plain names; a distro-only app without recipe names the homepage.
    #[test]
    fn unsafe_package_names_are_ignored() {
        let a = app(r#","pkg":{"apt":"x; rm -rf ~","pacman":"-Syu","dnf":"demo"}"#);
        let c = install_command(&a);
        parses(&c);
        assert!(!c.contains("rm -rf") && !c.contains("-Syu"), "{c}");
        assert!(c.contains("dnf install -y demo") && c.contains("No package for demo here"), "{c}");
    }

    /// WMCAT-006: an app with prebuilt binaries names its releases page in
    /// the best-effort chain and after a recipe install; one without does not,
    /// and a homepage with odd characters never builds a link.
    #[test]
    fn releases_page_named_only_when_flagged_and_plain() {
        let c = install_command(&app(r#","releases":true"#));
        parses(&c);
        assert!(c.contains("prebuilt binaries are at:'; echo '  https://github.com/o/demo/releases/latest'"), "{c}");
        let c = install_command(&app(r#","releases":true,"recipe":{"install":"cargo install demo","method":"cargo","verified":true}"#));
        parses(&c);
        assert!(c.contains("/o/demo/releases/latest"), "{c}");
        assert!(!install_command(&app("")).contains("releases/latest"));
        let mut odd = app(r#","releases":true"#);
        odd.homepage = "https://github.com/o/de'mo".into();
        assert_eq!(crate::releases_url(&odd), None);
        odd.homepage = "https://github.com/o/demo/tree/main/x".into();
        assert_eq!(crate::releases_url(&odd), None, "repository roots only");
    }

    /// No names, no recipe: the host manager by command name, then cargo, go,
    /// and brew only when it is installed (never bootstrapped).
    #[test]
    fn fallback_tries_host_manager_before_cargo_go_brew() {
        let c = install_command(&app(""));
        let (apt, cargo, go, brew) = (
            c.find("apt-cache show demo").unwrap(),
            c.find("cargo install demo").unwrap(),
            c.find("go install github.com/o/demo@latest").unwrap(),
            c.find("brew install demo").unwrap(),
        );
        parses(&c);
        assert!(apt < cargo && cargo < go && go < brew, "{c}");
        assert!(!c.contains("Homebrew/install"), "{c}");
    }
}
