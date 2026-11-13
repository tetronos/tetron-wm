# How tetron-wm installs apps: the Store, Settings > General > Apps and the suite installer

This describes the behaviour as of tetron-wm 0.15.6 plus commit b420150 (main, 2026-11-12). Three places install apps, and today each has its own engine with its own fallbacks. A rework into one engine (`tetron-wm collection install`) is planned; until it lands, this document is the reference for what happens.

Words used here: a **collection** is a named group of apps (`minimal`, `essentials`, `recommended`, `extras`), each including the smaller ones. The current code and screens still call a collection a "size" (`--size`, `Installed:`) or a "set" (`Remove set`, "In the recommended set"); those words are quoted where they appear and are to be replaced by "collection". The **collection packages** are the Debian metapackages `tetron-wm-desktop`, `tetron-wm-essentials`, `tetron-wm-recommended` and `tetron-wm-extras`.

## 1. Where the app lists come from

| File | Written by | Read by |
|---|---|---|
| `lists/tetron.toml` | hand edits, plus recipe blocks appended by `scripts/match_brew.py` (Homebrew formula index), `scripts/merge_recipes.py` (the derive-install-recipes workflow) and `scripts/derive_os.py` | the generators below |
| `lists/awesome-tuis.json` | `scripts/import_awesome_tuis.py`, from the awesome-tuis list | `scripts/merge_catalog.py` |
| `lists/repology.json` | `scripts/match_repology.py`, from repology.org | `scripts/merge_catalog.py` |
| `assets/suite-apps.txt` | `scripts/gen_suite_lists.py` | carried inside the catalog; the suite installer |
| `packaging/<collection>.toml` Recommends | `scripts/gen_suite_lists.py` | the collection packages |
| `assets/catalog.json` | `scripts/merge_catalog.py` (awesome-tuis, then Repology names, then `lists/tetron.toml`, which wins) | compiled into tetron-wm; the Store; Settings |

`scripts/rebuild-app-sets.sh` runs the generators and their checks in one go after an edit of `lists/tetron.toml`.

The catalog and the suite list are also published per core minor at https://tetronos.github.io/apt/lists/0.15/ (`catalog.json`, `suite-apps.txt`), so a running tetron-wm and the suite installer can use a newer list without a new release.

### The suite list format (`assets/suite-apps.txt`)

One line per entry, `:` separated:

- `collection:group:check:apt:dnf:pacman:brew`: a package. `group` is `apps`, `gui` (only with a desktop session, `DISPLAY` or `WAYLAND_DISPLAY`) or `services` (PipeWire, NetworkManager, BlueZ; a separate question, default no). `check` is how presence is tested (section 5). An empty name means that manager has no package.
- `collection:script:name`: an app no distribution packages, installed by its own method in the suite installer (opencode, fresh, uv, catt, casttui, the Nerd Font, the helper scripts; in recommended also croft, pyradio, llm, yt-dlp, freebuff).
- `collection:release:check:repo:amd64-asset:arm64-asset`: where a member comes from when the package manager has none (WMINST-015). Today: fastfetch, glow, xh, lazygit.

`desktop` in older lists counts as `essentials`.

## 2. The Store

Opened from the launcher. Source: `src/store.rs`, `src/catalog.rs`, `src/session.rs` (`store_activate`), `src/listfetch.rs`.

- **List shown:** every app in the catalog in use, filtered by category and by the search text (name or description). An app whose recipe names operating systems other than this one, and that has no package for this one, is hidden.
- **Catalog in use:** the last good **Update list** download (`~/.local/share/tetron-wm/catalog.json`) while it passes the checks (schema 1, has apps, has a suite list), else the compiled-in one. **Update list** (or F5) downloads `https://tetronos.github.io/apt/lists/<major.minor>/catalog.json` (`TETRON_WM_LISTS_URL` overrides the base), checks it and uses it at once; a failed check keeps the old one.
- **Installed or not:** the app's command (`bin`, or its name in lower case) is on PATH. `~/.local/bin` is added to PATH at start.
- **Detail pane:** description, homepage, the CLI badge, "In the recommended set (Settings > General > Apps)" when it belongs to a collection, and the recipe's setup tip.
- **Enter (or click) on an installed app:** launches it; coding agents and the AI category first ask for a working directory; CLI tools run with `--help` and stay in a shell.
- **Enter on a missing app:** opens a terminal window titled `install: NAME` that runs one shell command and stays open afterwards (closing it rescans PATH and rebuilds the launcher). The command tries, in order:
  1. on macOS, Homebrew when the app has a brew name;
  2. apt, dnf or pacman, each only when that manager exists and lists the package name (`apt-cache show`, `dnf info`, `pacman -Si`), with sudo unless already root;
  3. else the verified upstream recipe (its install command; a missing toolchain such as cargo, go or uv is offered first), or `brew install` by name on Linux;
  4. else, for an app with no package names and no verified recipe, a best-effort chain: the package manager by command name, `cargo install`, `go install <repo>@latest`, then brew if present;
  5. it always ends by printing the homepage, so the user can finish by hand.
