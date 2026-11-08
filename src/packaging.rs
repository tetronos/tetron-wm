//! Whether apt (dpkg) owns the running tetron-wm, so nothing in the app ever
//! writes over a packaged binary (WMDEB-005): Settings > Updates upgrades it
//! through the release `.deb` and apt, and the dev channel refuses.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The Debian package name.
pub const PKG: &str = "tetron-wm";

/// dpkg's record of the files each package installed.
const DPKG_INFO: &str = "/var/lib/dpkg/info";

/// Whether a dpkg `.list` file (one installed path per line) names `path`.
pub fn list_owns(list: &str, path: &Path) -> bool {
    let want = path.to_string_lossy();
    list.lines().any(|l| l == want)
}

/// The `.list` files of the tetron-wm package in `info_dir`: `tetron-wm.list`,
/// or `tetron-wm:<arch>.list` for a multi-arch install.
fn package_lists(info_dir: &Path) -> Vec<PathBuf> {
    let plain = info_dir.join(format!("{PKG}.list"));
    if plain.is_file() {
        return vec![plain];
    }
    let prefix = format!("{PKG}:");
    std::fs::read_dir(info_dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".list"))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Whether the tetron-wm package (per the dpkg records in `info_dir`) owns
/// `path`, compared after resolving symlinks.
pub fn owned_in(info_dir: &Path, path: &Path) -> bool {
    let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    package_lists(info_dir)
        .iter()
        .filter_map(|l| std::fs::read_to_string(l).ok())
        .any(|text| list_owns(&text, &real) || list_owns(&text, path))
}

/// Whether apt installed the running binary. Checked once per process.
pub fn running_is_packaged() -> bool {
    static PACKAGED: OnceLock<bool> = OnceLock::new();
    *PACKAGED.get_or_init(|| {
        let owned = std::env::current_exe().is_ok_and(|exe| owned_in(Path::new(DPKG_INFO), &exe));
        if owned {
            crate::dbg_log("packaging: the running tetron-wm is owned by the tetron-wm package (apt)");
        }
        owned
    })
}

/// How much of the app sets is installed (WMINST-005): the suite script's
/// sizes, the same as the tetron-wm, tetron-wm-essentials (with
/// tetron-wm-desktop), tetron-wm-recommended and tetron-wm-extras debs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Size {
    Minimal,
    Essentials,
    Recommended,
    Extras,
}

impl Size {
    pub const ALL: [Size; 4] = [Size::Minimal, Size::Essentials, Size::Recommended, Size::Extras];

    /// The size's name with a capital, for the Apps dialog.
    pub fn title(self) -> &'static str {
        match self {
            Size::Minimal => "Minimal",
            Size::Essentials => "Essentials",
            Size::Recommended => "Recommended",
            Size::Extras => "Extras",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Size::Minimal => "minimal",
            Size::Essentials => "essentials",
            Size::Recommended => "recommended",
            Size::Extras => "extras",
        }
    }

    /// A size recorded in the suite state. "desktop" (before the sets) had
    /// only the tools tetron-wm uses, so it counts as minimal and the Apps
    /// row offers essentials.
    fn parse(s: &str) -> Option<Size> {
        match s.trim() {
            "minimal" | "desktop" => Some(Size::Minimal),
            "essentials" => Some(Size::Essentials),
            "recommended" => Some(Size::Recommended),
            "extras" => Some(Size::Extras),
            _ => None,
        }
    }
}

/// Whether dpkg has package `name` installed: its file list exists (dpkg
/// deletes it when the package is removed).
fn dpkg_has(info_dir: &Path, name: &str) -> bool {
    info_dir.join(format!("{name}.list")).is_file()
}

/// The last `key=value` for `key` in the suite script's state file.
pub fn state_value(state: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    state.lines().rev().find_map(|l| l.strip_prefix(&prefix)).map(str::to_string)
}

/// The installed size: the largest metapackage dpkg has, else the suite
/// script's record, else minimal. tetron-wm-desktop alone (an install from
/// before the sets) is not a size of its own.
pub fn size_from(info_dir: &Path, state: &str) -> Size {
    if dpkg_has(info_dir, "tetron-wm-extras") {
        Size::Extras
    } else if dpkg_has(info_dir, "tetron-wm-recommended") {
        Size::Recommended
    } else if dpkg_has(info_dir, "tetron-wm-essentials") {
        Size::Essentials
    } else {
        state_value(state, "size").and_then(|s| Size::parse(&s)).unwrap_or(Size::Minimal)
    }
}

