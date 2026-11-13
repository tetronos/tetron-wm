use serde::{Deserialize, Serialize};

/// An `[[assistant_agent]]` table: an agent added by hand, or fields that
/// override the catalog agent with the same `id` (WMAST-001).
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct AgentDef {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_files: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub env: std::collections::BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp: Option<crate::catalog::McpWiring>,
}

/// A single application entry in the config file.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct AppEntry {
    /// Human-readable name shown in the dock.
    pub name: String,
    /// Executable to launch (resolved via `$PATH`).
    pub command: String,
    /// Optional extra arguments passed to the executable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// Optional launcher category (e.g. "System", "Git"). Apps without one are
    /// grouped under "Apps".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Prompt for a working directory on launch (overrides the catalog flag).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_cwd: Option<bool>,
    /// Fixed working directory; when set, launches there and skips the picker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Whether this is a CLI tool rather than a persistent TUI (overrides the
    /// catalog flag); shown with a `CLI` badge and launched via a shell wrapper
    /// so its `--help` output stays visible before dropping into a shell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli: Option<bool>,
    /// When set, launching this entry first shows a confirmation dialog with
    /// this message (see `launchwarn.rs`) instead of launching immediately —
    /// e.g. a variant that skips a tool's own safety prompts. User config
    /// entries can set this directly, not just catalog variants.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warn: Option<String>,
}

