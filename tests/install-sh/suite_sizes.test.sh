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
assets tetron-wm_9.9.9-1_$A.deb tetron-wm-desktop_9.9.9-1_all.deb tetron-wm-essentials_9.9.9-1_all.deb tetron-wm-recommended_9.9.9-1_all.deb tetron-wm-extras_9.9.9-1_all.deb
out="$(plan)"; ok "debian fresh, default size: essentials through its metapackages" "echo \"\$out\" | grep -q \"size=essentials method=deb\" && echo \"\$out\" | grep -q \"apt install tetron-wm-desktop tetron-wm-essentials (\""
out="$(plan --size desktop)"; ok "--size desktop is read as essentials" "echo \"\$out\" | grep -q \"size=essentials\""
out="$(plan --size recommended)"; ok "debian --size recommended: three metapackages" "echo \"\$out\" | grep -q \"apt install tetron-wm-desktop tetron-wm-essentials tetron-wm-recommended (\""
out="$(plan --size extras)"; ok "debian --size extras: all four metapackages" "echo \"\$out\" | grep -q \"apt install tetron-wm-desktop tetron-wm-essentials tetron-wm-recommended tetron-wm-extras (\""
out="$(plan)"; ok "no mouse device: the metapackage without gpm" "echo \"\$out\" | grep -q \"without gpm (no mouse device)\""
out="$(plan --size minimal)"; ok "debian --size minimal: no apps" "echo \"\$out\" | grep -q \"size=minimal\" && echo \"\$out\" | grep -q \"apps: none\""
out="$(plan --no-extras)"; ok "--no-extras means minimal" "echo \"\$out\" | grep -q \"size=minimal\""
printf "#!/bin/sh\necho raw\n" > /usr/local/bin/tetron-wm; chmod +x /usr/local/bin/tetron-wm
out="$(plan)"; ok "raw copy: not converted under -y, apps one by one with apt names" "echo \"\$out\" | grep -q \"method=binary wm=y\" && echo \"\$out\" | grep -q \"apps: .*poppler-utils\" && ! echo \"\$out\" | grep -q \"apt install tetron-wm-desktop\""
ok "no mouse device: gpm left out of the one-by-one list" "! echo \"\$out\" | grep -q \" gpm \""
mkdir -p /dev/input && touch /dev/input/mice
out="$(plan)"; ok "with a mouse device: gpm stays in" "echo \"\$out\" | grep -q \" gpm \""
rm -f /dev/input/mice
out="$(plan --convert)"; ok "raw copy with --convert: becomes the deb" "echo \"\$out\" | grep -q \"method=deb wm=y\""
out="$(plan --method deb)"; ok "raw copy with --method deb: becomes the deb, apps through the metapackages" "echo \"\$out\" | grep -q \"method=deb wm=y\" && echo \"\$out\" | grep -q \"apt install tetron-wm-desktop\""
rm /usr/local/bin/tetron-wm
assets tetron-wm_9.9.9-1_$A.deb tetron-wm-desktop_9.9.9-1_all.deb tetron-wm-extras_9.9.9-1_all.deb
out="$(plan)"; ok "release with only the metapackages from before the sets: apps one by one" "echo \"\$out\" | grep -q \"method=deb\" && echo \"\$out\" | grep -q \"apps: .*ripgrep\" && ! echo \"\$out\" | grep -q \"apt install tetron-wm\""
assets tetron-wm_9.9.9-1_$A.deb
out="$(plan)"; ok "release without metapackages: deb, apps one by one" "echo \"\$out\" | grep -q \"method=deb\" && echo \"\$out\" | grep -q \"apps: .*chafa\""
assets
out="$(plan)"; ok "release without any deb: binary" "echo \"\$out\" | grep -q \"method=binary\""
assets tetron-wm_9.9.9-1_$A.deb tetron-wm-desktop_9.9.9-1_all.deb tetron-wm-essentials_9.9.9-1_all.deb
out="$(plan --method binary)"; ok "--method binary, no package: binary, apps one by one" "echo \"\$out\" | grep -q \"method=binary wm=y\" && ! echo \"\$out\" | grep -q \"apt install tetron-wm-desktop\""
out="$(plan --from-clone /src)"; ok "--from-clone: an own build of the clone" "echo \"\$out\" | grep -q \"method=own (cargo build --release in /src) wm=y\""
out="$(bash /src/install-tetron-wm-suite.sh -y --dry-run --method bogus 2>&1 || true)"; ok "a bad --method is refused" "echo \"\$out\" | grep -q \"must be deb or binary\""
out="$(bash /src/install-tetron-wm-suite.sh -y --dry-run --method deb --from-clone /src 2>&1 || true)"; ok "--from-clone with --method deb is refused" "echo \"\$out\" | grep -q \"does not go with\""
printf "#!/bin/sh\necho own\n" > /usr/bin/tetron-wm; chmod +x /usr/bin/tetron-wm
dpkg-divert --local --rename --divert /usr/bin/tetron-wm.packaged --add /usr/bin/tetron-wm >/dev/null; printf "#!/bin/sh\necho own\n" > /usr/bin/tetron-wm; chmod +x /usr/bin/tetron-wm
out="$(plan)"; ok "own build (diverted): kept under -y" "echo \"\$out\" | grep -q \"method=own wm=n\""
out="$(plan --method deb)"; ok "own build with --method deb: back to the deb" "echo \"\$out\" | grep -q \"method=deb wm=y\""
'
run fedora:latest '
out="$(plan)"; ok "fedora: dnf names from the list" "echo \"\$out\" | grep -q \"method=binary\" && echo \"\$out\" | grep -q \"fuse-sshfs\" && echo \"\$out\" | grep -q \"vim-enhanced\" && echo \"\$out\" | grep -q \"dejavu-sans-mono-fonts\""
ok "essentials size plans opencode (the Assistant agent)" "echo \"\$out\" | grep -q \" opencode\""
ok "fedora essentials: fbida (View full screen images)" "echo \"\$out\" | grep -q fbida"
ok "fedora essentials: git, bind-utils; no recommended app" "echo \"\$out\" | grep -q \" git \" && echo \"\$out\" | grep -q bind-utils && ! echo \"\$out\" | grep -q ShellCheck"
out="$(plan --no-scripts)"; ok "--no-scripts: packages only, no opencode, no fresh" "echo \"\$out\" | grep -q fbida && ! echo \"\$out\" | grep -q \" opencode\" && ! echo \"\$out\" | grep -q \" fresh\""
out="$(plan --size recommended)"; ok "fedora recommended: dnf names of the recommended set" "echo \"\$out\" | grep -q ShellCheck && echo \"\$out\" | grep -q ffmpeg-free && ! echo \"\$out\" | grep -q lazygit"
ok "fedora recommended: croft, pyradio, llm, yt-dlp, freebuff (script apps)" "for a in croft pyradio llm yt-dlp freebuff; do echo \"\$out\" | grep -q \" \$a\" || exit 1; done"
ok "essentials plans none of the recommended script apps" "! plan | grep -E -q \" (croft|pyradio|freebuff)\""
out="$(plan --size recommended --no-scripts)"; ok "recommended --no-scripts: no croft, no freebuff" "echo \"\$out\" | grep -q ShellCheck && ! echo \"\$out\" | grep -q \" croft\" && ! echo \"\$out\" | grep -q \" freebuff\""
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
ok "arch essentials: gptfdisk, openbsd-netcat" "echo \"\$out\" | grep -q gptfdisk && echo \"\$out\" | grep -q openbsd-netcat"
out="$(plan --size recommended)"; ok "arch recommended: github-cli, go-yq, base-devel" "echo \"\$out\" | grep -q github-cli && echo \"\$out\" | grep -q go-yq && echo \"\$out\" | grep -q base-devel"
'
exit $fail
