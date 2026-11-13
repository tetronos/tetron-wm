"""Packaging and install requirements for tetron-wm (WMDEB-*, WMINST-*).
Only packaging lives here; the rest of tetron-wm follows CLAUDE.md."""
from libspec import Requirement


class MissingToolAudit(Requirement):
    """REQUIREMENT-ID: WMDEB-001

    Every outside program tetron-wm runs is checked before use or fails
    visibly: no crash, no hang of the render loop, no menu item that fails
    silently. Every spawn goes through `run_capped`/`run_capped_status` or
    runs off the render loop.
    """


class ThemeSearchList(Requirement):
    """REQUIREMENT-ID: WMDEB-002

    Theme files are looked up in an ordered list of directories; a name in an
    earlier directory wins:
    1. `$TETRON_WM_THEMES_DIR` when set, and then only that directory;
    2. `theme_dir` from config.toml when set, else `<config dir>/themes`
       (`~/.config/tetron-wm/themes`);
    3. `$XDG_DATA_HOME/tetron-wm/themes` (default
       `~/.local/share/tetron-wm/themes`, where in-app downloads go);
    4. each `$XDG_DATA_DIRS` entry (default `/usr/local/share:/usr/share`)
       plus `tetron-wm/themes`, for a later themes package.
    Missing directories are skipped. A theme's relative wallpaper path
    resolves against the directory the theme came from. The 4 built-in
    presets need no file. The example config is compiled into the binary;
    `tetron-wm config init` writes it to the config path only when no file
    is there, and says so either way.
    """


class SettingsRework(Requirement):
    """REQUIREMENT-ID: WMDEB-003

    Settings opens on Appearance. General > Config file (open, create from
    the example, reset with a confirm and a .bak, reload). Appearance > Get
    more themes lists `themes/` on main through the GitHub contents API and
    downloads missing .toml/.jpg/.jpeg/.png/.webp into the data dir, off the
    render loop, with a clear message on a 403 rate limit.
    """


class PrefixedHelperNames(Requirement):
    """REQUIREMENT-ID: WMDEB-004

    The bundled helper scripts carry a tetron prefix so no generic name lands
    on PATH: `tetron-wm-startpage` (part of the desktop, its data lives in
    `~/.config/tetron-wm`) and `tetron-aimode` (an independent terminal app).
    They install on PATH like any app (`/usr/bin` from the deb, `~/.local/bin`
    from the suite installer) and the launcher finds them there. No
    compatibility for the old bare names `startpage` and `aimode`.
    """


class DpkgAwareUpdater(Requirement):
    """REQUIREMENT-ID: WMDEB-005

    One install method per host. On a host where dpkg owns tetron-wm
    (`/var/lib/dpkg/info/tetron-wm.list`), nothing writes over the packaged
    binary: Settings > Updates installs the new release .deb with apt in a
    terminal window, and the dev channel refuses. The update check runs off
    the render loop.

    install.sh on a dpkg host: package installed -> the release .deb upgrades
    it through apt, or, with no .deb in the release, nothing changes and it
    exits 3; a raw copy -> upgraded in place with a note; neither -> the
    .deb when the release has one, else the raw binary. The .deb is checked
    against its .sha256 first. Callers (Settings > Updates, Systems > Add
    Remote) never fall back to a source build after exit 3.
    """


class LeanDeb(Requirement):
    """REQUIREMENT-ID: WMDEB-006

    `packaging/deb.toml`: the binary, the helpers `tetron-wm-startpage` and
    `tetron-aimode` in usr/bin (WMDEB-004), and the docs in
    usr/share/doc/tetron-wm (with `packaging/parity.toml`, which the
    TetronOS build checks its Desktop ISO against); Depends computed from the ELF only; no declared
    Depends or Recommends (python3, curl and the feature tools are
    Suggests, by their Debian names); no maintainer scripts. `apt install`
    of the deb on a fresh Debian 13 adds exactly one package. Release CI
    builds it from the released Linux binaries with the build-deb action
    pinned by commit SHA, tests the install in a clean container, and
    attaches the .deb and its .sha256 to the release, named
    `tetron-wm_<version>-<rev>_<arch>.deb` as install.sh expects (WMDEB-005).
    """


