#!/bin/sh
# Install the given debs together in a clean container WITH Recommends (as
# `apt install tetron-wm-desktop` does by default), then, for each metapackage
# among them (Architecture all), check from its own control fields that:
# - every Recommends is installed (for an `a | b` alternative, one of them);
# - no Suggests was installed, not even through a Recommends of a Recommends.
#   A Suggests such as openssh-server would start a network service.
# mkdeb's own install test uses --no-install-recommends, so it covers neither
# (WMDEB-007, WMDEB-009).
# Usage: packaging/test-metapackages.sh [-i IMAGE] tetron-wm.deb tetron-wm-desktop.deb [tetron-wm-extras.deb]
set -eu
image="debian:trixie"
[ "${1:-}" = "-i" ] && { image="$2"; shift 2; }
[ $# -ge 2 ] || { echo "usage: $0 [-i image] tetron-wm.deb metapackage.deb..." >&2; exit 2; }
mounts=""; checks=""; n=0
for d in "$@"; do
  n=$((n + 1)); abs="$(cd "$(dirname "$d")" && pwd)/$(basename "$d")"
  mounts="$mounts -v $abs:/debs/$n.deb:ro"
  if [ "$(dpkg-deb -f "$abs" Architecture)" = all ]; then
    checks="$checks$(dpkg-deb -f "$abs" Package);$(dpkg-deb -f "$abs" Recommends);$(dpkg-deb -f "$abs" Suggests)
"
  fi
done
echo "== $image: $*"
# shellcheck disable=SC2086
docker run --rm $mounts -e CHECKS="$checks" "$image" sh -c '
set -e
apt-get update -qq >/dev/null 2>&1
# The tetronos apt repo, for the Recommends Debian does not have (tetron-browser).
DEBIAN_FRONTEND=noninteractive apt-get install -y -qq ca-certificates curl >/dev/null 2>&1
curl -fsSLo /usr/share/keyrings/tetronos-archive-keyring.gpg https://tetronos.github.io/apt/tetronos-archive-keyring.gpg
printf "Types: deb\nURIs: https://tetronos.github.io/apt\nSuites: stable\nComponents: main\nSigned-By: /usr/share/keyrings/tetronos-archive-keyring.gpg\n" > /etc/apt/sources.list.d/tetronos.sources
apt-get update -qq >/dev/null 2>&1
if ! DEBIAN_FRONTEND=noninteractive apt-get install -y -qq /debs/*.deb >/tmp/apt.log 2>&1; then
  tail -20 /tmp/apt.log; echo "FAIL: apt install"; exit 1
fi
name() { echo "$1" | sed "s/(.*//; s/ //g"; }
fail=0
echo "$CHECKS" | while IFS=";" read -r pkg recs sugs; do
  [ -n "$pkg" ] || continue
  echo "-- $pkg"
  IFS=,
  for alt in $recs; do
    ok=""; IFS="|"
    for p in $alt; do p="$(name "$p")"; [ -n "$p" ] && dpkg -s "$p" >/dev/null 2>&1 && ok="$p"; done
    IFS=,
    if [ -n "$ok" ]; then echo "  ok   recommends $ok"; else echo "  FAIL recommends $alt not installed"; echo x >> /tmp/fail; fi
  done
  for s in $sugs; do
    s="$(name "$s")"
    if dpkg -s "$s" >/dev/null 2>&1; then echo "  FAIL suggests $s was installed"; echo x >> /tmp/fail; else echo "  ok   suggests $s not installed"; fi
  done
  IFS=" "
done
if [ -s /tmp/fail ]; then exit 1; fi
echo "ALL PASS"
'
