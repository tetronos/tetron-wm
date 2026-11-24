#!/usr/bin/env python3
# reconcile.py -- run from the tetron-wm repo root.
# Checks the automatable packaging constraints (CON-WD*) in spec/constraints.py.
# Requirements (WMDEB-*) are verified by cargo test, review and the VM passes.
import json
import re
import sys
import tomllib
from pathlib import Path

# mkdeb's word for the binary passed to it.
BUILT = {"binary", "asset"}


def check_packaging() -> dict:
    problems = []
    for p in sorted(Path("packaging").glob("*.toml")):
        try:
            d = tomllib.loads(p.read_text())
        except tomllib.TOMLDecodeError as e:
            problems.append(f"{p}: {e}")
            continue
        problems += [f"{p}: has {k}" for k in ("scripts", "maintainer") if k in d]
        for f in d.get("files", []):
            src, dst = f.get("src", ""), f.get("dst", "")
            # mkdeb reads paths relative to the toml file.
            if src not in BUILT and not (p.parent / src).is_file():
                problems.append(f"{p}: src {src} missing")
            if not dst.startswith("usr/"):
                problems.append(f"{p}: dst {dst} not under usr/")
    return {"valid": not problems, "problems": problems}


def check_lean_deb() -> dict:
    p = Path("packaging/deb.toml")
    if not p.exists():
        return {"ok": False, "problems": ["packaging/deb.toml missing"]}
    d = tomllib.loads(p.read_text())
    problems = [f"{p}: declares {k}" for k in ("depends", "recommends") if d.get(k)]
    return {"ok": not problems, "problems": problems}


def check_desktop_deb() -> dict:
    """CON-WD04: tetron-wm-desktop is a metapackage on tetron-wm, and the only one here."""
    problems = []
    p = Path("packaging/desktop.toml")
    d = tomllib.loads(p.read_text()) if p.exists() else {}
    if not d:
        problems.append(f"{p} missing")
    problems += [f"{p}: has a binary file entry" for f in d.get("files", []) if f.get("src") in BUILT]
    if d and not any(dep.split()[0] == "tetron-wm" for dep in d.get("depends", [])):
        problems.append(f"{p}: depends does not name tetron-wm")
    problems += [f"packaging/{n}.toml: built by tetron-appstore, not here" for n in ("essentials", "recommended", "extras")
                 if Path(f"packaging/{n}.toml").exists()]
    return {"ok": not problems, "problems": problems}


def catalog_file() -> Path:
    """assets/catalog.json of the tetron-catalog library tetron-wm is built
    with (a git dependency on tetron-appstore, pinned by Cargo.lock);
    `TETRON_CATALOG_JSON` overrides it."""
    import os
    import subprocess
    if os.environ.get("TETRON_CATALOG_JSON"):
        return Path(os.environ["TETRON_CATALOG_JSON"])
    meta = json.loads(subprocess.run(["cargo", "metadata", "--format-version", "1"],
                                     capture_output=True, text=True, check=True).stdout)
    crate = next(Path(p["manifest_path"]) for p in meta["packages"] if p["name"] == "tetron-catalog")
    return crate.parent.parent.parent / "assets" / "catalog.json"


def check_suite_apps() -> dict:
    """CON-WD05: packaging/desktop.toml recommends what the catalog's desktop collection recommends."""
    import subprocess
    path = catalog_file()
    cols = json.loads(path.read_text(encoding="utf-8")).get("collections", [])
    want = next((c.get("recommends", []) for c in cols if c.get("name") == "desktop"), None)
    have = tomllib.loads(Path("packaging/desktop.toml").read_text()).get("recommends", [])
    # The bundled tetron-wm-startpage helper's tests ride along.
    tests = subprocess.run([sys.executable, "-m", "unittest", "scripts/test_tetron_wm_startpage.py"],
                           capture_output=True, text=True)
    return {"current": want == have and tests.returncode == 0, "catalog": str(path),
            "missing": [r for r in (want or []) if r not in have], "extra": [r for r in have if r not in (want or [])],
            "fix": "" if want == have else "set the recommends of packaging/desktop.toml to the desktop collection's (tetron-appstore)",
            "tests": tests.stderr.strip().splitlines()[-1:]}