class DesktopMetapackage(Requirement):
    """REQUIREMENT-ID: WMDEB-007

    `tetron-wm-desktop` (`packaging/desktop.toml`, a metapackage: no
    binary, Architecture all) Depends `tetron-wm (>= {minor})`, so any
    later tetron-wm satisfies it and it never blocks an upgrade, and
    Recommends the apps tetron-wm calls (the desktop set of WMINST-001), by
    names present in Debian 12 and 13 and Ubuntu 22.04 and 24.04 (apt installs Recommends by default, so
    `apt install tetron-wm-desktop` brings the full set). One Recommends
    is the exception to those names: tetron-browser (amd64), the web
    browser tetron-wm opens pages in, comes from the tetronos apt repo
    (https://tetronos.github.io/apt); apt skips it where that repo is not
    configured or has no build for the architecture, and the suite
    script's one-by-one apt path skips every name apt has no candidate
    for, so a missing one never fails the batch. Services
    (pipewire, wireplumber, network-manager, bluez) and kmscon are
    Suggests, as are age and openssh-server (an SSH server must never
    start just because the desktop set was installed). Tools Debian does not package (carbonyl, fresh,
    casttui, catt, uv, opencode, a symbols Nerd Font) stay with the
    installer scripts; catt and casttui join the suite script's desktop
    tier (WMDEB-008). Release CI builds it once, on the amd64 job, and tests it by
    installing it together with that job's tetron-wm .deb in a clean
    container with the tetronos apt repo configured, Recommends included.

    It ships two systemd units and enables neither (no maintainer scripts):
    the full screen waiter template
    /usr/lib/systemd/system/tetron-wm-fullscreen@.service (instance = user
    name, `User=%i`, VT 9, `/usr/bin/tetron-wm fullscreen-waiter --vt 9`)
    and the app server user unit
    /usr/lib/systemd/user/tetron-wm-apphost.service (through a login shell,
    for the PATH and locale of a login). Settings > Windows switches the
    waiter instance of the running user (the unit an older suite installer
    wrote, `tetron-wm-fullscreen.service`, wins where it exists); the daemon
    starts the apphost unit when it is enabled for the user and otherwise
    spawns the apphost itself. The suite script enables the template
    instance when tetron-wm-desktop is installed and the VT is 9, replacing
    a unit it wrote earlier.
    """


class EssentialsMetapackage(Requirement):
    """REQUIREMENT-ID: WMDEB-010

    `tetron-wm-essentials` (`packaging/essentials.toml`, a metapackage,
    Architecture all) Depends `tetron-wm-desktop (>= {minor})` and
    Recommends the essentials set of WMINST-001: one good pick per need
    (files and disks, system, an editor, web, network, images,
    programming, sound). tetron-wm-desktop keeps the tools tetron-wm's own
    features call and the systemd units (WMDEB-007), so no file moves
    between packages. A Recommends missing on a release is skipped by apt.
    """


class RecommendedMetapackage(Requirement):
    """REQUIREMENT-ID: WMDEB-011

    `tetron-wm-recommended` (`packaging/recommended.toml`, a metapackage,
    Architecture all) Depends `tetron-wm-essentials (>= {minor})` and
    Recommends the recommended set of WMINST-001: a curated pick in every
    category. A package it brings that has a service or a firewall is
    installed, never enabled, by this package.
    """


