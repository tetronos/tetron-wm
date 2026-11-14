#!/usr/bin/env python3
"""Derive upstream install recipes by language for catalog apps that have no
package names and no verified recipe (recipe blocks appended to
lists/tetron.toml, then merged into assets/catalog.json).

Usage:
    python3 scripts/match_lang_recipes.py            # query (cached), write, merge
    python3 scripts/match_lang_recipes.py --offline  # cache only, no requests

Only apps whose homepage is a GitHub repository root. The repository's main
language (GitHub API through `gh`) picks the way:
  Rust:   a crates.io crate whose repository is the app's: cargo install --locked <crate>
  Python: a PyPI project whose URLs name the app's repository: pipx install <project>
  Go:     go.mod's module, installed at the root when it has main.go, else at
          cmd/<name> when that is the only command or is named after the app;
          skipped when go.mod has a replace directive (go install @latest refuses
          it) or a module path without a domain (go install can not fetch it)
A name is tried as the app's command, its name and its repository name. A
different project with the same name is never used: the repository must match.
The Store offers the language toolchain first when it is missing. Responses
are cached in target/lang-cache/ (not committed). Stdlib plus the gh CLI.
"""
import base64
import json
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))
from merge_catalog import catalog_recipes, gh_repo, save_recipes  # noqa: E402

CACHE = ROOT / "target" / "lang-cache"
OFFLINE = "--offline" in sys.argv[1:]
UA = {"User-Agent": "tetron-wm-catalog (https://github.com/tetronos/tetron-wm)"}


def cached(key, fetch):
    f = CACHE / (re.sub(r"[^A-Za-z0-9._-]", "_", key) + ".json")
    if f.exists():
        return json.loads(f.read_text())
    if OFFLINE:
        return None
    v = fetch()
    CACHE.mkdir(parents=True, exist_ok=True)
    f.write_text(json.dumps(v))
    return v


def http_json(url):
    time.sleep(1)  # crates.io and PyPI ask for at most one request per second
    try:
        with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=30) as r:
            return json.load(r)
    except urllib.error.HTTPError as e:
        if e.code == 404:
            return None
        raise


def gh(path):
    r = subprocess.run(["gh", "api", path], capture_output=True, text=True, timeout=60)
    return json.loads(r.stdout) if r.returncode == 0 else None


def mentions(urls, repo):
    return any(repo in (u or "").lower().replace("www.", "") for u in urls)


def rust(names, repo):
    for n in names:
        d = cached(f"crate-{n}", lambda: http_json(f"https://crates.io/api/v1/crates/{n}"))
        if d and mentions([d["crate"].get("repository"), d["crate"].get("homepage")], f"github.com/{repo}"):
            return "cargo", f"cargo install --locked {d['crate']['name']}"
    return None


def python(names, repo):
    for n in names:
        d = cached(f"pypi-{n}", lambda: http_json(f"https://pypi.org/pypi/{n}/json"))
        if d:
            info = d["info"]
            urls = [info.get("home_page"), info.get("download_url"), *(info.get("project_urls") or {}).values()]
            if mentions(urls, f"github.com/{repo}"):
                return "pipx", f"pipx install {info['name']}"
    return None


def go(names, repo):
    root = cached(f"gh-root-{repo}", lambda: gh(f"repos/{repo}/contents/"))
    if not isinstance(root, list) or not any(f["name"] == "go.mod" for f in root):
        return None
    mod = cached(f"gh-gomod-{repo}", lambda: gh(f"repos/{repo}/contents/go.mod"))
    if not mod:
        return None
    text = base64.b64decode(mod["content"]).decode("utf-8", "replace")
    m = re.search(r"^module\s+(\S+)", text, re.M)
    # A module without a domain (`module tool`) can not be fetched by go install.
    if not m or "." not in m.group(1).split("/")[0] or re.search(r"^\s*replace\b|^replace\s*\(", text, re.M):
        return None
    module = m.group(1)
    if any(f["name"] == "main.go" for f in root):
        return "go", f"go install {module}@latest"
    if any(f["name"] == "cmd" and f["type"] == "dir" for f in root):
        cmd = cached(f"gh-cmd-{repo}", lambda: gh(f"repos/{repo}/contents/cmd"))
        dirs = [f["name"] for f in cmd or [] if f["type"] == "dir"]
        pick = next((d for d in dirs if d.lower() in names), dirs[0] if len(dirs) == 1 else None)
        if pick:
            return "go", f"go install {module}/cmd/{pick}@latest"
    return None


WAYS = {"Rust": rust, "Python": python, "Go": go}


def main() -> int:
    catalog = json.loads((ROOT / "assets" / "catalog.json").read_text())["apps"]
    recipes = catalog_recipes()
    before = dict(recipes)
    tried = found = 0
    for app in catalog:
        repo = gh_repo(app["homepage"])
        if not repo or app.get("pkg") or recipes.get(app["name"], {}).get("verified"):
            continue
        meta = cached(f"gh-repo-{repo}", lambda: gh(f"repos/{repo}"))
        way = WAYS.get((meta or {}).get("language"))
        if not way or (meta or {}).get("archived"):
            continue
        tried += 1
        names = list(dict.fromkeys(n.lower() for n in (app["bin"], app["name"], repo.split("/")[1])))
        hit = way(names, repo)
        if hit:
            recipes[app["name"]] = {"install": hit[1], "method": hit[0], "verified": True}
            found += 1
            print(f"  {app['name']}: {hit[1]}")
    n = save_recipes(before, recipes)
    print(f"language recipes: {found} of {tried} tried; {n} written")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