# CON-WD06. Files where these words are expected: the changelog (dates), lock
# files and generated data, license text, .gitignore and the agent guide.
NOTES_ALLOWED = {"CHANGELOG.md", "Cargo.lock", "uv.lock",
                 "THIRD-PARTY-LICENSES.md", ".gitignore", "CLAUDE.md"}
# Built from parts so this file does not match itself.
OWNER = re.compile(r"(?<![$\w\"{])" + "US" + r"ER(?![\w])")
PRIVATE_DIR = "DO-NOT-" + "COMMIT"
DATE = r"\b20\d\d-[01]\d-[0-3]\d\b"
DATE_IN_COMMENT = re.compile(r"(#|//|/\*|<!--).*" + DATE)
DATE_ANY = re.compile(DATE)


def check_planning_notes() -> dict:
    """CON-WD06: no owner placeholder, private paths or comment dates in committed files."""
    import subprocess
    hits = []
    files = subprocess.run(["git", "ls-files"], capture_output=True, text=True).stdout.split()
    for f in files:
        if f in NOTES_ALLOWED or not Path(f).is_file():
            continue
        try:
            lines = Path(f).read_text().splitlines()
        except UnicodeDecodeError:
            continue
        for n, line in enumerate(lines, 1):
            if (OWNER.search(line) and "autologin " + "US" + "ER" not in line) or PRIVATE_DIR in line \
                    or DATE_IN_COMMENT.search(line) or (f.startswith("spec/") and DATE_ANY.search(line)):
                hits.append(f"{f}:{n}: {line.strip()[:100]}")
    return {"count": len(hits), "hits": hits[:20]}


def check_parity() -> dict:
    """CON-WD08: every suite script step has a parity row, every row says each path."""
    import re
    problems = []
    steps = set(re.findall(r"^# ── Step ([0-9a-z]+):", Path("install-tetron-wm-suite.sh").read_text(), re.M))
    rows = tomllib.loads(Path("packaging/parity.toml").read_text()).get("row", [])
    shipped = {Path(f["dst"]).name for f in tomllib.loads(Path("packaging/desktop.toml").read_text()).get("files", [])}
    for i, r in enumerate(rows):
        where = f"packaging/parity.toml row {i + 1} (step {r.get('step')})"
        if r.get("step") not in steps:
            problems.append(f"{where}: no such step in install-tetron-wm-suite.sh")
        for k in ("what", "deb", "iso", "inapp"):
            v = str(r.get(k, "")).strip()
            if not v or v == "none" or (v.startswith("none") and not v.startswith("none: ")) or v == "none:":
                problems.append(f"{where}: {k} must name the path or say \"none: <why>\"")
        problems += [f"{where}: unit {u} is not in packaging/desktop.toml files" for u in r.get("iso_units", []) if u not in shipped]
    problems += [f"install-tetron-wm-suite.sh step {s} has no row in packaging/parity.toml" for s in sorted(steps - {r.get("step") for r in rows})]
    return {"ok": not problems and bool(steps), "steps": sorted(steps), "problems": problems}


def check_suite_deps() -> dict:
    """CON-WD09: git dependencies on tetronos repos name a tag; the tetron-proto
    tag has tetron-wm's minor. WMDEP-004: the build without the mesh feature
    still compiles."""
    import re
    import subprocess
    problems = []
    text = Path("Cargo.toml").read_text()
    own = tomllib.loads(text)["package"]["version"]
    minor = lambda v: ".".join(v.lstrip("v").split(".")[:2])
    for line in text.splitlines():
        m = re.match(r'^([A-Za-z0-9_-]+) = \{.*git = "https://github\.com/tetronos/[^"]+"', line)
        if not m:
            continue
        tag = re.search(r'tag = "([^"]+)"', line)
        if not tag:
            problems.append(f"{m.group(1)}: git dependency without a tag")
        elif m.group(1) == "tetron-proto" and minor(tag.group(1)) != minor(own):
            problems.append(f"tetron-proto tag {tag.group(1)} is not tetron-wm's minor {minor(own)}")
    r = subprocess.run(["cargo", "check", "--quiet", "--locked", "--no-default-features"], capture_output=True, text=True)
    if r.returncode != 0:
        problems.append("cargo check --no-default-features failed: " + r.stderr.strip()[-300:])
    return {"ok": not problems, "problems": problems}


