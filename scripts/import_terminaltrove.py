#!/usr/bin/env python3
"""Import the terminaltrove.com tool list into lists/terminaltrove.json (WMCAT-001).

Usage:
    curl -fsSL https://carcaradotdev.github.io/terminaltrove-stars/data.json -o terminaltrove.json
    python3 scripts/import_terminaltrove.py terminaltrove.json
    python3 scripts/merge_catalog.py

terminaltrove.com answers scripted fetches with a browser challenge, so the
input is the data.json of terminaltrove-stars, which rebuilds the whole site
list every two days. Keeps every tool that runs on Linux or macOS and is not
archived, whatever its age; a tool without the "tui" tag is cli = true. The
category comes from the first of its tags in CATEGORY, else Miscellaneous.
Writes only lists/terminaltrove.json; our corrections live in
lists/tetron.toml. Never edit lists/terminaltrove.json by hand.
"""
import json
import re
import sys
from pathlib import Path

# terminaltrove tag -> our category (the awesome-tuis section names), in
# priority order: the first tag of a tool found here wins.
CATEGORY = {
    "ai": "AI", "gpt": "AI",
    "text-editors": "Editors",
    "file-manager": "File Managers",
    "docker": "Docker/LXC/K8s", "kubernetes": "Docker/LXC/K8s", "virtualization": "Docker/LXC/K8s",
    "music": "Multimedia", "video": "Multimedia",
    "messaging": "Messaging", "signal": "Messaging", "social": "Messaging", "email": "Messaging", "irc": "Messaging",
    "browser": "Web", "web": "Web", "news": "Web", "newsreader": "Web",
    "monitoring": "Dashboards", "top": "Dashboards", "cpu": "Dashboards", "hardware": "Dashboards",
    "observability": "Dashboards", "logging": "Dashboards", "filesystems": "Dashboards",
    "productivity": "Productivity", "markdown": "Productivity", "financial": "Productivity",
    "otp": "Productivity", "math": "Productivity", "typing": "Productivity", "backup": "Productivity",
    "git": "Development", "devops": "Development", "debugging": "Development", "testing": "Development",
    "sql": "Development", "mongodb": "Development", "nosql": "Development", "api": "Development",
    "json": "Development", "jq": "Development", "regex": "Development", "diff": "Development",
    "ssh": "Development", "networking": "Development", "tcp": "Development", "dns": "Development",
    "security": "Development", "system-administration": "Development", "text-processing": "Development",
}


def guess_bin(slug: str) -> str:
    return re.sub(r"[^a-z0-9._+-]", "", slug.lower())


def tools(doc):
    return doc if isinstance(doc, list) else next(v for v in doc.values() if isinstance(v, list))


def parse(doc) -> list:
    apps, seen = [], set()
    for t in tools(doc):
        tags = t.get("categories") or []
        name, repo = t.get("name", "").strip(), (t.get("repo") or "").strip()
        if not name or not repo or t.get("archived") or name.lower() in seen:
            continue
        if not {"linux", "macos"} & set(tags + (t.get("os") or [])):
            continue
        seen.add(name.lower())
        a = {"name": name, "bin": guess_bin(t.get("slug") or name),
             "category": next((CATEGORY[c] for c in CATEGORY if c in tags), "Miscellaneous"),
             "description": (t.get("tagline") or t.get("description") or "").strip(),
             "homepage": f"https://github.com/{repo}"}
        if "tui" not in tags:
            a["cli"] = True
        apps.append(a)
    apps.sort(key=lambda a: (a["category"].lower(), a["name"].lower()))
    return apps


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    apps = parse(json.loads(Path(sys.argv[1]).read_text(encoding="utf-8")))
    out = Path(__file__).resolve().parent.parent / "lists" / "terminaltrove.json"
    out.write_text(json.dumps(apps, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print(f"wrote {len(apps)} apps ({sum('cli' in a for a in apps)} cli) -> {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
