# Setup: tetron-wm on a bare VC tty with kmscon

Practical reference for running tetron-wm on a raw Linux virtual console (no X11
/ Wayland), through the maintained **kmscon** console, ending with a wallpaper
and a persistent autologin on **tty1**.

For the *why* behind each piece — the console stack, the maintained-fork
requirement, how the wallpaper renders on text, the autologin chain — read the
illustrated walkthrough **[`tetron-wm_bare_console_guide.html`](tetron-wm_bare_console_guide.html)**.
This file is the copy-paste companion.

Two routes to the same result:

- **[Path A — easy (scripted)](#path-a--easy-scripted)** — the repo's two install
  scripts do the work.
- **[Path B — by hand](#path-b--by-hand)** — every package, build, and font done
  manually, for when you want control or the scripts do not fit your distro.

Then both paths share **[Configure](#configure-both-paths)**,
**[Persist on tty1](#persist-on-tty1-both-paths)**, and
**[Troubleshooting](#troubleshooting)**.

Target: Debian trixie (apt) or Ubuntu/Mint (source build). Replace user `erik`
with your own throughout. Legend: `$` = your user, `#` = root/sudo. Switch VTs
with Ctrl+Alt+F1 … F6.

---

## What you need

**Required:** tetron-wm · the maintained **kmscon** (Aetf fork: truecolor +
mouse) · **chafa** (renders the image wallpaper to cells).

**Optional:** a **Symbols Nerd Font** (only for `icon_style = "nerd"`; the
default `ascii` needs no font) · **gpm** (mouse on a raw VT *without* kmscon; not
needed under kmscon) · **tetron** + **tetron-tui**
(https://github.com/tetronos/tetron-tui — the mesh-VPN tray segment
only; the desktop runs fine without them).

kmscon only matters at the physical console. Over SSH your client is the
terminal and tetron-wm just runs in it.

---

## Path A — easy (scripted)

Shortcut: `install-tetron-wm-suite.sh` does all of Path A and the
login console and autostart from "Persist on tty1" in one prompted run
(`--profile login`: a normal login prompt, tetron-wm starts after you log in;
it then asks "Do you want to log in automatically?", default no, and
`--autologin` answers yes; `--dry-run` to preview, `--uninstall` to undo). The
steps below are what it automates.

### A1. Install tetron-wm

```sh
curl -fsSL https://raw.githubusercontent.com/tetronos/tetron-wm/main/install.sh | sh
```

Installs tetron-wm only: the apt package on Debian and Ubuntu once releases carry a `.deb`, otherwise the prebuilt binary in `/usr/local/bin` (using `sudo`). The tools its features use (chafa, gpm, sshpass, the Nerd Font and the rest) come with the suite script's desktop size; `install.sh` prints that command when it finishes:

```sh
curl -fsSL https://tetronos.github.io/tetron-wm/install.sh | bash
```

For a no-sudo install set `TETRON_WM_BIN_DIR="$HOME/.local/bin"`, and put
`~/.local/bin` on PATH if it is not already:

```sh
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zprofile   # bash: ~/.bash_profile
```

### A2. Install kmscon

From a checkout of this repo:

```sh
./install-kmscon.sh
```

It apt-installs the maintained pair from **trixie-backports** on Debian, or
**builds the Aetf fork from source** on Ubuntu/Mint (behind a confirmation
prompt). It also installs console fonts with box/block glyphs. It changes **no**
getty/VT/login settings. Useful flags: `-y` (no prompt), `--source` (force
build), `--apt` (never build), `--no-fonts`.

Then continue at **[Configure](#configure-both-paths)**.

---

## Path B — by hand

### B1. Install tetron-wm

Prebuilt binary (resolve the tag via the redirect, not the rate-limited API):

```sh
mkdir -p ~/.local/bin
tag=$(curl -fsSI https://github.com/tetronos/tetron-wm/releases/latest \
      | awk 'tolower($1)=="location:"{print $2}' | tr -d '\r' | sed 's#.*/tag/##')
target=x86_64-unknown-linux-gnu          # ARM64: aarch64-unknown-linux-gnu
curl -fsSL "https://github.com/tetronos/tetron-wm/releases/download/$tag/tetron-wm-$target.tar.gz" \
  | tar -xz -C ~/.local/bin
chmod +x ~/.local/bin/tetron-wm
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zprofile   # bash: ~/.bash_profile
. ~/.zprofile && which tetron-wm
```

No binary for your CPU: `cargo install --git https://github.com/tetronos/tetron-wm` (Rust 1.95+).

### B2. Install chafa

```sh
sudo apt-get update && sudo apt-get install -y chafa
```

Without chafa the desktop falls back to a solid color; everything else works.

### B3. Install kmscon (maintained Aetf fork)

tetron-wm needs the maintained kmscon (truecolor + mouse passthrough), not the
stock kmscon older distros ship. kmscon has **no `--version` flag** — that is
normal. Do **B3-Debian** *or* **B3-Ubuntu**, not both.

#### B3-Debian (trixie) — apt from backports

```sh
sudo tee /etc/apt/sources.list.d/trixie-backports.sources >/dev/null <<'EOF'
Types: deb
URIs: http://deb.debian.org/debian
Suites: trixie-backports
Components: main
Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg
EOF
sudo apt-get update
sudo apt-get install -y -t trixie-backports kmscon libtsm4
sudo apt-get install -y --no-install-recommends fonts-dejavu-core fonts-terminus
```

The `-t trixie-backports` pin pulls libtsm4 4.7.1 to satisfy kmscon 10. kmscon
lands at `/usr/bin/kmscon`.

#### B3-Ubuntu / Mint — build from source

Build deps + console fonts (refresh the index first; a stale one 404s on
point-release deps):

```sh
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
  build-essential meson ninja-build pkg-config git ca-certificates \
  libdrm-dev libegl-dev libgles-dev libgbm-dev libglib2.0-dev \
  libpango1.0-dev libpixman-1-dev libsystemd-dev libudev-dev libxkbcommon-dev \
  fonts-dejavu-core fonts-terminus
```

Build libtsm first (kmscon links it), then kmscon:

```sh
cd /tmp && git clone --depth 1 https://github.com/Aetf/libtsm && cd libtsm
meson setup build --prefix=/usr/local --buildtype=release -Dc_args=-Wno-error -Dtests=false
ninja -C build && sudo ninja -C build install && sudo ldconfig

cd /tmp && git clone --depth 1 https://github.com/Aetf/kmscon && cd kmscon
m=$(uname -m)
export PKG_CONFIG_PATH="/usr/local/lib/pkgconfig:/usr/local/lib/$m-linux-gnu/pkgconfig"
export LD_LIBRARY_PATH="/usr/local/lib:/usr/local/lib/$m-linux-gnu"
meson setup build --prefix=/usr/local --buildtype=release -Dc_args=-Wno-error -Dtests=false
ninja -C build && sudo ninja -C build install && sudo ldconfig
```

`-Dc_args=-Wno-error` (tagged release builds `-Werror` and newer GCC trips a
format-truncation warning) and `-Dtests=false` (test subdir needs libcheck). A
source build lands kmscon at `/usr/local/bin/kmscon` — note that path for the
tty1 override below.

### B4. (Optional) Nerd Font — only for icon_style = "nerd"

```sh
sudo apt-get install -y unzip
mkdir -p ~/.local/share/fonts
curl -fsSL https://github.com/ryanoasis/nerd-fonts/releases/latest/download/NerdFontsSymbolsOnly.zip -o /tmp/nf.zip
unzip -o /tmp/nf.zip '*.ttf' -d ~/.local/share/fonts
fc-cache -f ~/.local/share/fonts
```

---

## Configure (both paths)

### Smoke-test kmscon on a spare VT first

From a root shell, launch kmscon on an unused VT and switch with Ctrl+Alt+F3:

```sh
sudo kmscon --vt 3 --xkb-layout us -- /bin/login
```

Log in there, run `tetron-wm`, confirm windows + mouse. Ctrl+Alt+F1 returns.

### First run

```sh
tetron-wm
```

With no config file it starts an empty desktop with the defaults: click the top-left button (or press `Ctrl+Space` then `l`) and pick **Shell**. Add apps to start automatically with `[[apps]]` in `config.toml` (see [CONFIG.md](CONFIG.md#app-entries)).

### Wallpaper

The default config shows a built-in wallpaper (`wallpaper = "default"`). To use your own, drop a file in the wallpapers folder and put its name in Settings → Appearance → Wallpaper file (or in `config.toml`); a full path works too:

```sh
mkdir -p ~/.config/tetron-wm/themes
cp /path/to/your-image.jpg ~/.config/tetron-wm/themes/
```

Quick test without editing config: `TETRON_WM_WALLPAPER=~/pics/foo.png tetron-wm`.

### config.toml knobs that matter on a console

In `~/.config/tetron-wm/config.toml` (or live in Settings → Appearance):

```toml
wallpaper = "your-image.jpg"   # or "default" for the theme's own picture
wallpaper_enabled = true
# truecolor = true          # if kmscon colors look 256-ish
# icon_style = "ascii"      # default; "nerd" needs the font from B4; "emoji" also works
# desktop_scrim = "none"    # backing behind icon labels over a wallpaper; or "#rrggbb[aa]"
```

---

## Persist on tty1 (both paths)

Goal: boot, land on tty1 already running tetron-wm. The chain is:

```
systemd → kmsconvt@tty1 → agetty --autologin erik → login shell → exec tetron-wm
```

> Keep a second VT (Ctrl+Alt+F2) or an SSH session open through these steps, so a
> mistake never locks you out of tty1.

### 1. Point tty1 at kmscon instead of the kernel getty

```sh
sudo systemctl disable --now getty@tty1
sudo systemctl enable  --now kmsconvt@tty1
```

### 2. Autologin your user on that console

`sudo systemctl edit kmsconvt@tty1` and add (use `/usr/local/bin/kmscon` for a
source build; the blank `ExecStart=` clears the default first):

```ini
[Service]
ExecStart=
ExecStart=/usr/bin/kmscon "--vt=%I" --seats=seat0 --no-switchvt --login -- /sbin/agetty --autologin erik --noclear - xterm-256color
```

> The last argument is the login `TERM`. Set it to the literal
> `xterm-256color` (what kmscon emulates), **not** `$TERM`: inside a systemd
> unit `$TERM` expands to the console value `linux`, which makes tetron-wm
> think it is on a kernel VT — no truecolor, mouse falls back to gpm, and the
> rendering looks coarse. Hardcoding `xterm-256color` gives truecolor and
> kmscon's own mouse. (With kmscon driving the mouse, a running `gpm` is
> redundant: `sudo systemctl disable --now gpm`.)

```sh
sudo systemctl daemon-reload && sudo systemctl restart kmsconvt@tty1
```

### 3. Launch tetron-wm from the login shell (tty1 only)

Append to `~/.zprofile` (bash: `~/.bash_profile`):

```sh
# Launch tetron-wm on the primary console (kmscon session, or raw VT1).
if [ -z "$TETRON_WM_ACTIVE" ] && [ -z "$SSH_CONNECTION" ]; then
  _tw=0
  [ "$(tty)" = "/dev/tty1" ] && _tw=1
  _p=$PPID
  while [ "$_tw" = 0 ] && [ "${_p:-0}" -gt 1 ]; do
    case "$(cat /proc/$_p/comm 2>/dev/null)" in kmscon) _tw=1 ;; esac
    _p=$(awk '/^PPid:/{print $2}' /proc/$_p/status 2>/dev/null)
  done
  if [ "$_tw" = 1 ]; then
    export TETRON_WM_ACTIVE=1
    export PATH="/usr/local/bin:$PATH"
    exec /usr/local/bin/tetron-wm
  fi
  unset _tw _p
fi
```

> Do **not** gate only on `[ "$(tty)" = "/dev/tty1" ]`: kmscon runs the login
> shell on a pseudo-terminal (`/dev/pts/N`), so that test is false under
> kmscon and the desktop never starts (you land at a shell prompt). The block
> above instead detects the console by walking the process tree up to
> `kmscon` (and still accepts a raw VT1), while excluding SSH logins.

`exec` hands the session to tetron-wm; quitting ends the session and kmscon
re-logs you in. Drop `exec` to land on a shell when you quit. Reboot (or restart
`kmsconvt@tty1` + Ctrl+Alt+F1) and tty1 comes up in the desktop with your
wallpaper.

---

## Troubleshooting

| Symptom | Fix |
|---|---|
| Solid-color desktop, no wallpaper | chafa missing (A1 opt-in / B2). |
| On tty1: no truecolor + no/laggy mouse, debug log shows `term=linux` | The autologin `TERM` is wrong. Set the override's last agetty arg to `xterm-256color`, not `$TERM` (see step 2). Confirm with `grep 'caps:' ~/.local/state/tetron-wm/debug.log` → want `truecolor=true term=xterm-256color`. |
| Colors look 256-ish on kmscon | `truecolor = true` in config.toml (and check `TERM` per the row above). |
| No mouse on a raw VT (no kmscon) | `sudo apt-get install -y gpm && sudo systemctl enable --now gpm`. Not needed under kmscon (disable it there: `sudo systemctl disable --now gpm`). |
| Boxes/tofu with `icon_style = "nerd"` | redo B4 / `fc-cache -f`, or use `ascii`. |
| Wallpaper glyph tofu on kmscon | console font lacks the sextant glyphs chafa uses — font coverage, not a bug; the DejaVu/Terminus fonts help. |
| tty1 login broken | from another VT/SSH: `sudo systemctl disable --now kmsconvt@tty1 && sudo systemctl enable --now getty@tty1`. |
| Desktop appears on tty1 but with no truecolor/mouse (debug log `term=linux`) | kmscon crashed and systemd fell back to `getty@tty1`. Check `journalctl -u kmsconvt@tty1 -b`. A `mod-*.so: undefined symbol` + `SEGV` means a **stale kmscon module** from an older build: `sudo rm -f /usr/local/lib/*/kmscon/mod-*.so` then re-run `install-kmscon.sh --source` (it now wipes old modules before reinstalling). |
| Debug the desktop | `~/.local/state/tetron-wm/debug.log`, or the in-app Logs viewer. |

---

## References

- Illustrated walkthrough (the "why"): [`tetron-wm_bare_console_guide.html`](tetron-wm_bare_console_guide.html)
- Installers: `install.sh`, `install-kmscon.sh`, `install-tetron-wm-suite.sh` (this repo)
- Config: [`CONFIG.md`](CONFIG.md) (reference and recipes), [`config.example.toml`](../config.example.toml)
- Maintained kmscon fork: https://github.com/Aetf/kmscon , https://github.com/Aetf/libtsm
- Nerd Fonts: https://github.com/ryanoasis/nerd-fonts · chafa: https://hpjansson.org/chafa/
