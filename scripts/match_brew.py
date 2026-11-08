#!/usr/bin/env python3
"""Mark brew-installable catalog apps as verified (recipe blocks appended to
lists/tetron.toml, then merged into assets/catalog.json).

Fetches the Homebrew formula index and matches it against the catalog by binary
name / app name, writing a verified `brew install <formula>` recipe for each hit.
Existing verified recipes are left untouched. Run after scripts/import_awesome_tuis.py and scripts/merge_catalog.py.
"""
import json
import re
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))
from merge_catalog import catalog_recipes, save_recipes  # noqa: E402
FORMULA_API = "https://formulae.brew.sh/api/formula.json"


def main() -> int:
    with urllib.request.urlopen(FORMULA_API) as resp:
        formulae = json.load(resp)
    names = {}
    for f in formulae:
        for n in [f["name"], *f.get("aliases", [])]:
            names[n.lower()] = f["name"]

    catalog = json.loads((ROOT / "assets" / "catalog.json").read_text())["apps"]
    recipes = catalog_recipes()
    before = dict(recipes)

    added = 0
    for app in catalog:
        if recipes.get(app["name"], {}).get("verified"):
            continue
        cands = [app["bin"].lower(), app["name"].lower(), app["name"].lower().replace("++", "")]
        hit = next((names[c] for c in cands if c in names), None)
        if hit:
            recipes[app["name"]] = {"install": f"brew install {hit}", "method": "brew", "verified": True}
            added += 1

    save_recipes(before, recipes)
    verified = sum(1 for r in recipes.values() if r.get("verified"))
    print(f"brew-verified added: {added}; total verified: {verified}/{len(catalog)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