- **Safety:** package names from a downloaded catalog are used only when they contain letters, digits and `. _ + - / @`; anything else is ignored.
- **Not done by the Store:** it installs one app at a time, never a collection; it does not use the release fallback of section 4; it asks no confirmation (the terminal window shows everything and sudo asks for the password).

## 3. Settings > General > Apps

Source: `src/appsdlg.rs`, `src/packaging.rs`, `src/settings.rs`. The dialog only decides; the work runs in a terminal window.

### What it shows

- `Installed: <collection>`: the collection packages dpkg has (largest wins), else the `size` recorded in `~/.config/tetron-wm/suite-state` by the last suite installer run, else minimal.
- One row per collection, `Essentials` and `Recommended`, plus `Extras` once the list has extras apps. Each row says `all installed`, `N to add`, or `needs curl`.
- `[x] also the apps with their own installers`: ticked by default; unticked passes `--no-scripts`.
- `Remove set: <collection>`, only on Debian and Ubuntu when `tetron-wm-essentials` is installed.

**How "N to add" is counted:** from the suite list of the catalog in use (section 2), for the chosen collection and the smaller ones, with the presence checks of section 5. `gui` rows count only with a desktop session, `gpm` only with a mouse device (`/dev/input/mice`), `services` never, the helper scripts only when tetron-wm is not the apt package. A package row counts on Linux when any of its apt, dnf or pacman names is filled in (the first filled one is shown), whether or not this machine's package manager has it; `release` rows are not counted separately (their app is a normal row too).

### Install

Enter on a collection opens a confirmation with focus on Cancel. It lists the apps to add (those with their own installer marked "(its own installer)"), and on apt hosts the download size and disk growth from apt (computed in the background). In a live session it also shows the RAM-backed free space and warns when the collection does not fit. It says: "Needs the network and your sudo password. Apps already here are kept." and how to undo.

Install opens a terminal window that:

1. writes the suite list in use to `~/.local/share/tetron-wm/suite-apps.txt`, so the installer installs exactly what was counted;
2. downloads `install-tetron-wm-suite.sh` **from this tetron-wm's own release tag** (for 0.15.6, `v0.15.6`), and from `main` only when that tag does not exist (a local build). A fix to the suite installer on main therefore reaches Settings only with the next tetron-wm release;
3. runs it with `--size <collection> --no-wm --no-config --no-themes --no-convert --no-kmscon --no-console --no-autologin --no-autostart --no-fullscreen --apps-list <file>`, plus `--no-scripts` when the box is unticked. So it only installs apps (section 4, steps 4 to 7);
4. writes the exit status to the tetron-wm log and waits for Enter.

### Remove set

Only on Debian and Ubuntu with the collection packages. It removes the collection package of the installed collection and the larger ones (`tetron-wm-desktop`, the tools tetron-wm itself uses, is never removed), then runs `apt-get autoremove`, which lists the apps no other package needs and asks before removing them. Apps installed by their own installer (uv tools, files in `~/.local/bin`) stay; the confirmation names them and how to remove one by hand.

## 4. The suite installer (`install-tetron-wm-suite.sh`)

Run by `curl -fsSL https://tetronos.github.io/tetron-wm/install.sh | bash`, by the tetronos installer (`curl -fsSL https://tetronos.github.io/install.sh | bash`), by the in-app updater, and by Settings (section 3). Piped from the web it uses `main`; a push to main is live at once. Full option list: `install-tetron-wm-suite.sh --help`.

Order of the app steps:

