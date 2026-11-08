"""Tests for gen_suite_lists.py (WMINST-001). Run: python3 -m unittest scripts/test_gen_suite_lists.py"""
import sys
import tomllib
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gen_suite_lists as g  # noqa: E402
import merge_catalog  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent
# The checks of the suite script's hand-written lists before WMINST-001.
FORMER = ["chafa", "uv", "mpv", "pdftoppm", "w3m", "fresh", "sshpass", "htop", "btop", "upower", "alsamixer",
          "font=DejaVu Sans Mono", "blueutil", "xclip", "wl-copy", "wpctl", "nmcli", "bluetoothctl",
          "tetron-wm-startpage", "tetron-aimode"]
# Former names that live on as a script entry.
SCRIPT_FOR = {"fresh": "fresh", "tetron-wm-startpage": "helpers", "tetron-aimode": "helpers"}
DEB = 'name = "x"\ndepends = []\nrecommends = []\nsuggests = ["svc"]\n'


def gen(packages: str = "", apps=(), scripts: str = ""):
    ours = tomllib.loads(packages + scripts)
    return g.generate(ours, list(apps), {s: DEB for s in g.SETS})


class Generated(unittest.TestCase):
    ours = tomllib.loads((ROOT / "lists" / "tetron.toml").read_text(encoding="utf-8"))
    debs = {s: (g.PKG / f"{s}.toml").read_text() for s in g.SETS}
    text, new = g.generate(ours, merge_catalog.merged_apps(), debs)
    rows = [l.split(":") for l in text.splitlines() if l and not l.startswith("#")]

    def test_every_recommends_is_listed_unless_deb_only(self):
        apt = {r[3] for r in self.rows if r[1] != "script"}
        deb_only = {p["name"] for p in self.ours["package"] if not p.get("check")}
        for s, text in self.new.items():
            for rel in tomllib.loads(text).get("recommends", []):
                n = g.debian_name(rel)
                self.assertTrue(n in apt or n in deb_only, f"{s}: {rel}")

    def test_nothing_from_the_former_lists_is_lost(self):
        checks = {c for r in self.rows if r[1] != "script" for c in r[2].split("|")}
        scripts = {r[2] for r in self.rows if r[1] == "script"}
        for c in FORMER:
            self.assertTrue(c in checks or SCRIPT_FOR.get(c) in scripts, f"former entry {c} is not generated")

    def test_line_shape(self):
        for r in self.rows:
            self.assertIn(r[0], ("essentials", "recommended", "extras"))
            self.assertEqual(len(r), 3 if r[1] == "script" else 7, r)
            if r[1] != "script":
                self.assertIn(r[1], ("apps", "gui", "services"))

    def test_the_desktop_tools_stay_in_tetron_wm_desktop(self):
        rec = [g.debian_name(r) for r in tomllib.loads(self.new["desktop"])["recommends"]]
        for n in ("mpv", "chafa", "fbi", "tetron-browser", "htop"):
            self.assertIn(n, rec)
        self.assertNotIn("git", rec)

    def test_git_moved_to_essentials(self):
        self.assertIn("git", tomllib.loads(self.new["essentials"])["recommends"])
        self.assertEqual(tomllib.loads(self.new["extras"])["recommends"], [])

    def test_a_store_app_in_a_set_takes_its_pkg_names(self):
        self.assertIn("essentials:apps:tmux:tmux:tmux:tmux:tmux", self.text)
        self.assertIn("lazygit", tomllib.loads(self.new["recommended"])["recommends"])


class Errors(unittest.TestCase):
    def test_defaults_to_the_debian_name(self):
        text, debs = gen('[[package]]\nname = "a"\nset = "essentials"\ncheck = "a"\n')
        self.assertIn("essentials:apps:a:a:a:a:a", text)
        self.assertEqual(tomllib.loads(debs["essentials"])["recommends"], ["a"])

    def test_desktop_set_is_the_essentials_size(self):
        text, debs = gen('[[package]]\nname = "a"\nset = "desktop"\ncheck = "a"\nbrew = ""\n')
        self.assertIn("essentials:apps:a:a:a:a:", text)
        self.assertEqual(tomllib.loads(debs["desktop"])["recommends"], ["a"])

    def test_no_check_is_deb_only(self):
        text, debs = gen('[[package]]\nname = "a"\nset = "recommended"\n')
        self.assertNotIn(":a:", text)
        self.assertEqual(tomllib.loads(debs["recommended"])["recommends"], ["a"])

    def test_services_are_suggests_and_must_be_listed(self):
        text, debs = gen('[[package]]\nname = "svc"\nset = "desktop"\ncheck = "s"\ngroup = "services"\n')
        self.assertIn("essentials:services:s:svc", text)
        self.assertEqual(tomllib.loads(debs["desktop"])["recommends"], [])
        with self.assertRaises(g.GenError):
            gen('[[package]]\nname = "other"\nset = "desktop"\ncheck = "o"\ngroup = "services"\n')

    def test_deb_relation_must_start_with_the_apt_name(self):
        _, debs = gen('[[package]]\nname = "7zip"\nset = "desktop"\ncheck = "7z"\ndeb = "7zip | p7zip-full"\n')
        self.assertEqual(tomllib.loads(debs["desktop"])["recommends"], ["7zip | p7zip-full"])
        with self.assertRaises(g.GenError):
            gen('[[package]]\nname = "a"\nset = "desktop"\ncheck = "a"\ndeb = "b"\n')

    def test_app_with_set(self):
        app = {"name": "Foo", "bin": "foo", "set": "recommended", "pkg": {"apt": "foo-tui", "brew": "foo"}}
        text, debs = gen(apps=[app, {"name": "Bar", "bin": "bar"}])
        self.assertIn("recommended:apps:foo:foo-tui:::foo", text)
        self.assertEqual(tomllib.loads(debs["recommended"])["recommends"], ["foo-tui"])

    def test_script(self):
        text, _ = gen(scripts='[[script]]\nname = "s"\nset = "recommended"\nhow = "its installer"\n')
        self.assertIn("recommended:script:s", text)

    def test_errors(self):
        for bad in ('[[package]]\nname = "a"\nset = "full"\ncheck = "a"\n',
                    '[[package]]\nname = "a"\nset = "essentials"\ncheck = "a"\ncolour = "x"\n',
                    '[[package]]\nname = "a"\nset = "essentials"\ncheck = "a"\ndnf = "x:y"\n',
                    '[[package]]\nname = "a"\nset = "essentials"\ncheck = "a"\n' * 2,
                    '[[package]]\nname = "a"\nset = "essentials"\napt = ""\n',
                    '[[script]]\nname = "s"\nset = "essentials"\n'):
            with self.assertRaises(g.GenError, msg=bad):
                gen(bad)


if __name__ == "__main__":
    unittest.main()
