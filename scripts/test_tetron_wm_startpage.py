"""Tests for scripts/tetron-wm-startpage: conversion from bookmarks.txt, add, list."""
import importlib.machinery, importlib.util, os, tempfile, unittest

HERE = os.path.dirname(os.path.abspath(__file__))


def load(config):
    os.environ["XDG_CONFIG_HOME"] = config
    loader = importlib.machinery.SourceFileLoader("startpage", os.path.join(HERE, "tetron-wm-startpage"))
    spec = importlib.util.spec_from_loader("startpage", loader)
    mod = importlib.util.module_from_spec(spec)
    loader.exec_module(mod)
    return mod


class StartPage(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.sp = load(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_default_page_is_a_netscape_file_with_a_search_box(self):
        self.sp.ensure()
        text = open(self.sp.PAGE).read()
        self.assertTrue(text.startswith("<!DOCTYPE NETSCAPE-Bookmark-file-1>"))
        self.assertIn('action="https://lite.duckduckgo.com/lite/"', text)
        self.assertIn("Search | DuckDuckGo Lite | https://lite.duckduckgo.com/lite", list(self.sp.listing(text)))

    def test_page_is_tetron_browsers_file(self):
        self.assertEqual(self.sp.PAGE, os.path.join(self.tmp.name, "tetron-browser", "bookmarks.html"))
        self.assertEqual(self.sp.OLD, os.path.join(self.tmp.name, "tetron-wm", "bookmarks.txt"))

    def test_converts_bookmarks_txt_once_and_keeps_it(self):
        os.makedirs(os.path.dirname(self.sp.OLD))
        open(self.sp.OLD, "w").write("# c\nLoose | https://l.org\n[News]\nHN | https://news.ycombinator.com\n")
        self.sp.ensure()
        rows = list(self.sp.listing(open(self.sp.PAGE).read()))
        self.assertEqual(rows, ["Links | Loose | https://l.org", "News | HN | https://news.ycombinator.com"])
        self.assertTrue(os.path.exists(self.sp.OLD))
        open(self.sp.OLD, "w").write("[Other]\nX | https://x.org\n")
        self.sp.ensure()
        self.assertNotIn("x.org", open(self.sp.PAGE).read())

    def test_add_matches_tetron_browser_rules(self):
        self.sp.ensure()
        text = open(self.sp.PAGE).read()
        text = self.sp.add(text, "A & <b>", "https://a.org/?x=1&y='z'", "News")
        rows = list(self.sp.listing(text))
        self.assertEqual(rows[rows.index("News | Hacker News | https://news.ycombinator.com") - 1],
                         "News | A & <b> | https://a.org/?x=1&y='z'")
        self.assertIn("HREF=\"https://a.org/?x=1&amp;y='z'\"", text)
        self.assertEqual(self.sp.add(text, "again", "https://a.org/?x=1&y='z'", "Links"), text)
        self.assertEqual(self.sp.add(text, "slash", "https://en.wikipedia.org/", "Links"), text)
        text = self.sp.add(text, "", "https://n.org", "Brand New")
        self.assertEqual(list(self.sp.listing(text))[-1], "Brand New | https://n.org | https://n.org")
        self.assertTrue(text.rstrip().endswith("</DL><p>"))


if __name__ == "__main__":
    unittest.main()
