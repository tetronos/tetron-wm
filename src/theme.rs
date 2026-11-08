//! The desktop color theme — a small palette read by the compositor, window
//! manager, and chrome each frame. A process-global current theme lets Settings
//! restyle the whole desktop live without threading a `Theme` through every call.

use crate::cell::Rgba;
use std::sync::{OnceLock, RwLock};

/// A complete desktop palette. `Copy` so render code can cheaply snapshot it.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub desktop_bg: Rgba,
    pub window_bg: Rgba,
    pub title_focus: Rgba,
    pub title_blur: Rgba,
    pub title_fg: Rgba,
    pub border: Rgba,
    pub shadow: Rgba,
    pub ctrl_fg: Rgba,
    pub close_fg: Rgba,
    pub menubar_bg: Rgba,
    pub dock_bg: Rgba,
    pub text: Rgba,
    pub dim: Rgba,
    pub accent: Rgba,
    pub active_bg: Rgba,
    /// "Good" status text: installed, verified, switched on, update available.
    pub success: Rgba,
    /// "Bad" status text: errors, low-contrast warnings, kill.
    pub error: Rgba,
    /// Text drawn on a fill of `accent` (the "+" new-shell button).
    pub accent_fg: Rgba,
    /// Desktop icon labels drawn over a wallpaper picture (on the scrim).
    pub icon_fg: Rgba,
    /// The 16 ANSI colours hosted apps see (black, red, … white, then the
    /// bright forms), used when an app asks for a palette colour.
    pub ansi: [Rgba; 16],
    /// Explicit 16-colour choices per role (see [`Vt16`]); usually empty.
    pub vt16: Vt16,
}

/// The ANSI palette a theme gets unless it sets its own `[ansi]` table.
pub const DEFAULT_ANSI: [Rgba; 16] = [
    rgb(0, 0, 0), rgb(205, 49, 49), rgb(13, 188, 121), rgb(229, 229, 16),
    rgb(36, 114, 200), rgb(188, 63, 188), rgb(17, 168, 205), rgb(229, 229, 229),
    rgb(102, 102, 102), rgb(241, 76, 76), rgb(35, 209, 139), rgb(245, 245, 67),
    rgb(59, 142, 234), rgb(214, 112, 214), rgb(41, 184, 219), rgb(255, 255, 255),
];

/// The `[ansi]` key for each palette index, in order.
pub const ANSI_NAMES: [&str; 16] = [
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
    "bright_black", "bright_red", "bright_green", "bright_yellow",
    "bright_blue", "bright_magenta", "bright_cyan", "bright_white",
];

const fn rgb(r: u8, g: u8, b: u8) -> Rgba {
    Rgba { r, g, b, a: 255 }
}

impl Theme {
    /// The default dark theme (tetron-wm's original palette).
    pub const fn midnight() -> Self {
        Theme {
            desktop_bg: rgb(44, 46, 50),
            window_bg: rgb(17, 20, 29),
            title_focus: rgb(29, 36, 51),
            title_blur: rgb(20, 24, 34),
            title_fg: rgb(143, 183, 255),
            border: rgb(58, 68, 88),
            shadow: Rgba { r: 0, g: 0, b: 0, a: 110 },
            ctrl_fg: rgb(150, 165, 190),
            close_fg: rgb(255, 107, 107),
            menubar_bg: rgb(22, 27, 39),
            dock_bg: rgb(22, 27, 39),
            text: rgb(200, 208, 220),
            dim: rgb(120, 130, 150),
            accent: rgb(108, 182, 255),
            active_bg: rgb(45, 58, 85),
            success: rgb(126, 231, 135),
            error: rgb(255, 107, 107),
            accent_fg: rgb(255, 255, 255),
            icon_fg: rgb(255, 255, 255),
            ansi: DEFAULT_ANSI,
            vt16: Vt16::NONE,
        }
    }

    /// Gruvbox — warm, retro.
    pub const fn gruvbox() -> Self {
        Theme {
            desktop_bg: rgb(60, 56, 54),
            window_bg: rgb(40, 40, 40),
            title_focus: rgb(60, 56, 54),
            title_blur: rgb(50, 48, 47),
            title_fg: rgb(250, 189, 47),
            border: rgb(102, 92, 84),
            shadow: Rgba { r: 0, g: 0, b: 0, a: 120 },
            ctrl_fg: rgb(213, 196, 161),
            close_fg: rgb(251, 73, 52),
            menubar_bg: rgb(50, 48, 47),
            dock_bg: rgb(50, 48, 47),
            text: rgb(235, 219, 178),
            dim: rgb(146, 131, 116),
            accent: rgb(250, 189, 47),
            active_bg: rgb(80, 73, 69),
            success: rgb(126, 231, 135),
            error: rgb(251, 73, 52),
            accent_fg: rgb(255, 255, 255),
            icon_fg: rgb(255, 255, 255),
            ansi: DEFAULT_ANSI,
            // On a 16-colour console: a warm brown desktop.
            vt16: Vt16 { desktop_bg: Some(3), ..Vt16::NONE },
        }
    }

    /// Light — paper ground, black-on-white terminals, tetron-green highlights.
    /// Pastoral — the only light-*appearance* preset (named for its palette, not
    /// its brightness, so the name does not collide with the `appearance` field).
    /// `accent` is a darkened green (`#2E7D32`) so button labels stay legible on
    /// the light panels, while the logo green `#4CAF50` carries the highlight as
    /// the window `border`.
    pub const fn pastoral() -> Self {
        Theme {
            desktop_bg: rgb(236, 230, 216),
            window_bg: rgb(255, 255, 255),
            // Active titlebar reads clearly green; inactive is a distinct taupe
            // bar, darker than the paper wallpaper so it does not blend in.
            title_focus: rgb(196, 227, 180),
            title_blur: rgb(212, 207, 193),
            title_fg: rgb(43, 43, 43),
            border: rgb(76, 175, 80),
            shadow: Rgba { r: 50, g: 45, b: 30, a: 51 },
            ctrl_fg: rgb(106, 106, 106),
            close_fg: rgb(209, 82, 76),
            menubar_bg: rgb(231, 226, 214),
            dock_bg: rgb(231, 226, 214),
            text: rgb(26, 26, 26),
            dim: rgb(110, 106, 94),
            accent: rgb(46, 125, 50),
            active_bg: rgb(215, 233, 205),
            success: rgb(26, 127, 55),
            error: rgb(209, 82, 76),
            accent_fg: rgb(255, 255, 255),
            icon_fg: rgb(255, 255, 255),
            ansi: DEFAULT_ANSI,
            // On a 16-colour console: a grey desktop (a whole green screen is too
            // intense; a green wallpaper is the way to get one), green focused
            // title and selection.
            vt16: Vt16 { desktop_bg: Some(7), title_focus: Some(2), active_bg: Some(2), ..Vt16::NONE },
        }
    }

    /// Windows 3.1 / early-95: teal desktop, navy title bars, silver chrome,
    /// white windows. Its own picture is embedded (`themes/windows.jpg`).
    pub const fn windows() -> Self {
        Theme {
            desktop_bg: rgb(0, 128, 128),
            window_bg: rgb(255, 255, 255),
            title_focus: rgb(0, 0, 128),
            title_blur: rgb(128, 128, 128),
            title_fg: rgb(255, 255, 255),
            border: rgb(128, 128, 128),
            shadow: Rgba { r: 0, g: 0, b: 0, a: 38 },
            ctrl_fg: rgb(255, 255, 255),
            close_fg: rgb(204, 0, 0),
            menubar_bg: rgb(192, 192, 192),
            dock_bg: rgb(192, 192, 192),
            text: rgb(0, 0, 0),
            dim: rgb(128, 128, 128),
            accent: rgb(0, 0, 128),
            active_bg: rgb(192, 192, 192),
            success: rgb(26, 127, 55),
            error: rgb(204, 0, 0),
            accent_fg: rgb(255, 255, 255),
            icon_fg: rgb(255, 255, 255),
            ansi: DEFAULT_ANSI,
            vt16: Vt16::NONE,
        }
    }

    /// Resolve a theme by name. A user theme file of the same name (loaded into the
    /// registry) wins over a built-in; otherwise a built-in preset; else `midnight`.
    pub fn named(name: &str) -> Self {
        let lname = name.to_lowercase();
        if let Some((_, t)) = registry().iter().find(|(n, _)| n.to_lowercase() == lname) {
            return *t;
        }
        Self::builtin(&lname)
    }

