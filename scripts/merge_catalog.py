#!/usr/bin/env python3
"""Merge the source lists into assets/catalog.json (WMCAT-001 to WMCAT-003).

Usage:
    python3 scripts/merge_catalog.py           # write assets/catalog.json
    python3 scripts/merge_catalog.py --check   # exit 1 when it is stale

Inputs: the imported lists (lists/<name>.json, in IMPORTED order), our
lists/tetron.toml, assets/suite-apps.txt (scripts/gen_suite_lists.py),
which gives each app its set and is carried whole as "suite_apps", and
lists/repology.json (scripts/match_repology.py, WMCAT-005): package names per
manager (apt, dnf, pacman, brew) by app name; a `pkg` table of ours overrides
them name by name ("" means that manager has none).

Matching: by name (any case), then by command name; an entry of ours with
distinct = true is a different app and never matches. Ours wins field by
field; among imported lists the earlier one wins. Stdlib only (Python 3.11+).
"""
import json
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
LISTS = ROOT / "lists"
OUT = ROOT / "assets" / "catalog.json"
SUITE = ROOT / "assets" / "suite-apps.txt"

SCHEMA = 1
IMPORTED = ["awesome-tuis"]
REQUIRED = ["name", "bin", "category", "description", "homepage"]
OPTIONAL = ["variants", "cli", "needs_url", "recipe", "tier", "distinct", "pkg", "set"]
MANAGERS = ("apt", "dnf", "pacman", "brew")
RECIPE_KEYS = {"install", "method", "verified", "os", "requires_cwd", "tip"}
TIERS = {"default", "recommended", "optional"}
# Suite sizes (WMINST-001) and the catalog tier an app of each gets.
SIZES = {"essentials": "default", "recommended": "recommended", "extras": "optional"}
ORDER = ["name", "bin", "variants", "category", "description", "homepage", "cli", "needs_url",
         "recipe", "pkg", "tier", "set", "sources"]


def fail(msg):
    sys.exit(f"merge_catalog: {msg}")


def suite_sets(text):
    """Command name -> smallest size (essentials, recommended, extras) in the suite list."""
    rank = {s: i for i, s in enumerate(SIZES)}
    sets = {}
    for line in text.splitlines():
        if not line or line.startswith("#"):
            continue
        f = line.split(":")
        if f[0] not in rank or len(f) < 3 or f[1] in ("services", "release"):
            continue
        names = [f[2]] if f[1] == "script" else [f[2]] + [n for n in f[3:] if n]
        for n in names:
            for alt in n.split("|"):
                if alt and (alt not in sets or rank[f[0]] < rank[sets[alt]]):
                    sets[alt] = f[0]
    return sets


def check_ours(app, i):
    if "name" not in app:
        fail(f"lists/tetron.toml app #{i + 1} has no name")
    extra = set(app) - set(REQUIRED) - set(OPTIONAL)
    if extra:
        fail(f"{app['name']}: unknown fields {sorted(extra)}")
    if "tier" in app and app["tier"] not in TIERS:
        fail(f"{app['name']}: tier must be one of {sorted(TIERS)}")
    if "set" in app and app["set"] not in SIZES:
        fail(f"{app['name']}: set must be one of {list(SIZES)}")
    if "recipe" in app and (set(app["recipe"]) - RECIPE_KEYS or "install" not in app["recipe"]):
        fail(f"{app['name']}: recipe needs install and only {sorted(RECIPE_KEYS)}")
    if "pkg" in app and (set(app["pkg"]) - set(MANAGERS) or not all(isinstance(v, str) for v in app["pkg"].values())):
        fail(f"{app['name']}: pkg takes only {list(MANAGERS)} as strings")


