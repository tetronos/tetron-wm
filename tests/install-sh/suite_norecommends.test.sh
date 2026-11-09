#!/bin/sh
# install-tetron-wm-suite.sh on a system that turns Recommends off
# (APT::Install-Recommends "false", as TetronOS images ship): the size debs
# still bring their apps, marked automatic, and a rerun over a size installed
# without its Recommends adds them (WMINST-002). Needs docker, network for
# apt, and built debs of this tree (tetron-wm and the four set debs).
# Usage: DEBS=DIR tests/install-sh/suite_norecommends.test.sh
set -eu
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
command -v docker >/dev/null 2>&1 || { echo "SKIP: no docker"; exit 0; }
[ -n "${DEBS:-}" ] && ls "$DEBS"/tetron-wm-essentials_*_all.deb >/dev/null 2>&1 || { echo "SKIP: set DEBS to a directory of built debs"; exit 0; }
DEBS="$(cd "$DEBS" && pwd)"
docker run --rm -v "$ROOT:/src:ro" -v "$DEBS:/debs:ro" -i debian:trixie bash -s <<'IN'
set -eu
fail=0
ok() { if eval "$2"; then echo "PASS $1"; else echo "FAIL $1"; fail=1; fi; }
apt-get update -qq >/dev/null
DEBIAN_FRONTEND=noninteractive apt-get install -y -qq curl ca-certificates sudo >/dev/null
echo 'APT::Install-Recommends "false";' > /etc/apt/apt.conf.d/00recommends
# The release, served as files: the installer's fetch takes file:// URLs.
v=$(ls /debs/tetron-wm_*_amd64.deb | sed 's/.*tetron-wm_\(.*\)-[0-9]*_amd64.deb/\1/'); tag="v$v"
mkdir -p "/rel/download/$tag" /rel/expanded_assets
cp /debs/*.deb /debs/*.sha256 "/rel/download/$tag/"
ls /debs | grep '\.deb$' > "/rel/expanded_assets/$tag"
mv /usr/bin/curl /usr/bin/curl.real
cat > /usr/bin/curl <<'C'
#!/bin/sh
for a; do url="$a"; done
case "$url" in file://*) cat "${url#file://}" ;; *) exec /usr/bin/curl.real "$@" ;; esac
C
chmod +x /usr/bin/curl
export TETRON_WM_RELEASES_URL=file:///rel TETRON_WM_TAG="$tag"
DEBIAN_FRONTEND=noninteractive apt-get install -y -qq /debs/tetron-wm_*_amd64.deb >/dev/null
useradd -m u && echo 'u ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/u
run() { su u -c "TETRON_WM_RELEASES_URL=file:///rel TETRON_WM_TAG=$tag bash /src/install-tetron-wm-suite.sh -y --size essentials --no-wm --no-config --no-themes --no-convert --no-kmscon --no-console --no-autologin --no-autostart --no-fullscreen --no-scripts" > /tmp/out 2>&1; }

# 1. Fresh install with Recommends off.
run && rc=0 || rc=$?
ok "fresh install exits 0" "[ $rc = 0 ]"
ok "fresh install: essentials apps present (tmux, git, ripgrep, mpv)" "command -v tmux >/dev/null && command -v git >/dev/null && command -v rg >/dev/null && command -v mpv >/dev/null"
ok "fresh install: apps are automatic, so Remove set takes them" "apt-mark showauto | grep -qx tmux"

# 2. A size installed without its Recommends, then the installer again.
DEBIAN_FRONTEND=noninteractive apt-get purge -y -qq tetron-wm-essentials >/dev/null
DEBIAN_FRONTEND=noninteractive apt-get autoremove -y -qq >/dev/null
ok "set removed: tmux gone" "! command -v tmux >/dev/null"
DEBIAN_FRONTEND=noninteractive apt-get install -y -qq /debs/tetron-wm-essentials_*_all.deb >/dev/null
ok "metapackage alone: tmux still missing" "! command -v tmux >/dev/null"
run && rc=0 || rc=$?
ok "rerun exits 0" "[ $rc = 0 ]"
ok "rerun: the missing apps are added" "command -v tmux >/dev/null && command -v rg >/dev/null"
ok "rerun names what apt left out" "grep -q 'apps apt left out' /tmp/out"
ok "rerun: added apps are automatic" "apt-mark showauto | grep -qx tmux"
[ "$fail" = 0 ] || { tail -30 /tmp/out; exit 1; }
echo "ALL PASS"
IN
