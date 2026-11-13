//! The settings panel: a native window with a category sidebar and a content
//! pane of editable settings, backed by [`Config`]. Changes are applied live and
//! persisted to `config.toml` by the session.

use crate::buffer::CellBuffer;
use crate::cell::{Cell, Rgba};
use crate::config::{AppEntry, Config};
use crate::geometry::Point;

// Chrome colors follow the active desktop theme (were hardcoded midnight, which
// left Settings dark under every theme). Named `pal_*` to avoid colliding with
// local `bg`/`fg` bindings in the render code.
fn pal_bg() -> Rgba { crate::theme::current().window_bg }
fn pal_fg() -> Rgba { crate::theme::current().text }
fn pal_dim() -> Rgba { crate::theme::current().dim }
fn pal_sel() -> Rgba { crate::theme::current().active_bg }
fn pal_accent() -> Rgba { crate::theme::current().accent }
// Semantic colors: the theme's `success` and `error`.
fn red() -> Rgba { crate::theme::current().error }
fn green() -> Rgba { crate::theme::current().success }

const SIDEBAR_W: i32 = 18;
const SECTIONS: &[&str] = &["Appearance", "Windows", "Updates", "Apps", "Default Apps", "AI", "Clipboard", "Terminal", "Sound", "Display", "General", "About"];

// Indices into [`SECTIONS`]; Settings opens on the first one. Code and tests
// name a section by these, never by a bare number.
const APPEARANCE: usize = 0;
const WINDOWS: usize = 1;
const UPDATES: usize = 2;
const APPS: usize = 3;
const DEFAULT_APPS: usize = 4;
const ASSISTANT: usize = 5;
const CLIPBOARD: usize = 6;
const TERMINAL: usize = 7;
const SOUND: usize = 8;
const DISPLAY: usize = 9;
const GENERAL: usize = 10;
// About is last (index 11) and has no rows.

/// Roles listed in the Default Apps section (config key, label).
const DEFAULT_APP_ROLES: &[(&str, &str)] = &[
    ("image", "Images"),
    ("text", "Text"),
    ("code", "Code"),
    ("audio", "Audio"),
    ("video", "Video"),
    ("pdf", "PDF"),
    ("archive", "Archives"),
    ("editor", "Editor"),
    ("terminal", "Terminal"),
];

/// An action the session must perform on behalf of the settings panel
/// (these touch the network / spawn processes, so the panel only requests them).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsAction {
    /// Restart the app server (closes all apps) — shown only after a compat
    /// warning flagged the running apphost as too old for this binary.
    RestartApphost,
    /// Check upstream for a newer commit.
    CheckUpdates,
    /// Install the latest version.
    InstallUpdate,
    /// Empty the clipboard history (memory and disk).
    ClearClipboard,
    /// Send the panel's staged terminal colors to the apphost without saving
    /// (read them with [`Settings::staged_colors`]).
    PreviewColors,
    /// Put the saved terminal colors back after a preview.
    RevertColors,
    /// Sound section: change the system volume / mute / default output now
    /// (machine state, never written to the config).
    VolumeDown,
    VolumeUp,
    VolumeSet(u8),
    ToggleMute,
    SetOutput(u32),
    /// Sound section: open the mixer program (or the Store when none).
    OpenMixer,
    /// Sound section was entered: re-read the output list (a shell-out, so it
    /// runs on this action, not on every frame).
    RefreshSound,
    /// Windows section: enable and start (`true`) or disable and stop the full
    /// screen waiter service. Runs `sudo systemctl` on the real terminal.
    WaiterService(bool),
    /// Display: set the screen brightness to this percent now (machine
    /// state, never written to the config).
    BrightnessSet(u8),
    /// General > Config file: open it in the Default Apps editor.
    OpenConfigFile,
    /// General > Config file: write the example (only when there is no file).
    CreateConfigFile,
    /// General > Config file: keep the file as `config.toml.bak` and write the
    /// example (asked only after the row's second Enter).
    ResetConfigFile,
    /// General > Config file: reload the desktop so hand edits apply.
    ReloadConfig,
    /// Appearance > Get more themes: download missing theme files on a worker.
    GetThemes,
    /// General > Apps: install the apps of this size with the suite script in
    /// a terminal window (WMINST-007, WMINST-011); `bool`: with the apps no
    /// distribution packages.
    InstallApps(crate::packaging::Size, bool),
    /// General > Apps > Remove set: remove the size debs of this size and
    /// larger with apt in a terminal window (WMINST-011).
    RemoveApps(crate::packaging::Size),
    /// General > Apps: compute the confirmation's download size off the loop
    /// (the packages come from [`Settings::apps_download_pkgs`]).
    AppsDownloadSize,
    /// Assistant > Agent: open the Store on this app (an agent not installed).
    OpenStoreFor(String),
}

/// What the open pick list chooses (WMAST-002).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PickFor {
    Agent,
    Cwd,
}

/// Picker key of the "Custom command…" row.
const PICK_CUSTOM: &str = "@custom";
/// Picker key prefix of an agent that is not installed.
const PICK_INSTALL: &str = "@install:";

/// Terminal color presets: (name, background, foreground). `None` = follow the
/// desktop theme.
const TERM_PRESETS: &[(&str, Option<&str>, Option<&str>)] = &[
    ("follow theme", None, None),
    ("dark", Some("#0d1117"), Some("#c9d1d9")),
    ("light", Some("#ffffff"), Some("#1a1a1a")),
    ("black", Some("#000000"), Some("#e6e6e6")),
    ("solarized dark", Some("#002b36"), Some("#839496")),
    ("solarized light", Some("#fdf6e3"), Some("#657b83")),
    ("gruvbox dark", Some("#282828"), Some("#ebdbb2")),
    ("nord", Some("#2e3440"), Some("#d8dee9")),
    ("dracula", Some("#282a36"), Some("#f8f8f2")),
    ("green phosphor", Some("#000000"), Some("#33ff66")),
    ("amber phosphor", Some("#000000"), Some("#ffb000")),
];

/// Which setting the single-line text box is editing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TextTarget {
    Wallpaper,
    /// Appearance → Launcher label (menubar button text).
    LauncherLabel,
    TermBg,
    TermFg,
    /// AI → Agent → Custom command… (a command line).
    AgentCommand,
    /// AI → Working folder → Other folder… (a path).
    AiCwd,
}

/// The open text box: its target, the in-progress text, and whether the last
/// commit attempt was rejected (invalid color).
struct TextEdit {
    target: TextTarget,
    buf: String,
    error: bool,
    /// The pre-filled text acts as selected: the first typed or pasted
    /// character replaces it and Backspace clears it. Set when the wallpaper
    /// box opens on the `default` keyword, so a path can be typed straight in.
    fresh: bool,
}

/// In-progress custom-app entry shown in the Apps section's add form.
#[derive(Default)]
struct AppEdit {
    name: String,
    command: String,
    /// Focused field: 0 = name, 1 = command.
    field: u8,
}

/// Settings panel state (owns a working copy of the config).
pub struct Settings {
    /// Whether the running apphost is older than this binary supports.
    apphost_outdated: bool,
    cfg: Config,
    section: usize,
    sel: usize,
    action: Option<SettingsAction>,
    update_status: String,
    /// `Some` while the Apps section's add form is open.
    edit: Option<AppEdit>,
    /// `Some` while a single-line text box is open (wallpaper filename, terminal
    /// background/foreground), committed on Enter.
    text: Option<TextEdit>,
    /// Terminal colors being edited in the Terminal section. They stay out of
    /// `cfg` (the saved values) until Save, so a preview never reaches disk.
    stage_bg: Option<String>,
    stage_fg: Option<String>,
    /// A preview of the staged colors is live in the apphost (unsaved).
    previewing: bool,
    /// The apphost can apply colors live (protocol v4+).
    colors_live: bool,
    /// The poller's live system snapshot, for the Sound section's volume and
    /// mute (shared with the tray, so both always show the same level).
    sound: Option<std::sync::Arc<std::sync::RwLock<crate::system::SystemState>>>,
    /// Audio outputs as of the last [`SettingsAction::RefreshSound`].
    outputs: Vec<crate::system::AudioOutput>,
    /// Keyboard focus is on the section list (left pane) rather than the rows.
    /// Tab switches panes, like mc; Up / Down move inside the focused one.
    in_sidebar: bool,
    /// Problem with the wallpaper setting (missing file, no chafa), shown under
    /// the Appearance rows. Refreshed on open and on each commit, not per frame.
    wallpaper_note: Option<String>,
    /// The waiter service as of open / the last section entry (a shell-out, so
    /// never per frame).
    waiter: crate::fullscreen::Service,
    /// Display: the backlight in percent as of open / entering Display (and
    /// after each change); `None` without a backlight.
    brightness: Option<u8>,
    /// Whether `config.toml` exists, as of open / entering General.
    cfg_exists: bool,
    /// General > Reset was pressed once; the next Enter on it resets. Any other
    /// key or move disarms it.
    reset_armed: bool,
    /// Appearance > Get more themes: progress or the last result.
    themes_status: String,
    /// A theme download is running (Enter on the row does nothing meanwhile).
    themes_busy: bool,
    /// About: how tetron-wm was installed and the size (WMINST-005), read on
    /// open and on entering About, never per frame.
    install_info: String,
    /// General > Apps: the installed size and the chosen target, read on
    /// open and on entering General.
    apps_size: crate::packaging::Size,
    /// Apps of the installed size this machine lacks (WMINST-007).
    apps_missing: Vec<String>,
    have_curl: bool,
    /// The Apps dialog, while open (WMINST-011).
    apps_dlg: Option<crate::appsdlg::AppsDlg>,
    /// The pick list, while open, and what it sets.
    picker: Option<(PickFor, crate::listpicker::ListPicker)>,
}

/// "apt package, size essentials" for Settings > About.
fn install_info() -> String {
    format!("{}, size {}", crate::packaging::install_method(), crate::packaging::installed_size().name())
}

/// Whether the config file is on disk.
fn config_file_exists() -> bool {
    crate::config::file_path().is_ok_and(|p| p.exists())
}

impl Settings {
    /// Create a settings panel editing a copy of `cfg`.
    pub fn new(cfg: Config) -> Self {
        let (stage_bg, stage_fg) = (cfg.terminal_bg.clone(), cfg.terminal_fg.clone());
        Self {
            apphost_outdated: false, cfg, section: 0, sel: 0, action: None, update_status: String::new(),
            edit: None, text: None, stage_bg, stage_fg, previewing: false, colors_live: true,
            sound: None, outputs: Vec::new(), in_sidebar: false, wallpaper_note: None,
            waiter: crate::fullscreen::service_state(), brightness: crate::brightness::read_pct(), cfg_exists: config_file_exists(), reset_armed: false,
            themes_status: String::new(), themes_busy: false, install_info: install_info(),
            apps_size: crate::packaging::installed_size(), apps_missing: Vec::new(), apps_dlg: None, picker: None, have_curl: crate::catalog::is_installed("curl"),
        }.with_wallpaper_note()
    }

    /// Recompute [`Self::wallpaper_note`] from the current config.
    fn with_wallpaper_note(mut self) -> Self {
        self.wallpaper_note = crate::wallpaper::note(self.cfg.wallpaper.as_deref());
        self
    }

    /// Take a pending action requested by the user (cleared on read).
    pub fn take_action(&mut self) -> Option<SettingsAction> {
        self.action.take()
    }

    /// Set the text shown under the Updates section after a check.
    pub fn set_update_status(&mut self, s: String) {
        self.update_status = s;
    }

    /// Show the "Restart app server" row (set when the apphost was flagged as
    /// older than this binary's minimum-compatible protocol).
    pub fn set_apphost_outdated(&mut self, outdated: bool) {
        self.apphost_outdated = outdated;
    }

    /// Whether the running apphost can apply terminal colors live; when it can
    /// not, the Terminal section says a restart is needed.
    pub fn set_colors_live(&mut self, live: bool) {
        self.colors_live = live;
    }

    /// Give the panel the live system snapshot (volume, mute, caps).
    pub fn set_sound_source(&mut self, st: std::sync::Arc<std::sync::RwLock<crate::system::SystemState>>) {
        self.sound = Some(st);
    }

    /// Replace the waiter service state (after a refresh or a switch).
    pub fn set_waiter(&mut self, st: crate::fullscreen::Service) {
        self.waiter = st;
    }

    /// Replace the audio output list (after a refresh or an output switch).
    pub fn set_outputs(&mut self, outputs: Vec<crate::system::AudioOutput>) {
        self.outputs = outputs;
    }

    /// Volume, mute and whether volume control works, from the live snapshot.
    fn sound_now(&self) -> (u8, bool, bool) {
        self.sound
            .as_ref()
            .and_then(|s| s.read().ok().map(|st| (st.volume.level, st.volume.muted, st.caps.volume)))
            .unwrap_or((0, false, false))
    }

    /// Switch to section `i`, asking the session to refresh the outputs when
    /// it is Sound.
    fn enter_section(&mut self, i: usize) {
        self.apps_dlg = None;
        self.section = i;
        self.sel = 0;
        self.reset_armed = false;
        if i == SOUND {
            self.action = Some(SettingsAction::RefreshSound);
        } else if i == WINDOWS {
            self.waiter = crate::fullscreen::service_state();
        } else if i == DISPLAY {
            self.brightness = crate::brightness::read_pct();
        } else if i == GENERAL {
            self.cfg_exists = config_file_exists();
            self.refresh_apps();
        } else if i == SECTIONS.len() - 1 {
            self.install_info = install_info();
        }
    }