/// How tetron-wm was installed, for Settings > About.
pub fn method_name(packaged: bool, exe: &Path) -> &'static str {
    if packaged {
        "apt package"
    } else if exe.parent().and_then(|d| d.parent()).and_then(|r| r.file_name()).is_some_and(|n| n == ".cargo") {
        "cargo"
    } else {
        "binary"
    }
}

fn suite_state_text() -> String {
    crate::config::config_dir()
        .and_then(|d| std::fs::read_to_string(d.join("suite-state")).ok())
        .unwrap_or_default()
}

/// The installed size on this machine (reads dpkg's records and the suite
/// state each call: cheap file checks, so the Settings row stays current).
pub fn installed_size() -> Size {
    size_from(Path::new(DPKG_INFO), &suite_state_text())
}

/// The install method of the running binary.
pub fn install_method() -> &'static str {
    let exe = std::env::current_exe().unwrap_or_default();
    method_name(running_is_packaged(), &exe)
}

/// The terminal command that installs the apps of `size` with the suite
/// script from `raw` (the branch's raw URL): apps only, no tetron-wm,
/// config, themes, conversion or console steps; waits for Enter at the end.
/// `list` is the app list Settings counted from ([`write_apps_list`]), handed
/// to the script so it installs the same list (WMCAT-004).
/// `scripts` false adds `--no-scripts`: the apps no distribution packages
/// (opencode, fresh, uv, catt, casttui, the Nerd Font) are left out.
pub fn apps_install_command(raw: &str, size: Size, list: Option<&Path>, scripts: bool) -> String {
    let s = size.name();
    let no_scripts = if scripts { "" } else { " --no-scripts" };
    let list = list.map(|p| format!(" --apps-list '{}'", p.to_string_lossy().replace('\'', "'\\''"))).unwrap_or_default();
    format!(
        "clear; echo 'Installing the {s} apps for tetron-wm (the suite installer)…'; echo; \
curl -fsSL {raw}/main/install-tetron-wm-suite.sh | bash -s -- --size {s} --no-wm --no-config --no-themes \
--no-convert --no-kmscon --no-console --no-autologin --no-autostart --no-fullscreen{no_scripts}{list}; \
echo \"apps: suite installer for {s} exited with status $?\" >> {log}; \
echo; printf 'Press Enter to close. '; read _",
        log = crate::logfile::shell_path(),
    )
}

/// Write the app list in use (the catalog's suite list, WMCAT-003) to
/// `~/.local/share/tetron-wm/suite-apps.txt` for the suite script.
pub fn write_apps_list() -> Option<std::path::PathBuf> {
    let path = crate::theme::data_home_dir()?.join("tetron-wm/suite-apps.txt");
    std::fs::create_dir_all(path.parent()?).ok()?;
    std::fs::write(&path, crate::catalog::suite_apps()).ok()?;
    Some(path)
}

/// What decides which rows of the app list apply to this machine.
#[derive(Clone, Copy, Debug)]
pub struct Host {
    pub linux: bool,
    /// A desktop session (X11 or Wayland): the gui rows apply.
    pub graphical: bool,
    /// A mouse device: gpm applies.
    pub mouse: bool,
    /// tetron-wm is the deb, which ships the helper scripts.
    pub packaged: bool,
}

impl Host {
    pub fn probe() -> Host {
        let set = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
        Host {
            linux: cfg!(target_os = "linux"),
            graphical: set("DISPLAY") || set("WAYLAND_DISPLAY"),
            mouse: Path::new("/dev/input/mice").exists(),
            packaged: running_is_packaged(),
        }
    }
}

/// The size of a list line; "desktop" is the essentials lines of a list
/// written before the sets.
fn size_rank(s: &str) -> u8 {
    match s {
        "essentials" | "desktop" => 1,
        "recommended" => 2,
        "extras" => 3,
        _ => u8::MAX,
    }
}

fn size_level(s: Size) -> u8 {
    s as u8
}

