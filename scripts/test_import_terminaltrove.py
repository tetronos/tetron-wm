"""Tests for scripts/import_terminaltrove.py (WMCAT-001). Run:
python3 -m unittest scripts/test_import_terminaltrove.py"""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import import_terminaltrove as it  # noqa: E402


def tool(name, tags, **kw):
    return dict({"name": name, "slug": name.lower(), "repo": f"o/{name}", "tagline": f"{name} d",
                 "categories": tags, "archived": False, "pushed_at": "2019-01-01"}, **kw)


class Import(unittest.TestCase):
    def test_filters_and_fields(self):
        apps = {a["name"]: a for a in it.parse({"tools": [
            tool("Old", ["linux", "tui", "git", "ai"]),
            tool("Jq", ["macos", "cli", "json"]),
            tool("Gone", ["linux", "tui"], archived=True),
            tool("Win", ["windows", "tui"]),
            tool("Odd", ["linux", "tui", "fonts"]),
        ]})}
        self.assertEqual(sorted(apps), ["Jq", "Odd", "Old"], "old tools stay; archived and Windows-only go")
        self.assertEqual(apps["Old"], {"name": "Old", "bin": "old", "category": "AI", "description": "Old d",
                                       "homepage": "https://github.com/o/Old"})
        self.assertTrue(apps["Jq"]["cli"])
        self.assertEqual((apps["Jq"]["category"], apps["Odd"]["category"]), ("Development", "Miscellaneous"))


if __name__ == "__main__":
    unittest.main()
