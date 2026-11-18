"""App catalog requirements for tetron-wm (WMCAT-*): source lists, the merge,
tiers and sets, and the Store's Update list fetch.

The catalog and its collections live in the tetron-appstore repository
(https://github.com/tetronos/tetron-appstore, WMCAT-012), whose own spec
(`spec/` there, AS*) holds the file formats, the build, the checks, the
publishing and the client. The paths `apps/`, `collections/`, `sources/`,
`scripts/`, `assets/catalog.json` and `crates/tetron-catalog` named below
are there. tetron-wm keeps no app list of its own."""
from libspec import Requirement


class CatalogFromSourceLists(Requirement):
    """REQUIREMENT-ID: WMCAT-001

    The Store catalog is built from one file per app (`apps/<id>.toml`) and
    one list per collection (`collections/<name>.toml`) in tetron-appstore,
    never edited in `assets/` by hand; the imported lists (awesome-tuis,
    Terminal Trove) and Repology and releases data are inputs its scripts
    turn into app files. The rules are tetron-appstore's spec; tetron-wm
    relies only on the catalog it gets through the `tetron-catalog` crate
    (WMCAT-008).
    """


class RecipesInTheCatalog(Requirement):
    """REQUIREMENT-ID: WMCAT-002

    The install recipe is a field of its catalog entry (`recipe`: install,
    method, verified, os, requires_cwd, tip); there is no separate recipes
    file. The `tetron-catalog` crate (WMCAT-008) reads one document:
    `{"schema": ..., "apps": [...], "collections": [...], "suite_apps":
    "..."}`; schema is a number or "major.minor", and a major this binary
    does not know is refused.
    """


class TiersAndSets(Requirement):
    """REQUIREMENT-ID: WMCAT-003

    Each catalog entry carries `tier` (`default`, `recommended` or
    `optional`), `collection` (the smallest collection that has it, when
    one does) and, for the released clients, `set`. The catalog carries the
    collections (desktop, essentials, recommended, extras: name,
    description, the collection it includes, its apps and the package
    Recommends of its metapackage) and the whole suite app list as
    `suite_apps`, so one fetch updates the Store, Settings > App
    Collections and the suite installer's list together. The Store shows
    the collection of the selected app.
    """


class UpdateListFetch(Requirement):
    """REQUIREMENT-ID: WMCAT-004

    The catalog update is the crate's (`tetron_catalog::update`): sources,
    checks, the last good copy, the packaged copy and the daily check are
    tetron-appstore's spec (ASCLI-001 to ASCLI-006). The Store runs it on
    F5, and both the Store and the tetron-wm daemon run it at start when
    the last check is a day old, on a worker thread. The copy in use is
    `~/.local/share/tetron-wm/catalog.json`; tetron-wm re-reads it when it
    changes (WMCAT-009), for the launcher, the assistant's agents and
    Settings > App Collections (WMINST-007). When an update found a catalog
    in a newer format than this binary reads, Settings > App Collections
    shows the crate's note to update tetron-wm. Settings > App Collections
    hands the suite script the list it used (`--apps-list FILE`); the suite
    script run on its own takes `suite-apps.txt` from tetron-appstore's
    published catalog (GitHub Pages), then the list published for its core
    minor (`https://tetronos.github.io/apt/lists/<major.minor>/`), then an
    installed tetron-wm (`tetron-wm suite-apps`), then tetron-appstore's
    main branch.
    """


class PackageNamesPerManager(Requirement):
    """REQUIREMENT-ID: WMCAT-005

    tetron-wm runs on Debian, Ubuntu, Fedora, Arch and macOS, so a catalog
    entry carries its package name per package manager, `pkg` (`apt`, `dnf`,
    `pacman`, `brew`), never Debian names alone. The names come from Repology
    (`scripts/match_repology.py` writes `sources/repology.json`: apt from Debian
    13, else Ubuntu 24.04, else Debian 12; dnf from the newest Fedora; pacman
    from Arch, not the AUR; brew from Homebrew formulae); an app file's
    `[install.pkg]` overrides them name by name (`""` removes one). A
    recipe is the upstream way only (binary, cargo, go, uv, script, …), never a
    distribution package manager command.

    The Store installs with the host's own package manager first, when it has
    the package (Homebrew on macOS; apt, dnf or pacman on Linux, checked with
    the manager before installing, so a name missing on one release falls
    through), else the verified upstream recipe, else Homebrew on Linux as
    the last resort, else a best-effort chain (the host's manager by command
    name, cargo, go, an installed brew). A Homebrew installed by the Store is
    loaded into the same shell before the app install, and an installed brew
    not on PATH is used rather than installed again. Toolchains (Go, Node,
    Python) come from the distribution's manager first. Only plain package
    names reach the shell. An app with a package for this OS is listed even
    when its recipe names other systems.
    """


class PrebuiltReleaseLink(Requirement):
    """REQUIREMENT-ID: WMCAT-006

    A catalog entry whose homepage is a GitHub repository root carries
    `releases: true` when that repository's latest release has downloadable
    assets (prebuilt binaries). `scripts/match_releases.py` (gh CLI, cached
    in `target/`) writes the repositories that have them to
    `sources/releases.json`; the merge sets the flag from it. For such an app
    the Store shows the link `https://github.com/<owner>/<repo>/releases/latest`
    in its details, and every install window for it (a recipe, a package or
    the best-effort chain) ends by naming that link next to the homepage,
    so a user whose install failed can download the binary by hand. The link
    is built only from a homepage whose owner and repository are plain names
    (letters, digits, `.`, `_`, `-`). Absent `releases` means false, so
    older catalogs and binaries are unaffected.
    """


