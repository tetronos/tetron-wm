//! The app catalog (WMCAT-001 to WMCAT-004): `scripts/merge_catalog.py`
//! merges the source lists in `lists/` into `assets/catalog.json`, which is
//! compiled in. The Store's Update list button can replace it with a newer
//! copy from the versioned lists location ([`crate::listfetch`]), kept in
//! [`cache_path`] and read at start while it passes the same checks.
//!
//! It powers two things: detecting which catalogued TUIs are already installed
//! on the user's `$PATH`, and (in the Store slice) browsing/installing apps.
//! There is no reliable way to tell whether an arbitrary executable is a TUI, so
//! detection is restricted to this curated set rather than scanning all of `$PATH`.

use crate::config::AppEntry;
use serde::Deserialize;
use std::collections::HashSet;

/// One catalogued terminal application.
#[derive(Clone, Debug, Deserialize)]
pub struct CatalogApp {
    /// Display name (from awesome-tuis).
    pub name: String,
    /// Best-effort executable name used for `$PATH` detection and launching.
    pub bin: String,
    /// Category (awesome-tuis section, e.g. "Dashboards", "Editors").
    pub category: String,
    /// One-line description.
    pub description: String,
    /// Project homepage / repository URL.
    pub homepage: String,
    /// Whether this is a CLI tool (prints output and exits, or needs
    /// subcommands) rather than a persistent full-screen TUI. Absent in the
    /// catalog JSON means `false` (TUI), so the existing entries don't churn.
    #[serde(default)]
    pub cli: bool,
    /// The app exits immediately when started without a URL (text browsers such
    /// as w3m). A bare launch gets the configured `web_home` as its argument.
    #[serde(default)]
    pub needs_url: bool,
    /// Alternate launch variants of this app (e.g. a "skip permissions" flavor
    /// of an AI coding agent), each surfaced as its own extra launcher entry
    /// when the app is installed. Empty for most apps.
    #[serde(default)]
    pub variants: Vec<Variant>,
    /// How to install it, when we have a recipe.
    #[serde(default)]
    pub recipe: Option<Recipe>,
    /// `default`, `recommended` or `optional` (WMCAT-003).
    #[serde(default = "optional_tier")]
    pub tier: String,
    /// The smallest suite size that brings it (`essentials`, `recommended`, `extras`), if any.
    #[serde(default)]
    pub set: Option<String>,
    /// The GitHub repository's latest release has downloadable assets
    /// (WMCAT-006, from lists/releases.json); the Store links its page.
    #[serde(default)]
    pub releases: bool,
    /// Package names per package manager (WMCAT-005), from Repology
    /// (lists/repology.json) and lists/tetron.toml. Empty: no package known.
    #[serde(default)]
    pub pkg: Packages,
    /// Set when this app is an agent CLI the ✦ Assistant can run (WMAST-001).
    #[serde(default)]
    pub agent: Option<AgentSpec>,
}

/// How the ✦ Assistant runs a catalogued agent CLI (the entry's `agent`
/// table). Every field is optional so newer lists stay readable here.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct AgentSpec {
    /// Name in Settings; the app's name when empty.
    #[serde(default)]
    pub label: String,
    /// Command to run; the app's `bin` when empty.
    #[serde(default)]
    pub command: String,
    /// Arguments it always gets.
    #[serde(default)]
    pub args: Vec<String>,
    /// Other file names it reads its instructions from, linked to AGENTS.md.
    #[serde(default)]
    pub context_files: Vec<String>,
    /// How it gets the briefing when it runs outside the AI folder (WMAST-007).
    #[serde(default)]
    pub briefing: Option<Wiring>,
    /// How it is told about `tetron-wm mcp` (WMAST-006).
    #[serde(default)]
    pub mcp: Option<Wiring>,
}

/// How an agent is handed something from outside its working folder (the
/// briefing, the MCP server): a file written in the AI folder from a named
/// template, extra arguments, and environment variables. In args and env,
/// `{{BRIEFING}}` (AGENTS.md), `{{AI_DIR}}`, `{{MCP_JSON}}` and `{{MCP_BIN}}`
/// are filled.
#[derive(Clone, Debug, Default, Deserialize, serde::Serialize, PartialEq)]
pub struct Wiring {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub file: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub template: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub env: std::collections::BTreeMap<String, String>,
}

