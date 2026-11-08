#!/bin/sh
# install.sh picks one install method per host (WMDEB-005). Runs install.sh in
# a debian:trixie container with no network against a fake local release: a
# real .deb built with dpkg-deb, its .sha256, the release asset list and a raw
# tarball, served through file:// by a tiny curl stand-in.
# Usage: tests/install-sh/install_sh_methods.test.sh   (needs docker and the debian:trixie image)
set -eu
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
IMAGE="${IMAGE:-debian:trixie}"
command -v docker >/dev/null 2>&1 || { echo "SKIP: no docker"; exit 0; }
docker image inspect "$IMAGE" >/dev/null 2>&1 || { echo "SKIP: no $IMAGE image (docker pull $IMAGE)"; exit 0; }
exec docker run --rm --network none -v "$ROOT/install.sh:/src/install.sh:ro" -i "$IMAGE" sh -s <<'IN'
set -eu
arch="$(dpkg --print-architecture)"
case "$(uname -m)" in x86_64) target=x86_64-unknown-linux-gnu ;; aarch64) target=aarch64-unknown-linux-gnu ;; esac
# curl stand-in: serves file:// URLs, fails on anything else (no network here).
cat > /usr/local/bin/curl <<'C'
#!/bin/sh
for a; do url="$a"; done
case "$url" in file://*) cat "${url#file://}" ;; *) exit 22 ;; esac
C
chmod +x /usr/local/bin/curl
mkdeb() { # mkdeb <version> <tag dir>
  d="$(mktemp -d)"; mkdir -p "$d/DEBIAN" "$d/usr/bin"
  printf 'Package: tetron-wm\nVersion: %s\nArchitecture: %s\nMaintainer: test <t@t>\nDescription: test\n' "$1" "$arch" > "$d/DEBIAN/control"
  printf '#!/bin/sh\necho deb-%s\n' "$1" > "$d/usr/bin/tetron-wm"; chmod 755 "$d/usr/bin/tetron-wm"
  f="tetron-wm_${1}_$arch.deb"; dpkg-deb -b --root-owner-group "$d" "$2/$f" >/dev/null
  (cd "$2" && sha256sum "$f" > "$f.sha256")
  printf '<a href="/o/r/releases/download/x/%s">%s</a>\n' "$f" "$f" >> "$2/assets.html"
}
mkmeta() { # mkmeta <name> <version> <tag dir>: an Architecture all metapackage on tetron-wm
  d="$(mktemp -d)"; mkdir -p "$d/DEBIAN"
  printf 'Package: %s\nVersion: %s\nArchitecture: all\nMaintainer: test <t@t>\nDepends: tetron-wm\nDescription: test\n' "$1" "$2" > "$d/DEBIAN/control"
  f="${1}_${2}_all.deb"; dpkg-deb -b --root-owner-group "$d" "$3/$f" >/dev/null
  (cd "$3" && sha256sum "$f" > "$f.sha256")
  printf '<a href="/o/r/releases/download/x/%s">%s</a>\n' "$f" "$f" >> "$3/assets.html"
}
mkraw() { # mkraw <version> <tag dir>
  d="$(mktemp -d)"; printf '#!/bin/sh\necho raw-%s\n' "$1" > "$d/tetron-wm"; chmod 755 "$d/tetron-wm"
  tar -czf "$2/tetron-wm-$target.tar.gz" -C "$d" tetron-wm
}
R=/rel; mkdir -p $R/download $R/expanded_assets
for t in v9.9.7 v9.9.8 v9.9.9 v9.9.6; do mkdir -p $R/download/$t; : > $R/download/$t/assets.html; done
mkdeb 9.9.8-1 $R/download/v9.9.8; mkraw 9.9.8 $R/download/v9.9.8
# The release also lists the tetron-wm-desktop metapackage, first: install.sh
# must never take it for tetron-wm.
printf '<a href="/o/r/releases/download/x/tetron-wm-desktop_9.9.9-1_all.deb">tetron-wm-desktop_9.9.9-1_all.deb</a>\n' >> $R/download/v9.9.9/assets.html
mkdeb 9.9.9-1 $R/download/v9.9.9; mkraw 9.9.9 $R/download/v9.9.9
for m in tetron-wm-desktop tetron-wm-essentials tetron-wm-recommended; do
  mkmeta $m 9.9.8-1 $R/download/v9.9.8; mkmeta $m 9.9.9-1 $R/download/v9.9.9
done
mkraw 9.9.7 $R/download/v9.9.7                       # v9.9.7: no .deb
mkdeb 9.9.6-1 $R/download/v9.9.6; mkraw 9.9.6 $R/download/v9.9.6
echo 0000 > $R/download/v9.9.6/tetron-wm_9.9.6-1_$arch.deb.sha256   # v9.9.6: bad checksum
for t in v9.9.6 v9.9.7 v9.9.8 v9.9.9; do cp $R/download/$t/assets.html $R/expanded_assets/$t; done

