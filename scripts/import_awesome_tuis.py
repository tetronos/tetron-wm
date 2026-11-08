#!/usr/bin/env python3
"""Import the awesome-tuis README into lists/awesome-tuis.json (WMCAT-001).

Usage:
    curl -fsSL https://raw.githubusercontent.com/rothgar/awesome-tuis/master/README.md -o README-awesome-tuis.md
    python3 scripts/import_awesome_tuis.py README-awesome-tuis.md
    python3 scripts/merge_catalog.py

Writes only lists/awesome-tuis.json: every app's name, category,
description, homepage and a best-effort command name. Our corrections
(command names, CLI flags, recipes, dropped entries) live in
lists/tetron.toml, which the merge applies, so this import never overwrites
them. Never edit lists/awesome-tuis.json by hand.
"""
import json
import re
import sys
from pathlib import Path

# "Libraries" are TUI frameworks, not runnable apps.
EXCLUDE_CATEGORIES = {"Libraries"}


def guess_bin(name: str) -> str:
    b = name.lower().replace("++", "").strip()
    return re.sub(r"[^a-z0-9._+-]", "", b)


def parse(text: str) -> list:
    category, seen, apps = None, set(), []
    for line in text.splitlines():
        h = re.search(r"<summary><h2>(.*?)</h2>", line)
        if h:
            category = re.sub("<[^>]*>", "", h.group(1)).strip()
            continue
        m = re.match(r"^- \[([^\]]+)\]\(([^)]+)\)\s*(.*)$", line)
        if not m or not category or category in EXCLUDE_CATEGORIES:
            continue
        name = m.group(1).strip()
        if name.lower() in seen:
            continue
        seen.add(name.lower())
        apps.append({"name": name, "bin": guess_bin(name), "category": category,
                     "description": m.group(3).strip(), "homepage": m.group(2).strip()})
    apps.sort(key=lambda a: (a["category"].lower(), a["name"].lower()))
    return apps


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    apps = parse(Path(sys.argv[1]).read_text(encoding="utf-8"))
    out = Path(__file__).resolve().parent.parent / "lists" / "awesome-tuis.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(apps, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print(f"wrote {len(apps)} apps across {len({a['category'] for a in apps})} categories -> {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