/// An app's package name for each package manager the Store uses; an empty
/// field means that manager has no package for it.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Packages {
    #[serde(default)]
    pub apt: String,
    #[serde(default)]
    pub dnf: String,
    #[serde(default)]
    pub pacman: String,
    #[serde(default)]
    pub brew: String,
}

impl Packages {
    /// Whether any distribution package manager (apt, dnf, pacman) has it.
    pub fn any_distro(&self) -> bool {
        !(self.apt.is_empty() && self.dnf.is_empty() && self.pacman.is_empty())
    }
}

fn optional_tier() -> String {
    "optional".into()
}

/// An alternate way to launch a catalogued app: same binary, different
/// arguments, shown as a separate launcher entry named `"<app name> <suffix>"`.
#[derive(Clone, Debug, Deserialize)]
pub struct Variant {
    /// Appended to the app's name to label the variant entry (e.g. "⚠️").
    pub suffix: String,
    /// Extra arguments passed to the binary for this variant.
    #[serde(default)]
    pub args: Vec<String>,
    /// When set, launching this variant asks for confirmation first (see
    /// `AppEntry::warn` / `launchwarn.rs`) — e.g. a flag that skips safety
    /// prompts in the wrapped tool.
    #[serde(default)]
    pub warn: Option<String>,
}

const CATALOG_JSON: &str = include_str!("../assets/catalog.json");

/// The catalog document layout this binary reads (`schema` in the file).
pub const SCHEMA: u32 = 1;

/// A curated install recipe for a catalogued app (its `recipe` field).
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Recipe {
    /// The shell command that installs the app.
    pub install: String,
    /// Install method ("brew", "cargo", "go", …) — informational.
    #[serde(default)]
    pub method: String,
    /// Whether this recipe was verified against the app's docs.
    #[serde(default)]
    pub verified: bool,
    /// Operating systems the app runs on (e.g. `["macos","linux"]`). Empty means
    /// "any/unknown" — shown everywhere.
    #[serde(default)]
    pub os: Vec<String>,
    /// Whether launching this app should first prompt for a working directory
    /// (true for coding agents that operate on a project tree).
    #[serde(default)]
    pub requires_cwd: bool,
    /// A one-paragraph setup tip shown in the Store's detail pane (e.g. "add
    /// models/providers with `hermes model` first").
    #[serde(default)]
    pub tip: String,
}

/// A parsed, checked catalog document.
#[derive(Debug, Deserialize)]
pub struct Catalog {
    pub schema: u32,
    pub apps: Vec<CatalogApp>,
    /// The suite installer's app list (assets/suite-apps.txt), carried whole.
    pub suite_apps: String,
}

/// Parse and check a catalog document: it parses, its schema is [`SCHEMA`],
/// it has apps, and its suite list has rows. `Err` says why not.
pub fn parse(text: &str) -> Result<Catalog, String> {
    let schema = serde_json::from_str::<serde_json::Value>(text)
        .map_err(|e| format!("not a catalog ({e})"))?
        .get("schema")
        .and_then(|v| v.as_u64())
        .ok_or("no schema number")?;
    if schema != SCHEMA as u64 {
        return Err(format!("schema {schema}, this tetron-wm reads {SCHEMA}"));
    }
    let c: Catalog = serde_json::from_str(text).map_err(|e| format!("bad catalog ({e})"))?;
    if c.apps.is_empty() {
        return Err("no apps".into());
    }
    if !c.suite_apps.lines().any(|l| !l.starts_with('#') && l.split(':').count() >= 3) {
        return Err("no suite app list".into());
    }
    Ok(c)
}

/// [`parse`], returning only the number of apps.
pub fn parse_count(text: &str) -> Result<usize, String> {
    parse(text).map(|c| c.apps.len())
}

/// Where the last good Update list download is kept.
pub fn cache_path() -> Option<std::path::PathBuf> {
    crate::theme::data_home_dir().map(|d| d.join("tetron-wm/catalog.json"))
}

