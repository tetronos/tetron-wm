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
    """CON-WD04: desktop and extras are metapackages on the next size down."""
    problems = []
    for file, base in (("packaging/desktop.toml", "tetron-wm"), ("packaging/extras.toml", "tetron-wm-desktop")):
        p = Path(file)
        if not p.exists():
            problems.append(f"{p} missing")
            continue
        d = tomllib.loads(p.read_text())
        problems += [f"{p}: has a binary file entry" for f in d.get("files", []) if f.get("src") in BUILT]
        if not any(dep.split()[0] == base for dep in d.get("depends", [])):
            problems.append(f"{p}: depends does not name {base}")
    return {"ok": not problems, "problems": problems}


def check_suite_apps() -> dict:
    """CON-WD05: assets/suite-apps.txt is what the generator makes, and its tests pass."""
    import subprocess
    gen = subprocess.run([sys.executable, "scripts/gen_suite_lists.py", "--check"], capture_output=True, text=True)
    # The bundled tetron-wm-startpage helper's tests ride along with the suite list's.
    tests = subprocess.run([sys.executable, "-m", "unittest", "scripts/test_gen_suite_lists.py",
                            "scripts/test_tetron_wm_startpage.py"],
                           capture_output=True, text=True)
    return {"current": gen.returncode == 0 and tests.returncode == 0,
            "generator": gen.stderr.strip(), "tests": tests.stderr.strip().splitlines()[-1:]}


def check_catalog() -> dict:
    """CON-WD07: assets/catalog.json is what the merge makes from lists/, and its tests pass."""
    import subprocess
    gen = subprocess.run([sys.executable, "scripts/merge_catalog.py", "--check"], capture_output=True, text=True)
    tests = subprocess.run([sys.executable, "-m", "unittest", "scripts/test_merge_catalog.py"], capture_output=True, text=True)
    return {"current": gen.returncode == 0 and tests.returncode == 0,
            "merge": gen.stderr.strip(), "tests": tests.stderr.strip().splitlines()[-1:]}


# CON-WD06. Files where these words are expected: the changelog (dates), lock
# files and generated data, license text, .gitignore and the agent guide.
NOTES_ALLOWED = {"CHANGELOG.md", "Cargo.lock", "uv.lock", "assets/catalog.json", "lists/awesome-tuis.json", "lists/tetron.toml",
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


def check_example_config() -> dict:
    hits = [str(p) for p in Path("src").rglob("*.rs") if 'include_str!("../config.example.toml")' in p.read_text()]
    return {"compiled_in": bool(hits), "files": hits}


if __name__ == "__main__":
    ctx = {"packaging": check_packaging(), "lean_deb": check_lean_deb(), "desktop_deb": check_desktop_deb(), "suite_apps": check_suite_apps(), "catalog": check_catalog(), "planning_notes": check_planning_notes(), "parity": check_parity(), "example_config": check_example_config()}
    print(json.dumps(ctx, indent=2))
    sys.exit(0 if ctx["packaging"]["valid"] and ctx["lean_deb"]["ok"] and ctx["desktop_deb"]["ok"] and ctx["suite_apps"]["current"] and ctx["catalog"]["current"] and ctx["planning_notes"]["count"] == 0 and ctx["parity"]["ok"] and ctx["example_config"]["compiled_in"] else 1)
