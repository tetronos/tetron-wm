"""App catalog requirements for tetron-wm (WMCAT-*): source lists, the merge,
tiers and sets, and the Store's Update list fetch."""
from libspec import Requirement


class CatalogFromSourceLists(Requirement):
    """REQUIREMENT-ID: WMCAT-001

    The Store catalog is built from source lists in `lists/`, never edited in
    `assets/` by hand. `lists/awesome-tuis.json` is written only by
    `scripts/import_awesome_tuis.py` from the awesome-tuis README.
    `lists/tetron.toml` is ours, hand-maintained: `[[app]]` blocks add apps
    found on no other list or annotate an app from any list (any catalog
    field, the recipe, tier), and `exclude` drops imported names (duplicates,
    libraries). `scripts/merge_catalog.py` (stdlib) merges them into
    `assets/catalog.json`: one entry per app, matched by name (any case),
    then by command name; ours wins field by field; each entry records its
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