/// The catalog in use: the cached download while it passes [`parse`], else the
/// built-in one. Replaced by [`install`]; each replacement leaks the old one
/// (a few hundred KB per Update list press) so `&'static` borrows stay valid.
static ACTIVE: std::sync::RwLock<Option<&'static Catalog>> = std::sync::RwLock::new(None);

fn builtin() -> Catalog {
    parse(CATALOG_JSON).expect("the built-in catalog passes its own checks")
}

fn active() -> &'static Catalog {
    if let Some(c) = *ACTIVE.read().unwrap() {
        return c;
    }
    let cached = if cfg!(test) { None } else { cache_path().and_then(|p| std::fs::read_to_string(p).ok()) };
    let c = match cached.map(|t| parse(&t)) {
        Some(Ok(c)) => c,
        Some(Err(e)) => {
            crate::dbg_log(&format!("catalog: cached list not used ({e}); using the built-in one"));
            builtin()
        }
        None => builtin(),
    };
    let c: &'static Catalog = Box::leak(Box::new(c));
    *ACTIVE.write().unwrap() = Some(c);
    c
}

/// Check a downloaded catalog and, when it passes, keep it in [`cache_path`]
/// and use it from now on. Returns the number of apps.
pub fn install(text: &str) -> Result<usize, String> {
    let c = parse(text)?;
    let path = cache_path().ok_or("no data directory")?;
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("could not create {}: {e}", d.display()))?;
    }
    let part = path.with_extension("json.part");
    std::fs::write(&part, text).and_then(|_| std::fs::rename(&part, &path)).map_err(|e| format!("could not save {}: {e}", path.display()))?;
    let n = c.apps.len();
    *ACTIVE.write().unwrap() = Some(Box::leak(Box::new(c)));
    Ok(n)
}

/// The suite installer's app list of the catalog in use (WMCAT-003).
pub fn suite_apps() -> &'static str {
    &active().suite_apps
}

/// The current operating system as a recipe `os` token ("macos", "linux", …).
pub fn current_os() -> &'static str {
    std::env::consts::OS
}

/// Whether an app is applicable to the current OS (true unless its recipe lists
/// operating systems that exclude this one and no package for this OS exists).
/// Terminal web browsers that open a URL given as their argument, in the
/// order the Store tries them for a releases page (WMCAT-006).
pub const WEB_BROWSERS: [&str; 5] = ["tetron-browser", "w3m", "lynx", "links", "elinks"];

/// The releases page of an app with prebuilt binaries (WMCAT-006):
/// `https://github.com/<owner>/<repo>/releases/latest`, built only from a
/// homepage that is a GitHub repository root with plain owner and repository
/// names, because the link is echoed in the install shell.
pub fn releases_url(app: &CatalogApp) -> Option<String> {
    if !app.releases {
        return None;
    }
    let rest = app.homepage.strip_prefix("https://github.com/")?.trim_end_matches('/');
    let (owner, repo) = rest.split_once('/')?;
    let plain = |s: &str| !s.is_empty() && !s.starts_with('.') && s.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    (plain(owner) && plain(repo)).then(|| format!("https://github.com/{owner}/{repo}/releases/latest"))
}

pub fn runs_on_current_os(name: &str) -> bool {
    let Some(a) = catalog().iter().find(|a| a.name == name) else { return true };
    match &a.recipe {
        Some(r) if !r.os.is_empty() => {
            r.os.iter().any(|o| o == current_os()) || has_package_for(&a.pkg, current_os())
        }
        _ => true,
    }
}

/// Whether `p` names a package for a manager that runs on `os`: the
/// distribution managers on Linux, Homebrew on macOS and Linux.
pub fn has_package_for(p: &Packages, os: &str) -> bool {
    match os {
        "linux" => p.any_distro() || !p.brew.is_empty(),
        "macos" => !p.brew.is_empty(),
        _ => false,
    }
}

/// The catalog in use.
pub fn catalog() -> &'static [CatalogApp] {
    &active().apps
}

/// The curated install recipe for `name`, if one exists.
pub fn recipe(name: &str) -> Option<&'static Recipe> {
    catalog().iter().find(|a| a.name == name)?.recipe.as_ref()
}

