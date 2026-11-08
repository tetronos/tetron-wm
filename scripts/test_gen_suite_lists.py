"""Tests for gen_suite_lists.py (WMINST-001). Run: python3 -m unittest scripts/test_gen_suite_lists.py"""
import shutil
import sys
import tempfile
import tomllib
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gen_suite_lists as g  # noqa: E402

# The checks of the suite script's hand-written lists before WMINST-001.
FORMER = ["chafa", "uv", "mpv", "pdftoppm", "w3m", "fresh", "sshpass", "htop", "btop", "upower", "alsamixer",
          "font=DejaVu Sans Mono", "blueutil", "xclip", "wl-copy", "wpctl", "nmcli", "bluetoothctl",
          "tetron-wm-startpage", "tetron-aimode"]
# Former names that live on as a script-only entry.
SCRIPT_FOR = {"fresh": "fresh", "tetron-wm-startpage": "helpers", "tetron-aimode": "helpers"}


class Generated(unittest.TestCase):
    text = g.generate()
    rows = [l.split(":") for l in text.splitlines() if l and not l.startswith("#")]

    def test_every_recommends_of_both_debs_is_listed(self):
        apt = {r[3] for r in self.rows if r[1] != "script"}
        for _, toml in g.SIZES:
            for rel in tomllib.loads(toml.read_text()).get("recommends", []):
                self.assertIn(g.debian_name(rel), apt, f"{toml.name}: {rel}")

    def test_nothing_from_the_former_lists_is_lost(self):
        checks = {c for r in self.rows if r[1] != "script" for c in r[2].split("|")}
        scripts = {r[2] for r in self.rows if r[1] == "script"}
        for c in FORMER:
            ok = c in checks or SCRIPT_FOR.get(c) in scripts or c in g.DROPPED
            self.assertTrue(ok, f"former entry {c} is neither generated nor in DROPPED")

    def test_line_shape(self):
        for r in self.rows:
            self.assertIn(r[0], ("desktop", "extras"))
            self.assertEqual(len(r), 3 if r[1] == "script" else 7, r)
            if r[1] != "script":
                self.assertIn(r[1], ("apps", "gui", "services"))

    def test_extras_only_add_to_desktop(self):
        self.assertEqual({r[3] for r in self.rows if r[0] == "extras"}, {"git"})


class Errors(unittest.TestCase):
    def setUp(self):
        self.d = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.d)
        (self.d / "desk.toml").write_text('recommends = ["a"]\n')
        (self.d / "so.toml").write_text("")

    def gen(self, names: str) -> str:
        (self.d / "names.toml").write_text(names)
        return g.generate((("desktop", self.d / "desk.toml"),), self.d / "names.toml", self.d / "so.toml")

    def test_recommends_without_a_map_entry_is_an_error(self):
        with self.assertRaises(g.GenError):
            self.gen("")

    def test_map_entry_no_package_uses_is_an_error(self):
        with self.assertRaises(g.GenError):
            self.gen('[app.a]\ncheck = "a"\n[app.stale]\ncheck = "s"\n')

    def test_separator_in_a_name_is_an_error(self):
        with self.assertRaises(g.GenError):
            self.gen('[app.a]\ncheck = "a"\ndnf = "x:y"\n')

    def test_defaults_to_the_debian_name(self):
        self.assertIn("desktop:apps:a:a:a:a:a", self.gen('[app.a]\ncheck = "a"\n'))


if __name__ == "__main__":
    unittest.main()
