# tetron-wm features

What tetron-wm can do, grouped by what you are trying to get done. Each section says where to click or what to press, and links to the document that has the details. New to it? Start with the [README](../README.md).

Keys below are leader sequences: press `Ctrl+Space`, release, then the key. The full table is [KEYS_tetron-wm_leader_key_table.md](KEYS_tetron-wm_leader_key_table.md).

## Windows

- **Floating windows.** Each window runs a real terminal application in its own pseudo-terminal, drawn by a full terminal emulator, so demanding apps such as btop render correctly.
- **Mouse everywhere.** Drag a titlebar to move, an edge to resize, use the titlebar buttons to minimize, maximize and close. The mouse also passes through to apps that ask for it (btop, yazi, lazygit, vim with `mouse=a`).
- **Keyboard window switching.** Leader `Tab` focuses the next window (leader `Shift+Tab` the previous), then plain `Tab` / `Shift+Tab` keep cycling until Enter, Esc or another key. Leader `f` opens a numbered list of every window, minimized ones included: press its number or use the arrows and Enter.
- **Keyboard move and resize.** Leader `w` moves the focused window with the arrow keys (`Shift`+arrow for 5 cells), `r` switches to resizing, Enter keeps the result and Esc puts the window back. With these, a console without a mouse can still arrange every window.
- **Scrollback.** The wheel over a window scrolls its history (10,000 lines). Typing returns you to the live bottom. `Shift+PageUp` and `Shift+PageDown` work too.
- **Safe close.** Closing an app window asks first, since it ends the process. Built-in panels (Store, Settings, Files) close without asking.
- **Rename.** Double-click a window title or press leader `r`. A renamed window stays grouped with its app.
- **Simple view.** The `⊞` / `▦` toggle in the top bar switches to a full-screen single-app view like tmux, keeping the menubar and taskbar. The same apps keep running in both views.

## Tiling and snapping

- **Grid tiling.** Choose a rows by columns grid (for example 1x3 on an ultrawide). Leader `t` tiles every window now, leader `T` toggles auto-tile, and leader `1` to `9` sends the focused window to a numbered cell.
- **Snap to edges.** Drag a window to a screen edge or to a grid cell, with a live preview. Off by default.
- **Settings:** Settings → Windows. Keys: `snapping_enabled`, `auto_tile`, `grid_rows`, `grid_cols`, `tile_gap` in [CONFIG.md](CONFIG.md).

## Taskbar and tray