    /// Resolve a *built-in* preset by (lowercased) name, ignoring the registry.
    /// This is the base a partial theme file inherits from.
    fn builtin(lname: &str) -> Self {
        match lname {
            "gruvbox" => Self::gruvbox(),
            "pastoral" => Self::pastoral(),
            "windows" => Self::windows(),
            // Deprecated alias: the preset was renamed light → pastoral. Kept so
            // existing `theme = "light"` configs still resolve (not in PRESETS).
            "light" => Self::pastoral(),
            _ => Self::midnight(),
        }
    }
}

/// The names of the built-in presets (for the Settings cycler). File themes are
/// appended at runtime by [`preset_names`].
///
/// Four are built in (each with its own picture). The rest (nord, dracula, dawn,
/// newmoon, contrast) are theme files the suite installer downloads into `themes/`.
pub const PRESETS: &[&str] = &["midnight", "gruvbox", "pastoral", "windows"];

/// All theme names offered in the UI: the built-in presets, then any file themes
/// whose name does not shadow a built-in (a same-named file overrides the built-in
/// palette but keeps its slot, so it is not listed twice).
pub fn preset_names() -> Vec<String> {
    let mut names: Vec<String> = PRESETS.iter().map(|s| s.to_string()).collect();
    for (n, _) in registry().iter() {
        if !names.iter().any(|x| x.eq_ignore_ascii_case(n)) {
            names.push(n.clone());
        }
    }
    names
}

fn slot() -> &'static RwLock<Theme> {
    static THEME: OnceLock<RwLock<Theme>> = OnceLock::new();
    THEME.get_or_init(|| RwLock::new(Theme::midnight()))
}

/// Snapshot the current desktop theme, adapted to the attached client's colour
/// depth (see [`adapt`]). Every widget reads colours through here, so this is
/// the one place where 16-colour and black-and-white rendering is decided.
pub fn current() -> Theme {
    adapt(*slot().read().unwrap(), crate::display::depth())
}

// ── Low colour depths ────────────────────────────────────────────────────────
//
// A theme is designed in truecolor. Quantizing it to 16 colours collapses the
// subtle dark backgrounds (desktop, window, selection) onto black, so the
// selection disappears; in black and white every colour is gone. Instead of
// quantizing, map each palette *role* to a colour chosen for that depth, so the
// distinctions the UI relies on (desktop vs window, focused vs blurred,
// selected vs not) survive. The client still guards every fg/bg pair
// (`terminal::readable_fg16`), so a widget that pairs roles unusually stays
// legible.

/// The theme to render with at colour depth `depth`. Truecolor and 256 colours
/// keep the theme as designed (the 256-colour quantizer has a fine grey ramp).
pub fn adapt(t: Theme, depth: crate::display::ColorDepth) -> Theme {
    use crate::display::ColorDepth;
    match depth {
        ColorDepth::TrueColor | ColorDepth::Ansi256 => t,
        ColorDepth::Ansi16 => vt16(t),
        ColorDepth::Mono => mono(t),
    }
}

/// Whether a theme is light (dark text on a light window background).
fn is_light(t: &Theme) -> bool {
    luminance(t.window_bg) > 0.4
}

/// Per-theme 16-colour choices: a palette index (0-15, ANSI order, see
/// `display::VGA16`) per role, overriding what [`vt16`] derives. Set on
/// built-in presets and by a theme file's `[vt16]` table. Background roles can
/// only use 0-7 (a bare VT has 8 backgrounds); 8-15 there fold to 0-7.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(default)]
pub struct Vt16 {
    pub desktop_bg: Option<u8>,
    pub window_bg: Option<u8>,
    pub title_focus: Option<u8>,
    pub title_blur: Option<u8>,
    pub title_fg: Option<u8>,
    pub border: Option<u8>,
    pub ctrl_fg: Option<u8>,
    pub close_fg: Option<u8>,
    pub menubar_bg: Option<u8>,
    pub dock_bg: Option<u8>,
    pub text: Option<u8>,
    pub dim: Option<u8>,
    pub accent: Option<u8>,
    pub active_bg: Option<u8>,
    pub success: Option<u8>,
    pub error: Option<u8>,
    pub accent_fg: Option<u8>,
    pub icon_fg: Option<u8>,
}

impl Vt16 {
    /// No overrides: everything derived.
    pub const NONE: Vt16 = Vt16 {
        desktop_bg: None, window_bg: None, title_focus: None, title_blur: None,
        title_fg: None, border: None, ctrl_fg: None, close_fg: None,
        menubar_bg: None, dock_bg: None, text: None, dim: None, accent: None,
        active_bg: None, success: None, error: None, accent_fg: None, icon_fg: None,
    };
}

/// Spread of a colour's channels: 0 for greys, large for clearly coloured.
fn chroma(c: Rgba) -> u8 {
    c.r.max(c.g).max(c.b) - c.r.min(c.g).min(c.b)
}

/// Below this chroma a theme colour counts as a neutral (grey, near black, near
/// white) and takes the generic role colour instead of its nearest hue.
const HUE_CHROMA: u8 = 40;

/// 16-colour palette by role, using only exact VGA colours (so the client's
/// quantizer maps them 1:1) and only the 8 backgrounds a bare VT can show.
///
/// Three layers, later wins:
/// 1. a generic table for dark or light themes (window and text colours always
///    come from here: dark themes black windows, light themes grey);
/// 2. the theme's own hue for clearly coloured roles (desktop, title bars,
///    menubar, selection, accent, close), e.g. a teal desktop stays cyan;
/// 3. the theme's explicit [`Vt16`] choices.
///
/// Then the distinctions the UI relies on are enforced: the selection differs
/// from the window and the desktop, the focused title from the blurred one.
fn vt16(t: Theme) -> Theme {
    let v = |i: u8| crate::display::VGA16[i as usize];
    let near = |c: Rgba, set: &[u8]| -> u8 {
        let d = |a: u8, b: u8| (a as i32 - b as i32).pow(2);
        *set.iter().min_by_key(|&&i| { let p = v(i); d(p.r, c.r) + d(p.g, c.g) + d(p.b, c.b) }).unwrap()
    };
    // The coloured entries: backgrounds 1-6, text 1-6 and 9-14.
    const BG_HUES: [u8; 6] = [1, 2, 3, 4, 5, 6];
    const FG_HUES: [u8; 12] = [1, 2, 3, 4, 5, 6, 9, 10, 11, 12, 13, 14];
    let hue_bg = |c: Rgba| (chroma(c) >= HUE_CHROMA).then(|| near(c, &BG_HUES));
    let light = is_light(&t);
    // Text on a light theme sits on grey/white: only the dark hues read there.
    let fg_hues: &[u8] = if light { &BG_HUES } else { &FG_HUES };
    let hue_fg = |c: Rgba| (chroma(c) >= HUE_CHROMA).then(|| near(c, fg_hues));

    // 1. Generic table.
    let (mut desktop, window, mut focus, blur, title_fg, border, ctrl, mut close, mut menubar, mut dock, text, dim, mut accent, mut active) =
        if light {
            (6, 7, 4, 7, 15, 0, 0, 1, 7, 7, 0, 8, 4, 4)
        } else {
            // Bright accent from the bright hues only, so it reads on black.
            let acc = if chroma(t.accent) >= HUE_CHROMA { near(t.accent, &[9, 10, 11, 12, 13, 14]) } else { 14 };
            (0, 0, 7, 0, 0, 7, 7, 9, 0, 0, 7, 8, acc, 6)
        };
    // 2. The theme's own hues.
    desktop = hue_bg(t.desktop_bg).unwrap_or(desktop);
    focus = hue_bg(t.title_focus).unwrap_or(focus);
    menubar = hue_bg(t.menubar_bg).unwrap_or(menubar);
    dock = hue_bg(t.dock_bg).unwrap_or(dock);
    active = hue_bg(t.active_bg).unwrap_or(active);
    if light {
        accent = hue_fg(t.accent).unwrap_or(accent);
    }
    // `close` stays red (danger) unless a theme overrides it explicitly.
    // 3. Explicit choices (backgrounds folded to the 8 a VT can show).
    let o = t.vt16;
    let bg = |cur: u8, x: Option<u8>| x.map(|i| (i & 15) % 8).unwrap_or(cur);
    let fg = |cur: u8, x: Option<u8>| x.map(|i| i & 15).unwrap_or(cur);
    desktop = bg(desktop, o.desktop_bg);
    let window = bg(window, o.window_bg);
    focus = bg(focus, o.title_focus);
    let blur = bg(blur, o.title_blur);
    menubar = bg(menubar, o.menubar_bg);
    dock = bg(dock, o.dock_bg);
    active = bg(active, o.active_bg);
    let (title_fg, border, ctrl) = (fg(title_fg, o.title_fg), fg(border, o.border), fg(ctrl, o.ctrl_fg));
    close = fg(close, o.close_fg);
    let (text, dim) = (fg(text, o.text), fg(dim, o.dim));
    accent = fg(accent, o.accent);
    // Status colours and the two "ink" colours: bright on dark, dark on light.
    let success = fg(if light { 2 } else { 10 }, o.success);
    let error = fg(if light { 1 } else { 9 }, o.error);
    let icon_fg = fg(15, o.icon_fg);
    // White on the accent fill unless that would vanish (as for a white accent).
    let accent_fg = fg(if contrast(v(15), v(accent)) >= ACCENT_FG_MIN { 15 } else { 0 }, o.accent_fg);
    // Distinctions the UI relies on, unless the theme chose the role itself.
    if o.active_bg.is_none() && (active == window || active == desktop) {
        active = [6, 4, 2, 5, 3, 1].into_iter().find(|&i| i != window && i != desktop).unwrap();
    }
    if o.title_focus.is_none() && focus == blur {
        focus = [7, 6, 4, 2].into_iter().find(|&i| i != blur).unwrap();
    }
    Theme {
        desktop_bg: v(desktop),
        window_bg: v(window),
        title_focus: v(focus),
        title_blur: v(blur),
        title_fg: v(title_fg),
        border: v(border),
        shadow: t.shadow,
        ctrl_fg: v(ctrl),
        close_fg: v(close),
        menubar_bg: v(menubar),
        dock_bg: v(dock),
        text: v(text),
        dim: v(dim),
        accent: v(accent),
        active_bg: v(active),
        success: v(success),
        error: v(error),
        accent_fg: v(accent_fg),
        icon_fg: v(icon_fg),
        ansi: t.ansi,
        vt16: t.vt16,
    }
}

