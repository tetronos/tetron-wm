#!/usr/bin/env bash
# install-tetron-wm-suite.sh — one script to stand up (or remove) a tetron-wm desktop.
#
# Runs the individual installers for you and, optionally, turns a bare Linux box
# into a console that boots straight into the desktop. Every step is opt-in and
# prompted (or chosen with flags); nothing happens without a yes. Pipe it:
#
#   curl -fsSL https://tetronos.github.io/tetron-wm/install.sh | bash
#
# Common setups (pick one from the menu, or pass --profile):
#   gui        a machine with a desktop: tetron-wm in a terminal window.
#              Installs the binary + config only.
#   console    a VC tty with kmscon (truecolor + mouse) and a normal login
#              prompt. You log in and run `tetron-wm` yourself.
#   login      the same, and tetron-wm starts after you log in (this is the
#              default on a console-only box). Both then ask "Do you want to
#              log in automatically?" (default no).
#   appliance  not in the menu, for scripts: kmscon, autologin with no password
#              and tetron-wm at boot (autologin + autostart). Never preselected:
#              anyone at the keyboard gets your account.
#   custom     answer each question yourself.
#
# The pieces (all idempotent: safe to re-run, it converges, never duplicates):
#   1. tetron-wm      — the latest prebuilt binary (this is install.sh)         [all OSes]
#   2. config dir     — ~/.config/tetron-wm/{config.toml,themes/}               [all OSes]
#      themes         — every theme in the repository's themes/ with its picture,
#                       into ~/.local/share/tetron-wm/themes/; never overwrites
#      size           — which apps come with it (asked of everyone):           [Linux, macOS]
#                         minimal      tetron-wm only
#                         essentials   plus the tools its features use and one
#                                      good app per need (default): chafa, mpv,
#                                      htop, mc, vim, git, tmux, ripgrep, ... and
#                                      fresh, uv, catt, casttui, a Nerd Font, opencode
#                         recommended  plus a curated pick in every category
#                         extras       plus the extra apps
#                       The list is assets/suite-apps.txt, generated from the
#                       "App sets" section of lists/tetron.toml (the same sets as
#                       the tetron-wm-desktop, -essentials, -recommended and
#                       -extras debs). On Debian and Ubuntu, when tetron-wm is
#                       the apt package, those debs are installed so apt tracks
#                       the set; otherwise the apps one by one.
#      services       — PipeWire audio (volume, outputs), NetworkManager (Wi-Fi),
#                       BlueZ (Bluetooth). Default NO: they change how the
#                       machine manages sound, network and Bluetooth
#                       (docs/DEPENDENCIES_tetron-wm_recommended_installs.md)
#   3. kmscon         — the truecolor + mouse console for a bare VT             [Linux]
#   4. console        — run kmscon on a VT in place of the kernel getty         [Linux + systemd]
#      autologin      — that VT logs your user in without a password prompt
#      autostart      — logging in on a kmscon console launches tetron-wm
#
# The console/autologin/autostart steps edit your login console: they are off
# unless you choose them, always confirmed, and `--uninstall` reverses them.
#
# Usage:  install-tetron-wm-suite.sh [options]
#   -y, --yes        assume the default answer to every prompt (non-interactive).
#                    Defaults: wm=yes, config=yes, kmscon=yes on a console box
#                    (no on a GUI box), console/autologin/autostart=NO unless
#                    selected by a flag or --profile.
#   --profile P      gui | console | login | appliance (see above)
#   --all            wm + config + kmscon + console + autologin + autostart (=appliance)
#   --wm / --no-wm             force the tetron-wm step on/off
#   --config / --no-config     force the config-dir step on/off
#   --themes / --no-themes     force the extra-themes download on/off (default yes)
#   --size S                   minimal | essentials | recommended | extras (default essentials;
#                              asked when not given; desktop is read as essentials)
#   --apps-list FILE           read the app list from FILE (Settings > General > Apps hands
#                              over the list it counted from); default: the list published
#                              for this core minor, then the repository's
#   --no-extras                same as --size minimal (older flag)
#   --no-scripts               leave out the apps no distribution packages (opencode, fresh,
#                              uv, catt, casttui, the Nerd Font, the helper scripts; with
#                              recommended also croft, pyradio, llm, yt-dlp, freebuff)
#   --method M                 deb | binary: how tetron-wm is installed (asked on Debian and
#                              Ubuntu, default the current method). deb: the apt package, plus
#                              the tetronos apt repo so apt upgrade updates it. binary: a
#                              release binary; where the package is installed it stays and its
#                              binary is diverted (dpkg-divert), so the size packages, units
#                              and autostart keep working
#   --from-clone DIR           an own build: cargo build --release in a tetron-wm clone DIR,
#                              installed like binary (implies --method binary)
#   --convert                  same as --method deb (older flag)
#   --services / --no-services force the system-services step on/off
#   --kmscon / --no-kmscon     force the kmscon step on/off
#   --console / --no-console   run kmscon on a VT (replaces getty there)
#   --vt N                     which VT (default 1, the console that opens at boot)
#   --fullscreen / --no-fullscreen   install the full screen video helper: a service as your
#                              user on a spare VT that plays videos full screen from kmscon
#                              (no sudo at play time; default no)
#   --fullscreen-vt N          the spare VT for it (default 9; it must not be a VT you use)
#   --autologin / --no-autologin   log in on that VT without a prompt (implies --console)
#   --autostart / --no-autostart   launch tetron-wm on login at that VT only (--vt)
#   --tty1                     shorthand for the full appliance on tty1 (older flag)
#   --uninstall      undo the console / autologin / autostart changes and the
#                    apphost service (prompted; keeps the binary, config, kmscon)
#   --dry-run        print the plan the choices add up to, change nothing
#   --purge          with --uninstall: also remove the binary and ~/.config/tetron-wm
#   -h, --help       show this help
#
# Env overrides: REPO (default tetronos/tetron-wm), BRANCH (default main),
# TETRON_WM_LISTS_URL (default https://tetronos.github.io/apt/lists),
# TETRON_WM_METHOD and TETRON_WM_FROM_CLONE (as --method and --from-clone),
# and anything install.sh honours (TETRON_WM_BIN_DIR, TETRON_WM_RELEASES_URL, TETRON_WM_TAG,
# TETRON_WM_NO_APT_REPO).
set -euo pipefail

REPO="${REPO:-tetronos/tetron-wm}"
BRANCH="${BRANCH:-main}"
RAW="https://raw.githubusercontent.com/$REPO/$BRANCH"
# The app list published per core minor (WMCAT-004), so a list made for a newer
# core never reaches this one. LISTS_MINOR follows Cargo.toml (a test checks).
LISTS_MINOR="0.15"
LISTS="${TETRON_WM_LISTS_URL:-https://tetronos.github.io/apt/lists}/$LISTS_MINOR"
# Release downloads and the tag can be overridden (tests serve a local release);
# install.sh reads the same two variables.
RELEASES="${TETRON_WM_RELEASES_URL:-https://github.com/$REPO/releases}"
# Run from a checkout (not piped): use its files instead of downloading them.
HERE="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd || true)"
[ -n "$HERE" ] && [ -f "$HERE/install-tetron-wm-suite.sh" ] && [ -f "$HERE/assets/suite-apps.txt" ] || HERE=""

# ── Output helpers (match install-kmscon.sh) ────────────────────────────────────
if [ -t 1 ]; then B=$'\033[1m'; Y=$'\033[33m'; R=$'\033[31m'; G=$'\033[32m'; N=$'\033[0m'; else B= Y= R= G= N=; fi
log()  { printf '%s==>%s %s\n'  "$G" "$N" "$*"; }
warn() { printf '%sWARN:%s %s\n' "$Y" "$N" "$*" >&2; }
die()  { printf '%sERROR:%s %s\n' "$R" "$N" "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }
# A terminal to ask on. Not `[ -r /dev/tty ]`: in a container or CI job the
# node exists and is readable but opening it fails, which would abort a
# redirect from it before the command runs.
tty_ok() { { : </dev/tty; } 2>/dev/null; }
# A user tool: on PATH, or in ~/.local/bin, where uv and the upstream
# installers put it even when that directory is not on PATH yet.
# /usr/sbin and /sbin: a normal user's PATH lacks them on Debian and Ubuntu.
have_tool() { have "$1" || [ -x "$HOME/.local/bin/$1" ] || [ -x "/usr/sbin/$1" ] || [ -x "/sbin/$1" ]; }

usage() { sed -n '2,/^set -euo/p' "$0" | sed 's/^# \{0,1\}//; s/^#$//' | sed '$d'; exit "${1:-0}"; }

# Stock Debian often has wget but not curl; accept either.
fetch() {
  if have curl; then curl -fsSL "$1"
  elif have wget; then wget -qO- "$1"
  else die "need curl or wget to download"
  fi
}

# ── Args ─────────────────────────────────────────────────────────────────────
ASSUME_YES=0
SIZE=""; APPS_LIST=""; NO_SCRIPTS=0
METHOD="${TETRON_WM_METHOD:-}"; CLONE="${TETRON_WM_FROM_CLONE:-}"
WANT_WM=""; WANT_CONFIG=""; WANT_THEMES=""; WANT_EXTRAS=""; WANT_SERVICES=""; WANT_KMSCON=""; WANT_CONSOLE=""; WANT_AUTOLOGIN=""; WANT_AUTOSTART=""; WANT_FULLSCREEN=""
FSVT=9; VT=""; PROFILE=""; UNINSTALL=0; PURGE=0; DRY_RUN=0
while [ $# -gt 0 ]; do
  case "$1" in
    -y|--yes)     ASSUME_YES=1 ;;
    --profile)    shift; PROFILE="${1:-}" ;;
    --all)        PROFILE=appliance ;;
    --wm)         WANT_WM=y ;;     --no-wm)     WANT_WM=n ;;
    --config)     WANT_CONFIG=y ;; --no-config) WANT_CONFIG=n ;;
    --themes)     WANT_THEMES=y ;; --no-themes) WANT_THEMES=n ;;
    --extras)     WANT_EXTRAS=y ;; --no-extras) WANT_EXTRAS=n; SIZE=minimal ;;
    --size)       shift; SIZE="${1:-}" ;;
    --apps-list)  shift; APPS_LIST="${1:-}" ;;
    --no-scripts) NO_SCRIPTS=1 ;;
    --method)     shift; METHOD="${1:-}" ;;
    --from-clone) shift; CLONE="${1:-}" ;;
    --convert)    METHOD=deb ;; --no-convert) ;;
    --services)   WANT_SERVICES=y ;; --no-services) WANT_SERVICES=n ;;
    --kmscon)     WANT_KMSCON=y ;; --no-kmscon) WANT_KMSCON=n ;;
    --console)    WANT_CONSOLE=y ;;   --no-console)   WANT_CONSOLE=n ;;
    --autologin)  WANT_AUTOLOGIN=y ;; --no-autologin) WANT_AUTOLOGIN=n ;;
    --autostart)  WANT_AUTOSTART=y ;; --no-autostart) WANT_AUTOSTART=n ;;
    --fullscreen) WANT_FULLSCREEN=y ;; --no-fullscreen) WANT_FULLSCREEN=n ;;
    --fullscreen-vt) shift; FSVT="${1:-}" ;;
    --vt)         shift; VT="${1:-}" ;;
    --tty1)       WANT_CONSOLE=y; WANT_AUTOLOGIN=y; WANT_AUTOSTART=y; VT=1 ;;
    --no-tty1)    WANT_CONSOLE=n; WANT_AUTOLOGIN=n; WANT_AUTOSTART=n ;;
    --uninstall)  UNINSTALL=1 ;;
    --purge)      PURGE=1 ;;
    --dry-run)    DRY_RUN=1 ;;
    -h|--help)    usage 0 ;;
    *) die "unknown option: $1 (see --help)" ;;
  esac
  shift
