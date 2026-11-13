//! `tetron-wm --help` text. The config-key table is checked against the
//! `Config` struct by a test, so a new setting cannot ship undocumented.

/// (key, default, description) for every `config.toml` setting.
pub const CONFIG_KEYS: &[(&str, &str, &str)] = &[
    ("snapping_enabled", "false", "drag a window to a screen edge to snap it"),
    ("snap_threshold", "3", "cells from the edge that trigger snapping"),
    ("window_shadows", "false", "draw window drop shadows (file only)"),
    ("natural_scroll", "true", "invert wheel/touchpad scroll direction"),
    ("fullscreen", "true", "full screen video, images and PDFs; false hides the items and skips the framebuffer, DRM and waiter probes"),
    ("grid_rows", "2", "tiling grid rows (1-6)"),
    ("grid_cols", "2", "tiling grid columns (1-6)"),
    ("tile_gap", "0", "cells of gutter between tiled windows"),
    ("auto_tile", "false", "auto-arrange all windows into the grid"),
    ("launch_maximized", "false", "open new app windows maximized"),
    ("theme", "midnight", "built-in (midnight, gruvbox, pastoral, windows) or themes/<name>.toml stem (nord, dracula, dawn, newmoon, contrast, mocha, hotdog, dorfic: suite installer --themes)"),
    ("theme_dir", "<config>/themes", "directory of *.toml theme files"),
    ("terminal_bg", "theme", "terminal default background (#rrggbb or name)"),
    ("terminal_fg", "theme", "terminal default foreground (#rrggbb or name)"),
    ("scrollback", "10000", "scrollback lines per app window (0 to 100000); applies to new windows"),
    ("truecolor", "auto", "force 24-bit color on/off"),
    ("wallpaper", "default", "desktop picture: default (the theme's own), black, #rrggbb, none (the theme desktop colour), a file in themes/, or a full path (pictures need chafa)"),
    ("wallpaper_enabled", "true", "master wallpaper toggle"),
    ("colors", "auto", "color depth: auto | mono (no color) | 16 | 256 | truecolor (Settings > Appearance; TETRON_WM_COLORS wins)"),
    ("cast_device", "", "Chromecast for right-click Cast to Chromecast: a name or IP for catt -d; empty = catt's default device"),
    ("image_viewer", "auto", "image viewer: auto (Kitty graphics if the terminal has them, cells on a bare VT) | graphics | cells"),
    ("icon_style", "auto", "icon glyphs: auto (ascii on a bare console, else unicode) | ascii | unicode | nerd | emoji"),
    ("default_project_dir", "~", "root of the working-directory picker"),
    ("recent_dirs", "[]", "legacy: now in ~/.local/share/tetron-wm/state.toml"),
    ("show_hidden_dirs", "false", "show dot directories in the picker"),
    ("apps", "[]", "[[apps]] auto-started at launch (and docked)"),
    ("launcher", "[]", "[[launcher]] apps in the launcher (falls back to apps)"),
    ("default_apps", "built-in", "[default_apps] file role -> handler"),
    ("open_with", "none", "[open_with] extra right-click Open with... programs per role, e.g. video = [\"casttui\"]"),
    ("filemanager_view", "-", "default file manager view: icon | list"),
    ("desktop_enabled", "true", "show desktop icons"),
    ("desktop_scrim", "#000000c8", "icon label backdrop over wallpaper; none/off disables"),
    ("desktop_pins", "[]", "[[desktop_pins]] pinned desktop shortcuts"),
    ("desktop_positions", "{}", "legacy: now in ~/.local/share/tetron-wm/state.toml"),
    ("assistant_command", "opencode", "assistant agent: an agent id (Settings > Assistant lists them) or a command line"),
    ("assistant_args", "[]", "extra arguments for the assistant CLI"),
    ("assistant_agent", "[]", "[[assistant_agent]] agents added by hand, or catalog agents overridden by id"),
    ("assistant_mcp", "false", "give the assistant the tetron-wm mcp desktop tools"),
    ("assistant_code_dir", "~/code", "folder the assistant clones and builds suite code in"),
    ("assistant_mode", "panel", "assistant layout: panel | window"),
    ("update_branch", "main", "updater channel: main (release) | dev (source build)"),
    ("clipboard_history", "true", "record everything copied; browse from the menubar clipboard segment"),
    ("clipboard_persist", "true", "save the history to disk (~/.local/share/tetron-wm, mode 0600)"),
    ("clipboard_record_selection", "true", "record mouse selections, not just explicit copies"),
    ("clipboard_history_size", "100", "maximum unpinned history entries"),
    ("clipboard_item_max_kb", "256", "do not record entries larger than this"),
    ("clipboard_shims", "auto", "xclip/xsel/wl-copy/wl-paste shims for launched apps: auto (no display) | on | off"),
    ("clipboard_fake_display", "false", "dummy WAYLAND_DISPLAY (no display) so Neovim uses the clipboard shims"),
    ("clipboard_gui_bridge", "auto", "mirror the X11/Wayland clipboard into history: auto | on | off"),
    ("clipboard_osc52_read", "focused", "who may read the clipboard via OSC 52: focused | always | never"),
    ("clipboard_ignore_apps", "[]", "do not record while these apps are focused (e.g. password managers)"),
    ("clipboard_ttl_secs", "0", "expire unpinned entries after N seconds (0 = never)"),
    ("web_home", "https://lite.duckduckgo.com/lite/", "start page for w3m and other text browsers launched without a URL"),
    ("screensaver", "black", "effect for the on-demand screensaver: black | clock (tetron-wm screensaver list)"),
    ("power_commands", "{}", "[power_commands] suspend/reboot/poweroff -> command line or list tried in order (replaces the built-in chain)"),
    ("log_level", "info", "how much goes into the log: error | warn | info | debug (TETRON_WM_DEBUG=1 forces debug)"),
    ("power_menu", "true", "show launcher > Power (Suspend/Reboot/Power off); false disables them everywhere (remote servers)"),
    ("launcher_label", "tetron", "menubar launcher button text: up to 6 characters (centred), or @session for the session name"),
    ("screenshot_dir", "", "where screenshots are saved (empty = ~/Pictures if present, else $HOME)"),
    ("leader", "ctrl+space", "leader chord, ctrl+<key> (needs reload)"),
    ("keys", "{}", "[keys] leader remaps: action = \"x\" or [\"x\", \"enter\"]"),
    ("tray_placement", "{}", "[tray_placement] segment -> bar | info | auto (defaults: cpu, mem, bluetooth info; rest auto)"),
    ("dock_badges", "built-in", "[dock_badges] app keyword -> badge color"),
    ("tray_tools", "built-in", "[tray_tools] cpu/mem/battery/bluetooth/network/volume/calendar -> Open-row commands"),
];