/// Black-and-white palette. The client emits no colour in mono mode; it turns
/// a light background into reverse video, a dark fg into dim and a near-white
/// fg into bold (`terminal::push_colors`). So every role is a grey chosen to
/// land on the right side of those thresholds: highlights (selection, focused
/// title) are white backgrounds, secondary text is mid grey, the accent is
/// pure white. Light themes map the same way: the terminal's own default
/// colours decide what "black" and "white" look like.
fn mono(t: Theme) -> Theme {
    let g = |v: u8| rgb(v, v, v);
    Theme {
        desktop_bg: g(0),
        window_bg: g(0),
        title_focus: g(230),
        title_blur: g(0),
        title_fg: g(192),
        border: g(192),
        shadow: t.shadow,
        ctrl_fg: g(192),
        close_fg: g(255),
        menubar_bg: g(0),
        dock_bg: g(0),
        text: g(192),
        dim: g(110),
        accent: g(255),
        active_bg: g(230),
        success: g(192),
        error: g(255),
        accent_fg: g(0),
        icon_fg: g(192),
        ansi: t.ansi,
        vt16: t.vt16,
    }
}

/// Replace the current desktop theme (applied on the next rendered frame).
pub fn set(name: &str) {
    if !known(name) {
        crate::log_warn(&format!("theme: '{name}' not found, using midnight (extra themes: install-tetron-wm-suite.sh --themes)"));
    }
    *slot().write().unwrap() = Theme::named(name);
}

/// Set the desktop theme by name, then apply the optional `terminal_bg`/
/// `terminal_fg` color overrides (hex or named; unparsable values are ignored).
/// Setting the theme first resets any earlier override, so passing `None` goes
/// back to following the theme. Used by the apphost, at start-up and on `SetColors`.
pub fn set_with_terminal(name: &str, bg: Option<&str>, fg: Option<&str>) {
    set(name);
    let bg = bg.and_then(crate::badge::parse_color);
    let fg = fg.and_then(crate::badge::parse_color);
    if bg.is_some() || fg.is_some() {
        set_terminal_colors(bg, fg);
    }
}

/// Override just the terminal-relevant colors (`window_bg`/`text`) on the current
/// theme. The apphost uses this so the `terminal_bg`/`terminal_fg` config keys can
/// decouple terminal colors from the desktop theme: in the apphost process the
/// theme global is read only by `ptyhost` for hosted-app cell colors, so this
/// does not affect any chrome. `None` leaves that color following the theme.
pub fn set_terminal_colors(bg: Option<Rgba>, fg: Option<Rgba>) {
    let mut t = slot().write().unwrap();
    if let Some(bg) = bg {
        t.window_bg = bg;
    }
    if let Some(fg) = fg {
        t.text = fg;
    }
}

// ── File-based themes ────────────────────────────────────────────────────────
//
// A theme file is `<config>/themes/<name>.toml` (dir overridable via the
// `theme_dir` config key or `$TETRON_WM_THEMES_DIR`). It is a flat table of the
// palette fields → `#rrggbb`, `#rrggbbaa`, or a named color; any field left out
// inherits from `base` (a built-in preset name, default `midnight`). The file
// carries only colors — layout and the contrast rules stay in code so a theme
// cannot break them; [`validate`] warns (never aborts) on unreadable contrast.

use std::path::{Path, PathBuf};

/// On-disk form of a theme: string-typed so colors can be hex or named, every
/// field optional so a partial file still loads. Unknown keys are ignored, so a
/// file written for a newer tetron-wm still loads (version-skew tolerant).
#[derive(Debug, Default, serde::Deserialize)]
#[serde(default)]
struct ThemeFile {
    /// Theme name; defaults to the file stem when absent.
    name: Option<String>,
    /// Built-in preset to inherit unset fields from (default `midnight`).
    base: Option<String>,
    // Metadata the renderer ignores — kept for tooling / forward-compat.
    schema: Option<u32>,
    appearance: Option<String>,
    // Colors (unset = inherit from `base`).
    desktop_bg: Option<String>,
    window_bg: Option<String>,
    title_focus: Option<String>,
    title_blur: Option<String>,
    title_fg: Option<String>,
    border: Option<String>,
    shadow: Option<String>,
    ctrl_fg: Option<String>,
    close_fg: Option<String>,
    menubar_bg: Option<String>,
    dock_bg: Option<String>,
    text: Option<String>,
    dim: Option<String>,
    accent: Option<String>,
    active_bg: Option<String>,
    // Optional colours: unset = derived from the colours above (see `into_theme`).
    success: Option<String>,
    error: Option<String>,
    accent_fg: Option<String>,
    icon_fg: Option<String>,
    /// Optional `[ansi]` table: the 16 colours hosted apps see, by name.
    ansi: Option<std::collections::BTreeMap<String, String>>,
    /// Optional 16-colour choices: `[vt16]` with `role = 0..15`.
    vt16: Option<Vt16>,
    /// The theme's picture: a file next to the theme file, an absolute or `~`
    /// path, `black`, `#rrggbb`, or `none`. Unset pairs with `<stem>.jpg|jpeg|
    /// png|webp` in the same folder. Only used while the `wallpaper` config key
    /// is `default`.
    wallpaper: Option<String>,
}

/// Apply one optional color string over a default, logging (and keeping the
/// default) when a value is present but unparseable.
fn field(name: &str, dst: Rgba, src: &Option<String>) -> Rgba {
    match src {
        Some(s) => match crate::badge::parse_color(s) {
            Some(c) => c,
            None => {
                crate::log_warn(&format!("theme: field '{name}' invalid color {s:?}; keeping default"));
                dst
            }
        },
        None => dst,
    }
}

/// Below this contrast against `accent`, white text on an accent fill is
/// replaced by black (a white or very pale accent would otherwise hide it).
const ACCENT_FG_MIN: f64 = 1.5;

/// Defaults for the optional colours `(success, error, accent_fg, icon_fg)`,
/// from the rest of a palette: a dark green on light themes, the theme's own
/// `close_fg` for errors, white on the accent fill unless it would vanish.
fn derived(t: &Theme) -> (Rgba, Rgba, Rgba, Rgba) {
    let white = rgb(255, 255, 255);
    let success = if is_light(t) { rgb(26, 127, 55) } else { rgb(126, 231, 135) };
    let accent_fg = if contrast(white, t.accent) >= ACCENT_FG_MIN { white } else { rgb(0, 0, 0) };
    (success, t.close_fg, accent_fg, white)
}