/// A missing app of the list: its display name, the package to install
/// with this host's manager ("" for a script app) and the size that brings it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Missing {
    pub name: String,
    pub pkg: String,
    pub size: Size,
}

impl Missing {
    pub fn is_script(&self) -> bool {
        self.pkg.is_empty()
    }
}

/// The apps of `size` and the smaller sizes in `list` (the suite-apps.txt
/// format) that `present` says are missing on `host`. Services never count:
/// they are a separate, default-no choice.
pub fn missing_rows(list: &str, size: Size, host: &Host, present: &dyn Fn(&str) -> bool) -> Vec<Missing> {
    let mut out = Vec::new();
    for line in list.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let f: Vec<&str> = line.split(':').collect();
        let rank = size_rank(f[0]);
        if f.len() < 3 || rank > size_level(size) {
            continue;
        }
        let at = Size::ALL[rank as usize];
        if f[1] == "script" {
            let (name, check) = match f[2] {
                "helpers" if host.packaged => continue,
                "helpers" => ("helpers", "tetron-wm-startpage"),
                n => (n, n),
            };
            if !present(check) {
                out.push(Missing { name: name.to_string(), pkg: String::new(), size: at });
            }
            continue;
        }
        if f.len() < 7 || f[1] == "services" || (f[1] == "gui" && !host.graphical) || (f[2] == "gpm" && !host.mouse) {
            continue;
        }
        let names = if host.linux { &f[3..6] } else { &f[6..7] };
        let Some(name) = names.iter().find(|n| !n.is_empty()) else { continue };
        if !present(f[2]) {
            out.push(Missing { name: name.to_string(), pkg: name.to_string(), size: at });
        }
    }
    out
}

/// [`missing_rows`] by display name.
pub fn missing_in(list: &str, size: Size, host: &Host, present: &dyn Fn(&str) -> bool) -> Vec<String> {
    missing_rows(list, size, host, present).into_iter().map(|m| m.name).collect()
}

/// Whether `list` has any line of `size` (extras is offered only then).
pub fn list_has_size(list: &str, size: Size) -> bool {
    list.lines().filter(|l| !l.starts_with('#')).any(|l| l.split(':').next().is_some_and(|s| size_rank(s) == size as u8))
}

/// The size debs that make up `size`, smallest first.
pub fn size_debs(size: Size) -> &'static [&'static str] {
    const ALL: [&str; 4] = ["tetron-wm-desktop", "tetron-wm-essentials", "tetron-wm-recommended", "tetron-wm-extras"];
    &ALL[..(size as usize + usize::from(size != Size::Minimal)).min(4)]
}

/// Settings > General > Apps > Remove set (WMINST-011): the size debs dpkg
/// has for `size` and the larger sizes, so the next smaller size stays. Empty
/// when tetron-wm-essentials is not installed (nothing the set added is
/// recorded); tetron-wm-desktop, the tools tetron-wm uses, is never removed.
pub fn remove_debs(info_dir: &Path, size: Size) -> Vec<&'static str> {
    if size == Size::Minimal || !dpkg_has(info_dir, "tetron-wm-essentials") {
        return Vec::new();
    }
    size_debs(Size::Extras)[size as usize..].iter().copied().filter(|p| dpkg_has(info_dir, p)).collect()
}

/// The size debs to remove for the installed size on this machine.
pub fn remove_debs_here(size: Size) -> Vec<&'static str> {
    if !Path::new("/usr/bin/apt-get").exists() {
        return Vec::new();
    }
    remove_debs(Path::new(DPKG_INFO), size)
}

/// The terminal command that removes `pkgs` and then lets apt list the apps
/// they brought and ask before removing them (`apt-get autoremove`).
pub fn apps_remove_command(pkgs: &[&str]) -> String {
    let list = pkgs.join(" ");
    format!(
        "clear; echo 'Removing {list}. apt then lists the apps no other package needs and asks before removing them.'; echo; \
sudo apt-get remove {list} && sudo apt-get autoremove; \
echo \"apps: removed {list} (status $?)\" >> {log}; \
echo; printf 'Press Enter to close. '; read _",
        log = crate::logfile::shell_path(),
    )
}