class ExtrasMetapackage(Requirement):
    """REQUIREMENT-ID: WMDEB-009

    `tetron-wm-extras` (`packaging/extras.toml`, a metapackage, Architecture
    all) Depends `tetron-wm-recommended (>= {minor})` and Recommends the
    extras set of WMINST-001, apps added over time beyond the recommended
    set (empty while none is chosen). Four sizes result: minimal
    (tetron-wm), essentials (tetron-wm-desktop and tetron-wm-essentials),
    recommended, extras. Release CI builds the four metapackages once, on
    the amd64 job, and checks their Recommends install together with the
    tetron-wm deb.
    """


class SuiteScriptOnDpkgHost(Requirement):
    """REQUIREMENT-ID: WMDEB-008

    The suite script installs the deb on a dpkg host, converts a raw install,
    downloads themes through the same directory listing, and ends with one
    tetron-wm on the host. Carried out by WMINST-002 (install matrix plan).
    """


class AppListSingleSource(Requirement):
    """REQUIREMENT-ID: WMINST-001

    The app sets are defined once, in the "App sets" section of
    `lists/tetron.toml`: four sets, desktop (the tools tetron-wm's
    features call), essentials, recommended and extras. A `[[package]]`
    entry is a member that is not a Store app: its Debian name, the check
    that tells it is present (a command, `font=NAME`, or `consolefonts`;
    none: in the deb only), the Fedora, Arch and Homebrew names where they
    differ (an empty name: not on that OS), each verified in a container or
    against Homebrew, an optional group (gui, services) and the deb
    relation where it is not the plain name. An `[[app]]` with `set` is a
    Store app in a set: its bin is the check and its package names are its
    merged `pkg` (Repology plus our overrides, WMCAT-005). A `[[script]]`
    entry is an app no distro packages (catt, casttui, fresh, opencode, the
    Nerd Font, the helper scripts on a raw install), installed by the suite
    script's own method. `scripts/gen_suite_lists.py` (stdlib) writes
    `assets/suite-apps.txt` from them, one
    `size:group:check:apt:dnf:pacman:brew` line per member with a check and
    `size:script:name` per script entry, where size is the suite size of
    the set (desktop and essentials: essentials), and writes the
    `recommends` array of `packaging/<set>.toml` for each of the four
    metapackages (services stay hand-written Suggests and must be listed
    there). A name listed twice, an unknown field or set fails the
    generator. The catalog tier of a Store app in a set follows its size:
    essentials default, recommended recommended, extras optional.
    """


class SuiteScriptSizes(Requirement):
    """REQUIREMENT-ID: WMINST-002

    Depends on WMINST-001 and WMDEB-005; carries out WMDEB-008 and the plan's
    WMINST-003 (other systems), since both replace the same hand-written
    lists. `install-tetron-wm-suite.sh`:

    - Asks every user the size: minimal (tetron-wm only), essentials (the
      default, preselected), recommended or extras; `--size` answers it,
      `-y` takes essentials, `--no-extras` means minimal, `--extras` keeps
      the default. The older size name desktop is read as essentials.
    - Reads the apps from `assets/suite-apps.txt` (the checkout's copy when
      run from one, else fetched from the branch), never from lists of its
      own. A size installs its lines and the smaller sizes' lines; gui
      lines only with a desktop session; services lines only after their
      own question (default no).
    - On a Debian or Ubuntu host where tetron-wm is the apt package and the
      release has the size's metapackages (essentials: tetron-wm-desktop and
      tetron-wm-essentials; each larger size adds its own), installs them from the
      release (checked against its .sha256) with apt, so apt tracks the set,
      with Recommends forced on (a system may turn them off; TetronOS images
      do). It then installs by name, marked automatic, any Recommends group
      of the installed size debs that is still missing with a candidate (a
      rerun over a size installed without its Recommends), and ends with
      exit status 1, naming them, when any stays missing.
      Otherwise (tetron-wm not from the package, or no metapackage in the
      release) it installs the same apps one by one with the package
      manager. A metapackage is never installed next to a raw tetron-wm,
      since it would add a second copy.
    - On a dpkg host where tetron-wm is a raw copy and the release has the
      tetron-wm .deb, offers to convert it (default no; `--convert` answers
      yes): removes the raw copy, then installs the package.
    - Installs the script-only apps of the size (fresh, uv where not
      packaged, catt, casttui, the symbols Nerd Font, and the helper scripts
      when tetron-wm is not the deb), unless `--no-scripts` is given, and
      calls install.sh with its own dependency step skipped.
    - Downloads the themes listed in `themes/` on the branch (GitHub
      contents API) into `~/.local/share/tetron-wm/themes`, falling back to
      the built-in list when the listing fails; never overwrites a file.
    - Skips gpm on a Linux machine without a mouse device
      (/dev/input/mice): Debian's gpm fails to configure there and leaves a
      broken package (gpm stays a desktop Recommends; the suite installer
      guards it). One by one it is left out; through the
      metapackage it is excluded with `gpm-` in the same apt command. Systems > Add Remote
      installs gpm on a remote only when it has a mouse device too (over
      ssh the mouse comes from the local terminal, not gpm).
    - Records `size` and `method` (deb or raw) in `suite-state`.
    - `--dry-run` prints the size, method and every package it would
      install. Release URL and tag can be overridden (tests).
    """


