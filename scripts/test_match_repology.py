"""Tests for scripts/match_repology.py (WMCAT-005). Run:
python3 -m unittest scripts/test_match_repology.py"""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import match_repology as mr  # noqa: E402

GDU = [
    {"repo": "debian_12", "srcname": "gdu", "binnames": ["gdu"]},
    {"repo": "debian_13", "srcname": "gdu", "binnames": ["gdu"]},
    {"repo": "fedora_42", "srcname": "gdu", "binnames": ["gdu"]},
    {"repo": "fedora_43", "srcname": "gdu", "binnames": ["gdu-debuginfo", "gdu"], "summary": "Fast disk usage analyzer"},
    {"repo": "arch", "srcname": "gdu", "binname": "gdu"},
    {"repo": "aur", "srcname": "gdu-bin", "binname": "gdu-bin"},
    {"repo": "homebrew", "srcname": "gdu", "binname": "gdu"},
]


def app(name, bin_, home, desc="Fast disk usage analyzer"):
    return {"name": name, "bin": bin_, "homepage": home, "description": desc}


class Pick(unittest.TestCase):
    def test_newest_releases_and_the_installable_name(self):
        self.assertEqual(mr.pick(GDU, "gdu"), {"apt": "gdu", "dnf": "gdu", "pacman": "gdu", "brew": "gdu"})

    def test_apt_falls_back_to_ubuntu_then_debian_12_and_skips_debug(self):
        p = [{"repo": "debian_12", "srcname": "x", "binnames": ["x-dbg", "x-tui"]}, {"repo": "aur", "binname": "x"}]
        self.assertEqual(mr.pick(p, "x"), {"apt": "x-tui"})
        p.append({"repo": "ubuntu_24_04", "srcname": "x", "binnames": ["x"]})
        self.assertEqual(mr.pick(p, "x"), {"apt": "x"})

    def test_candidates_bin_name_repo(self):
        self.assertEqual(mr.candidates(app("Bottom Tool", "btm", "https://github.com/o/bottom/")), ["btm", "bottom-tool", "bottom"])

    def test_match_tries_candidates_and_flags_unrelated_summaries(self):
        db = {"btm": [], "bottom": GDU}
        names, review, _ = mr.match([app("bottom", "btm", "https://github.com/o/bottom", "process monitor")], get=lambda n, o: db.get(n, []))
        self.assertEqual(names["bottom"]["pacman"], "gdu")
        self.assertEqual(len(review), 1, "a process monitor matched a disk analyzer is listed for a look")
        names, review, _ = mr.match([app("gdu", "gdu", "https://github.com/o/gdu")], get=lambda n, o: db.get(n, GDU))
        self.assertEqual((names["gdu"]["apt"], review), ("gdu", []))

    def test_unreachable_is_reported_not_written_as_no_package(self):
        names, _, unchecked = mr.match([app("gdu", "gdu", "https://github.com/o/gdu")], get=lambda n, o: None)
        self.assertEqual((names, unchecked), ({}, ["gdu"]))


class SameProject(unittest.TestCase):
    def test_forge_repository_must_match(self):
        self.assertTrue(mr.same_project("https://github.com/dundee/gdu", "https://github.com/dundee/gdu/"))
        self.assertTrue(mr.same_project("https://github.com/junegunn/fzf", "http://www.github.com/junegunn/fzf.git"))
        self.assertFalse(mr.same_project("https://github.com/FedericoBruzzone/tgt", "https://github.com/fujita/tgt"))

    def test_project_site_named_after_the_repository(self):
        self.assertTrue(mr.same_project("https://github.com/htop-dev/htop", "https://htop.dev/"))
        self.assertFalse(mr.same_project("https://github.com/FedericoBruzzone/tgt", "http://stgt.sourceforge.net/"))
        self.assertFalse(mr.same_project("https://github.com/x/trx", "http://www.pogo.org.uk/~mark/trx/"))
        self.assertFalse(mr.same_project("https://github.com/o/a", ""))

    def test_registry_pages_count_by_repository_name(self):
        self.assertTrue(mr.same_project("https://github.com/Byron/dua-cli", "https://crates.io/crates/dua-cli"))
        self.assertTrue(mr.same_project("https://github.com/TomasTomecek/sen", "http://pypi.python.org/pypi/sen"))
        self.assertFalse(mr.same_project("https://github.com/o/below", "https://crates.io/crates/other"))
        self.assertTrue(mr.same_project("https://github.com/gokcehan/lf", "https://godoc.org/github.com/gokcehan/lf"))

    def test_same_site_prefix(self):
        self.assertFalse(mr.same_project("https://www.gnu.org/software/nano", "https://www.nano-editor.org/"))
        self.assertTrue(mr.same_project("https://midnight-commander.org", "https://midnight-commander.org/"))

    def test_verify_keeps_only_matching_homepages(self):
        apps = [app("tgt", "tgt", "https://github.com/FedericoBruzzone/tgt"), app("gdu", "gdu", "https://github.com/dundee/gdu")]
        names = {"tgt": {"apt": "tgt", "brew": "tgt"}, "gdu": {"apt": "gdu"}}
        homes = {("apt", "tgt"): "http://stgt.sourceforge.net/", ("brew", "tgt"): "https://github.com/FedericoBruzzone/tgt",
                 ("apt", "gdu"): "https://github.com/dundee/gdu"}
        kept, rejected = mr.verify(apps, names, apt={}, brew={}, home=lambda m, n, a, b: homes[(m, n)])
        self.assertEqual(kept, {"tgt": {"brew": "tgt"}, "gdu": {"apt": "gdu"}})
        self.assertEqual(len(rejected), 1)

    def test_pick_prefers_the_command_and_skips_lib32(self):
        p = [{"repo": "arch", "binname": "lib32-util-linux"}, {"repo": "arch", "binname": "util-linux"}]
        self.assertEqual(mr.pick(p, "cfdisk"), {"pacman": "util-linux"})


if __name__ == "__main__":
    unittest.main()
