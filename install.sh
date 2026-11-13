#!/bin/sh
# tetron-wm installer — downloads the latest prebuilt binary for your platform.
#
#   curl -fsSL https://raw.githubusercontent.com/tetronos/tetron-wm/main/install.sh | sh
#
# The default install dir is /usr/local/bin (system-wide, already on PATH), which
# needs root — the script uses sudo when the dir is not writable. For a no-sudo,
# userspace install set TETRON_WM_BIN_DIR to a writable dir, e.g.:
#   curl -fsSL …/install.sh | TETRON_WM_BIN_DIR="$HOME/.local/bin" sh
#
# One install method per host. On Debian and Ubuntu (dpkg and apt-get present):
# - tetron-wm installed by apt: the release .deb upgrades it, through apt. When
#   the release has no .deb the script changes nothing and exits with status 3,
#   so it never writes over a file the package owns.
# - a raw copy (not from a package): upgraded in place, as before.
# - neither: the release .deb is installed when the release has one, else the
#   raw binary.
# Everywhere else (other Linux, macOS) the raw binary, as before.
#
# Choose the method (WMINST-013):
#   TETRON_WM_METHOD=deb      the apt package; also adds the tetronos apt repo
#                             (package tetronos-apt-source) so apt upgrade
#                             updates it. TETRON_WM_NO_APT_REPO=1 skips the repo.
#   TETRON_WM_METHOD=binary   a binary. Where the package is installed it stays,
#                             its binary is diverted to /usr/bin/tetron-wm.packaged
#                             and this one goes to /usr/bin/tetron-wm.
#   TETRON_WM_FROM_CLONE=DIR  an own build: cargo build --release in DIR
#                             (implies binary).
# Without a method on a host with an own build nothing changes (exit 4).
# Exit status: 1 failed, 2 bad choice, 3 package without a release .deb,
# 4 own build kept.
set -eu

# Prebuilt binaries come from the tetron-wm release repo. The binary is named
# `tetron-wm` and installs to its own paths, so it collides with nothing else.
REPO="tetronos/tetron-wm"
PKG="tetron-wm"
BIN_DIR="${TETRON_WM_BIN_DIR:-/usr/local/bin}"
# Release downloads and the tag can be overridden (tests serve a local release).
RELEASES="${TETRON_WM_RELEASES_URL:-https://github.com/$REPO/releases}"
METHOD="${TETRON_WM_METHOD:-}"
CLONE="${TETRON_WM_FROM_CLONE:-}"
# The tetronos apt repository and its archive key, pinned by SHA256: apt then
# checks the repository with it (tetronos-apt-source ships the same file).
APT_URL="${TETRON_WM_APT_URL:-https://tetronos.github.io/apt}"
APT_KEY_SHA256="ca45c573c4c4c2ad96492d463a8d9cea213efea9e5ea1e70c0f97db7bebd4295"
WM=/usr/bin/tetron-wm
DIVERTED_TO=/usr/bin/tetron-wm.packaged

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
      echo "  Retry with:  curl -fsSL …/install.sh | TETRON_WM_BIN_DIR=\"\$HOME/.local/bin\" sh" >&2
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

# Debian and Ubuntu: dpkg and apt-get present.
deb_host() {
  [ "$os" = Linux ] && command -v dpkg >/dev/null 2>&1 && command -v apt-get >/dev/null 2>&1
}
# Root for system paths: nothing when root, else sudo.
ROOT=""
[ "$(id -u)" -eq 0 ] || ROOT="sudo"

[ -z "$CLONE" ] || [ -n "$METHOD" ] || METHOD=binary
case "$METHOD" in
  ""|deb|binary) ;;
  *) echo "tetron-wm: TETRON_WM_METHOD must be deb or binary, not '$METHOD'." >&2; exit 2 ;;
esac
if [ -n "$CLONE" ] && [ "$METHOD" = deb ]; then
  echo "tetron-wm: TETRON_WM_FROM_CLONE builds a binary; it does not go with TETRON_WM_METHOD=deb." >&2; exit 2