1. **Collection:** `--size` if given (`--no-extras` means minimal), else asked ("minimal / essentials / recommended / extras", default essentials); `-y` takes essentials.
2. **List:** `--apps-list FILE` when given and valid; else the clone's own `assets/suite-apps.txt` when run from a clone; else the published `https://tetronos.github.io/apt/lists/<minor>/suite-apps.txt`; else the repository's raw file. With none, no apps are installed and it says so.
3. **Services:** PipeWire, NetworkManager, BlueZ, asked separately (default no) when missing; installed after step 8.
4. **Collection packages or one by one:**
   - on Debian and Ubuntu when tetron-wm is (or becomes) the apt package and the release has collection packages: the packages for the collection are downloaded from the release, checked against their `.sha256`, installed with apt, then any Recommends apt left out are installed and marked automatic;
   - otherwise the missing packages of the collection are installed with the host's package manager (apt, dnf, pacman, or brew on macOS). On apt hosts, names apt has no candidate for are skipped first, each with a line "skipping NAME: apt has no package of that name here", since one unknown name makes apt refuse the whole batch.
5. **A failed batch** (WMINST-015) is judged by what the package manager lists as installed, not by its exit status: the packages not installed are retried one by one, and only those still missing are named. With `--yes`, or without a terminal on stdin, apt runs with `DEBIAN_FRONTEND=noninteractive`, so no configuration question (console-setup asks one) blocks or fails the batch.
6. **tetron-browser** (WMINST-014), when still missing on Linux x86_64: the latest release, the `.deb` with apt on Debian and Ubuntu, else the zip under `~/.local/share/tetron-browser` with `~/.local/bin/tetron-browser`; checked against its `.sha256`.
7. **Release fallbacks** (WMINST-015): each `release` member still missing comes from the latest GitHub release of its repository: a `.deb` with apt (Debian and Ubuntu only) or a `.tar.gz` whose command goes to `~/.local/bin`. The file must match the SHA256 digest GitHub lists for it, or it is not installed. Then brew, when installed, for any member still missing. What is still missing is named in one line: "not installed (no package here, no release to fall back on): ...".
8. **Apps with their own installers** (unless `--no-scripts`): fresh (apt from the tetronos repo first, else its installer; brew on macOS), uv, catt and casttui, the Nerd Font (apt `tetronos-nerd-font-symbols` first, else the font files), opencode, then croft, freebuff and the uv tools of recommended, and the helper scripts when tetron-wm is not the apt package.
9. **State:** `~/.config/tetron-wm/suite-state` records `size` and `method`.
10. **Result:** exit 0, or "finished with problems: ..." and exit 1.

`--dry-run` prints the plan (collection, method, the apps that would be added) and changes nothing. Every step is idempotent: a second run installs nothing new.

## 5. How "installed" is decided

| Check | Suite installer and Settings | Store |
|---|---|---|
| a command | on PATH, or in `~/.local/bin`, `/usr/sbin`, `/sbin` | on PATH (`~/.local/bin` is added to PATH at start) |
| `a\|b` | any of them | n/a |
| `consolefonts` | a file in `/usr/share/consolefonts` or `/usr/lib/kbd/consolefonts` | n/a |
| `font=NAME` | `fc-list` has it (else DejaVu Sans Mono files) | n/a |
| `nerd-font` | `fc-list` has a Nerd Font (else the symbols font files) | n/a |
| `yt-dlp` | `~/.local/bin/yt-dlp` only (the uv copy; a distribution's falls behind the sites) | on PATH |
| helpers | `tetron-wm-startpage` present | n/a |

## 6. Known differences and gaps (to be removed by the planned engine)

- Three install engines: the Store (shell command per app), Settings (the suite installer from the release tag) and the suite installer (from main when piped). The same app can install on one path and fail on another.
- The release fallback of section 4, step 7 exists only in the suite installer; the Store does not use it.
- Settings uses the installer of its own release tag, so suite installer fixes reach it one release later.
- Settings counts a package row as installable when any distribution has a name for it, not only this machine's package manager; the installer then reports it as missing.
- Apps with no package and no release on a distribution stay missing there. Known: newsboat on Ubuntu 24.04 (Debian 13 and LMDE 7 have it; it publishes no release binaries).
- No way yet to add one's own apps to a collection or to leave an app out.

## Reference

- Specs: WMINST-001 (collections in `lists/tetron.toml`), WMINST-002, WMINST-007, WMINST-011 (the Apps dialog), WMINST-013 (install method), WMINST-014 (tetron-browser from its release), WMINST-015 (release fallbacks, failed batches), WMCAT-001 to WMCAT-005 (catalog, Update list, Store install order), in `spec/`.
- Related docs: `docs/INSTALL_tetron-wm_methods_and_sizes.md` (install methods and setups), `docs/DEPENDENCIES_tetron-wm_recommended_installs.md` (what each app is for).
