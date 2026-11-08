#!/usr/bin/env python3
"""Find each catalog app's package name per package manager (WMCAT-005) on
Repology (https://repology.org, read-only API, one request per second as its
terms ask) and write lists/repology.json; then merge the catalog again.

Usage:
    python3 scripts/match_repology.py            # query (cached), write, merge
    python3 scripts/match_repology.py --offline  # cache only, no requests

A project is tried under the app's command name, its name and its repository
name. Picked per manager: apt from debian_13, else ubuntu_24_04, else debian_12;
dnf from the newest Fedora release; pacman from Arch (not the AUR); brew from
Homebrew formulae. A name is kept only when that package's own homepage is the
app's (same repository, or a project site named after it): Repology matches by
project name, and a different program with the same name must never be
installed. Homepages come from apt metadata (debian:trixie, ubuntu:24.04,
debian:12 containers, needs docker), Fedora mdapi, the Arch package API and
Homebrew's formula index. Rejected names are listed for a look; a `pkg` table in
lists/tetron.toml overrides any name ("" removes it). Responses are cached in
target/repology-cache/ (not committed). Stdlib only.
"""
import json
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "lists" / "repology.json"
CACHE = ROOT / "target" / "repology-cache"
API = "https://repology.org/api/v1/project/"
AGENT = "tetron-wm-catalog/1 (+https://github.com/tetronos/tetron-wm)"
APT_REPOS = ("debian_13", "ubuntu_24_04", "debian_12")
SKIP_SUFFIX = ("-dbg", "-dbgsym", "-debuginfo", "-debugsource", "-doc", "-dev", "-devel")
WORD = re.compile(r"[a-z]{4,}")


def candidates(app):
    """Repology project names to try for an app, most specific first."""
    repo = app["homepage"].rstrip("/").rsplit("/", 1)[-1].lower()
    out = []
    for n in (app["bin"], app["name"].lower().replace(" ", "-"), repo):
        n = n.lower()
        if n and re.fullmatch(r"[a-z0-9][a-z0-9._+-]*", n) and n not in out:
            out.append(n)
    return out


def binary(pkg, want):
    """The installable package name of one Repology package entry."""
    names = pkg.get("binnames") or ([pkg["binname"]] if pkg.get("binname") else [pkg.get("srcname", "")])
    names = [n for n in names if n and not n.endswith(SKIP_SUFFIX)]
    return want if want in names else (names[0] if names else "")


def pick(packages, want):
    """{manager: name} from one project's package list."""
    by = {}
    for p in packages:
        if not (p.get("binname") or p.get("srcname") or "").startswith("lib32-"):
            by.setdefault(p["repo"], []).append(p)
    for r in by:  # the entry that installs the command itself first
        by[r].sort(key=lambda p: binary(p, want) != want)
    out = {}
    for r in APT_REPOS:
        if r in by:
            out["apt"] = binary(by[r][0], want)
            break
    fedora = sorted((int(r.split("_")[1]), r) for r in by if re.fullmatch(r"fedora_\d+", r))
    if fedora:
        out["dnf"] = binary(by[fedora[-1][1]][0], want)
    if "arch" in by:
        out["pacman"] = binary(by["arch"][0], want)
    if "homebrew" in by:
        out["brew"] = by["homebrew"][0].get("srcname", "")
    return {k: v for k, v in out.items() if v}


def summaries(packages):
    return " ".join(p.get("summary", "") for p in packages).lower()


def fetch(name, offline):
    """A project's package list (cached); [] when Repology has none, None
    when it could not be reached."""
    f = CACHE / f"{name}.json"
    if f.exists():
        return json.loads(f.read_text(encoding="utf-8"))
    if offline:
        return []
    req = urllib.request.Request(API + urllib.parse.quote(name), headers={"User-Agent": AGENT})
    for attempt in range(5):
        try:
            with urllib.request.urlopen(req, timeout=30) as r:
                data = json.load(r)
            break
        except (urllib.error.URLError, OSError, ValueError) as e:
            if isinstance(e, urllib.error.HTTPError) and e.code not in (429, 500, 502, 503, 504):
                raise
            if attempt == 4:
                return None  # not checked this run; not cached, so the next run asks again
            time.sleep(10 * (attempt + 1))
    time.sleep(1.1)
    CACHE.mkdir(parents=True, exist_ok=True)
    f.write_text(json.dumps(data), encoding="utf-8")
    return data


def match(apps, offline=False, get=fetch):
    names, review, unchecked = {}, [], []
    for a in apps:
        for c in candidates(a):
            pk = get(c, offline)
            if pk is None:
                unchecked.append(a["name"])
                break
            found = pick(pk, a["bin"])
            if found:
                names[a["name"]] = found
                text = summaries(pk)
                if text and not set(WORD.findall(a["description"].lower())) & set(WORD.findall(text)):
                    review.append(f"{a['name']} -> {c}: {text[:80]}")
                break
    return names, review, unchecked


FORGES = ("github.com", "gitlab.com", "codeberg.org", "git.sr.ht", "sr.ht", "bitbucket.org")
# Package registries: a page there names the package, not the project, so it
# counts when that name is the app's repository name.
REGISTRIES = ("crates.io", "lib.rs", "hackage.haskell.org", "pypi.python.org", "pypi.org")
GO_DOCS = ("godoc.org", "pkg.go.dev")


def norm(url):
    """(host, path parts) of a homepage, lowercase, without scheme, www and .git."""
    u = urllib.parse.urlparse(url.strip().lower() if "://" in url else "https://" + url.strip().lower())
    host = u.netloc.removeprefix("www.")
    parts = [x for x in u.path.removesuffix(".git").split("/") if x]
    return host, parts