class InstallShBinaryOnly(Requirement):
    """REQUIREMENT-ID: WMINST-004

    `install.sh` installs only tetron-wm: the deb or the raw binary per
    WMDEB-005. Its optional-dependency step is removed (the apps belong to
    the suite script's sizes); at the end it prints the suite script
    command for the apps, unless the suite script called it
    (TETRON_WM_SKIP_DEPS=1). TETRON_WM_INSTALL_DEPS is no longer read.
    When it installs or upgrades the tetron-wm deb and any of the
    metapackages tetron-wm-desktop, -essentials, -recommended or -extras is
    installed, the release's matching metapackage debs
    are checked against their .sha256 and installed in the same apt
    command, so a release's new Recommends reach existing machines. Systems > Add Remote no longer sets
    TETRON_WM_INSTALL_DEPS.
    """


class InstallMethodAndSize(Requirement):
    """REQUIREMENT-ID: WMINST-005

    tetron-wm knows how it was installed and which size is present, without
    a shell-out: the method is "apt package" when dpkg owns the running
    binary, "cargo" when it runs from a cargo bin directory, else "binary";
    the size is extras, recommended or essentials when dpkg has the
    tetron-wm-extras, tetron-wm-recommended or tetron-wm-essentials
    package, else the `size` in the suite script's
    `~/.config/tetron-wm/suite-state` (desktop read as essentials), else
    minimal; tetron-wm-desktop alone (an install from before the sets) is
    minimal plus tools, shown as minimal so the row offers essentials. Settings > About shows
    both. `packaging/desktop.toml` suggests `kmscon (>= 9.1)`, since Debian
    12 ships kmscon 9.0.0 without truecolor and mouse passthrough.
    """


class SettingsAppsRow(Requirement):
    """REQUIREMENT-ID: WMINST-007

    Depends on WMINST-002 and WMINST-005. Settings > General has an Apps
    row showing the installed size and how many apps of it are missing
    ("essentials, 3 missing"), by the checks of `assets/suite-apps.txt`
    (compiled into the binary): the rows of that size and the smaller
    ones, package rows only where this OS has a name for them, gui rows
    only with a desktop session, gpm only with a mouse device, never the
    services, and the helper scripts only when tetron-wm is not the deb.
    So a size installed through apt alone, which brings no script-only
    app, shows what the suite script would still add. The row re-reads the
    size when General is entered. Enter opens the Apps dialog (WMINST-011).
    """


