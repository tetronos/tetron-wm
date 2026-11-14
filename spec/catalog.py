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
    file. `src/catalog.rs` reads one document: `{"schema": N, "apps": [...],
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
