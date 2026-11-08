#!/bin/sh
# install-tetron-wm-suite.sh sizes and methods (WMINST-002), as --dry-run plans,
# in containers with no network against a fake local release served through a
# curl stand-in. Debian: deb versus one-by-one, sizes, raw conversion. Fedora
# and Arch: the package names from assets/suite-apps.txt.
# Usage: tests/install-sh/suite_sizes.test.sh   (needs docker; debian:trixie, fedora, archlinux images)
set -eu
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
command -v docker >/dev/null 2>&1 || { echo "SKIP: no docker"; exit 0; }
fail=0
run() { # run IMAGE SCRIPT
  docker run --rm --network none -v "$ROOT:/src:ro" -i "$1" bash -s <<IN || fail=1
set -eu
cat > /usr/local/bin/curl <<'C'
#!/bin/sh
for a; do url="\$a"; done
case "\$url" in file://*) cat "\${url#file://}" ;; *) exit 22 ;; esac
C
chmod +x /usr/local/bin/curl
export TETRON_WM_RELEASES_URL=file:///rel TETRON_WM_TAG=v9.9.9
mkdir -p /rel/expanded_assets
plan() { bash /src/install-tetron-wm-suite.sh -y --dry-run --no-kmscon --no-console --no-autostart --no-themes --no-config "\$@" 2>&1 | grep -E "plan|apps:" ; }
ok() { if eval "\$2"; then echo "PASS \$1"; else echo "FAIL \$1"; echo "\$out" | sed 's/^/    /'; echo x >> /tmp/fail; fi; }
$2
[ ! -s /tmp/fail ]
IN
}

run debian:trixie '
A=$(dpkg --print-architecture)
assets() { printf "%s\n" "$@" > /rel/expanded_assets/v9.9.9; }
assets tetron-wm_9.9.9-1_$A.deb tetron-wm-desktop_9.9.9-1_all.deb tetron-wm-extras_9.9.9-1_all.deb
out="$(plan)"; ok "debian fresh, default size: desktop through the metapackage" "echo \"\$out\" | grep -q \"size=desktop method=deb\" && echo \"\$out\" | grep -q \"apt install tetron-wm-desktop \""
out="$(plan --size extras)"; ok "debian --size extras: both metapackages" "echo \"\$out\" | grep -q \"apt install tetron-wm-desktop tetron-wm-extras\""
out="$(plan)"; ok "no mouse device: the metapackage without gpm" "echo \"\$out\" | grep -q \"without gpm (no mouse device)\""
out="$(plan --size minimal)"; ok "debian --size minimal: no apps" "echo \"\$out\" | grep -q \"size=minimal\" && echo \"\$out\" | grep -q \"apps: none\""
out="$(plan --no-extras)"; ok "--no-extras means minimal" "echo \"\$out\" | grep -q \"size=minimal\""
printf "#!/bin/sh\necho raw\n" > /usr/local/bin/tetron-wm; chmod +x /usr/local/bin/tetron-wm
out="$(plan)"; ok "raw copy: not converted under -y, apps one by one with apt names" "echo \"\$out\" | grep -q \"method=raw wm=y convert=n\" && echo \"\$out\" | grep -q \"apps: .*poppler-utils\" && ! echo \"\$out\" | grep -q \"apt install tetron-wm-desktop\""
ok "no mouse device: gpm left out of the one-by-one list" "! echo \"\$out\" | grep -q \" gpm \""
mkdir -p /dev/input && touch /dev/input/mice
out="$(plan)"; ok "with a mouse device: gpm stays in" "echo \"\$out\" | grep -q \" gpm \""
rm -f /dev/input/mice
out="$(plan --convert)"; ok "raw copy with --convert: becomes the deb" "echo \"\$out\" | grep -q \"method=deb wm=y convert=y\""
rm /usr/local/bin/tetron-wm
assets tetron-wm_9.9.9-1_$A.deb
out="$(plan)"; ok "release without metapackages: deb, apps one by one" "echo \"\$out\" | grep -q \"method=deb\" && echo \"\$out\" | grep -q \"apps: .*chafa\""
assets
out="$(plan)"; ok "release without any deb: raw" "echo \"\$out\" | grep -q \"method=raw\""
'
run fedora:latest '
out="$(plan)"; ok "fedora: dnf names from the list" "echo \"\$out\" | grep -q \"method=raw\" && echo \"\$out\" | grep -q \"fuse-sshfs\" && echo \"\$out\" | grep -q \"vim-enhanced\" && echo \"\$out\" | grep -q \"dejavu-sans-mono-fonts\""
ok "desktop size plans opencode (the Assistant agent)" "echo \"\$out\" | grep -q \" opencode\""
out="$(plan --size extras)"; ok "fedora extras: fbida" "echo \"\$out\" | grep -q fbida"
printf "desktop:apps:onlyme:onlyme:onlyme:onlyme:onlyme\n" > /tmp/list
out="$(plan --apps-list /tmp/list)"; ok "--apps-list: the given list, not the checkout one" "echo \"\$out\" | grep -q onlyme && ! echo \"\$out\" | grep -q vim-enhanced"
m="$(sed -n "s/^LISTS_MINOR=\"\(.*\)\"$/\1/p" /src/install-tetron-wm-suite.sh)"
mkdir -p "/lists/$m" /tmp/piped
printf "desktop:apps:fromlists:fromlists:fromlists:fromlists:fromlists\n" > "/lists/$m/suite-apps.txt"
cp /src/install-tetron-wm-suite.sh /tmp/piped/
out="$(TETRON_WM_LISTS_URL=file:///lists bash /tmp/piped/install-tetron-wm-suite.sh -y --dry-run --no-kmscon --no-console --no-autostart --no-themes --no-config 2>&1 | grep -E "plan|apps:")"
ok "run on its own: the list published for its core minor" "echo \"\$out\" | grep -q fromlists"
'
run archlinux:latest '
out="$(plan)"; ok "arch: pacman names from the list" "echo \"\$out\" | grep -q \"apps: .*poppler \" && echo \"\$out\" | grep -q ttf-dejavu && echo \"\$out\" | grep -q openssh"
'
exit $fail