/// (name, description) for every environment variable.
pub const ENV_VARS: &[(&str, &str)] = &[
    ("TETRON_WM_THEME", "theme override (wins over config theme)"),
    ("TETRON_WM_THEMES_DIR", "directory of theme files (wins over theme_dir)"),
    ("TETRON_WM_WALLPAPER", "wallpaper path (wins over config wallpaper)"),
    ("TETRON_WM_TRUECOLOR", "1/0 force truecolor on/off (wins over config truecolor)"),
    ("TETRON_WM_COLORS", "mono|16|256|truecolor force the color depth, over the colors setting (default auto: TERM=linux is 16)"),
    ("NO_COLOR", "set (non-empty) to draw in black and white (no-color.org); TETRON_WM_COLORS wins"),
    ("TETRON_WM_GRAPHICS", "1 force Kitty graphics on, 0 off"),
    ("TETRON_WM_GPM", "1 force the gpm mouse reader on, 0 off (Linux console)"),
    ("TETRON_WM_DEBUG", "set to print the exact system-switch script that will run"),
    ("TETRON_WM_SESSION", "session name (same as -s); set inside app windows so tetron-wm commands there reach their own session"),
    ("TETRON_WM_BIN_DIR", "install.sh/updater install dir (default /usr/local/bin)"),
    ("TETRON_WM_METHOD", "install.sh: deb (apt package and the tetronos apt repo) | binary (diverts the package's binary)"),
    ("TETRON_WM_FROM_CLONE", "install.sh: cargo build --release in this clone and install it as the binary (own build)"),
    ("TETRON_WM_NO_APT_REPO", "install.sh: 1 installs the deb without adding the tetronos apt repo"),
    ("XDG_CONFIG_HOME", "config root (default ~/.config)"),
    ("XDG_DATA_HOME", "data root (default ~/.local/share)"),
    ("XDG_RUNTIME_DIR", "socket directory"),
    ("COLORTERM / TERM / TERM_PROGRAM", "terminal capability detection"),
    ("EDITOR / SHELL", "default editor / shell for launched apps"),
    ("PATH / HOME / SHELL / LANG", "captured into the apphost service by `service install`"),
];

