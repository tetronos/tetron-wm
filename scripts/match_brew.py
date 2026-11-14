#!/usr/bin/env python3
"""Mark brew-installable catalog apps as verified (recipe blocks appended to
lists/tetron.toml, then merged into assets/catalog.json).

Fetches the Homebrew formula index and matches it against the catalog by binary
name / app name, writing a verified `brew install <formula>` recipe for each hit
whose formula points at the app's own GitHub repository (homepage, source or
head URL), so a different program with the same name is never installed. An
app whose homepage is not a GitHub repository root matches by name alone.
Existing verified recipes are left untouched. Run after scripts/import_awesome_tuis.py and scripts/merge_catalog.py.
"""
import json
import re
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))
from merge_catalog import gh_repo, catalog_recipes, save_recipes  # noqa: E402
FORMULA_API = "https://formulae.brew.sh/api/formula.json"


def url_repo(u):
    """owner/repo (lower case) of any github.com URL (archive, tarball, git), else None."""
    m = re.match(r"https?://(?:www\.)?github\.com/([^/?#]+)/([^/?#]+)", u or "", re.I)
    return f"{m.group(1)}/{m.group(2).removesuffix('.git')}".lower() if m else None


def main() -> int:
    with urllib.request.urlopen(FORMULA_API) as resp:
        formulae = json.load(resp)
    names, repos = {}, {}
    for f in formulae:
        for n in [f["name"], *f.get("aliases", [])]:
            names[n.lower()] = f["name"]
        urls = [f.get("homepage"), *((f.get("urls") or {}).get(k, {}).get("url") for k in ("stable", "head"))]
        repos[f["name"]] = {r for r in (url_repo(u) for u in urls) if r}

    catalog = json.loads((ROOT / "assets" / "catalog.json").read_text())["apps"]
    recipes = catalog_recipes()
    before = dict(recipes)

    added = 0
    for app in catalog:
        if recipes.get(app["name"], {}).get("verified"):
            continue
        cands = [app["bin"].lower(), app["name"].lower(), app["name"].lower().replace("++", "")]
        hit = next((names[c] for c in cands if c in names), None)
        own = gh_repo(app["homepage"])
        if hit and own and own not in repos[hit]:
            continue
        if hit:
            recipes[app["name"]] = {"install": f"brew install {hit}", "method": "brew", "verified": True}
            added += 1

    save_recipes(before, recipes)
    verified = sum(1 for r in recipes.values() if r.get("verified"))
    print(f"brew-verified added: {added}; total verified: {verified}/{len(catalog)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
