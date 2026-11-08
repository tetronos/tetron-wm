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

/// How much of the app set is installed (WMINST-005): the suite script's
/// sizes, the same as the tetron-wm / tetron-wm-desktop / tetron-wm-extras debs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Size {
    Minimal,
    Desktop,
    Extras,
}

impl Size {
    pub fn name(self) -> &'static str {
        match self {
            Size::Minimal => "minimal",
            Size::Desktop => "desktop",
            Size::Extras => "extras",
        }
    }

    fn parse(s: &str) -> Option<Size> {
        match s.trim() {
            "minimal" => Some(Size::Minimal),
            "desktop" => Some(Size::Desktop),
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

/// The installed size: the metapackages dpkg has, else the suite script's
/// record, else minimal.
pub fn size_from(info_dir: &Path, state: &str) -> Size {
    if dpkg_has(info_dir, "tetron-wm-extras") {
        Size::Extras
    } else if dpkg_has(info_dir, "tetron-wm-desktop") {
        Size::Desktop
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

/// The sizes Settings > General > Apps offers from `current`: the larger
/// ones, or at extras only extras itself (a check that adds anything a
/// release added).
pub fn size_choices(current: Size) -> Vec<Size> {
    match current {
        Size::Minimal => vec![Size::Desktop, Size::Extras],
        Size::Desktop => vec![Size::Extras],
        Size::Extras => vec![Size::Extras],
    }
}

/// The terminal command that installs the apps of `size` with the suite
/// script from `raw` (the branch's raw URL): apps only, no tetron-wm,
/// config, themes, conversion or console steps; waits for Enter at the end.
/// `list` is the app list Settings counted from ([`write_apps_list`]), handed
/// to the script so it installs the same list (WMCAT-004).
pub fn apps_install_command(raw: &str, size: Size, list: Option<&Path>) -> String {
    let s = size.name();
    let list = list.map(|p| format!(" --apps-list '{}'", p.to_string_lossy().replace('\'', "'\\''"))).unwrap_or_default();
    format!(
        "clear; echo 'Installing the {s} apps for tetron-wm (the suite installer)…'; echo; \
curl -fsSL {raw}/main/install-tetron-wm-suite.sh | bash -s -- --size {s} --no-wm --no-config --no-themes \
--no-convert --no-kmscon --no-console --no-autologin --no-autostart --no-fullscreen{list}; \
echo \"apps: suite installer for {s} exited with status $?\" >> \"$HOME/tetron-wm-debug.log\"; \
echo; printf 'Press Enter to close. '; read _"
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

fn size_rank(s: &str) -> u8 {
    match s {
        "desktop" => 1,
        "extras" => 2,
        _ => u8::MAX,
    }
}

fn size_level(s: Size) -> u8 {
    match s {
        Size::Minimal => 0,
        Size::Desktop => 1,
        Size::Extras => 2,
    }
}

/// The apps of `size` and the smaller sizes in `list` (the suite-apps.txt
/// format) that `present` says are missing on `host`, by display name.
/// Services never count: they are a separate, default-no choice.
pub fn missing_in(list: &str, size: Size, host: &Host, present: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut out = Vec::new();
    for line in list.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let f: Vec<&str> = line.split(':').collect();
        if f.len() < 3 || size_rank(f[0]) > size_level(size) {
            continue;
        }
        if f[1] == "script" {
            let (name, check) = match f[2] {
                "helpers" if host.packaged => continue,
                "helpers" => ("helpers", "tetron-wm-startpage"),
                n => (n, n),
            };
            if !present(check) {
                out.push(name.to_string());
            }
            continue;
        }
        if f.len() < 7 || f[1] == "services" || (f[1] == "gui" && !host.graphical) || (f[2] == "gpm" && !host.mouse) {
            continue;
        }
        let names = if host.linux { &f[3..6] } else { &f[6..7] };
        let Some(name) = names.iter().find(|n| !n.is_empty()) else { continue };
        if !present(f[2]) {
            out.push(name.to_string());
        }
    }
    out
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
    missing_in(crate::catalog::suite_apps(), size, &Host::probe(), &check_present)
}

/// The step of every update command that updates the apps with their own
/// updater (WMINST-009): uv itself when no package owns it, then the uv tools;
/// then opencode in ~/.local/bin when upstream's latest (stable) release
/// differs, its tarball checked against GitHub's sha256 digest (WMINST-010;
/// `opencode upgrade` would install a second copy in ~/.opencode/bin). Errors never stop the tetron-wm
/// update that follows.
pub fn tool_updates_snippet() -> &'static str {
    "u=\"$(command -v uv 2>/dev/null || { [ -x \"$HOME/.local/bin/uv\" ] && echo \"$HOME/.local/bin/uv\"; } || true)\"; \
if [ -n \"$u\" ]; then echo 'Updating uv and the apps it installed (catt, casttui)…'; \
if ! dpkg -S \"$(readlink -f \"$u\")\" >/dev/null 2>&1; then \"$u\" self update 2>/dev/null || true; fi; \
\"$u\" tool upgrade --all || true; echo \"update: uv self update and uv tool upgrade --all\" >> \"$HOME/tetron-wm-debug.log\"; echo; fi; \
o=\"$HOME/.local/bin/opencode\"; if [ -x \"$o\" ] && [ \"$(uname -s)\" = Linux ] && command -v python3 >/dev/null; then \
case \"$(uname -m)\" in x86_64|amd64) a=x64; grep avx2 /proc/cpuinfo >/dev/null 2>&1 || a=x64-baseline;; aarch64|arm64) a=arm64;; *) a=;; esac; \
[ -n \"$a\" ] && ldd --version 2>&1 | grep -i musl >/dev/null && a=\"$a-musl\"; f=\"opencode-linux-$a.tar.gz\"; \
rel=\"$(curl -fsSL https://api.github.com/repos/anomalyco/opencode/releases/latest 2>/dev/null | python3 -c 'import json,sys; r=json.load(sys.stdin); a=[x for x in r[\"assets\"] if x[\"name\"]==sys.argv[1]][0]; print(r[\"tag_name\"], a[\"browser_download_url\"], a[\"digest\"].split(\":\")[1])' \"$f\" 2>/dev/null)\"; \
t=\"${rel%% *}\"; t=\"${t#v}\"; v=\"$(\"$o\" --version 2>/dev/null | sed 's/.*v//')\"; \
if [ -n \"$a\" ] && [ -n \"$t\" ] && [ \"$t\" != \"$v\" ]; then echo \"Updating opencode $v to $t (checked against GitHub's sha256 digest)…\"; \
url=\"$(echo \"$rel\" | cut -d' ' -f2)\"; sum=\"$(echo \"$rel\" | cut -d' ' -f3)\"; d=\"$(mktemp -d)\"; \
if curl -fsSL -o \"$d/$f\" \"$url\" && echo \"$sum  $d/$f\" | sha256sum -c - >/dev/null && tar xzf \"$d/$f\" -C \"$d\" opencode && install -m 755 \"$d/opencode\" \"$o\"; \
then echo \"update: opencode $t\" >> \"$HOME/tetron-wm-debug.log\"; else echo 'opencode was not updated (download or checksum failed)'; fi; \
rm -rf \"$d\"; echo; fi; fi; "
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(linux: bool, graphical: bool, mouse: bool, packaged: bool) -> Host {
        Host { linux, graphical, mouse, packaged }
    }

    const LIST: &str = "# header\n\
desktop:apps:mpv:mpv:mpv:mpv:mpv\n\
desktop:apps:gpm:gpm:gpm:gpm:\n\
desktop:gui:xclip:xclip:xclip:xclip:\n\
desktop:apps:tetron-browser:tetron-browser:::\n\
desktop:services:nmcli:network-manager:NetworkManager:networkmanager:\n\
extras:apps:git:git:git:git:git\n\
desktop:apps:uv::uv:uv:uv\n\
desktop:apps:blueutil::::blueutil\n\
desktop:script:fresh\n\
desktop:script:nerd-font\n\
desktop:script:helpers\n\
desktop:script:opencode\n";

    #[test]
    fn missing_counts_the_size_and_smaller_never_services() {
        let none = |_: &str| false;
        let h = host(true, false, true, false);
        assert!(missing_in(LIST, Size::Minimal, &h, &none).is_empty(), "minimal has no apps");
        let d = missing_in(LIST, Size::Desktop, &h, &none);
        assert_eq!(d, ["mpv", "gpm", "tetron-browser", "uv", "fresh", "nerd-font", "helpers", "opencode"]);
        let e = missing_in(LIST, Size::Extras, &h, &none);
        assert!(e.contains(&"git".to_string()) && e.len() == d.len() + 1, "extras adds its own rows: {e:?}");
        assert!(!e.iter().any(|n| n == "network-manager"), "services never count");
    }

    #[test]
    fn missing_follows_the_host() {
        let none = |_: &str| false;
        let d = missing_in(LIST, Size::Desktop, &host(true, true, false, true), &none);
        assert!(d.contains(&"xclip".to_string()), "gui rows with a desktop session: {d:?}");
        assert!(!d.contains(&"gpm".to_string()), "no mouse device: no gpm");
        assert!(!d.contains(&"helpers".to_string()), "the deb ships the helpers");
        let mac = missing_in(LIST, Size::Desktop, &host(false, false, true, false), &none);
        assert!(mac.contains(&"blueutil".to_string()) && !mac.contains(&"gpm".to_string()), "brew names on macOS: {mac:?}");
        assert!(!mac.contains(&"tetron-browser".to_string()), "no brew name: not counted");
    }

    #[test]
    fn present_apps_are_not_missing() {
        let have = |c: &str| matches!(c, "mpv" | "gpm" | "tetron-browser" | "uv" | "fresh" | "nerd-font" | "tetron-wm-startpage" | "opencode");
        assert!(missing_in(LIST, Size::Desktop, &host(true, false, true, false), &have).is_empty());
    }

    #[test]
    fn the_compiled_list_parses_and_every_row_applies_somewhere() {
        let none = |_: &str| false;
        let linux = missing_in(crate::catalog::suite_apps(), Size::Extras, &host(true, true, true, false), &none);
        assert!(linux.len() > 20, "{linux:?}");
        assert!(linux.contains(&"fresh".to_string()) && linux.contains(&"git".to_string()));
    }

    #[test]
    fn tool_updates_snippet_is_valid_sh() {
        let ok = std::process::Command::new("sh").args(["-n", "-c", tool_updates_snippet()]).status().unwrap().success();
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
        std::fs::write(d.join("tetron-wm-desktop.list"), "/.\n").unwrap();
        assert_eq!(size_from(&d, "size=minimal\n"), Size::Desktop, "dpkg wins over the record");
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

    #[test]
    fn apps_row_offers_only_larger_sizes() {
        assert_eq!(size_choices(Size::Minimal), [Size::Desktop, Size::Extras]);
        assert_eq!(size_choices(Size::Desktop), [Size::Extras]);
        assert_eq!(size_choices(Size::Extras), [Size::Extras], "a check, not a downgrade");
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
        let cmd = apps_install_command("https://raw.githubusercontent.com/o/r", Size::Extras, None);
        assert!(std::process::Command::new("sh").args(["-n", "-c", &cmd]).status().unwrap().success(), "{cmd}");
        assert!(!cmd.contains("--apps-list"), "{cmd}");
        let odd = Path::new("/home/a b/it's/suite-apps.txt");
        let with = apps_install_command("https://raw.githubusercontent.com/o/r", Size::Desktop, Some(odd));
        assert!(std::process::Command::new("sh").args(["-n", "-c", &with]).status().unwrap().success(), "{with}");
        assert!(with.contains("--apps-list '/home/a b/it'\\''s/suite-apps.txt'"), "{with}");
        assert!(cmd.contains("--size extras") && cmd.contains("--no-wm") && cmd.contains("--no-console"), "{cmd}");
        assert!(cmd.contains("install-tetron-wm-suite.sh"), "{cmd}");
        assert!(cmd.contains("read _"), "the window stays until Enter: {cmd}");
    }
}