/// Top-level configuration for tetron-wm.
///
/// All fields have sensible defaults; missing keys in the TOML file
/// fall back to [`Default::default`].
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Config {
    /// Whether dragging a window to a screen edge snaps it.
    pub snapping_enabled: bool,
    /// Distance in cells from the screen edge that triggers snapping.
    pub snap_threshold: i32,
    /// Whether windows draw drop shadows.
    pub window_shadows: bool,
    /// Invert mouse-wheel / touchpad scroll direction ("natural" scrolling):
    /// scrolling down goes back into history instead of toward the live bottom.
    /// Affects the wheel only, not the Shift+PageUp/PageDown keys.
    /// (No per-field serde default: inherit the struct Default, which is true.)
    pub natural_scroll: bool,
    /// Tiling grid rows (1..=6).
    #[serde(default = "default_grid_rows")]
    pub grid_rows: u8,
    /// Tiling grid columns (1..=6).
    #[serde(default = "default_grid_cols")]
    pub grid_cols: u8,
    /// Cells of gutter between tiled windows.
    #[serde(default)]
    pub tile_gap: i32,
    /// Auto-arrange all windows into the grid.
    #[serde(default)]
    pub auto_tile: bool,
    /// Open new app windows maximized (filling the work area).
    #[serde(default)]
    pub launch_maximized: bool,
    /// Full screen video, images and PDFs (right-click items, the viewer `f` key).
    /// Off skips the `/dev/fb0`, DRM and waiter probes and hides the items.
    #[serde(default = "default_true")]
    pub fullscreen: bool,
    /// Active color theme name: a built-in preset (`theme::PRESETS`) or the stem
    /// of a `<config>/themes/<name>.toml` file (which, same name, overrides a
    /// built-in).
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Directory to load `*.toml` theme files from. Unset = `<config>/themes/`.
    /// A relative path is resolved against the config dir.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme_dir: Option<String>,
    /// Terminal default background/foreground, independent of the desktop theme
    /// (hex `#rrggbb` or a named color). Unset = follow the theme's `window_bg`/
    /// `text`, so e.g. the pastoral theme gives black-on-white terminals. Set these
    /// to decouple the terminal, e.g. a light desktop with a dark terminal.
    /// Applied by the apphost at startup; a change takes effect on the next
    /// apphost start (`tetron-wm kill` then relaunch), not a bare `reload`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_bg: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_fg: Option<String>,
    /// Scrollback lines kept per app window (0 = none, capped at 100000). The
    /// main memory cost of a busy window. Read when a window opens, so a change
    /// applies to new windows without restarting the apphost.
    pub scrollback: usize,
    /// How the image viewer draws: "auto" (Kitty graphics when the terminal
    /// has them, never on a bare VT; otherwise cells), "graphics", "cells".
    pub image_viewer: String,
    /// Colour depth: "auto" (what the terminal reports), "mono" (black and
    /// white), "16", "256" or "truecolor". `TETRON_WM_COLORS` overrides it.
    pub colors: String,
    /// The Chromecast for the right-click "Cast to Chromecast": a device name
    /// or IP, as catt's `-d` takes it. Empty = catt's own default device.
    pub cast_device: String,
    /// Force 24-bit ("truecolor") SGR output on/off. Unset = auto-detect from the
    /// terminal (known truecolor terminals + `COLORTERM`). Set `true` when your
    /// terminal supports truecolor but does not advertise it (many set
    /// `TERM=xterm-256color` without `COLORTERM`), else subtle themes (such as
    /// nord) are quantized to the 256-color palette and look wrong. The
    /// `TETRON_WM_TRUECOLOR` env var (1/0) overrides this key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truecolor: Option<bool>,
    /// Desktop wallpaper image. An absolute path, a `~`-path, or a path relative
    /// to the config dir (`~/.config/tetron-wm/`, checking its `themes/` subdir
    /// first, then the root, then the old `wallpapers/` subdir). Rendered to colored cells via `chafa` (so it works on any terminal,
    /// including a kmscon console) and composited behind the windows. Quotes, a
    /// `file://` URL and a leading `$HOME` are accepted (see
    /// `wallpaper::clean_spec`). Needs `chafa` installed; if it is missing or
    /// the image fails to load, the desktop falls back to solid.
    /// The `TETRON_WM_WALLPAPER` env var overrides this key (for quick testing).
    /// Defaults to `default`, the built-in image (empty means the same). A
    /// missing file also falls back to the built-in image, so the desktop is
    /// only solid with `wallpaper_enabled = false`.
    /// Three more values need no file and no `chafa`: `black`, a `#rrggbb`
    /// colour (a flat desktop of that colour) and `none` (the theme's own
    /// `desktop_bg`). Below 256 colours `default` is the theme's `desktop_bg`
    /// too, since the image turns to noise there; a file you chose is still drawn.
    #[serde(default = "default_wallpaper", skip_serializing_if = "Option::is_none")]
    pub wallpaper: Option<String>,
    /// Master on/off for the wallpaper. When false, the desktop is solid even if
    /// `wallpaper` names an image (the Settings → Appearance toggle flips this).
    #[serde(default = "default_true")]
    pub wallpaper_enabled: bool,
    /// Glyph set for file-type icons where the Kitty graphics image icons can't
    /// render (kmscon, a bare VT, plain SSH): "auto" (default: ascii on a bare
    /// VT, unicode elsewhere), "ascii" (renders in every font), "unicode"
    /// (DejaVu Sans Mono symbols), "nerd" (Nerd Font glyphs -- needs one
    /// installed, tetron-os ships it), or "emoji". See `iconset::IconStyle`.
    #[serde(default)]
    pub icon_style: crate::iconset::IconStyle,
    /// Root directory the working-directory picker opens at (default `~`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_project_dir: Option<String>,
    /// LEGACY: recently chosen working directories. Now kept in `state.toml`
    /// (see [`crate::state`]); still read from an old config for one-time
    /// migration and never written back.
    #[serde(default, skip_serializing)]
    pub recent_dirs: Vec<String>,
    /// Show hidden (dot) directories in the picker by default.
    #[serde(default)]
    pub show_hidden_dirs: bool,
    /// Ordered list of apps auto-started at launch (and shown in the dock).
    pub apps: Vec<AppEntry>,
    /// Apps offered in the launcher menu / spotlight. Falls back to `apps`
    /// (via [`Config::launcher_apps`]) when left empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub launcher: Vec<AppEntry>,
    /// File-role → handler map for opening files (see `openwith`).
    #[serde(default)]
    pub default_apps: std::collections::BTreeMap<String, String>,
    /// `[open_with]`: extra programs for the right-click "Open with…" menu, per
    /// role (`video = ["casttui"]`). Always shown, in addition to the detected ones.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub open_with: std::collections::BTreeMap<String, Vec<String>>,
    /// Persisted default file-manager view (`"icon"` or `"list"`); wired fully in
    /// a later stage — stored here so the config round-trips.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filemanager_view: Option<String>,
    /// Whether desktop icons are shown on the wallpaper.
    #[serde(default = "default_true")]
    pub desktop_enabled: bool,
    /// Background "scrim" drawn behind desktop icon glyphs and labels **when a
    /// wallpaper is shown**, for legibility. A color (`#rrggbb`, `#rrggbbaa`, or
    /// a named color); `"none"` (or `"off"`) disables it so labels sit directly
    /// on the wallpaper. Unset = a semi-transparent black (`#000000c8`). It has
    /// no effect on the plain (solid) desktop, which never draws a scrim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop_scrim: Option<String>,
    /// Pinned desktop shortcuts (reuses AppEntry); shown alongside ~/Desktop files.
    /// Seeded with Files + Store for existing configs that predate this field.
    #[serde(default = "default_desktop_pins", skip_serializing_if = "Vec::is_empty")]
    pub desktop_pins: Vec<AppEntry>,
    /// LEGACY: saved desktop icon grid positions. Now kept in `state.toml`
    /// (see [`crate::state`]); still read from an old config for one-time
    /// migration and never written back.
    #[serde(default, skip_serializing)]
    pub desktop_positions: std::collections::BTreeMap<String, (u16, u16)>,
    /// Agent the ✦ assistant runs (default: `opencode`): an agent id from the
    /// catalog or `assistant_agent`, else a command line (WMAST-001).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_command: Option<String>,
    /// Extra arguments passed to the assistant CLI.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assistant_args: Vec<String>,
    /// Agents added by hand, or overrides of catalog agents by `id`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assistant_agent: Vec<AgentDef>,
    /// Hand the agent the `tetron-wm mcp` desktop tools (WMAST-006).
    #[serde(default)]
    pub assistant_mcp: bool,
    /// Folder the assistant clones and builds suite code in (default `~/code`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assistant_code_dir: Option<String>,
    /// How the ✦ assistant opens: "panel" (right-docked) or "window" (floating).
    #[serde(default = "default_assistant_mode")]
    pub assistant_mode: String,
    /// Git branch the in-app updater tracks ("main" = stable prebuilt releases;
    /// any other = build that branch from source, e.g. a "dev" test channel).
    #[serde(default = "default_update_branch")]
    pub update_branch: String,
    /// Keep a history of everything copied (OSC 52 stores, Ctrl+Shift+C, mouse
    /// selections, the `clip` CLI). Browse it from the menubar clipboard segment.
    pub clipboard_history: bool,
    /// Save the history to `~/.local/share/tetron-wm/clipboard-history.jsonl`
    /// (mode 0600). Turn off for memory-only history.
    pub clipboard_persist: bool,
    /// Record mouse text selections in the history (not just explicit copies).
    pub clipboard_record_selection: bool,
    /// Maximum unpinned history entries.
    pub clipboard_history_size: usize,
    /// Entries larger than this many KB are not recorded.
    pub clipboard_item_max_kb: usize,
    /// Put xclip/xsel/wl-copy/wl-paste shims on the PATH of launched apps so
    /// they use this clipboard: "auto" (only when there is no display), "on", "off".
    pub clipboard_shims: String,
    /// Give launched apps a dummy `WAYLAND_DISPLAY` when there is no display, so
    /// tools that only look for a clipboard helper when a display variable is set
    /// (Neovim) use the shims. Side effect: programs that test for a desktop
    /// session (xdg-open, GUI toolkits) think one exists, so it is off by default.
    pub clipboard_fake_display: bool,
    /// Mirror the real X11/Wayland clipboard into the history: "auto" (when a
    /// display exists), "on", "off".
    pub clipboard_gui_bridge: String,
    /// Who may READ the clipboard with an OSC 52 query: "focused" (only the
    /// focused window), "always", "never".
    pub clipboard_osc52_read: String,
    /// Do not record copies made while an app whose name or command contains
    /// one of these (case-insensitive) is focused, e.g. password managers.
    pub clipboard_ignore_apps: Vec<String>,
    /// Expire unpinned entries after this many seconds (0 = never).
    pub clipboard_ttl_secs: u64,
    /// Start page for text web browsers launched without a URL (w3m exits at
    /// once without one). Empty = launch them bare.
    pub web_home: String,
    /// Screensaver effect used when it is started without naming one: `black`
    /// or `clock` (see `tetron-wm screensaver list`). It runs on demand only.
    pub screensaver: String,
    /// `[power_commands]`: per-action command chains for Suspend / Reboot / Power
    /// off (`suspend`, `reboot`, `poweroff`). A value is a command line or a list
    /// of them, tried in order until one succeeds; it replaces the built-in chain
    /// (systemd, elogind, sudo/doas, busybox). An empty list hides the action.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub power_commands: crate::traytools::TrayTools,
    /// Show the launcher's Power submenu (Suspend / Reboot / Power off). Set
    /// `false` on machines that must never be shut down from here (remote
    /// servers); the actions are then refused everywhere, not just hidden.
    pub power_menu: bool,
    /// Text of the menubar launcher button: up to 6 characters (shorter labels
    /// are centred in the same 6 cells, so the bar never shifts), or `@session`
    /// to show the session name in a named session. Single-width characters only.
    pub launcher_label: String,
    /// Where screenshots are saved. Empty = `~/Pictures` if it exists, else `$HOME`.
    pub screenshot_dir: String,
    /// Leader chord, `ctrl+<key>` (default `ctrl+space`). Needs `tetron-wm reload`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leader: Option<String>,
    /// `[keys]` table: leader-key overrides per action, e.g. `launcher = "l"` or
    /// `launcher = ["l", "enter"]`. Actions and defaults: see `tetron-wm --help`.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub keys: std::collections::BTreeMap<String, crate::keymap::KeySpec>,
    /// `[tray_tools]`: per-item "Open <tool>" candidates for the tray popovers
    /// (`cpu`, `mem`, `battery`, `network`); the first installed wins. A value
    /// is a command line or a list of them; a `hold:` prefix keeps the window
    /// open until Enter; an empty list removes the row.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub tray_tools: crate::traytools::TrayTools,
    /// `[tray_placement]`: where each status segment lives, by key (`cpu`, `mem`,
    /// `battery`, `bluetooth`, `wifi`, `tetron`, `bell`): `"bar"` (on the bar),
    /// `"info"` (in the ⓘ menu) or `"auto"` (on the bar, overflowing into the ⓘ
    /// menu when space runs out). Defaults: cpu, mem and bluetooth `info`, the
    /// rest `auto`. A CPU / memory reading of 90% or more shows on the bar anyway.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub tray_placement: std::collections::BTreeMap<String, String>,
    /// Per-app dock badge colors: keyword (matched case-insensitively as a
    /// substring of the app name/command) → color (named or `#rrggbb`).
    #[serde(default = "default_dock_badges", skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub dock_badges: std::collections::BTreeMap<String, String>,
    /// How much goes into the log (`~/.local/state/tetron-wm/debug.log`):
    /// `error`, `warn`, `info` (default: events such as launches, closes,
    /// settings changes, updates) or `debug` (also every apphost message and
    /// every wallpaper load). `TETRON_WM_DEBUG=1` forces `debug` for one run.
    #[serde(default)]
    pub log_level: crate::logfile::Level,
}