done
case "$PROFILE" in ""|gui|console|login|appliance) ;; *) die "--profile must be gui, console, login or appliance" ;; esac
[ "$SIZE" = desktop ] && SIZE=essentials   # the size's name before recommended existed
case "$SIZE" in ""|minimal|essentials|recommended|extras) ;; *) die "--size must be minimal, essentials, recommended or extras" ;; esac
[ -z "$CLONE" ] || [ -n "$METHOD" ] || METHOD=binary
case "$METHOD" in ""|deb|binary) ;; *) die "--method must be deb or binary" ;; esac
[ -z "$CLONE" ] || [ "$METHOD" = binary ] || die "--from-clone builds a binary; it does not go with --method deb"
if [ -n "$CLONE" ]; then
  CLONE="$(cd "$CLONE" 2>/dev/null && pwd)" || die "--from-clone: no such directory"
  [ -f "$CLONE/Cargo.toml" ] || die "--from-clone: $CLONE is not a tetron-wm clone (no Cargo.toml)"
fi
case "$VT" in ""|[1-9]|[1-5][0-9]|6[0-3]) ;; *) die "--vt must be a VT number 1-63" ;; esac
case "$FSVT" in [1-9]|[1-5][0-9]|6[0-3]) ;; *) die "--fullscreen-vt must be a VT number 1-63" ;; esac

OS="$(uname -s)"
IS_LINUX=0; [ "$OS" = "Linux" ] && IS_LINUX=1

# ── Privilege: sudo only where a step needs it (kmscon pkgs, tty1 unit edits) ───
if [ "$(id -u)" -eq 0 ]; then SUDO=""; else
  if have sudo; then SUDO="sudo"; else SUDO=""; fi
fi

# yes/no prompt with a default; reads /dev/tty so it still works under curl|bash.
# $1 = question, $2 = default (y|n). Honors -y (take default) and no-tty (default).
confirm() {
  local q="$1" def="$2" ans
  if [ "$ASSUME_YES" = 1 ] || ! tty_ok; then
    [ "$def" = y ]; return
  fi
  if [ "$def" = y ]; then printf '%s%s [Y/n] %s' "$B" "$q" "$N" >&2
  else                    printf '%s%s [y/N] %s' "$B" "$q" "$N" >&2; fi
  read -r ans </dev/tty || ans=""
  ans="${ans:-$def}"
  case "$ans" in y|Y|yes|YES) return 0 ;; *) return 1 ;; esac
}

# Resolve a WANT_ flag: keep a value set on the command line, else ask (or take
# the default under -y / no tty). $1 = var name, $2 = question, $3 = default.
resolve() {
  local cur; eval "cur=\${$1}"
  [ -n "$cur" ] && return 0
  if confirm "$2" "$3"; then eval "$1=y"; else eval "$1=n"; fi
}

# ── Step 1: tetron-wm ──────────────────────────────────────────────────────────
install_wm() {
  log "installing tetron-wm (latest prebuilt release)…"
  local tmp; tmp="$(mktemp)"
  if [ -n "$HERE" ]; then cp "$HERE/install.sh" "$tmp"
  else fetch "$RAW/install.sh" > "$tmp" || die "could not download install.sh"; fi
  # The apps come from the size chosen here (WMINST-002), so install.sh only
  # installs tetron-wm itself. It picks the deb or the raw binary (WMDEB-005).
  export TETRON_WM_SKIP_DEPS=1
  export TETRON_WM_METHOD="$METHOD" TETRON_WM_FROM_CLONE="$CLONE"
  local rc=0
  if tty_ok; then sh "$tmp" </dev/tty || rc=$?; else sh "$tmp" </dev/null || rc=$?; fi
  rm -f "$tmp"
  case "$rc" in
    0) ;;
    3) warn "install.sh changed nothing: tetron-wm is the apt package and the latest release has no .deb" ;;
    4) log "kept the own build of tetron-wm (choose --method to replace it)" ;;
    *) warn "install.sh failed (status $rc); tetron-wm was not installed or updated" ;;
  esac
}

# ── Debian and Ubuntu: the release debs (WMINST-002) ───────────────────────────
deb_host() { [ "$IS_LINUX" = 1 ] && have dpkg && have apt-get; }
# tetron-wm is the apt package (dpkg owns it).
wm_packaged() { dpkg-query -W -f='${db:Status-Status}' tetron-wm 2>/dev/null | grep -qx installed; }
# A tetron-wm on PATH that no package owns, or nothing.
wm_raw_copy() {
  local p; p="$(command -v tetron-wm 2>/dev/null)" || return 1
  have dpkg && dpkg -S "$(readlink -f "$p")" >/dev/null 2>&1 && return 1
  printf '%s\n' "$p"
}
RELEASE_TAG=""; RELEASE_ASSETS=""
# The latest release tag, through the web redirect (not the rate-limited API).
release_tag() {
  if [ -z "$RELEASE_TAG" ]; then
    RELEASE_TAG="${TETRON_WM_TAG:-}"
    [ -n "$RELEASE_TAG" ] || RELEASE_TAG="$(curl -fsSI "https://github.com/$REPO/releases/latest" 2>/dev/null \
      | tr -d '\r' | awk 'tolower($1)=="location:"{print $2}' | tail -1 | sed 's|.*/releases/tag/||')"
  fi
  printf '%s' "$RELEASE_TAG"
}
# asset_for PACKAGE ARCH: the release's PACKAGE_<version>-<rev>_ARCH.deb, or nothing.
asset_for() {
  local tag; tag="$(release_tag)"; [ -n "$tag" ] || return 0
  [ -n "$RELEASE_ASSETS" ] || RELEASE_ASSETS="$(fetch "$RELEASES/expanded_assets/$tag" 2>/dev/null || true)"
  printf '%s' "$RELEASE_ASSETS" | grep -o "${1}_[^\"/<> ]*_${2}\.deb" | head -1 || true
}
# The size's metapackages: each size adds its own to the smaller sizes'.
size_debs() {
  case "$SIZE" in
    essentials) echo "tetron-wm-desktop tetron-wm-essentials" ;;
    recommended) echo "tetron-wm-desktop tetron-wm-essentials tetron-wm-recommended" ;;
    extras) echo "tetron-wm-desktop tetron-wm-essentials tetron-wm-recommended tetron-wm-extras" ;;
  esac
}
# Every metapackage of the size is in the release (Architecture all).
release_has_size_debs() {
  local p; [ -n "$(size_debs)" ] || return 1
  for p in $(size_debs); do [ -n "$(asset_for "$p" all)" ] || return 1; done
}
APT_UPDATED=0
FAILED=""
apt_update_once() { [ "$APT_UPDATED" = 1 ] || { $SUDO apt-get update -qq || true; APT_UPDATED=1; }; }
# Download the size's metapackages, check each against its .sha256, install them
# with apt (Recommends included, so apt tracks the whole set). One already
# installed at the release's version is skipped, so a rerun converges.
install_size_debs() {
  local tag p a v tmp files="" want got; tag="$(release_tag)"; tmp="$(mktemp -d)"
  for p in $(size_debs); do
    a="$(asset_for "$p" all)"
    v="${a#"${p}_"}"; v="${v%_all.deb}"
    if [ "$(dpkg-query -W -f='${Version}' "$p" 2>/dev/null || true)" = "$v" ]; then
      log "$p $v is already installed"; continue
    fi
    log "downloading $a ($tag)"
    if ! fetch "$RELEASES/download/$tag/$a" > "$tmp/$a" || ! fetch "$RELEASES/download/$tag/$a.sha256" > "$tmp/$a.sha256"; then
      warn "could not download $a; installing the apps one by one instead"; rm -rf "$tmp"; return 1
    fi
    want="$(cut -d' ' -f1 < "$tmp/$a.sha256")"; got="$(sha256sum "$tmp/$a" | cut -d' ' -f1)"
    if [ -z "$want" ] || [ "$want" != "$got" ]; then
      warn "checksum mismatch for $a; installing the apps one by one instead"; rm -rf "$tmp"; return 1
    fi
    files="$files $tmp/$a"
  done
  [ -n "$files" ] || { rm -rf "$tmp"; return 0; }
  apt_update_once
  # "gpm-" keeps apt from installing that Recommends (no mouse device).
  local skip=""; no_mouse && ! dpkg -s gpm >/dev/null 2>&1 && skip="gpm-"
  log "installing$files with apt${skip:+ (without gpm: no mouse device)}"
  # shellcheck disable=SC2086
  # Recommends forced on: the set debs are only Recommends, and a system may turn
  # them off (TetronOS images ship APT::Install-Recommends "false").
  $SUDO env DEBIAN_FRONTEND=noninteractive apt-get install -y -o APT::Install-Recommends=true $files $skip \
    || { rm -rf "$tmp"; warn "apt could not install the $SIZE packages"; return 1; }
  rm -rf "$tmp"
}
# One name per Recommends group of the installed size debs that apt has not
# installed: the first alternative with a candidate here. gpm is left out
# without a mouse device. apt never adds a Recommends of a package that is
# already installed, so a rerun after an install without Recommends needs this.
size_recommends_missing() {
  local p rel grp alt pick
  for p in $(size_debs); do
    rel="$(dpkg-query -W -f='${Recommends}' "$p" 2>/dev/null)" || continue
    printf '%s\n' "$rel" | tr ',' '\n'
  done | while IFS= read -r grp; do
    [ -n "$(echo "$grp" | tr -d ' ')" ] || continue
    pick=""
    for alt in $(echo "$grp" | sed 's/([^)]*)//g; s/|/ /g'); do
      dpkg-query -W -f='${Status}' "$alt" 2>/dev/null | grep -q "ok installed" && { pick=""; break; }
      [ -z "$pick" ] && [ -n "$(apt-cache policy "$alt" 2>/dev/null | sed -n 's/^ *Candidate: //p' | grep -v '(none)')" ] && pick="$alt"
    done
    [ "$pick" = gpm ] && no_mouse && pick=""
    [ -n "$pick" ] && echo "$pick"
  done | sort -u
}
# Install what size_recommends_missing names, marked automatic as apt would
# have done, so Remove set takes them away again. Fails when any stays missing.
install_size_recommends() {
  local miss; miss="$(size_recommends_missing | tr '\n' ' ')"
  [ -n "${miss% }" ] || return 0
  log "installing the $SIZE apps apt left out: $miss"
  # shellcheck disable=SC2086
  $SUDO env DEBIAN_FRONTEND=noninteractive apt-get install -y $miss && $SUDO apt-mark auto $miss >/dev/null
  miss="$(size_recommends_missing | tr '\n' ' ')"
  [ -z "${miss% }" ] || { warn "these $SIZE apps did not install: $miss"; return 1; }
}
# The package's binary is diverted: tetron-wm here is an own build or a
# release binary in its place (WMINST-013).
wm_diverted() { deb_host && [ "$(dpkg-divert --truename /usr/bin/tetron-wm 2>/dev/null || true)" = /usr/bin/tetron-wm.packaged ]; }

