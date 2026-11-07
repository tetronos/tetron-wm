# Changelog

All notable changes to tetron-wm are recorded here. The project uses
[semantic versioning](https://semver.org); while pre-1.0, minor versions may
carry user-visible feature work and the occasional breaking config change.

## [Unreleased]

## [0.15.3] — 2026-11-07

### Fixed
- macOS binaries build again: GitHub retired the macos-14 runner image (jobs were cancelled with "configuration not supported"), so 0.15.1 and 0.15.2 shipped Linux only. The release workflow uses macos-latest, like the other suite repos, so a retired image can not stop it again.

## [0.15.2] — 2026-11-07

### Changed
- The Start Page is now tetron-browser's bookmarks file `~/.config/tetron-browser/bookmarks.html` (one list for both; it works without tetron-browser): a Netscape bookmarks file (the format every major browser imports and exports) with the search box on top, instead of `bookmarks.txt` rendered to `start.html`. `tetron-wm-startpage` converts an existing `bookmarks.txt` once and keeps it; `add TITLE URL [FOLDER]` adds at the top of the folder, `list` prints "Folder | Title | URL". tetron-browser's home and bookmark buttons use the same file.

## [0.15.1] — 2026-11-06

### Added
- tetron-browser, the maintained continuation of carbonyl (the TetronOS web browser), comes first in a web page's **Open with…** list, is what the Start Page opens (else w3m), and has its own catalog entry.

### Changed
- The project moved to the tetronos GitHub organization: repository, release, install, catalog and docs links point to `github.com/tetronos/tetron-wm` and `tetronos.github.io/tetron-wm`. Old `github.com/ErikAllanKincaid/...` links keep redirecting; old `erikallankincaid.github.io` links do not (the Start Page's seeded tetron-wm bookmark in an existing `bookmarks.txt` is not rewritten yet).
- Short install command: `curl -fsSL https://tetronos.github.io/tetron-wm/install.sh | bash`. The docs site is published by a workflow that serves `install-tetron-wm-suite.sh` from the same commit as `/install.sh`; the long raw URL keeps working.
- The Start Page no longer opens carbonyl, even when it is installed: carbonyl crashes too often for a built-in item. It uses tetron-browser, else w3m.

### Fixed
- The Store installs carbonyl as `carbonyl@0.0.3-next.ab80a27`. The npm `latest` tag still points to an older build (0.0.2-next.bacf3db) that keeps one CPU core at 100% even when idle; upstream fixed this in 0.0.3.

## [0.15.0] — 2026-10-31

### Added
- `docs/INSTALL_tetron-wm_methods_and_sizes.md`: every way to install tetron-wm, what each size brings, the console setups, and how each is updated and removed; linked from the README.
- Settings > General > **Apps** shows the installed size and installs a larger one: from minimal, desktop or extras (Left and Right choose); at extras it checks for apps a release added. It runs the suite installer for apps only in a terminal window, so the sudo prompt is visible. When that window closes, the row shows the new size and the launcher lists the new apps.
- Settings > About shows how tetron-wm was installed (apt package, binary or cargo) and the installed size (minimal, desktop or extras).
- Releases include a `.deb` for Debian and Ubuntu (amd64 and arm64; Debian 12+, Ubuntu 22.04+), with `tetron-wm`, `tetron-wm-startpage`, `tetron-aimode` and the docs. It depends only on the C libraries the binary links, so installing it adds no other package; the tools that light up features (chafa, mpv, poppler-utils, python3, curl, ...) are listed as Suggests. Each `.deb` is tested by installing it in a clean Debian 13 container before it is attached to the release.
- `tetron-wm-desktop` and `tetron-wm-extras`, two more `.deb` files on each release (Architecture all). `tetron-wm-desktop` installs tetron-wm and recommends the tools its features use: mpv, chafa, poppler-utils, w3m, htop, upower, gpm, xclip, wl-clipboard, alsa-utils, openssh-client, sshpass, sshfs, 7zip, unzip, gnupg, python3, curl, console-setup-linux, fonts-dejavu-core, mc and vim. `tetron-wm-extras` adds fbi and git. Services (an SSH server, PipeWire, NetworkManager, BlueZ), kmscon, btop and age are only suggested, so installing either never starts a network service. Every name exists in Debian 12 and 13 and Ubuntu 22.04 and 24.04, and each release checks that the recommended tools install and that nothing suggested is pulled in.
- `tetron-wm config init` writes the example config to `~/.config/tetron-wm/config.toml` when there is none, and never overwrites one; `tetron-wm config example` prints it. The example is built into the binary, so it no longer needs to be downloaded or shipped as a file.
- Settings > Appearance > **Get more themes** downloads the theme files (`.toml` and their pictures) in the `themes/` directory on GitHub that are not yet in `~/.local/share/tetron-wm/themes/`, so a new theme can be picked without reinstalling. It runs in the background with progress in the row. GitHub allows 60 unauthenticated requests per hour and each click uses one; when they run out the row says "GitHub rate limit, try again later".
- Settings > **General** > Config file: **Open in editor** (the Editor from Default Apps), **Create from example** when there is no file, **Reset to defaults** (press Enter twice; the old file is kept as `config.toml.bak`) and **Reload**.
- Themes are also read from `~/.local/share/tetron-wm/themes/` and from `tetron-wm/themes/` under `/usr/local/share` and `/usr/share` (the XDG data directories), after your own `~/.config/tetron-wm/themes/`. A theme name found earlier wins and keeps its own picture. A relative `wallpaper` path is looked up in the same directories.
- Right-click **Compress…** and **Encrypt…** on files and folders, in Files and on the desktop. Compress writes `.7z`, `.zip` or `.tar.gz` beside the entry. Encrypt writes a password-protected `.7z` (AES-256, file names hidden, opens on any OS; the default, first in the list), an AES `.zip`, `.tar.gz.age` or `.tar.gz.gpg` (Linux, keeps permissions), whichever tools are installed. The tool asks for the password in a window, so it never passes through the command line, disk or the debug log; gpg asks twice and checks the archive. A failed or cancelled run removes the partial file; the original is kept. Hidden for folders on a remote system.
- **Extract here** decrypts `.gpg` and `.age` files (`.tar.gz.gpg` and `.tar.gz.age` straight into the folder) and opens AES zips with 7-Zip when installed. **List contents** of a `.7z`, `.zip` or encrypted archive runs in a window so the password prompt works.
- Right-click **Open with…** on a web page (`.html`, `.htm`, `.xhtml`) lists the installed text browsers (carbonyl, w3m, lynx, links, elinks, browsh) first, then the editors. Before, it offered editors only.
- Start Page and AI Mode ship with tetron-wm as `tetron-wm-startpage` and `tetron-aimode` (in `scripts/`; the prefix keeps generic names off `PATH`) and the suite installer installs them with the recommended tools, into `~/.local/bin`, with w3m. Both are in the catalog, so the launcher lists them (Web, AI) once they are on `PATH`, and the Store can install them. `tetron-wm-startpage` with no arguments now opens the page in carbonyl, else w3m (`tetron-wm-startpage build` only regenerates). AI Mode needs a free Gemini API key (https://aistudio.google.com/apikey): its first run asks for it, checks it with Google and saves it to `~/.config/aimode/key` with mode 600; `tetron-aimode --key` replaces it, and `tetron-aimode --help` prints usage instead of sending it as a question. Its default model is gemini-3.6-flash: gemini-3.8-flash often sent nothing for over 100 seconds, so every question waited out the 45 second limit and fell back to gemini-3.1-flash-lite (`AIMODE_MODEL` still picks any model). `tetron-aimode --strong` asks gemini-3.8-flash instead, waits up to 180 seconds for it, and falls back to gemini-3.6-flash. In the full screen app a **Strong ^T** button switches between the two for the next questions.

### Fixed
- The suite installer and Systems > Add Remote no longer install gpm on a machine without a mouse device (headless servers, many VMs), where Debian's gpm fails to configure and leaves a broken package and an apt error. gpm is the mouse for the machine's own text console; over ssh the mouse comes from your terminal.
- `install-tetron-wm-suite.sh --uninstall --purge` removes a packaged tetron-wm with apt (with its desktop and extras packages) instead of deleting `/usr/bin/tetron-wm`, which left a broken package.
- The suite installer no longer fails to install tetron-wm when run without a terminal (a container, a CI job, an ssh command): it took a readable `/dev/tty` to mean a usable one. casttui now installs on a host without git.
- Files: Enter on the **Move to Trash** confirmation opened the file instead of trashing it, Esc closed the whole Files window, and the dialog could not be answered with the mouse. The dialog now has **Move to Trash** and **Cancel** buttons; Enter trashes, Esc or a click outside cancels, and the arrows and clicks no longer reach the listing underneath. Error and Get Info boxes close on Enter, Esc or a click.
- Files: the preview pane no longer freezes the desktop on a PDF. It ran `pdftotext` on the whole document, with no time limit, on every redraw; it now reads the first page with a 3 second limit, once per file. A text preview reads only the first 64 KB, so a multi-gigabyte log previews instantly.
- Status readers that print more than 64 KB (a long `khal` agenda, many Bluetooth devices or Wi-Fi networks, a dense PDF page) no longer stall until their time limit and come back empty: tetron-wm now reads their output while they run.
- A screenshot that can not be saved now opens a window saying why, instead of failing silently (the reason only reached the debug log). Without console fonts the hint now names the right package: `console-setup-linux` on Debian and Ubuntu (it said `fonts-terminus`, which has no console fonts there).
- `tetron-wm launch @files [dir]`, `@settings`, `@store` and the other built-ins now open the built-in window, as the launcher does. Before, the name was run as a program and the window closed at once.

### Changed
- `install.sh` installs only tetron-wm and then prints the suite installer command for the apps; it no longer installs gpm, sshpass, chafa, unzip or a Nerd Font itself, and `TETRON_WM_INSTALL_DEPS` is gone. Updating a packaged tetron-wm (Settings > Updates or `install.sh`) also updates an installed `tetron-wm-desktop` or `tetron-wm-extras` from the same release, in one apt command, so tools added to a size reach existing machines.
- The suite installer asks how much to install: minimal (tetron-wm only), desktop (the tools its features use; the default) or extras (plus fbi and git); `--size` answers it. The apps are the same sets as the `tetron-wm-desktop` and `tetron-wm-extras` debs, read from one generated list, with the right package names on Debian, Ubuntu, Fedora, Arch and macOS. On Debian and Ubuntu, when tetron-wm is the apt package, the installer installs those debs so apt tracks the whole set; a tetron-wm installed without the package can be converted to it (asked, default no; `--convert`). fresh, uv, catt, casttui and the Symbols Nerd Font come with the desktop size. Themes now go to `~/.local/share/tetron-wm/themes/` and include every theme in the repository. A rerun only adds what is missing.
- Updating on Debian and Ubuntu follows how tetron-wm was installed. When apt installed it (the `.deb`), Settings → Updates and `install.sh` download the release `.deb`, check its SHA-256, and install it with apt; when a release has no `.deb`, nothing is changed. The `dev` channel and the source-build fallback are not used on a packaged install, so a second copy can not appear beside it. A copy installed without the package is still upgraded in place, and a fresh Debian or Ubuntu host gets the `.deb` once releases carry one. Systems → Add Remote follows the same rules on the remote.
- Settings → Updates → Check for updates runs in the background, so the desktop no longer freezes while it waits for GitHub (up to about 12 seconds offline). The row shows "Checking…" meanwhile.
- Settings opens on **Appearance**, now the first section; Windows is second.
- Installer: the kmscon console prompt now says what it does in plain terms ("Make kmscon the login screen on tty1 (Ctrl+Alt+F1)? It replaces the plain kernel text console, adding truecolor and mouse") instead of "Run kmscon on tty1 in place of the kernel login prompt?".

## [0.14.18] — 2026-10-26

### Fixed
- The macOS release builds failed to compile (`libc::ucred` and `SO_PEERCRED` exist only on Linux), so 0.14.17 shipped without its two macOS binaries. The full screen helper now reads the other end of its socket with `getpeereid` on macOS. Linux is unchanged.

## [0.14.17] — 2026-10-25

### Changed
- The wallpaper follows the theme. `wallpaper = "default"` now shows the active theme's own picture: the one built into the binary for midnight, gruvbox, pastoral and windows (all four are in the binary, about 134 KB), or, for a theme file, the image next to it with the same name (`dawn.toml` + `dawn.jpg`, also png and webp) or the file its `wallpaper` key names. Any other `wallpaper` value is your own pick and stays when you change theme. New values need no `chafa`: `black`, a `#rrggbb` colour, and `none` (the theme's `desktop_bg`). Below 256 colors (the 16-color console) `default` is the theme's `desktop_bg`, because a picture turns to noise there; a file you chose is still drawn. Bare file names are looked up in `~/.config/tetron-wm/themes/` first, then the config root, then the old `wallpapers/` folder (still works, no longer documented).
- Built-in themes are now midnight, gruvbox, pastoral and windows (new, the Windows 3.1 look), and midnight is the default theme. newmoon, nord and dracula are no longer built in: they come with the extra themes the suite installer downloads into `~/.config/tetron-wm/themes/` (nord, dracula, dawn, newmoon and the new high-contrast `contrast`, with their pictures; `--themes` / `--no-themes`, on by default, never overwrites a file you have). The themes folder is read again when something in it changes (checked once a second), so a theme or picture dropped in while tetron-wm runs is found without a restart; the theme in use keeps its colours and picture until it is chosen again. A config that names a removed theme shows midnight and a note in Settings → Appearance until the theme file is there; `theme --dump` only writes built-ins. The 16-color look of every theme is unchanged.
- The default wallpaper picture is now midnight's (`themes/midnight.jpg`) instead of the old `assets/wallpaper.jpg`.
- Suite installer: the default setup on a console-only machine is no longer the autologin appliance. The menu gains a `login` profile (kmscon, a normal login prompt, tetron-wm starts after you log in) and makes it the default. Autologin is a separate question, "Do you want to log in automatically?", default no, asked after the console choice; `--autologin` / `--no-autologin` answer it, and the `appliance` profile (autologin plus autostart) is only for scripts and is not in the menu. A warning is printed before autologin is set. To remove an autologin that an earlier run set up, rerun with `--profile login` and answer no.
- The contrast theme sets its own 16-color selection (`[vt16] active_bg = 7`, light grey), so the selection and panel buttons are grey on the console instead of teal, and it now uses a white accent. Themes in the downloadable pack may carry a `[vt16]` table like any other theme file.
- Settings → Appearance → Colors calls the no-color choice "Mono", matching the `mono` config value and `TETRON_WM_COLORS=mono` (it was "Black and white").

### Fixed
- A command that ran past its time cap was killed but never reaped, so it stayed behind as a defunct (zombie) process of the daemon until it restarted. On a machine where `nmcli` hangs (a wedged NetworkManager), every poll leaked one, about 150 in five hours, filling the process table. Timed-out commands, and a clipboard helper that failed or timed out, are now reaped on a background thread.
- `tetron-wm theme NAME` (and a client attaching with `TETRON_WM_THEME`) now redraws the wallpaper when it follows the theme (`wallpaper = "default"`). It kept the old theme's picture until the next Settings change or restart.

### Added
- More theme file keys, so a theme can restyle what was fixed before. `success` (installed, verified, on, update available), `error` (errors, kill, low-contrast warning), `accent_fg` (text on an `accent` fill, the "+" button) and `icon_fg` (desktop icon labels over a wallpaper) are optional colors; unset, they are derived from the rest of the file (a deeper green on a light `window_bg`, `close_fg` for errors, white on the accent unless that would be unreadable, white labels), so existing theme files look as before except where the old fixed green or white was unreadable. A new `[ansi]` table sets the 16 colors hosted apps see (`black red green yellow blue magenta cyan white` and `bright_` forms; any subset, also answered to apps that query the palette) and running apps repaint on a theme switch. `[vt16]` accepts the four new roles too, `theme --dump` writes them (the ANSI palette commented out), and the keys are listed in `--help` and `docs/CONFIG.md` (Theme file keys). App server protocol v6 (the new `ansi` field is ignored by an older app server, no restart needed).
- A theming guide, `docs/tetron-wm_theming_guide.html`: a picture of the desktop with every theme key numbered (switchable between five themes), real screenshots of the four built-in themes (`docs/images/`, made by `scripts/theme-screenshots.py`), the key reference, building a theme step by step, `[ansi]`, `[vt16]`, the readability checks and troubleshooting.
- New config key `fullscreen` (default true) and Settings → Windows → Full screen video/images: off hides the full screen menu items and the viewer `f` key, and the client skips the `/dev/fb0`, DRM and waiter probes. Takes effect for the probes on the next attach. A second row, Full screen helper service, shows whether the waiter service is installed, running or stopped; Enter runs `sudo systemctl enable --now` or `disable --now` on the real terminal (the sudo prompt works there), so the helper can be switched off to save the process and tty9, and back on without rerunning the installer.
- Play full screen (video) and View full screen (images, PDFs, and the viewer's `f` key) work under kmscon, with no sudo. The suite installer can set up a full screen helper (`--fullscreen`, `--fullscreen-vt N`, default tty9; default no, removed by `--uninstall`): a systemd service running `tetron-wm fullscreen-waiter` as your user in its own login session on a spare VT. Play full screen asks it to play the file; it switches the seat there through logind, runs `mpv --vo=drm` with hardened options (no user config or scripts, no youtube-dl, no playlist references) or fbi, fim or fbgs for images and PDFs, and switches back when the program exits. The request names only a kind and absolute files; the helper picks the program. The route follows the hardware and the installed programs, on a plain console too: video mpv over DRM else cvlc on the framebuffer; images fbi, fim, then mpv over DRM (the viewer's keys); PDFs fbgs with gs, else pdftoppm pages through mpv. A machine with no `/dev/fb0` (the NVIDIA driver) takes the DRM routes, and the right-click item appears only when a route exists. The request socket is owner-only and the peer uid is checked.
- Right-click Cast to Chromecast on video, audio and image files (desktop and Files), shown when catt is installed. It runs `catt cast` in a window: catt serves the file from this machine until playback ends, and closing the window stops the cast. New config key `cast_device` (a device name or IP for `catt -d`; empty uses catt's default device).
- Right-click Open with… casttui for video and audio files when casttui is installed: a window with play/pause, seek, volume and stop for the cast (casttui also converts codecs a Chromecast cannot play, which Cast to Chromecast does not). casttui now takes the file as an argument and casts it as soon as a device is connected.
- The suite installer offers uv, catt and casttui with the recommended tools: uv from the package manager (its official installer on Debian and Ubuntu), then `uv tool install` for catt and casttui.
- New `[open_with]` config table: extra programs for the right-click Open with… menu per file role (`video = ["casttui"]`), always listed after the detected ones and also offered in Settings → Default Apps.
- The Info menu opens from the keyboard with leader `i` (new `[keys]` action `info`): arrows or Tab move through its rows, Enter opens one (CPU, Memory, Bluetooth, Sessions, Screensaver, Logs, Shortcuts, Settings), Esc closes. Settings → About now shows the version and git commit, the session, the app server protocol, and where the config, log and clipboard history live.

## [0.14.16] — 2026-10-23

### Added
- PDFs open in the built-in viewer, one page at a time, on every terminal (rendered with poppler's `pdftoppm`; characters by chafa, or the real page with Kitty graphics). `n` / `p` turn pages, zoom and pan read small print, and the title shows the page. Open with… also offers the text in a pager (`pdftotext -layout`, searchable with `/`), and `tdf` and `timg` when installed; on a plain Linux console, View full screen (and `f` in the viewer) uses `fbgs`. The suite installer's recommended tools include poppler.
- Right-click menu, more actions: Extract here and List contents for archives (tar, zip, 7z, rar and single gz/xz/bz2/zst files, in a window that shows the result), Get Info on the desktop (opens Files with the item's info), Send to… a saved Systems entry (scp in a window, so password and host-key prompts work), and Play full screen for video on a plain Linux console (`mpv --vo=drm`, or `cvlc --vout fb`; the same handoff as View full screen). With no `[default_apps]` handler set, Open plays video and audio in the window with mpv instead of asking.
- A window's title bar has a right-click menu: Minimize, Maximize, Move (keyboard move mode), Rename and Close, with the same keys and hover as the file menu.
- Color depth in the app: Settings → Appearance → Colors (Auto, Black and white, 16 colors, 256 colors, Truecolor), new config key `colors`. It applies at once, with no detach or reattach: the theme adapts, the wallpaper is redrawn for the new palette, and the screen repaints. Auto keeps the detection from before; `TETRON_WM_COLORS` set for a client still wins. The Settings window is wider (up to 84 columns) so values and the hints under the rows are no longer cut off.
- Right-click menu: "View full screen" on images hands the screen to fbi (or fim) with the folder's images, the chosen one first, and comes back to the desktop when you quit it (`q`); the apps keep running. It is offered only on a plain Linux console with a writable framebuffer and fbi or fim installed, since under kmscon, a GUI terminal, ssh or tmux fbi draws nowhere visible. `f` in the image viewer does the same. Also new in the menu: "Set as wallpaper" (images), "Copy path" (into the clipboard and its history), and "Open terminal here" (the file's folder, the folder itself, or the desktop folder from the empty desktop).
- One right-click menu for the desktop and Files: the same look, keys and actions in both. Arrows (or the mouse, which now highlights the row under it) and Enter pick, Esc closes, and it stays on screen near the edges. Leader `.` opens it from the keyboard for the selected file in Files, or on the desktop when no window is focused (new `[keys]` action `context_menu`). "Open with…" now works and fits the file type: the image viewer for images, `mpv --vo=tct` (video drawn in the window on any terminal) and `cvlc` for video, `mpv --no-video` for audio, editors and pagers for text (fresh, nvim, vim, vi, hx, micro, nano, `emacs -nw`, less, bat), plus a GUI player window when a display exists; it runs the choice; before, it did nothing on the desktop and showed an empty box in Files. Files' "Delete" is named "Move to Trash", which is what it does. Settings → Default Apps offers the same per-type choices.
- The suite installer's recommended tools include the fresh editor: Homebrew `fresh-editor` on macOS, and on Linux (where no distro packages it) fresh's own official installer, run only when fresh is missing and you accept the recommended tools.
- The suite installer's recommended tools include mpv (apt, pacman, Homebrew; on Fedora it comes from RPM Fusion), which plays video inside a tetron-wm window on any terminal with `mpv --vo=tct`.
- The built-in image viewer works on every terminal. It uses Kitty graphics where the terminal supports them (kitty, Ghostty, WezTerm) and otherwise draws the image as colored cells with chafa, dithered on 8 and 16 colors, so images show on a bare Linux console, kmscon, tmux and over ssh; without chafa it falls back to half blocks. The image keeps its aspect ratio (Kitty graphics used to stretch it over the whole window). Zoom with `+` / `-` or the wheel, `0` fits, the arrows or a left-button drag pan, `n` / `p` step through the images in the folder, `m` switches graphics / cells, `i` toggles an info line, `Esc` closes. Decoding and drawing run in the background, so a large image no longer stalls the desktop while it opens. New config key `image_viewer` (`auto`, `graphics`, `cells`).

### Fixed
- `tetron-wm reload` after the binary was replaced on disk (a manual install or package upgrade while running) no longer leaves you without a desktop. tetron-wm restarted itself from the path Linux reports for a replaced binary, "PATH (deleted)", which does not exist; it now uses the newly installed binary at the same path.
- Opening a file with fresh (Open with…, or `[default_apps]` set to `fresh`) adds it as a new tab in the fresh window that is already open and focuses that window, instead of starting a second fresh (needs fresh 0.5 or later, which lists its running windows; older fresh still gets a new window). A new fresh window starts with `--restore`, so the folder's saved tab list comes back with the file added instead of being replaced by it; an explicit `--restore` or `--no-restore` in the handler is kept.
- The right-click menu on the desktop is opaque. Its background was slightly translucent, so the wallpaper's characters showed through and made the items hard to read.
- Images drawn by chafa (the wallpaper and the new image viewer) no longer show black cells at the start of some rows. chafa draws a solid cell as inverse video plus a space, and the inverse was ignored.
- Kitty graphics images sent by an app in a window are placed where the app drew them even when it writes more text in the same output burst; before, the image landed where the cursor ended up after that text.

## [0.14.15] — 2026-10-22

### Added
- New config key `scrollback`: lines of scrollback kept per app window, default 10000 as before, 0 to 100000. It is the main memory cost of a busy window (two full 10000-line windows took about 40 MB; 1000 lines, about 4 MB). Read when a window opens, so a change applies to new windows without restarting the app server.
- The suite installer offers the recommended tools (chafa, sshpass, htop, btop, upower, alsa-utils, and xclip or wl-clipboard on a desktop; default yes) and, as a separate question that defaults to no, the system services PipeWire, NetworkManager and BlueZ. Only missing packages are installed, with apt, dnf, pacman or Homebrew. Flags `--extras` / `--no-extras`, `--services` / `--no-services`; `--dry-run` lists what would be installed.
- The suite installer's recommended tools include the DejaVu Sans Mono font when it is missing (checked with `fc-list`), so the default icon set has every symbol it draws on Linux terminals as well as kmscon.
- README lists recommended extra packages, and `docs/DEPENDENCIES_tetron-wm_recommended_installs.md` explains per feature which outside program tetron-wm uses, what happens without it, and the macOS equivalent.
- Settings has a Sound section: output volume (Left / Right, the mouse wheel, or a click on the bar), mute (Space), the output device (Left / Right cycles the machine's outputs, for example HDMI to a monitor, headphones or speakers; PipeWire `wpctl set-default`), and the mixer link from the volume popover. These act on the machine at once and are not saved to the config.
- Volume from the keyboard. Leader `=` opens the menubar volume popover; Left / Right (or `-` / `=` / `+`) change the volume, `m` or Space mutes, Esc closes. The keys also work when the popover was opened with a click. The popover gains a row that opens a mixer program: the first installed of wiremix (in ASCII mode on a bare console), pulsemixer, ncpamixer and alsamixer, configurable as `[tray_tools] volume`; with none installed, Linux offers `Get wiremix (Store)`. New `[keys]` action `volume`.
- Keyboard move and resize. Leader then `w` puts the focused window in move mode, with a key hint in its title: arrows move it one cell (`Shift`+arrow five), `r` switches to resizing the right and bottom edges, Enter keeps the result and Esc puts the window back exactly as it was, maximized or snapped state included. New `[keys]` action `move`.
- Keyboard window switching. Leader then `Tab` focuses and raises the next window in taskbar order (leader then `Shift+Tab` the previous one); further bare `Tab` / `Shift+Tab` keep cycling without the leader until Enter, Esc, a click or any other key. Minimized windows are skipped unless all are minimized. Leader then `f` opens a window picker: a numbered list of every window, minimized ones included, where arrows or `Tab` move, Enter or `1` to `9` focuses and restores the window, and Esc closes. New `[keys]` actions `focus_next`, `focus_prev` and `windows`, and `backtab` is accepted as a key name.
- Color depth is one setting with four levels: black and white (`mono`), 16, 256 and truecolor. It is detected (the kernel console `TERM=linux` gets 16; `NO_COLOR` or a vt100-class `TERM` gets black and white) and can be forced with `TETRON_WM_COLORS=mono|16|256|truecolor`. The client tells the daemon its depth on attach, and the theme adapts by role instead of being quantized: in 16 colors the desktop, windows, menubar and the selection stay distinct (previously they all collapsed to black and the selection was invisible), using only the 8 backgrounds a VT can show; in black and white no color is sent at all, and the selection and focused title are drawn in reverse video, secondary text dim and the accent bold. Every 16-color fg/bg pair is checked and unreadable pairs switch to black or white text. The wallpaper asks chafa for the matching palette (8 colors on a VT, 16, 240) and is skipped in black and white. Detaching and attaching from a different terminal re-adapts.
- Each theme's 16-color version keeps the theme's own hues where it has them (a teal desktop stays cyan, navy title bars stay blue, pastoral stays green), while windows follow the rule dark theme black, light theme grey and the close button stays red. A theme file can pin any role with a `[vt16]` table (`desktop_bg = 6`), and `tetron-wm theme --dump` writes a commented example.
- `icon_style` gains `unicode` (richer symbols, all present in DejaVu Sans Mono, the kmscon font and the usual GUI fallback) and `auto`, the new default: `ascii` on a bare Linux console, `unicode` everywhere else, decided from the attached terminal and re-decided when a different terminal attaches. GUI and kmscon users get the richer window buttons, markers, file-type and status icons back without configuration; the console stays tofu-free. An explicit `icon_style = "ascii"` (as in the old example config) is kept as is.

### Fixed
- `tetron-wm -s NAME launch CMD` launched a program called NAME instead of CMD; `screenshot`, `theme` and `msg` also read their arguments from the wrong position after `-s NAME`. All now use the arguments after the command.
- `tetron-wm ps` and `kill-app` no longer hang while a desktop is attached. The app server serves one frontend connection at a time, so these now use its new control socket `apphost-ctl.sock`; against an older app server they time out with a message instead of waiting forever.
- Pasting (Ctrl+Shift+V, leader `v`, middle click, Shift+Insert) now works in Settings text fields such as the wallpaper path and terminal colours; it used to go nowhere.
- Leader shortcuts work while the Shortcuts help, the Logs viewer, the launcher or the folder picker has focus. The leader key was swallowed there, so for example leader `,` did nothing with the help window open. The help window now lists the store (`S`) and settings (`,`) on separate rows.
- Settings sections can be reached from the keyboard. Like mc's panels, `Tab` moves focus between the section list and the settings rows, and Up / Down move inside the focused pane (in the list, Right or Enter also opens the section). PageDown / PageUp switch section from either pane. Before, the list could only be clicked. The sidebar shows the keys.
- The mouse pointer drawn by tetron-wm (the only pointer on a bare console with gpm) now follows the mouse inside a focused app that uses the mouse (mc, fresh, htop); it used to stay behind at the window edge.
- The pointer is a solid white cell on dark backgrounds and black on light ones, so it no longer vanishes over wallpaper, image art, same-colour cells or, on the bare console, inverted bright text. Reverse video is now applied by swapping colours before they are reduced to the terminal's palette, so a bare console never gets a bright background it can not show.
- Apps that ask the terminal for its colours (OSC 10/11/12, OSC 4) now get the theme's foreground, background and palette, so editors and prompts that choose a light or dark scheme from the terminal background match the desktop theme. Needs an app server restart (`tetron-wm kill`, then start again) to take effect.
- Forcing `TETRON_WM_COLORS=256` or `truecolor` on the bare Linux console is honoured, and text stays readable: tetron-wm predicts how the kernel reduces those colours to its 16 and switches unreadable text to black or white (the `windows` theme's grey Settings labels on white were invisible). Colours the kernel would lose (it decodes 256-colour cube indices at about half brightness, so the `windows` theme's teal desktop showed black) are sent as the basic index it reads correctly.
- The Info menu is drawn as `≡` instead of a plain `?`, which read as a missing glyph.
- The default `ascii` icon style now draws only characters present in the Linux kernel console fonts and DejaVu Sans Mono (kmscon). Window buttons (close, maximize, restore), the menubar toggles and assistant button, the Info menu, clipboard/store/launcher markers, on/off dots, file-type and status icons no longer show missing-glyph boxes on a bare tty. The `nerd` and `emoji` styles keep the richer symbols. A test fails if an unsafe glyph is added to the UI code.
- Wallpaper (chafa) on the `ascii` style uses space, full block and shades on the kernel VT (`TERM=linux`, whose fonts have no half blocks) and half blocks plus quadrants elsewhere. Previously `all-wide` emitted about 230 different glyphs, over 60% of cells in glyphs DejaVu lacks, so kmscon showed tofu or clipped cells. Dropped the `--dither` flag, which chafa ignores in truecolor.
- Window-count badges above 3 use the `·N` form (superscripts above ³ are missing from console fonts).
- The launcher warning label on variant entries (Claude Code skip-permissions) now follows `icon_style`: `[!]` for ascii (default), a Nerd Font glyph for nerd, the emoji only for emoji.

### Changed
- Far fewer bytes written to the terminal: the client no longer repositions the cursor before every cell (consecutive cells on a row need no move; other moves use the shortest form) and no longer repeats colour codes that are already in force. A full-screen redraw of plain text went from about 440 KB to 10 KB, a typical busy frame to about a fifth, so the hosting terminal (or tmux, or ssh) has much less to parse. The screen drawn is the same; after emoji and other glyphs whose width terminals disagree on, the next cell is still positioned absolutely. Each frame is now one write.
- Lower CPU under heavy terminal output: the app server sends each changed window grid to the desktop as compact run-length rows, only the rows that changed, instead of a JSON object per cell (a full 200x50 grid went from about 1.5 MB and 20 ms to encode and decode to about 120 KB and 0.7 ms). App server protocol v5. Either side older than v5 keeps the previous format, so an app server left running from an older version keeps working.
- Wallpaper: `wallpaper = "default"` is the built-in image and the new default. An empty value, or a configured file that is missing, also shows the built-in image instead of a blank desktop (the log says why, and Settings → Appearance shows "Not found" under the rows, or that chafa is missing). A solid desktop is `wallpaper_enabled = false`. Clearing the Wallpaper file field in Settings writes `default` back, and when the field shows `default` the first key typed replaces it. Configs that still say `wallpaper.jpg` keep working.
- Wallpaper paths are cleaned before use: surrounding quotes, `file://` URLs (with `%20` decoded) and a leading `$HOME` are accepted, so a path pasted from a shell or file manager works.
- The built-in wallpaper copy is now `~/.local/share/tetron-wm/wallpaper.jpg` (the old `default-wallpaper.jpg` is removed) and is rewritten whenever the shipped image changes, not only when its size changes.
- The suite installer and the kmscon installer moved to the repo root next to `install.sh`: `install-tetron-wm-suite.sh` and `install-kmscon.sh`. The old paths (`contrib/install-tetron-wm-suite.sh`, `scripts/install-kmscon.sh`) remain as small redirects, so existing links and one-liners keep working.
- Much less idle CPU (6 idle shells at 200x50: about 18 % of a core down to about 2 %, measured). The daemon no longer re-composites or re-sends an unchanged screen and slows its tick to 50 ms after half a second without changes, waking at once on input; the app server only snapshots windows that produced output and sleeps until output arrives instead of polling every 16 ms. Compositing is about 2.5 times faster (same output), and plain app output skips the image-escape scan.
- Idle CPU cut further (6 idle shells: daemon about 2.2 % down to 0.7 %, the tray's helper programs about 3.8 % down to 1.3 %, measured). App output, control commands such as `tetron-wm launch` and client input now wake the daemon directly, so its idle tick is 250 ms instead of 50 ms with no added latency (output and `launch` show up sooner than before). The menubar clock runs `date` once a minute instead of three times a second, and WiFi, network and Bluetooth status refresh every 10 s instead of every 3 s (at once when shown, on attach or after a tray action); volume still refreshes every 3 s.
- Less CPU drawing busy screens. The daemon now sends changed cells to the client as runs of same-styled text instead of one JSON object per cell (about a tenth of the bytes, and about 6 times faster to encode and decode on a noisy 200x50 frame), and the client builds the terminal output without per-cell allocations (about 3 times faster, the same bytes). A new client announces that it reads runs; an older client or an older daemon keeps the previous format, so mixing versions across a reload still works.

## [0.14.14] — 2026-10-19

### Added
- **Screensaver**, on demand: leader `z`, launcher → tetron-wm → Screensaver, a Screensaver link in the ⓘ menu, or `tetron-wm screensaver [on|off|toggle|list] [effect]` (works from another shell or over ssh). Any key or mouse event in the attached client wakes it, and that event is not passed to the focused app; attaching also wakes it. Effects: `black` (plain, free) and `clock` (a big animated `HH:MM:SS` with a blinking colon and the date, drifting around the screen to spare burn-in). Pick the default with `screensaver = "..."` in the config. It covers the cells only: the monitor stays on, there is no idle timer, and it is not a lock. New effects are one `Effect` implementation plus a registry line (`src/screensaver.rs`).

- `power_menu = false` in the config removes launcher → Power and refuses Suspend, Reboot and Power off everywhere, for servers that must never be shut down from here (default `true`).

### Fixed
- The mouse wheel now scrolls the Working directory picker (it only worked from the keyboard).

### Changed
- `tetron-wm --help` now explains the session workflow: an OPTIONS block for `-s/--session`, a SESSIONS section (detach, come back, `attach -d`, stop, named sessions, what each `ls` state means, `launcher_label = "@session"`), a launcher system section summary (Systems, Detach, Reload, Quit, Power), the socket directory, and the `shutdown` key now reads "quit tetron-wm" instead of "shut down the daemon". A test keeps these topics in the text.

## [0.14.13] — 2026-10-18

### Added
- **Power** submenu at the bottom of the launcher with **Suspend**, **Reboot** and **Power off** for the computer. Each asks first, naming the host and how many running apps would be lost. They work across systemd, elogind and Alpine/busybox setups by trying `systemctl`, `loginctl`, `sudo -n` / `doas -n` and the bare command in turn (the first that succeeds wins); a refused action opens a window listing what was tried. `[power_commands]` in the config replaces a chain, and an empty list hides the action; actions the computer cannot do are not shown.
- `launcher_label` config (and Settings → Appearance → Launcher label): the text of the menubar launcher button. Up to 6 characters, centred in a fixed width so nothing else on the bar moves; empty falls back to `tetron`. `@session` shows the session name in a named session, so the bar tells you which session you are in. Use single-width characters (CJK and emoji are two cells wide and will overflow).
- The `ⓘ` Info menu in the tray: session name, window count, version, links to a live Sessions window (`tetron-wm ls`), Logs, Shortcuts and Settings, plus the tray segments that are not on the bar.
- `[tray_placement]` config: put each tray segment on the `bar`, in the `info` menu, or `auto` (on the bar, overflowing into the menu when space runs out). **Behaviour change:** CPU, memory and Bluetooth now live in the Info menu by default; pin them back with `cpu = "bar"` (and `mem`, `bluetooth`). A CPU or memory reading of 90% or more still shows on the bar. Tray segments that do not fit a narrow terminal now move into the Info menu instead of silently disappearing; the clipboard, volume and clock are never moved.
- `tetron-wm attach -d` takes over the session from a client already attached (that client exits with "detached by another client"). A plain `attach` while another client is attached is now refused with a message instead of hanging on a blank screen.
- Named sessions: `tetron-wm -s NAME` (or `attach -s NAME`, or `$TETRON_WM_SESSION`) runs a separate daemon, apphost, and set of apps. `tetron-wm ls` lists them with a state (`attached`, `detached` = running with no client, `recoverable` = daemon gone but apps alive, `stale`) and window count; cleanly stopped sessions are not listed. App windows inherit the session, so `tetron-wm launch`/`clip` run inside a window reach that window's session. The unnamed session is `default` and keeps its existing socket paths.

### Changed
- Launcher system section renamed and regrouped: **Exit** is now **Detach**, **Restart** is **Reload**, **Shutdown** is **Quit** (it only ever stopped tetron-wm, never the computer), with a divider between Systems and them. All keep their confirm dialogs, and the internal ids (`@exit`, keymap `detach` / `shutdown`) are unchanged so existing key remaps keep working.
- The keyboard shortcuts (leader `?`, launcher, Info menu) now open in a real window instead of a modal overlay: it stays open as a reference, you can move and resize it, and the launcher, other windows and the menubar keep working while it is up. `?` or the launcher entry refocuses it; `Esc`, `q`, `Enter` or `?` close it when it is focused (other keys are ignored), and the arrow, page and wheel scroll it.
- The background pollers (CPU, battery, Wi-Fi, volume, Bluetooth, calendar, remote reachability, tetron) now pause while no client is attached, so a detached session costs almost nothing, and refresh at once when you attach. Wi-Fi/network and Bluetooth are only read while something that shows them is on screen (on the bar, or its popover open); opening one refreshes it immediately.

### Fixed
- The taskbar `…+N` marker (shown when too many windows to list) is now clickable: it opens a dropdown of the hidden windows, and picking one focuses it. Before, the hidden windows could not be reached from the taskbar. The same fix repairs the grouped-window dropdown: clicking one of its rows over a terminal window used to start a text selection instead of picking the row.
- On a narrow terminal the status tray no longer draws over the launcher, view-mode, assistant, and "+" buttons. Tray items are now shed (lowest priority first) as soon as their first cell would reach the left buttons, instead of the tray overwriting them.
- The Wi-Fi/network tray segment could disappear: with the new demand-based reads it was only read once it was already on the bar, so it never appeared. Wi-Fi and Bluetooth are now read until their segment exists.
- The Info menu popover was one row too short, so its bottom border cut through the "Settings" link.

## [0.14.12] — 2026-10-14

### Added
- Leader `Esc` closes the focused window (keyboard-only close; app windows still ask for confirmation first). Rebind with `close = "..."` in `[keys]`.

### Fixed
- The clipboard history popover scrolls with the mouse wheel (the highlight moves, the list follows).

### Changed
- Leader keys follow common conventions. Moved: paste `p` → `v` (`p` still pastes), clipboard history `v` → `C`, screenshot `P` → `s`, store `s` → `S` only. Dropped: launcher `a` (use `l` or `Enter`), screenshot `P`. New: `y` also starts select mode. Beware: a leader `v` that used to open the clipboard history now pastes. Overrides in `[keys]` are by action name and keep working.

## [0.14.11] — 2026-10-13

### Added
- `tetron-wm screenshot [path.png]` saves a PNG of the whole composited screen, for kmscon and bare-VT use where no host screenshot tool exists. Needs an attached client. Glyphs come from an installed Terminus PSF console font (`fonts-terminus`); block, quadrant, braille and sextant cells are drawn exactly, icon glyphs the font lacks show as `?`. Also on the leader key `P` and launcher → tetron-wm → Screenshot (saved to `screenshot_dir`, default `~/Pictures` or `$HOME`).

### Fixed
- Cancelling a Shutdown/Restart/Exit confirmation no longer pops the retired Exit/Restart/Shutdown dropdown in the top-right corner; cancelling the forget-system dialog or the Add Remote form no longer shows it either.

## [0.14.10] — 2026-10-13

### Fixed

- Saving the config no longer wipes your comments or reverts hand edits. The app now writes only the keys it changed into the existing `config.toml`, keeping comments, key order, commented-out blocks and unknown keys, and a hand edit to another key survives even while the daemon is running. Nothing is written when nothing changed (the theme message sent on every client attach and idle Settings clicks used to rewrite the file), the write is atomic, and the previous file is kept once per session as `config.toml.bak`.
- A `config.toml` with a syntax error is no longer overwritten with defaults at the next save; the error is logged and the file left alone until it parses again.

### Changed

- Desktop icon positions and recent working directories moved from `config.toml` to `~/.local/share/tetron-wm/state.toml`. Old values are migrated once and removed from the config, which is now only what you set. The Settings panel can no longer revert them.
- `tetron-wm --help` lists the `theme` default as `newmoon`.

### Added

- Clipboard popover has a **[Select]** button that starts select mode with the mouse (same as `Ctrl+Space x`), and clicking the `SELECT` marker in the menubar cancels it.

## [0.14.9] — 2026-10-13

### Fixed

- Live terminal colors now work with theme files: the daemon sends the resolved background and foreground to the app server instead of a theme name the app server could not look up (it scans the themes dir only once, at start, so a theme file added later silently fell back to midnight). No protocol change; 0.14.8 app servers keep working.

## [0.14.8] — 2026-10-13

### Added

- Live terminal colors: `terminal_bg`, `terminal_fg` and the desktop theme now apply to running apps without an app-server restart (new `SetColors` request, apphost protocol v4; older app servers keep their start-up colors, and Settings says so). Switching the theme now recolors terminal contents immediately too.
- Settings → Terminal: color presets (follow theme, dark, light, black, solarized dark/light, gruvbox dark, nord, dracula, green/amber phosphor), hex or named-color entry for background and foreground, and a Preview / Save / Revert flow (a preview is never written to disk; closing Settings mid-preview reverts it).
- Settings panel surfaces more keys: Natural scroll (Windows), Icon style and Desktop icons (Appearance), Expire entries after and Max item size (Clipboard). The Settings window is taller to fit.
- Default theme is now `newmoon` (built into the binary; a `newmoon.toml` in the themes dir still overrides it). Configs that set `theme` are unchanged.
- Built-in default wallpaper (70 KB, embedded): used when the default `wallpaper.jpg` is not on disk, so a fresh install has a wallpaper with no setup (still needs `chafa`).
- Network tray item: the label is `wifi` (with signal bars) on a WiFi link or `net` on any other link, never the SSID. The popover shows the SSID, the IP and interface of the default route (click copies the IP), `Offline` when there is no link, and an `Open` row (`wlctl`, `nmtui`, `iwctl`, else `ip -br addr`; macOS `networksetup`/`ifconfig`). The item now also appears on wired-only hosts.
- Battery popover shows time left / to full, and its `Open` row falls back from `jolt` to `upower` (first battery device) then `pmset`.
- Bluetooth popover `Open` row: `bluetuith`, else `bluetoothctl`, else `blueutil --paired` (macOS).
- `[tray_tools]` config table: override or disable the `Open` command per tray item (`cpu`, `mem`, `battery`, `bluetooth`, `network`); first installed candidate wins, `hold:` keeps one-shot output open.
- `web_home` config key (default `https://lite.duckduckgo.com/lite/`): the start page for text browsers launched with no URL.

### Changed

- Menubar tray popovers gain an action row that opens a real tool in a window: CPU offers `htop` (or `top` when htop is absent), Memory offers `free -h` (or `vm_stat` on macOS, held open until Enter), Battery offers `jolt` when installed. With no tool available the popover is unchanged. The WiFi label is truncated to 6 characters.
- Bluetooth tray glyph in the ascii and emoji icon styles is now ≋ (U+224B) instead of ⏻, which read as the power symbol and is missing from most monospace fonts; DejaVu Sans Mono covers ≋.
- Windows no longer draw a left or bottom border: content now reaches the left edge and the bottom row, reclaiming a column and a row. Resize is by the right edge (including the bottom-right corner) only.

### Fixed

- Launching w3m with no URL (launcher > Web > w3m, or `tetron-wm launch w3m`) opened and closed a window instantly because w3m prints its usage and exits; it now opens `web_home`. Catalog entries can opt in with `"needs_url": true`.

## [0.14.7] — 2026-10-11

### Added

- **Clipboard manager.** A history of everything copied (app OSC 52 stores, Ctrl+Shift+C, mouse selections, the CLI, the desktop clipboard), saved to `~/.local/share/tetron-wm/clipboard-history.jsonl` (0600; switch off with `clipboard_persist = false`). Menubar clipboard segment with a popover (filter, paste, copy, pin, delete, pause; mouse and keyboard), leader `v`, launcher entry, Settings → Clipboard.
- OSC 52 clipboard **reads** are answered (`clipboard_osc52_read`), so apps such as vim's osc52 package can paste. Apphost protocol v3 (additive, no forced restart).
- `tetron-wm clip copy|paste|list|clear|pin|delete|copy-file|setup-mc`, and `xclip` / `xsel` / `wl-copy` / `wl-paste` shims on the PATH of launched apps when there is no display (`clipboard_shims`).
- `clipboard_fake_display` (opt-in): a dummy `WAYLAND_DISPLAY` for launched apps on a console so Neovim picks the clipboard shims.
- **Select mode** (leader `x`): a one-shot mode where the mouse is not passed into apps, so text can be selected out of w3m, carbonyl or vim without relying on Shift (which most desktop terminals intercept). A `SELECT` marker shows in the menubar. The Help overlay (and `--help`) has a Clipboard section explaining how to copy text out of w3m, carbonyl or vim.
- GUI clipboard bridge for X11 (`xclip`) and Wayland (`wl-clipboard`): `clipboard_gui_bridge`.
- Pastes are bracketed when the app enables bracketed paste (multi-line text no longer runs line by line in a shell). Leader `c` / `p` copy and paste on terminals that cannot send Ctrl+Shift+C/V.

## [0.14.6] — 2026-10-11

### Added

- Suite installer (`contrib/install-tetron-wm-suite.sh`): setup menu and `--profile gui|console|appliance`; the console step is split into `--console`, `--vt N` (default 1), `--autologin` and `--autostart`, each also asked interactively; kmscon defaults off on a desktop machine; idempotent re-runs; `--uninstall` (and `--purge`) and `--dry-run`.
- Launcher keyboard triggers: leader then `l` or Enter (alongside `a`), and bare Enter or Space on an empty desktop (no window focused). Esc never opens anything.
- `leader` and a `[keys]` table in `config.toml` remap the leader chord and every leader action (`launcher = ["l", "enter"]`). Listed in `tetron-wm --help`.
- `tetron-wm --help` lists commands, every `config.toml` key with its default, and all environment variables. A test keeps the key list in sync with the `Config` struct.
- Launcher > tetron-wm > Help opens the keyboard-shortcut overlay.

### Fixed

- The shortcut overlay (Ctrl+Space, `?`) is scrollable (arrows, PgUp/PgDn, Home/End, j/k); lower rows were unreachable on short terminals.
- The Exit / Restart / Shut Down confirm dialog and the forget-system dialog are keyboard-operable: Enter or `y` confirms, Esc or `n` cancels (a hint is shown in the dialog).
- Spotlight (Ctrl+Space, Space) and tall launcher menu panels now scroll to keep the highlighted row visible; previously rows past the 8th (Spotlight) or past the screen edge (menu) could be selected but never seen.

## [0.14.5] — 2026-10-08

### Changed
- **Truecolor is auto-detected under kmscon.** kmscon renders truecolor but sets
  `TERM=xterm-256color` with no `COLORTERM`, so env detection alone downsampled
  subtle themes. tetron-wm now also detects kmscon by walking `/proc` up the
  parent chain (the client runs under the kmscon process) and enables truecolor
  automatically — no config needed. The explicit `truecolor` config key / env
  override still wins.
- **Default install dir is now `/usr/local/bin`** (was `~/.local/bin`), so the
  binary is system-wide and already on `PATH`. `install.sh` escalates with
  `sudo` when the target needs root; set `TETRON_WM_BIN_DIR` for a no-sudo
  install into your home. The in-app updater installs to wherever the running
  binary lives, so a `/usr/local/bin` install updates in place (prompting for
  `sudo` in the update window).

### Added
- **One-shot suite installer** (`contrib/install-tetron-wm-suite.sh`, with a
  TLDR in the README). A single prompted `curl | bash` that optionally installs
  tetron-wm, seeds `~/.config/tetron-wm/` (config.toml + themes/ + wallpapers/),
  installs kmscon, and sets up tty1 autologin into the desktop. Every step is
  opt-in; the tty1 step is off by default, confirmed, and prints how to undo it.
  `-y` takes the defaults; `--all -y` builds the full console appliance.

### Fixed
- **Backspace erases in tetron-wm's own text fields under kmscon / the Linux
  console.** Those consoles send the Backspace key as `Ctrl-H` (0x08), not DEL
  (0x7f)/`KeyCode::Backspace`, so the Settings wallpaper box, window rename,
  launcher search, Add-Remote form, etc. never erased (the keystroke fell
  through unhandled). The client now normalizes `Ctrl-H` to Backspace for its
  own key routing, while the app-passthrough path keeps the raw key so `Ctrl-H`
  still reaches shells/apps (which already treated it as erase — hence shells
  were unaffected). Terminals that send DEL were always fine.
- **Window move/resize work under kmscon (and any terminal that reports
  button-less drag motion).** tetron-wm keyed titlebar-move / edge-resize on
  crossterm's `Drag` event, which only fires when a motion carries the held
  button. kmscon's mouse reports motion during a drag as a plain `Moved` with
  no button, so drags never started. The client now tracks the left button
  itself (via the existing pointer grab) and treats button-less motion while
  held as a drag — for chrome and for in-app drags. Also types the `ioctl`
  request constants per libc so the crate builds for the musl (static) target.
- **Suite installer no longer aborts the kmscon step on an Enter-through run.**
  A "yes" to the suite's "Install kmscon?" prompt now carries through to the
  kmscon installer (passes `-y`) instead of hitting a second, No-default
  "Proceed with the source build?" prompt that an Enter would decline. The
  kmscon script now also logs that it is building from source under `-y`
  instead of proceeding silently.
- **kmscon source build no longer dies on Debian bookworm / LMDE 6's old
  meson.** The maintained fork needs meson >= 1.1 but those ship 1.0.1;
  `install-kmscon.sh` now detects a too-old meson and bootstraps a newer one
  into a throwaway venv for the build (ninja from apt is new enough).
- **tty1 launch snippet now uses the binary's absolute path and prepends
  `~/.local/bin` to PATH.** The login profile runs before the interactive rc,
  so a bare `tetron-wm` could be missing from PATH at login and drop you to a
  shell instead of the desktop.
- **tty1 autologin now sets `TERM=xterm-256color`, not `$TERM`.** In a systemd
  unit `$TERM` expands to the console's `linux`, so the desktop came up with no
  truecolor and the mouse fell back to gpm (coarse, laggy). The suite's tty1
  step now hardcodes `xterm-256color` and disables the now-redundant `gpm`
  under kmscon; the bare-console setup doc is corrected to match.
- **Console launch snippet detects kmscon instead of testing `tty = /dev/tty1`.**
  kmscon runs the login shell on a pseudo-terminal, so the old `tty1` test was
  always false under kmscon and the desktop never started (the login shell just
  sat at a prompt). The snippet now walks the process tree up to `kmscon` (and
  still accepts a raw VT1), excluding SSH logins.
- **Appliance forces `truecolor = true` in config.toml.** kmscon supports
  truecolor but does not set `COLORTERM`, so auto-detection downsampled subtle
  themes. The suite's tty1 step now sets the `truecolor` config key.
- **kmscon source build now builds `main`, not the latest release tag, and
  enables mouse.** The newest tagged releases (kmscon v9.1.0 / libtsm v4.3.0)
  predate the mouse-report passthrough, so a tag build gave truecolor but no
  mouse — under kmscon the login shell is on a PTS where gpm cannot help, so
  there was no mouse at all. `install-kmscon.sh` now builds the maintained line
  from `main` (what Debian ships as kmscon 10 / libtsm 4.7.1) and enables the
  (default-off) `mouse` option in `kmscon.conf`. It also **wipes kmscon's module
  dir before reinstalling**: the module set differs between versions (9.1.0 had
  `mod-bbulk.so`/`mod-pixman.so`, `main` does not), and a stale leftover made the
  new kmscon fail with `undefined symbol: uterm_display_blit` and SEGV on
  startup — which silently fell back to the kernel getty (no truecolor/mouse).

## [0.14.4] — 2026-10-08

### Fixed
- **Settings → Appearance no longer freezes when editing the wallpaper
  filename.** With the Wallpaper-file text box open, mouse clicks were swallowed
  and every row went dead, so a mouse-driven user had no way to close it and the
  panel looked frozen. A click now dismisses the box (committing the typed
  filename, or cancelling the Apps add-form), just like clicking away.
- **A wedged `chafa` can no longer hang the whole UI.** The image-wallpaper
  render shelled out to `chafa` with inherited stdin and no timeout, on the
  daemon's input/render loop; now it runs with stdin closed, its output drained
  on a thread, and an 8-second hard timeout (then killed, with a log line).

### Added
- **Desktop icon-label scrim is configurable.** The dark backing drawn behind
  desktop icon glyphs and labels over a wallpaper is now set by `desktop_scrim`
  in `config.toml` (a color, or `"none"`/`"off"` to turn it off so labels sit
  directly on the wallpaper); unset keeps the default semi-transparent black.
  **Settings → Appearance** gains an **Icon label scrim** on/off toggle. Only
  applies over a wallpaper; the solid desktop is unchanged.

## [0.14.3] — 2026-10-07

### Added
- **Image wallpaper, configurable (v1).** Set `wallpaper = "<path>"` in
  `config.toml` to put an image behind your windows. The path may be absolute,
  `~`-relative, or relative to the config dir (a `wallpapers/` subdir is checked
  first, then the config root), so a bare filename means "next to your config".
  The image is rendered to colored cells via `chafa`, so it works on any
  terminal including a bare kmscon console. **Settings → Appearance** gains a
  **Wallpaper** on/off toggle (`wallpaper_enabled`) and a **Wallpaper file** text
  box: drop an image in `~/.config/tetron-wm/wallpapers/` and type its filename
  (the panel shows that path as a reminder). If `chafa` is missing or the image
  can not be read, the desktop stays solid and the reason is logged. The
  `TETRON_WM_WALLPAPER` env var still overrides the config for quick testing.
  `install.sh` now installs `chafa` as an optional dependency.
- **`install.sh` installs a Symbols Nerd Font** (the glyphs `icon_style = "nerd"`
  draws) as an optional dependency: a brew cask on macOS, and on Linux the
  "Symbols Only" release dropped into `~/.local/share/fonts` (no sudo) so
  fontconfig uses it as a glyph fallback in kmscon and most terminals.

## [0.14.2] — 2026-10-06

### Changed
- **Open windows now live in a top-panel taskbar; the bottom dock is gone.** The
  menubar row carries the taskbar of open windows (same-app grouping, a
  drop-down group chooser, and a colored letter badge per app), with a `+`
  new-shell button just right of the ✦ assistant. Pills show full labels, then
  shrink to badge-only, then collapse to a trailing `…+N` overflow marker under
  width pressure so they never collide with the tray (taskbar scroll is a
  planned follow-up). Retiring the bottom row gives every window one more line
  of screen. Right-click a pill for its context menu (minimise / maximise /
  close / reset size), which now drops down from the bar.
- **Power actions moved into the launcher.** The top-right host-name button is
  gone; the launcher's bottom section now holds **Systems**, **Exit**, **Restart**,
  **Shutdown** (Systems opens a top-left popover; the others open their confirm
  dialog). Reuses the existing power/systems flows.
- **Volume sits in the far-right corner** of the menubar (past the clock), flush
  to the screen edge — a fixed, eyes-free spot to scroll to change volume. Its
  level is zero-shift padded so changing digits (5 → 100) never nudges the other
  segments.
- **The menubar clock shows the time only** (`HH:MM`); the date/day live in the
  calendar popover (one click on the clock).
- **Tray drop-priority** sheds CPU → Mem → Battery → Bell → Tetron → Wi-Fi →
  Bluetooth under width pressure; the clock and volume are always kept.
- **Renamed from `tuiui` to `tetron-wm` (breaking).** This is a fork of upstream
  `jaylfc/tuiui`; sharing the name let installs and the in-app updater collide
  with upstream. The binary, crate, config dir (`~/.config/tetron-wm`), state dir,
  log, socket, service unit (`tetron-wm-apphost`), `TETRON_WM_*` env vars, and
  release assets (`tetron-wm-<target>.tar.gz`) are all renamed. Existing installs
  must be reinstalled; config/service paths do not migrate automatically.
- **Menubar launcher button now reads `tetron`**, not the full binary name.
- **The theme now applies to every surface.** Terminals, the launcher, the file
  manager, desktop icon labels, the Store, Settings, and the Activity Monitor all
  read the active theme instead of a hardcoded midnight palette, so theme switches
  (and the pastoral theme) are no longer skin-deep.
- **Finer window borders** — a thin green line instead of a wide white band.
- **No default desktop pins** — the desktop shows only the real contents of
  `~/Desktop`; add pins via `[[desktop_pins]]`.
- **The file manager opens in list view by default** (was the icon grid). The
  grid view's fallback glyph now sits just above the label instead of floating
  high in the tile.

### Added
- **Scroll the wheel over the Volume tray segment to change the system volume**
  (up = louder, ±5 per notch), instead of it falling through to window scrollback.
  Works regardless of which window is focused (the menubar row is treated as
  chrome, so a focused Store/Files/Logs no longer swallows the wheel there).
- **Choose truecolor vs 256-color** — a `truecolor` config key (and
  `TETRON_WM_TRUECOLOR=1/0` env override) forces 24-bit color on or off. Auto-
  detection only enabled truecolor for known terminals or when `COLORTERM`
  advertised it, so terminals that do truecolor but set `TERM=xterm-256color`
  without `COLORTERM` fell back to the 256-color palette — which quantized the
  subtle themes (nord → teal/navy, dracula) into the wrong colors. Env wins over
  the config key, both win over auto-detection and the graphics probe.
- **File-based themes.** Drop a `*.toml` in `~/.config/tetron-wm/themes/` (or set
  `theme_dir`) and it appears everywhere a built-in does — Settings → Appearance,
  the power-menu remote form, `tetron-wm theme <name>`. A file is a flat table of
  the palette fields → `#rrggbb` / `#rrggbbaa` (alpha) / named color, with optional
  `name` and `base` (a built-in to inherit unset fields from); a file named after a
  built-in overrides it. `tetron-wm theme --dump <name>` writes a built-in out as an
  editable starter; `--list` prints every theme name. Malformed files are skipped
  and logged; low-contrast themes are warned about (never rejected) so a user theme
  cannot silently reproduce the red-on-green button bug.
- **Pastoral theme** — a fifth, and first light-appearance, palette
  (`theme = "pastoral"`, or cycle it in Settings → Appearance): paper wallpaper,
  black-on-white terminals, light panels, tetron-green highlights. (Named for the
  palette, not its brightness, so the name does not clash with a theme's
  light/dark `appearance`; `theme = "light"` still works as a deprecated alias.)
- **`terminal_bg` / `terminal_fg` config keys** — decouple terminal colors from the
  desktop theme (hex or named color); unset = follow the theme.
- **`icon_style` config key** — picks the file-type icon glyphs used where the
  graphical (Kitty) icons can't render (kmscon, a bare VT, plain SSH): `ascii`
  (default; BMP geometric shapes that render in every font and take the theme
  color), `nerd` (Nerd Font glyphs — needs a Nerd Font installed), or `emoji`.
  Previously the file manager and desktop always used emoji, which showed as
  tofu on a console.

### Fixed
- **The menubar tray glyphs now follow `icon_style`.** Volume, battery, and the
  bell used hardcoded color emoji (🔇/🔉/🔊, ⚡/🔋, 🔔) regardless of the icon
  style, so they clashed with the ascii/nerd file icons (and showed as tofu on a
  console). All tray indicators (CPU, memory, battery, volume, bluetooth, network,
  bell) now resolve through the same `icon_style` set — BMP shapes under `ascii`,
  Nerd Font glyphs under `nerd`, emoji under `emoji`.
- **Desktop icon glyph sits just above its label** — the no-graphics fallback
  glyph was stranded high in the tile (centered in the image area); it now sits at
  the bottom of the icon rect like the file manager's grid view.
- **Dragging a mouse-reporting app's window (e.g. tetron-tui) works now.** The
  client routed each mouse event by the pointer's position in the *last frame's*
  app content rect, so dragging the titlebar downward "fell into" the app as the
  cursor crossed that stale rect before the window caught up — the window stuck,
  stalled, or only dragged upward (a plain shell window, which reports no mouse
  area, was unaffected). The client now takes a pointer grab on the button press:
  the whole drag goes to whatever the press landed on (chrome or app) until
  release, regardless of where the pointer wanders.
- **Window dragging is smoother in general.** Two more drag glitches fixed:
  (1) a window could stay "stuck" to the pointer after release — a lost release
  event left the drag active, and a no-button move now ends it (self-heal, as text
  selection already did); (2) fast vertical drags froze or dragged only one
  direction — the spurious-teleport guard dropped *any* half-screen step, now it
  only drops a big jump that lands on a screen edge (the real off-screen-fling
  signature), so fast mid-screen drags pass through.
- **Confirm-dialog buttons are readable now.** The destructive confirms (Close,
  Shut Down, Remove, the dangerous-launch Launch, Restart) drew red text on the
  green accent -- an unreadable red-on-green, especially on the pastoral (light) theme. They
  now use light text on a red fill (a proper destructive button); the benign
  Connect button and the calendar's "today" cell use light text on the accent.
- **Window rename triggers only on the title text now**, not anywhere on the
  titlebar, and clears any in-progress move/drag when it starts — so a
  double-click on empty titlebar space no longer accidentally starts a rename or
  leaves a stray, mispositioned titlebar behind.
- **Install/update now pull from the fork, not upstream.** `install.sh`, the in-app
  updater, and the Systems remote-installer pointed at `jaylfc/tuiui`, so a prebuilt
  install or a Settings → Updates run fetched upstream and dropped the tetron
  integration. All now target `tetronos/tetron-wm`.

## [0.14.1] — 2026-10-05

### Fixed
- **Tray "Open terminal" now works when a window sits under the popover.** The
  popover opened the native `tetron-tui` only when nothing was behind it; over
  an app window the click was swallowed as a text-selection on the window
  beneath and nothing launched. `mouse_select` runs before the tray router on
  `MouseDown` but did not treat an open tray popover as an overlay, so it
  claimed the click. The tray popover now counts as an overlay like every other
  modal menu.

## [0.14.0] — 2026-10-05

### Added
- `tetron-wm --version` (and `-V`) prints the version.

### Changed
- **Version scheme now tracks tetron core's minor.** Addons version as
  `0.<tetron-core-minor>.<patch>`; this release follows `tetron-proto 0.14.x`,
  so the jump from `0.2.14` to `0.14.0` is a scheme change, not 12 minors of
  feature work. Patch resets to 0.

## [0.2.14] — 2026-07-20

### Fixed
- **File-manager icons are now real desktop-style tiles.** 0.2.13's FM icons
  rendered as a squashed one-row smear: the Icon view's tiles were still the
  old 3-row (glyph + label) cells, so images were crushed into a single row.
  Tiles are now the desktop's proportions (icon area + centered label), role
  icons and thumbnails fill the tile without stretching, and render /
  hit-test / image placement all share the same geometry helpers so they
  can't drift. Glyph fallback matches the new layout.
- **The FM context menu is now a real right-click menu.** It used to open as
  a centered "Actions" dialog with one horizontal text line. It's now a
  compact vertical menu (Open / Rename / Copy / Cut / Delete / Get Info)
  anchored at the click (or the focused tile via keyboard), with clickable
  rows, hover/arrow-key highlight, click-outside-to-dismiss — and Esc now
  closes the menu instead of the whole Files window.

## [0.2.13] — 2026-07-20

### Added
- **File-manager icons now match the desktop.** The FM's Icon view renders
  the same role-icon image tiles as desktop icons (folders, text, code,
  archives, …) on Kitty-graphics terminals, instead of text glyphs; image
  files keep their real thumbnails. Glyph fallback elsewhere is unchanged.
- **Right-click in the file manager.** Right-clicking an entry raises the
  window, selects the entry, and opens the context menu (open / rename /
  delete / copy / …) — previously the menu was keyboard-only.
- **App variants in the catalog.** A catalog entry can declare launcher
  variants (extra entries emitted when the app is installed). First user:
  Claude Code ships a **⚠️ variant** running `--dangerously-skip-permissions`.
- **Launch-warning dialog.** Launcher entries can carry a `warn` message
  (the Claude ⚠️ variant does; your own `[[launcher]]` config entries can
  too): launching one opens a confirm dialog before anything spawns —
  Enter/`y` launches, Esc/`n` cancels.

### Fixed
- **The desktop now notices external changes to `~/Desktop`.** A folder or
  file created in a terminal or the file manager appears on the desktop
  within ~2 seconds (throttled mtime poll), instead of only after a reload.

## [0.2.12] — 2026-07-19

### Fixed
- **`tetron-wm launch <cli-tool>` no longer opens a window that instantly dies.**
  The v0.2.11 help-then-shell wrapper for catalog-tagged CLI tools only
  applied to the launcher menu / Spotlight / Store paths; the `tetron-wm launch`
  escape hatch (used by the AI assistant and scripts) still spawned the bare
  binary, which printed usage and exited. A bare `tetron-wm launch gum` now gets
  the same wrapper; passing args (`tetron-wm launch gum choose a b`) runs the
  command exactly as given.

## [0.2.11] — 2026-07-04

### Added
- **CLI tools get a proper launch path.** Some catalog apps are CLI tools, not
  persistent TUIs (himalaya, gum, freeze, khal, dust, …) — launching one used
  to open a window that printed an error or usage text and immediately died.
  A one-time audit flagged **52** such entries with a new `"cli": true` catalog
  field; they now show a **`CLI` tag** in the launcher (menu + Spotlight) and
  the Store, and launching one opens a shell that prints the tool's `--help`
  first, then drops to your interactive shell with the tool on `$PATH` — see
  the commands, then use them. `requires_cwd` still applies (the shell starts
  in the picked directory), and user config can set `cli = true` on custom
  entries too.

### Fixed
- **Activity monitor now shows the command/args of local app sessions.** An
  `Any`-downcast through `Box<dyn AppHost>` could never match the concrete
  host type, so the cmd column was always blank; it now uses the `AppHost`
  trait's own `launch_cmd`.

## [0.2.10] — 2026-07-04

### Fixed
- **Installing and self-updating no longer break when GitHub rate-limits the
  API.** `install.sh` resolved the latest release through the unauthenticated
  `api.github.com` REST endpoint, which is capped at 60 requests/hour per IP.
  Once that budget is spent GitHub answers **403**, and the script reported it
  as `no published release yet` before falling back to a slow `cargo install`
  source build — so `curl | sh` appeared to say the project had no releases,
  and the in-app **Update & Reload** silently dropped into a multi-minute build
  that looked hung (the long-standing "update from Settings gets stuck"
  report). Both now resolve the tag from the web redirect
  `github.com/OWNER/REPO/releases/latest` → `.../releases/tag/vX.Y.Z`, which
  isn't subject to the API rate limit, and only fall back to the REST API if
  that redirect can't be parsed. Settings' "check for updates" uses the same
  path, so a spent API budget no longer shows a false "Couldn't check
  (offline?)".

## [0.2.9] — 2026-06-13

### Changed
- **The debug log now survives a reload, and records the version + binary** of
  every daemon start. `dbg_init` previously *truncated* `~/tetron-wm-debug.log` on
  each daemon startup — so an in-app update that reloaded the daemon wiped its
  own trace, leaving the log useless for diagnosing update failures. It now
  appends a richer banner (`v<version>, git <sha>, exe <path>`) and the reload
  → respawn seam is logged on both sides (`daemon: reload — exiting…`,
  `client: daemon reload — …respawning`, `daemon: spawning <exe> --daemon`).
  An update attempt now leaves a complete, persistent trace: the install
  steps, the reload, and the **post-update version** in the next banner — so a
  "version never changed" failure is finally visible in one log.

## [0.2.8] — 2026-06-13

### Added
- **Switchable assistant agent**: Settings → Assistant now has an **Agent** row
  that flips the ✦ panel between the two supported CLIs — **opencode** (default)
  and **hermes**. The choice is stored in `assistant_command`, so an existing
  config that points at one of them is reflected in the switch (and a
  hand-edited `assistant_command` can still name any binary).

## [0.2.7] — 2026-06-13

### Fixed
- **In-app update could "succeed" without changing the running version**: the
  updater reloads by running a bare `tetron-wm reload`, but it runs in a
  non-interactive `sh -lc` whose PATH may not include the install dir
  (`~/.local/bin` is added by interactive shell config, not a login `sh`). When
  `tetron-wm` wasn't found, `install.sh` still wrote the new binary but the daemon
  never restarted onto it — so the version never moved and the update appeared
  to fail every time. The updater now reloads via the **absolute path** of the
  freshly-installed binary (`{install_dir}/tetron-wm reload`), removing the PATH
  dependency.

### Changed
- **The updater now logs to `~/tetron-wm-debug.log`**: the in-app update runs in a
  window whose output was otherwise lost. Update checks, the install request,
  and each install step (install.sh / cargo fallback / reload / failure) now
  leave breadcrumbs in the debug log, so a failed update is visible in the log
  users paste.

## [0.2.6] — 2026-06-13

### Added
- **Dock right-click context menu**: right-click a dock pill for Minimise /
  Maximise / Close / Reset size. Grouped pills target the group's focused
  window (else the first); Close routes through the usual confirm dialog;
  Reset re-centres the window at half the work area — the rescue hatch for a
  stranded or mis-sized window. Render and hit-testing share the same geometry
  fns, and the menu y-clamps so it stays on-screen (and clickable) on very
  short terminals. Ported from the `dev` branch (#11, hardened in #20).

### Changed
- **Activity-monitor / apphost follow-ups**: `tetron-wm ps` and `tetron-wm kill-app`
  now drain the on-connect roster (and any queued events) before matching the
  `AppList` reply, via a shared `fetch_app_list()` helper; `kill-app all` is
  safe-by-default (only reaps already-dead apps — live apps need an explicit
  id). The apphost protocol gains an optional `pid` field on
  `HostEvt::Spawned` (PROTO_VERSION → 2, additive, `MIN_COMPAT` unchanged) so
  the daemon's `RemoteAppHost` can fill the activity panel's `pid` column, and
  it now stashes each spawn's command + args so the panel shows real data in
  normal daemon mode. Re-landed from the stale `#16` branch onto current
  `main` (keeping main's newer visibility-gated `refresh_activity`).

## [0.2.5] — 2026-06-13

### Fixed
- **Update loop on the `main` channel**: the v0.2.4 release binaries were built
  from the pre-bump commit and reported their version as `0.2.3`, so the in-app
  updater saw a perpetual "update available" (`0.2.3 → 0.2.4`) that installing
  could never settle. Re-released as 0.2.5 (identical code) built from the
  correct commit. The release workflow now fails if the release tag and the
  `Cargo.toml` version disagree, so a version-skewed release can't ship again.

### Changed
- **Releases cut themselves on a version bump**: merging a `Cargo.toml` version
  change to `main` now auto-tags and publishes the release (no tag push
  required), alongside the existing tag-push and manual `workflow_dispatch`
  paths.

## [0.2.4] — 2026-06-13

### Fixed
Robustness fixes from a code review of the networked seams — these had landed
on the `dev` branch but never reached `main`, so they were absent from the
0.2.x releases. Ported to `main`:
- **Log-copy could crash the daemon**: copying a large (>200 KB) log whose
  non-ASCII bytes straddled the tail cut sliced a string mid-UTF-8 and panicked.
  The cut now advances to a char boundary first.
- **Apphost reader resilience**: a single corrupted/blank protocol line used to
  tear down the whole apphost connection (dropping every window's frame stream);
  `recv` now logs-and-skips a bad line while still propagating genuine IO errors.
- **Updater branch validation**: a hand-edited/typo'd `update_branch` flowed
  into a shell command and a URL; it's now sanitized to a git-safe charset and
  falls back to `main`.
- **Control-socket lock resilience**: `apply_ctl` no longer holds the queue lock
  across `core.apply` (which could block the control thread), and both lock
  sites recover a poisoned lock so one panic can't wedge `tetron-wm launch/tile/
  theme/msg`.
- **Remote file-manager freeze bounded**: tightened SSH `ConnectTimeout` (4→3s)
  and the frequent navigation ops (list/home → 5s) so an unreachable saved
  system is a brief hitch, not a multi-second freeze.

## [0.2.3] — 2026-06-13

### Fixed
- **In-app update could land in a shadowed binary**: when the fast path
  (`install.sh`) failed — e.g. a brief network blip or racing a release's
  asset upload — the updater fell back to `cargo install --git`, which writes to
  cargo's default `~/.cargo/bin`. If that differs from the dir of the running
  binary (e.g. a release install in `~/.local/bin`), the rebuilt binary landed
  in a `$PATH`-shadowed location and the update silently appeared to do nothing.
  The cargo fallback now targets the running binary's own `bin/` dir via
  `--root`, so both update paths replace the binary the user actually runs.

## [0.2.2] — 2026-06-13

### Changed
- **Assistant standardised on opencode**: the ✦ assistant now runs a single
  agent CLI — **opencode** (model-agnostic, MCP-extensible) — instead of a
  six-framework menu. The per-framework machinery is gone (Claude Code / Kilo /
  Hermes / OpenClaw / smallcode plumbing, the `CLAUDE.md` / `HERMES.md` /
  `knowledge/*.md` stamping, the smallcode `.env` and OpenClaw workspace pointer,
  and the Settings framework picker); the briefing is stamped once as `AGENTS.md`.
  `assistant_command` in config.toml still overrides the binary, and broader
  OS/computer-use control is added via MCP servers in opencode's own config.
  Migration: if you had `assistant_command` pinned to `hermes` or `openclaw`,
  add their TUI launch arg to `assistant_args` (`["--tui"]` / `["tui"]`) — the
  per-framework default arguments are no longer applied.

### Added
- **Revoke SSH key when forgetting a system**: the ✕ in the Systems menu now
  opens a confirm with an opt-in *"Also revoke this PC's key on &lt;host&gt;"*
  toggle. When checked, removing the system also strips this machine's public
  key from that remote's `~/.ssh/authorized_keys` (the inverse of the key copy
  that adding it performed). Exact full-line match only (never touches other
  keys), across all your local identities; it rewrites the file in place inside
  `~/.ssh` (preserving its `0600` perms and SELinux context) and keeps a
  `.tetron-wm.bak`. Best-effort on a background thread — an offline host never blocks
  the removal or the UI, the local forget always succeeds, and any remote-side
  failure is logged rather than silently assumed done.

### Fixed
- **Activity Monitor refresh while hidden**: the per-frame app snapshot was
  still running when the Activity panel was open but **minimized** (or hidden in
  simple mode), so the window-drag stutter could persist in those states. The
  refresh now also requires the panel to be visible.
- **Update check no longer offers downgrades**: the main-channel version
  comparison was a plain string compare, so a local build *newer* than the
  latest release (mid-release-cut, or a dev/source build) showed a bogus
  "update available → older version". It now compares release version numbers
  (`major.minor.patch`) and only offers a strictly-newer release; an
  unparseable release tag reports "couldn't check" instead of guessing.

## [0.2.1] — 2026-06-13

### Added
- **Activity Monitor** (Ctrl+Space → A, or `@activity` in the launcher): a
  live, auto-refreshing table of every app the apphost is hosting (id, pid,
  command, age, dimensions, state) with kill-app controls and an Enter/y vs
  Esc/n confirm for live apps. Also `tetron-wm ps` and `tetron-wm kill-app <id|all>`
  CLI subcommands that work from any terminal or SSH session.

### Fixed
- **Window dragging stutter**: moving (or resizing) a floating window felt
  sticky and jerky once a few apps were open. The activity monitor's row
  refresh was running every frame even while its panel was closed, and each
  pass cloned every hosted app's full terminal grid just to read its size —
  stalling the render loop mid-drag. The refresh now does nothing unless the
  Activity Monitor window is actually open.
- **Update loop on the main channel**: "Check for updates" compared the
  installed build against the tip of the `main` branch, but the main channel
  installs the latest prebuilt *release*. Any commit landed on `main` after the
  last release showed a permanent "update available" that re-installing the
  same release could never clear. The check now compares the installed version
  against the latest release tag — matching what `install.sh` installs — and
  reports it as versions (`v0.2.0 → v0.2.1`) instead of commit hashes. The dev
  channel still tracks the branch tip and shows short commit hashes.

## [0.2.0] — 2026-06-10

The "desktop, networked" release: switch between machines, an AI copilot, and
a lot of polish.

### Added
- **Systems switcher** (power menu → Systems): saved machines with live ●/○
  reachability dots; **Add Remote** transfers an SSH key, installs tetron-wm + gpm
  + your terminal's terminfo on the remote, syncs your systems list, and
  connects; per-system themes; drop back to the local desktop when the remote
  session ends.
- **AI assistant** (✦ menubar button): a persistent chat panel — or a floating
  window (**Settings → Assistant**) — running any of six agent frameworks
  (Claude Code, opencode, smallcode, Kilo, Hermes, OpenClaw). Instructions live
  in the repo `agent/` pack, stamped into the agent's forced working directory
  in every convention the CLIs read. The agent can drive the desktop
  (`tetron-wm launch/tile/theme/msg`), read the logs, fix tetron-wm and open PRs, and
  operate across your saved machines over ssh/scp.
- **Menubar clock + calendar**: date+time, click for a month calendar with
  month navigation and `khal` events.
- **Notifications**: a background app's bell rings a dock attention dot + a 🔔
  tray popover (click to focus); apps' OSC-52 copies are forwarded to the host
  clipboard.
- **Remote files**: browse saved systems over ssh in the file manager; copy
  between machines with Ctrl+C/Ctrl+V (background `scp`, `-3` for remote↔remote).
- **Logs viewer** (launcher → tetron-wm → Logs): tails `~/tetron-wm-debug.log`; `c`
  copies it to the host clipboard via OSC 52.
- **Scrollback in app windows**: the mouse wheel scrolls a PTY window's history;
  any keystroke snaps back to the live bottom.
- **Battery** tray segment (Linux sysfs / macOS `pmset`).
- **Control CLI**: `tetron-wm launch`, `tile`, `theme`, `msg` drive a running
  desktop from any shell (also the assistant's control surface).
- **Updates channel switcher** (Settings → Updates): track `main` (fast
  prebuilt releases) or a `dev` branch (built from source).
- **Update safety net**: updates never kill running apps today (the app server
  is a separate process the reload doesn't touch, and the wire protocol is
  skew-tolerant by construction) — but if a future update ever *must* break
  that protocol, a safety dialog now appears after the reload: it warns that
  restarting the app server will close the user's N apps, and offers
  **Keep apps** (save your work first; restart later from a row in
  Settings → Updates) or **Restart app server**. Armed by bumping
  `MIN_COMPAT` in `apphost/proto.rs`; dormant otherwise.

### Changed
- The in-app updater now installs the **prebuilt latest release** by default
  (a download, not a source compile), falling back to `cargo install`; it
  reopens Settings on the Updates screen after the reload instead of vanishing.
- Add Remote / reconnect copies the local terminal's terminfo to the remote and
  always falls back to `xterm-256color`, fixing "cannot find terminfo entry for
  'xterm-ghostty'" (and the same for Kitty and other modern terminals).
- Dependencies refreshed to current: crossterm 0.29, portable-pty 0.9,
  sysinfo 0.39, toml 1.1, infer 0.19. Minimum supported Rust is now **1.95**
  (declared via `rust-version` in Cargo.toml).

### Fixed
- Settings' "Check for updates" / "Update & Reload" now respond to **mouse**
  clicks, not only the keyboard.
- The updater window auto-closes after a successful reload.
- Clippy is clean across all targets.

## [0.1.0]

Initial development series: floating windows, persistent daemon/thin client,
app store, settings, theming, menubar tray, grid tiling, native image layer,
file manager, desktop icons, cascading launcher, apphost/frontend split with
live reloads, mouse passthrough, simple view mode, dock grouping, bare-console
(gpm) mouse, apphost service, terminal office suite.