class StoreAppMenu(Requirement):
    """REQUIREMENT-ID: WMCAT-007

    Depends on WMCAT-006. In the Store (tetron-appstore), a right-click on an
    app's row (which selects it) or on its details, or Alt+O for the selected
    app, opens a menu kept inside the Store's window, driven by the arrow,
    Enter and Esc keys and the mouse: Install (Launch when installed), Open
    project page, Copy project link, and for an app with a releases page Open
    releases page and Copy releases link. A left click on the project link or
    the "Prebuilt" line in the details opens the menu of that link alone
    (Open, Copy). Copy puts the link in the clipboard by OSC 52, which
    tetron-wm records and passes on to the real terminal. Open runs the first
    installed terminal web browser (tetron-browser, w3m, lynx, links,
    elinks) with the link, in a tetron-wm window inside tetron-wm; with none
    installed the Open entries are left out. The Store's status line says
    what was done. The install-method choice (one entry per way an app can be
    installed here) is meant to join this menu.
    """


class CatalogLibrary(Requirement):
    """REQUIREMENT-ID: WMCAT-008

    The catalog code is a library crate of its own, `tetron-catalog` in
    tetron-appstore's `crates/tetron-catalog` (a member of its Cargo
    workspace), so tetron-wm (a git dependency, pinned by Cargo.lock) and the
    standalone Store use one copy. It holds the catalog
    types, parsing and the built-in copy (`assets/catalog.json`), the cached
    Update list copy and its checks, lookups (recipe, category, CLI, URL),
    the `$PATH` scan of installed apps, the releases link (WMCAT-006), the
    toolchain preambles and the Store install command (WMCAT-005). It depends
    on nothing in tetron-wm: it logs through a function tetron-wm hands it at
    start (`set_logger`), and finds its data folder from `XDG_DATA_HOME` or
    `~/.local/share` itself. tetron-wm keeps what is its own: launcher
    entries from installed apps, the Update list fetch (WMCAT-004, through
    `run_capped`) and the Store window. The crate's `builtin-only` feature
    ignores a downloaded catalog; tetron-wm turns it on for its tests only
    (a dev-dependency), so no test reads the user's downloaded copy.
    Behaviour for users is unchanged.
    """


class StandaloneStoreHooks(Requirement):
    """REQUIREMENT-ID: WMCAT-009

    Depends on WMCAT-008. What tetron-wm gives the standalone Store
    (tetron-appstore) so it feels the same inside tetron-wm:
    - Theme: the daemon writes the active theme's Store colours (bg, fg, dim,
      sel, accent, panel, success as `#rrggbb`) and the check mark of the
      icon style in use (`check`; the font may lack ✓) to
      `$XDG_STATE_HOME/tetron-wm/theme-colors.json` (default
      `~/.local/state`, on every OS) whenever they change, by atomic rename;
      the Store re-reads it when its modification time changes.
    - Launch: `ClientMsg::Launch` takes an optional `cwd_prompt`; when true,
      tetron-wm asks for a working folder first, as its launcher does for
      coding agents. The Store opens installs (window title `install: <app>`,
      so closing it rescans installed apps), apps and links with
      `tetron-wm msg`.
    - Catalog: when the shared catalog file (`cache_path`, which the Store's
      Update list writes) changes, the daemon re-reads it within a second
      (`reload_if_changed`) and rebuilds the launcher.
    """


class ListedBy(Requirement):
    """REQUIREMENT-ID: WMCAT-010

    Depends on WMCAT-001. The Store says which lists an app came from: its
    details show "Listed by:" with the imported lists that have it, in merge
    order (awesome-tuis, Terminal Trove), or "tetron" when only our own list
    has it (our list annotates nearly every app, so it is not named
    otherwise). Typing `from:NAME` in the search shows only the apps whose
    shown lists match NAME (any case, any part, so `from:trove` works), and
    combines with the other words typed. The catalog's `sources` field carries
    the lists; a catalog without it shows no line. Shown by tetron-appstore.
    """


class StoreOpensTheApp(Requirement):
    """REQUIREMENT-ID: WMCAT-011

    Depends on WMCAT-009. The Store is the standalone program; tetron-wm has
    no Store of its own. Every way to open it (leader S, the launcher's Store
    entry and `@store`, `tetron-wm msg '"OpenStore"'`, the assistant when its
    agent is missing, Settings and the tray opening it for one app) runs the
    command in the config key `store_app` (default `tetron-appstore`; empty
    means the default) in a window named "Store", with the app's name as its
    argument when opened for one app. With a Store window of that program
    open, opening the Store focuses it instead of starting a second one;
    opening it for one app replaces it with one started for that app (a
    running program can not be handed a search). When the program is not
    installed, tetron-wm opens a window titled "install: tetron-appstore"
    that installs it with its install.sh (the latest release, checked against
    its SHA256, into `~/.local/bin`) and says to press leader S again; closing
    it rescans the installed apps. Every choice is logged.
    """


class CatalogInTheStoreRepo(Requirement):
    """REQUIREMENT-ID: WMCAT-012

    Depends on WMCAT-008 and WMINST-001. The catalog, its collections and
    their sources live in tetron-appstore: the library, the built-in
    `assets/catalog.json` and `assets/suite-apps.txt`, the app files, the
    collection lists, the imported lists, Repology and releases data and
    every import and match script, with CI checking that the catalog is
    current. tetron-appstore publishes the catalog (`catalog-*` tags) and
    builds the tetron-wm-essentials, -recommended and -extras metapackages,
    so a change to an app or a collection needs no tetron-wm release.
    tetron-wm takes the catalog only through the crate it is built with
    (pinned by Cargo.lock), and builds only tetron-wm and tetron-wm-desktop.
    """