/// Count of verified recipes (progress indicator).
pub fn verified_count() -> usize {
    catalog().iter().filter(|a| a.recipe.as_ref().is_some_and(|r| r.verified)).count()
}

/// Look up the category for a known app by its display name or binary.
pub fn category_for(name_or_bin: &str) -> Option<String> {
    catalog()
        .iter()
        .find(|c| c.name.eq_ignore_ascii_case(name_or_bin) || c.bin.eq_ignore_ascii_case(name_or_bin))
        .map(|c| c.category.clone())
}

/// Whether a known app wants a working-directory prompt on launch, by display
/// name or binary. `None` when the app isn't in the catalog (so callers can keep
/// an explicitly-configured flag instead of overriding it).
pub fn requires_cwd_for(name_or_bin: &str) -> Option<bool> {
    let c = catalog().iter().find(|c| {
        c.name.eq_ignore_ascii_case(name_or_bin) || c.bin.eq_ignore_ascii_case(name_or_bin)
    })?;
    Some(recipe(&c.name).map(|r| r.requires_cwd).unwrap_or(false))
}

/// Whether a known app is a CLI tool (not a persistent TUI), by display name or
/// binary. Apps not in the catalog default to `false` (unknown apps launch as-is).
pub fn is_cli(name_or_bin: &str) -> bool {
    catalog()
        .iter()
        .find(|c| c.name.eq_ignore_ascii_case(name_or_bin) || c.bin.eq_ignore_ascii_case(name_or_bin))
        .map(|c| c.cli)
        .unwrap_or(false)
}

/// Whether a bare launch of `command` (a name or a path) must be given a start
/// URL because the app would otherwise print its usage and exit.
pub fn needs_url(command: &str) -> bool {
    let base = command.rsplit('/').next().unwrap_or(command);
    catalog().iter().any(|c| c.needs_url && c.bin.eq_ignore_ascii_case(base))
}

/// Cached set of `$PATH` executable names. `None` until first use; replaced by
/// [`refresh_installed`] after an install so newly-added binaries are detected
/// without restarting the daemon.
static BINS: std::sync::RwLock<Option<HashSet<String>>> = std::sync::RwLock::new(None);

/// Snapshot of the current `$PATH` executables (scanning + caching on first use).
fn path_bins() -> HashSet<String> {
    if let Some(set) = BINS.read().unwrap().clone() {
        return set;
    }
    let set = path_executables();
    *BINS.write().unwrap() = Some(set.clone());
    set
}

/// Re-scan `$PATH`, replacing the cache. Call after an install completes so the
/// store and launcher pick up the newly-installed binary.
pub fn refresh_installed() {
    *BINS.write().unwrap() = Some(path_executables());
}

/// Whether an executable `bin` is present on `$PATH`.
pub fn is_installed(bin: &str) -> bool {
    // Check the cache in place: this runs per candidate per frame while a tray
    // popover is open, so it must not clone the whole set.
    if let Some(set) = BINS.read().unwrap().as_ref() {
        return set.contains(&bin.to_lowercase());
    }
    path_bins().contains(&bin.to_lowercase())
}

/// Return catalog apps whose binary is present on the current `$PATH`, plus
/// one extra entry per declared [`Variant`] of each installed app (e.g. the
/// Claude Code "⚠️" skip-permissions flavor) — data-driven, no per-app
/// special-casing here.
pub fn detect_installed() -> Vec<AppEntry> {
    let bins = path_bins();
    let mut entries = Vec::new();
    for c in catalog().iter().filter(|c| bins.contains(&c.bin) || bins.contains(&c.name.to_lowercase())) {
        let requires_cwd = Some(recipe(&c.name).map(|r| r.requires_cwd).unwrap_or(false));
        entries.push(AppEntry {
            name: c.name.clone(),
            command: c.bin.clone(),
            args: Vec::new(),
            category: Some(c.category.clone()),
            requires_cwd,
            cwd: None,
            cli: Some(c.cli),
            warn: None,
        });
        for v in &c.variants {
            entries.push(AppEntry {
                name: format!("{} {}", c.name, if v.suffix == "\u{26A0}\u{FE0F}" { crate::iconset::warning() } else { v.suffix.as_str() }),
                command: c.bin.clone(),
                args: v.args.clone(),
                category: Some(c.category.clone()),
                requires_cwd,
                cwd: None,
                // Variants launch exactly as declared, not through the CLI
                // help-then-shell wrapper.
                cli: None,
                warn: v.warn.clone(),
            });
        }
    }
    entries
}

