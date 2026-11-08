#!/bin/sh
# tetron-wm installer — downloads the latest prebuilt binary for your platform.
#
#   curl -fsSL https://raw.githubusercontent.com/tetronos/tetron-wm/main/install.sh | sh
#
# The default install dir is /usr/local/bin (system-wide, already on PATH), which
# needs root — the script uses sudo when the dir is not writable. For a no-sudo,
# userspace install set TETRON_WM_BIN_DIR to a writable dir, e.g.:
#   TETRON_WM_BIN_DIR="$HOME/.local/bin" curl -fsSL …/install.sh | sh
#
# One install method per host. On Debian and Ubuntu (dpkg and apt-get present):
# - tetron-wm installed by apt: the release .deb upgrades it, through apt. When
#   the release has no .deb the script changes nothing and exits with status 3,
#   so it never writes over a file the package owns.
# - a raw copy (not from a package): upgraded in place, as before.
# - neither: the release .deb is installed when the release has one, else the
#   raw binary.
# Everywhere else (other Linux, macOS) the raw binary, as before.
set -eu

# Prebuilt binaries come from the tetron-wm release repo. The binary is named
# `tetron-wm` and installs to its own paths, so it collides with nothing else.
REPO="tetronos/tetron-wm"
PKG="tetron-wm"
BIN_DIR="${TETRON_WM_BIN_DIR:-/usr/local/bin}"
# Release downloads and the tag can be overridden (tests serve a local release).
RELEASES="${TETRON_WM_RELEASES_URL:-https://github.com/$REPO/releases}"

# Does writing BIN_DIR need root? Walk up to the first existing ancestor and
# test it: a path under $HOME (even if the dir doesn't exist yet) is writable
# and needs no sudo, while /usr/local/bin exists but is root-owned.
need_sudo_for() {
  d="$1"
  while [ ! -e "$d" ]; do d="$(dirname "$d")"; [ "$d" = "/" ] && break; done
  [ -w "$d" ] && return 1 || return 0
}
SUDO=""
raw_sudo() {
  if need_sudo_for "$BIN_DIR"; then
    if [ "$(id -u)" -eq 0 ]; then :
    elif command -v sudo >/dev/null 2>&1; then
      echo "tetron-wm: $BIN_DIR needs root — using sudo (set TETRON_WM_BIN_DIR to a user dir for a no-sudo install)"
      SUDO="sudo"
    else
      echo "tetron-wm: cannot write $BIN_DIR and sudo is unavailable." >&2
      echo "  Retry with:  TETRON_WM_BIN_DIR=\"\$HOME/.local/bin\" curl -fsSL …/install.sh | sh" >&2
      exit 1
    fi
  fi
}

# Stock Debian often has wget but not curl; accept either.
fetch() {
  if command -v curl >/dev/null 2>&1; then curl -fsSL "$1"
  elif command -v wget >/dev/null 2>&1; then wget -qO- "$1"
  else echo "tetron-wm: need curl or wget to download" >&2; return 1
  fi
}

# Resolve the latest release tag WITHOUT the api.github.com REST endpoint.
# That endpoint is rate-limited to 60 requests/hour per IP and answers 403 once
# the budget is spent — which previously surfaced as a bogus "no published
# release yet" and silently wedged the in-app updater into a slow source-build
# fallback. The web redirect github.com/OWNER/REPO/releases/latest 302s to
# .../releases/tag/vX.Y.Z off a different, effectively-unlimited path; we read
# the tag straight out of its Location header. The REST API is only a fallback
# for when the redirect can't be parsed (and we're still under the hourly cap).
latest_tag() {
  redirect="https://github.com/$REPO/releases/latest"
  loc=""
  if command -v curl >/dev/null 2>&1; then
    loc="$(curl -fsSI "$redirect" | tr -d '\r' | awk 'tolower($1)=="location:"{print $2}' | tail -1)"
  elif command -v wget >/dev/null 2>&1; then
    loc="$(wget -qS --max-redirect=0 -O /dev/null "$redirect" 2>&1 | tr -d '\r' | awk 'tolower($1)=="location:"{print $2}' | tail -1)"
  fi
  case "$loc" in
    */releases/tag/*) printf '%s\n' "${loc##*/}"; return 0 ;;
  esac
  fetch "https://api.github.com/repos/$REPO/releases/latest" \
    | grep '"tag_name"' | head -1 | cut -d'"' -f4
}