class SettingsAppsDialog(Requirement):
    """REQUIREMENT-ID: WMINST-011

    Depends on WMINST-007. The Apps dialog, inside Settings, lists
    Essentials, Recommended and Extras (Extras only when the app list has
    extras apps), each with what it adds and how many of its apps are
    missing here (PATH is scanned again when the dialog opens, and
    /usr/sbin and /sbin count, which a Debian user's PATH lacks); a
    checkbox "also the apps with their own installers", ticked by default;
    and, on a Debian or Ubuntu host where tetron-wm-essentials is installed,
    "Remove set: <installed size>". Up and Down move, Enter or Space
    chooses, Left / Right or Space flip the checkbox, Esc closes; section
    changes wait until it is closed.

    Every choice opens a confirmation first, with focus on Cancel (Left /
    Right switch the buttons, Esc goes back). Installing a size shows the
    missing apps it adds (a script app marked "its own installer"), their
    count, on an apt host the download size of the packages among them
    (`apt-cache policy` keeps the names this release has and that are not
    installed, `apt-get
    install --print-uris` sums the sizes, run as the user off the main
    loop), that it needs the network and the sudo password, and how to
    undo it. With no package lists (apt has no candidate for any name) the
    size reads "unknown". In a live session (a live medium and an overlay
    or tmpfs root, so changes are kept in RAM) it also gives the free room
    on / and, when apt has said, what the set needs (download plus apt's
    disk growth plus 150 MB per app with its own installer), with a NOT
    ENOUGH ROOM warning when it does not fit; Install stays possible. Install runs `install-tetron-wm-suite.sh --size
    <size>` of this binary's release tag (main only when that tag does not
    exist, a local build), so the installer knows the sizes this Settings
    offers, in a terminal window as before, with `--no-scripts` when the box is not
    ticked, which leaves out the script apps. Without curl a size does
    nothing and says it needs curl. Remove set lists the size debs it
    removes: the installed size's and the larger ones dpkg has, never
    tetron-wm-desktop; then runs `sudo apt-get remove <debs> && sudo
    apt-get autoremove` in a terminal window without -y, so apt lists the
    apps and asks. Apps the user installed by hand stay (apt marks them
    manual), and so do the script apps, which the confirmation names. All
    text wraps at the panel width. On Fedora, Arch and macOS there is
    no Remove set: nothing records which apps a set added.
    """


class ScriptAppsFromAptFirst(Requirement):
    """REQUIREMENT-ID: WMINST-008

    On an apt host, the suite script installs a script-only app with apt
    when apt has a candidate for its package (the tetronos apt repo is
    configured): fresh as fresh-editor (upstream's own .deb, mirrored
    unchanged), the Nerd Font as tetronos-nerd-font-symbols, casttui as
    casttui. Only where apt has none does it use the app's own method
    (fresh's installer, the Nerd Font download into ~/.local/share/fonts,
    `uv tool install`). uv and catt keep their own methods everywhere:
    each has its own updater (WMINST-009).
    """


class ToolSelfUpdates(Requirement):
    """REQUIREMENT-ID: WMINST-009

    Settings > Updates (every update command that installs something) first
    updates the apps that keep their own updater: when uv is on PATH or in
    ~/.local/bin it runs `uv self update` (only for a uv no package owns;
    errors ignored) and `uv tool upgrade --all` (catt, and casttui where it
    came from uv), logs the step to the tetron-wm log, and goes on to
    the tetron-wm update whatever their result.
    """


class OpencodeForTheAssistant(Requirement):
    """REQUIREMENT-ID: WMINST-010

    opencode, the agent the ✦ Assistant runs by default, comes with the
    essentials size: an `essentials:script:opencode` row in the app list. On Linux
    the suite script takes upstream's GitHub "latest" release, its stable
    channel (the one its own installer and npm `latest` follow), downloads
    the tarball for this CPU (opencode-linux-x64, x64-baseline without
    AVX2, arm64, with -musl on musl systems), installs it into ~/.local/bin
    only when it matches the sha256 digest GitHub records for the asset
    (1.x releases carry no SHA256SUMS); on macOS Homebrew. Settings >
    Updates, after the uv step, replaces ~/.local/bin/opencode the same way
    when that release differs from `opencode --version` (not `opencode
    upgrade`, which installs a second copy in ~/.opencode/bin). Settings >
    General > Apps counts it when missing.
    """