# Where the binary landed, for later steps (install.sh default: ~/.local/bin).
wm_bin() { command -v tetron-wm 2>/dev/null || echo "${TETRON_WM_BIN_DIR:-/usr/local/bin}/tetron-wm"; }

# ── Step 2: config dir ─────────────────────────────────────────────────────────
seed_config() {
  local cfg="$HOME/.config/tetron-wm"
  log "setting up $cfg (config.toml, themes/)…"
  mkdir -p "$cfg/themes"
  if [ -f "$cfg/config.toml" ]; then
    log "config.toml already exists — leaving it untouched"
  else
    # The example is every option at its default: a safe starter. tetron-wm
    # carries it (`tetron-wm config init`, WMDEB-002); older binaries do not.
    if "$(wm_bin)" config init >/dev/null 2>&1 && [ -s "$cfg/config.toml" ]; then
      log "wrote $cfg/config.toml (tetron-wm config init: every option at its default)"
    elif fetch "$RAW/config.example.toml" > "$cfg/config.toml" 2>/dev/null && [ -s "$cfg/config.toml" ]; then
      log "wrote $cfg/config.toml (all defaults, commented — edit to taste)"
    else
      rm -f "$cfg/config.toml"
      warn "could not fetch config.example.toml; run with no config (defaults apply) and add one later"
    fi
  fi
  log "drop images in $cfg/themes/ and set wallpaper=... ; editable themes: tetron-wm theme --dump <name>"
}

# ── Step 2a: the extra themes ──────────────────────────────────────────────────
# The themes/ directory on the branch is the list (as Settings > Appearance >
# Get more themes): one GitHub contents API request, then each .toml and picture
# not already here. Into ~/.local/share/tetron-wm/themes, tetron-wm's download
# directory; your own ~/.config/tetron-wm/themes is left to you. A file that
# exists in either is kept. If the listing fails (no network, or GitHub's limit
# of 60 requests an hour), the themes are skipped with a note: there is no
# second list to keep in step with the folder.
theme_files() {
  local names
  names="$(fetch "https://api.github.com/repos/$REPO/contents/themes?ref=$BRANCH" 2>/dev/null \
    | grep -o '"name": *"[^"]*"' | sed 's/.*"\([^"]*\)"$/\1/' \
    | grep -iE '^[^./][^/]*\.(toml|jpg|jpeg|png|webp)$' || true)"
  [ -z "$names" ] || printf '%s\n' "$names"
}
install_themes() {
  local dir="${XDG_DATA_HOME:-$HOME/.local/share}/tetron-wm/themes" own="$HOME/.config/tetron-wm/themes" f files got=0 kept=0
  files="$(theme_files)"
  if [ -z "$files" ]; then
    warn "could not list the themes on GitHub (no network, or its hourly limit); skipped. Get them later in Settings > Appearance > Get more themes."
    return 0
  fi
  mkdir -p "$dir"
  log "downloading the themes into $dir…"
  for f in $files; do
    if [ -e "$dir/$f" ] || [ -e "$own/$f" ]; then kept=$((kept + 1)); continue; fi
    if fetch "$RAW/themes/$f" > "$dir/$f.part" 2>/dev/null && [ -s "$dir/$f.part" ]; then
      mv "$dir/$f.part" "$dir/$f"; got=$((got + 1))
    else
      rm -f "$dir/$f.part"; warn "could not fetch themes/$f"
    fi
  done
  log "themes: $got downloaded, $kept already there. Pick one in Settings > Appearance (a running tetron-wm finds new files within a second)."
}

# kmscon supports truecolor but does not set COLORTERM, so tetron-wm would
# auto-downsample subtle themes (nord → teal/navy). Force truecolor on in
# config.toml for the kmscon appliance (top level, before any [table] header).
ensure_truecolor() {
  local f="$HOME/.config/tetron-wm/config.toml"
  if [ ! -f "$f" ]; then
    mkdir -p "$(dirname "$f")"; printf 'truecolor = true\n' > "$f"
    log "created config.toml with truecolor = true (kmscon)"; return 0
  fi
  grep -qE '^[[:space:]]*truecolor[[:space:]]*=' "$f" && return 0
  local tmp; tmp="$(mktemp)"
  awk 'BEGIN{ins=0} /^\[/ && ins==0 {print "truecolor = true"; ins=1} {print} END{if(ins==0) print "truecolor = true"}' "$f" > "$tmp" \
    && mv "$tmp" "$f" && log "set truecolor = true in config.toml (kmscon supports it; no COLORTERM)"
}

# ── Step 2b: the apps of the chosen size, and system services ─────────────────
# The apps are defined once, in lists/tetron.toml, and generated into
# assets/suite-apps.txt (WMINST-001): one "size:group:check:apt:dnf:pacman:brew"
# line per package (an empty name = not on that system) and "size:script:name"
# per app no distro packages. A size installs its lines and the smaller sizes'.
# Only packages whose check fails install, so re-running converges.
# Where the list comes from, first that works: --apps-list FILE, the checkout
# this script runs from, the list published for this core minor, the repository.
APPS=""
apps_list_ok() { grep '^[a-z]*:[a-z]*:' <<<"$1" >/dev/null; }
load_apps() {
  if [ -n "$APPS_LIST" ]; then
    APPS="$(cat "$APPS_LIST" 2>/dev/null)" || APPS=""
    apps_list_ok "$APPS" || { warn "no app list in $APPS_LIST"; APPS=""; }
  fi
  if [ -z "$APPS" ] && [ -n "$HERE" ]; then APPS="$(cat "$HERE/assets/suite-apps.txt")"; fi
  if [ -z "$APPS" ]; then
    APPS="$(fetch "$LISTS/suite-apps.txt" 2>/dev/null)" || APPS=""
    apps_list_ok "$APPS" || APPS=""
  fi
  if [ -z "$APPS" ]; then
    APPS="$(fetch "$RAW/assets/suite-apps.txt" 2>/dev/null)" || APPS=""
    apps_list_ok "$APPS" || APPS=""
  fi
  [ -n "$APPS" ] || warn "could not read the app list (assets/suite-apps.txt); no apps will be installed"
}
# desktop: the essentials lines of a list written before recommended existed.
size_rank() { case "$1" in minimal) echo 0 ;; essentials|desktop) echo 1 ;; recommended) echo 2 ;; extras) echo 3 ;; *) echo 9 ;; esac; }
# apps_for SIZE GROUP: the "check:apt:dnf:pacman:brew" lines (or script names)
# of GROUP for SIZE and the smaller sizes.
apps_for() {
  local max size group rest; max="$(size_rank "$1")"
  while IFS=: read -r size group rest; do
    case "$size" in ""|\#*) continue ;; esac
    [ "$group" = "$2" ] && [ "$(size_rank "$size")" -le "$max" ] && printf '%s\n' "$rest"
  done <<EOF_APPS
$APPS
EOF_APPS
  return 0
}
# Not `| grep -q`: it stops reading at the first match and, under pipefail,
# the writer's SIGPIPE would turn a match into a failure.
want_script() { [ "$NO_SCRIPTS" = 0 ] || return 1; local l; l="$(apps_for "$SIZE" script)"; printf '%s\n' "$l" | grep -x "$1" >/dev/null; }