    /// Mouse wheel at a content-local point: on the Sound section's volume row
    /// it steps the volume (`lines` > 0 = wheel up = louder). Returns whether
    /// the wheel was used here.
    pub fn wheel_at(&mut self, local: Point, lines: i32) -> bool {
        if let Some((_, p)) = self.picker.as_mut() {
            if lines != 0 {
                p.scroll_by(lines);
            }
            return lines != 0;
        }
        if self.section != SOUND || local.y != 3 || local.x < SIDEBAR_W || lines == 0 {
            return false;
        }
        self.sel = 0;
        self.action = Some(if lines > 0 { SettingsAction::VolumeUp } else { SettingsAction::VolumeDown });
        true
    }

    /// The terminal colors being edited (background, foreground), unsaved.
    pub fn staged_colors(&self) -> (Option<&str>, Option<&str>) {
        (self.stage_bg.as_deref(), self.stage_fg.as_deref())
    }

    /// A preview of the staged colors is live and unsaved.
    pub fn previewing(&self) -> bool {
        self.previewing
    }

    /// Show a theme download's progress (`done` = it finished; the row takes
    /// Enter again).
    pub fn set_themes_status(&mut self, s: String, done: bool) {
        self.themes_status = s;
        self.themes_busy = !done;
    }

    /// Re-read the installed size and curl (after an install window closed).
    pub fn refresh_apps(&mut self) {
        self.apps_size = crate::packaging::installed_size();
        self.apps_missing = crate::packaging::missing_apps(self.apps_size);
        self.have_curl = crate::catalog::is_installed("curl");
        self.install_info = install_info();
    }

    /// Re-check whether the config file exists (after Create / Reset).
    pub fn refresh_config_file(&mut self) {
        self.cfg_exists = config_file_exists();
    }

    /// Jump to the General section (used to reopen there after a config reload).
    pub fn show_general_section(&mut self) {
        self.enter_section(GENERAL);
    }

    /// Jump to the Updates section (used to reopen there after an update reload).
    pub fn show_updates_section(&mut self) {
        if let Some(i) = SECTIONS.iter().position(|s| *s == "Updates") {
            self.section = i;
            self.sel = 0;
        }
    }

    /// The live (edited) config.
    pub fn config(&self) -> &Config {
        &self.cfg
    }

    /// Whether the Apps section's text-entry form is currently open (the client
    /// forwards typed characters only in this state).
    pub fn is_editing(&self) -> bool {
        self.edit.is_some() || self.text.is_some() || self.apps_dlg.is_some() || self.picker.is_some()
    }

    /// Open the Apps dialog: reads the missing apps (font checks run fc-list)
    /// and the size debs Remove set would remove.
    /// PATH is scanned again first, so apps installed outside tetron-wm since
    /// Settings opened count (the row's count follows).
    fn open_apps_dlg(&mut self) {
        use crate::packaging::{self as pk, Size};
        crate::catalog::refresh_installed();
        let size = pk::installed_size();
        let missing = pk::missing_rows_here(Size::Extras);
        self.apps_size = size;
        self.apps_missing = missing.iter().filter(|m| m.size <= size).map(|m| m.name.clone()).collect();
        self.apps_dlg = Some(crate::appsdlg::AppsDlg::new(
            size,
            self.have_curl,
            std::path::Path::new("/usr/bin/apt-get").exists(),
            missing,
            pk::list_has_size(crate::catalog::suite_apps(), Size::Extras),
            pk::remove_debs_here(size),
            pk::kept_scripts_here(size),
        ));
        if let Some(d) = self.apps_dlg.as_mut() {
            d.set_live_free(pk::live_free_bytes());
        }
    }

    fn apps_outcome(&mut self, o: crate::appsdlg::Outcome) {
        use crate::appsdlg::Outcome;
        match o {
            Outcome::None => {}
            Outcome::Close => self.apps_dlg = None,
            Outcome::Install(s, scripts) => {
                self.apps_dlg = None;
                self.action = Some(SettingsAction::InstallApps(s, scripts));
            }
            Outcome::Remove(s) => {
                self.apps_dlg = None;
                self.action = Some(SettingsAction::RemoveApps(s));
            }
            Outcome::Download => self.action = Some(SettingsAction::AppsDownloadSize),
        }
    }

    /// Assistant > Agent: installed agents, then the others (Store), then
    /// Custom command… (WMAST-002).
    fn open_agent_picker(&mut self) {
        use crate::listpicker::{ListPicker, PickItem};
        crate::catalog::refresh_installed();
        let (have, miss): (Vec<_>, Vec<_>) =
            crate::assistant::agents(&self.cfg.assistant_agent).into_iter().partition(|a| a.installed());
        let mut items: Vec<PickItem> =
            have.into_iter().map(|a| PickItem { detail: a.command.clone(), label: a.label, key: a.id }).collect();
        items.extend(miss.into_iter().map(|a| PickItem {
            detail: "not installed: Enter opens the Store".into(),
            label: a.label,
            key: format!("{PICK_INSTALL}{}", a.id),
        }));
        items.push(PickItem { label: "Custom command\u{2026}".into(), detail: "any command line".into(), key: PICK_CUSTOM.into() });
        let cur = crate::assistant::selected(&self.cfg).id;
        self.picker = Some((PickFor::Agent, ListPicker::new("AI \u{203A} Agent", items, &cur)));
    }

    fn pick_outcome(&mut self, o: crate::listpicker::Pick) {
        use crate::listpicker::Pick;
        let Some((what, _)) = self.picker.as_ref() else { return };
        let what = *what;
        match o {
            Pick::None => {}
            Pick::Close => self.picker = None,
            Pick::Chosen(key) => {
                self.picker = None;
                match what {
                    PickFor::Agent => self.choose_agent(&key),
                    PickFor::Cwd => self.choose_cwd(&key),
                }
            }
        }
    }

    /// AI > Working folder: the code folder, ask each launch, the AI folder,
    /// or another folder (WMAST-007).
    fn open_cwd_picker(&mut self) {
        use crate::listpicker::{ListPicker, PickItem};
        let code = crate::assistant::tilde(&crate::assistant::code_dir(&self.cfg));
        let ai = crate::assistant::workdir().map(|d| crate::assistant::tilde(&d)).unwrap_or_default();
        let item = |label: &str, detail: String, key: &str| PickItem { label: label.into(), detail, key: key.into() };
        let items = vec![
            item("Code folder", format!("{code} (default)"), "@code"),
            item("Ask each launch", "pick a folder when the agent starts".into(), "ask"),
            item("AI folder", format!("{ai}, where AGENTS.md is"), "ai"),
            item("Other folder\u{2026}", "type a path".into(), "@other"),
        ];
        let cur = match self.cfg.assistant_cwd.as_deref() {
            None => "@code",
            Some("ask") => "ask",
            Some("ai") => "ai",
            Some(_) => "@other",
        };
        self.picker = Some((PickFor::Cwd, ListPicker::new("AI \u{203A} Working folder", items, cur)));
    }

    fn choose_cwd(&mut self, key: &str) {
        match key {
            "@code" => self.cfg.assistant_cwd = None,
            "@other" => {
                let buf = self.cfg.assistant_cwd.clone().filter(|c| c != "ask" && c != "ai").unwrap_or_default();
                self.text = Some(TextEdit { target: TextTarget::AiCwd, buf, error: false, fresh: false });
            }
            k => self.cfg.assistant_cwd = Some(k.to_string()),
        }
    }

    fn choose_agent(&mut self, key: &str) {
        if key == PICK_CUSTOM {
            let a = crate::assistant::selected(&self.cfg);
            let buf = if a.app.is_none() && a.label.starts_with("custom: ") { a.id } else { String::new() };
            self.text = Some(TextEdit { target: TextTarget::AgentCommand, buf, error: false, fresh: false });
            return;
        }
        let id = key.strip_prefix(PICK_INSTALL).unwrap_or(key);
        self.cfg.assistant_command = Some(id.to_string());
        if key.starts_with(PICK_INSTALL) {
            let app = crate::assistant::selected(&self.cfg).app.unwrap_or_else(|| id.to_string());
            self.action = Some(SettingsAction::OpenStoreFor(app));
        }
    }

    /// The packages whose download size the Apps confirmation shows.
    pub fn apps_download_pkgs(&self) -> Vec<String> {
        self.apps_dlg.as_ref().map(|d| d.download_pkgs()).unwrap_or_default()
    }

    /// The download size computed for the Apps confirmation.
    pub fn set_apps_download(&mut self, est: Option<crate::packaging::AptEstimate>) {
        if let Some(d) = self.apps_dlg.as_mut() {
            d.set_download(est);
        }
    }

    /// Number of interactive rows in the current section.
    fn item_count(&self) -> usize {
        match self.section {
            WINDOWS => 10,                           // snapping, threshold, grid rows/cols, gap, auto-tile, launch-maximized, natural scroll, full screen, waiter service
            APPEARANCE => 9,                            // theme, wallpaper toggle, wallpaper file, icon-scrim, icon style, desktop icons, launcher label, colors, get more themes
            UPDATES => if self.apphost_outdated { 4 } else { 3 }, // check, install, branch [, restart apphost]
            APPS => self.cfg.launcher.len() + 1,  // custom apps + "＋ Add app…"
            DEFAULT_APPS => DEFAULT_APP_ROLES.len(),
            ASSISTANT => 4,                            // AI: open-as mode, agent, working folder, MCP
            CLIPBOARD => 10,                           // clipboard: history, disk, selections, size, shims, osc52 read, fake display, expiry, max item, clear
            TERMINAL => 6,                            // terminal: preset, background, foreground, preview, save, revert
            SOUND => 4,                        // sound: volume, mute, output, mixer
            DISPLAY => self.brightness.is_some() as usize, // brightness, when there is a backlight
            GENERAL => 5,                      // config file: open, create or reset, reload; apps; log level
            _ => 0,                            // About
        }
    }

    /// Tab / Shift+Tab: move keyboard focus between the section list and the
    /// rows. Ignored while a text box or the add form is open.
    pub fn toggle_pane(&mut self) {
        if self.edit.is_none() && self.text.is_none() && self.apps_dlg.is_none() && self.picker.is_none() {
            self.in_sidebar = !self.in_sidebar;
        }
    }

    /// Whether the section list has keyboard focus (for tests).
    pub fn sidebar_focused(&self) -> bool {
        self.in_sidebar
    }

    pub fn move_up(&mut self) {
        self.reset_armed = false;
        if let Some((_, p)) = self.picker.as_mut() {
            p.up();
            return;
        }
        if let Some(d) = self.apps_dlg.as_mut() {
            d.up();
            return;
        }
        if self.in_sidebar {
            if self.section > 0 {
                self.enter_section(self.section - 1);
            }
            return;
        }
        if self.text.is_some() {
            return; // typing a filename — arrows do nothing
        }
        if let Some(e) = self.edit.as_mut() {
            e.field = 0; // focus the Name field
            return;
        }
        self.sel = self.sel.saturating_sub(1);
    }
    pub fn move_down(&mut self) {
        self.reset_armed = false;
        if let Some((_, p)) = self.picker.as_mut() {
            p.down();
            return;
        }
        if let Some(d) = self.apps_dlg.as_mut() {
            d.down();
            return;
        }
        if self.in_sidebar {
            if self.section + 1 < SECTIONS.len() {
                self.enter_section(self.section + 1);
            }
            return;
        }
        if self.text.is_some() {
            return;
        }
        if let Some(e) = self.edit.as_mut() {
            e.field = 1; // focus the Command field
            return;
        }
        if self.sel + 1 < self.item_count() {
            self.sel += 1;
        }
    }
    pub fn prev_section(&mut self) {
        if self.edit.is_some() || self.text.is_some() || self.apps_dlg.is_some() || self.picker.is_some() {
            return; // don't leave a half-typed form via arrow keys
        }
        self.enter_section((self.section + SECTIONS.len() - 1) % SECTIONS.len());
    }
    pub fn next_section(&mut self) {
        if self.edit.is_some() || self.text.is_some() || self.apps_dlg.is_some() || self.picker.is_some() {
            return;
        }
        self.enter_section((self.section + 1) % SECTIONS.len());
    }

    /// Append a character to the active text field (Apps add form, or the
    /// Appearance → Wallpaper filename box).
    pub fn type_char(&mut self, c: char) {
        if let Some((_, p)) = self.picker.as_mut() {
            p.type_char(c);
            return;
        }
        if let Some(d) = self.apps_dlg.as_mut() {
            // Space ticks the box or chooses, like Enter, on the choice list.
            if c == ' ' {
                let o = d.enter();
                self.apps_outcome(o);
            }
            return;
        }
        if let Some(e) = self.edit.as_mut() {
            match e.field {
                0 => e.name.push(c),
                _ => e.command.push(c),
            }
        } else if let Some(t) = self.text.as_mut() {
            // The launcher label is at most 6 characters (so the menubar never
            // shifts); `@session` is the one longer value allowed.
            if t.target == TextTarget::LauncherLabel
                && t.buf.chars().count() >= if t.buf.starts_with('@') { 8 } else { crate::chrome::LABEL_CHARS }
            {
                return;
            }
            if std::mem::take(&mut t.fresh) {
                t.buf.clear();
            }
            t.buf.push(c);
            t.error = false;
        }
    }

    /// Paste `text` into the open text field (the Apps add form or a text
    /// box), one character at a time so the field limits still apply. Line
    /// breaks and other control characters are dropped. Returns false when no
    /// field is open, so the caller can paste elsewhere.
    pub fn paste(&mut self, text: &str) -> bool {
        if self.edit.is_none() && self.text.is_none() {
            return false;
        }
        for c in text.chars().filter(|c| !c.is_control()) {
            self.type_char(c);
        }
        true
    }

    /// Delete the last character of the active text field.
    pub fn backspace(&mut self) {
        if let Some((_, p)) = self.picker.as_mut() {
            p.backspace();
            return;
        }
        if let Some(e) = self.edit.as_mut() {
            match e.field {
                0 => { e.name.pop(); }
                _ => { e.command.pop(); }
            }
        } else if let Some(t) = self.text.as_mut() {
            if std::mem::take(&mut t.fresh) {
                t.buf.clear();
            } else {
                t.buf.pop();
            }
            t.error = false;
        }
    }