run() { TETRON_WM_TAG="$1" TETRON_WM_RELEASES_URL=file://$R TETRON_WM_SKIP_DEPS=1 sh /src/install.sh </dev/null > /tmp/out 2>&1; }
reset() { dpkg -P tetron-wm-recommended tetron-wm-essentials tetron-wm-desktop tetron-wm >/dev/null 2>&1 || true; rm -f /usr/local/bin/tetron-wm; }
mver() { dpkg-query -W -f='${Version}' "${1:-tetron-wm-desktop}" 2>/dev/null || true; }
ver() { dpkg-query -W -f='${Version}' tetron-wm 2>/dev/null || true; }
fail=0
check() { if eval "$2"; then echo "PASS $1"; else echo "FAIL $1 (rc=$rc ver=$(ver) usrbin=$(/usr/bin/tetron-wm 2>&1) local=$(ls /usr/local/bin/tetron-wm 2>&1))"; sed 's/^/    /' /tmp/out; fail=1; fi; }

reset; run v9.9.9; rc=$?
check "fresh host with a .deb in the release: apt installs the package" '[ $rc = 0 ] && [ "$(ver)" = 9.9.9-1 ] && [ "$(/usr/bin/tetron-wm)" = deb-9.9.9-1 ] && [ ! -e /usr/local/bin/tetron-wm ]'

reset; dpkg -i $R/download/v9.9.8/tetron-wm_9.9.8-1_$arch.deb >/dev/null; run v9.9.9; rc=$?
check "packaged host: the new .deb upgrades the package" '[ $rc = 0 ] && [ "$(ver)" = 9.9.9-1 ] && [ ! -e /usr/local/bin/tetron-wm ]'

reset; dpkg -i $R/download/v9.9.8/tetron-wm_9.9.8-1_$arch.deb >/dev/null; rc=0; run v9.9.7 || rc=$?
check "packaged host, release without a .deb: exit 3, nothing changed" '[ $rc = 3 ] && [ "$(ver)" = 9.9.8-1 ] && [ ! -e /usr/local/bin/tetron-wm ] && grep -q "Nothing changed" /tmp/out'

reset; mkraw 0.0.1 /tmp && tar -xzf /tmp/tetron-wm-$target.tar.gz -C /usr/local/bin; run v9.9.9; rc=$?
check "raw copy present: upgraded in place, package not installed, note shown" '[ $rc = 0 ] && [ "$(/usr/local/bin/tetron-wm)" = raw-9.9.9 ] && [ -z "$(ver)" ] && grep -q "not from a package" /tmp/out'

reset; run v9.9.7; rc=$?
check "fresh host, release without a .deb: raw binary" '[ $rc = 0 ] && [ "$(/usr/local/bin/tetron-wm)" = raw-9.9.7 ] && [ -z "$(ver)" ]'

reset; rc=0; run v9.9.6 || rc=$?
check "bad checksum: refused, nothing installed" '[ $rc = 1 ] && [ -z "$(ver)" ] && [ ! -e /usr/local/bin/tetron-wm ] && grep -q "checksum mismatch" /tmp/out'

reset; dpkg -i $R/download/v9.9.8/tetron-wm_9.9.8-1_$arch.deb $R/download/v9.9.8/tetron-wm-desktop_9.9.8-1_all.deb >/dev/null; run v9.9.9; rc=$?
check "installed tetron-wm-desktop moves to the release with tetron-wm" '[ $rc = 0 ] && [ "$(ver)" = 9.9.9-1 ] && [ "$(mver)" = 9.9.9-1 ]'

reset; dpkg -i $R/download/v9.9.8/tetron-wm_9.9.8-1_$arch.deb $R/download/v9.9.8/tetron-wm-desktop_9.9.8-1_all.deb $R/download/v9.9.8/tetron-wm-recommended_9.9.8-1_all.deb >/dev/null; run v9.9.9; rc=$?
check "size packages up to the largest installed move together; a smaller one missing is added" '[ $rc = 0 ] && [ "$(mver tetron-wm-recommended)" = 9.9.9-1 ] && [ "$(mver tetron-wm-essentials)" = 9.9.9-1 ] && [ "$(mver)" = 9.9.9-1 ]'

reset; dpkg -i $R/download/v9.9.8/tetron-wm_9.9.8-1_$arch.deb $R/download/v9.9.8/tetron-wm-desktop_9.9.8-1_all.deb >/dev/null; run v9.9.9; rc=$?
check "only tetron-wm-desktop installed: essentials not added" '[ $rc = 0 ] && [ "$(mver)" = 9.9.9-1 ] && [ -z "$(mver tetron-wm-essentials)" ]'

reset; dpkg -i $R/download/v9.9.8/tetron-wm_9.9.8-1_$arch.deb >/dev/null; run v9.9.9; rc=$?
check "metapackage not installed: not added by an update" '[ $rc = 0 ] && [ "$(ver)" = 9.9.9-1 ] && [ -z "$(mver)" ]'

reset; rc=0; TETRON_WM_TAG=v9.9.7 TETRON_WM_RELEASES_URL=file://$R sh /src/install.sh </dev/null > /tmp/out 2>&1 || rc=$?
check "direct run: tetron-wm only, then points to the suite script" '[ $rc = 0 ] && grep -q "install-tetron-wm-suite.sh" /tmp/out && ! grep -qi "installing optional" /tmp/out'
run v9.9.7
check "called by the suite (TETRON_WM_SKIP_DEPS=1): no pointer" '! grep -q "install-tetron-wm-suite.sh" /tmp/out'

exit $fail
IN