# The package manager column to read: 2 apt, 3 dnf, 4 pacman, 5 brew.
pm_col() {
  if have apt-get; then echo 2; elif have dnf; then echo 3; elif have pacman; then echo 4
  elif have brew; then echo 5; else echo 0; fi
}

# Is a list entry's check satisfied: a command on PATH, or for "font=NAME"
# that font. fontconfig answers when present; without it (a minimal box),
# look for the DejaVu file in the usual distro font dirs.
present() {
  local alt
  case "$1" in *"|"*)
    for alt in $(printf '%s' "$1" | tr '|' ' '); do present "$alt" && return 0; done
    return 1 ;;
  esac
  case "$1" in
    consolefonts) ls /usr/share/consolefonts/* /usr/lib/kbd/consolefonts/* >/dev/null 2>&1 ;;
    font=*)
      if have fc-list; then fc-list "${1#font=}" 2>/dev/null | grep -q .
      else ls /usr/share/fonts/*/*/DejaVuSansMono.ttf /usr/share/fonts/*/DejaVuSansMono.ttf >/dev/null 2>&1; fi ;;
    *) have "$1" ;;
  esac
}

# Packages from a list (above) whose check fails, for this package manager.
missing_pkgs() {
  local col line cmd pkg out=""; col="$(pm_col)"
  [ "$col" = 0 ] && return 0
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    cmd="${line%%:*}"; pkg="$(printf '%s' "$line" | cut -d: -f"$col")"
    if [ -n "$pkg" ] && ! present "$cmd"; then out="$out $pkg"; fi
  done <<EOF_LIST
$1
EOF_LIST
  printf '%s' "${out# }"
}

# The names of $1 apt has an install candidate for (call after apt-get update).
# One unknown name makes apt-get refuse the whole batch, and tetron-browser
# exists only where the tetronos apt repo is configured (amd64).
apt_installable() {
  local p out=""
  for p in $1; do
    if apt-cache policy "$p" 2>/dev/null | grep 'Candidate: [^(]' >/dev/null; then out="$out $p"
    else log "skipping $p: apt has no package of that name here" >&2; fi
  done
  printf '%s' "${out# }"
}

# apt_first PKG: on an apt host where apt has a candidate for PKG (the tetronos
# apt repo is configured), install it with apt and succeed; else fail, so the
# caller uses the app's own method (WMINST-008).
apt_first() {
  [ "$(pm_col)" = 2 ] || return 1
  apt_update_once
  [ -n "$(apt_installable "$1" 2>/dev/null)" ] || return 1
  log "installing $1 with apt (tetronos apt repo)"
  $SUDO env DEBIAN_FRONTEND=noninteractive apt-get install -y "$1"
}

pm_install() {  # $1 = space-separated packages
  local pkgs="$1"
  [ "$(pm_col)" = 2 ] && { apt_update_once; pkgs="$(apt_installable "$pkgs")"; }
  [ -n "$pkgs" ] || return 0
  log "installing: $pkgs"
  # shellcheck disable=SC2086
  case "$(pm_col)" in
    2) $SUDO apt-get install -y $pkgs ;;
    3) $SUDO dnf install -y $pkgs ;;
    4) $SUDO pacman -S --needed --noconfirm $pkgs ;;
    5) brew install $pkgs ;;
  esac || warn "some packages did not install: $pkgs (see docs/DEPENDENCIES_tetron-wm_recommended_installs.md)"
}

# gpm needs a mouse device: without one (headless servers, many VMs) Debian's
# gpm fails to configure and leaves a broken package, so the installer skips
# it there. gpm is the mouse for the machine's own text console only.
no_mouse() { [ "$IS_LINUX" = 1 ] && [ ! -e /dev/input/mice ]; }

# Missing packages of the chosen size (gui ones only with a desktop session;
# gpm only with a mouse device).
size_list() {
  local l; l="$(apps_for "$SIZE" apps)"
  no_mouse && l="$(printf '%s\n' "$l" | grep -v '^gpm:' || true)"
  is_graphical && l="$l
$(apps_for "$SIZE" gui)"
  missing_pkgs "$l"
}
services_list() { missing_pkgs "$(apps_for essentials services)"; }

# fresh is in no Linux distro repository; its official installer picks the
# right method itself (.deb, .rpm, AUR, …). Offered with the extras when missing.
FRESH_INSTALLER="https://raw.githubusercontent.com/sinelaw/fresh/refs/heads/master/scripts/install.sh"
fresh_via_script() { [ "$IS_LINUX" = 1 ] && ! have_tool fresh; }
fresh_via_brew() { [ "$IS_LINUX" = 0 ] && have brew && ! have_tool fresh; }

install_fresh() {
  log "installing fresh with its official installer ($FRESH_INSTALLER)"
  local tmp; tmp="$(mktemp)"
  if fetch "$FRESH_INSTALLER" > "$tmp"; then
    sh "$tmp" || warn "the fresh installer failed; see https://github.com/sinelaw/fresh#installation"
  else
    warn "could not download the fresh installer; see https://github.com/sinelaw/fresh#installation"
  fi
  rm -f "$tmp"
}

# uv: the package manager column has it except on apt (and with no package
# manager); there its official installer puts it in ~/.local/bin.
UV_INSTALLER="https://astral.sh/uv/install.sh"
uv_bin() {
  if have uv; then echo uv; elif [ -x "$HOME/.local/bin/uv" ]; then echo "$HOME/.local/bin/uv"; fi
}
uv_via_script() { [ "$NO_SCRIPTS" = 0 ] && [ -z "$(uv_bin)" ] && { [ "$(pm_col)" = 2 ] || [ "$(pm_col)" = 0 ]; }; }

install_uv() {
  log "installing uv with its official installer ($UV_INSTALLER)"
  local tmp; tmp="$(mktemp)"
  if fetch "$UV_INSTALLER" > "$tmp"; then
    sh "$tmp" || warn "the uv installer failed; see https://docs.astral.sh/uv/"
  else
    warn "could not download the uv installer; see https://docs.astral.sh/uv/"
  fi
  rm -f "$tmp"
}

# Chromecast casting: catt (right-click Cast to Chromecast) and casttui (Open
# with… casttui). Python tools in no distro repository, installed with
# `uv tool install` into ~/.local/bin. Offered with the extras when missing.
# The archive, not git+https: that needs git, which a minimal host lacks.
CASTTUI_SRC="https://github.com/tetronos/casttui/archive/refs/heads/main.tar.gz"
cast_tools_missing() {
  local out=""
  have_tool catt || out="catt"
  have_tool casttui || out="${out:+$out }casttui"
  printf '%s' "$out"
}

# tetron-wm's own small apps from scripts/: Start Page (bookmarks start page,
# opened in carbonyl or w3m) and AI Mode (Gemini Q&A). Single stdlib Python 3
# files, fetched into ~/.local/bin; the launcher lists them once on PATH.
OWN_APPS="tetron-wm-startpage tetron-aimode"
own_apps_missing() {
  local a out=""
  for a in $OWN_APPS; do have "$a" || [ -x "$HOME/.local/bin/$a" ] || out="${out:+$out }$a"; done
  printf '%s' "$out"
}

install_own_apps() {
  have python3 || { warn "python3 is missing; tetron-wm-startpage and tetron-aimode not installed"; return 0; }
  local a d="$HOME/.local/bin"; mkdir -p "$d"
  for a in $(own_apps_missing); do
    log "installing $a into $d"
    if fetch "$RAW/scripts/$a" > "$d/$a.tmp" && [ -s "$d/$a.tmp" ]; then
      chmod +x "$d/$a.tmp" && mv "$d/$a.tmp" "$d/$a"
    else
      rm -f "$d/$a.tmp"; warn "could not download $a from $RAW/scripts/$a"
    fi
  done
  [ -x "$d/tetron-aimode" ] && [ -z "${GEMINI_API_KEY:-}" ] && [ ! -s "$HOME/.config/aimode/key" ] &&
    log "AI Mode needs a free Gemini API key: https://aistudio.google.com/apikey, its first run asks for it (tetron-aimode --key replaces it)"
  return 0
}

install_cast_tools() {
  local uv; uv="$(uv_bin)"
  [ -n "$uv" ] || { warn "uv is missing; catt and casttui not installed"; return 0; }
  if ! have_tool catt; then
    log "installing catt: $uv tool install catt"
    "$uv" tool install catt || warn "catt did not install; see https://github.com/skorokithakis/catt"
  fi
  if ! have_tool casttui; then
    log "installing casttui: $uv tool install $CASTTUI_SRC"
    "$uv" tool install "$CASTTUI_SRC" || warn "casttui did not install; see https://github.com/tetronos/casttui"
  fi
}

# opencode: the agent of the ✦ Assistant (WMINST-010). No distro packages it.
# Upstream's stable channel is its GitHub "latest" release (what its own
# installer and npm `latest` follow; the 2.x line is separate). The tarball
# for this CPU (baseline without AVX2, musl on musl systems) is checked
# against the sha256 digest GitHub records for the asset (1.x releases carry
# no SHA256SUMS) and goes to ~/.local/bin. Settings > Updates replaces it the
# same way: `opencode upgrade` would put a second copy in ~/.opencode/bin.
# Homebrew on macOS.
OPENCODE_API="https://api.github.com/repos/anomalyco/opencode/releases/latest"
opencode_asset() {
  local a
  case "$(uname -m)" in
    x86_64|amd64) a=x64; grep avx2 /proc/cpuinfo >/dev/null 2>&1 || a=x64-baseline ;;
    aarch64|arm64) a=arm64 ;;
    *) return 1 ;;
  esac
  grep -i musl <<<"$(ldd --version 2>&1)" >/dev/null && a="$a-musl"
  echo "opencode-linux-$a.tar.gz"
}
# "TAG URL SHA256" of asset $1 in the latest release.
opencode_release() {
  fetch "$OPENCODE_API" | python3 -c 'import json,sys; r=json.load(sys.stdin); a=[x for x in r["assets"] if x["name"]==sys.argv[1]][0]; print(r["tag_name"], a["browser_download_url"], a["digest"].split(":")[1])' "$1"
}
install_opencode() {
  if [ "$IS_LINUX" = 0 ]; then
    have brew && { brew install opencode || warn "opencode did not install; see https://opencode.ai"; }
    return 0
  fi
  have python3 || { warn "python3 is missing; opencode not installed"; return 0; }
  local f rel tag url sum tmp
  f="$(opencode_asset)" || { warn "no opencode build for $(uname -m); see https://opencode.ai"; return 0; }
  rel="$(opencode_release "$f" 2>/dev/null)" || rel=""
  read -r tag url sum <<<"$rel"
  [ -n "${sum:-}" ] || { warn "could not read the opencode release from GitHub; see https://opencode.ai"; return 0; }
  tmp="$(mktemp -d)"
  log "installing opencode $tag ($f, checked against GitHub's sha256 digest) into ~/.local/bin"
  if fetch "$url" > "$tmp/$f" && sha256sum -c - <<<"$sum  $tmp/$f" >/dev/null && tar xzf "$tmp/$f" -C "$tmp" opencode; then
    mkdir -p "$HOME/.local/bin" && install -m 755 "$tmp/opencode" "$HOME/.local/bin/opencode"
  else
    warn "opencode did not install (download or checksum failed); see https://opencode.ai"
  fi
  rm -rf "$tmp"
}

# The recommended size's apps no distribution packages (WMINST-012): pyradio,
# llm (with its Ollama plugin) and yt-dlp with `uv tool install` (Settings >
# Updates upgrades them with the other uv tools); croft, its musl build checked
# against the release's SHA256SUMS (it updates itself); freebuff, the native
# binary its npm launcher would fetch, checked against the sha256 the npm
# package lists (binaryChecksums), so no node is needed. Linux only for croft
# and freebuff.
SCRIPT_APPS="croft pyradio llm yt-dlp freebuff"
PYRADIO_SRC="git+https://github.com/coderholic/pyradio"
CROFT_URL="https://github.com/vitali87/croft/releases/latest/download"
FREEBUFF_NPM="https://registry.npmjs.org/freebuff/latest"
FREEBUFF_DL="https://codebuff.com/api/releases/download"
FREEBUFF_DIR="$HOME/.local/share/freebuff"
# yt-dlp counts only in ~/.local/bin: a distribution's copy (Debian's comes
# with mpv's Recommends) falls behind the sites it reads.
script_app_present() { if [ "$1" = yt-dlp ]; then [ -x "$HOME/.local/bin/yt-dlp" ]; else have_tool "$1"; fi; }
script_apps_missing() {
  local a out=""
  for a in $SCRIPT_APPS; do want_script "$a" && ! script_app_present "$a" && out="${out:+$out }$a"; done
  printf '%s' "$out"
}
install_uv_app() {
  local uv; uv="$(uv_bin)"
  [ -n "$uv" ] || { warn "uv is missing; $1 not installed"; return 0; }
  local -a args
  case "$1" in
    pyradio) args=("$PYRADIO_SRC" --with rich --with requests --with dnspython --with psutil --with netifaces-plus --with python-dateutil --with charset-normalizer) ;;
    llm) args=(llm --with llm-ollama) ;;
    # deno: the JS runtime yt-dlp needs for YouTube, inside its own uv venv.
    yt-dlp) args=("yt-dlp[default,deno]") ;;
    *) return 0 ;;
  esac
  log "installing $1: $uv tool install ${args[*]}"
  "$uv" tool install "${args[@]}" || warn "$1 did not install"
}
install_croft() {
  [ "$IS_LINUX" = 1 ] || { warn "croft: Linux builds only here; see https://github.com/vitali87/croft"; return 0; }
  local f tmp; f="croft-$(uname -m)-unknown-linux-musl"; tmp="$(mktemp -d)"
  log "installing croft ($f, checked against the release's SHA256SUMS) into ~/.local/bin"
  if fetch "$CROFT_URL/$f.tar.gz" > "$tmp/$f.tar.gz" && fetch "$CROFT_URL/SHA256SUMS" > "$tmp/SHA256SUMS" \
    && grep " $f.tar.gz\$" "$tmp/SHA256SUMS" | (cd "$tmp" && sha256sum -c - >/dev/null) \
    && tar xzf "$tmp/$f.tar.gz" -C "$tmp"; then
    mkdir -p "$HOME/.local/bin" && install -m 755 "$tmp/$f/croft" "$HOME/.local/bin/croft"
  else
    warn "croft did not install (download or checksum failed); see https://github.com/vitali87/croft"
  fi
  rm -rf "$tmp"
}
# The binaryChecksums key for this CPU: x64 needs AVX2, else x64-baseline.
freebuff_target() {
  case "$(uname -m)" in
    x86_64|amd64) grep avx2 /proc/cpuinfo >/dev/null 2>&1 && echo linux-x64 || echo linux-x64-baseline ;;
    aarch64|arm64) echo linux-arm64 ;;
    *) return 1 ;;
  esac
}
# freebuff and tree-sitter.wasm (which it reads next to itself) go to
# ~/.local/share/freebuff with the version; ~/.local/bin/freebuff links there.
install_freebuff() {
  [ "$IS_LINUX" = 1 ] || { warn "freebuff: Linux only here; npm install -g freebuff elsewhere"; return 0; }
  have python3 || { warn "python3 is missing; freebuff not installed"; return 0; }
  local t rel ver sum tmp
  t="$(freebuff_target)" || { warn "no freebuff build for $(uname -m)"; return 0; }
  rel="$(fetch "$FREEBUFF_NPM" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d["version"], d["binaryChecksums"][sys.argv[1]])' "$t" 2>/dev/null)" || rel=""
  read -r ver sum <<<"$rel"
  case "${ver:-}" in ""|*[!0-9A-Za-z.+-]*) warn "could not read the freebuff release from npm; see https://github.com/CodebuffAI/codebuff"; return 0 ;; esac
  tmp="$(mktemp -d)"
  log "installing freebuff $ver ($t, checked against the sha256 its npm package lists) into $FREEBUFF_DIR"
  if fetch "$FREEBUFF_DL/$ver/freebuff-$t.tar.gz" > "$tmp/fb.tar.gz" && sha256sum -c - <<<"$sum  $tmp/fb.tar.gz" >/dev/null \
    && tar xzf "$tmp/fb.tar.gz" -C "$tmp" freebuff; then
    mkdir -p "$FREEBUFF_DIR" "$HOME/.local/bin"
    tar xzf "$tmp/fb.tar.gz" -C "$tmp" tree-sitter.wasm 2>/dev/null && install -m 644 "$tmp/tree-sitter.wasm" "$FREEBUFF_DIR/"
    install -m 755 "$tmp/freebuff" "$FREEBUFF_DIR/freebuff" && echo "$ver" > "$FREEBUFF_DIR/version" \
      && ln -sfn "$FREEBUFF_DIR/freebuff" "$HOME/.local/bin/freebuff"
  else
    warn "freebuff did not install (download or checksum failed); see https://github.com/CodebuffAI/codebuff"
  fi
  rm -rf "$tmp"
}

# The symbols-only Nerd Font: the glyphs tetron-wm draws for icon_style = "nerd".
# No distro packages it; tiny, and fontconfig uses it only as a fallback.
# fontconfig answers when present; without it, look for the font file.
nerd_font_missing() {
  if have fc-list; then ! fc-list 2>/dev/null | grep -i "nerd font" >/dev/null
  else ! ls "$HOME/.local/share/fonts"/SymbolsNerdFont* >/dev/null 2>&1; fi
}
install_nerd_font() {
  if [ "$IS_LINUX" = 0 ]; then
    have brew && { brew install --cask font-symbols-only-nerd-font || warn "the Nerd Font did not install"; }
    return 0
  fi
  have unzip || { warn "unzip is missing; Symbols Nerd Font not installed"; return 0; }
  local fdir="$HOME/.local/share/fonts" tmp
  tmp="$(mktemp -d)"
  log "installing Symbols Nerd Font into $fdir"
  if fetch "https://github.com/ryanoasis/nerd-fonts/releases/latest/download/NerdFontsSymbolsOnly.zip" > "$tmp/nf.zip" \
    && mkdir -p "$fdir" && unzip -o -q "$tmp/nf.zip" '*.ttf' -d "$fdir"; then
    have fc-cache && fc-cache -f "$fdir" >/dev/null 2>&1
  else
    warn "the Nerd Font download failed (icon_style = nerd needs one)"
  fi
  rm -rf "$tmp"
}

# ── Step 3: kmscon ─────────────────────────────────────────────────────────────
install_kmscon() {
  log "installing kmscon (truecolor + mouse console)…"
  local tmp; tmp="$(mktemp)"
  fetch "$RAW/install-kmscon.sh" > "$tmp" || die "could not download install-kmscon.sh"
  # The suite already captured consent ("Install kmscon?"), so always pass -y: a
  # second prompt here defaults to No and would abort on an Enter-through run.
  # install-kmscon.sh still logs the source build, and still uses sudo itself.
  bash "$tmp" -y
  rm -f "$tmp"
}

# ── Console state (so --uninstall and re-runs know what we changed) ────────────
STATE="$HOME/.config/tetron-wm/suite-state"
state_get() { [ -f "$STATE" ] && sed -n "s/^$1=//p" "$STATE" | tail -1; }
state_set() {  # $1=key $2=value (replaces any previous value)
  mkdir -p "$(dirname "$STATE")"
  { [ -f "$STATE" ] && grep -v "^$1=" "$STATE"; echo "$1=$2"; } > "$STATE.new" 2>/dev/null || true
  mv "$STATE.new" "$STATE"
}

OVR_MARK='# managed by install-tetron-wm-suite.sh'
SNIP_BEGIN='# >>> tetron-wm autostart (managed by install-tetron-wm-suite.sh) >>>'
SNIP_END='# <<< tetron-wm autostart <<<'

profile_file() {
  case "$(basename "${SHELL:-sh}")" in zsh) echo "$HOME/.zprofile" ;; *) echo "$HOME/.bash_profile" ;; esac
}

# Remove our autostart block from a profile file — the marked block, and the
# unmarked snippet older versions of this script wrote. No-op if absent.
strip_autostart() {
  local f="$1" tmp; [ -f "$f" ] || return 0
  grep -qE 'tetron-wm autostart|TETRON_WM_ACTIVE' "$f" || return 0
  tmp="$(mktemp)"
  awk -v b="$SNIP_BEGIN" -v e="$SNIP_END" '
    index($0,b)==1 {skip=1; marked=1}
    !skip && /^# Launch tetron-wm on the primary console/ {skip=1; legacy=1}
    skip {
      if (marked && index($0,e)==1) {skip=0; marked=0}
      else if (legacy && $0=="fi") {skip=0; legacy=0}
      next
    }
    {print}' "$f" > "$tmp" && mv "$tmp" "$f"
}

# ── Step 4a: kmscon on a VT (optionally with autologin) ────────────────────────
#   systemd → kmsconvt@ttyN (→ agetty --autologin USER, if autologin)
setup_console() {
  local vt="$1" autologin="$2"
  have systemctl || { warn "no systemd (systemctl) — skipping console setup"; return 0; }
  have kmscon    || { warn "kmscon is not installed — run the kmscon step first; skipping console"; return 0; }

  warn "This edits the login console on tty$vt. Keep a second VT (Ctrl+Alt+F2) or an"
  warn "SSH session open so a mistake can never lock you out. To undo later:"
  warn "  install-tetron-wm-suite.sh --uninstall"
  confirm "Make kmscon the login screen on tty$vt (Ctrl+Alt+F$vt)? It replaces the plain kernel text console, adding truecolor and mouse" y || { log "skipping console setup"; return 0; }

  # TERM must name what kmscon emulates (xterm-256color). Do NOT use $TERM here:
  # in a systemd unit it expands to the console's "linux", which makes tetron-wm
  # think it is on a kernel VT — no truecolor, mouse via gpm, clunky rendering.
  local user kbin term='xterm-256color'
  user="$(logname 2>/dev/null || echo "${SUDO_USER:-$USER}")"
  kbin="$(command -v kmscon)"

  log "1/2 point tty$vt at kmscon instead of the kernel getty"
  $SUDO systemctl disable --now "getty@tty$vt" 2>/dev/null || true
  $SUDO systemctl enable  --now "kmsconvt@tty$vt"

  local ovrd="/etc/systemd/system/kmsconvt@tty$vt.service.d"
  if [ "$autologin" = y ]; then
    warn "autologin: nobody is asked for a password on tty$vt; anyone at this keyboard gets the account $user"
    log "2/2 autologin $user on tty$vt (drop-in override)"
    $SUDO mkdir -p "$ovrd"
    # %%I -> %I (systemd instance). The final arg is the login TERM: a literal
    # xterm-256color (what kmscon emulates), NOT $TERM — see the note above.
    { printf '%s\n' "$OVR_MARK"
      printf '[Service]\nExecStart=\nExecStart=%s "--vt=%%I" --seats=seat0 --no-switchvt --login -- /sbin/agetty --autologin %s --noclear - %s\n' \
        "$kbin" "$user" "$term"; } | $SUDO tee "$ovrd/override.conf" >/dev/null
  else
    log "2/2 normal login prompt on tty$vt (no autologin)"
    # Converge: drop an autologin override a previous run left behind.
    if $SUDO grep -qF "$OVR_MARK" "$ovrd/override.conf" 2>/dev/null; then
      $SUDO rm -f "$ovrd/override.conf"; $SUDO rmdir "$ovrd" 2>/dev/null || true
    fi
  fi
  # Under kmscon the mouse comes from kmscon itself; a running gpm is redundant
  # and can fight it for the input device. Disable it on the console.
  if have gpm && systemctl is-enabled gpm >/dev/null 2>&1; then
    $SUDO systemctl disable --now gpm >/dev/null 2>&1 && { log "disabled gpm (kmscon provides the mouse)"; state_set gpm_disabled 1; } || true
  fi
  $SUDO systemctl daemon-reload
  $SUDO systemctl restart "kmsconvt@tty$vt"
  state_set vt "$vt"; state_set autologin "$autologin"
  ensure_truecolor
  log "tty$vt is set. Reboot (or switch with Ctrl+Alt+F$vt) to see it."
}

# ── Step 4b: autostart tetron-wm on kmscon login ───────────────────────────────
setup_autostart() {
  local vt="$1" wm wmdir prof; wm="$(wm_bin)"; wmdir="$(dirname "$wm")"; prof="$(profile_file)"
  [ -x "$wm" ] || { warn "tetron-wm not found on PATH — install it first; skipping autostart"; return 0; }
  strip_autostart "$prof"   # converge: replace any earlier (marked or legacy) block
  # Launch only on the chosen VT. kmscon runs the login shell on a PTS, not
  # /dev/ttyN, so a `tty = /dev/ttyN` test never matches under kmscon; walk the
  # process tree up to the kmscon process and read its --vt=ttyN argument
  # instead (also accept the raw VT). Exclude SSH. Absolute binary path
  # + PATH prepend: this profile runs before the interactive rc, where the
  # install dir may not be on PATH yet. `$wm`/`$wmdir` expand now; the rest stays
  # literal for login time.
  cat >> "$prof" <<EOF

$SNIP_BEGIN
# Launch tetron-wm on tty$vt (kmscon, or the raw VT), not over SSH, in nested
# shells, or on other VTs. Drop \`exec\` to land on a shell when you quit.
if [ -z "\$TETRON_WM_ACTIVE" ] && [ -z "\$SSH_CONNECTION" ]; then
  _tw=0
  [ "\$(tty)" = "/dev/tty$vt" ] && _tw=1
  _p=\$PPID
  while [ "\$_tw" = 0 ] && [ "\${_p:-0}" -gt 1 ]; do
    case "\$(cat /proc/\$_p/comm 2>/dev/null)" in
      kmscon) case "\$(tr '\\0' ' ' </proc/\$_p/cmdline 2>/dev/null)" in *"--vt=tty$vt "*) _tw=1 ;; esac ;;
    esac
    _p=\$(awk '/^PPid:/{print \$2}' /proc/\$_p/status 2>/dev/null)
  done
  if [ "\$_tw" = 1 ]; then
    export TETRON_WM_ACTIVE=1
    export PATH="$wmdir:\$PATH"
    exec "$wm"
  fi
  unset _tw _p
fi
$SNIP_END
EOF
  state_set autostart_profile "$prof"; state_set autostart_vt "$vt"
  ensure_truecolor
  # Keep apps alive across UI reloads/updates by running the apphost as a service.
  "$wm" service install >/dev/null 2>&1 && { log "installed the tetron-wm apphost service"; state_set apphost_service 1; } || true
  log "autostart set in $prof (tetron-wm starts on tty$vt only)"
}

# ── Step 4c: full screen video helper (kmscon) ─────────────────────────────────
# kmscon owns the display, so a player started from it can not draw or read keys.
# A waiter runs as the user on a spare VT in its own login session; the client
# asks it to play, it switches there through logind (no root), runs mpv and
# switches back. See src/fullscreen.rs. The unit runs as the user, never root.
# With tetron-wm-desktop installed, its template unit (VT 9, /usr/bin/tetron-wm)
# is used, one instance per user, as on the TetronOS Desktop and from Settings;
# a unit this script wrote earlier is then removed. Another VT, or no template,
# writes FS_UNIT as before.
FS_UNIT=/etc/systemd/system/tetron-wm-fullscreen.service
FS_TEMPLATE=/usr/lib/systemd/system/tetron-wm-fullscreen@.service
setup_fullscreen() {
  local vt="$1" wm user; wm="$(wm_bin)"
  have systemctl || { warn "no systemd (systemctl): skipping the full screen helper"; return 0; }
  [ -x "$wm" ] || { warn "tetron-wm not found: install it first; skipping the full screen helper"; return 0; }
  have mpv || warn "mpv is not installed: the helper will refuse to play until it is"
  user="$(logname 2>/dev/null || echo "${SUDO_USER:-$USER}")"
  [ "$user" != root ] || { warn "run this as your own user, not root: skipping the full screen helper"; return 0; }
  if [ "$vt" = 9 ] && [ -f "$FS_TEMPLATE" ] && [ "$wm" = /usr/bin/tetron-wm ]; then
    if $SUDO grep -qF "$OVR_MARK" "$FS_UNIT" 2>/dev/null; then
      log "replacing the full screen helper this script wrote with the packaged one"
      $SUDO systemctl disable --now tetron-wm-fullscreen.service 2>/dev/null || true
      $SUDO rm -f "$FS_UNIT"; $SUDO systemctl daemon-reload
    fi
    $SUDO systemctl enable --now "tetron-wm-fullscreen@$user.service"
    state_set fullscreen_vt "$vt"; state_set fullscreen_unit "tetron-wm-fullscreen@$user.service"
    log "full screen helper is on tty$vt (tetron-wm-fullscreen@$user). Right-click a video in tetron-wm on kmscon: Play full screen."
    return 0
  fi
  log "installing the full screen helper on tty$vt (runs as $user)"
  $SUDO tee "$FS_UNIT" >/dev/null <<UNIT
$OVR_MARK
[Unit]
Description=tetron-wm full screen video waiter on tty$vt
After=systemd-user-sessions.service
Conflicts=getty@tty$vt.service

[Service]
User=$user
ExecStart=$wm fullscreen-waiter --vt $vt
Restart=always
RestartSec=1
TTYPath=/dev/tty$vt
TTYReset=yes
TTYVHangup=yes
StandardInput=tty
StandardOutput=tty
StandardError=journal
PAMName=login
UtmpIdentifier=tty$vt
UtmpMode=user

[Install]
WantedBy=multi-user.target
UNIT
  $SUDO systemctl daemon-reload
  $SUDO systemctl enable --now tetron-wm-fullscreen.service
  state_set fullscreen_vt "$vt"
  log "full screen helper is on tty$vt. Right-click a video in tetron-wm on kmscon: Play full screen."
}

# ── Uninstall ──────────────────────────────────────────────────────────────────
do_uninstall() {
  printf '%stetron-wm suite uninstall%s\n\n' "$B" "$N"
  local wm; wm="$(wm_bin)"

  # 1. autostart snippet
  local prof; for prof in "$HOME/.zprofile" "$HOME/.bash_profile"; do
    if [ -f "$prof" ] && grep -qE 'tetron-wm autostart|TETRON_WM_ACTIVE' "$prof"; then
      if confirm "Remove the tetron-wm autostart block from $prof?" y; then strip_autostart "$prof"; log "removed autostart from $prof"; fi
    fi
  done

  # 2. console: every VT we set up (state file), else any marked override
  local v vts=""; vts="$(state_get vt)"
  local d; for d in /etc/systemd/system/kmsconvt@tty*.service.d; do
    [ -d "$d" ] || continue
    if $SUDO grep -qF "$OVR_MARK" "$d/override.conf" 2>/dev/null; then
      v="${d##*kmsconvt@tty}"; v="${v%.service.d}"
      case " $vts " in *" $v "*) ;; *) vts="$vts $v" ;; esac
    fi
  done
  for v in $vts; do
    if confirm "Give tty$v back to the normal login prompt (stop kmscon there)?" y; then
      $SUDO rm -f "/etc/systemd/system/kmsconvt@tty$v.service.d/override.conf"
      $SUDO rmdir "/etc/systemd/system/kmsconvt@tty$v.service.d" 2>/dev/null || true
      $SUDO systemctl daemon-reload
      $SUDO systemctl disable --now "kmsconvt@tty$v" 2>/dev/null || true
      $SUDO systemctl enable  --now "getty@tty$v" 2>/dev/null || true
      log "tty$v restored to getty"
    fi
  done
  if [ "$(state_get gpm_disabled)" = 1 ] && have gpm; then
    confirm "Re-enable gpm (the raw-console mouse daemon we disabled)?" y \
      && $SUDO systemctl enable --now gpm >/dev/null 2>&1 && log "gpm re-enabled"
  fi

  # 2b. full screen helper
  if $SUDO grep -qF "$OVR_MARK" "$FS_UNIT" 2>/dev/null && confirm "Remove the full screen video helper (tty$(state_get fullscreen_vt))?" y; then
    $SUDO systemctl disable --now tetron-wm-fullscreen.service 2>/dev/null || true
    $SUDO rm -f "$FS_UNIT"; $SUDO systemctl daemon-reload
    log "removed the full screen helper"
  fi
  fsu="$(state_get fullscreen_unit)"
  if [ -n "$fsu" ] && confirm "Disable the full screen video helper ($fsu)?" y; then
    $SUDO systemctl disable --now "$fsu" 2>/dev/null || true
    log "disabled $fsu (tetron-wm-desktop keeps the unit file)"
  fi

  # 3. apphost service
  if [ -x "$wm" ] && confirm "Remove the tetron-wm apphost service?" y; then
    "$wm" service uninstall >/dev/null 2>&1 && log "removed the apphost service" || true
  fi

  rm -f "$STATE"

  # 4. optional: binary + config
  if [ "$PURGE" = 1 ]; then
    if deb_host && wm_packaged; then
      # apt owns it: removing the file by hand would leave a broken package.
      if confirm "Remove the tetron-wm package and its size packages with apt?" n; then
        # An own build first: remove it and the diversion, so nothing is left behind.
        if wm_diverted; then
          $SUDO rm -f /usr/bin/tetron-wm
          $SUDO dpkg-divert --local --rename --remove /usr/bin/tetron-wm >/dev/null && log "removed the own build and the diversion"
        fi
        # shellcheck disable=SC2046
        $SUDO apt-get remove -y $(for p in tetron-wm-extras tetron-wm-recommended tetron-wm-essentials tetron-wm-desktop tetron-wm; do
          dpkg-query -W -f='${db:Status-Status}' "$p" 2>/dev/null | grep -qx installed && echo "$p"; done) \
          && log "removed the tetron-wm packages (apt autoremove removes the apps they brought)"
      fi
    elif [ -x "$wm" ] && confirm "Remove the tetron-wm binary ($wm)?" n; then
      rm -f "$wm" 2>/dev/null || $SUDO rm -f "$wm"; log "removed $wm"
    fi
    if [ -d "$HOME/.config/tetron-wm" ] && confirm "Delete ~/.config/tetron-wm (config and themes)?" n; then
      rm -rf "$HOME/.config/tetron-wm"; log "removed ~/.config/tetron-wm"
    fi
  else
    log "kept the binary, ~/.config/tetron-wm and kmscon (add --purge to remove the first two)"
  fi
  log "done."
}

# ── Interactive choices (the same options as the flags) ─────────────────────────
HAVE_TTY=0; [ "$ASSUME_YES" != 1 ] && tty_ok && HAVE_TTY=1

# Pick from a numbered menu; default on Enter. $1=question, $2=default number,
# rest = options. Sets CHOICE to the chosen number.
choose() {
  local q="$1" def="$2" n=0 o ans; shift 2
  printf '%s%s%s\n' "$B" "$q" "$N" >&2
  for o in "$@"; do n=$((n+1)); printf '  %d) %s\n' "$n" "$o" >&2; done
  printf '%sChoice [%s]: %s' "$B" "$def" "$N" >&2
  read -r ans </dev/tty || ans=""
  ans="${ans:-$def}"
  case "$ans" in [1-9]) [ "$ans" -le "$n" ] && { CHOICE="$ans"; return 0; } ;; esac
  warn "not a valid choice — using $def"; CHOICE="$def"
}

# Ask for the VT number (default 1) unless set by --vt / non-interactive.
resolve_vt() {
  [ -n "$VT" ] && return 0
  VT=1
  [ "$HAVE_TTY" = 1 ] || return 0
  local ans; printf '%sWhich VT? tty1 opens at boot [1]: %s' "$B" "$N" >&2
  read -r ans </dev/tty || ans=""
  case "${ans:-1}" in [1-9]|[1-5][0-9]|6[0-3]) VT="${ans:-1}" ;; *) warn "not a VT number — using 1" ;; esac
}

# Is this a machine with a desktop session (so kmscon is not what you want)?
is_graphical() {
  [ -n "${DISPLAY:-}${WAYLAND_DISPLAY:-}" ] && return 0
  case "${XDG_SESSION_TYPE:-}" in x11|wayland) return 0 ;; esac
  have systemctl && [ "$(systemctl get-default 2>/dev/null)" = "graphical.target" ]
}

apply_profile() {  # set any WANT_ flag the user has not already set on the command line
  local k v
  set_default() { eval "[ -n \"\$$1\" ] || $1=$2"; }
  case "$1" in
    gui)       set_default WANT_KMSCON n; set_default WANT_CONSOLE n; set_default WANT_AUTOLOGIN n; set_default WANT_AUTOSTART n ;;
    # console and login leave autologin unset: it is asked about below, default no.
    console)   set_default WANT_KMSCON y; set_default WANT_CONSOLE y; set_default WANT_AUTOSTART n ;;
    login)     set_default WANT_KMSCON y; set_default WANT_CONSOLE y; set_default WANT_AUTOSTART y ;;
    appliance) set_default WANT_KMSCON y; set_default WANT_CONSOLE y; set_default WANT_AUTOLOGIN y; set_default WANT_AUTOSTART y ;;
  esac
}

# ── Orchestration ──────────────────────────────────────────────────────────────
main() {
  if [ "$UNINSTALL" = 1 ]; then do_uninstall; return; fi
  printf '%stetron-wm suite installer%s  (repo: %s, branch: %s)\n\n' "$B" "$N" "$REPO" "$BRANCH"

  local gui_default=2
  if [ "$IS_LINUX" = 1 ]; then
    is_graphical && gui_default=1 || gui_default=3   # 3 = login prompt, then tetron-wm; never autologin
    # Menu only when no profile and nothing console-related was given as a flag.
    if [ -z "$PROFILE" ] && [ "$HAVE_TTY" = 1 ] && [ -z "$WANT_KMSCON$WANT_CONSOLE$WANT_AUTOLOGIN$WANT_AUTOSTART" ]; then
      choose "What are you setting up?" "$gui_default" \
        "A desktop machine: tetron-wm in a terminal window  (gui)" \
        "A VC tty with kmscon, normal login prompt, start tetron-wm yourself  (console)" \
        "A VC tty with kmscon, normal login prompt, tetron-wm starts after you log in  (login)" \
        "Custom: ask me about each piece"
      case "$CHOICE" in 1) PROFILE=gui ;; 2) PROFILE=console ;; 3) PROFILE=login ;; esac
    fi
    [ -n "$PROFILE" ] && apply_profile "$PROFILE"
  else
    PROFILE=gui; apply_profile gui
  fi

  # Size: every user chooses; essentials is the default.
  if [ -z "$SIZE" ]; then
    SIZE=essentials
    if [ "$HAVE_TTY" = 1 ]; then
      choose "How much do you want installed?" 2 \
        "minimal: tetron-wm only" \
        "essentials: tetron-wm, the tools its features use and one good app per need (default)" \
        "recommended: essentials plus a curated pick in every category" \
        "extras: recommended plus the extra apps"
      case "$CHOICE" in 1) SIZE=minimal ;; 3) SIZE=recommended ;; 4) SIZE=extras ;; esac
    fi
  fi
  load_apps

  # Debian and Ubuntu: how tetron-wm is installed (WMINST-013). An own build
  # (diverted package) is kept unless a method is chosen.
  local raw="" debarch="" cur=""
  if deb_host; then
    debarch="$(dpkg --print-architecture)"
    raw="$(wm_raw_copy || true)"
    if wm_diverted; then cur=own; elif wm_packaged; then cur=deb; elif [ -n "$raw" ]; then cur=binary; fi
    if [ -z "$METHOD" ] && [ "$cur" = own ] && [ "$WANT_WM" != n ]; then
      if confirm "tetron-wm here is your own build (the package's binary is diverted). Keep it?" y; then
        [ -n "$WANT_WM" ] || WANT_WM=n
      else
        choose "Install tetron-wm as:" 1 \
          "deb: the apt package again, updated by apt upgrade" \
          "binary: the latest release binary in place of your build"
        case "$CHOICE" in 2) METHOD=binary ;; *) METHOD=deb ;; esac
        WANT_WM=y
      fi
    elif [ -z "$METHOD" ] && [ "$HAVE_TTY" = 1 ] && [ "$WANT_WM" != n ]; then
      local def=1; [ "$cur" = binary ] && def=2
      choose "How should tetron-wm be installed?" "$def" \
        "deb: the apt package; apt upgrade updates it (adds the tetronos apt repo)" \
        "binary: the release binary$( [ "$cur" = deb ] && echo '; the package stays, its binary is set aside (dpkg-divert)')"
      case "$CHOICE" in 2) METHOD=binary ;; *) METHOD=deb ;; esac
      [ "$METHOD" = deb ] && [ "$cur" = deb ] && METHOD=""   # unchanged: install.sh keeps it
      [ "$METHOD" = binary ] && [ "$cur" = binary ] && METHOD=""
    fi
  fi
  [ -n "$METHOD" ] && WANT_WM=y
  resolve WANT_WM     "Install tetron-wm (latest release)?" y
  resolve WANT_CONFIG "Set up ~/.config/tetron-wm/ (config.toml, themes/)?" y
  resolve WANT_THEMES "Download the extra themes into ~/.local/share/tetron-wm/themes/? Files you already have are kept." y

  # How the apps arrive: the release metapackages through apt when tetron-wm is
  # (or is about to be) the apt package, else one by one. Never a metapackage
  # beside a raw tetron-wm: it depends on the package and would add a copy.
  local will_be_deb=0 use_debs=0
  if deb_host; then
    # The package is (or stays) installed: deb, or binary with it diverted.
    if wm_packaged || { [ "$WANT_WM" = y ] && [ "$METHOD" != binary ] && { [ "$METHOD" = deb ] || [ -z "$raw" ]; } && [ -n "$(asset_for tetron-wm "$debarch")" ]; }; then
      will_be_deb=1
    fi
    [ "$will_be_deb" = 1 ] && release_has_size_debs && use_debs=1
  fi
  local apps="" shown="" services=""
  if [ "$SIZE" != minimal ]; then
    apps="$(size_list)"
    shown="$apps"
    { fresh_via_script || fresh_via_brew; } && want_script fresh && shown="${shown:+$shown }fresh"
    uv_via_script && shown="${shown:+$shown }uv"
    local cast; cast="$(cast_tools_missing)"
    { want_script catt || want_script casttui; } && [ -n "$cast" ] && shown="${shown:+$shown }$cast"
    want_script nerd-font && nerd_font_missing && shown="${shown:+$shown }nerd-font"
    want_script opencode && ! have_tool opencode && shown="${shown:+$shown }opencode"
    local more; more="$(script_apps_missing)"; [ -n "$more" ] && shown="${shown:+$shown }$more"
  fi
  [ "$IS_LINUX" = 1 ] && services="$(services_list)"
  if [ -n "$services" ]; then
    resolve WANT_SERVICES "Install system services for the tray ($services)? They change how this machine manages sound, network or Bluetooth" n
  else
    WANT_SERVICES=n
  fi
  if [ "$IS_LINUX" = 1 ]; then
    local kdef=y; is_graphical && kdef=n
    resolve WANT_KMSCON "Install kmscon (truecolor + mouse console; compiles from source except on Debian trixie)?" "$kdef"
    [ "$WANT_AUTOLOGIN" = y ] && [ -z "$WANT_CONSOLE" ] && WANT_CONSOLE=y
    resolve WANT_CONSOLE   "Make kmscon the login screen on a text console (Ctrl+Alt+F1..F6)? It replaces the plain kernel text console, adding truecolor and mouse" n
    if [ "$WANT_CONSOLE" = y ]; then
      resolve_vt
      resolve WANT_AUTOLOGIN "Do you want to log in automatically on tty$VT (no password; anyone at the keyboard gets your account)?" n
    else
      [ -z "$WANT_AUTOLOGIN" ] && WANT_AUTOLOGIN=n
    fi
    resolve WANT_AUTOSTART "Start tetron-wm automatically when you log in on a kmscon console?" n
    [ "$WANT_AUTOSTART" = y ] && resolve_vt
    if have systemctl && { [ "$WANT_KMSCON" = y ] || have kmscon; }; then
      resolve WANT_FULLSCREEN "Install the full screen video helper on tty$FSVT (Play full screen from kmscon, no sudo; runs as you)?" n
    else
      WANT_FULLSCREEN=n
    fi
  else
    WANT_KMSCON=n; WANT_CONSOLE=n; WANT_AUTOLOGIN=n; WANT_AUTOSTART=n; WANT_FULLSCREEN=n
  fi
  [ -n "$VT" ] || VT=1

  echo
  local method=binary; [ "$will_be_deb" = 1 ] && method=deb
  [ "$METHOD" = binary ] && method=binary
  [ -n "$CLONE" ] && method="own (cargo build --release in $CLONE)"
  [ -z "$METHOD" ] && [ "$cur" = own ] && method=own
  [ "$method" = binary ] && [ "$will_be_deb" = 1 ] && method="binary (package kept, its binary diverted)"
  if [ "$DRY_RUN" = 1 ]; then
    log "plan (dry run, nothing changed): size=$SIZE method=$method wm=$WANT_WM config=$WANT_CONFIG themes=$WANT_THEMES services=$WANT_SERVICES${services:+ ($services)} kmscon=$WANT_KMSCON console=$WANT_CONSOLE vt=$VT autologin=$WANT_AUTOLOGIN autostart=$WANT_AUTOSTART fullscreen=$WANT_FULLSCREEN$([ "$WANT_FULLSCREEN" = y ] && echo " (tty$FSVT)")"
    if [ "$use_debs" = 1 ]; then log "apps: apt install $(size_debs) (from release $(release_tag))$(no_mouse && echo ', without gpm (no mouse device)')${shown:+; missing now: $shown}"
    else log "apps: ${shown:-none}"; fi
    return 0
  fi
  [ "$WANT_WM" = y ]        && install_wm
  [ "$WANT_CONFIG" = y ]    && seed_config
  [ "$WANT_THEMES" = y ]    && install_themes
  if [ "$SIZE" != minimal ]; then
    if [ "$use_debs" = 1 ] && wm_packaged && install_size_debs; then
      install_size_recommends || FAILED="$FAILED $SIZE-apps"
    else
      [ "$(pm_col)" = 2 ] && [ -n "$apps" ] && apt_update_once
      pm_install "$apps"
    fi
    want_script fresh && { fresh_via_script && { apt_first fresh-editor || install_fresh; }; fresh_via_brew && { brew install fresh-editor || warn "fresh did not install"; }; }
    uv_via_script && install_uv
    want_script casttui && ! have_tool casttui && { apt_first casttui || true; }
    { want_script catt || want_script casttui; } && [ -n "$(cast_tools_missing)" ] && install_cast_tools
    want_script nerd-font && nerd_font_missing && { apt_first tetronos-nerd-font-symbols || install_nerd_font; }
    want_script opencode && ! have_tool opencode && install_opencode
    local a; for a in $(script_apps_missing); do
      case "$a" in croft) install_croft ;; freebuff) install_freebuff ;; *) install_uv_app "$a" ;; esac
    done
    # The deb ships the helper scripts; a raw install gets them here.
    want_script helpers && ! { deb_host && wm_packaged; } && [ -n "$(own_apps_missing)" ] && install_own_apps
  fi
  [ "$WANT_SERVICES" = y ]  && pm_install "$services"
  [ "$WANT_KMSCON" = y ]    && install_kmscon
  [ "$WANT_CONSOLE" = y ]   && setup_console "$VT" "$WANT_AUTOLOGIN"
  [ "$WANT_AUTOSTART" = y ] && setup_autostart "$VT"
  [ "$WANT_FULLSCREEN" = y ] && setup_fullscreen "$FSVT"
  # deb | binary | own (an own build; a kept diverted binary counts as one).
  method=binary
  if [ -n "$CLONE" ] && [ "$WANT_WM" = y ]; then method=own
  elif deb_host && wm_diverted; then [ "$WANT_WM" = y ] || method=own
  elif deb_host && wm_packaged; then method=deb
  elif [ "$WANT_WM" != y ] && [ "$(state_get method)" = own ]; then method=own
  fi
  state_set size "$SIZE"; state_set method "$method"

  echo
  if [ -n "$FAILED" ]; then
    warn "finished with problems:$FAILED (see the messages above)"
    exit 1
  fi
  log "done."
  if [ "$WANT_AUTOSTART" != y ]; then
    log "run it with:  tetron-wm   (install.sh noted the PATH if the dir is not already on it)"
  fi
}

main "$@"
