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
# (a catalog app instead gets an [[app]] entry with only its name and set). It
# writes assets/suite-apps.txt and the recommends of packaging/<set>.toml;
# commit them with the list.
set -eu
cd "$(dirname "$0")/.."
python3 scripts/gen_suite_lists.py
python3 scripts/gen_suite_lists.py --check
python3 -m unittest -q scripts/test_gen_suite_lists.py
git status --short assets/suite-apps.txt packaging/ 2>/dev/null || true
echo "rebuild-app-sets: done; commit the files listed above with lists/tetron.toml."
echo "When assets/suite-apps.txt changed: push, run scripts/sync_suite_list.sh in tetron-appstore"
echo "(the catalog gives each app its set from it), push that, then here: cargo update -p tetron-catalog"