class RecommendedScriptApps(Requirement):
    """REQUIREMENT-ID: WMINST-012

    Depends on WMINST-002 and WMINST-009. The recommended size brings five
    apps no distribution packages, as `recommended:script:<name>` rows in
    the app list (left out with --no-scripts, counted by Settings > General
    > Apps when missing): pyradio (`uv tool install` from its git
    repository with its Python dependencies), llm (`uv tool install llm
    --with llm-ollama`) and yt-dlp (`uv tool install
    yt-dlp[default,deno]`, deno being the JS runtime it needs for YouTube,
    kept inside the tool's venv),
    which Settings > Updates upgrades with the other uv tools (yt-dlp
    counts as present only in ~/.local/bin: a distribution's copy, such as
    the one mpv's Recommends brings on Debian, falls behind the sites); croft, its
    musl release for this CPU, installed into ~/.local/bin only when it
    matches the release's SHA256SUMS (croft updates itself); freebuff, the
    native binary its npm launcher would fetch from codebuff.com
    (linux-x64, x64-baseline without AVX2, arm64), installed only when it
    matches the sha256 the npm package lists for that target
    (binaryChecksums), into ~/.local/share/freebuff with tree-sitter.wasm
    and a version file, linked from ~/.local/bin/freebuff, so no node is
    needed. Settings > Updates, after the opencode step, replaces it the
    same way when npm lists another version. croft and freebuff are Linux
    only here.
    """


class InstallGuide(Requirement):
    """REQUIREMENT-ID: WMINST-006

    `docs/INSTALL_tetron-wm_methods_and_sizes.md` is the user-facing guide:
    the ways to install (suite script, install.sh, debs, source), the three
    sizes and what each brings, the setups (console levels), the matrix of
    method by size, how each is updated and removed, and how to grow the
    size later (suite script `--size`, Settings > General > Apps). The
    README's Install section links to it.
    """