/// The full `--help` text.
pub fn text() -> String {
    let mut s = format!(
        "tetron-wm {} - a window manager and desktop for the terminal\n\n\
USAGE:\n  tetron-wm [-s NAME] [COMMAND]\n\n\
OPTIONS:\n\
\x20 -s, --session NAME    use the named session: its own daemon, apps and sockets.\n\
\x20                       Works with every command. Default: $TETRON_WM_SESSION, else \"default\"\n\
\x20 -V, --version         print version\n\
\x20 -h, --help            print this help (also works after any command)\n\n\
COMMANDS:\n\
\x20 (none)                start the session's daemon if needed, then attach\n\
\x20 attach [-d] [-s NAME] attach to a RUNNING session (never starts one). A session has one\n\
\x20                       client: attach is refused while another is attached; -d detaches\n\
\x20                       that client and takes over (tmux attach -d)\n\
\x20 ls                    list sessions and windows: attached | detached | recoverable | stale\n\
\x20 reload                restart the frontend only; apps keep running\n\
\x20 fullscreen-waiter [--vt N]   the helper that plays video full screen from kmscon (runs\n\
\x20                       as you on a spare VT, default 9; set up by the suite installer)\n\
\x20 kill                  stop the session's daemon and app host (closes all windows)\n\
\x20 kill-app <id|all>     close app <id> (see ps); all reaps only exited apps\n\
\x20 ps                    list running apps\n\
\x20 launch <cmd> [args]   open an app window in the running desktop; a built-in by name:\n\
\x20                       @files [dir], @settings, @store, @activity, @logs, @image <file>\n\
\x20 tile                  tile all windows into the grid\n\
\x20 screenshot [file.png] save a PNG of the whole screen (needs an attached client)\n\
\x20 screensaver [on|off|toggle|list] [effect]   cover the screen (default on); any key or mouse\n\
\x20                       in the attached client wakes it. Effects: black, clock (see list)\n\
\x20 config init           write the example config to ~/.config/tetron-wm/config.toml if no\n\
\x20                       file is there (never overwrites); config example prints it.\n\
\x20                       In the app: Settings > General > Config file (open, create, reset\n\
\x20                       keeping config.toml.bak, reload)\n\
\x20 theme <name>          switch theme\n\
\x20 theme --list          list built-in and file themes. Theme files are read from\n\
\x20                       <config dir>/themes, ~/.local/share/tetron-wm/themes, then\n\
\x20                       /usr/local/share and /usr/share (tetron-wm/themes); first wins.\n\
\x20                       Settings > Appearance > Get more themes downloads the ones new on\n\
\x20                       GitHub into ~/.local/share/tetron-wm/themes\n\
\x20 theme --dump <name>   write a built-in theme to <config dir>/themes/<name>.toml\n\
\x20                       to edit (e.g. theme --dump gruvbox, then set theme = \"gruvbox\")\n\
\x20                       A theme file X.toml shows the picture X.jpg (or png, webp) next to it,\n\
\x20                       or the one its wallpaper key names, while wallpaper = \"default\".\n\
\x20                       Theme file keys: desktop_bg window_bg title_focus title_blur title_fg\n\
\x20                       border shadow ctrl_fg close_fg menubar_bg dock_bg text dim accent\n\
\x20                       active_bg, optional success error accent_fg icon_fg (derived when\n\
\x20                       unset), [ansi] (the 16 colours apps see: black red ... bright_white)\n\
\x20                       and [vt16] (palette index 0-15 per role for the 16-colour console).\n\
\x20                       See docs/CONFIG.md (Theme file keys) and docs/tetron-wm_theming_guide.html.\n\
\x20 msg '<json>'          send a raw ClientMsg\n\
\x20 clip copy|paste|list|clear|pin N|delete N   the clipboard history (see `clip` alone)\n\
\x20 service [install|uninstall|status]  per-user apphost service (default session only)\n\
\x20 --daemon, --apphost   run the daemon / app host directly\n\n\
SESSIONS:\n\
\x20 Apps run in a background daemon, so you can detach and come back to the same windows:\n\
\x20   detach     leader q, or launcher > Detach (apps keep running, nothing closes)\n\
\x20   come back  run tetron-wm (or tetron-wm attach); see what is waiting with tetron-wm ls\n\
\x20   take over  tetron-wm attach -d, from another terminal or over ssh\n\
\x20   stop       launcher > Quit, leader Q, or tetron-wm kill (closes every app)\n\
\x20 Name a session to keep separate workspaces side by side:\n\
\x20   tetron-wm -s work              start or attach the session \"work\"\n\
\x20   tetron-wm -s work launch htop  any command can target a session\n\
\x20   tetron-wm ls                   SESSION  STATE  WINDOWS for every session\n\
\x20 ls states: attached = a client is connected; detached = running with no client (your\n\
\x20 work is waiting); recoverable = daemon gone but apps alive, start it to restore the\n\
\x20 windows; stale = leftover sockets, nothing answers. A cleanly stopped session is not listed.\n\
\x20 Windows opened inside a session inherit it (TETRON_WM_SESSION), so tetron-wm commands run\n\
\x20 in a window reach that window's session. Names: 1-32 chars of A-Z a-z 0-9 _ -.\n\
\x20 The launcher button shows the session name with launcher_label = \"@session\".\n\n\
RIGHT-CLICK MENU (desktop icons and Files; leader . from the keyboard):\n\
\x20 Open (video and audio play in the window with mpv), Open with... (apps for the file\n\
\x20 type, fresh and other editors for text), Set as wallpaper, Extract here / List\n\
\x20 contents (archives), Cast to Chromecast (video, audio and images; needs catt, picks\n\
\x20 `cast_device`, runs in a window and closing it stops the cast), Copy path, Open\n\
\x20 terminal here, Get Info, Send to... (scp to a saved system), Rename, Compress...\n\
\x20 (.7z, .zip, .tar.gz), Encrypt... (.7z AES-256 with hidden names, AES .zip,\n\
\x20 .tar.gz.age, .tar.gz.gpg; the tool asks for the password in a window; needs 7z,\n\
\x20 age or gpg), Move to Trash. Extract here also decrypts .gpg and .age. On a plain Linux console (not kmscon, ssh or a\n\
\x20 GUI terminal) View full screen shows images with fbi and Play full screen plays\n\
\x20 video with mpv; q returns, apps keep running. Under kmscon both work once the\n\
\x20 full screen helper is installed (install-tetron-wm-suite.sh --fullscreen): it switches\n\
\x20 to the spare VT, shows the video, images or PDF pages with mpv (fbi, fim or fbgs only\n\
\x20 without mpv or a DRM display) and switches\n\
\x20 back, no sudo. Settings > Windows > Full screen helper service starts or stops it (sudo\n\
\x20 systemctl, prompted in the terminal); the helper is off unless you install it. A window's title bar has Minimize,\n\
\x20 Maximize, Move, Rename and Close.\n\n\
IMAGE VIEWER (images and PDFs; image_viewer = auto | graphics | cells):\n\
\x20 Kitty graphics where the terminal has them, else chafa cells (any terminal).\n\
\x20 + - or the wheel zoom, 0 fits, arrows or a drag pan, n p next/previous in the folder,\n\
\x20 m graphics/cells, i info line, f full screen (plain console), Esc or q closes.\n\
\x20 A PDF shows one page at a time (pdftoppm); n p turn pages. Open with... also offers\n\
\x20 its text in a pager (pdftotext), tdf and timg; full screen on a console uses fbgs.\n\n\
LAUNCHER SYSTEM SECTION (click the top-left button):\n\
\x20 Systems  switch to a saved remote machine over ssh\n\
\x20 Detach   leave; apps keep running        Reload  restart the UI, apps keep running\n\
\x20 Quit     stop tetron-wm, closing all apps\n\
\x20 Power    Suspend / Reboot / Power off the COMPUTER (each asks first; a refused action\n\
\x20          opens a window saying what was tried; set [power_commands] to change how;\n\
\x20          power_menu = false removes it, for servers that must stay up)\n\n\
FILES:\n  config:  $XDG_CONFIG_HOME/tetron-wm/config.toml (default ~/.config/tetron-wm/)\n\
\x20 themes:  <config dir>/themes/*.toml\n  log:     ~/.local/state/tetron-wm/debug.log (rotated .1 .. .3)\n\
\x20 sockets: $XDG_RUNTIME_DIR/tetron-wm-$USER/ (named sessions in sessions/NAME/)\n\n\
CONFIG KEYS (config.toml):\n",
        env!("CARGO_PKG_VERSION")
    );
    let w = CONFIG_KEYS.iter().map(|k| k.0.len()).max().unwrap_or(0);
    let dw = CONFIG_KEYS.iter().map(|k| k.1.len()).max().unwrap_or(0);
    for (k, d, desc) in CONFIG_KEYS {
        s += &format!("  {k:<w$}  {d:<dw$}  {desc}\n");
    }
    s += "\nKEYS - press the leader (Ctrl+Space), release it, then the key:\n\
\x20 Rebind the leader with leader = \"ctrl+b\" and any action with [keys] action = \"x\" (see CONFIG KEYS).\n\
\x20 ACTION      KEY(S)           DOES\n";
    for (action, keys, desc) in crate::keymap::DEFAULTS {
        s += &format!("  {action:<11} {:<16} {desc}\n", keys.join(" "));
    }
    s += "\x20 (fixed)     1-9              send the focused window to grid cell N\n\
\x20 Copying out of w3m, tetron-browser or vim (they grab the mouse): leader x, drag over the text,\n\
\x20 then leader C to pick it, or leader v to paste it into another window.\n";
    s += "\nENVIRONMENT:\n";
    let w = ENV_VARS.iter().map(|e| e.0.len()).max().unwrap_or(0);
    for (n, desc) in ENV_VARS {
        s += &format!("  {n:<w$}  {desc}\n");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `pub` field of `struct Config` must have a CONFIG_KEYS row, and vice versa.
    #[test]
    fn config_keys_match_config_struct() {
        let src = include_str!("config.rs");
        let body = src.split("pub struct Config {").nth(1).unwrap().split("\n}\n").next().unwrap();
        let mut fields: Vec<&str> = body
            .lines()
            .filter_map(|l| l.trim().strip_prefix("pub "))
            .filter_map(|l| l.split(':').next())
            .collect();
        let mut keys: Vec<&str> = CONFIG_KEYS.iter().map(|k| k.0).collect();
        fields.sort();
        keys.sort();
        assert_eq!(fields, keys);
    }

    /// `--help` must keep explaining the detach / attach / named-session workflow.
    #[test]
    fn text_documents_detach_attach_and_named_sessions() {
        let t = text();
        for needle in [
            "-s, --session NAME",
            "attach [-d] [-s NAME]",
            "tetron-wm ls",
            "attached | detached | recoverable | stale",
            "launcher > Detach",
            "TETRON_WM_SESSION",
            "@session",
            "SESSIONS:",
            "RIGHT-CLICK MENU",
            "IMAGE VIEWER",
            "View full screen",
            "Power ",
            "[power_commands]",
            "screensaver [on|off|toggle|list]",
            "screensaver  ",
            "launcher_label",
            "power_menu",
            "tray_placement",
            "sessions/NAME/",
        ] {
            assert!(t.contains(needle), "--help is missing {needle:?}");
        }
        // The old wording must not come back.
        assert!(!t.contains("launcher > Exit") && !t.contains("Shutdown"), "stale labels in --help");
    }

    #[test]
    fn text_lists_every_env_var() {
        let t = text();
        for (n, _) in ENV_VARS {
            assert!(t.contains(n));
        }
    }
}