/// The packages of `apt-cache policy` output that have a candidate.
fn with_candidate(policy: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Option<&str> = None;
    for l in policy.lines() {
        if !l.starts_with(' ') && l.ends_with(':') {
            cur = Some(l.trim_end_matches(':'));
        } else if let (Some(n), Some(c)) = (cur, l.trim().strip_prefix("Candidate:")) {
            if c.trim() != "(none)" {
                out.push(n.to_string());
            }
            cur = None;
        }
    }
    out
}

/// The sum of the sizes in `apt-get --print-uris` output ('url' file size hash).
fn sum_uri_sizes(out: &str) -> u64 {
    out.lines().filter(|l| l.starts_with('\'')).filter_map(|l| l.split_whitespace().nth(2)?.parse::<u64>().ok()).sum()
}

/// The download size of installing `pkgs` with apt (with their Recommends,
/// as the size debs bring them), or None without apt or when apt can not
/// say. Names this release lacks are left out, as apt skips them. Needs no
/// root; runs apt-cache and apt-get, so call it off the main loop.
pub fn apt_download_size(pkgs: &[String]) -> Option<u64> {
    if pkgs.is_empty() {
        return Some(0);
    }
    let run = |cmd: &str, args: &[&str]| {
        std::process::Command::new(cmd).args(args).env("LC_ALL", "C").output().ok().filter(|o| o.status.success())
    };
    let mut args = vec!["policy"];
    args.extend(pkgs.iter().map(String::as_str));
    let have = with_candidate(&String::from_utf8_lossy(&run("apt-cache", &args)?.stdout));
    if have.is_empty() {
        return Some(0);
    }
    let mut args = vec!["install", "--print-uris", "-qq", "-y"];
    args.extend(have.iter().map(String::as_str));
    Some(sum_uri_sizes(&String::from_utf8_lossy(&run("apt-get", &args)?.stdout)))
}

/// "about 120 MB" for a byte count (at least 1 MB).
pub fn fmt_download(bytes: u64) -> String {
    format!("about {} MB", (bytes / 1_000_000).max(1))
}

/// Whether a check of the app list holds here: a command on PATH or in
/// ~/.local/bin, alternatives with `|`, `font=NAME`, `consolefonts`, or
/// `nerd-font`.
fn check_present(check: &str) -> bool {
    if check.contains('|') {
        return check.split('|').any(check_present);
    }
    let fonts = |pattern: &str| {
        std::process::Command::new("fc-list")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase().contains(&pattern.to_lowercase()))
    };
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let any_file = |dirs: &[PathBuf], prefix: &str| {
        dirs.iter().any(|d| {
            std::fs::read_dir(d).is_ok_and(|rd| rd.flatten().any(|e| e.file_name().to_string_lossy().starts_with(prefix)))
        })
    };
    match check {
        "consolefonts" => any_file(&[PathBuf::from("/usr/share/consolefonts"), PathBuf::from("/usr/lib/kbd/consolefonts")], ""),
        "nerd-font" => fonts("nerd font").unwrap_or_else(|| {
            any_file(&[home.join(".local/share/fonts"), PathBuf::from("/usr/share/fonts/truetype/nerd-font-symbols")], "SymbolsNerdFont")
        }),
        c if c.starts_with("font=") => fonts(&c[5..]).unwrap_or_else(|| {
            any_file(&[PathBuf::from("/usr/share/fonts/truetype/dejavu"), PathBuf::from("/usr/share/fonts/dejavu")], "DejaVuSansMono")
        }),
        c => crate::catalog::is_installed(c) || home.join(".local/bin").join(c).is_file(),
    }
}

/// The apps of `size` missing on this machine (WMINST-007). Runs fc-list once
/// per font check, so call it on entering Settings, not per frame.
pub fn missing_apps(size: Size) -> Vec<String> {
    missing_rows_here(size).into_iter().map(|m| m.name).collect()
}

/// [`missing_rows`] for this machine (runs the checks, so not per frame).
pub fn missing_rows_here(size: Size) -> Vec<Missing> {
    missing_rows(crate::catalog::suite_apps(), size, &Host::probe(), &check_present)
}