class InstallMethodChoice(Requirement):
    """REQUIREMENT-ID: WMINST-013

    Depends on WMDEB-005, WMINST-002 and WMINST-005. One install method per
    host, chosen by the user: `deb` (the tetron-wm package, managed by apt)
    or `binary` (a release binary, or an own build from a clone). Both
    installers take it: `install.sh` reads `TETRON_WM_METHOD` (deb |
    binary) and `TETRON_WM_FROM_CLONE` (a tetron-wm checkout, implies
    binary); the suite script takes `--method deb|binary` and
    `--from-clone DIR` and passes them on, and the tetronos installer
    (`tetronos.github.io/install.sh`) hands its arguments to the suite
    script. Without a method install.sh keeps WMDEB-005, except on a host
    with an own build, where it changes nothing and exits 4 so a rerun or
    an update never writes over the own build.

    deb: on a Debian or Ubuntu host the release .deb is installed as in
    WMDEB-005, and the tetronos apt repository is added so `apt upgrade`
    updates tetron-wm from then on. The repository files belong to the
    `tetronos-apt-source` package (its sources file is a conffile), so
    the installer never writes them: it downloads the archive key, checks
    it against the SHA256 pinned in install.sh, lets apt check the
    repository with that key from a temporary source list, and installs
    `tetronos-apt-source` from it, keeping a sources file the user made
    (`--force-confold`). Skipped when the package is installed (TetronOS),
    when another sources file already names the repository, with
    `TETRON_WM_NO_APT_REPO=1`, or with a local release
    (`TETRON_WM_RELEASES_URL`); a failure is a warning and the release
    .deb still installs. An own build is undone first (below); a raw copy
    is removed and user units that name it are pointed at
    `/usr/bin/tetron-wm`. deb on a host without dpkg and apt-get exits 2.

    binary on a host where the tetron-wm package is installed keeps the
    package and diverts its binary (`dpkg-divert --local --rename
    --divert /usr/bin/tetron-wm.packaged`), then puts the binary at
    `/usr/bin/tetron-wm`: metapackages (tetron-wm-desktop and the sizes,
    tetronos-desktop on TetronOS), system units and autostart keep working
    and run it, and an apt upgrade of the package writes only the
    diverted file. Without the package the binary replaces a raw copy
    where it is (keeping user units), else goes to TETRON_WM_BIN_DIR.
    Going back to deb removes the own binary and the diversion
    (`--rename`, so the packaged binary returns; reinstalled when it is
    missing). Repository files stay when switching to binary.

    Own build: `cargo build --release` in the clone, then
    `target/release/tetron-wm --version` must run; a failed build or check
    changes nothing. Without cargo it exits 1 and names rustup.

    Every new binary is copied beside its target and renamed over it, so a
    running tetron-wm is never written into. A live session (overlay or
    tmpfs root) gets a warning that the change is lost at reboot.

    tetron-wm reads the diversion from `/var/lib/dpkg/diversions`: a
    running `/usr/bin/tetron-wm` diverted there is not the package, so
    Settings > About shows "own build (package diverted)", and Settings >
    Updates does not run apt or install.sh; it shows the commands to
    rebuild from the clone or to go back to deb. The suite script records
    `method deb | binary | own` in `suite-state`, offers the choice when
    asked (`--method`, else a question on a Debian or Ubuntu host that
    has a tty, default the current method), `--convert` is read as
    `--method deb`, and `--dry-run` prints the method and the clone.
    `--uninstall --purge` on a diverted host removes the own binary and
    the diversion before removing the packages.
    """


class TetronBrowserFromRelease(Requirement):
    """REQUIREMENT-ID: WMINST-014

    Depends on WMINST-002 and WMINST-013. tetron-browser, in the desktop
    set, is a package of the tetronos apt repository only, which is added
    by the deb method alone. When the suite script installs a size with
    it and it is still missing afterwards on Linux x86_64 (apt had no
    candidate, or another distribution), the script installs the latest
    tetron-browser release: the .deb with apt on Debian and Ubuntu, else
    the zip unpacked into `~/.local/share/tetron-browser` with
    `~/.local/bin/tetron-browser` linked to it; each checked against its
    .sha256 first. A failure names tetron-browser in the final problems
    (exit 1) instead of passing silently. `--dry-run` lists it.
    """


class SuiteAppsFallBackAndReportByState(Requirement):
    """REQUIREMENT-ID: WMINST-015

    Depends on WMINST-002 and WMINST-014. A member of the chosen app
    collection that is still missing after the package manager ran (the
    distribution has no package of that name, for example Ubuntu 24.04
    lacks fastfetch, glow, xh and lazygit) is installed from the GitHub
    release named for it in `[[release]]` of `lists/tetron.toml`: a .deb
    with apt on Debian and Ubuntu, or a .tar.gz whose command goes to
    `~/.local/bin`. The asset must match the SHA256 digest GitHub lists for
    it, or it is not installed. Then brew, when installed, for what is
    still missing. Members missing after that are named in one line.
    Presence counts `~/.local/bin`, `/usr/sbin` and `/sbin` as well as
    PATH. A failed package batch is judged by what the package manager
    lists as installed, not by its exit status: the packages not installed
    are retried one by one and only those still missing are named. With
    `--yes`, or without a terminal on stdin, apt runs with
    `DEBIAN_FRONTEND=noninteractive`, so no debconf question (console-setup
    asks one) blocks or fails the batch. A second run installs nothing.
    """