impl Config {
    /// Resolve the desktop icon-label scrim color (see [`desktop_scrim`]).
    /// Unset → a semi-transparent black; `"none"`/`"off"` → transparent (no
    /// scrim); any other value → that color, falling back to the default when it
    /// does not parse.
    ///
    /// [`desktop_scrim`]: Self::desktop_scrim
    pub fn desktop_scrim_color(&self) -> crate::cell::Rgba {
        const DEFAULT: crate::cell::Rgba = crate::cell::Rgba { r: 0, g: 0, b: 0, a: 200 };
        match self.desktop_scrim.as_deref().map(str::trim) {
            None | Some("") => DEFAULT,
            Some(s) if s.eq_ignore_ascii_case("none") || s.eq_ignore_ascii_case("off") => {
                crate::cell::Rgba::TRANSPARENT
            }
            Some(s) => crate::badge::parse_color(s).unwrap_or(DEFAULT),
        }
    }

    /// The apps the launcher should offer: the explicit `launcher` list, or the
    /// autostart `apps` when no launcher list is configured.
    pub fn launcher_apps(&self) -> Vec<AppEntry> {
        if self.launcher.is_empty() {
            self.apps.clone()
        } else {
            self.launcher.clone()
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            // Defaults: no drag-to-edge snapping and no window drop shadows, so
            // a box with no config.toml behaves the way we want out of the box.
            snapping_enabled: false,
            snap_threshold: 3,
            window_shadows: false,
            natural_scroll: true,
            grid_rows: 2,
            grid_cols: 2,
            tile_gap: 0,
            auto_tile: false,
            launch_maximized: false,
            fullscreen: true,
            theme: "midnight".into(),
            theme_dir: None,
            terminal_bg: None,
            terminal_fg: None,
            scrollback: DEFAULT_SCROLLBACK,
            image_viewer: "auto".into(),
            colors: "auto".into(),
            cast_device: String::new(),
            truecolor: None,
            wallpaper: default_wallpaper(),
            wallpaper_enabled: true,
            icon_style: crate::iconset::IconStyle::default(),
            default_project_dir: None,
            recent_dirs: Vec::new(),
            show_hidden_dirs: false,
            // No apps auto-start by default — launch from the menu/desktop instead.
            apps: vec![],
            launcher: vec![],
            default_apps: crate::openwith::default_handlers(),
            open_with: std::collections::BTreeMap::new(),
            filemanager_view: None,
            desktop_enabled: true,
            desktop_scrim: None,
            desktop_pins: default_desktop_pins(),
            desktop_positions: std::collections::BTreeMap::new(),
            assistant_command: None,
            assistant_args: Vec::new(),
            assistant_agent: Vec::new(),
            assistant_mcp: false,
            assistant_code_dir: None,
            assistant_mode: default_assistant_mode(),
            update_branch: default_update_branch(),
            clipboard_history: true,
            clipboard_persist: true,
            clipboard_record_selection: true,
            clipboard_history_size: 100,
            clipboard_item_max_kb: 256,
            clipboard_shims: "auto".into(),
            clipboard_fake_display: false,
            clipboard_gui_bridge: "auto".into(),
            clipboard_osc52_read: "focused".into(),
            clipboard_ignore_apps: Vec::new(),
            clipboard_ttl_secs: 0,
            web_home: "https://lite.duckduckgo.com/lite/".into(),
            launcher_label: "tetron".into(),
            power_menu: true,
            power_commands: std::collections::BTreeMap::new(),
            screensaver: crate::screensaver::DEFAULT_EFFECT.into(),
            screenshot_dir: String::new(),
            leader: None,
            keys: std::collections::BTreeMap::new(),
            tray_tools: std::collections::BTreeMap::new(),
            tray_placement: std::collections::BTreeMap::new(),
            dock_badges: default_dock_badges(),
            log_level: crate::logfile::Level::Info,
        }
    }
}

