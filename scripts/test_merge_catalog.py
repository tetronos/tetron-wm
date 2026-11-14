"""Tests for scripts/merge_catalog.py (WMCAT-001, WMCAT-003). Run:
python3 -m unittest scripts/test_merge_catalog.py"""
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import merge_catalog as mc  # noqa: E402

SUITE = ("# list\nessentials:apps:htop:htop:htop:htop:htop\nrecommended:apps:lazygit:lazygit::lazygit:lazygit\n"
         "extras:apps:git:git:git:git:git\nessentials:script:fresh\nessentials:services:pipewire:pipewire:::\n")


def app(name, bin_, cat="Tools"):
    return {"name": name, "bin": bin_, "category": cat, "description": f"{name} d", "homepage": f"https://{bin_}"}


class Merge(unittest.TestCase):
    def merged(self, imported, ours, repology=None, trove=None, releases=None):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            if releases is not None:
                (d / "releases.json").write_text(json.dumps({"repos": releases}))
            if repology is not None:
                (d / "repology.json").write_text(json.dumps({"names": repology}))
            (d / "awesome-tuis.json").write_text(json.dumps(imported))
            if trove is not None:
                (d / "terminaltrove.json").write_text(json.dumps(trove))
            (d / "tetron.toml").write_text(ours)
            (d / "suite.txt").write_text(SUITE)
            doc = json.loads(mc.merge(d, d / "suite.txt"))
        return doc, {a["name"]: a for a in doc["apps"]}

    def test_ours_wins_field_by_field_matched_by_name_any_case(self):
        _, a = self.merged([app("Htop", "htop")], '[[app]]\nname = "htop"\ndescription = "ours"\n')
        self.assertEqual(list(a), ["htop"])
        self.assertEqual((a["htop"]["description"], a["htop"]["homepage"]), ("ours", "https://htop"))
        self.assertEqual(a["htop"]["sources"], ["awesome-tuis", "tetron"])

    def test_matched_by_command_name_when_names_differ(self):
        _, a = self.merged([app("btop++", "btop")], '[[app]]\nname = "btop"\nbin = "btop"\ncli = true\n')
        self.assertEqual(list(a), ["btop"])
        self.assertTrue(a["btop"]["cli"])

    def test_second_list_matches_by_repo_and_never_adds_cli(self):
        imported = [dict(app("GitUI", "gitui"), homepage="https://github.com/gitui-org/gitui")]
        trove = [dict(app("gitui-org", "gitui-x"), homepage="https://github.com/GitUI-org/gitui/", cli=True),
                 dict(app("jq", "jq"), cli=True)]
        _, a = self.merged(imported, "", trove=trove)
        self.assertEqual(sorted(a), ["GitUI", "jq"])
        self.assertEqual(a["GitUI"]["sources"], ["awesome-tuis", "terminaltrove"])
        self.assertNotIn("cli", a["GitUI"], "the earlier list said TUI")
        self.assertTrue(a["jq"]["cli"])

    def test_releases_flag_from_the_release_list(self):
        imported = [dict(app("a", "a"), homepage="https://github.com/O/A"), dict(app("b", "b"), homepage="https://github.com/o/b"),
                    dict(app("c", "c"), homepage="https://github.com/o/a/tree/main/c")]
        _, a = self.merged(imported, "", releases=["o/a"])
        self.assertEqual([n for n in "abc" if a[n].get("releases")], ["a"], "repository roots only")
        self.assertEqual(list(a["a"])[4:6], ["homepage", "releases"])

    def test_ours_never_matched_by_repo(self):
        imported = [dict(app("tetron-wm", "tetron-wm"), homepage="https://github.com/tetronos/tetron-wm")]
        ours = ('[[app]]\nname = "AI Mode"\nbin = "tetron-aimode"\ncategory = "AI"\ndescription = "d"\n'
                'homepage = "https://github.com/tetronos/tetron-wm"\n')
        _, a = self.merged(imported, ours)
        self.assertEqual(sorted(a), ["AI Mode", "tetron-wm"])

    def test_distinct_never_matches_and_exclude_drops(self):
        imported = [app("amp", "amp", "Editors"), app("termui", "termui")]
        ours = 'exclude = ["termui"]\n[[app]]\nname = "Amp"\ndistinct = true\nbin = "amp"\ncategory = "AI"\ndescription = "agent"\nhomepage = "https://ampcode.com"\n'
        _, a = self.merged(imported, ours)
        self.assertEqual(sorted(a), ["Amp", "amp"])
        self.assertNotIn("distinct", a["Amp"])

    def test_tier_and_set_from_the_suite_list(self):
        imported = [app("htop", "htop"), app("git", "git"), app("Fresh", "fresh"), app("pipewire", "pipewire"), app("x", "x"),
                    app("lazygit", "lazygit")]
        doc, a = self.merged(imported, '[[app]]\nname = "x"\ntier = "recommended"\n')
        self.assertEqual((a["htop"]["set"], a["htop"]["tier"]), ("essentials", "default"))
        self.assertEqual((a["lazygit"]["set"], a["lazygit"]["tier"]), ("recommended", "recommended"))
        self.assertEqual((a["git"]["set"], a["git"]["tier"]), ("extras", "optional"))
        self.assertEqual(a["Fresh"]["set"], "essentials")
        self.assertNotIn("set", a["pipewire"], "services are not a set")
        self.assertEqual((a["x"]["tier"], "set" in a["x"]), ("recommended", False))
        self.assertEqual(doc["suite_apps"], SUITE)
        self.assertEqual(doc["schema"], mc.SCHEMA)

    def test_package_names_from_repology_ours_override_name_by_name(self):
        found = {"htop": {"apt": "htop", "dnf": "htop", "pacman": "htop", "brew": "htop"}, "x": {"apt": "x-tui"}}
        ours = '[[app]]\nname = "htop"\npkg = { brew = "", dnf = "htop-ng" }\n'
        _, a = self.merged([app("htop", "htop"), app("x", "x"), app("y", "y")], ours, found)
        self.assertEqual(a["htop"]["pkg"], {"apt": "htop", "dnf": "htop-ng", "pacman": "htop"})
        self.assertEqual(a["x"]["pkg"], {"apt": "x-tui"})
        self.assertNotIn("pkg", a["y"])

    def test_bad_entries_fail_loudly(self):
        for ours in ('[[app]]\nname = "x"\ncolour = "red"\n', '[[app]]\nname = "x"\ntier = "must"\n',
                     '[[app]]\nname = "new"\nbin = "new"\n', '[[app]]\nname = "x"\n[app.recipe]\nmethod = "apt"\n',
                     '[[app]]\nname = "x"\npkg = { zypper = "x" }\n'):
            with self.subTest(ours=ours), self.assertRaises(SystemExit):
                self.merged([app("x", "x")], ours)

    def test_agent_table_carried_and_checked(self):
        """WMAST-001: an [app.agent] table reaches the catalog; bad keys fail."""
        ok = '[[app]]\nname = "htop"\n[app.agent]\nlabel = "H"\nmcp = { file = "x.json", template = "json" }\n'
        _, a = self.merged([app("htop", "htop")], ok)
        self.assertEqual(a["htop"]["agent"], {"label": "H", "mcp": {"file": "x.json", "template": "json"}})
        for bad in ('nope = 1', 'context_files = ["a/b.md"]', 'context_files = ["AGENTS.md"]',
                    'mcp = { file = "x.json" }', 'mcp = { file = "/etc/x", template = "json" }',
                    'mcp = { file = "x", template = "yaml" }', 'args = [1]',
                    'briefing = { env = { A = 1 } }', 'briefing = { nope = "x" }'):
            with self.assertRaises(SystemExit, msg=bad):
                self.merged([app("htop", "htop")], f'[[app]]\nname = "htop"\n[app.agent]\n{bad}\n')

    def test_committed_catalog_is_current(self):
        self.assertEqual(mc.merge(), mc.OUT.read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