- **Taskbar.** Open windows appear as pills along the top. Windows of the same app group into one pill with a colored letter badge (colors in `[dock_badges]`). When space runs short, pills shrink to badges and then to a `…+N` marker, so the bar never collides with the tray.
- **Pill menu.** Right-click a pill for minimize, maximize, close and Reset, which re-centers a window that ended up off screen or tiny.
- **Status tray.** Clock, CPU, memory, volume, WiFi, Bluetooth and battery. Popovers let you change the host volume, switch to a known WiFi network or connect a paired Bluetooth device. Leader `=` opens the volume popover from the keyboard: Left / Right set the level, `m` mutes, and Down then Enter opens a mixer (wiremix, pulsemixer, ncpamixer or alsamixer, whichever is installed). Settings → Sound has the same volume and mute plus an output device picker (send sound to an HDMI monitor, headphones or speakers; needs PipeWire).
- **The ⓘ menu.** Session name, window count and version, plus links to Sessions, Logs, Shortcuts and Settings. It also holds tray segments you moved off the bar. Choose where each lives with `[tray_placement]`.
- **Calendar.** Click the clock for a month view. If [khal](https://github.com/pimutils/khal) is installed, your upcoming events show too.
- **Notifications.** When a background or minimized app rings the terminal bell, its pill gets an attention dot and a bell counter appears in the tray. The popover lists who rang and when, and a click jumps to that window.
- **Tray tools.** Each tray popover has an "Open ..." row (htop, a network tool and so on). Which program it runs is `[tray_tools]`.

## Launching apps

- **Launcher.** A cascading menu from the top-left button (or leader `l`), with a one-click **Shell** at the top and your apps in categories. A `+` button in the menubar opens a new shell instantly.
- **Spotlight.** Leader `space` opens a search over every app.
- **App store.** Leader `S`. Browse, search and install from a catalog of 1,300+ terminal apps, including AI coding agents and a terminal office suite. The catalog is OS-aware, so Linux-only tools do not show on macOS. An install uses your system's package manager when it has the app (apt, dnf, pacman, or Homebrew on macOS), else the app's own release or language installer, and Homebrew on Linux only as the last resort. If an install needs a toolchain you lack (Go, Rust, Node, Python), you are offered it first.
- **CLI tools.** Catalog entries that are commands rather than full-screen apps carry a `CLI` tag and open in a shell with `--help` printed, instead of a window that runs and vanishes.
- **Your own entries.** Add them in Settings → Apps, or as `[[launcher]]` / `[[apps]]` tables. Fields and examples: [CONFIG.md](CONFIG.md#app-entries).
- **Working-directory picker.** Coding agents ask which project directory to start in, with a browsable tree and a recent list. An entry can set `warn` to show a confirmation first, which the store uses for the Claude Code variant that skips permission prompts.

## Files and the desktop

- **File manager.** Launcher → Files. The sidebar has Home, Code (`~/code`, the AI agent's code folder), Desktop, Documents, Downloads and Pictures. Icon, list and Miller-column views, thumbnails and a preview pane, tabs, an info panel, copy, cut, paste, rename and new folder. Delete goes to the Trash, never a hard delete; Empty Trash (right-click on empty space, confirmed) deletes for good. Right-click opens a context menu: Copy and Cut on files (on the desktop too), Paste on empty space and Paste into folder on a folder; a name already there gets " copy", " copy 2". Copy in one window and paste in another or on the desktop.
- **Open with.** A file-type to app map (Settings → Default Apps) decides what opens each file: images in the built-in viewer, text and code in your `$EDITOR`.
- **Desktop icons.** Clickable icons on the wallpaper, from your `~/Desktop` folder plus pinned shortcuts. They refresh within a couple of seconds when the folder changes, and drag to rearrange.
- **Image viewing.** Real raster images in windows on terminals that speak the Kitty graphics protocol (Kitty, Ghostty, WezTerm), with a cell fallback elsewhere.
- **Remote files.** Each saved system gets a "Files on name" entry. Browse its disk over ssh and copy between machines with Ctrl+C in one window and Ctrl+V in another.

## Look and feel

- **Themes.** Four built in (midnight, gruvbox, pastoral, windows) plus theme files: the suite installer and Settings → Appearance → Get more themes add the ones on GitHub, and your own `*.toml` go in `~/.config/tetron-wm/themes/`. Switch live in Settings → Appearance → Theme (Enter lists every theme, type to filter; Left/Right step) or with `tetron-wm theme NAME`.
- **Terminal colors.** `terminal_bg` and `terminal_fg` decouple app colors from the desktop, for example a light desktop with dark terminals.
- **Wallpaper.** Any image, rendered to colored cells with [chafa](https://hpjansson.org/chafa/), so it works on any terminal including a bare console.
- **Icon style.** `auto` (the default) draws `ascii` glyphs on a bare Linux console, whose fonts lack most symbols, and `unicode` everywhere else. `nerd` and `emoji` are there for terminals with those fonts.
- **Color depth.** Detected from the terminal: truecolor, 256, 16 on the bare console, black and white with `NO_COLOR`. Force it with `TETRON_WM_COLORS=mono|16|256|truecolor`. Themes adapt by role, so the desktop, windows and selection stay distinct even in 16 colors, and a theme file can pin its 16-color choices with a `[vt16]` table and set the 16 colors apps see with an `[ansi]` table.
- **Screensaver.** On demand only: leader `z`, the launcher, or `tetron-wm screensaver`. Any key or mouse movement wakes it.
- **Screenshots.** Leader `s` or `tetron-wm screenshot` saves a PNG of the whole screen.

## Clipboard

- **History of everything you copy:** app copies over OSC 52 (vim, tmux, Helix), `Ctrl+Shift+C`, mouse selections, `tetron-wm clip`, and on a desktop machine the real X11 or Wayland clipboard. Open it from the tray, leader `C`, or the launcher. Type to filter, click to paste, pin entries you want to keep.
- **Select out of apps that grab the mouse** (w3m, vim): leader `x`, drag over the text, then leader `C` to pick it or leader `v` to paste.
- **Bracketed paste.** Multi-line text arrives as one paste instead of running line by line in a shell.
- **Shims** for `xclip`, `xsel`, `wl-copy` and `wl-paste` make tools that shell out to them (Neovim's `"+` register, scripts) use this clipboard on a machine with no display.
- **From the shell:** `tetron-wm clip copy|paste|list|pin|delete|clear`. Privacy knobs: `clipboard_persist`, `clipboard_ignore_apps`, `clipboard_ttl_secs`, `clipboard_osc52_read`. All in [CONFIG.md](CONFIG.md).

## Sessions and remote machines

- **Persistent.** Apps live in a separate background process, so they survive detach, a dropped ssh connection, a UI reload and an update. Closing the terminal does not close your work.
- **Named sessions.** `tetron-wm -s NAME` gives a separate set of windows. `tetron-wm ls` lists them and their state, and `attach -d` takes one over from another terminal.
- **Systems switcher.** Launcher → Systems hops between tetron-wm desktops on other machines over ssh. Saved machines show a reachable or not dot. **Add Remote** copies your ssh key, installs tetron-wm there and connects. Each system can carry its own theme. Passwords are used once and never saved.
- **Reload without losing apps.** Launcher → Reload, `tetron-wm reload`, or Settings → Update & Reload.
- **Per-user service.** `tetron-wm service install` keeps the app host running across logins.

Sessions in detail: `tetron-wm --help`, and the Recipes in [CONFIG.md](CONFIG.md#recipes).

## Updates

- **In-app updater.** Settings → Updates. The default `main` channel installs the latest prebuilt release and reloads with apps running. The `dev` channel builds from source for testing.
- **Safety dialog.** If an update ever needs the app server restarted (which closes apps), you are warned first and can save your work.

## AI assistant

- The ✦ button opens a chat panel (or a floating window, your choice) running the agent CLI you pick: [opencode](https://opencode.ai) by default, or Claude Code, Codex, pi, Antigravity CLI, Hermes. Settings → AI → Agent lists them (installed first; a missing one opens the Store) and takes any custom command line. The list comes from the app catalog, so new agents arrive with the Store's Update list; `[[assistant_agent]]` in config.toml adds or adjusts one by hand.
- Settings → AI → Working folder: the code folder `~/code` (default), ask each launch (folder picker), the AI folder, or another folder. Outside the AI folder opencode, Claude Code and pi get the briefing through their own flags; nothing is written into your folder.
- Every agent starts with the same `AGENTS.md`: the desktop, your saved systems, the tetron suite, and how to work on its code in `~/code` (spec first, branch, test, install an own build, `tetron-wm reload`).
- Desktop tools over MCP (Settings → AI, off by default): the agent starts `tetron-wm mcp` and gets windows, launch, tile, themes and screenshots as tools. Nothing runs while no agent does.
- It is told about the desktop: the control commands (`tetron-wm launch`, `tile`, `theme`, `msg`), the logs, and your saved systems. See [agent/](../agent/README.md).
- It can also drive the desktop from any shell with the same commands, so scripts can too: `tetron-wm launch htop`, `tetron-wm tile`.

## Power and servers

- **Power menu.** Launcher → Power offers Suspend, Reboot and Power off for the computer, each confirmed first. `[power_commands]` changes how they run, and `power_menu = false` removes them for machines that must stay up.
- **Bare console.** Runs on a raw Linux console with the kmscon fork (colors, mouse) and the `gpm` mouse service, and can autologin and start itself at boot. See [SETUP_tetron-wm_bare_console_kmscon.md](SETUP_tetron-wm_bare_console_kmscon.md).

## tetron mesh VPN

If [tetron](https://github.com/tetronos/tetron) is installed, a tray segment shows its status (`⇄` with a peer count and a direct or relay dot). Its popover toggles tetron or single networks on and off and opens a terminal into the tetron CLI or [tetron-tui](https://github.com/tetronos/tetron-tui). If tetron is absent or not running, the segment hides and nothing else changes.

## Configuration

All of it is optional. [config.example.toml](../config.example.toml) is a ready-to-copy file with every option at its default, and [CONFIG.md](CONFIG.md) is the full reference with recipes for common setups.