impl ThemeFile {
    fn into_theme(self) -> (Option<String>, Theme) {
        let base = Theme::builtin(&self.base.as_deref().unwrap_or("midnight").to_lowercase());
        let mut t = Theme {
            desktop_bg: field("desktop_bg", base.desktop_bg, &self.desktop_bg),
            window_bg: field("window_bg", base.window_bg, &self.window_bg),
            title_focus: field("title_focus", base.title_focus, &self.title_focus),
            title_blur: field("title_blur", base.title_blur, &self.title_blur),
            title_fg: field("title_fg", base.title_fg, &self.title_fg),
            border: field("border", base.border, &self.border),
            shadow: field("shadow", base.shadow, &self.shadow),
            ctrl_fg: field("ctrl_fg", base.ctrl_fg, &self.ctrl_fg),
            close_fg: field("close_fg", base.close_fg, &self.close_fg),
            menubar_bg: field("menubar_bg", base.menubar_bg, &self.menubar_bg),
            dock_bg: field("dock_bg", base.dock_bg, &self.dock_bg),
            text: field("text", base.text, &self.text),
            dim: field("dim", base.dim, &self.dim),
            accent: field("accent", base.accent, &self.accent),
            active_bg: field("active_bg", base.active_bg, &self.active_bg),
            success: base.success,
            error: base.error,
            accent_fg: base.accent_fg,
            icon_fg: base.icon_fg,
            ansi: base.ansi,
            vt16: self.vt16.unwrap_or(base.vt16),
        };
        // The optional colours follow this file's own palette, not the base's:
        // a light file on a dark base must not keep the dark green.
        let d = derived(&t);
        t.success = field("success", d.0, &self.success);
        t.error = field("error", d.1, &self.error);
        t.accent_fg = field("accent_fg", d.2, &self.accent_fg);
        t.icon_fg = field("icon_fg", d.3, &self.icon_fg);
        if let Some(tbl) = &self.ansi {
            for (i, name) in ANSI_NAMES.iter().enumerate() {
                t.ansi[i] = field(&format!("ansi.{name}"), t.ansi[i], &tbl.get(*name).cloned());
            }
            for k in tbl.keys().filter(|k| !ANSI_NAMES.contains(&k.as_str())) {
                crate::log_warn(&format!("theme: [ansi] key '{k}' is not a colour name; ignored"));
            }
        }
        (self.name, t)
    }
}

/// Parse a theme TOML string into its name (if the file set one) and `Theme`.
fn parse_theme_toml(text: &str) -> Result<(Option<String>, Theme), String> {
    let tf: ThemeFile = toml::from_str(text).map_err(|e| e.to_string())?;
    Ok(tf.into_theme())
}

/// `#rrggbb`, or `#rrggbbaa` when the color carries alpha.
pub fn to_hex(c: Rgba) -> String {
    if c.a == 255 {
        format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
    } else {
        format!("#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a)
    }
}

/// The 16 ANSI colours of `theme` as hex strings, in palette order, for the
/// apphost (see [`resolve_terminal_colors`]).
pub fn resolve_ansi(theme: &str) -> Vec<String> {
    Theme::named(theme).ansi.iter().map(|c| to_hex(*c)).collect()
}

/// Replace the ANSI palette of the current theme from 16 hex strings; anything
/// else (wrong count, bad colour) is ignored. Used by the apphost.
pub fn set_ansi(ansi: &[String]) {
    apply_ansi(&mut slot().write().unwrap(), ansi);
}

fn apply_ansi(t: &mut Theme, ansi: &[String]) {
    if ansi.len() != 16 {
        return;
    }
    for (i, s) in ansi.iter().enumerate() {
        if let Some(c) = crate::badge::parse_color(s) {
            t.ansi[i] = c;
        }
    }
}

/// The default terminal background and foreground for `theme` with the optional
/// `terminal_bg`/`terminal_fg` overrides applied, as hex strings. The apphost
/// only ever uses these two colors, so the daemon resolves them (file themes
/// included) and sends the result instead of a theme name the apphost might not
/// be able to look up.
pub fn resolve_terminal_colors(theme: &str, bg: Option<&str>, fg: Option<&str>) -> (String, String) {
    let t = Theme::named(theme);
    let pick = |over: Option<&str>, base: Rgba| to_hex(over.and_then(crate::badge::parse_color).unwrap_or(base));
    (pick(bg, t.window_bg), pick(fg, t.text))
}

/// Serialize a palette to theme-file TOML (`--dump` starter). `#rrggbbaa` only
/// when the color actually carries alpha, else the shorter `#rrggbb`.
pub fn to_file_toml(t: &Theme, name: &str) -> String {
    fn hex(c: Rgba) -> String {
        if c.a == 255 {
            format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a)
        }
    }
    format!(
        "name = \"{name}\"\n\
         # base = \"midnight\"   # unset fields inherit from this built-in\n\
         # appearance = \"dark\"\n\n\
         desktop_bg  = \"{}\"\n\
         window_bg   = \"{}\"\n\
         title_focus = \"{}\"\n\
         title_blur  = \"{}\"\n\
         title_fg    = \"{}\"\n\
         border      = \"{}\"\n\
         shadow      = \"{}\"\n\
         ctrl_fg     = \"{}\"\n\
         close_fg    = \"{}\"\n\
         menubar_bg  = \"{}\"\n\
         dock_bg     = \"{}\"\n\
         text        = \"{}\"\n\
         dim         = \"{}\"\n\
         accent      = \"{}\"\n\
         active_bg   = \"{}\"\n\n\
         # Optional: unset, each is derived from the colours above.\n\
         success     = \"{}\"   # installed, verified, on\n\
         error       = \"{}\"   # errors, kill\n\
         accent_fg   = \"{}\"   # text on an accent fill\n\
         icon_fg     = \"{}\"   # desktop icon labels over a wallpaper\n\n\
         # 16-colour console (bare Linux VT): derived automatically from the\n\
         # colours above. Uncomment to pin a role to a palette index 0-15\n\
         # (0 black, 1 red, 2 green, 3 brown, 4 blue, 5 magenta, 6 cyan, 7 grey,\n\
         # 8-15 their bright forms; backgrounds can only use 0-7).\n\
         # [vt16]\n\
         # desktop_bg = 6\n\
         # title_focus = 4\n\
         # (also success, error, accent_fg, icon_fg)\n\n\
         # The 16 colours hosted apps see; uncomment to change.\n\
         # [ansi]\n{}",
        hex(t.desktop_bg), hex(t.window_bg), hex(t.title_focus), hex(t.title_blur),
        hex(t.title_fg), hex(t.border), hex(t.shadow), hex(t.ctrl_fg), hex(t.close_fg),
        hex(t.menubar_bg), hex(t.dock_bg), hex(t.text), hex(t.dim), hex(t.accent),
        hex(t.active_bg), hex(t.success), hex(t.error), hex(t.accent_fg), hex(t.icon_fg),
        ANSI_NAMES.iter().zip(t.ansi).map(|(n, c)| format!("# {n} = \"{}\"\n", hex(c))).collect::<String>(),
    )
}