def same_project(app_home, pkg_home):
    """Whether a package homepage is the app's: the same forge repository, one
    URL a prefix of the other, or a project site named after the repository or
    its owner (htop.dev for github.com/htop-dev/htop)."""
    if not app_home or not pkg_home:
        return False
    (ah, ap), (ph, pp) = norm(app_home), norm(pkg_home)
    if ph in GO_DOCS and pp:
        return same_project(app_home, "https://" + "/".join(pp))
    if ph in REGISTRIES:
        return ah in FORGES and len(ap) >= 2 and bool(pp) and pp[-1] == ap[1]
    if ah in FORGES and ph in FORGES:
        return ah == ph and ap[:2] == pp[:2] and len(ap) >= 2
    if ah == ph and (ap[:len(pp)] == pp or pp[:len(ap)] == ap):
        return True
    names = {x for x in ap[:2]} if ah in FORGES else {ah.split(".")[0]}
    label = ph.split(".")[0] if ph not in FORGES else (pp[1] if len(pp) > 1 else "")
    return bool(label) and label in names


def cached_json(sub, key, url):
    f = CACHE / sub / f"{key}.json"
    if f.exists():
        return json.loads(f.read_text(encoding="utf-8"))
    req = urllib.request.Request(url, headers={"User-Agent": AGENT})
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            data = json.load(r)
    except urllib.error.HTTPError as e:
        if e.code not in (400, 404):  # 400: mdapi for a name it does not know
            raise
        data = {}
    f.parent.mkdir(parents=True, exist_ok=True)
    f.write_text(json.dumps(data), encoding="utf-8")
    time.sleep(0.5)
    return data


def apt_homepages(names):
    """{package: homepage} from apt metadata of debian:trixie, ubuntu:24.04 and
    debian:12 (the first that has the package wins); cached per image."""
    import subprocess
    out = {}
    for image in ("debian:trixie", "ubuntu:24.04", "debian:12"):
        f = CACHE / "apt" / (image.replace(":", "-") + ".json")
        have = json.loads(f.read_text(encoding="utf-8")) if f.exists() else {}
        want = sorted(n for n in names if n not in have)
        if want:
            script = ("apt-get update -qq >/dev/null && for p in " + " ".join(want) +
                      "; do h=$(apt-cache show --no-all-versions $p 2>/dev/null | sed -n 's/^Homepage: //p' | head -1); echo \"$p $h\"; done")
            r = subprocess.run(["docker", "run", "--rm", image, "sh", "-c", script], capture_output=True, text=True, check=True)
            for line in r.stdout.splitlines():
                p, _, h = line.partition(" ")
                have[p] = h.strip()
            f.parent.mkdir(parents=True, exist_ok=True)
            f.write_text(json.dumps(have, sort_keys=True), encoding="utf-8")
        for n in names:
            if have.get(n) and n not in out:
                out[n] = have[n]
    return out


def homepage(manager, name, apt, brew):
    if manager == "apt":
        return apt.get(name, "")
    if manager == "brew":
        return brew.get(name, "")
    if manager == "dnf":
        return cached_json("fedora", name, f"https://mdapi.fedoraproject.org/f43/pkg/{urllib.parse.quote(name)}").get("url", "")
    res = cached_json("arch", name, f"https://archlinux.org/packages/search/json/?name={urllib.parse.quote(name)}").get("results", [])
    return res[0].get("url", "") if res else ""


def verify(apps, names, apt=None, brew=None, home=homepage):
    """Keep only names whose package homepage is the app's; returns (kept,
    rejected lines)."""
    by = {a["name"]: a for a in apps}
    if apt is None:
        apt = apt_homepages({v["apt"] for v in names.values() if "apt" in v})
    if brew is None:
        f = CACHE / "brew-formula.json"
        if not f.exists():
            req = urllib.request.Request("https://formulae.brew.sh/api/formula.json", headers={"User-Agent": AGENT})
            with urllib.request.urlopen(req, timeout=120) as r:
                f.write_bytes(r.read())
        brew = {x["name"]: x.get("homepage", "") for x in json.loads(f.read_text(encoding="utf-8"))}
    kept, rejected = {}, []
    for app, found in names.items():
        ok = {}
        for m, n in found.items():
            h = home(m, n, apt, brew)
            if same_project(by[app]["homepage"], h):
                ok[m] = n
            else:
                rejected.append(f"{app} ({by[app]['homepage']}): {m} {n} is {h or 'no homepage'}")
        if ok:
            kept[app] = ok
    return kept, rejected


def main():
    sys.path.insert(0, str(ROOT / "scripts"))
    import merge_catalog  # noqa: E402
    offline = "--offline" in sys.argv[1:]
    apps = json.loads(merge_catalog.OUT.read_text(encoding="utf-8"))["apps"]
    names, review, unchecked = match(apps, offline)
    if unchecked:
        print(f"not checked (Repology unreachable), run again: {', '.join(unchecked)}")
        return 1
    found = len(names)
    names, rejected = verify(apps, names)
    doc = {"source": "https://repology.org", "names": dict(sorted(names.items()))}
    OUT.write_text(json.dumps(doc, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    merge_catalog.OUT.write_text(merge_catalog.merge(), encoding="utf-8")
    count = {m: sum(1 for v in names.values() if m in v) for m in ("apt", "dnf", "pacman", "brew")}
    print(f"{len(names)}/{len(apps)} apps have a package (Repology matched {found}); {count}")
    if rejected:
        print(f"rejected {len(rejected)} names (package homepage is not the app's; add a pkg table if one is right):")
        print("\n".join("  " + r for r in rejected))
    if review:
        print(f"Repology summaries sharing no word with the description ({len(review)}):")
        print("\n".join("  " + r for r in review))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
