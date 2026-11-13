#!/bin/sh
# rebuild-app-sets.sh: regenerate everything made from the "App sets" section
# of lists/tetron.toml (WMINST-001), then check it converged. Run it after
# adding or moving an app between desktop, essentials, recommended and extras.
#
#   scripts/rebuild-app-sets.sh
#
# To add an app to a set, add an entry to lists/tetron.toml, for example:
#
#   [[package]]
#   name = "cmatrix"      # the Debian name
#   set = "extras"        # desktop | essentials | recommended | extras
#   check = "cmatrix"     # the command that shows it is installed
#
# (a Store app instead gets `set = "extras"` on its [[app]] entry). It writes
# assets/suite-apps.txt, the recommends of packaging/<set>.toml and
# assets/catalog.json; commit them with the list.
set -eu
cd "$(dirname "$0")/.."
python3 scripts/gen_suite_lists.py
python3 scripts/merge_catalog.py
python3 scripts/gen_suite_lists.py --check
python3 scripts/merge_catalog.py --check
python3 -m unittest -q scripts/test_gen_suite_lists.py scripts/test_merge_catalog.py
git status --short assets/suite-apps.txt assets/catalog.json packaging/ 2>/dev/null || true
echo "rebuild-app-sets: done; commit the files listed above with lists/tetron.toml"