def merged_apps(lists=LISTS):
    """Every app of the lists merged, with its package names (pkg), before
    the suite sets give tiers (scripts/gen_suite_lists.py reads this too)."""
    apps, by_name, by_bin = [], {}, {}

    def find(a):
        return by_name.get(a["name"].lower()) or (by_bin.get(a["bin"]) if a.get("bin") else None)

    def add(a):
        apps.append(a)
        by_name.setdefault(a["name"].lower(), a)
        if a.get("bin"):
            by_bin.setdefault(a["bin"], a)

    ours = tomllib.loads((lists / "tetron.toml").read_text(encoding="utf-8"))
    exclude = {n.lower() for n in ours.get("exclude", [])}
    for src in IMPORTED:
        for a in json.loads((lists / f"{src}.json").read_text(encoding="utf-8")):
            if a["name"].lower() in exclude:
                continue
            hit = find(a)
            if hit:
                for k, v in a.items():
                    hit.setdefault(k, v)
                hit["sources"].append(src)
            else:
                add(dict(a, sources=[src]))
    for i, a in enumerate(ours.get("app", [])):
        check_ours(a, i)
        # distinct: another app than a listed one with the same name (any case)
        # or command, so it is added, never matched.
        hit = None if a.pop("distinct", False) else find(a)
        if hit:
            old = hit["name"].lower()
            hit.update(a)
            hit["sources"].append("tetron")
            if a["name"].lower() != old:
                by_name.setdefault(a["name"].lower(), hit)
        else:
            add(dict(a, sources=["tetron"]))

    rp = lists / "repology.json"
    found = json.loads(rp.read_text(encoding="utf-8"))["names"] if rp.exists() else {}
    for a in apps:
        names = {k: v for k, v in {**found.get(a["name"], {}), **a.get("pkg", {})}.items() if v}
        if names:
            a["pkg"] = {k: names[k] for k in MANAGERS if k in names}
        else:
            a.pop("pkg", None)
    return apps


def merge(lists=LISTS, suite_path=SUITE):
    apps = merged_apps(lists)
    suite = suite_path.read_text(encoding="utf-8")
    sets = suite_sets(suite)
    out = []
    for a in apps:
        missing = [k for k in REQUIRED if not a.get(k)]
        if missing:
            fail(f"{a['name']}: missing {missing} (add them in lists/tetron.toml)")
        a.pop("set", None)
        s = sets.get(a["bin"])
        if s:
            a["set"] = s
        a.setdefault("tier", SIZES[s] if s else "optional")
        for k in ("cli", "needs_url"):
            if not a.get(k):
                a.pop(k, None)
        out.append({k: a[k] for k in ORDER if k in a})
    out.sort(key=lambda a: (a["category"].lower(), a["name"].lower()))
    doc = {"schema": SCHEMA, "apps": out, "suite_apps": suite}
    return json.dumps(doc, ensure_ascii=False, indent=1) + "\n"


def catalog_recipes():
    """{name: recipe} from the merged assets/catalog.json (for the recipe tools)."""
    doc = json.loads(OUT.read_text(encoding="utf-8"))
    return {a["name"]: a["recipe"] for a in doc["apps"] if "recipe" in a}


def save_recipes(before, after):
    """Append a recipe block to lists/tetron.toml for every recipe in `after`
    that differs from `before` (a later block for the same app overrides its
    recipe), then merge again. Returns how many were written."""
    q = lambda v: json.dumps(v, ensure_ascii=False)
    blocks = []
    for name, r in sorted(after.items()):
        if before.get(name) == r:
            continue
        blocks += ["", "[[app]]", f"name = {q(name)}", "[app.recipe]"]
        blocks += [f"{k} = {q(r[k])}" for k in ("install", "method", "verified", "os", "requires_cwd", "tip") if k in r]
    if blocks:
        with open(LISTS / "tetron.toml", "a", encoding="utf-8") as f:
            f.write("\n".join(blocks) + "\n")
        OUT.write_text(merge(), encoding="utf-8")
    return blocks.count("[[app]]")


def main():
    text = merge()
    if "--check" in sys.argv[1:]:
        if not OUT.exists() or OUT.read_text(encoding="utf-8") != text:
            fail("assets/catalog.json is stale; run python3 scripts/merge_catalog.py")
        return 0
    OUT.write_text(text, encoding="utf-8")
    print(f"wrote {OUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