fn default_true() -> bool { true }

/// Default scrollback lines per app window (alacritty's own default).
pub const DEFAULT_SCROLLBACK: usize = 10_000;
/// Upper bound for `scrollback`, so a typo can not reserve gigabytes per window.
pub const MAX_SCROLLBACK: usize = 100_000;

/// Default wallpaper: the `default` keyword, meaning the built-in image.
fn default_wallpaper() -> Option<String> { Some(crate::wallpaper::DEFAULT_NAME.into()) }

/// Default desktop shortcuts: none. The desktop shows only the real contents of
/// `~/Desktop`; pins are opt-in via `[[desktop_pins]]`.
fn default_desktop_pins() -> Vec<AppEntry> {
    // No pins by default: the desktop shows only what is really in ~/Desktop.
    // Add Files/Store (or anything) back explicitly via [[desktop_pins]].
    Vec::new()
}


fn default_dock_badges() -> std::collections::BTreeMap<String, String> {
    [("claude", "orange"), ("kilo", "yellow")]
        .iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

fn default_theme() -> String { "midnight".into() }
fn default_assistant_mode() -> String { "panel".into() }
fn default_update_branch() -> String { "main".into() }

/// Branches the Updates-section switcher cycles through.
pub const UPDATE_BRANCHES: &[&str] = &["main", "dev"];

fn default_grid_rows() -> u8 { 2 }
fn default_grid_cols() -> u8 { 2 }

/// Resolve the config file path from `$XDG_CONFIG_HOME` (if set) else
/// `<home>/.config`, on every platform.
///
/// Note: we deliberately do **not** use `dirs::config_dir()` — on macOS that
/// returns `~/Library/Application Support`, but tetron-wm standardises on the
/// XDG-style `~/.config/tetron-wm/config.toml` across all platforms.
pub fn config_file_path(
    xdg_config_home: Option<std::ffi::OsString>,
    home: Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    let base = xdg_config_home
        .map(std::path::PathBuf::from)
        .or_else(|| home.map(|h| h.join(".config")))?;
    Some(base.join("tetron-wm").join("config.toml"))
}

/// The example config, compiled in so a packaged install needs no file for it
/// (WMDEB-002). Every default it sets is the built-in default, so writing it
/// out changes nothing.
pub const EXAMPLE: &str = include_str!("../config.example.toml");

/// What [`init_at`] did.
#[derive(Debug, PartialEq, Eq)]
pub enum InitOutcome {
    /// The example was written to this path.
    Written(std::path::PathBuf),
    /// A file was already there; it was left untouched.
    Exists(std::path::PathBuf),
}

/// Write [`EXAMPLE`] to `path` only when nothing is there (`create_new`, so a
/// file appearing at the same moment is never overwritten either).
pub fn init_at(path: &std::path::Path) -> std::io::Result<InitOutcome> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    match std::fs::OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(mut f) => {
            f.write_all(EXAMPLE.as_bytes())?;
            Ok(InitOutcome::Written(path.to_path_buf()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(InitOutcome::Exists(path.to_path_buf())),
        Err(e) => Err(e),
    }
}

/// The real config file path (`$XDG_CONFIG_HOME/tetron-wm/config.toml` or
/// `~/.config/tetron-wm/config.toml`).
pub fn file_path() -> std::io::Result<std::path::PathBuf> {
    config_file_path(std::env::var_os("XDG_CONFIG_HOME"), dirs::home_dir())
        .ok_or_else(|| std::io::Error::other("no home directory and no XDG_CONFIG_HOME"))
}

/// `tetron-wm config init` against the real config path.
pub fn init() -> std::io::Result<InitOutcome> {
    init_at(&file_path()?)
}

/// `<path>.bak`, where [`reset_at`] keeps the old file.
pub fn backup_path(path: &std::path::Path) -> std::path::PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".bak");
    std::path::PathBuf::from(s)
}