    /// Abandon the active text field without saving (Apps add form or the
    /// wallpaper filename box).
    pub fn cancel_edit(&mut self) {
        if let Some((_, p)) = self.picker.as_mut() {
            let o = p.esc();
            self.pick_outcome(o);
            return;
        }
        if let Some(d) = self.apps_dlg.as_mut() {
            let o = d.esc();
            self.apps_outcome(o);
            return;
        }
        self.edit = None;
        self.text = None;
    }

    /// Toggle / activate the selected row.
    pub fn toggle(&mut self) {
        self.adjust(0);
    }
    /// Decrease / turn off / remove the selected row.
    pub fn left(&mut self) {
        self.adjust(-1);
    }
    /// Increase / turn on the selected row.
    pub fn right(&mut self) {
        self.adjust(1);
    }

    /// Apply a change to the selected setting. `dir`: 0 = toggle, -1/+1 = down/up.
    fn adjust(&mut self, dir: i32) {
        // In the section list, Right / Enter open the section (focus the rows);
        // Left has nothing further left to go to.
        if self.in_sidebar {
            if dir >= 0 {
                self.in_sidebar = false;
            }
            return;
        }
        if let Some((_, p)) = self.picker.as_mut() {
            let o = if dir == 0 { p.enter() } else { crate::listpicker::Pick::None };
            self.pick_outcome(o);
            return;
        }
        if let Some(d) = self.apps_dlg.as_mut() {
            let o = if dir == 0 {
                d.enter()
            } else {
                d.side();
                crate::appsdlg::Outcome::None
            };
            self.apps_outcome(o);
            return;
        }
        // While a text box is open, Enter (dir 0) commits it and every other
        // adjust is swallowed so typing cannot change another row.
        if self.text.is_some() {
            if dir == 0 {
                self.commit_text();
            }
            return;
        }
        // Reset needs two Enters in a row: anything else in between disarms it.
        let armed = std::mem::take(&mut self.reset_armed);
        match (self.section, self.sel) {
            (WINDOWS, 0) => self.cfg.snapping_enabled = flip(self.cfg.snapping_enabled, dir),
            (WINDOWS, 1) => {
                self.cfg.snap_threshold = match dir {
                    -1 => (self.cfg.snap_threshold - 1).max(1),
                    1 => (self.cfg.snap_threshold + 1).min(10),
                    // Enter/space on a number wraps 1..=10.
                    _ => if self.cfg.snap_threshold >= 10 { 1 } else { self.cfg.snap_threshold + 1 },
                };
            }
            (WINDOWS, 2) => self.cfg.grid_rows = step_u8(self.cfg.grid_rows, dir, 1, 6),
            (WINDOWS, 3) => self.cfg.grid_cols = step_u8(self.cfg.grid_cols, dir, 1, 6),
            (WINDOWS, 4) => {
                self.cfg.tile_gap = match dir {
                    -1 => (self.cfg.tile_gap - 1).max(0),
                    1 => (self.cfg.tile_gap + 1).min(4),
                    _ => if self.cfg.tile_gap >= 4 { 0 } else { self.cfg.tile_gap + 1 },
                };
            }
            (WINDOWS, 5) => self.cfg.auto_tile = flip(self.cfg.auto_tile, dir),
            (WINDOWS, 6) => self.cfg.launch_maximized = flip(self.cfg.launch_maximized, dir),
            (WINDOWS, 7) => self.cfg.natural_scroll = flip(self.cfg.natural_scroll, dir),
            (WINDOWS, 8) => self.cfg.fullscreen = flip(self.cfg.fullscreen, dir),
            (WINDOWS, 9) if self.waiter != crate::fullscreen::Service::Missing => {
                self.action = Some(SettingsAction::WaiterService(self.waiter != crate::fullscreen::Service::Running));
            }
            (APPEARANCE, 0) => {
                let presets = crate::theme::preset_names();
                let cur_idx = presets.iter().position(|p| p == &self.cfg.theme).unwrap_or(0);
                let next_idx = if dir == -1 {
                    (cur_idx + presets.len() - 1) % presets.len()
                } else {
                    (cur_idx + 1) % presets.len()
                };
                self.cfg.theme = presets[next_idx].clone();
            }
            (APPEARANCE, 1) => self.cfg.wallpaper_enabled = flip(self.cfg.wallpaper_enabled, dir),
            // Wallpaper filename: Enter opens the text box (pre-filled with the
            // current value); ←/→ do nothing (it is free text, not a cycler).
            (APPEARANCE, 2) if dir == 0 => {
                let cur = self.cfg.wallpaper.as_deref().map(str::trim).unwrap_or_default();
                let cur = if cur.is_empty() { crate::wallpaper::DEFAULT_NAME } else { cur };
                let fresh = cur == crate::wallpaper::DEFAULT_NAME;
                self.text = Some(TextEdit { target: TextTarget::Wallpaper, buf: cur.to_string(), error: false, fresh });
            }
            // Icon label scrim on/off: "on" keeps the default (or a configured
            // color); "off" stores "none" so labels sit directly on the wallpaper.
            (APPEARANCE, 3) => {
                self.cfg.desktop_scrim = if scrim_on(&self.cfg) { Some("none".into()) } else { None };
            }
            (APPEARANCE, 4) => {
                use crate::iconset::IconStyle::*;
                let all = [Auto, Ascii, Unicode, Nerd, Emoji];
                let cur = all.iter().position(|x| *x == self.cfg.icon_style).unwrap_or(0);
                let next = if dir == -1 { (cur + all.len() - 1) % all.len() } else { (cur + 1) % all.len() };
                self.cfg.icon_style = all[next];
            }
            (APPEARANCE, 5) => self.cfg.desktop_enabled = flip(self.cfg.desktop_enabled, dir),
            // Launcher label: Enter opens the text box (up to 6 characters, or
            // `@session`); ←/→ do nothing, it is free text.
            (APPEARANCE, 6) if dir == 0 => {
                self.text = Some(TextEdit { target: TextTarget::LauncherLabel, buf: self.cfg.launcher_label.clone(), error: false, fresh: false });
            }
            (APPEARANCE, 7) => {
                let cur = COLOR_CHOICES.iter().position(|(v, _)| crate::display::ColorDepth::parse(v) == crate::display::ColorDepth::parse(&self.cfg.colors)).unwrap_or(0);
                let n = COLOR_CHOICES.len();
                let next = if dir == -1 { (cur + n - 1) % n } else { (cur + 1) % n };
                self.cfg.colors = COLOR_CHOICES[next].0.to_string();
            }
            (APPEARANCE, 8) if dir == 0 && !self.themes_busy => {
                self.themes_busy = true;
                self.themes_status = "Starting".into();
                self.action = Some(SettingsAction::GetThemes);
            }
            // Updates section: Enter/Space (dir 0) requests an action from the session.
            (UPDATES, 0) if dir == 0 => self.action = Some(SettingsAction::CheckUpdates),
            (UPDATES, 1) if dir == 0 => self.action = Some(SettingsAction::InstallUpdate),
            (UPDATES, 3) if dir == 0 && self.apphost_outdated => {
                self.action = Some(SettingsAction::RestartApphost)
            }
            (UPDATES, 2) => {
                let b = crate::config::UPDATE_BRANCHES;
                let cur = b.iter().position(|x| *x == self.cfg.update_branch).unwrap_or(0);
                let next = match dir { -1 => (cur + b.len() - 1) % b.len(), _ => (cur + 1) % b.len() };
                self.cfg.update_branch = b[next].to_string();
            }
            // Apps section.
            (APPS, _) => self.adjust_apps(dir),
            (ASSISTANT, 0) => {
                self.cfg.assistant_mode =
                    if self.cfg.assistant_mode == "panel" { "window".into() } else { "panel".into() };
            }
            (ASSISTANT, 1) if dir == 0 => self.open_agent_picker(),
            (ASSISTANT, 2) if dir == 0 => self.open_cwd_picker(),
            (ASSISTANT, 3) => self.cfg.assistant_mcp = flip(self.cfg.assistant_mcp, dir),
            // Clipboard section.
            (CLIPBOARD, 0) => self.cfg.clipboard_history = flip(self.cfg.clipboard_history, dir),
            (CLIPBOARD, 1) => self.cfg.clipboard_persist = flip(self.cfg.clipboard_persist, dir),
            (CLIPBOARD, 2) => self.cfg.clipboard_record_selection = flip(self.cfg.clipboard_record_selection, dir),
            (CLIPBOARD, 3) => {
                const SIZES: [usize; 7] = [10, 25, 50, 100, 250, 500, 1000];
                let cur = SIZES.iter().position(|s| *s >= self.cfg.clipboard_history_size).unwrap_or(SIZES.len() - 1);
                let next = match dir { -1 => cur.saturating_sub(1), _ => (cur + 1) % SIZES.len() };
                self.cfg.clipboard_history_size = SIZES[next];
            }
            (CLIPBOARD, 4) => self.cfg.clipboard_shims = cycle(&["auto", "on", "off"], &self.cfg.clipboard_shims, dir),
            (CLIPBOARD, 5) => self.cfg.clipboard_osc52_read = cycle(&["focused", "always", "never"], &self.cfg.clipboard_osc52_read, dir),
            (CLIPBOARD, 6) => self.cfg.clipboard_fake_display = flip(self.cfg.clipboard_fake_display, dir),
            (CLIPBOARD, 7) => {
                // 0 = never expire; else drop unpinned entries older than this.
                const TTLS: [u64; 5] = [0, 3_600, 86_400, 604_800, 2_592_000];
                let cur = TTLS.iter().position(|t| *t == self.cfg.clipboard_ttl_secs).unwrap_or(0);
                let next = if dir == -1 { (cur + TTLS.len() - 1) % TTLS.len() } else { (cur + 1) % TTLS.len() };
                self.cfg.clipboard_ttl_secs = TTLS[next];
            }
            (CLIPBOARD, 8) => {
                const KBS: [usize; 5] = [16, 64, 256, 1024, 4096];
                let cur = KBS.iter().position(|k| *k >= self.cfg.clipboard_item_max_kb).unwrap_or(KBS.len() - 1);
                let next = match dir { -1 => cur.saturating_sub(1), _ => (cur + 1) % KBS.len() };
                self.cfg.clipboard_item_max_kb = KBS[next];
            }
            (CLIPBOARD, 9) if dir == 0 => self.action = Some(SettingsAction::ClearClipboard),
            // Terminal section: staged colors; nothing reaches the apphost or disk
            // until Preview / Save.
            (TERMINAL, 0) => {
                let same = |a: &Option<String>, b: &Option<&str>| a.as_deref().map(str::to_lowercase) == b.map(str::to_string);
                let cur = TERM_PRESETS.iter().position(|(_, bg, fg)| same(&self.stage_bg, bg) && same(&self.stage_fg, fg));
                let n = TERM_PRESETS.len();
                let next = match (cur, dir) {
                    (Some(c), -1) => (c + n - 1) % n,
                    (Some(c), _) => (c + 1) % n,
                    (None, -1) => n - 1,
                    (None, _) => 0,
                };
                let (_, bg, fg) = TERM_PRESETS[next];
                self.stage_bg = bg.map(String::from);
                self.stage_fg = fg.map(String::from);
            }
            (TERMINAL, 1) if dir == 0 => {
                self.text = Some(TextEdit { target: TextTarget::TermBg, buf: self.stage_bg.clone().unwrap_or_default(), error: false, fresh: false });
            }
            (TERMINAL, 2) if dir == 0 => {
                self.text = Some(TextEdit { target: TextTarget::TermFg, buf: self.stage_fg.clone().unwrap_or_default(), error: false, fresh: false });
            }
            (TERMINAL, 3) if dir == 0 => {
                self.action = Some(SettingsAction::PreviewColors);
                self.previewing = true;
            }
            (TERMINAL, 4) if dir == 0 => {
                self.cfg.terminal_bg = self.stage_bg.clone();
                self.cfg.terminal_fg = self.stage_fg.clone();
                self.previewing = false;
            }
            (TERMINAL, 5) if dir == 0 => {
                self.stage_bg = self.cfg.terminal_bg.clone();
                self.stage_fg = self.cfg.terminal_fg.clone();
                if self.previewing {
                    self.action = Some(SettingsAction::RevertColors);
                }
                self.previewing = false;
            }
            // Sound section: machine state, applied by the session right away.
            (SOUND, 0) if dir != 0 => {
                self.action = Some(if dir < 0 { SettingsAction::VolumeDown } else { SettingsAction::VolumeUp });
            }
            (SOUND, 1) => {
                let (_, muted, _) = self.sound_now();
                if flip(muted, dir) != muted {
                    self.action = Some(SettingsAction::ToggleMute);
                }
            }
            (SOUND, 2) if !self.outputs.is_empty() => {
                let n = self.outputs.len();
                let cur = self.outputs.iter().position(|o| o.default).unwrap_or(0);
                let next = if dir < 0 { (cur + n - 1) % n } else { (cur + 1) % n };
                // Show the switch at once; the refresh after it confirms.
                for (i, o) in self.outputs.iter_mut().enumerate() {
                    o.default = i == next;
                }
                self.action = Some(SettingsAction::SetOutput(self.outputs[next].id));
            }
            (SOUND, 3) if dir == 0 => self.action = Some(SettingsAction::OpenMixer),
            (DISPLAY, 0) if dir != 0 => {
                if let Some(p) = self.brightness {
                    let next = crate::brightness::step(p, dir > 0);
                    self.brightness = Some(next);
                    self.action = Some(SettingsAction::BrightnessSet(next));
                }
            }
            (GENERAL, 0) if dir == 0 => self.action = Some(SettingsAction::OpenConfigFile),
            (GENERAL, 1) if dir == 0 && !self.cfg_exists => self.action = Some(SettingsAction::CreateConfigFile),
            (GENERAL, 1) if dir == 0 && armed => self.action = Some(SettingsAction::ResetConfigFile),
            (GENERAL, 1) if dir == 0 => self.reset_armed = true,
            (GENERAL, 2) if dir == 0 => self.action = Some(SettingsAction::ReloadConfig),
            (GENERAL, 3) if dir == 0 => self.open_apps_dlg(),
            (GENERAL, 4) => {
                use crate::logfile::Level;
                let n = Level::ALL.len();
                let cur = self.cfg.log_level as usize;
                self.cfg.log_level = Level::ALL[if dir == -1 { (cur + n - 1) % n } else { (cur + 1) % n }];
            }
            (DEFAULT_APPS, i) => {
                if let Some((key, _)) = DEFAULT_APP_ROLES.get(i) {
                    let role = role_from_key(key);
                    let cands = crate::openwith::candidates(role);
                    let cur = self.cfg.default_apps.get(*key).cloned().unwrap_or_default();
                    let idx = cands.iter().position(|c| c == &cur).unwrap_or(0);
                    let next = match dir {
                        -1 => (idx + cands.len() - 1) % cands.len(),
                        _ => (idx + 1) % cands.len(),
                    };
                    let val = cands[next].clone();
                    if val.is_empty() {
                        self.cfg.default_apps.remove(*key);
                    } else {
                        self.cfg.default_apps.insert((*key).to_string(), val);
                    }
                }
            }
            _ => {}
        }
    }

