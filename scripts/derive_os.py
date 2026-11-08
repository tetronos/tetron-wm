#!/usr/bin/env python3
"""Fill in the `os` field for recipes that don't already have one (agent-derived
`platforms` are written by merge_recipes.py and are left untouched). Changed
recipes are appended to lists/tetron.toml and the catalog is merged again.

Safe by construction — it never excludes a platform without a hard signal:
  - brew recipes      -> ["macos", "linux"]  (Homebrew is cross-platform)
  - distro-only method -> ["linux"]           (apt/pacman/... only exist on Linux)
  - anything else      -> unset                (shown on every OS)
"""
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))
from merge_catalog import catalog_recipes, save_recipes  # noqa: E402
LINUX_METHODS = {"apt", "pacman", "dnf", "yum", "apk", "snap", "yay", "yaourt"}


def main() -> int:
    recipes = catalog_recipes()
    before = json.loads(json.dumps(recipes))

    filled = 0
    for r in recipes.values():
        if r.get("os"):
            continue  # keep agent-derived platforms
        method = r.get("method")
        if method in LINUX_METHODS:
            r["os"] = ["linux"]
            filled += 1
        elif method == "brew":
            r["os"] = ["macos", "linux"]
        else:
            r.pop("os", None)  # unset -> shown everywhere

    save_recipes(before, recipes)
    linux = sum(1 for r in recipes.values() if r.get("os") == ["linux"])
    print(f"linux-only (distro method): {linux}; brew apps: cross-platform")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