/// Settings > General > Reset to defaults: move the file at `path` to
/// `<path>.bak` (replacing an older backup), then write [`EXAMPLE`]. Returns
/// the backup path, or `None` when there was no file to keep.
pub fn reset_at(path: &std::path::Path) -> std::io::Result<Option<std::path::PathBuf>> {
    let bak = backup_path(path);
    let kept = match std::fs::rename(path, &bak) {
        Ok(()) => Some(bak),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    init_at(path)?;
    Ok(kept)
}

/// The tetron-wm config directory (`$XDG_CONFIG_HOME/tetron-wm` or
/// `~/.config/tetron-wm`) — the parent of `config.toml`, used to locate the
/// `themes/` subdir. `None` only if neither env var nor home dir resolves.
pub fn config_dir() -> Option<std::path::PathBuf> {
    config_file_path(std::env::var_os("XDG_CONFIG_HOME"), dirs::home_dir())
        .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
}

impl Config {
    /// Parse a `Config` from a TOML string.
    pub fn from_toml_str(s: &str) -> Result<Config, toml::de::Error> { toml::from_str(s) }

    /// Load from `$XDG_CONFIG_HOME/tetron-wm/config.toml` (or `~/.config/tetron-wm/config.toml`),
    /// falling back to defaults on any error. A file that fails to parse is logged
    /// and left untouched: `save` refuses to overwrite a file that does not parse.
    pub fn load() -> Config {
        let path = config_file_path(std::env::var_os("XDG_CONFIG_HOME"), dirs::home_dir());
        if let Some(p) = path {
            if let Ok(text) = std::fs::read_to_string(&p) {
                match Config::from_toml_str(&text) {
                    Ok(cfg) => {
                        if let Ok(t) = toml::Table::try_from(&cfg) {
                            crate::configfile::note_loaded(&p, t);
                        }
                        return cfg;
                    }
                    Err(e) => crate::dbg_log(&format!(
                        "config: {} failed to parse, using defaults and NOT overwriting it: {e}",
                        p.display()
                    )),
                }
            }
        }
        Config::default()
    }

    /// Persist the keys this process changed since it loaded (or last saved) the
    /// config into `$XDG_CONFIG_HOME/tetron-wm/config.toml` (or
    /// `~/.config/tetron-wm/config.toml`), creating the directory if needed.
    ///
    /// Comments, key order, unknown keys and hand edits to other keys are kept:
    /// see [`crate::configfile`]. Nothing is written when no key changed, and a
    /// file that does not parse is never overwritten.
    pub fn save(&self) -> std::io::Result<()> {
        let path = config_file_path(std::env::var_os("XDG_CONFIG_HOME"), dirs::home_dir())
            .ok_or_else(|| std::io::Error::other("no config directory"))?;
        let new = toml::Table::try_from(self).map_err(std::io::Error::other)?;
        crate::configfile::save(&path, new, || toml::Table::try_from(Config::default()).unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_writes_the_example_once_and_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("tetron-wm-init-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("tetron-wm/config.toml");
        assert_eq!(init_at(&path).unwrap(), InitOutcome::Written(path.clone()), "creates the parent dirs");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), EXAMPLE);
        std::fs::write(&path, "theme = \"gruvbox\"\n").unwrap();
        assert_eq!(init_at(&path).unwrap(), InitOutcome::Exists(path.clone()));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "theme = \"gruvbox\"\n", "untouched");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reset_keeps_the_old_file_as_bak_and_writes_the_example() {
        let dir = std::env::temp_dir().join(format!("tetron-wm-reset-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("tetron-wm/config.toml");
        assert_eq!(reset_at(&path).unwrap(), None, "nothing to keep, still writes");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), EXAMPLE);
        std::fs::write(&path, "theme = \"gruvbox\"\n").unwrap();
        let bak = reset_at(&path).unwrap().unwrap();
        assert_eq!(bak, dir.join("tetron-wm/config.toml.bak"));
        assert_eq!(std::fs::read_to_string(&bak).unwrap(), "theme = \"gruvbox\"\n");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), EXAMPLE);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn compiled_in_example_parses() {
        assert!(Config::from_toml_str(EXAMPLE).is_ok());
    }
    use std::path::PathBuf;

    #[test]
    fn config_path_prefers_xdg_config_home() {
        let p = config_file_path(Some("/x/cfg".into()), Some(PathBuf::from("/home/u")));
        assert_eq!(p.unwrap(), PathBuf::from("/x/cfg/tetron-wm/config.toml"));
    }

    #[test]
    fn config_path_falls_back_to_dotconfig_on_all_platforms() {
        let p = config_file_path(None, Some(PathBuf::from("/home/u")));
        assert_eq!(p.unwrap(), PathBuf::from("/home/u/.config/tetron-wm/config.toml"));
    }
}
