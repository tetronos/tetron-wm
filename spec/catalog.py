"""App catalog requirements for tetron-wm (WMCAT-*): source lists, the merge,
tiers and sets, and the Store's Update list fetch."""
from libspec import Requirement


class CatalogFromSourceLists(Requirement):
    """REQUIREMENT-ID: WMCAT-001

    The Store catalog is built from source lists in `lists/`, never edited in
    `assets/` by hand. `lists/awesome-tuis.json` is written only by
    `scripts/import_awesome_tuis.py` from the awesome-tuis README, and
    `lists/terminaltrove.json` only by `scripts/import_terminaltrove.py` from
    the terminaltrove.com tool list (the terminaltrove-stars data.json,
    https://carcaradotdev.github.io/terminaltrove-stars/data.json, since the
    site itself refuses scripted fetches). The terminaltrove import keeps
    every tool that runs on Linux or macOS and is not archived, whatever its
    age; a tool without the `tui` tag is `cli = true`. Imported apps are
    unreviewed (`optional` unless ours says otherwise).
    `lists/tetron.toml` is ours, hand-maintained: `[[app]]` blocks add apps
    found on no other list or annotate an app from any list (any catalog
    field, the recipe, tier), and `exclude` drops imported names (duplicates,
    libraries). `scripts/merge_catalog.py` (stdlib) merges them into
    `assets/catalog.json`: one entry per app, matched by name (any case),
    then by command name, then by GitHub repository of the homepage (any
    case); ours wins field by field, among imported lists the earlier one
    (awesome-tuis before terminaltrove); each entry records its
    `sources`; entries sorted by category, then name. An import never
    overwrites our decisions.
    """


class RecipesInTheCatalog(Requirement):
    """REQUIREMENT-ID: WMCAT-002

    The install recipe is a field of its catalog entry (`recipe`: install,
    method, verified, os, requires_cwd, tip); there is no separate recipes
    file. The `tetron-catalog` crate (`crates/tetron-catalog`, WMCAT-008)
    reads one document: `{"schema": N, "apps": [...],
    "suite_apps": "..."}`. A schema this binary does not know is refused.
    """


class TiersAndSets(Requirement):
    """REQUIREMENT-ID: WMCAT-003

    Each catalog entry carries `tier` (`default`, `recommended` or
    `optional`) and, when the suite installer's app list has it, `set` (the
    smallest size that brings it: `desktop` or `extras`). The set comes from
    `assets/suite-apps.txt` (WMINST-001), matched by command name; the tier
    is the one in `lists/tetron.toml`, else `default` for an app in a set,
    else `optional`. The catalog also carries the whole suite app list as
    `suite_apps`, so one fetch updates the Store and the set lists together.
    The Store shows the set of the selected app.
    """


class UpdateListFetch(Requirement):
    """REQUIREMENT-ID: WMCAT-004

    The Store's Update list button (row 0, also F5) fetches the catalog from
    a versioned location keyed by the core minor of this tetron-wm,
    `https://tetronos.github.io/apt/lists/<major.minor>/catalog.json`, never
    from a moving branch, on a worker thread through `run_capped`, only on
    that press (no automatic or periodic fetch). The download is used only
    when it parses, its schema is the one this binary knows, it has apps and
    its suite list has rows; it is then kept in
    `~/.local/share/tetron-wm/catalog.json` and replaces the built-in copy
    for the Store, the launcher and Settings > General > Apps (WMINST-007).
    Otherwise the built-in copy stays, the reason is shown in the Store and
    logged. The cached copy is read at start and used while it passes the
    same checks. Settings > General > Apps hands the suite script the list it
    used (`--apps-list FILE`); the suite script run on its own fetches
    `suite-apps.txt` from the same versioned location, then from the
    repository, so a list for a newer core never reaches an older one.
    """


class PackageNamesPerManager(Requirement):
    """REQUIREMENT-ID: WMCAT-005

    tetron-wm runs on Debian, Ubuntu, Fedora, Arch and macOS, so a catalog
    entry carries its package name per package manager, `pkg` (`apt`, `dnf`,
    `pacman`, `brew`), never Debian names alone. The names come from Repology
    (`scripts/match_repology.py` writes `lists/repology.json`: apt from Debian
    13, else Ubuntu 24.04, else Debian 12; dnf from the newest Fedora; pacman
    from Arch, not the AUR; brew from Homebrew formulae); a `pkg` table in
    `lists/tetron.toml` overrides them name by name (`""` removes one). A
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
    `lists/releases.json`; the merge sets the flag from it. For such an app
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

    Depends on WMCAT-006. In the Store, a right-click on an app's row (which
    selects it) or on its details, or Alt+O for the selected app, opens a menu
    (the shared right-click menu of `src/contextmenu.rs`, drawn and hit-tested
    like the title bar menu, driven by the same arrow, Enter and Esc keys):
    Install (Launch when installed), Open project page, Copy project link,
    and for an app with a releases page Open releases page and Copy releases
    link. A left click on the project link or the "Prebuilt" line in the
    details opens the menu of that link alone (Open, Copy). Copy puts the link
    in the clipboard, which reaches the real terminal by OSC 52. Open runs the
    first installed terminal web browser (tetron-browser, w3m, lynx, links,
    elinks) with the link as its argument in a new window; with none
    installed the Open entries are left out. The Store's status line says
    what was done. The install-method choice (one entry per way an app can be
    installed here) is meant to join this menu.
    """


class CatalogLibrary(Requirement):
    """REQUIREMENT-ID: WMCAT-008

    The catalog code is a library crate of its own, `tetron-catalog` in
    `crates/tetron-catalog` (a Cargo workspace member), so tetron-wm and the
    standalone Store (tetron-appstore) use one copy. It holds the catalog
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
      (`reload_if_changed`), rebuilds the launcher and refreshes an open
      built-in Store.
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
    the lists; a catalog without it shows no line. Shown by tetron-appstore;
    tetron-wm's built-in Store, which phase 3 removes, does not.
    """


class StoreOpensTheApp(Requirement):
    """REQUIREMENT-ID: WMCAT-011

    Depends on WMCAT-009. tetron-wm opens the Store as the standalone
    program: every way to open it (leader S, the launcher's Store entry and
    `@store`, `tetron-wm msg '"OpenStore"'`, the assistant when its agent is
    missing, Settings and the tray opening it for one app) runs the command in
    the config key `store_app` (default `tetron-appstore`) in a window named
    "Store", with the app's name as its argument when opened for one app.
    With a Store window of that program open, opening the Store focuses it
    instead of starting a second one; opening it for one app replaces it with
    one started for that app (a running program can not be handed a search).
    When `store_app` is empty, or its command is not installed, the built-in
    Store opens as before, so no one is left without a Store. Every choice is
    logged.
    """