    /// Apps section: Enter on the form commits it; Enter on "＋ Add app…" opens
    /// the form; Left removes the selected custom app.
    fn adjust_apps(&mut self, dir: i32) {
        if self.edit.is_some() {
            if dir == 0 {
                self.commit_edit();
            }
            return;
        }
        let n = self.cfg.launcher.len();
        if dir == 0 {
            if self.sel == n {
                self.edit = Some(AppEdit::default()); // "＋ Add app…" row
            }
        } else if dir == -1 && self.sel < n {
            self.cfg.launcher.remove(self.sel);
            self.sel = self.sel.min(self.item_count().saturating_sub(1));
        }
    }

    /// Validate and save the add form into `cfg.launcher`. Empty fields just move
    /// focus to the offending field instead of committing.
    fn commit_edit(&mut self) {
        let (name, command) = match self.edit.as_ref() {
            Some(e) => (e.name.trim().to_string(), e.command.trim().to_string()),
            None => return,
        };
        if name.is_empty() {
            self.edit.as_mut().unwrap().field = 0;
            return;
        }
        if command.is_empty() {
            self.edit.as_mut().unwrap().field = 1;
            return;
        }
        // Split the command line into program + args on whitespace.
        let mut parts = command.split_whitespace().map(String::from);
        let program = parts.next().unwrap();
        let args: Vec<String> = parts.collect();
        self.cfg.launcher.push(AppEntry {
            name,
            command: program,
            args,
            category: Some("Custom".into()),
            requires_cwd: None,
            cwd: None,
            cli: None,
            warn: None,
        });
        self.edit = None;
        self.sel = self.cfg.launcher.len(); // park on the "＋ Add app…" row
    }

    /// Commit the open text box. The wallpaper name is saved into `cfg` (empty =
    /// none). A terminal color goes to the staged value (empty = follow the
    /// theme); an invalid color keeps the box open with an error note.
    fn commit_text(&mut self) {
        let Some(mut e) = self.text.take() else { return };
        let s = e.buf.trim().to_string();
        match e.target {
            // Empty becomes `default` (the built-in image), so the field
            // always shows a name that can be typed back. Pasted quotes,
            // `file://` and `$HOME` are cleaned before saving.
            TextTarget::Wallpaper => {
                let s = crate::wallpaper::clean_spec(&s);
                self.cfg.wallpaper = Some(if s.is_empty() { crate::wallpaper::DEFAULT_NAME.to_string() } else { s });
                self.wallpaper_note = crate::wallpaper::note(self.cfg.wallpaper.as_deref());
            }
            // Empty = the default; otherwise at most 6 characters (padding and
            // `@session` are resolved when drawn, not stored).
            TextTarget::LauncherLabel => {
                self.cfg.launcher_label = if s == "@session" {
                    s
                } else if s.is_empty() {
                    crate::chrome::DEFAULT_LABEL.to_string()
                } else {
                    s.chars().filter(|c| !c.is_control()).take(crate::chrome::LABEL_CHARS).collect()
                };
            }
            // Empty goes back to the default agent.
            TextTarget::AgentCommand => {
                self.cfg.assistant_command = (!s.is_empty()).then_some(s);
            }
            // Empty goes back to the code folder.
            TextTarget::AiCwd => {
                self.cfg.assistant_cwd = (!s.is_empty()).then_some(s);
            }
            TextTarget::TermBg | TextTarget::TermFg => {
                let val = if s.is_empty() {
                    None
                } else if crate::badge::parse_color(&s).is_some() {
                    Some(s)
                } else {
                    e.error = true;
                    self.text = Some(e);
                    return;
                };
                if e.target == TextTarget::TermBg {
                    self.stage_bg = val;
                } else {
                    self.stage_fg = val;
                }
            }
        }
    }

    /// Handle a content-local click; returns `true` if a setting changed.
    pub fn handle_click(&mut self, p: Point, _w: i32, h: i32) -> bool {
        if let Some((_, pk)) = self.picker.as_mut() {
            let o = pk.click(p.x - SIDEBAR_W - 2, p.y, h);
            self.pick_outcome(o);
            return true;
        }
        if let Some(d) = self.apps_dlg.as_mut() {
            let o = d.click(p.x - SIDEBAR_W - 2, p.y, h);
            self.apps_outcome(o);
            return self.action.is_some();
        }
        // A click while a text box is open dismisses it. Mouse-driven users
        // otherwise had no way out — the click was swallowed and every row went
        // dead, so the panel looked frozen (most visibly the Wallpaper filename
        // box). A click commits the wallpaper name (like clicking away to
        // confirm) and cancels the multi-field Apps form (a half-typed command
        // should not be saved). Either way this click only dismisses; it does
        // not also action the row under it, so nothing changes by surprise.
        if let Some(t) = self.text.as_ref() {
            // The wallpaper name commits on click-away; a half-typed color is
            // discarded instead (committing it would stage a wrong color).
            if matches!(t.target, TextTarget::Wallpaper | TextTarget::LauncherLabel | TextTarget::AgentCommand | TextTarget::AiCwd) {
                self.commit_text();
                return true;
            }
            self.text = None;
            return false;
        }
        if self.edit.is_some() {
            self.edit = None;
            return false;
        }
        if p.x < SIDEBAR_W {
            let i = (p.y - 1) as usize;
            if i < SECTIONS.len() {
                self.enter_section(i);
                self.in_sidebar = true;
            }
            // `true` only when an action (the Sound refresh) needs draining.
            return self.action.is_some();
        }
        self.in_sidebar = false;
        let row = p.y - 3;
        // Sound volume row: ◂ steps down, ▸ steps up, a click on the bar sets
        // the level there (cell k of 10 = (k + 1) * 10 %).
        if self.section == SOUND && row == 0 {
            self.sel = 0;
            let vx = SIDEBAR_W + 2 + 30;
            self.action = match p.x - vx {
                x if x < 2 => Some(SettingsAction::VolumeDown),
                x if x < 12 => Some(SettingsAction::VolumeSet(((x - 1) * 10) as u8)),
                _ => Some(SettingsAction::VolumeUp),
            };
            return true;
        }
        if row >= 0 && (row as usize) < self.item_count() {
            self.sel = row as usize;
            self.toggle();
            return true;
        }
        false
    }