/// The step of every update command that updates the apps with their own
/// updater (WMINST-009): uv itself when no package owns it, then the uv tools;
/// then opencode in ~/.local/bin when upstream's latest (stable) release
/// differs, its tarball checked against GitHub's sha256 digest (WMINST-010;
/// `opencode upgrade` would install a second copy in ~/.opencode/bin). Errors never stop the tetron-wm
/// update that follows.
pub fn tool_updates_snippet() -> String {
    "u=\"$(command -v uv 2>/dev/null || { [ -x \"$HOME/.local/bin/uv\" ] && echo \"$HOME/.local/bin/uv\"; } || true)\"; \
if [ -n \"$u\" ]; then echo 'Updating uv and the apps it installed (catt, casttui)…'; \
if ! dpkg -S \"$(readlink -f \"$u\")\" >/dev/null 2>&1; then \"$u\" self update 2>/dev/null || true; fi; \
\"$u\" tool upgrade --all || true; echo \"update: uv self update and uv tool upgrade --all\" >> @LOG@; echo; fi; \
o=\"$HOME/.local/bin/opencode\"; if [ -x \"$o\" ] && [ \"$(uname -s)\" = Linux ] && command -v python3 >/dev/null; then \
case \"$(uname -m)\" in x86_64|amd64) a=x64; grep avx2 /proc/cpuinfo >/dev/null 2>&1 || a=x64-baseline;; aarch64|arm64) a=arm64;; *) a=;; esac; \
[ -n \"$a\" ] && ldd --version 2>&1 | grep -i musl >/dev/null && a=\"$a-musl\"; f=\"opencode-linux-$a.tar.gz\"; \
rel=\"$(curl -fsSL https://api.github.com/repos/anomalyco/opencode/releases/latest 2>/dev/null | python3 -c 'import json,sys; r=json.load(sys.stdin); a=[x for x in r[\"assets\"] if x[\"name\"]==sys.argv[1]][0]; print(r[\"tag_name\"], a[\"browser_download_url\"], a[\"digest\"].split(\":\")[1])' \"$f\" 2>/dev/null)\"; \
t=\"${rel%% *}\"; t=\"${t#v}\"; v=\"$(\"$o\" --version 2>/dev/null | sed 's/.*v//')\"; \
if [ -n \"$a\" ] && [ -n \"$t\" ] && [ \"$t\" != \"$v\" ]; then echo \"Updating opencode $v to $t (checked against GitHub's sha256 digest)…\"; \
url=\"$(echo \"$rel\" | cut -d' ' -f2)\"; sum=\"$(echo \"$rel\" | cut -d' ' -f3)\"; d=\"$(mktemp -d)\"; \
if curl -fsSL -o \"$d/$f\" \"$url\" && echo \"$sum  $d/$f\" | sha256sum -c - >/dev/null && tar xzf \"$d/$f\" -C \"$d\" opencode && install -m 755 \"$d/opencode\" \"$o\"; \
then echo \"update: opencode $t\" >> @LOG@; else echo 'opencode was not updated (download or checksum failed)'; fi; \
rm -rf \"$d\"; echo; fi; fi; "
        .replace("@LOG@", &crate::logfile::shell_path())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(linux: bool, graphical: bool, mouse: bool, packaged: bool) -> Host {
        Host { linux, graphical, mouse, packaged }
    }

    const LIST: &str = "# header\n\
essentials:apps:mpv:mpv:mpv:mpv:mpv\n\
essentials:apps:gpm:gpm:gpm:gpm:\n\
essentials:gui:xclip:xclip:xclip:xclip:\n\
essentials:apps:tetron-browser:tetron-browser:::\n\
essentials:services:nmcli:network-manager:NetworkManager:networkmanager:\n\
recommended:apps:lazygit:lazygit::lazygit:lazygit\n\
extras:apps:git:git:git:git:git\n\
essentials:apps:uv::uv:uv:uv\n\
essentials:apps:blueutil::::blueutil\n\
essentials:script:fresh\n\
essentials:script:nerd-font\n\
essentials:script:helpers\n\
essentials:script:opencode\n";

    #[test]
    fn missing_counts_the_size_and_smaller_never_services() {
        let none = |_: &str| false;
        let h = host(true, false, true, false);
        assert!(missing_in(LIST, Size::Minimal, &h, &none).is_empty(), "minimal has no apps");
        let d = missing_in(LIST, Size::Essentials, &h, &none);
        assert_eq!(d, ["mpv", "gpm", "tetron-browser", "uv", "fresh", "nerd-font", "helpers", "opencode"]);
        let r = missing_in(LIST, Size::Recommended, &h, &none);
        assert!(r.contains(&"lazygit".to_string()) && r.len() == d.len() + 1, "recommended adds its own rows: {r:?}");
        let e = missing_in(LIST, Size::Extras, &h, &none);
        assert!(e.contains(&"git".to_string()) && e.len() == r.len() + 1, "extras adds its own rows: {e:?}");
        assert!(!e.iter().any(|n| n == "network-manager"), "services never count");
    }

    #[test]
    fn missing_follows_the_host() {
        let none = |_: &str| false;
        let d = missing_in(LIST, Size::Essentials, &host(true, true, false, true), &none);
        assert!(d.contains(&"xclip".to_string()), "gui rows with a desktop session: {d:?}");
        assert!(!d.contains(&"gpm".to_string()), "no mouse device: no gpm");
        assert!(!d.contains(&"helpers".to_string()), "the deb ships the helpers");
        let mac = missing_in(LIST, Size::Essentials, &host(false, false, true, false), &none);
        assert!(mac.contains(&"blueutil".to_string()) && !mac.contains(&"gpm".to_string()), "brew names on macOS: {mac:?}");
        assert!(!mac.contains(&"tetron-browser".to_string()), "no brew name: not counted");
    }

    #[test]
    fn a_list_from_before_the_sets_reads_desktop_as_essentials() {
        let none = |_: &str| false;
        let old = "desktop:apps:mpv:mpv:mpv:mpv:mpv\nextras:apps:git:git:git:git:git\n";
        let h = host(true, false, true, false);
        assert_eq!(missing_in(old, Size::Essentials, &h, &none), ["mpv"]);
    }

    #[test]
    fn missing_rows_carry_the_package_and_size() {
        let none = |_: &str| false;
        let r = missing_rows(LIST, Size::Recommended, &host(true, false, true, false), &none);
        let lazygit = r.iter().find(|m| m.name == "lazygit").unwrap();
        assert_eq!((lazygit.pkg.as_str(), lazygit.size), ("lazygit", Size::Recommended));
        let fresh = r.iter().find(|m| m.name == "fresh").unwrap();
        assert!(fresh.is_script() && fresh.size == Size::Essentials);
    }

    #[test]
    fn extras_is_offered_only_with_extras_lines() {
        assert!(list_has_size(LIST, Size::Extras));
        assert!(!list_has_size(crate::catalog::suite_apps(), Size::Extras), "no extras apps yet");
        assert!(list_has_size(crate::catalog::suite_apps(), Size::Recommended));
    }

    #[test]
    fn size_debs_add_up() {
        assert!(size_debs(Size::Minimal).is_empty());
        assert_eq!(size_debs(Size::Essentials), ["tetron-wm-desktop", "tetron-wm-essentials"]);
        assert_eq!(size_debs(Size::Extras).len(), 4);
    }

    #[test]
    fn remove_set_removes_the_size_and_larger_never_the_tools() {
        let d = tmp("remove");
        std::fs::write(d.join("tetron-wm-desktop.list"), "/.\n").unwrap();
        assert!(remove_debs(&d, Size::Essentials).is_empty(), "only the tools: nothing recorded");
        std::fs::write(d.join("tetron-wm-essentials.list"), "/.\n").unwrap();
        std::fs::write(d.join("tetron-wm-recommended.list"), "/.\n").unwrap();
        assert_eq!(remove_debs(&d, Size::Recommended), ["tetron-wm-recommended"]);
        assert_eq!(remove_debs(&d, Size::Essentials), ["tetron-wm-essentials", "tetron-wm-recommended"]);
        assert!(remove_debs(&d, Size::Minimal).is_empty());
        std::fs::remove_dir_all(&d).ok();
        let cmd = apps_remove_command(&["tetron-wm-recommended"]);
        assert!(std::process::Command::new("sh").args(["-n", "-c", &cmd]).status().unwrap().success(), "{cmd}");
        assert!(cmd.contains("sudo apt-get remove tetron-wm-recommended && sudo apt-get autoremove"), "{cmd}");
        assert!(!cmd.contains(" -y"), "apt asks before removing: {cmd}");
    }

    #[test]
    fn download_size_parsers() {
        let policy = "glow:\n  Installed: (none)\n  Candidate: (none)\n  Version table:\ntmux:\n  Installed: (none)\n  Candidate: 3.5a-3\n  Version table:\n     3.5a-3 500\n";
        assert_eq!(with_candidate(policy), ["tmux"]);
        let uris = "'http://deb.debian.org/debian/pool/main/t/tmux/tmux_3.5a-3_amd64.deb' tmux_3.5a-3_amd64.deb 476020 SHA256:ab\n'http://x/y.deb' y.deb 1000 SHA256:cd\n";
        assert_eq!(sum_uri_sizes(uris), 477_020);
        assert_eq!(fmt_download(477_020), "about 1 MB");
        assert_eq!(fmt_download(123_400_000), "about 123 MB");
    }

    #[test]
    fn present_apps_are_not_missing() {
        let have = |c: &str| matches!(c, "mpv" | "gpm" | "tetron-browser" | "uv" | "fresh" | "nerd-font" | "tetron-wm-startpage" | "opencode");
        assert!(missing_in(LIST, Size::Essentials, &host(true, false, true, false), &have).is_empty());
    }

    #[test]
    fn the_compiled_list_parses_and_every_row_applies_somewhere() {
        let none = |_: &str| false;
        let linux = missing_in(crate::catalog::suite_apps(), Size::Extras, &host(true, true, true, false), &none);
        assert!(linux.len() > 20, "{linux:?}");
        assert!(linux.contains(&"fresh".to_string()) && linux.contains(&"git".to_string()));
        let ess = missing_in(crate::catalog::suite_apps(), Size::Essentials, &host(true, true, true, false), &none);
        assert!(ess.contains(&"git".to_string()) && !ess.contains(&"lazygit".to_string()), "{ess:?}");
    }

    #[test]
    fn tool_updates_snippet_is_valid_sh() {
        let ok = std::process::Command::new("sh").args(["-n", "-c", &tool_updates_snippet()]).status().unwrap().success();
        assert!(ok, "{}", tool_updates_snippet());
    }

    #[test]
    fn opencode_update_is_checked_and_in_place() {
        let s = tool_updates_snippet();
        assert!(s.contains("$HOME/.local/bin/opencode") && s.contains("sha256sum -c") && s.contains("releases/latest"), "{s}");
        assert!(!s.contains("opencode upgrade"), "its own upgrade installs a second copy elsewhere");
        assert!(s.find("tool upgrade --all").unwrap() < s.find(".local/bin/opencode").unwrap(), "after the uv step");
    }

    #[test]
    fn opencode_update_does_nothing_without_our_copy() {
        let home = tmp("noopencode");
        let out = std::process::Command::new("sh").arg("-c").arg(tool_updates_snippet()).env("HOME", &home)
            .env("PATH", "/usr/bin:/bin").output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(!text.contains("opencode"), "{text}");
        let _ = std::fs::remove_dir_all(&home);
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tetron-wm-packaging-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn list_matches_whole_lines_only() {
        let list = "/.\n/usr\n/usr/bin\n/usr/bin/tetron-wm\n/usr/share/doc/tetron-wm\n";
        assert!(list_owns(list, Path::new("/usr/bin/tetron-wm")));
        assert!(!list_owns(list, Path::new("/usr/local/bin/tetron-wm")));
        assert!(!list_owns(list, Path::new("/usr/bin/tetron")), "no prefix match");
    }

    #[test]
    fn owned_through_the_package_list_and_symlinks() {
        let d = tmp("owned");
        let (info, bin) = (d.join("info"), d.join("usr/bin"));
        std::fs::create_dir_all(&info).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        let exe = bin.join("tetron-wm");
        std::fs::write(&exe, "x").unwrap();
        let raw = d.join("raw-tetron-wm");
        std::fs::write(&raw, "x").unwrap();
        assert!(!owned_in(&info, &exe), "no package installed");
        std::fs::write(info.join("tetron-wm.list"), format!("/.\n{}\n", std::fs::canonicalize(&exe).unwrap().display())).unwrap();
        assert!(owned_in(&info, &exe));
        assert!(!owned_in(&info, &raw), "a copy elsewhere is not the package's");
        #[cfg(unix)]
        {
            let link = d.join("link");
            std::os::unix::fs::symlink(&exe, &link).unwrap();
            assert!(owned_in(&info, &link), "resolved through a symlink");
        }
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn multiarch_list_name_is_found() {
        let d = tmp("multiarch");
        std::fs::write(d.join("tetron-wm-desktop.list"), "/usr/bin/tetron-wm\n").unwrap();
        assert!(!owned_in(&d, Path::new("/usr/bin/tetron-wm")), "another package's list is not ours");
        std::fs::write(d.join("tetron-wm:amd64.list"), "/usr/bin/tetron-wm\n").unwrap();
        assert!(owned_in(&d, Path::new("/usr/bin/tetron-wm")));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn size_prefers_dpkg_then_suite_state_then_minimal() {
        let d = tmp("size");
        assert_eq!(size_from(&d, ""), Size::Minimal);
        assert_eq!(size_from(&d, "vt=1\nsize=desktop\nsize=extras\n"), Size::Extras, "last value wins");
        assert_eq!(size_from(&d, "size=bogus\n"), Size::Minimal);
        assert_eq!(size_from(&d, "size=desktop\n"), Size::Minimal, "desktop before the sets: offer essentials");
        assert_eq!(size_from(&d, "size=recommended\n"), Size::Recommended);
        std::fs::write(d.join("tetron-wm-desktop.list"), "/.\n").unwrap();
        assert_eq!(size_from(&d, "size=essentials\n"), Size::Essentials, "tetron-wm-desktop alone is no size");
        std::fs::write(d.join("tetron-wm-essentials.list"), "/.\n").unwrap();
        assert_eq!(size_from(&d, "size=minimal\n"), Size::Essentials, "dpkg wins over the record");
        std::fs::write(d.join("tetron-wm-recommended.list"), "/.\n").unwrap();
        assert_eq!(size_from(&d, ""), Size::Recommended);
        std::fs::write(d.join("tetron-wm-extras.list"), "/.\n").unwrap();
        assert_eq!(size_from(&d, ""), Size::Extras);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn method_from_ownership_and_path() {
        assert_eq!(method_name(true, Path::new("/usr/bin/tetron-wm")), "apt package");
        assert_eq!(method_name(false, Path::new("/home/u/.cargo/bin/tetron-wm")), "cargo");
        assert_eq!(method_name(false, Path::new("/usr/local/bin/tetron-wm")), "binary");
        assert_eq!(method_name(false, Path::new("tetron-wm")), "binary");
    }

    /// The suite script fetches the list published for its core minor; that
    /// minor follows this build's version (WMCAT-004).
    #[test]
    fn suite_script_lists_minor_follows_the_core_version() {
        let script = include_str!("../install-tetron-wm-suite.sh");
        let want = format!("LISTS_MINOR=\"{}\"", crate::listfetch::minor_of(env!("CARGO_PKG_VERSION")));
        assert!(script.lines().any(|l| l == want), "install-tetron-wm-suite.sh needs {want}");
    }

    #[test]
    fn apps_command_is_valid_sh_and_installs_apps_only() {
        let cmd = apps_install_command("https://raw.githubusercontent.com/o/r", Size::Extras, None, true);
        assert!(std::process::Command::new("sh").args(["-n", "-c", &cmd]).status().unwrap().success(), "{cmd}");
        assert!(!cmd.contains("--apps-list"), "{cmd}");
        let odd = Path::new("/home/a b/it's/suite-apps.txt");
        let with = apps_install_command("https://raw.githubusercontent.com/o/r", Size::Essentials, Some(odd), false);
        assert!(std::process::Command::new("sh").args(["-n", "-c", &with]).status().unwrap().success(), "{with}");
        assert!(with.contains("--apps-list '/home/a b/it'\\''s/suite-apps.txt'"), "{with}");
        assert!(with.contains("--no-scripts") && !cmd.contains("--no-scripts"), "{with}");
        assert!(cmd.contains("--size extras") && cmd.contains("--no-wm") && cmd.contains("--no-console"), "{cmd}");
        assert!(cmd.contains("install-tetron-wm-suite.sh"), "{cmd}");
        assert!(cmd.contains("read _"), "the window stays until Enter: {cmd}");
    }
}
