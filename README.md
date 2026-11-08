# tetron-wm

**Website: [tetronos.github.io/tetron-wm](https://tetronos.github.io/tetron-wm/)**

**A TUI desktop. A graphical tmux.** Floating windows, a taskbar, a launcher, an app store and a mouse, all drawn as text inside the terminal you already have.

<p align="center">
  <img src="docs/images/tetron-wm.png" alt="tetron-wm running in a terminal: floating windows, a top taskbar, a status tray and a mouse cursor" width="900">
</p>

Your apps run in the background, so you can close the terminal, drop an ssh connection or update tetron-wm, and everything is still there when you come back.

## Why use it

- **Windows in a terminal.** Run btop, a shell, vim and a file manager side by side. Drag titlebars, resize edges, click the taskbar. The mouse also reaches apps that use it.
- **Keeps running.** Detach and reattach from any terminal or over ssh, like tmux. Reloading or updating the UI does not close your apps.
- **Works almost anywhere.** Any modern terminal, over ssh, or on a bare Linux console with no X11 or Wayland.
- **Batteries included.** An app store with 600+ terminal apps, a file manager, desktop icons, themes, wallpapers, a clipboard history and an optional AI assistant.

## Install

Prebuilt binary for Linux (x86_64, aarch64) and macOS (Apple silicon, Intel). No Rust needed.

```sh
curl -fsSL https://tetronos.github.io/tetron-wm/install.sh | bash
```

It asks how much to install (minimal: tetron-wm only; desktop: plus the tools its features use, the default; extras: plus fbi and git) and what kind of machine this is, then installs. On Debian and Ubuntu it uses the `.deb` packages once a release carries them, so apt manages tetron-wm and the apps. Every way to install, what each size brings, and how to update and remove: [docs/INSTALL_tetron-wm_methods_and_sizes.md](docs/INSTALL_tetron-wm_methods_and_sizes.md).

Only tetron-wm, nothing else (the suite installer, or Settings > General > Apps, adds the apps later). It installs to `/usr/local/bin` with `sudo`; for your home instead:

```sh
TETRON_WM_BIN_DIR="$HOME/.local/bin" curl -fsSL https://raw.githubusercontent.com/tetronos/tetron-wm/main/install.sh | sh
```

Or build from source with a [Rust toolchain](https://rustup.rs) (1.95 or newer):

```sh
cargo install --git https://github.com/tetronos/tetron-wm
```

### Recommended extras

tetron-wm works on its own; the desktop size adds the tools that make the most of it. By hand on Ubuntu or Debian (the same list as the `tetron-wm-desktop` package, [`packaging/desktop.toml`](packaging/desktop.toml)):

```sh
sudo apt install mpv chafa poppler-utils upower w3m gpm xclip wl-clipboard alsa-utils sshpass fonts-dejavu-core 7zip gnupg python3 curl openssh-client unzip console-setup-linux htop sshfs mc vim
```

The package also recommends tetron-browser, the web browser tetron-wm opens pages in; on amd64 it installs with `sudo apt install tetron-browser` once the [tetronos apt repo](https://tetronos.github.io/apt) is configured.

The suite installer above installs them for you with the desktop size (only the missing ones; the system services PipeWire, NetworkManager and BlueZ are a separate question that defaults to no). They add the image wallpaper and images in the image viewer on any terminal (chafa), video in a window (mpv), PDFs in the viewer (poppler), a friendly terminal editor ([fresh](https://github.com/sinelaw/fresh), from its own installer on Linux, `brew install fresh-editor` on macOS), casting to a Chromecast ([catt](https://github.com/skorokithakis/catt) and [casttui](https://github.com/tetronos/casttui), installed with [uv](https://docs.astral.sh/uv/)), a bookmarks **Start Page** for tetron-browser or w3m (`tetron-wm-startpage`) and **AI Mode** for Gemini questions (`tetron-aimode`, needs a free [Gemini API key](https://aistudio.google.com/apikey) that it asks for on first run), volume and output switching (PipeWire `wpctl`), Wi-Fi, Bluetooth and battery details, desktop clipboard sync, password-based Add Remote, and better tray tools. On a bare console add `gpm` for the mouse, or kmscon for colors and mouse. What each one does, what happens without it, and the macOS equivalents: [docs/DEPENDENCIES_tetron-wm_recommended_installs.md](docs/DEPENDENCIES_tetron-wm_recommended_installs.md). It also downloads the extra themes (nord, dracula, dawn, newmoon and the high-contrast `contrast`, each with its wallpaper) into `~/.config/tetron-wm/themes/` without overwriting anything you have (`--no-themes` skips it).

Want a bare Linux machine that boots straight into the desktop? Use the suite installer, which can also set up the kmscon console, autologin and autostart, and can undo all of it. See [Bare console](#bare-console-no-x11-or-wayland).

## tetron mesh VPN companion network

###### TLDR: Install the entire suite.

Install suite.

[tetron](https://github.com/tetronos/tetron)

```shell
curl -fsSL https://tetronos.github.io/tetron/install.sh | bash
```

tetron-wm has first-class, optional support for [tetron](https://github.com/tetronos/tetron), the P2P mesh VPN it ships alongside in the tetron-os image (below), right in the menubar tray. The integration is self-effacing: the tray talks to the tetron daemon over its own Unix IPC socket using `tetron-proto` (the shared wire protocol, the same crate tetron-systray and tetron-webui speak), and if no daemon answers (tetron not installed, or not running) the segment simply hides. There is nothing to configure, and tetron-wm works identically with or without tetron present.

- **Status at a glance.** The tray shows `⇄<peers>` with a connection-quality dot: `●` when at least one network has a direct path, `○` when every connection is relay-only, and `off` when tetron is reachable but in standby. It is read-only, a background poll that bounds every call so a stuck daemon can never hang the tray.
- **On/off from the popover.** Click the segment for a popover with a global **Tetron [on/off]** toggle plus one row per network. Each network row toggles just that network's data plane (**Resume** / **Standby**) with its own `●`/`○` direct-or-relay indicator; the global toggle moves them all at once. These map to tetron's own `Resume`/`Standby` IPC messages, the same effect as `tetron resume` / `tetron standby` from the CLI.
- **Open terminal.** An action row at the bottom of the popover opens a terminal into tetron: it launches [`tetron-tui`](https://github.com/tetronos/tetron-tui) (the full-screen tetron dashboard) when that is installed, and otherwise prints `tetron status` and drops you into an interactive shell with the full `tetron` CLI. Invites and joining a network live in the cli or tetron-tui, not in the tray.

## First five minutes

```sh
tetron-wm
```

1. **Open the launcher.** Click the button at the top left, or press `Ctrl+Space` then `l`. The first entry is a **Shell**.
2. **Move windows.** Drag a titlebar to move, an edge to resize. The titlebar buttons minimize, maximize and close. From the keyboard, `Ctrl+Space` then `Tab` focuses the next window (keep pressing `Tab` to cycle), `Ctrl+Space` then `f` lists every window to pick one by number, and `Ctrl+Space` then `w` moves the focused window with the arrow keys (`r` to resize, Enter when done).
3. **Get apps.** Open the **Store** (`Ctrl+Space`, then `S`), search and install. If an app needs a toolchain you do not have, the installer notices and offers to set it up first.
4. **Find things fast.** `Ctrl+Space` then `space` opens a search over every app.
5. **Open files.** Double-click a desktop icon or a file in **Files**. Right-click it (or `Ctrl+Space` then `.`) for **Open with…** (for example fresh for text, or tetron-browser, carbonyl, w3m or lynx for a web page), **Set as wallpaper**, **Extract here**, **Compress…** and **Encrypt…** (a password-protected `.7z` by default, opens on any OS; the password is asked in a window), **Cast to Chromecast** (media files, needs [catt](https://github.com/skorokithakis/catt)), **Copy path**, **Open terminal here**, **Get Info** and **Send to…** a saved system. Videos play right in the window with mpv; images open in the built-in viewer on any terminal; on a plain Linux console, **View full screen** and **Play full screen** use fbi and mpv, and under kmscon both work after the suite installer's optional full screen helper (`--fullscreen`: a service as you on a spare VT, no sudo to play). The helper is opt-in: **Settings → Windows → Full screen helper service** starts or stops it (with a sudo prompt), and `fullscreen = false` hides the full screen items and skips the hardware probes. A window's title bar has its own right-click menu.
6. **See all shortcuts.** `Ctrl+Space` then `?`.
7. **Leave without closing anything.** `Ctrl+Space` then `q` detaches. Run `tetron-wm` again to come back.

`Ctrl+Space` is the **leader**: press it, release it, then press the next key. It keeps tetron-wm shortcuts from colliding with your terminal or the apps inside it. Every key, and how to change it, is in [docs/KEYS_tetron-wm_leader_key_table.md](docs/KEYS_tetron-wm_leader_key_table.md).

## Sessions: detach, reattach, stay running

```sh
tetron-wm ls                    # list sessions: attached | detached | recoverable | stale
tetron-wm attach                # come back to a running session
tetron-wm attach -d             # take over a session another terminal or ssh login holds
tetron-wm -s work               # a separate workspace called "work", side by side with the default
tetron-wm reload                # restart only the UI; apps keep running
tetron-wm kill                  # stop everything and close all apps
```

Any command can target a session with `-s NAME`. More examples are in the Recipes section of [docs/CONFIG.md](docs/CONFIG.md), and `tetron-wm --help` lists every command.

## Make it yours

Everything works with no config file. To change something, copy the example and edit it:

```sh
mkdir -p ~/.config/tetron-wm
cp config.example.toml ~/.config/tetron-wm/config.toml
tetron-wm reload
```

- [config.example.toml](config.example.toml) is a ready-to-copy file with every option at its default and a note on each.
- [docs/CONFIG.md](docs/CONFIG.md) is the full reference plus copy-paste recipes: tiling grids, themes, wallpaper, launcher entries, clipboard, servers that must stay up, and more.
- [The theming guide](https://tetronos.github.io/tetron-wm/tetron-wm_theming_guide.html) teaches you to make your own theme: every color key shown on a picture of the desktop, screenshots of the built-in themes, and a step-by-step walkthrough.
- Many options are also in the app under **Settings** (`Ctrl+Space`, then `,`). It opens on **Appearance**, where **Get more themes** downloads the themes new on GitHub. **General → Config file** opens the file in your editor, creates it from the example, resets it to defaults (the old file is kept as `config.toml.bak`) or reloads it after a hand edit.

Common first changes:

| I want                                   | Set                                                                                                                        |
| ---------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| A different color theme                  | `theme = "gruvbox"` (also `midnight`, `pastoral`, `windows`; `nord`, `dracula`, `dawn`, `newmoon`, `contrast` from the installer)                                            |
| Your own color theme                     | `tetron-wm theme --dump midnight`, rename and edit it in `~/.config/tetron-wm/themes/` ([theming guide](docs/tetron-wm_theming_guide.html), keys: docs/CONFIG.md)  |
| Windows to snap to screen edges          | `snapping_enabled = true`                                                                                                  |
| Windows arranged in a grid automatically | `auto_tile = true`, `grid_rows`, `grid_cols`                                                                               |
| Full screen video, images, PDFs          | `fullscreen = false` to turn them off                                                                                      |
| A wallpaper image                        | put it in `~/.config/tetron-wm/themes/` and set `wallpaper = "name.jpg"`, or a full path; `"default"` is the theme's own picture; `black`, `#rrggbb` or `none` for a flat desktop (pictures need [chafa](https://hpjansson.org/chafa/)) |
| A different leader key                   | `leader = "ctrl+b"`                                                                                                        |
| No Power menu on a server                | `power_menu = false`                                                                                                       |
| Less memory per window                   | `scrollback = 2000` (lines kept per window; default 10000)                                                                 |
| Mono (no color), or fewer colors       | Settings → Appearance → Colors, or `colors = "mono"` (also `"16"`, `"256"`, `"truecolor"`)                                  |
| Images always drawn as characters        | `image_viewer = "cells"` (default `"auto"` uses Kitty graphics where the terminal has them)                                 |

## Bare console (no X11 or Wayland)

tetron-wm draws everything as cells, so it also runs on a raw Linux console. For colors and mouse there, use the maintained [kmscon](https://github.com/Aetf/kmscon) fork. One script can set it all up, including starting tetron-wm after you log in, and optionally autologin (never the default):

```sh
curl -fsSL https://tetronos.github.io/tetron-wm/install.sh | bash
```

It asks before every step, is safe to re-run, and `--uninstall` puts your login console back. Choose `--profile gui`, `console` (a login prompt, you start tetron-wm) or `login` (a login prompt, tetron-wm starts after you log in); it then asks "Do you want to log in automatically?" (default no, `--autologin` to say yes), and use `--dry-run` to preview the plan.

- Step-by-step: [docs/SETUP_tetron-wm_bare_console_kmscon.md](docs/SETUP_tetron-wm_bare_console_kmscon.md)
- Why each piece is needed, with diagrams: [the bare console guide](https://tetronos.github.io/tetron-wm/tetron-wm_bare_console_guide.html)
- Just kmscon: [install-kmscon.sh](install-kmscon.sh)
- Mouse on a plain Linux VT comes from the `gpm` service (`sudo apt install gpm`). Without a mouse, every window can still be switched (leader `Tab`, leader `f`) and moved or resized (leader `w`) from the keyboard, and the right-click menu of a file opens with leader `.`.

## Updating

Inside the app: **Settings → Updates**. The default channel installs the latest prebuilt release, reloads, and keeps your apps running. Or re-run the install command above. If you track the `dev` branch it builds from source.

On Debian and Ubuntu, tetron-wm installed by apt (the `.deb`) is updated through apt: Settings → Updates and the install command download the release `.deb`, check it, and let apt install it, so the packaged file is never overwritten. The `dev` channel is not used there, since a source build would put a second copy beside the package. A copy installed without the package keeps being updated in place; to switch it to the package, delete it and run the install command again.

## Troubleshooting

- **Colors look washed out.** Your terminal may do 24-bit color without saying so. Set `truecolor = true`, or run once with `TETRON_WM_TRUECOLOR=1 tetron-wm`.
- **Colors are wrong or unreadable on a basic terminal.** Pick the color depth in Settings → Appearance → Colors (Auto, Mono, 16, 256, Truecolor; config key `colors`), or force it per terminal: `TETRON_WM_COLORS=16 tetron-wm` (16 colors, the default on the bare Linux console), `TETRON_WM_COLORS=mono` (black and white, also set by `NO_COLOR`), or `256` / `truecolor`.
- **Nothing happens on `attach`.** Run `tetron-wm ls`. A `detached` session is waiting for you. `recoverable` means the daemon is gone but the apps are alive, so run `tetron-wm` to restore the windows.
- **Copying text out of an app that grabs the mouse** (vim, w3m): press `Ctrl+Space` then `x`, drag over the text, then `Ctrl+Space` then `C`.
- **No wallpaper.** Install `chafa`, or set `wallpaper_enabled = false` for a solid desktop.
- **Something else.** The in-app **Logs** viewer shows `~/.local/state/tetron-wm/debug.log`, which is what to paste into a bug report.

## Files

| Path                                         | What                                                                    |
| -------------------------------------------- | ----------------------------------------------------------------------- |
| `~/.config/tetron-wm/config.toml`            | your settings                                                           |
| `~/.config/tetron-wm/themes/`                | your themes and their pictures                                          |
| `~/.local/share/tetron-wm/`                  | app-managed data: clipboard history, icon positions, recent directories |
| `~/.local/state/tetron-wm/debug.log` (`.1` .. `.3`)    | log, rotated at 4 MiB, level from `log_level`                           |
| `$XDG_RUNTIME_DIR/tetron-wm-$USER/`          | sockets                                                                 |

## More

- [docs/CONFIG.md](docs/CONFIG.md): every config option, plus copy-paste recipes for common setups
- [config.example.toml](config.example.toml): a ready-to-copy config with every option at its default
- [docs/FEATURES_tetron-wm_what_it_can_do.md](docs/FEATURES_tetron-wm_what_it_can_do.md): everything tetron-wm can do, by task
- [docs/KEYS_tetron-wm_leader_key_table.md](docs/KEYS_tetron-wm_leader_key_table.md): the leader key table
- [docs/DEPENDENCIES_tetron-wm_recommended_installs.md](docs/DEPENDENCIES_tetron-wm_recommended_installs.md): optional programs per feature
- [docs/SETUP_tetron-wm_bare_console_kmscon.md](docs/SETUP_tetron-wm_bare_console_kmscon.md): the bare console setup
- [CHANGELOG.md](CHANGELOG.md): what changed in each release
- [agent/](agent/README.md): what the built-in AI assistant (the ✦ button) is told about the desktop
- [CLAUDE.md](CLAUDE.md): architecture and conventions for contributors and coding agents

Build and test from a clone:

```sh
cargo build
cargo test
cargo clippy --all-targets
```

## License

[MIT](LICENSE)