    /// Render the panel into a `w × h` content buffer.
    pub fn render(&self, w: i32, h: i32) -> CellBuffer {
        let mut buf = CellBuffer::new(w, h);
        buf.fill(Cell { ch: ' ', fg: pal_fg(), bg: pal_bg(), attrs: Default::default() });

        // Sidebar.
        for (i, s) in SECTIONS.iter().enumerate() {
            let y = 1 + i as i32;
            let sel = i == self.section;
            // The current section is always highlighted; the ▸ marker shows
            // when the list itself has keyboard focus.
            let (fg, bg) = if sel { (pal_accent(), pal_sel()) } else { (pal_dim(), pal_bg()) };
            for x in 0..SIDEBAR_W {
                buf.set(x, y, Cell { ch: ' ', fg, bg, attrs: Default::default() });
            }
            if sel && self.in_sidebar {
                buf.write_str(0, y, "\u{25B8}", fg, bg);
            }
            buf.write_str(2, y, s, fg, bg);
        }
        for y in 0..h {
            buf.set(SIDEBAR_W, y, Cell { ch: '\u{2502}', fg: pal_dim(), bg: pal_bg(), attrs: Default::default() });
        }
        // Keyboard hint for the section list, at the sidebar's foot (below the
        // list, so it never covers a section name).
        let foot = 1 + SECTIONS.len() as i32 + 1;
        if h - 2 >= foot {
            let hint = if self.in_sidebar { "Tab: settings" } else { "Tab: sections" };
            buf.write_str(1, h - 2, hint, pal_dim(), pal_bg());
            buf.write_str(1, h - 1, "\u{2191}\u{2193}: move", pal_dim(), pal_bg());
        }

        let cx = SIDEBAR_W + 2;
        buf.write_str(cx, 1, SECTIONS[self.section], pal_accent(), pal_bg());

        match self.section {
            WINDOWS => {
                self.row(&mut buf, cx, 3, 0, "Drag-to-cell snapping", toggle_val(self.cfg.snapping_enabled));
                self.row(&mut buf, cx, 4, 1, "Snap threshold (cells)", format!("\u{25C2} {} \u{25B8}", self.cfg.snap_threshold));
                self.row(&mut buf, cx, 5, 2, "Grid rows", format!("\u{25C2} {} \u{25B8}", self.cfg.grid_rows));
                self.row(&mut buf, cx, 6, 3, "Grid columns", format!("\u{25C2} {} \u{25B8}", self.cfg.grid_cols));
                self.row(&mut buf, cx, 7, 4, "Tile gap (cells)", format!("\u{25C2} {} \u{25B8}", self.cfg.tile_gap));
                self.row(&mut buf, cx, 8, 5, "Auto-tile windows", toggle_val(self.cfg.auto_tile));
                self.row(&mut buf, cx, 9, 6, "Launch maximized", toggle_val(self.cfg.launch_maximized));
                self.row(&mut buf, cx, 10, 7, "Natural scroll", toggle_val(self.cfg.natural_scroll));
                self.row(&mut buf, cx, 11, 8, "Full screen video/images", toggle_val(self.cfg.fullscreen));
                let waiter = match self.waiter {
                    crate::fullscreen::Service::Missing => "not installed",
                    crate::fullscreen::Service::Stopped => "stopped (Enter starts, sudo)",
                    crate::fullscreen::Service::Running => "running (Enter stops, sudo)",
                };
                self.row(&mut buf, cx, 12, 9, "Full screen helper service", waiter.into());
            }
            APPEARANCE => {
                self.row(&mut buf, cx, 3, 0, "Theme", self.cfg.theme.clone());
                self.row(&mut buf, cx, 4, 1, "Wallpaper", toggle_val(self.cfg.wallpaper_enabled));
                // Filename text box. While editing, show the live buffer with a
                // cursor; otherwise the saved value (or a placeholder).
                let file_val = match self.text.as_ref().filter(|t| t.target == TextTarget::Wallpaper) {
                    Some(t) => format!("{}\u{2588}", t.buf), // block cursor while typing
                    None => match self.cfg.wallpaper.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                        Some(p) => p.to_string(),
                        None => crate::wallpaper::DEFAULT_NAME.to_string(),
                    },
                };
                self.row(&mut buf, cx, 5, 2, "Wallpaper file", file_val);
                self.row(&mut buf, cx, 6, 3, "Icon label scrim", toggle_val(scrim_on(&self.cfg)));
                self.row(&mut buf, cx, 7, 4, "Icon style", format!("\u{25C2} {} \u{25B8}", icon_style_name(self.cfg.icon_style)));
                self.row(&mut buf, cx, 8, 5, "Desktop icons", toggle_val(self.cfg.desktop_enabled));
                let label_val = match self.text.as_ref().filter(|t| t.target == TextTarget::LauncherLabel) {
                    Some(t) => format!("{}\u{2588}", t.buf), // block cursor while typing
                    None => self.cfg.launcher_label.clone(),
                };
                self.row(&mut buf, cx, 9, 6, "Launcher label", label_val);
                self.row(&mut buf, cx, 10, 7, "Colors", format!("\u{25C2} {} \u{25B8}", color_choice_name(&self.cfg.colors)));
                let themes_val = if self.themes_status.is_empty() { "Enter (downloads from GitHub)".to_string() } else { self.themes_status.clone() };
                self.row(&mut buf, cx, 11, 8, "Get more themes", themes_val);
                // A wallpaper problem goes on the spacer line under the rows,
                // in the accent colour so it is not mistaken for a hint.
                let theme_note = (!crate::theme::known(&self.cfg.theme)).then(|| format!("Theme '{}' not found, using midnight.", self.cfg.theme));
                if let Some(n) = self.wallpaper_note.as_ref().or(theme_note.as_ref()) {
                    buf.write_str(cx, 12, n, pal_accent(), pal_bg());
                }
                buf.write_str(cx, 13, "Wallpaper: default (follows the theme), black, #rrggbb,", pal_dim(), pal_bg());
                buf.write_str(cx, 14, "none, a file in ~/.config/tetron-wm/themes/, or a full path.", pal_dim(), pal_bg());
                buf.write_str(cx, 15, "Scrim: dark backing behind icon labels over a wallpaper.", pal_dim(), pal_bg());
                buf.write_str(cx, 16, "Icon style: auto | ascii | unicode | nerd | emoji.", pal_dim(), pal_bg());
                buf.write_str(cx, 17, "Launcher label: up to 6 characters, or @session for the session name.", pal_dim(), pal_bg());
                buf.write_str(cx, 18, "Colors: auto follows the terminal; mono is no color; TETRON_WM_COLORS wins.", pal_dim(), pal_bg());
                buf.write_str(cx, 19, "New themes are saved in ~/.local/share/tetron-wm/themes/.", pal_dim(), pal_bg());
            }
            UPDATES => {
                self.row(&mut buf, cx, 3, 0, "Check for updates", String::new());
                self.row(&mut buf, cx, 4, 1, "Update & Reload", String::new());
                self.row(&mut buf, cx, 5, 2, "Channel", format!("\u{25C2} {} \u{25B8}", self.cfg.update_branch));
                let sha = &crate::GIT_SHA[..crate::GIT_SHA.len().min(7)];
                let by = if crate::packaging::running_is_diverted() {
                    ", own build"
                } else if crate::packaging::running_is_packaged() {
                    ", by apt"
                } else {
                    ""
                };
                buf.write_str(cx, 7, &format!("installed: v{} ({}){by}", crate::VERSION, sha), pal_dim(), pal_bg());
                if !self.update_status.is_empty() {
                    let col = if self.update_status.contains("available") { green() } else { pal_dim() };
                    buf.write_str(cx, 8, &self.update_status, col, pal_bg());
                }
                if self.cfg.update_branch != "main" && crate::packaging::running_is_packaged() {
                    buf.write_str(cx, 9, "dev channel: not used on an apt install (use main).", pal_dim(), pal_bg());
                } else if self.cfg.update_branch != "main" {
                    buf.write_str(cx, 9, "dev channel: builds from source (slower).", pal_dim(), pal_bg());
                }
                if self.apphost_outdated {
                    self.row(&mut buf, cx, 6, 3, "Restart app server", "(closes apps)".into());
                    buf.write_str(cx, 10, "The app server predates this update; some features", pal_dim(), pal_bg());
                    buf.write_str(cx, 11, "won't work until it restarts (your apps will close).", pal_dim(), pal_bg());
                }
            }
            APPS => self.render_apps(&mut buf, cx, w),
            ASSISTANT if self.picker.is_some() => {
                for x in cx..w {
                    buf.set(x, 1, Cell { ch: ' ', fg: pal_fg(), bg: pal_bg(), attrs: Default::default() });
                }
                if let Some((_, p)) = self.picker.as_ref() {
                    p.render(&mut buf, cx, w, h);
                }
            }
            ASSISTANT => {
                let a = crate::assistant::selected(&self.cfg);
                let agent_val = match self.text.as_ref().filter(|t| t.target == TextTarget::AgentCommand) {
                    Some(t) => format!("{}\u{2588}", t.buf),
                    None if a.installed() => format!("{} \u{25BE}", a.label),
                    None => format!("{} (not installed) \u{25BE}", a.label),
                };
                let cwd_val = match self.text.as_ref().filter(|t| t.target == TextTarget::AiCwd) {
                    Some(t) => format!("{}\u{2588}", t.buf),
                    None => format!("{} \u{25BE}", crate::assistant::cwd_label(&self.cfg)),
                };
                let mcp_val = match (self.cfg.assistant_mcp, a.mcp.is_some()) {
                    (true, false) => "on (this agent has no MCP entry)".to_string(),
                    (on, _) => toggle_val(on),
                };
                self.row(&mut buf, cx, 3, 0, "Open as", format!("\u{25C2} {} \u{25B8}", self.cfg.assistant_mode));
                self.row(&mut buf, cx, 4, 1, "Agent", agent_val);
                self.row(&mut buf, cx, 5, 2, "Working folder", cwd_val);
                self.row(&mut buf, cx, 6, 3, "Desktop tools (MCP)", mcp_val);
                let away = crate::assistant::cwd_choice(&self.cfg) != crate::assistant::Cwd::Ai;
                if away && a.briefing.is_none() {
                    buf.write_str(cx, 7, &format!("{} reads the briefing only in the AI folder.", a.label), pal_accent(), pal_bg());
                }
                buf.write_str(cx, 8, &format!("The {} menubar button opens the AI agent.", crate::glyphs::get(crate::glyphs::Glyph::Assistant)), pal_dim(), pal_bg());
                buf.write_str(cx, 9, "Enter on Agent or Working folder lists the choices;", pal_dim(), pal_bg());
                buf.write_str(cx, 10, "more agents: [[assistant_agent]] in config.toml.", pal_dim(), pal_bg());
                buf.write_str(cx, 11, "MCP: the agent starts tetron-wm mcp itself.", pal_dim(), pal_bg());
            }
            DEFAULT_APPS => {
                for (i, (key, label)) in DEFAULT_APP_ROLES.iter().enumerate() {
                    let val = self.cfg.default_apps.get(*key).cloned().unwrap_or_else(|| "(ask)".into());
                    let shown = match val.as_str() { "@image" => "image viewer".into(), "@navigate" => "open folder".into(), v => v.to_string() };
                    self.row(&mut buf, cx, 3 + i as i32, i, label, format!("\u{25C2} {} \u{25B8}", shown));
                }
            }
            CLIPBOARD => {
                let c = &self.cfg;
                self.row(&mut buf, cx, 3, 0, "Keep history", toggle_val(c.clipboard_history));
                self.row(&mut buf, cx, 4, 1, "Save to disk", toggle_val(c.clipboard_persist));
                self.row(&mut buf, cx, 5, 2, "Record selections", toggle_val(c.clipboard_record_selection));
                self.row(&mut buf, cx, 6, 3, "History size", format!("\u{25C2} {} \u{25B8}", c.clipboard_history_size));
                self.row(&mut buf, cx, 7, 4, "xclip/wl-copy shims", format!("\u{25C2} {} \u{25B8}", c.clipboard_shims));
                self.row(&mut buf, cx, 8, 5, "OSC 52 reads", format!("\u{25C2} {} \u{25B8}", c.clipboard_osc52_read));
                self.row(&mut buf, cx, 9, 6, "Display for Neovim", toggle_val(c.clipboard_fake_display));
                self.row(&mut buf, cx, 10, 7, "Expire entries after", format!("\u{25C2} {} \u{25B8}", ttl_name(c.clipboard_ttl_secs)));
                self.row(&mut buf, cx, 11, 8, "Max item size", format!("\u{25C2} {} KB \u{25B8}", c.clipboard_item_max_kb));
                self.row(&mut buf, cx, 12, 9, "Clear history", "Enter".into());
                buf.write_str(cx, 14, "Open it from the menubar clipboard segment or leader + C.", pal_dim(), pal_bg());
                buf.write_str(cx, 15, "History file: ~/.local/share/tetron-wm/clipboard-history.jsonl", pal_dim(), pal_bg());
                buf.write_str(cx, 16, "Shims apply to newly launched apps after an app server restart.", pal_dim(), pal_bg());
                buf.write_str(cx, 17, "More: clipboard_ignore_apps in config.toml.", pal_dim(), pal_bg());
            }
            TERMINAL => self.render_terminal(&mut buf, cx),
            SOUND => self.render_sound(&mut buf, cx),
            DISPLAY => self.render_display(&mut buf, cx),
            GENERAL => match self.apps_dlg.as_ref() {
                Some(d) => {
                    // The dialog draws its own title over the section's.
                    for x in cx..w {
                        buf.set(x, 1, Cell { ch: ' ', fg: pal_fg(), bg: pal_bg(), attrs: Default::default() });
                    }
                    d.render(&mut buf, cx, w, h);
                }
                None => self.render_general(&mut buf, cx),
            },
            _ => {
                buf.write_str(cx, 3, "tetron-wm — a desktop environment for the terminal", pal_fg(), pal_bg());
                let sha = &crate::GIT_SHA[..crate::GIT_SHA.len().min(7)];
                let facts = [
                    ("Version", format!("{} (git {sha})", crate::VERSION)),
                    ("Session", crate::protocol::session_name()),
                    ("App server", format!("protocol v{}", crate::apphost::proto::PROTO_VERSION)),
                    ("Installed", self.install_info.clone()),
                    ("Config", "~/.config/tetron-wm/config.toml".to_string()),
                    ("Log", crate::logfile::display_path()),
                    ("Clipboard", "~/.local/share/tetron-wm/clipboard-history.jsonl".to_string()),
                ];
                for (i, (k, v)) in facts.iter().enumerate() {
                    buf.write_str(cx, 5 + i as i32, &format!("{k:<12}{v}"), pal_fg(), pal_bg());
                }
                buf.write_str(cx, 13, "Shortcuts: leader ?   Info menu: leader i", pal_dim(), pal_bg());
                buf.write_str(cx, 14, "github.com/tetronos/tetron-wm", pal_dim(), pal_bg());
            }
        }
        buf
    }

    /// General > Apps value: the installed size and the chosen action.
    fn apps_value(&self) -> String {
        let have = match self.apps_missing.len() {
            0 => self.apps_size.name().to_string(),
            n => format!("{}, {n} missing", self.apps_size.name()),
        };
        format!("{have}: Enter for the choices")
    }

    /// Render the General section: the config file group.
    fn render_display(&self, buf: &mut CellBuffer, cx: i32) {
        match self.brightness {
            Some(p) => {
                self.row(buf, cx, 3, 0, "Brightness", format!("\u{25C2} {p}% \u{25B8}"));
                buf.write_str(cx, 5, "Left / Right change it in 5% steps; leader - opens", pal_dim(), pal_bg());
                buf.write_str(cx, 6, "the same control from anywhere.", pal_dim(), pal_bg());
            }
            None => {
                buf.write_str(cx, 3, "No backlight on this machine (an external monitor", pal_fg(), pal_bg());
                buf.write_str(cx, 4, "or a virtual machine): use the monitor's own buttons.", pal_fg(), pal_bg());
            }
        }
    }

    fn render_general(&self, buf: &mut CellBuffer, cx: i32) {
        let path = crate::config::file_path().map(|p| p.display().to_string()).unwrap_or_else(|_| "(no home directory)".into());
        let state = if self.cfg_exists { "" } else { "  (not created yet, defaults in use)" };
        // Rows start at y 3 like every section (clicks map y - 3 to the row).
        self.row(buf, cx, 3, 0, "Open in editor", "Enter".into());
        if self.cfg_exists {
            let v = if self.reset_armed { "Enter again to reset" } else { "Enter (keeps config.toml.bak)" };
            self.row(buf, cx, 4, 1, "Reset to defaults", v.into());
        } else {
            self.row(buf, cx, 4, 1, "Create from example", "Enter".into());
        }
        self.row(buf, cx, 5, 2, "Reload", "Enter (applies hand edits)".into());
        self.row(buf, cx, 6, 3, "Apps", self.apps_value());
        self.row(buf, cx, 7, 4, "Log level", format!("\u{25C2} {} \u{25B8}", self.cfg.log_level.name()));
        buf.write_str(cx, 9, &format!("Config file: {path}{state}"), pal_fg(), pal_bg());
        buf.write_str(cx, 10, &format!("Log: {}", crate::logfile::display_path()), pal_fg(), pal_bg());
        buf.write_str(cx, 12, "Open in editor uses the Editor in Default Apps.", pal_dim(), pal_bg());
        buf.write_str(cx, 13, "Reset and Create write the example: every key at its default.", pal_dim(), pal_bg());
        buf.write_str(cx, 14, "Same as: tetron-wm config init | config example | reload", pal_dim(), pal_bg());
        buf.write_str(cx, 15, "Apps: install Essentials or Recommended, or remove a set;", pal_dim(), pal_bg());
        buf.write_str(cx, 16, "each choice is confirmed first. sudo asks in a window.", pal_dim(), pal_bg());
        buf.write_str(cx, 17, "Log level: info logs events, debug every message.", pal_dim(), pal_bg());
    }

    /// Render the Terminal section: staged colors with swatches, the
    /// Preview / Save / Revert rows, and status or warning notes.
    fn render_terminal(&self, buf: &mut CellBuffer, cx: i32) {
        let same = |a: &Option<String>, b: &Option<&str>| a.as_deref().map(str::to_lowercase) == b.map(str::to_string);
        let preset = TERM_PRESETS
            .iter()
            .find(|(_, bg, fg)| same(&self.stage_bg, bg) && same(&self.stage_fg, fg))
            .map(|(n, _, _)| *n)
            .unwrap_or("custom");
        self.row(buf, cx, 3, 0, "Colors preset", format!("\u{25C2} {preset} \u{25B8}"));
        let shown = |target: TextTarget, staged: &Option<String>| match self.text.as_ref().filter(|t| t.target == target) {
            Some(t) => format!("{}\u{2588}", t.buf),
            None => staged.clone().unwrap_or_else(|| "(theme)".into()),
        };
        let th = crate::theme::current();
        let bg_val = shown(TextTarget::TermBg, &self.stage_bg);
        let fg_val = shown(TextTarget::TermFg, &self.stage_fg);
        self.row(buf, cx, 4, 1, "Background", bg_val.clone());
        self.row(buf, cx, 5, 2, "Foreground", fg_val.clone());
        let bg_rgb = self.stage_bg.as_deref().and_then(crate::badge::parse_color);
        let fg_rgb = self.stage_fg.as_deref().and_then(crate::badge::parse_color);
        // Swatches: the color itself where set, else the theme default it falls back to.
        buf.write_str(cx + 30 + bg_val.chars().count() as i32 + 1, 4, "\u{25A0}", bg_rgb.unwrap_or(th.window_bg), pal_bg());
        buf.write_str(cx + 30 + fg_val.chars().count() as i32 + 1, 5, "\u{25A0}", fg_rgb.unwrap_or(th.text), pal_bg());
        let dirty = self.stage_bg != self.cfg.terminal_bg || self.stage_fg != self.cfg.terminal_fg;
        self.row(buf, cx, 6, 3, "Preview (not saved)", "Enter".into());
        self.row(buf, cx, 7, 4, "Save", if dirty { "Enter (unsaved)".into() } else { "Enter".into() });
        self.row(buf, cx, 8, 5, "Revert", "Enter".into());
        let mut y = 10;
        let mut note = |buf: &mut CellBuffer, text: &str, col: Rgba| {
            buf.write_str(cx, y, text, col, pal_bg());
            y += 1;
        };
        if self.previewing {
            note(buf, "Preview is live. Save keeps it, Revert undoes it.", green());
        } else if dirty {
            note(buf, "Edited, not applied yet. Preview to see it.", pal_dim());
        }
        if crate::theme::contrast(bg_rgb.unwrap_or(th.window_bg), fg_rgb.unwrap_or(th.text)) < 3.0 {
            note(buf, "Low contrast: terminal text may be hard to read.", red());
        }
        if self.text.as_ref().is_some_and(|t| t.error) {
            note(buf, "Not a color: use #rrggbb or a color name.", red());
        }
        if !self.colors_live {
            note(buf, "App server predates live colors: restart it to apply.", pal_dim());
        }
        note(buf, "Empty value = follow the desktop theme.", pal_dim());
        note(buf, "Running terminals repaint on Preview; no restart.", pal_dim());
    }

    /// Render the Sound section: volume bar, mute, output picker, mixer link,
    /// and the note that these rows act on the machine immediately.
    fn render_sound(&self, buf: &mut CellBuffer, cx: i32) {
        let (level, muted, available) = self.sound_now();
        let filled = (level as usize).div_ceil(10).min(10);
        let bar: String = (0..10)
            .map(|i| if i < filled { crate::glyphs::ch(crate::glyphs::Glyph::BarOn) } else { crate::glyphs::ch(crate::glyphs::Glyph::BarOff) })
            .collect();
        self.row(buf, cx, 3, 0, "Output volume", format!("\u{25C2} {bar} \u{25B8} {level}%"));
        self.row(buf, cx, 4, 1, "Mute", toggle_val(muted));
        let output = match self.outputs.iter().find(|o| o.default) {
            Some(o) => format!("\u{25C2} {} \u{25B8}", truncate(&o.name, 30)),
            None if self.outputs.is_empty() => "(none found)".into(),
            None => "\u{25C2} (unknown) \u{25B8}".into(),
        };
        self.row(buf, cx, 5, 2, "Output device", output);
        let mixer = crate::tray::volume_mixer_now(&self.cfg.tray_tools).map(|(l, _)| l).unwrap_or_else(|| "(none)".into());
        self.row(buf, cx, 6, 3, "Mixer", mixer);
        let mut y = 8;
        if !available {
            buf.write_str(cx, y, "No volume control: wpctl (PipeWire) not found.", red(), pal_bg());
            y += 1;
        }
        for note in [
            "These apply now and are not saved to config.toml.",
            "They act on this machine (over ssh: the remote one).",
            "Left/Right change a row, Space mutes, Enter opens the mixer.",
            "Leader = opens the same controls from the menubar.",
        ] {
            buf.write_str(cx, y, note, pal_dim(), pal_bg());
            y += 1;
        }
    }

    /// Render the Apps section: either the add form or the custom-app list.
    fn render_apps(&self, buf: &mut CellBuffer, cx: i32, w: i32) {
        if let Some(e) = self.edit.as_ref() {
            buf.write_str(cx, 3, "Add a custom app", pal_fg(), pal_bg());
            self.field(buf, cx, 5, "Name", &e.name, e.field == 0);
            self.field(buf, cx, 6, "Command", &e.command, e.field == 1);
            buf.write_str(cx, 8, "Enter save \u{00B7} \u{2191}\u{2193} switch field \u{00B7} Esc cancel", pal_dim(), pal_bg());
            return;
        }

        if self.cfg.launcher.is_empty() {
            buf.write_str(cx, 3, "No custom apps yet.", pal_dim(), pal_bg());
        }
        for (i, a) in self.cfg.launcher.iter().enumerate() {
            let y = 3 + i as i32;
            let sel = i == self.sel && !self.in_sidebar;
            let marker = if sel { "\u{25B8} " } else { "  " };
            buf.write_str(cx, y, marker, pal_accent(), pal_bg());
            buf.write_str(cx + 2, y, &a.name, if sel { pal_fg() } else { pal_dim() }, pal_bg());
            let cmd = command_line(a);
            let cmd_x = cx + 16;
            let max = (w - cmd_x - 12).max(4) as usize;
            buf.write_str(cmd_x, y, &truncate(&cmd, max), pal_dim(), pal_bg());
            if sel {
                buf.write_str(w - 12, y, "\u{2190} remove", pal_dim(), pal_bg());
            }
        }
        // "＋ Add app…" row.
        let add_y = 3 + self.cfg.launcher.len() as i32;
        let add_sel = self.sel == self.cfg.launcher.len() && !self.in_sidebar;
        let marker = if add_sel { "\u{25B8} " } else { "  " };
        buf.write_str(cx, add_y, marker, pal_accent(), pal_bg());
        buf.write_str(cx + 2, add_y, &format!("{} Add app\u{2026}", crate::glyphs::get(crate::glyphs::Glyph::Plus)), if add_sel { green() } else { pal_dim() }, pal_bg());
    }

    /// Draw one labelled form field with a block cursor on the focused field.
    fn field(&self, buf: &mut CellBuffer, x: i32, y: i32, label: &str, value: &str, focused: bool) {
        let lcol = if focused { pal_accent() } else { pal_dim() };
        buf.write_str(x, y, &format!("{label}:"), lcol, pal_bg());
        let vx = x + 10;
        let shown = if focused { format!("{value}\u{2588}") } else { value.to_string() };
        buf.write_str(vx, y, &shown, pal_fg(), pal_bg());
    }

    fn row(&self, buf: &mut CellBuffer, x: i32, y: i32, idx: usize, label: &str, value: String) {
        let sel = idx == self.sel && self.item_count() > 0 && !self.in_sidebar;
        let marker = if sel { "\u{25B8} " } else { "  " };
        buf.write_str(x, y, marker, pal_accent(), pal_bg());
        buf.write_str(x + 2, y, label, if sel { pal_fg() } else { pal_dim() }, pal_bg());
        let vx = x + 30;
        let vcol = if value.contains("on") { green() } else { pal_fg() };
        buf.write_str(vx, y, &value, vcol, pal_bg());
    }
}