os="$(uname -s)"
arch="$(uname -m)"
case "$os/$arch" in
  Darwin/arm64)        target="aarch64-apple-darwin" ;;
  Darwin/x86_64)       target="x86_64-apple-darwin" ;;
  Linux/x86_64)        target="x86_64-unknown-linux-gnu" ;;
  Linux/aarch64)       target="aarch64-unknown-linux-gnu" ;;
  *)
    echo "tetron-wm: no prebuilt binary for $os/$arch."
    echo "Install with Rust instead:  cargo install --git https://github.com/$REPO"
    exit 1 ;;
esac

tag="${TETRON_WM_TAG:-}"
if [ -z "$tag" ]; then
  echo "tetron-wm: finding latest release…"
  tag="$(latest_tag || true)"
fi
if [ -z "${tag:-}" ]; then
  echo "tetron-wm: couldn't resolve the latest release (GitHub unreachable or rate-limited)."
  echo "Install with Rust instead:  cargo install --git https://github.com/$REPO"
  exit 1
fi

# Debian and Ubuntu: dpkg and apt-get present.
deb_host() {
  [ "$os" = Linux ] && command -v dpkg >/dev/null 2>&1 && command -v apt-get >/dev/null 2>&1
}
# apt installed the tetron-wm package.
pkg_installed() {
  dpkg-query -W -f='${db:Status-Status}' "$PKG" 2>/dev/null | grep -qx installed
}
# The first tetron-wm (in BIN_DIR, else on PATH) that no package owns.
raw_copy() {
  for p in "$BIN_DIR/tetron-wm" "$(command -v tetron-wm 2>/dev/null || true)"; do
    [ -n "$p" ] && [ -f "$p" ] || continue
    dpkg -S "$(readlink -f "$p")" >/dev/null 2>&1 && continue
    printf '%s\n' "$p"
    return 0
  done
  return 1
}
# Name of the release's tetron-wm_<version>-<rev>_<arch>.deb, or nothing. Read
# from the release page's asset list (not the rate-limited REST API), so a
# packaging-only revision (-2) is found without guessing.
ASSETS=""
# asset_of NAME ARCH: the release's NAME_<version>-<rev>_ARCH.deb, or nothing.
asset_of() {
  [ -n "$ASSETS" ] || ASSETS="$(fetch "$RELEASES/expanded_assets/$tag" 2>/dev/null || true)"
  printf '%s' "$ASSETS" | grep -o "${1}_[^\"/<> ]*_$2\.deb" | head -1 || true
}
deb_asset() { asset_of "$PKG" "$1"; }
# The size debs of this release up to the largest one installed here, so they
# move to the release with tetron-wm (a new Recommends then arrives with the
# update) and a larger size finds the smaller ones it depends on.
meta_assets() {
  top=""
  for m in tetron-wm-desktop tetron-wm-essentials tetron-wm-recommended tetron-wm-extras; do
    dpkg-query -W -f='${db:Status-Status}' "$m" 2>/dev/null | grep -qx installed && top="$m"
  done
  [ -n "$top" ] || return 0
  for m in tetron-wm-desktop tetron-wm-essentials tetron-wm-recommended tetron-wm-extras; do
    a="$(asset_of "$m" all)"
    [ -n "$a" ] && printf '%s ' "$a"
    [ "$m" = "$top" ] && break
  done
  return 0
}
# Download each .deb and its .sha256, check them all, then install them in one
# apt-get command. Any failure changes nothing.
install_deb() {
  tmp="$(mktemp -d)"
  files=""
  for asset in "$@"; do
    echo "tetron-wm: downloading $asset ($tag)…"
    if ! fetch "$RELEASES/download/$tag/$asset" > "$tmp/$asset" \
      || ! fetch "$RELEASES/download/$tag/$asset.sha256" > "$tmp/$asset.sha256"; then
      echo "tetron-wm: download of $asset failed; nothing changed." >&2
      rm -rf "$tmp"; exit 1
    fi
    want="$(cut -d' ' -f1 < "$tmp/$asset.sha256")"
    got="$(sha256sum "$tmp/$asset" | cut -d' ' -f1)"
    if [ -z "$want" ] || [ "$want" != "$got" ]; then
      echo "tetron-wm: checksum mismatch for $asset; nothing changed." >&2
      rm -rf "$tmp"; exit 1
    fi
    files="$files $tmp/$asset"
  done
  apt_sudo=""
  [ "$(id -u)" -eq 0 ] || apt_sudo="sudo"
  echo "tetron-wm: installing$(for a in "$@"; do printf ' %s' "$a"; done) with apt-get…"
  # shellcheck disable=SC2086
  if ! $apt_sudo env DEBIAN_FRONTEND=noninteractive apt-get install -y $files; then
    echo "tetron-wm: apt-get could not install them." >&2
    rm -rf "$tmp"; exit 1
  fi
  rm -rf "$tmp"
  BIN_DIR="/usr/bin"
  echo "tetron-wm: installed $tag -> /usr/bin/tetron-wm (package $PKG, managed by apt)"
}