# CON-WD10 (WMNIX-008): the fixed /usr, /sbin and dpkg paths non-test code may
# use, as (file, literal). Everything else finds programs on PATH and data in
# the XDG data dirs through src/paths.rs.
FIXED_PATHS_ALLOWED = {
    ("src/cli_help.rs", "install.sh/updater install dir (default /usr/local/bin)"),  # help text
    ("src/packaging.rs", "/var/lib/dpkg/info"),  # dpkg support (WMDEB-005, WMINST-005)
    ("src/packaging.rs", "/var/lib/dpkg/diversions"),  # dpkg support (WMINST-013)
    ("src/packaging.rs", "/usr/bin/apt-get"),  # apt host check (WMINST-011)
    ("src/settings.rs", "/usr/bin/apt-get"),  # apt host check (WMINST-011)
    ("src/packaging.rs", "/usr/sbin"),  # app check: not on a Debian user's PATH
    ("src/packaging.rs", "/sbin"),  # app check: not on a Debian user's PATH
    ("src/paths.rs", "/usr/local/share:/usr/share"),  # XDG default (WMDEB-002)
    ("src/paths.rs", "/usr/lib/kbd/consolefonts"),  # fallback (WMNIX-005)
    ("src/service.rs", "/usr/lib/systemd/user"),  # packaged unit dir (WMNIX-006)
    ("src/session.rs", "/usr/local/bin"),  # updater fallback dir (WMDEB-005)
}
FIXED_PATH = re.compile(r'(^|[^\w.])/(usr|sbin|var/lib/dpkg)(/|$)')
STRING_LIT = re.compile(r'"((?:[^"\\]|\\.)*)"')


def check_fixed_paths() -> dict:
    """CON-WD10: no fixed /usr, /sbin or dpkg path in non-test code outside the allowlist."""
    hits = []
    for p in sorted(Path("src").rglob("*.rs")):
        for n, line in enumerate(p.read_text().splitlines(), 1):
            if line.startswith("#[cfg(test)]"):
                break
            if line.strip().startswith("//"):
                continue
            for m in STRING_LIT.finditer(line):
                if FIXED_PATH.search(m.group(1)) and (str(p), m.group(1)) not in FIXED_PATHS_ALLOWED:
                    hits.append(f"{p}:{n}: {m.group(1)[:100]}")
    return {"count": len(hits), "hits": hits[:20]}


def check_example_config() -> dict:
    hits = [str(p) for p in Path("src").rglob("*.rs") if 'include_str!("../config.example.toml")' in p.read_text()]
    return {"compiled_in": bool(hits), "files": hits}


if __name__ == "__main__":
    ctx = {"packaging": check_packaging(), "lean_deb": check_lean_deb(), "desktop_deb": check_desktop_deb(), "suite_apps": check_suite_apps(), "planning_notes": check_planning_notes(), "parity": check_parity(), "example_config": check_example_config(), "suite_deps": check_suite_deps(), "fixed_paths": check_fixed_paths()}
    print(json.dumps(ctx, indent=2))
    sys.exit(0 if ctx["packaging"]["valid"] and ctx["lean_deb"]["ok"] and ctx["desktop_deb"]["ok"] and ctx["suite_apps"]["current"] and ctx["planning_notes"]["count"] == 0 and ctx["parity"]["ok"] and ctx["example_config"]["compiled_in"] and ctx["suite_deps"]["ok"] and ctx["fixed_paths"]["count"] == 0 else 1)
