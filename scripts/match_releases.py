#!/usr/bin/env python3
"""Find which catalog apps publish prebuilt binaries on GitHub (WMCAT-006) and
write lists/releases.json; then merge the catalog again.

Usage:
    python3 scripts/match_releases.py            # query (cached), write, merge
    python3 scripts/match_releases.py --offline  # cache only, no requests

For every app whose homepage is a GitHub repository root, asks the GitHub API
(through `gh`, so it is authenticated) for the latest release; a repository
whose latest release has assets is listed. The merge then sets the app's
`releases` flag, and the Store links its releases page. Responses are cached
in target/release-cache/ (not committed); delete it to look again.
"""
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))
import merge_catalog as mc  # noqa: E402

CACHE = ROOT / "target" / "release-cache"
OUT = ROOT / "lists" / "releases.json"


def latest_assets(repo, offline):
    f = CACHE / (re.sub(r"[^A-Za-z0-9._-]", "_", repo) + ".json")
    if f.exists():
        return json.loads(f.read_text())
    if offline:
        return None
    r = subprocess.run(["gh", "api", f"repos/{repo}/releases/latest", "--jq", "[.assets[].name]"],
                       capture_output=True, text=True, timeout=60)
    names = json.loads(r.stdout) if r.returncode == 0 and r.stdout.strip() else []
    CACHE.mkdir(parents=True, exist_ok=True)
    f.write_text(json.dumps(names))
    return names


def main() -> int:
    offline = "--offline" in sys.argv[1:]
    apps = json.loads(mc.OUT.read_text(encoding="utf-8"))["apps"]
    repos = sorted({r for r in (mc.gh_repo(a["homepage"]) for a in apps) if r})
    have = []
    for i, repo in enumerate(repos, 1):
        assets = latest_assets(repo, offline)
        if assets:
            have.append(repo)
        if i % 100 == 0:
            print(f"  {i}/{len(repos)} checked, {len(have)} with release assets", flush=True)
    OUT.write_text(json.dumps({"repos": have}, indent=1) + "\n", encoding="utf-8")
    print(f"wrote {len(have)} of {len(repos)} repositories -> {OUT.relative_to(ROOT)}")
    mc.OUT.write_text(mc.merge(), encoding="utf-8")
    print(f"wrote {mc.OUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