fi
if [ "$METHOD" = deb ] && ! deb_host; then
  echo "tetron-wm: the deb method needs Debian or Ubuntu (dpkg and apt-get)." >&2; exit 2
fi

# The tetron-wm package has its binary diverted here (an own build or a
# release binary in its place).
diverted() {
  deb_host && [ "$(dpkg-divert --truename "$WM" 2>/dev/null || true)" = "$DIVERTED_TO" ]
}
if [ -z "$METHOD" ] && diverted; then
  echo "tetron-wm: $WM is an own build (the package's binary is diverted to $DIVERTED_TO). Nothing changed." >&2
  echo "  Rebuild:          TETRON_WM_FROM_CLONE=~/code/tetron-wm sh install.sh" >&2
  echo "  Release binary:   TETRON_WM_METHOD=binary sh install.sh" >&2
  echo "  Back to apt:      TETRON_WM_METHOD=deb sh install.sh" >&2
  exit 4
fi

# A live session (a live medium, changes in RAM on an overlay or tmpfs root,
# as in packaging.rs live_free_bytes) loses every change at reboot.
for d in /run/live/medium /run/initramfs/live /cdrom/casper /rofs; do
  [ -e "$d" ] || continue
  case "$(awk '$2=="/"{print $3}' /proc/mounts 2>/dev/null | tail -1)" in
    overlay|tmpfs) echo "tetron-wm: WARNING: this is a live session; this install is lost at reboot." >&2 ;;
  esac
  break
done

# Own build: compile the clone first, so a failed build changes nothing.
BUILT=""
if [ -n "$CLONE" ]; then
  [ -f "$CLONE/Cargo.toml" ] || { echo "tetron-wm: $CLONE is not a tetron-wm clone (no Cargo.toml)." >&2; exit 1; }
  command -v cargo >/dev/null 2>&1 || [ -x "$HOME/.cargo/bin/cargo" ] || {
    echo "tetron-wm: cargo is needed for an own build. Install Rust with rustup: https://rustup.rs" >&2; exit 1; }
  cargo="$(command -v cargo 2>/dev/null || echo "$HOME/.cargo/bin/cargo")"
  echo "tetron-wm: building $CLONE (cargo build --release)…"
  if ! (cd "$CLONE" && "$cargo" build --release) || ! "$CLONE/target/release/tetron-wm" --version >/dev/null 2>&1; then
    echo "tetron-wm: the build failed; nothing changed." >&2; exit 1
  fi
  BUILT="$CLONE/target/release/tetron-wm"
fi

tag="${TETRON_WM_TAG:-}"
if [ -z "$tag" ] && [ -z "$BUILT" ]; then
  echo "tetron-wm: finding latest release…"
  tag="$(latest_tag || true)"
fi
if [ -z "${tag:-}" ] && [ -z "$BUILT" ]; then
  echo "tetron-wm: couldn't resolve the latest release (GitHub unreachable or rate-limited)."
  echo "Install with Rust instead:  cargo install --git https://github.com/$REPO"
  exit 1
fi

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
  if ! $apt_sudo env DEBIAN_FRONTEND=noninteractive apt-get install -y $REINSTALL $files; then
    echo "tetron-wm: apt-get could not install them." >&2
    rm -rf "$tmp"; exit 1
  fi
  rm -rf "$tmp"
  BIN_DIR="/usr/bin"
  echo "tetron-wm: installed $tag -> /usr/bin/tetron-wm (package $PKG, managed by apt)"
}