fn role_from_key(key: &str) -> crate::openwith::Role {
    use crate::openwith::Role::*;
    match key {
        "image" => Image, "video" => Video, "audio" => Audio, "text" => Text,
        "code" => Code, "archive" => Archive, "pdf" => Pdf, _ => Other,
    }
}

/// Step through `options` (wrapping); unknown current value starts at the first.
fn cycle(options: &[&str], current: &str, dir: i32) -> String {
    let cur = options.iter().position(|o| *o == current).unwrap_or(0);
    let next = if dir == -1 { (cur + options.len() - 1) % options.len() } else { (cur + 1) % options.len() };
    options[next].to_string()
}

fn flip(current: bool, dir: i32) -> bool {
    match dir {
        0 => !current,
        d => d > 0,
    }
}

/// Step a `u8` setting within `[lo, hi]`. `dir` -1/+1 decrements/increments;
/// 0 (Enter/Space) wraps up to `lo` after `hi`.
fn step_u8(v: u8, dir: i32, lo: u8, hi: u8) -> u8 {
    match dir {
        -1 => v.saturating_sub(1).max(lo),
        1 => (v + 1).min(hi),
        _ => if v >= hi { lo } else { v + 1 },
    }
}

/// The Colors row's choices: the config value and its label.
const COLOR_CHOICES: [(&str, &str); 5] = [
    ("auto", "Auto"),
    ("mono", "Mono"),
    ("16", "16 colors"),
    ("256", "256 colors"),
    ("truecolor", "Truecolor"),
];

fn color_choice_name(v: &str) -> &'static str {
    let d = crate::display::ColorDepth::parse(v);
    COLOR_CHOICES.iter().find(|(c, _)| crate::display::ColorDepth::parse(c) == d).map_or("Auto", |(_, n)| n)
}

fn icon_style_name(s: crate::iconset::IconStyle) -> &'static str {
    use crate::iconset::IconStyle::*;
    match s { Auto => "auto", Ascii => "ascii", Unicode => "unicode", Nerd => "nerd", Emoji => "emoji" }
}

fn ttl_name(secs: u64) -> String {
    match secs {
        0 => "never".into(),
        s if s % 86_400 == 0 => format!("{} d", s / 86_400),
        s => format!("{} h", s / 3_600),
    }
}

fn toggle_val(on: bool) -> String {
    if on { "\u{25CF} on".into() } else { format!("{} off", crate::glyphs::get(crate::glyphs::Glyph::Off)) }
}

/// Whether the desktop icon-label scrim is on (anything but `"none"`/`"off"`).
fn scrim_on(cfg: &Config) -> bool {
    !matches!(
        cfg.desktop_scrim.as_deref().map(str::trim).map(str::to_ascii_lowercase).as_deref(),
        Some("none") | Some("off")
    )
}

/// The full command line (program + args) of a launcher entry, for display.
fn command_line(a: &AppEntry) -> String {
    if a.args.is_empty() {
        a.command.clone()
    } else {
        format!("{} {}", a.command, a.args.join(" "))
    }
}