/// WCAG relative luminance (sRGB linearized).
pub fn luminance(c: Rgba) -> f64 {
    let lin = |v: u8| {
        let s = v as f64 / 255.0;
        if s <= 0.03928 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// WCAG contrast ratio between two colors (1.0 = identical … 21.0 = black/white).
pub fn contrast(a: Rgba, b: Rgba) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la >= lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Check a theme against the readability invariants the renderer relies on (see
/// the theming doc §5). Returns human-readable warnings; never fails — the theme
/// still loads. Keeps the red-on-green class of bug out of user themes.
pub fn validate(t: &Theme) -> Vec<String> {
    let mut warns = Vec::new();
    let mut check = |a: Rgba, b: Rgba, min: f64, msg: &str| {
        let c = contrast(a, b);
        if c < min {
            warns.push(format!("low contrast ({c:.1}:1 < {min:.1}:1): {msg}"));
        }
    };
    // Buttons draw window_bg text on a saturated fill (close_fg / accent).
    check(t.window_bg, t.close_fg, 2.0, "destructive-button text (window_bg) on fill (close_fg)");
    check(t.window_bg, t.accent, 2.0, "affirmative-button text (window_bg) on fill (accent)");
    // Body/terminal text, and desktop icon labels, both use `text`.
    check(t.text, t.window_bg, 3.0, "body text on window_bg");
    check(t.text, t.desktop_bg, 2.5, "icon-label text on desktop_bg (wallpaper)");
    // An inactive titlebar that matches the wallpaper vanishes.
    check(t.title_blur, t.desktop_bg, 1.12, "inactive titlebar vs wallpaper (too similar)");
    warns
}

/// The user's own theme directory, where `theme --dump` writes:
/// `$TETRON_WM_THEMES_DIR`, else the `theme_dir` config key (relative to the
/// config dir), else `<config>/themes`. The first entry of [`themes_dirs`].
pub(crate) fn themes_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("TETRON_WM_THEMES_DIR") {
        return Some(PathBuf::from(d));
    }
    let cfg_dir = crate::config::config_dir();
    match crate::config::Config::load().theme_dir {
        Some(d) => {
            let p = PathBuf::from(&d);
            if p.is_absolute() { Some(p) } else { cfg_dir.map(|c| c.join(p)) }
        }
        None => cfg_dir.map(|c| c.join("themes")),
    }
}

/// Where in-app theme downloads go: `$XDG_DATA_HOME/tetron-wm/themes`
/// (default `~/.local/share/tetron-wm/themes`).
pub fn download_dir() -> Option<PathBuf> {
    data_home_dir().map(|d| d.join("tetron-wm/themes"))
}

/// `$XDG_DATA_HOME` when set and absolute, else `~/.local/share`.
pub fn data_home_dir() -> Option<PathBuf> {
    data_home(std::env::var_os("XDG_DATA_HOME"), dirs::home_dir())
}

fn data_home(xdg: Option<std::ffi::OsString>, home: Option<PathBuf>) -> Option<PathBuf> {
    xdg.filter(|v| !v.is_empty()).map(PathBuf::from).filter(|p| p.is_absolute()).or_else(|| home.map(|h| h.join(".local/share")))
}

/// Every directory theme files are read from, first wins (WMDEB-002):
/// `$TETRON_WM_THEMES_DIR` alone when set; else the user's dir
/// ([`themes_dir`]), the download dir, then each `$XDG_DATA_DIRS` entry
/// (default `/usr/local/share:/usr/share`) + `tetron-wm/themes`.
pub(crate) fn themes_dirs() -> Vec<PathBuf> {
    search_dirs(
        std::env::var_os("TETRON_WM_THEMES_DIR"),
        themes_dir(),
        download_dir(),
        std::env::var_os("XDG_DATA_DIRS"),
    )
}

/// [`themes_dirs`] with its inputs passed in, so it is unit-testable.
fn search_dirs(
    over: Option<std::ffi::OsString>,
    user: Option<PathBuf>,
    download: Option<PathBuf>,
    data_dirs: Option<std::ffi::OsString>,
) -> Vec<PathBuf> {
    if let Some(d) = over.filter(|d| !d.is_empty()) {
        return vec![PathBuf::from(d)];
    }
    let system = data_dirs.filter(|v| !v.is_empty()).unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    let mut out: Vec<PathBuf> = user.into_iter().chain(download).collect();
    for d in std::env::split_paths(&system).filter(|d| d.is_absolute()) {
        out.push(d.join("tetron-wm/themes"));
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|d| seen.insert(d.clone()));
    out
}

/// `(name, palette)` pairs, and `(lowercase name, wallpaper value)` pairs.
type ThemeList = Vec<(String, Theme)>;
type PictureList = Vec<(String, String)>;

/// The themes and theme pictures of `dirs`, first directory wins per theme
/// name. A theme's picture always comes from the directory its colours came
/// from, so a later directory can not pair its own picture with it.
fn load_all(dirs: &[PathBuf]) -> (ThemeList, PictureList) {
    let mut themes: ThemeList = Vec::new();
    let mut pictures: PictureList = Vec::new();
    for dir in dirs {
        let fresh: Vec<(String, Theme)> = load_dir(dir)
            .into_iter()
            .filter(|(n, _)| !themes.iter().any(|(t, _)| t.eq_ignore_ascii_case(n)))
            .collect();
        if fresh.is_empty() {
            continue;
        }
        let names: Vec<String> = fresh.iter().map(|(n, _)| n.to_lowercase()).collect();
        pictures.extend(load_wallpapers(dir).into_iter().filter(|(n, _)| names.contains(n)));
        themes.extend(fresh);
    }
    (themes, pictures)
}

/// Load every `*.toml` in `dir` into `(name, Theme)` pairs. Pure (no globals), so
/// it is unit-testable; malformed files are skipped and logged, not fatal.
fn load_dir(dir: &Path) -> Vec<(String, Theme)> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<(String, Theme)> = Vec::new();
    for ent in rd.flatten() {
        let path = ent.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
            continue;
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => match parse_theme_toml(&text) {
                Ok((name, theme)) => {
                    let name = name.unwrap_or(stem);
                    for w in validate(&theme) {
                        crate::log_warn(&format!("theme: '{name}': {w}"));
                    }
                    out.push((name, theme));
                }
                Err(e) => crate::log_warn(&format!("theme: skip {} — {e}", path.display())),
            },
            Err(e) => crate::log_warn(&format!("theme: cannot read {} — {e}", path.display())),
        }
    }
    out
}

/// What the themes folder held when last read.
struct Scanned {
    /// Newest modification time of the folders and their files, to spot a change.
    stamp: Option<std::time::SystemTime>,
    /// When the stamp was last compared (at most once a second).
    checked: std::time::Instant,
    themes: std::sync::Arc<Vec<(String, Theme)>>,
    pictures: std::sync::Arc<Vec<(String, String)>>,
}

type Themes = std::sync::Arc<Vec<(String, Theme)>>;

/// Newest modification time of `dir` and the files in it.
fn dir_stamp(dir: &Path) -> Option<std::time::SystemTime> {
    let newest = std::fs::metadata(dir).and_then(|m| m.modified()).ok()?;
    let files = std::fs::read_dir(dir).ok()?.flatten().filter_map(|e| e.metadata().and_then(|m| m.modified()).ok());
    Some(files.fold(newest, |a, b| a.max(b)))
}

/// The file themes and the pictures they bring, reading the themes folder again
/// when something in it changed (checked at most once a second). So a theme or
/// a picture dropped in while the daemon runs is found without a restart; the
/// theme in use keeps its colours until it is selected again.
fn scanned() -> (Themes, std::sync::Arc<Vec<(String, String)>>) {
    static DIRS: OnceLock<Vec<PathBuf>> = OnceLock::new();
    static SCAN: std::sync::Mutex<Option<Scanned>> = std::sync::Mutex::new(None);
    let dirs = DIRS.get_or_init(themes_dirs);
    let mut guard = SCAN.lock().unwrap_or_else(|e| e.into_inner());
    let now = std::time::Instant::now();
    let due = guard.as_ref().is_none_or(|s| now.duration_since(s.checked) >= std::time::Duration::from_secs(1));
    if due {
        let stamp = dirs.iter().filter_map(|d| dir_stamp(d)).max();
        let changed = guard.as_ref().is_none_or(|s| s.stamp != stamp);
        match (changed, guard.as_mut()) {
            (false, Some(s)) => s.checked = now,
            _ => {
                let (themes, pictures) = load_all(dirs);
                *guard = Some(Scanned { stamp, checked: now, themes: themes.into(), pictures: pictures.into() });
            }
        }
    }
    let s = guard.as_ref().expect("scanned above");
    (s.themes.clone(), s.pictures.clone())
}

/// The file themes, as `(name, palette)`.
fn registry() -> Themes {
    scanned().0
}

/// Picture extensions a theme file pairs with, best first.
const PAIR_EXTS: [&str; 4] = ["jpg", "jpeg", "png", "webp"];

/// The wallpaper value each file theme brings, as `(lowercase name, value)`:
/// its `wallpaper` key (a relative path is looked up next to the theme file),
/// else the picture sharing its file stem (`dawn.toml` + `dawn.jpg`). Themes
/// with neither are left out. Pure (no globals), like [`load_dir`].
fn load_wallpapers(dir: &Path) -> Vec<(String, String)> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let files: Vec<String> = rd.flatten().filter_map(|e| e.file_name().into_string().ok()).collect();
    let mut out = Vec::new();
    for file in &files {
        let Some(stem) = file.strip_suffix(".toml") else { continue };
        let Ok(text) = std::fs::read_to_string(dir.join(file)) else { continue };
        let Ok(tf) = toml::from_str::<ThemeFile>(&text) else { continue };
        let name = tf.name.clone().unwrap_or_else(|| stem.to_string()).to_lowercase();
        let value = match tf.wallpaper.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            Some(v) if !v.starts_with(['/', '~']) && dir.join(v).exists() => Some(dir.join(v).to_string_lossy().into_owned()),
            Some(v) => Some(v.to_string()),
            None => PAIR_EXTS.iter().find_map(|ext| {
                let want = format!("{}.{ext}", stem.to_lowercase());
                files.iter().find(|f| f.to_lowercase() == want).map(|f| dir.join(f).to_string_lossy().into_owned())
            }),
        };
        if let Some(v) = value {
            out.push((name, v));
        }
    }
    out
}

/// The wallpaper value the theme `name` brings from its file, if any. Built-in
/// themes bring their embedded picture instead (see `wallpaper::theme_image`).
pub fn wallpaper_for(name: &str) -> Option<String> {
    let lname = name.trim().to_lowercase();
    scanned().1.iter().find(|(n, _)| *n == lname).map(|(_, v)| v.clone())
}