# ── deb method helpers (WMINST-013) ───────────────────────────────────────────
# The tetronos apt repository: installs tetronos-apt-source, which owns the
# sources file and the key, so this script never writes them. Best effort: a
# failure is a warning and the release .deb still installs.
apt_source_installed() {
  dpkg-query -W -f='${db:Status-Status}' tetronos-apt-source 2>/dev/null | grep -qx installed
}
add_apt_repo() {
  apt_source_installed && return 0
  [ "${TETRON_WM_NO_APT_REPO:-0}" = 1 ] && return 0
  [ -z "${TETRON_WM_RELEASES_URL:-}" ] || return 0     # a local release (tests)
  other="$(grep -lsF "${APT_URL#*://}" /etc/apt/sources.list /etc/apt/sources.list.d/* 2>/dev/null | grep -v '/tetronos\.sources$' | head -1 || true)"
  if [ -n "$other" ]; then
    echo "tetron-wm: the tetronos apt repo is already in $other; not adding it again."
    return 0
  fi
  echo "tetron-wm: adding the tetronos apt repo (package tetronos-apt-source), so apt upgrade updates tetron-wm…"
  t="$(mktemp -d)"; chmod 755 "$t"; mkdir -p "$t/parts" "$t/lists/partial"
  if ! fetch "$APT_URL/tetronos-archive-keyring.gpg" > "$t/key.gpg" \
    || [ "$(sha256sum < "$t/key.gpg" | cut -d' ' -f1)" != "$APT_KEY_SHA256" ]; then
    echo "tetron-wm: WARNING: could not get the tetronos archive key, or it does not match the pinned one; the apt repo was not added." >&2
    rm -rf "$t"; return 0
  fi
  chmod 644 "$t/key.gpg"
  echo "deb [signed-by=$t/key.gpg] $APT_URL stable main" > "$t/tetronos.list"
  o="-o Dir::Etc::SourceList=$t/tetronos.list -o Dir::Etc::SourceParts=$t/parts -o Dir::State::Lists=$t/lists"
  # shellcheck disable=SC2086
  if $ROOT apt-get $o update -qq \
    && $ROOT env DEBIAN_FRONTEND=noninteractive apt-get $o install -y -o Dpkg::Options::=--force-confold tetronos-apt-source; then
    echo "tetron-wm: apt repo added (/etc/apt/sources.list.d/tetronos.sources)."
  else
    echo "tetron-wm: WARNING: apt could not add the tetronos apt repo; tetron-wm updates come from this script and Settings > Updates." >&2
  fi
  $ROOT rm -rf "$t"
}
# Put the packaged binary back: drop ours and the diversion (dpkg renames
# the packaged file back), reinstalling the package when that file is gone.
undivert() {
  diverted || return 0
  echo "tetron-wm: removing the own binary and the diversion; the packaged tetron-wm comes back."
  [ -f "$DIVERTED_TO" ] || REINSTALL="--reinstall"
  $ROOT rm -f "$WM"
  $ROOT dpkg-divert --local --rename --remove "$WM" >/dev/null
}
REINSTALL=""
# Point user units that name OLD at the packaged binary.
retarget_units() {
  for u in "${XDG_CONFIG_HOME:-$HOME/.config}"/systemd/user/tetron-wm*.service; do
    [ -f "$u" ] && grep -qF "$1" "$u" || continue
    sed -i "s|$1|$WM|g" "$u" && echo "tetron-wm: $u now runs $WM"
    command -v systemctl >/dev/null 2>&1 && systemctl --user daemon-reload 2>/dev/null || true
  done
}

# ── binary method: put a new binary in place ──────────────────────────────────
# Copy beside the target, then rename over it, so a running tetron-wm is never
# written into. $1 = new binary, $2 = target path, $3 = command prefix (sudo).
place() {
  $3 mkdir -p "$(dirname "$2")"
  $3 cp "$1" "$2.new.$$" && $3 chmod 755 "$2.new.$$" && $3 mv -f "$2.new.$$" "$2"
}
# Where the package is installed it stays: divert its binary, ours goes to
# /usr/bin/tetron-wm (metapackages, units and autostart keep working).
place_diverted() {
  if ! diverted; then
    echo "tetron-wm: keeping the tetron-wm package; its binary moves to $DIVERTED_TO (dpkg-divert)."
    $ROOT dpkg-divert --local --rename --divert "$DIVERTED_TO" --add "$WM" >/dev/null
  fi
  place "$1" "$WM" "$ROOT"
  BIN_DIR=/usr/bin
  echo "tetron-wm: installed $2 -> $WM (own binary; apt upgrades of the package go to $DIVERTED_TO)"
  echo "  Back to the package:  TETRON_WM_METHOD=deb sh install.sh"
}

method="raw"
note=""
raw=""
if [ "$METHOD" = deb ]; then
  method="deb"
elif [ "$METHOD" = binary ]; then
  method="raw"
  if deb_host && pkg_installed; then method="divert"; fi
fi
if deb_host && [ -z "$BUILT" ]; then
  debarch="$(dpkg --print-architecture)"
  if [ "$METHOD" = deb ]; then
    asset="$(deb_asset "$debarch")"
  elif [ -n "$METHOD" ]; then
    :
  elif pkg_installed; then
    asset="$(deb_asset "$debarch")"
    if [ -z "$asset" ]; then
      echo "tetron-wm: installed by apt, and release $tag has no .deb for $debarch." >&2
      echo "  Nothing changed. See $RELEASES/tag/$tag" >&2
      exit 3
    fi
    method="deb"
  elif raw="$(raw_copy)"; then
    note="tetron-wm: $raw is not from a package, so it was upgraded in place. To let apt manage tetron-wm instead: TETRON_WM_METHOD=deb sh install.sh"
  else
    asset="$(deb_asset "$debarch")"
    [ -n "$asset" ] && method="deb"
  fi
fi

if [ "$method" = deb ]; then
  if [ -z "${asset:-}" ]; then
    if pkg_installed || diverted; then
      echo "tetron-wm: release $tag has no .deb for $debarch; putting the installed package back." >&2
    else
      echo "tetron-wm: release $tag has no .deb for $debarch; nothing changed. See $RELEASES/tag/$tag" >&2
      exit 3
    fi
  fi
  undivert
  old=""; [ "$METHOD" = deb ] && old="$(raw_copy || true)"
  add_apt_repo
  if [ -n "${asset:-}" ]; then
    # shellcheck disable=SC2046
    install_deb "$asset" $(meta_assets)
  elif [ -n "$REINSTALL" ]; then
    $ROOT apt-get update -qq || true
    $ROOT env DEBIAN_FRONTEND=noninteractive apt-get install -y --reinstall "$PKG"
  fi
  if [ -n "$old" ]; then
    s=""; [ -w "$(dirname "$old")" ] || s="$ROOT"
    echo "tetron-wm: removed $old (not from a package; the apt package replaces it)"
    $s rm -f "$old"
    retarget_units "$old"
  fi
  BIN_DIR=/usr/bin
else
  tmpb="$(mktemp -d)"
  if [ -n "$BUILT" ]; then
    cp "$BUILT" "$tmpb/tetron-wm"
    what="own build of $CLONE ($("$BUILT" --version 2>/dev/null | head -1))"
  else
    url="$RELEASES/download/$tag/tetron-wm-$target.tar.gz"
    echo "tetron-wm: downloading $tag ($target)…"
    if ! fetch "$url" | tar -xz -C "$tmpb" 2>/dev/null || [ ! -f "$tmpb/tetron-wm" ]; then
      echo "tetron-wm: no prebuilt binary for $target in $tag."
      echo "Install with Rust instead:  cargo install --git https://github.com/$REPO"
      rm -rf "$tmpb"; exit 1
    fi
    what="$tag"
  fi
  if [ "$method" = divert ]; then
    place_diverted "$tmpb/tetron-wm" "$what"
  else
    # A raw copy is replaced where it is (user units keep naming it).
    [ -n "$raw" ] || raw="$( { deb_host && raw_copy; } || true)"
    if [ -n "$raw" ] && [ -z "${TETRON_WM_BIN_DIR:-}" ]; then BIN_DIR="$(dirname "$raw")"; fi
    raw_sudo
    place "$tmpb/tetron-wm" "$BIN_DIR/tetron-wm" "$SUDO"
    echo "tetron-wm: installed $what -> $BIN_DIR/tetron-wm"
    [ -z "$note" ] || echo "$note"
  fi
  rm -rf "$tmpb"
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