/// Truncate `s` to at most `max` characters, appending `…` when cut.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let keep = max.saturating_sub(1);
        format!("{}\u{2026}", s.chars().take(keep).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Advance the section selector to the Apps panel ("Appearance" → … → "Apps").
    fn to_apps(s: &mut Settings) {
        while SECTIONS[s.section] != "Apps" {
            s.next_section();
        }
    }

    fn to_terminal(s: &mut Settings) {
        s.section = SECTIONS.iter().position(|x| *x == "Terminal").unwrap();
        s.sel = 0;
    }

    /// All text on the panel, one string per row, for assertions.
    fn screen(s: &Settings) -> String {
        let b = s.render(100, 30);
        (0..b.height())
            .map(|y| (0..b.width()).filter_map(|x| b.get(x, y).map(|c| c.ch)).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn type_str(s: &mut Settings, t: &str) {
        for c in t.chars() {
            s.type_char(c);
        }
    }

    #[test]
    fn about_shows_the_version() {
        let mut s = Settings::new(Config::default());
        s.section = SECTIONS.len() - 1;
        let buf = s.render(100, 30);
        let text: String = (0..30).flat_map(|y| (0..100).map(move |x| (x, y))).map(|(x, y)| buf.get(x, y).unwrap().ch).collect();
        assert!(text.contains(&format!("Version     {}", crate::VERSION)), "About lists the version");
        assert!(text.contains(&format!("Log         {}", crate::logfile::display_path())));
    }

    #[test]
    fn terminal_preset_stages_without_touching_saved_config() {
        let mut s = Settings::new(Config::default());
        to_terminal(&mut s);
        assert_eq!(s.item_count(), 6);
        s.right(); // follow theme -> dark
        assert_eq!(s.staged_colors(), (Some("#0d1117"), Some("#c9d1d9")));
        assert_eq!(s.config().terminal_bg, None, "staging never writes the saved config");
        assert!(screen(&s).contains("dark"));
        s.left();
        assert_eq!(s.staged_colors(), (None, None));
        s.left(); // wraps to the last preset
        assert_eq!(s.staged_colors(), (Some("#000000"), Some("#ffb000")));
    }

    #[test]
    fn preview_then_save_persists_and_revert_restores() {
        let mut s = Settings::new(Config::default());
        to_terminal(&mut s);
        s.right();
        s.sel = 3; // Preview
        s.toggle();
        assert_eq!(s.take_action(), Some(SettingsAction::PreviewColors));
        assert!(s.previewing());
        assert_eq!(s.config().terminal_bg, None, "preview is not saved");
        assert!(screen(&s).contains("Preview is live"));
        // Revert: staged goes back to the saved (unset) colors and the apphost is told.
        s.sel = 5;
        s.toggle();
        assert_eq!(s.take_action(), Some(SettingsAction::RevertColors));
        assert!(!s.previewing());
        assert_eq!(s.staged_colors(), (None, None));
        // Stage again and save.
        s.sel = 0;
        s.right();
        s.sel = 4;
        s.toggle();
        assert_eq!(s.config().terminal_bg.as_deref(), Some("#0d1117"));
        assert_eq!(s.config().terminal_fg.as_deref(), Some("#c9d1d9"));
        assert_eq!(s.take_action(), None, "save does not preview; the session syncs the config");
        // Revert with nothing previewed asks the session for nothing.
        s.sel = 5;
        s.toggle();
        assert_eq!(s.take_action(), None);
    }

    #[test]
    fn hex_entry_validates_and_clears() {
        let mut s = Settings::new(Config::default());
        to_terminal(&mut s);
        s.sel = 1;
        s.toggle(); // open the Background box
        assert!(s.is_editing());
        type_str(&mut s, "zzz");
        s.toggle(); // Enter on garbage
        assert!(s.is_editing(), "an invalid color keeps the box open");
        assert!(screen(&s).contains("Not a color"));
        s.backspace();
        s.backspace();
        s.backspace();
        type_str(&mut s, "#1a2b3c");
        s.toggle();
        assert!(!s.is_editing());
        assert_eq!(s.staged_colors().0, Some("#1a2b3c"));
        assert!(screen(&s).contains("custom"), "a hand-typed value matches no preset");
        // A color name works too.
        s.sel = 2;
        s.toggle();
        type_str(&mut s, "red");
        s.toggle();
        assert_eq!(s.staged_colors().1, Some("red"));
        // Empty clears back to following the theme.
        s.sel = 1;
        s.toggle();
        for _ in 0..7 {
            s.backspace();
        }
        s.toggle();
        assert_eq!(s.staged_colors().0, None);
    }

    #[test]
    fn color_box_discards_on_escape_and_click_away() {
        let mut s = Settings::new(Config::default());
        to_terminal(&mut s);
        s.sel = 1;
        s.toggle();
        type_str(&mut s, "#abcdef");
        s.cancel_edit();
        assert_eq!(s.staged_colors().0, None, "Esc discards");
        s.toggle();
        type_str(&mut s, "#abcdef");
        assert!(!s.handle_click(Point::new(40, 12), 100, 30), "a click away only dismisses");
        assert!(!s.is_editing());
        assert_eq!(s.staged_colors().0, None, "click-away discards a half-typed color");
    }

    #[test]
    fn terminal_section_warns_on_low_contrast_and_old_apphost() {
        let mut s = Settings::new(Config::default());
        to_terminal(&mut s);
        s.sel = 1;
        s.toggle();
        type_str(&mut s, "#777777");
        s.toggle();
        s.sel = 2;
        s.toggle();
        type_str(&mut s, "#7a7a7a");
        s.toggle();
        assert!(screen(&s).contains("Low contrast"));
        s.set_colors_live(false);
        assert!(screen(&s).contains("restart"));
    }

    #[test]
    fn add_custom_app_splits_command_and_args() {
        let mut s = Settings::new(Config::default());
        to_apps(&mut s);
        assert!(s.config().launcher.is_empty());

        s.toggle(); // open the add form on the "＋ Add app…" row
        assert!(s.is_editing());
        for c in "SSH box".chars() { s.type_char(c); }
        s.move_down(); // focus the Command field
        for c in "ssh user@host".chars() { s.type_char(c); }
        s.toggle(); // commit

        assert!(!s.is_editing());
        let apps = &s.config().launcher;
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "SSH box");
        assert_eq!(apps[0].command, "ssh");
        assert_eq!(apps[0].args, vec!["user@host".to_string()]);
        assert_eq!(apps[0].category.as_deref(), Some("Custom"));
    }

    #[test]
    fn empty_field_blocks_commit_and_refocuses() {
        let mut s = Settings::new(Config::default());
        to_apps(&mut s);
        s.toggle(); // open form
        // Commit with both fields empty: stays editing, focused on Name.
        s.toggle();
        assert!(s.is_editing());
        assert!(s.config().launcher.is_empty());
    }

    #[test]
    fn cancel_edit_discards_the_form() {
        let mut s = Settings::new(Config::default());
        to_apps(&mut s);
        s.toggle();
        for c in "junk".chars() { s.type_char(c); }
        s.cancel_edit();
        assert!(!s.is_editing());
        assert!(s.config().launcher.is_empty());
    }

    /// Move selection to the Appearance → "Wallpaper file" row.
    fn to_wallpaper_file(s: &mut Settings) {
        while SECTIONS[s.section] != "Appearance" {
            s.next_section();
        }
        s.sel = 0;
        while s.sel < 2 {
            s.move_down();
        }
    }

    #[test]
    fn wallpaper_file_box_commits_filename() {
        let cfg = Config { wallpaper: None, ..Config::default() };
        let mut s = Settings::new(cfg);
        to_wallpaper_file(&mut s);
        s.toggle(); // open the filename box (empty, since wallpaper is None)
        assert!(s.is_editing());
        for c in "gentle-dawn.png".chars() { s.type_char(c); }
        s.toggle(); // commit
        assert!(!s.is_editing());
        assert_eq!(s.config().wallpaper.as_deref(), Some("gentle-dawn.png"));
    }

    #[test]
    fn click_while_wallpaper_box_open_commits_and_closes() {
        // Regression: a mouse click used to be swallowed while the filename box
        // was open, leaving a mouse-driven user with no way out (the panel
        // looked frozen). A click now commits the typed name and closes the box.
        let cfg = Config { wallpaper: None, ..Config::default() };
        let mut s = Settings::new(cfg);
        to_wallpaper_file(&mut s);
        s.toggle(); // open the box
        for c in "ty.jpg".chars() { s.type_char(c); }
        assert!(s.is_editing());
        let changed = s.handle_click(Point::new(SIDEBAR_W + 5, 3), 80, 24);
        assert!(changed, "click should report a change so the session re-syncs");
        assert!(!s.is_editing(), "the box must close on a click");
        assert_eq!(s.config().wallpaper.as_deref(), Some("ty.jpg"));
    }

    /// Move selection to the Appearance → "Launcher label" row (the last one).
    fn to_launcher_label(s: &mut Settings) {
        while SECTIONS[s.section] != "Appearance" {
            s.next_section();
        }
        s.sel = 6;
    }

    #[test]
    fn launcher_label_box_edits_and_limits_to_six_characters() {
        let mut s = Settings::new(Config::default());
        to_launcher_label(&mut s);
        assert_eq!(s.config().launcher_label, "tetron");
        s.toggle(); // open, pre-filled with "tetron"
        assert!(s.is_editing());
        for _ in 0..6 { s.backspace(); }
        for c in "workbench".chars() { s.type_char(c); }
        s.toggle(); // commit
        assert!(!s.is_editing());
        assert_eq!(s.config().launcher_label, "workbe", "capped at 6 characters");
    }

    #[test]
    fn launcher_label_empty_restores_default_and_session_token_is_allowed() {
        let mut s = Settings::new(Config { launcher_label: "go".into(), ..Config::default() });
        to_launcher_label(&mut s);
        s.toggle();
        s.backspace(); s.backspace();
        s.toggle(); // commit empty
        assert_eq!(s.config().launcher_label, "tetron", "empty means the default");
        s.toggle();
        for _ in 0..6 { s.backspace(); }
        for c in "@session".chars() { s.type_char(c); }
        s.toggle();
        assert_eq!(s.config().launcher_label, "@session", "the 8-character token is allowed");
        // Esc cancels without changing it.
        s.toggle();
        s.backspace();
        s.cancel_edit();
        assert_eq!(s.config().launcher_label, "@session");
    }

    #[test]
    fn click_while_launcher_label_box_open_commits() {
        let mut s = Settings::new(Config::default());
        to_launcher_label(&mut s);
        s.toggle();
        for _ in 0..6 { s.backspace(); }
        for c in "dev".chars() { s.type_char(c); }
        assert!(s.handle_click(Point::new(SIDEBAR_W + 5, 3), 80, 24));
        assert!(!s.is_editing());
        assert_eq!(s.config().launcher_label, "dev");
    }

    #[test]
    fn wallpaper_default_is_the_default_keyword() {
        assert_eq!(Config::default().wallpaper.as_deref(), Some("default"));
    }

    #[test]
    fn icon_scrim_toggle_flips() {
        let mut s = Settings::new(Config::default());
        while SECTIONS[s.section] != "Appearance" {
            s.next_section();
        }
        s.sel = 3; // "Icon label scrim" row
        assert!(s.config().desktop_scrim.is_none()); // default = on
        s.toggle();
        assert_eq!(s.config().desktop_scrim.as_deref(), Some("none")); // -> off
        s.toggle();
        assert!(s.config().desktop_scrim.is_none()); // -> back on
    }

    #[test]
    fn wallpaper_file_empty_autofills_default() {
        let cfg = Config { wallpaper: Some("old.png".into()), ..Config::default() };
        let mut s = Settings::new(cfg);
        to_wallpaper_file(&mut s);
        s.toggle(); // opens pre-filled with "old.png"
        for _ in 0.."old.png".len() { s.backspace(); }
        s.toggle(); // commit empty → the `default` keyword, never blank
        assert_eq!(s.config().wallpaper.as_deref(), Some("default"));
    }

    #[test]
    fn wallpaper_box_on_default_is_replaced_by_first_key() {
        let mut s = Settings::new(Config::default());
        to_wallpaper_file(&mut s);
        s.toggle(); // opens showing "default", as if selected
        s.type_char('/');
        s.type_char('a');
        s.toggle();
        assert_eq!(s.config().wallpaper.as_deref(), Some("/a"));
        // A real path is edited in place: typing appends.
        s.toggle();
        s.type_char('b');
        s.toggle();
        assert_eq!(s.config().wallpaper.as_deref(), Some("/ab"));
    }

    #[test]
    fn wallpaper_box_backspace_on_default_clears_it() {
        let mut s = Settings::new(Config::default());
        to_wallpaper_file(&mut s);
        s.toggle();
        s.backspace(); // clears the whole word
        s.type_char('x');
        s.toggle();
        assert_eq!(s.config().wallpaper.as_deref(), Some("x"));
    }

    #[test]
    fn paste_fills_wallpaper_box_and_cleans_the_path() {
        let mut s = Settings::new(Config::default());
        assert!(!s.paste("ignored"), "no field open, the caller pastes elsewhere");
        to_wallpaper_file(&mut s);
        s.toggle();
        assert!(s.paste("'file:///home/u/My%20Pics/a.jpg'\n"));
        s.toggle();
        assert_eq!(s.config().wallpaper.as_deref(), Some("/home/u/My Pics/a.jpg"));
    }

    #[test]
    fn wallpaper_file_cancel_keeps_prior_value() {
        let cfg = Config { wallpaper: Some("keep.png".into()), ..Config::default() };
        let mut s = Settings::new(cfg);
        to_wallpaper_file(&mut s);
        s.toggle();
        for c in "junk".chars() { s.type_char(c); }
        s.cancel_edit();
        assert!(!s.is_editing());
        assert_eq!(s.config().wallpaper.as_deref(), Some("keep.png"));
    }

    #[test]
    fn left_removes_selected_custom_app() {
        let cfg = Config {
            launcher: vec![
                AppEntry { name: "A".into(), command: "a".into(), args: vec![], category: Some("Custom".into()), requires_cwd: None, cwd: None, cli: None, warn: None },
                AppEntry { name: "B".into(), command: "b".into(), args: vec![], category: Some("Custom".into()), requires_cwd: None, cwd: None, cli: None, warn: None },
            ],
            ..Config::default()
        };
        let mut s = Settings::new(cfg);
        to_apps(&mut s);
        s.sel = 0;
        s.left(); // remove "A"
        assert_eq!(s.config().launcher.len(), 1);
        assert_eq!(s.config().launcher[0].name, "B");
    }

    #[test]
    fn default_apps_section_cycles_handler() {
        let mut s = Settings::new(Config::default());
        while SECTIONS[s.section] != "Default Apps" {
            s.next_section();
        }
        // Row 0 is the first role; cycling changes its handler in the config.
        s.sel = 0;
        let before = s.config().default_apps.clone();
        s.right();
        assert_ne!(s.config().default_apps, before, "cycling changed a handler");
    }

    #[test]
    fn windows_section_steps_grid_dimensions() {
        let mut s = Settings::new(Config::default());
        // Row 2 = grid rows, row 3 = grid columns.
        s.section = WINDOWS;
        s.sel = 2;
        s.right();
        assert_eq!(s.config().grid_rows, 3);
        s.left();
        assert_eq!(s.config().grid_rows, 2);
        s.sel = 3;
        s.right();
        assert_eq!(s.config().grid_cols, 3);
        s.sel = 5; // auto-tile toggle
        s.toggle();
        assert!(s.config().auto_tile);
    }

    #[test]
    fn waiter_row_asks_for_the_opposite_state_and_is_inert_when_missing() {
        use crate::fullscreen::Service;
        let mut s = Settings::new(Config::default());
        s.section = WINDOWS;
        s.sel = 9;
        s.set_waiter(Service::Missing);
        s.toggle();
        assert_eq!(s.take_action(), None);
        s.set_waiter(Service::Stopped);
        s.toggle();
        assert_eq!(s.take_action(), Some(SettingsAction::WaiterService(true)));
        s.set_waiter(Service::Running);
        s.toggle();
        assert_eq!(s.take_action(), Some(SettingsAction::WaiterService(false)));
    }

    #[test]
    fn newly_surfaced_keys_edit_config() {
        use crate::iconset::IconStyle;
        let mut s = Settings::new(Config::default());
        s.section = WINDOWS;
        assert_eq!(s.item_count(), 10);
        s.sel = 7;
        let before = s.config().natural_scroll;
        s.toggle();
        assert_eq!(s.config().natural_scroll, !before);
        s.sel = 8;
        assert!(s.config().fullscreen);
        s.toggle();
        assert!(!s.config().fullscreen);
        s.section = APPEARANCE;
        assert_eq!(s.item_count(), 9);
        s.sel = 7;
        for want in ["mono", "16", "256", "truecolor", "auto"] {
            s.right();
            assert_eq!(s.config().colors, want, "Colors cycles and wraps");
        }
        s.left();
        assert_eq!(s.config().colors, "truecolor");
        s.sel = 4;
        s.right();
        assert_eq!(s.config().icon_style, IconStyle::Ascii);
        s.right();
        assert_eq!(s.config().icon_style, IconStyle::Unicode);
        s.right();
        s.right();
        s.right();
        assert_eq!(s.config().icon_style, IconStyle::Auto, "wraps");
        s.sel = 5;
        s.toggle();
        assert!(!s.config().desktop_enabled);
        for sec in 0..SECTIONS.len() {
            s.section = sec;
            let _ = s.render(100, 30);
        }
    }

    #[test]
    fn clipboard_section_edits_config() {
        let mut s = Settings::new(Config::default());
        s.section = SECTIONS.iter().position(|x| *x == "Clipboard").unwrap();
        assert_eq!(s.item_count(), 10);
        s.sel = 0;
        s.toggle();
        assert!(!s.config().clipboard_history);
        s.sel = 1;
        s.toggle();
        assert!(!s.config().clipboard_persist);
        s.sel = 3; // 100 -> 250
        s.right();
        assert_eq!(s.config().clipboard_history_size, 250);
        s.left();
        s.left();
        assert_eq!(s.config().clipboard_history_size, 50);
        s.sel = 4;
        s.toggle();
        assert_eq!(s.config().clipboard_shims, "on");
        s.left();
        s.left();
        assert_eq!(s.config().clipboard_shims, "off");
        s.sel = 5;
        s.toggle();
        assert_eq!(s.config().clipboard_osc52_read, "always");
        s.sel = 6;
        s.toggle();
        assert!(s.config().clipboard_fake_display);
        s.sel = 7; // never -> 1 h
        s.right();
        assert_eq!(s.config().clipboard_ttl_secs, 3_600);
        s.left();
        s.left();
        assert_eq!(s.config().clipboard_ttl_secs, 2_592_000, "wraps back to 30 d");
        s.sel = 8; // 256 -> 1024
        s.right();
        assert_eq!(s.config().clipboard_item_max_kb, 1024);
        s.sel = 9;
        s.toggle();
        assert_eq!(s.take_action(), Some(SettingsAction::ClearClipboard));
        let _ = s.render(80, 24); // renders without panicking
    }

    #[test]
    fn assistant_agent_row_opens_a_picker() {
        let mut s = Settings::new(Config::default());
        s.section = ASSISTANT;
        assert_eq!(SECTIONS[s.section], "AI");
        s.sel = 1; // the Agent row
        assert!(s.config().assistant_command.is_none(), "unset defaults to opencode");
        s.toggle();
        assert!(s.is_editing(), "the picker takes the keys");
        // Filter to Hermes and choose it (installed or not, it is chosen).
        for c in "hermes".chars() {
            s.type_char(c);
        }
        s.toggle();
        assert!(s.picker.is_none());
        assert_eq!(s.config().assistant_command.as_deref(), Some("hermes"));
        // Esc on an open picker closes it without a change.
        s.toggle();
        s.cancel_edit();
        assert!(s.picker.is_none());
        assert_eq!(s.config().assistant_command.as_deref(), Some("hermes"));
    }

    #[test]
    fn ai_working_folder_picker() {
        let mut s = Settings::new(Config::default());
        s.section = ASSISTANT;
        s.sel = 2;
        assert!(s.config().assistant_cwd.is_none(), "default: the code folder");
        s.toggle();
        for c in "ask".chars() {
            s.type_char(c);
        }
        s.toggle();
        assert_eq!(s.config().assistant_cwd.as_deref(), Some("ask"));
        s.toggle();
        for c in "other".chars() {
            s.type_char(c);
        }
        s.toggle();
        for c in "~/src".chars() {
            s.type_char(c);
        }
        s.toggle();
        assert_eq!(s.config().assistant_cwd.as_deref(), Some("~/src"));
        s.toggle();
        for c in "code".chars() {
            s.type_char(c);
        }
        s.toggle();
        assert!(s.config().assistant_cwd.is_none());
    }

    #[test]
    fn assistant_custom_command_and_mcp_row() {
        let mut s = Settings::new(Config::default());
        s.section = ASSISTANT;
        s.sel = 1;
        s.toggle();
        for c in "custom".chars() {
            s.type_char(c);
        }
        s.toggle(); // Custom command… opens the text box
        assert!(s.picker.is_none() && s.text.is_some());
        for c in "my-agent --x".chars() {
            s.type_char(c);
        }
        s.toggle();
        assert_eq!(s.config().assistant_command.as_deref(), Some("my-agent --x"));
        s.sel = 3;
        assert!(!s.config().assistant_mcp, "MCP is off by default");
        s.toggle();
        assert!(s.config().assistant_mcp);
    }

    fn to_sound(s: &mut Settings) {
        while s.section != SOUND {
            s.next_section();
        }
    }

    fn outs() -> Vec<crate::system::AudioOutput> {
        let o = |id, name: &str, default| crate::system::AudioOutput { id, name: name.into(), default };
        vec![o(56, "HDMI / DisplayPort 3 Output", false), o(59, "Headphones", false), o(60, "Speaker", true)]
    }

    #[test]
    fn entering_sound_asks_for_outputs_and_rows_map_to_actions() {
        let mut s = Settings::new(Config::default());
        to_sound(&mut s);
        assert_eq!(s.take_action(), Some(SettingsAction::RefreshSound));
        s.set_outputs(outs());
        assert!(screen(&s).contains("Speaker"));
        s.left();
        assert_eq!(s.take_action(), Some(SettingsAction::VolumeDown));
        s.right();
        assert_eq!(s.take_action(), Some(SettingsAction::VolumeUp));
        s.toggle(); // Enter on the volume row does nothing
        assert_eq!(s.take_action(), None);
        s.move_down();
        s.toggle(); // Space on Mute
        assert_eq!(s.take_action(), Some(SettingsAction::ToggleMute));
        s.move_down();
        s.right(); // Speaker -> wraps to HDMI 3
        assert_eq!(s.take_action(), Some(SettingsAction::SetOutput(56)));
        assert!(screen(&s).contains("HDMI / DisplayPort 3 Output"));
        s.left();
        assert_eq!(s.take_action(), Some(SettingsAction::SetOutput(60)));
        s.move_down();
        s.toggle();
        assert_eq!(s.take_action(), Some(SettingsAction::OpenMixer));
    }

    #[test]
    fn sound_volume_row_click_and_wheel() {
        let mut s = Settings::new(Config::default());
        to_sound(&mut s);
        s.take_action();
        let vx = SIDEBAR_W + 2 + 30;
        assert!(s.handle_click(Point::new(vx, 3), 100, 30));
        assert_eq!(s.take_action(), Some(SettingsAction::VolumeDown));
        s.handle_click(Point::new(vx + 6, 3), 100, 30); // 5th bar cell
        assert_eq!(s.take_action(), Some(SettingsAction::VolumeSet(50)));
        s.handle_click(Point::new(vx + 13, 3), 100, 30);
        assert_eq!(s.take_action(), Some(SettingsAction::VolumeUp));
        assert!(s.wheel_at(Point::new(vx, 3), 1));
        assert_eq!(s.take_action(), Some(SettingsAction::VolumeUp));
        assert!(!s.wheel_at(Point::new(vx, 4), 1), "only the volume row takes the wheel");
    }

    #[test]
    fn sound_without_outputs_says_so() {
        let mut s = Settings::new(Config::default());
        to_sound(&mut s);
        s.take_action();
        assert!(screen(&s).contains("(none found)"));
        s.move_down();
        s.move_down();
        s.right(); // no outputs: nothing to switch to
        assert_eq!(s.take_action(), None);
    }

    #[test]
    fn tab_switches_panes_and_arrows_move_inside_the_focused_one() {
        let mut s = Settings::new(Config::default());
        s.toggle_pane();
        assert!(s.sidebar_focused());
        assert!(screen(&s).contains("Tab: settings"));
        s.move_down();
        s.move_down();
        assert_eq!(SECTIONS[s.section], "Updates", "Down in the list changes section");
        s.move_up();
        assert_eq!(SECTIONS[s.section], "Windows");
        s.left(); // nothing further left: stays in the list
        assert!(s.sidebar_focused());
        s.right(); // Right opens the section
        assert!(!s.sidebar_focused());
        s.move_down();
        assert_eq!(s.sel, 1, "Down in the rows moves the row");
        s.toggle_pane();
        s.toggle(); // Enter in the list also opens the section
        assert!(!s.sidebar_focused());
    }

    #[test]
    fn opens_on_appearance_and_general_sits_before_about() {
        let s = Settings::new(Config::default());
        assert_eq!(SECTIONS[s.section], "Appearance");
        assert_eq!(SECTIONS[GENERAL], "General");
        assert_eq!(SECTIONS.last(), Some(&"About"));
        for (i, name) in [(APPEARANCE, "Appearance"), (WINDOWS, "Windows"), (UPDATES, "Updates"), (APPS, "Apps"), (DEFAULT_APPS, "Default Apps"), (ASSISTANT, "AI"), (CLIPBOARD, "Clipboard"), (TERMINAL, "Terminal"), (SOUND, "Sound"), (DISPLAY, "Display")] {
            assert_eq!(SECTIONS[i], name);
        }
    }

    #[test]
    fn general_creates_when_absent_and_resets_only_on_a_second_enter() {
        let mut s = Settings::new(Config::default());
        s.enter_section(GENERAL);
        s.cfg_exists = false;
        s.sel = 1;
        assert!(screen(&s).contains("Create from example"));
        s.toggle();
        assert_eq!(s.take_action(), Some(SettingsAction::CreateConfigFile));
        s.cfg_exists = true;
        assert!(screen(&s).contains("Reset to defaults"));
        s.toggle();
        assert_eq!(s.take_action(), None, "the first Enter only arms");
        assert!(screen(&s).contains("Enter again to reset"));
        s.toggle();
        assert_eq!(s.take_action(), Some(SettingsAction::ResetConfigFile));
        s.toggle();
        s.move_up();
        s.move_down();
        s.toggle();
        assert_eq!(s.take_action(), None, "a move in between disarms");
        s.sel = 0;
        s.toggle();
        assert_eq!(s.take_action(), Some(SettingsAction::OpenConfigFile));
        s.sel = 2;
        s.toggle();
        assert_eq!(s.take_action(), Some(SettingsAction::ReloadConfig));
    }

    #[test]
    fn get_more_themes_asks_once_until_the_worker_is_done() {
        let mut s = Settings::new(Config::default());
        s.sel = 8;
        assert!(screen(&s).contains("Get more themes"));
        s.toggle();
        assert_eq!(s.take_action(), Some(SettingsAction::GetThemes));
        s.toggle();
        assert_eq!(s.take_action(), None, "busy: Enter does nothing");
        s.set_themes_status("GitHub rate limit, try again later".into(), true);
        assert!(screen(&s).contains("GitHub rate limit"));
        s.toggle();
        assert_eq!(s.take_action(), Some(SettingsAction::GetThemes));
    }

    #[test]
    fn about_shows_how_it_was_installed() {
        let mut s = Settings::new(Config::default());
        s.enter_section(SECTIONS.len() - 1);
        let text = screen(&s);
        assert!(text.contains("Installed"), "{text}");
        assert!(text.contains(", size "), "{text}");
    }

    #[test]
    fn general_apps_row_opens_the_dialog_and_esc_closes_it() {
        let mut s = Settings::new(Config::default());
        s.enter_section(GENERAL);
        s.have_curl = true;
        s.sel = 3;
        assert!(screen(&s).contains("Enter for the choices"));
        s.toggle();
        assert!(s.is_editing(), "the dialog takes the keys");
        let sc = screen(&s);
        assert!(sc.contains("General \u{203A} Apps") && sc.contains("Essentials") && sc.contains("Recommended"), "{sc}");
        s.next_section();
        assert_eq!(s.section, GENERAL, "no section change while the dialog is open");
        s.move_down();
        s.toggle();
        assert!(screen(&s).contains("[ Cancel ]"), "a size opens its confirmation");
        s.right();
        s.toggle();
        let a = s.take_action();
        assert!(matches!(a, Some(SettingsAction::InstallApps(crate::packaging::Size::Recommended, true))), "{a:?}");
        assert!(!s.is_editing(), "installing closes the dialog");
        s.toggle();
        s.toggle();
        s.cancel_edit();
        assert!(s.is_editing(), "Esc leaves the confirmation for the choices");
        s.cancel_edit();
        assert!(!s.is_editing());
    }

    #[test]
    fn display_brightness_row_steps_and_asks_to_set_it() {
        let mut s = Settings::new(Config::default());
        s.enter_section(DISPLAY);
        s.brightness = Some(50);
        assert!(screen(&s).contains("50%"));
        s.right();
        assert_eq!(s.take_action(), Some(SettingsAction::BrightnessSet(55)));
        s.left();
        s.left();
        assert_eq!(s.take_action(), Some(SettingsAction::BrightnessSet(45)));
        s.toggle();
        assert_eq!(s.take_action(), None, "Enter does nothing");
        s.brightness = None;
        assert_eq!(s.item_count(), 0);
        assert!(screen(&s).contains("No backlight"));
    }

    #[test]
    fn general_log_level_row_cycles_all_levels_and_shows_the_path() {
        use crate::logfile::Level;
        let mut s = Settings::new(Config::default());
        s.enter_section(GENERAL);
        s.sel = 4;
        assert!(screen(&s).contains("info"), "default info");
        assert!(screen(&s).contains(&format!("Log: {}", crate::logfile::display_path())));
        s.right();
        assert_eq!(s.config().log_level, Level::Debug);
        s.right();
        assert_eq!(s.config().log_level, Level::Error, "wraps");
        s.left();
        assert_eq!(s.config().log_level, Level::Debug);
        s.toggle();
        assert_eq!(s.config().log_level, Level::Error);
        assert_eq!(s.take_action(), None, "applied by the config, no action");
    }

    #[test]
    fn general_apps_row_says_how_many_apps_are_missing() {
        use crate::packaging::Size;
        let mut s = Settings::new(Config::default());
        s.enter_section(GENERAL);
        s.apps_size = Size::Essentials;
        s.have_curl = true;
        s.apps_missing = vec!["fresh".into(), "uv".into()];
        assert!(s.apps_value().starts_with("essentials, 2 missing"), "{}", s.apps_value());
        s.apps_missing.clear();
        assert!(s.apps_value().starts_with("essentials:"), "{}", s.apps_value());
    }
}