/// Whether `name` is a theme that exists: a built-in, the old `light`, or a
/// theme file. An unknown name renders as midnight.
pub fn known(name: &str) -> bool {
    let lname = name.trim().to_lowercase();
    lname == "light"
        || PRESETS.iter().any(|p| *p == lname)
        || registry().iter().any(|(n, _)| n.to_lowercase() == lname)
}

/// Write a built-in preset out as an editable theme file in the themes dir,
/// returning the path. Backs `tetron-wm theme --dump <name>`.
pub fn dump_builtin(name: &str) -> std::io::Result<PathBuf> {
    let dir = themes_dir().ok_or_else(|| std::io::Error::other("no themes directory"))?;
    let lname = name.to_lowercase();
    if lname != "light" && !PRESETS.contains(&lname.as_str()) {
        return Err(std::io::Error::other(format!(
            "'{name}' is not a built-in theme (built in: {}); the extra themes are files in {}",
            PRESETS.join(", "),
            dir.display()
        )));
    }
    std::fs::create_dir_all(&dir)?;
    let theme = Theme::builtin(&lname);
    let path = dir.join(format!("{name}.toml"));
    std::fs::write(&path, to_file_toml(&theme, name))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_builtin_matches_its_theme_file() {
        let (name, from_file) = parse_theme_toml(include_str!("../themes/windows.toml")).unwrap();
        assert_eq!(name.as_deref(), Some("windows"));
        assert_eq!(to_file_toml(&Theme::windows(), "windows"), to_file_toml(&from_file, "windows"));
        assert!(PRESETS.contains(&"windows"));
        assert!(validate(&Theme::named("windows")).is_empty());
    }

    #[test]
    fn optional_colours_follow_the_files_own_palette() {
        // A light file on the (dark) midnight base gets the light green, the
        // file's own close_fg as the error colour and white ink on a dark accent.
        let (_, t) = parse_theme_toml("window_bg = \"#ffffff\"\nclose_fg = \"#aa0000\"\naccent = \"#000080\"").unwrap();
        assert_eq!(t.success, rgb(26, 127, 55));
        assert_eq!(t.error, rgb(170, 0, 0));
        assert_eq!(t.accent_fg, rgb(255, 255, 255));
        assert_eq!(t.icon_fg, rgb(255, 255, 255));
        // A white accent would hide white text: black takes over.
        let (_, t) = parse_theme_toml("accent = \"#ffffff\"").unwrap();
        assert_eq!(t.accent_fg, rgb(0, 0, 0));
        // Set explicitly, they win; a bad value keeps the derived one.
        let (_, t) = parse_theme_toml("success = \"#00ff00\"\nerror = \"#ff00ff\"\naccent_fg = \"#101010\"\nicon_fg = \"nonsense\"").unwrap();
        assert_eq!((t.success, t.error, t.accent_fg), (rgb(0, 255, 0), rgb(255, 0, 255), rgb(16, 16, 16)));
        assert_eq!(t.icon_fg, rgb(255, 255, 255));
    }

    #[test]
    fn optional_colours_survive_a_dump_and_reload() {
        let (_, t) = parse_theme_toml("success = \"#00ff00\"\nicon_fg = \"#eeeeee\"").unwrap();
        let (_, back) = parse_theme_toml(&to_file_toml(&t, "x")).unwrap();
        assert_eq!((back.success, back.error, back.accent_fg, back.icon_fg), (t.success, t.error, t.accent_fg, t.icon_fg));
    }

    #[test]
    fn vt16_and_mono_cover_the_optional_colours() {
        let dark = vt16(Theme::midnight());
        assert_eq!((dark.success, dark.error), (crate::display::VGA16[10], crate::display::VGA16[9]));
        assert_eq!(dark.icon_fg, crate::display::VGA16[15]);
        let light = vt16(Theme::pastoral());
        assert_eq!((light.success, light.error), (crate::display::VGA16[2], crate::display::VGA16[1]));
        // A white-ish accent gets black ink; [vt16] overrides win.
        let mut t = Theme::midnight();
        t.accent = rgb(255, 255, 255);
        assert_eq!(vt16(t).accent_fg, crate::display::VGA16[0]);
        t.vt16 = Vt16 { success: Some(14), error: Some(13), accent_fg: Some(4), icon_fg: Some(7), ..Vt16::NONE };
        let o = vt16(t);
        let p = &crate::display::VGA16;
        assert_eq!((o.success, o.error, o.accent_fg, o.icon_fg), (p[14], p[13], p[4], p[7]));
        let m = mono(Theme::midnight());
        assert_eq!((m.success, m.error, m.accent_fg), (rgb(192, 192, 192), rgb(255, 255, 255), rgb(0, 0, 0)));
    }

    #[test]
    fn ansi_table_sets_named_colours_and_survives_a_dump() {
        let (_, t) = parse_theme_toml("[ansi]\nred = \"#ff0000\"\nbright_white = \"#fefefe\"\nbogus = \"#000000\"\nblue = \"nope\"").unwrap();
        assert_eq!(t.ansi[1], rgb(255, 0, 0));
        assert_eq!(t.ansi[15], rgb(254, 254, 254));
        assert_eq!(t.ansi[4], DEFAULT_ANSI[4], "a bad value keeps the default");
        assert_eq!(t.ansi[2], DEFAULT_ANSI[2], "unset keeps the default");
        // The dump lists the palette commented out; uncommented it reloads equal.
        let dump = to_file_toml(&t, "x");
        assert!(dump.contains("# red = \"#ff0000\""));
        let live: String = dump.lines().map(|l| l.strip_prefix("# ").filter(|r| r.starts_with("[ansi]") || ANSI_NAMES.iter().any(|n| r.starts_with(&format!("{n} =")))).unwrap_or(l)).collect::<Vec<_>>().join("\n");
        let (_, back) = parse_theme_toml(&live).unwrap();
        assert_eq!(back.ansi, t.ansi);
        assert_eq!(resolve_ansi("midnight").len(), 16);
    }

    #[test]
    fn set_ansi_replaces_only_a_full_valid_list() {
        let mut t = Theme::midnight();
        let mut list: Vec<String> = DEFAULT_ANSI.iter().map(|c| to_hex(*c)).collect();
        apply_ansi(&mut t, &list[..3]);
        assert_eq!(t.ansi, DEFAULT_ANSI);
        list[1] = "#123456".into();
        list[2] = "garbage".into();
        apply_ansi(&mut t, &list);
        assert_eq!(t.ansi[1], rgb(0x12, 0x34, 0x56));
        assert_eq!(t.ansi[2], DEFAULT_ANSI[2]);
    }

    #[test]
    fn alpha_hex_parses() {
        assert_eq!(crate::badge::parse_color("#11223344"), Some(Rgba { r: 0x11, g: 0x22, b: 0x33, a: 0x44 }));
        assert_eq!(crate::badge::parse_color("#112233"), Some(Rgba { r: 0x11, g: 0x22, b: 0x33, a: 255 }));
        assert_eq!(crate::badge::parse_color("#1234"), None); // bad length
    }

    #[test]
    fn partial_file_inherits_base() {
        // `base = "light"` also exercises the deprecated light → pastoral alias.
        let (name, t) = parse_theme_toml("name = \"x\"\nbase = \"light\"\naccent = \"#010203\"\n").unwrap();
        assert_eq!(name.as_deref(), Some("x"));
        assert_eq!(t.accent, Rgba { r: 1, g: 2, b: 3, a: 255 });
        assert_eq!(t.window_bg, Theme::pastoral().window_bg); // unset inherits from base
    }

    #[test]
    fn invalid_color_keeps_base_default_not_fatal() {
        let (_n, t) = parse_theme_toml("accent = \"not-a-color\"\n").unwrap();
        assert_eq!(t.accent, Theme::midnight().accent); // default base = midnight
    }

    #[test]
    fn unknown_keys_are_ignored() {
        // Version-skew: a file from a newer tetron-wm with extra keys still loads.
        let r = parse_theme_toml("accent = \"#abcdef\"\nfuture_field = \"nope\"\n");
        assert!(r.is_ok());
        assert_eq!(r.unwrap().1.accent, Rgba { r: 0xab, g: 0xcd, b: 0xef, a: 255 });
    }

    #[test]
    fn dump_roundtrips_including_alpha() {
        let src = Theme::gruvbox();
        let (_n, back) = parse_theme_toml(&to_file_toml(&src, "gruvbox")).unwrap();
        assert_eq!(back.desktop_bg, src.desktop_bg);
        assert_eq!(back.accent, src.accent);
        assert_eq!(back.shadow, src.shadow); // alpha survives
    }

    #[test]
    fn validate_flags_low_contrast_button() {
        let mut t = Theme::midnight();
        t.window_bg = Rgba::rgb(0, 180, 0);
        t.accent = Rgba::rgb(0, 170, 0); // green-on-green: unreadable affirmative button
        assert!(validate(&t).iter().any(|s| s.contains("affirmative")), "{:?}", validate(&t));
    }

    #[test]
    fn validate_passes_shipped_pastoral_and_midnight() {
        assert!(validate(&Theme::midnight()).is_empty(), "{:?}", validate(&Theme::midnight()));
        assert!(validate(&Theme::pastoral()).is_empty(), "{:?}", validate(&Theme::pastoral()));
    }

    #[test]
    fn light_alias_resolves_to_pastoral() {
        // Use builtin() (not named()) to stay hermetic — no registry / home-dir IO.
        assert_eq!(Theme::builtin("light").accent, Theme::pastoral().accent);
        assert_eq!(Theme::builtin("pastoral").accent, Theme::pastoral().accent);
    }

    #[test]
    fn a_theme_brings_the_picture_with_its_stem_or_the_one_it_names() {
        let dir = std::env::temp_dir().join(format!("tetron-theme-pics-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let w = |f: &str, t: &str| std::fs::write(dir.join(f), t).unwrap();
        w("dawn.toml", "name = \"Dawn\"\n");
        w("dawn.PNG", "x"); // pairs by file stem, any case
        w("dawn.jpg", "x"); // jpg beats png
        w("sea.toml", "wallpaper = \"waves.webp\"\n");
        w("waves.webp", "x"); // named, next to the file
        w("flat.toml", "wallpaper = \"#102030\"\n");
        w("quiet.toml", "wallpaper = \"none\"\n");
        w("quiet.jpg", "x"); // an explicit key beats pairing
        w("bare.toml", "accent = \"#010203\"\n"); // no picture at all
        w("abs.toml", "wallpaper = \"/usr/share/x.png\"\n");
        let got: std::collections::BTreeMap<_, _> = load_wallpapers(&dir).into_iter().collect();
        let d = |f: &str| dir.join(f).to_string_lossy().into_owned();
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(got.get("dawn"), Some(&d("dawn.jpg")));
        assert_eq!(got.get("sea"), Some(&d("waves.webp")));
        assert_eq!(got.get("flat").map(String::as_str), Some("#102030"));
        assert_eq!(got.get("quiet").map(String::as_str), Some("none"));
        assert_eq!(got.get("bare"), None);
        assert_eq!(got.get("abs").map(String::as_str), Some("/usr/share/x.png"));
    }

    #[test]
    fn removed_names_are_midnight_until_their_file_is_installed() {
        // builtin() ignores the registry, so this is hermetic.
        for n in ["newmoon", "nord", "dracula", "nonsense"] {
            assert_eq!(Theme::builtin(n).desktop_bg, Theme::midnight().desktop_bg, "{n}");
            assert!(!PRESETS.contains(&n), "{n}");
        }
        assert_eq!(PRESETS, ["midnight", "gruvbox", "pastoral", "windows"]);
    }

    /// The files in the repo `themes/` folder, as the suite installer lays them
    /// out, bring the pictures they promise.
    #[test]
    fn the_shipped_theme_folder_pairs_pictures_with_themes() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("themes");
        let got: std::collections::BTreeMap<_, _> = load_wallpapers(&dir).into_iter().collect();
        let pic = |f: &str| dir.join(f).to_string_lossy().into_owned();
        for n in ["nord", "dracula", "dawn", "midnight", "gruvbox", "pastoral", "windows"] {
            assert_eq!(got.get(n), Some(&pic(&format!("{n}.jpg"))), "{n}");
        }
        assert_eq!(got.get("newmoon").map(String::as_str), Some("black"));
        assert_eq!(got.get("contrast").map(String::as_str), Some("none"));
    }

    #[test]
    fn a_picture_added_later_changes_the_folder_stamp() {
        let dir = std::env::temp_dir().join(format!("tetron-theme-stamp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("dawn.toml"), "name = \"dawn\"\n").unwrap();
        let before = dir_stamp(&dir).unwrap();
        assert!(load_wallpapers(&dir).is_empty());
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(dir.join("dawn.jpg"), "x").unwrap();
        let after = dir_stamp(&dir).unwrap();
        let found = load_wallpapers(&dir);
        std::fs::remove_dir_all(&dir).ok();
        assert!(after > before, "adding a file moves the stamp");
        assert_eq!(found.len(), 1, "and the picture is paired");
    }

    #[test]
    fn known_covers_builtins_the_old_light_name_and_nothing_else() {
        for n in PRESETS {
            assert!(known(n), "{n}");
        }
        assert!(known("Light") && known(" gruvbox "));
        assert!(!known("no-such-theme"));
    }

    /// The repo `themes/` folder is what users install: every file must load
    /// complete and readable, with the picture its `wallpaper` key names.
    #[test]
    fn repo_theme_files_are_complete_and_readable() {
        const ROLES: [&str; 15] = [
            "desktop_bg", "window_bg", "title_focus", "title_blur", "title_fg", "border", "shadow", "ctrl_fg",
            "close_fg", "menubar_bg", "dock_bg", "text", "dim", "accent", "active_bg",
        ];
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let dir = root.join("themes");
        let mut names = Vec::new();
        for ent in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = ent.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
            let (name, theme) = parse_theme_toml(&text).unwrap_or_else(|e| panic!("{stem}: {e}"));
            assert_eq!(name.as_deref(), Some(stem.as_str()), "{stem}: name matches the file");
            let doc: toml::Table = text.parse().unwrap();
            for role in ROLES {
                assert!(doc.contains_key(role), "{stem}: missing {role}");
            }
            // nord and dracula keep the stock palettes, whose inactive title bar
            // sits close to the desktop colour (a picture covers it in use);
            // contrast makes it black on a black desktop on purpose, and the
            // white window border sets it off.
            let stock = stem == "nord" || stem == "dracula" || stem == "contrast";
            let warns: Vec<String> = validate(&theme).into_iter().filter(|w| !(stock && w.contains("inactive titlebar"))).collect();
            assert!(warns.is_empty(), "{stem}: {warns:?}");
            // A picture a theme names with its `wallpaper` key ships next to it
            // (keywords such as none, black, default and #rrggbb colours are not files).
            if let Some(w) = doc.get("wallpaper").and_then(|v| v.as_str()) {
                let is_picture = Path::new(w).extension().and_then(|e| e.to_str())
                    .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "jpg" | "jpeg" | "png" | "webp"));
                if is_picture {
                    assert!(dir.join(w).is_file(), "{stem}: wallpaper themes/{w} is missing");
                }
            }
            names.push(stem);
        }
        // The folder is the list: the suite installer and Settings > Appearance >
        // Get more themes both download whatever is in themes/.
        assert!(!names.is_empty(), "themes/ has theme files");
    }

    #[test]
    fn search_dirs_order_and_override() {
        let p = PathBuf::from;
        let user = Some(p("/h/.config/tetron-wm/themes"));
        let dl = Some(p("/h/.local/share/tetron-wm/themes"));
        assert_eq!(
            search_dirs(None, user.clone(), dl.clone(), None),
            vec![
                p("/h/.config/tetron-wm/themes"),
                p("/h/.local/share/tetron-wm/themes"),
                p("/usr/local/share/tetron-wm/themes"),
                p("/usr/share/tetron-wm/themes"),
            ],
            "default XDG_DATA_DIRS"
        );
        assert_eq!(
            search_dirs(None, user.clone(), dl.clone(), Some("/opt/share:relative:/opt/share".into())),
            vec![p("/h/.config/tetron-wm/themes"), p("/h/.local/share/tetron-wm/themes"), p("/opt/share/tetron-wm/themes")],
            "set XDG_DATA_DIRS replaces the default; relative entries and duplicates dropped"
        );
        assert_eq!(search_dirs(None, user.clone(), dl.clone(), Some("".into())).len(), 4, "empty means unset");
        assert_eq!(search_dirs(Some("/t".into()), user, dl, None), vec![p("/t")], "override is exclusive");
    }

    #[test]
    fn data_home_follows_xdg_spec() {
        let home = Some(PathBuf::from("/h"));
        assert_eq!(data_home(None, home.clone()), Some(PathBuf::from("/h/.local/share")));
        assert_eq!(data_home(Some("/x".into()), home.clone()), Some(PathBuf::from("/x")));
        assert_eq!(data_home(Some("rel".into()), home.clone()), Some(PathBuf::from("/h/.local/share")), "relative is invalid per the spec");
        assert_eq!(data_home(Some("".into()), home), Some(PathBuf::from("/h/.local/share")));
    }

    #[test]
    fn load_all_first_dir_wins_and_keeps_its_own_picture() {
        let root = std::env::temp_dir().join(format!("tetron-theme-dirs-{}", std::process::id()));
        let (a, b, gone) = (root.join("a"), root.join("b"), root.join("missing"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        // "dawn" in both: a's colours win, and a has no picture, so b's dawn.jpg must not attach.
        std::fs::write(a.join("dawn.toml"), "accent = \"#111111\"\n").unwrap();
        std::fs::write(b.join("dawn.toml"), "accent = \"#222222\"\n").unwrap();
        std::fs::write(b.join("dawn.jpg"), "x").unwrap();
        // "nord" only in b, with its picture.
        std::fs::write(b.join("nord.toml"), "accent = \"#333333\"\n").unwrap();
        std::fs::write(b.join("nord.jpg"), "x").unwrap();
        let (themes, pics) = load_all(&[a.clone(), gone, b.clone()]);
        std::fs::remove_dir_all(&root).ok();
        let dawn = themes.iter().find(|(n, _)| n == "dawn").expect("dawn");
        assert_eq!(dawn.1.accent, Rgba { r: 0x11, g: 0x11, b: 0x11, a: 255 }, "first dir wins");
        assert_eq!(themes.iter().filter(|(n, _)| n == "dawn").count(), 1, "no duplicate");
        assert!(themes.iter().any(|(n, _)| n == "nord"), "later dir adds new names");
        assert!(!pics.iter().any(|(n, _)| n == "dawn"), "b's picture not paired with a's theme: {pics:?}");
        assert_eq!(pics.iter().find(|(n, _)| n == "nord").map(|(_, v)| v.clone()), Some(b.join("nord.jpg").to_string_lossy().into_owned()));
    }

    #[test]
    fn load_dir_reads_toml_and_skips_junk() {
        let dir = std::env::temp_dir().join(format!("tetron-theme-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("solar.toml"), "accent = \"#abcdef\"\n").unwrap();
        std::fs::write(dir.join("notatheme.txt"), "ignored").unwrap();
        std::fs::write(dir.join("broken.toml"), "accent = [unclosed\n").unwrap();
        let loaded = load_dir(&dir);
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].0, "solar");
        assert_eq!(loaded[0].1.accent, Rgba { r: 0xab, g: 0xcd, b: 0xef, a: 255 });
    }
}