/// The set of executable names found across `$PATH` (lowercased), scanned once.
fn path_executables() -> HashSet<String> {
    let mut set = HashSet::new();
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    if let Ok(name) = entry.file_name().into_string() {
                        set.insert(name.to_lowercase());
                    }
                }
            }
        }
    }
    set
}

/// `PATH` with `~/.local/bin` appended when it is not on it yet, else `None`.
/// The suite installer, uv and the Settings Apps choices put apps there
/// (opencode for the ✦ Assistant among them), but a login that starts
/// tetron-wm before `~/.profile` runs (the TetronOS console session), or one
/// that began before the folder existed, has no such entry, so those apps
/// would read as missing and children could not start them.
pub fn path_with_user_bin(path: Option<&std::ffi::OsStr>, home: &std::path::Path) -> Option<std::ffi::OsString> {
    let bin = home.join(".local/bin");
    let mut dirs: Vec<std::path::PathBuf> = path.map(|p| std::env::split_paths(p).collect()).unwrap_or_default();
    if dirs.iter().any(|d| d == &bin) {
        return None;
    }
    dirs.push(bin);
    std::env::join_paths(dirs).ok()
}

/// Apply [`path_with_user_bin`] to this process, so the app scan and every
/// child (terminals, the ✦ Assistant, Settings scripts) see `~/.local/bin`.
/// Called first thing in `main`, before any thread starts.
pub fn add_user_bin_to_path() {
    if let Some(home) = dirs::home_dir() {
        if let Some(p) = path_with_user_bin(std::env::var_os("PATH").as_deref(), &home) {
            std::env::set_var("PATH", p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_bin_is_appended_once() {
        let h = std::path::Path::new("/home/u");
        let p = path_with_user_bin(Some(std::ffi::OsStr::new("/usr/local/bin:/usr/bin:/bin")), h).unwrap();
        assert_eq!(p, "/usr/local/bin:/usr/bin:/bin:/home/u/.local/bin");
        assert_eq!(path_with_user_bin(Some(std::ffi::OsStr::new("/home/u/.local/bin:/usr/bin")), h), None);
        assert_eq!(path_with_user_bin(None, h).unwrap(), "/home/u/.local/bin");
    }

    #[test]
    fn common_editors_are_catalogued_so_the_launcher_lists_them() {
        // Detection lists only catalogued apps: an installed vim, nano or
        // nvim must be in the catalog, in Editors, to reach Launcher > Editors.
        for bin in ["vim", "nvim", "nano"] {
            let c = catalog().iter().find(|c| c.bin == bin).unwrap_or_else(|| panic!("{bin} must be in the catalog"));
            assert_eq!(c.category, "Editors", "{bin}");
            assert!(!c.cli, "{bin} is a full-screen editor, not a command that prints and exits");
            assert!(c.pkg.any_distro(), "{bin} installs through the Store with the host package manager");
        }
    }

    /// `detect_installed` emits a base entry plus one extra entry per declared
    /// variant, carrying the variant's args/warn through and leaving `cli`
    /// unset (variants launch exactly as declared). Fakes the `$PATH` scan via
    /// the private `BINS` cache — real `is_installed`/`detect_installed`
    /// callers elsewhere have no inline tests, so this can't race them.
    #[test]
    fn detect_installed_emits_variant_entry_for_claude_code() {
        let mut fake = HashSet::new();
        fake.insert("claude".to_string());
        *BINS.write().unwrap() = Some(fake);

        let entries = detect_installed();

        let base = entries.iter().find(|e| e.name == "Claude Code").expect("base entry present");
        assert_eq!(base.command, "claude");
        assert!(base.args.is_empty());
        assert_eq!(base.warn, None, "the plain entry carries no warning");

        let variant = entries.iter().find(|e| e.name == format!("Claude Code {}", crate::iconset::warning())).expect("variant entry present");
        assert_eq!(variant.command, "claude");
        assert_eq!(variant.args, vec!["--dangerously-skip-permissions".to_string()]);
        assert_eq!(variant.cli, None, "variants launch as given, not through the CLI wrapper");
        assert_eq!(variant.category, base.category);
        assert_eq!(variant.requires_cwd, base.requires_cwd);
        assert!(variant.warn.as_deref().unwrap_or("").contains("--dangerously-skip-permissions"));

        // Leave the cache empty so a later real scan repopulates it lazily.
        *BINS.write().unwrap() = None;
    }

    /// The built-in catalog is the merged document: it passes the same checks
    /// a download must pass, recipes are folded in, and the suite list rides
    /// along (WMCAT-002, WMCAT-003).
    #[test]
    fn builtin_catalog_is_one_checked_document_with_recipes_tiers_and_sets() {
        let c = parse(CATALOG_JSON).expect("built-in passes");
        assert_eq!(c.schema, SCHEMA);
        assert!(c.apps.len() > 600);
        assert!(c.apps.iter().filter(|a| a.recipe.is_some()).count() > 600);
        assert!(c.apps.iter().all(|a| ["default", "recommended", "optional"].contains(&a.tier.as_str())), "tiers");
        let fresh = c.apps.iter().find(|a| a.bin == "fresh").expect("fresh");
        assert_eq!((fresh.set.as_deref(), fresh.tier.as_str()), (Some("essentials"), "default"));
        let lazygit = c.apps.iter().find(|a| a.bin == "lazygit").expect("lazygit");
        assert_eq!((lazygit.set.as_deref(), lazygit.tier.as_str()), (Some("recommended"), "recommended"));
        assert_eq!(c.suite_apps, include_str!("../assets/suite-apps.txt"));
        assert!(recipe("Claude Code").is_some_and(|r| r.requires_cwd));
    }

    /// A download is refused unless it is a catalog of the schema this binary
    /// reads, with apps and a suite list (WMCAT-004).
    #[test]
    fn parse_refuses_other_schemas_empty_lists_and_garbage() {
        let doc = |schema: u32, apps: &str, suite: &str| format!(r#"{{"schema":{schema},"apps":[{apps}],"suite_apps":"{suite}"}}"#);
        let app = r#"{"name":"x","bin":"x","category":"c","description":"d","homepage":"h"}"#;
        let suite = "desktop:apps:x:x:x:x:x\\n";
        assert_eq!(parse_count(&doc(SCHEMA, app, suite)), Ok(1));
        assert!(parse(&doc(SCHEMA + 1, app, suite)).unwrap_err().contains("schema"));
        assert_eq!(parse(&doc(SCHEMA, "", suite)).unwrap_err(), "no apps");
        assert_eq!(parse(&doc(SCHEMA, app, "# only a comment")).unwrap_err(), "no suite app list");
        assert!(parse("<html>404</html>").is_err());
        assert!(parse(r#"[{"name":"old bare list"}]"#).unwrap_err().contains("schema"));
        let c = parse(&doc(SCHEMA, app, suite)).unwrap();
        assert_eq!((c.apps[0].tier.as_str(), c.apps[0].set.is_none()), ("optional", true));
    }

    /// tetron-wm's own helpers: the launcher finds them by a prefixed name on
    /// PATH (never a generic `startpage`/`aimode`), and each Store recipe
    /// fetches a script that exists in `scripts/` under that same name.
    #[test]
    fn own_helpers_use_prefixed_names_backed_by_scripts() {
        for (name, bin) in [("Start Page", "tetron-wm-startpage"), ("AI Mode", "tetron-aimode")] {
            let app = catalog().iter().find(|a| a.name == name).unwrap_or_else(|| panic!("{name} in the catalog"));
            assert_eq!(app.bin, bin);
            let r = recipe(name).unwrap_or_else(|| panic!("{name} recipe"));
            assert!(r.install.contains(&format!("/scripts/{bin} ")), "{}", r.install);
            assert!(r.install.contains(&format!("~/.local/bin/{bin}")), "{}", r.install);
            assert!(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts").join(bin).is_file());
        }
    }
}