method="raw"
note=""
if deb_host; then
  debarch="$(dpkg --print-architecture)"
  if pkg_installed; then
    asset="$(deb_asset "$debarch")"
    if [ -z "$asset" ]; then
      echo "tetron-wm: installed by apt, and release $tag has no .deb for $debarch." >&2
      echo "  Nothing changed. See $RELEASES/tag/$tag" >&2
      exit 3
    fi
    method="deb"
  elif raw="$(raw_copy)"; then
    note="tetron-wm: $raw is not from a package, so it was upgraded in place. To let apt manage tetron-wm, delete it and run this script again."
  else
    asset="$(deb_asset "$debarch")"
    [ -n "$asset" ] && method="deb"
  fi
fi

if [ "$method" = deb ]; then
  # shellcheck disable=SC2046
  install_deb "$asset" $(meta_assets)
else
  raw_sudo
  url="$RELEASES/download/$tag/tetron-wm-$target.tar.gz"
  echo "tetron-wm: downloading $tag ($target)…"
  $SUDO mkdir -p "$BIN_DIR"
  if ! fetch "$url" | $SUDO tar -xz -C "$BIN_DIR" 2>/dev/null || [ ! -f "$BIN_DIR/tetron-wm" ]; then
    echo "tetron-wm: no prebuilt binary for $target in $tag."
    echo "Install with Rust instead:  cargo install --git https://github.com/$REPO"
    exit 1
  fi
  $SUDO chmod +x "$BIN_DIR/tetron-wm"
  echo "tetron-wm: installed $tag -> $BIN_DIR/tetron-wm"
  [ -z "$note" ] || echo "$note"
fi

# tetron-wm only: the apps its features use come with the suite script's sizes
# (minimal, essentials, recommended, extras), the same sets as the size debs. The suite script calls this with TETRON_WM_SKIP_DEPS=1
# and installs them itself, so the pointer is only for a direct run.
if [ "${TETRON_WM_SKIP_DEPS:-0}" != "1" ]; then
  echo "tetron-wm: for the apps its features use (the essentials size), run:"
  echo "  curl -fsSL https://raw.githubusercontent.com/$REPO/main/install-tetron-wm-suite.sh | bash"
  echo "  It finds tetron-wm already installed and only adds what is missing."
fi

case ":$PATH:" in
  *":$BIN_DIR:"*) echo "Run it with:  tetron-wm" ;;
  *) echo "Add $BIN_DIR to your PATH, then run:  tetron-wm"
     echo "  e.g.  echo 'export PATH=\"$BIN_DIR:\$PATH\"' >> ~/.zprofile" ;;
esac