#[cfg(test)]
mod depth_tests {
    use super::*;
    use crate::display::{ColorDepth, VGA16};

    /// Print the 16-colour role table of every built-in, plus any theme files
    /// listed in `TETRON_WM_VT16_DUMP` (space-separated paths). A design aid:
    /// `TETRON_WM_VT16_DUMP="a.toml b.toml" cargo test vt16_table -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn vt16_table() {
        let idx = |c: Rgba| VGA16.iter().position(|p| *p == c).unwrap();
        let mut themes: Vec<(String, Theme)> = PRESETS.iter().map(|n| (n.to_string(), Theme::named(n))).collect();
        for p in std::env::var("TETRON_WM_VT16_DUMP").unwrap_or_default().split_whitespace() {
            let (n, t) = parse_theme_toml(&std::fs::read_to_string(p).unwrap()).unwrap();
            themes.push((n.unwrap_or_else(|| p.into()), t));
        }
        println!("theme       desk win focus blur tfg menu dock text dim acc sel close");
        for (n, t) in themes {
            let a = adapt(t, ColorDepth::Ansi16);
            println!("{n:<11} {:>4} {:>3} {:>5} {:>4} {:>3} {:>4} {:>4} {:>4} {:>3} {:>3} {:>3} {:>5}",
                idx(a.desktop_bg), idx(a.window_bg), idx(a.title_focus), idx(a.title_blur), idx(a.title_fg),
                idx(a.menubar_bg), idx(a.dock_bg), idx(a.text), idx(a.dim), idx(a.accent), idx(a.active_bg), idx(a.close_fg));
        }
    }

    fn all_roles(t: &Theme) -> [Rgba; 14] {
        [t.desktop_bg, t.window_bg, t.title_focus, t.title_blur, t.title_fg, t.border,
         t.ctrl_fg, t.close_fg, t.menubar_bg, t.dock_bg, t.text, t.dim, t.accent, t.active_bg]
    }

    #[test]
    fn full_colour_depths_keep_the_theme() {
        let t = Theme::midnight();
        assert_eq!(adapt(t, ColorDepth::TrueColor).desktop_bg, t.desktop_bg);
        assert_eq!(adapt(t, ColorDepth::Ansi256).active_bg, t.active_bg);
    }

    /// The bug this exists for: quantized midnight drew desktop, window and
    /// selection all on black. Every built-in, light or dark, must keep the
    /// selection apart from desktop and window (dark themes deliberately share a
    /// black desktop and window) and keep body text readable.
    #[test]
    fn vt16_keeps_desktop_window_and_selection_apart() {
        for name in PRESETS {
            let t = adapt(Theme::named(name), ColorDepth::Ansi16);
            for c in all_roles(&t) {
                assert!(VGA16.contains(&c), "{name}: {c:?} is not an exact VGA colour");
            }
            for bg in [t.desktop_bg, t.window_bg, t.active_bg, t.title_focus, t.menubar_bg] {
                assert!(VGA16[..8].contains(&bg), "{name}: bright background {bg:?} (a VT has 8)");
            }
            assert_ne!(t.active_bg, t.window_bg, "{name}");
            if Theme::named(name).vt16.active_bg.is_none() {
                assert_ne!(t.active_bg, t.desktop_bg, "{name}"); // a pinned choice may match
            }
            assert!(contrast(t.text, t.window_bg) >= 4.5, "{name}: text on window");
            assert!(contrast(t.dim, t.window_bg) >= 2.0, "{name}: dim on window");
            assert!(contrast(t.title_fg, t.title_focus) >= 2.0, "{name}: focused title");
            // Focused and blurred titles differ; two neutrals must also differ
            // in brightness (a hue against a neutral is distinct by itself).
            assert_ne!(t.title_focus, t.title_blur, "{name}: focused vs blurred title");
            let neutral = |c: Rgba| chroma(c) == 0;
            if neutral(t.title_focus) && neutral(t.title_blur) {
                assert!(contrast(t.title_focus, t.title_blur) >= 3.0, "{name}: focused vs blurred title");
            }
        }
    }

    #[test]
    fn vt16_keeps_a_themes_own_hues_and_honours_overrides() {
        let idx = |c: Rgba| VGA16.iter().position(|p| *p == c).unwrap();
        // A Windows 3.1 style file theme: teal desktop, navy titles, red close.
        let (_, t) = parse_theme_toml(concat!(
            "desktop_bg = \"#008080\"\nwindow_bg = \"#ffffff\"\ntitle_focus = \"#000080\"\n",
            "title_blur = \"#808080\"\ntext = \"#000000\"\nclose_fg = \"#cc0000\"\n",
            "active_bg = \"#c0c0c0\"\n",
        )).unwrap();
        let a = adapt(t, ColorDepth::Ansi16);
        assert_eq!((idx(a.desktop_bg), idx(a.title_focus), idx(a.close_fg)), (6, 4, 1)); // close: generic red
        // Light theme: grey windows, and the grey selection falls back to blue.
        assert_eq!((idx(a.window_bg), idx(a.active_bg)), (7, 4));
        // Dark themes keep black windows whatever their tint.
        assert_eq!(idx(adapt(Theme::midnight(), ColorDepth::Ansi16).window_bg), 0);
        // Built-in 16-colour overrides.
        let p = adapt(Theme::pastoral(), ColorDepth::Ansi16);
        assert_eq!((idx(p.desktop_bg), idx(p.title_focus), idx(p.active_bg)), (7, 2, 2));
        assert_eq!(idx(adapt(Theme::gruvbox(), ColorDepth::Ansi16).desktop_bg), 3);
        // A file `[vt16]` table wins; a bright background folds to the dark half.
        let (_, t) = parse_theme_toml("[vt16]\ndesktop_bg = 12\naccent = 11\n").unwrap();
        let a = adapt(t, ColorDepth::Ansi16);
        assert_eq!((idx(a.desktop_bg), idx(a.accent)), (4, 11));
    }

    #[test]
    fn mono_highlights_are_light_and_plain_backgrounds_dark() {
        for name in PRESETS {
            let t = adapt(Theme::named(name), ColorDepth::Mono);
            assert!(luminance(t.active_bg) >= 0.2 && luminance(t.title_focus) >= 0.2, "{name}");
            for bg in [t.desktop_bg, t.window_bg, t.menubar_bg, t.dock_bg, t.title_blur] {
                assert!(luminance(bg) < 0.2, "{name}");
            }
            assert!(luminance(t.dim) < 0.3 && luminance(t.text) >= 0.3, "{name}: dim must read as dim");
        }
    }
}
