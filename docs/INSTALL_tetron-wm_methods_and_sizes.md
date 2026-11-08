# Installing tetron-wm: methods, sizes and setups

Three separate choices make up an install. **How** tetron-wm arrives (the suite installer, `install.sh`, the `.deb` packages, or a source build). **How much** comes with it (the size: minimal, desktop or extras). And, on Linux, **how the console is set up** (nothing, kmscon, or tetron-wm at login or at boot). This page covers each, then shows them together in one table.

## The short version

For most people, one command, then answer two questions (the size, then what kind of machine this is):

```sh
curl -fsSL https://tetronos.github.io/tetron-wm/install.sh | bash
```

Press Enter at each question to take the default: the desktop size, and the setup that suits the machine. Running it again later is safe; it only adds what is missing.

## Sizes

| Size | What you get |
|---|---|
| minimal | tetron-wm only. Everything works that needs no other program; features that need one (image wallpaper, video, PDF pages, sound controls, ...) stay off until that program is installed. |
| desktop (default) | tetron-wm plus the tools its features use: tetron-browser (Debian and Ubuntu amd64, from the [tetronos apt repo](https://tetronos.github.io/apt)), chafa, mpv, poppler-utils, w3m, htop, upower, gpm, xclip and wl-clipboard (with a desktop session), alsa-utils, openssh-client, sshpass, sshfs, 7zip, unzip, gnupg, python3, curl, console fonts, the DejaVu font, mc and vim; also fresh, uv, catt, casttui and the Symbols Nerd Font, which no distribution packages. |
| extras | desktop plus fbi and git. |

System services (PipeWire for sound, NetworkManager for Wi-Fi, BlueZ for Bluetooth) are never part of a size: the suite installer asks about them separately, defaulting to no, because they change how the machine manages sound, network and Bluetooth. No size installs an SSH server.

The sizes are the same everywhere: the suite installer's list, the `tetron-wm-desktop` and `tetron-wm-extras` packages and the Store all come from one set of files in [`packaging/`](../packaging/).

## Ways to install

**The suite installer** (`install-tetron-wm-suite.sh`) is the front door. It installs tetron-wm, the apps of the size you choose and, if you ask, the console setup. On Debian and Ubuntu it uses the `.deb` packages when the release has them, so apt manages tetron-wm and the whole app set; elsewhere it uses the system's package manager (dnf, pacman, Homebrew). Options: `--size minimal|desktop|extras`, `--profile gui|console|login|appliance`, `-y` (take every default), `--dry-run` (print the plan, change nothing), `--help`.

**`install.sh`** installs tetron-wm only: the `.deb` on Debian and Ubuntu when the release has one, otherwise the prebuilt binary in `/usr/local/bin` (or `TETRON_WM_BIN_DIR`). When it finishes it prints the suite installer command for the apps.

```sh
curl -fsSL https://raw.githubusercontent.com/tetronos/tetron-wm/main/install.sh | sh
```

**The `.deb` packages**, from the [releases page](https://github.com/tetronos/tetron-wm/releases), for Debian 12 and later and Ubuntu 22.04 and later:

| Package | What it installs |
|---|---|
| `tetron-wm_<version>_amd64.deb` (or `_arm64`) | tetron-wm, `tetron-wm-startpage`, `tetron-aimode` and the docs. Adds no other package. |
| `tetron-wm-desktop_<version>_all.deb` | the desktop size, as Recommends (apt installs Recommends by default; each can be removed later). |
| `tetron-wm-extras_<version>_all.deb` | the extras size, on top of desktop. |

```sh
sudo apt install ./tetron-wm_<version>_amd64.deb ./tetron-wm-desktop_<version>_all.deb
```

This gives the same packages as the suite installer, without the apps no distribution packages (fresh, uv, catt, casttui, the Nerd Font) and without the console setup. On a machine with no mouse device (`/dev/input/mice` missing: headless servers, many VMs), add `gpm-` to that command: Debian's gpm, the console mouse, fails to install there. The suite installer does this for you, and Systems > Add Remote installs gpm on a remote only when it has a mouse device. Over ssh the mouse comes from your own terminal, so a remote never needs gpm for that.

**From source**, with a [Rust toolchain](https://rustup.rs) 1.95 or newer: `cargo install --git https://github.com/tetronos/tetron-wm`. tetron-wm only.

## Console setups (Debian and Ubuntu)

Asked by the suite installer as "What are you setting up?", or given with `--profile`:

| Setup | What it does |
|---|---|
| gui | A machine with a graphical desktop: tetron-wm runs in a terminal window. No console changes. |
| console | Installs kmscon (a console with truecolor and mouse) on a text console with a normal login prompt; you start `tetron-wm` yourself. |
| login | The same, and tetron-wm starts after you log in on that console. |
| appliance | kmscon, automatic login and tetron-wm at boot. Only with `--profile appliance`: anyone at the keyboard gets your account. |

`--uninstall` reverses the console changes. The `.deb` packages never change the console.

## Everything together

| Method | minimal | desktop | extras | Console setups | Updated by |
|---|---|---|---|---|---|
| suite installer, Debian and Ubuntu | yes | yes (the `tetron-wm-desktop` package) | yes (`tetron-wm-extras`) | all | Settings > Updates (through apt) |
| suite installer, Fedora, Arch | yes | yes (dnf, pacman) | yes | none yet (the kmscon installer supports Debian and Ubuntu only) | Settings > Updates |
| suite installer, macOS | yes | yes (Homebrew) | yes | none (no text console) | Settings > Updates |
| `install.sh` alone | yes | add later with the suite installer or Settings | add later | none | Settings > Updates |
| `.deb` by hand | `tetron-wm` | add `tetron-wm-desktop` | add `tetron-wm-extras` | none | Settings > Updates (through apt) |
| source build | yes | add later with the suite installer | add later | none | `cargo install` again |

## Growing the size later

- Inside tetron-wm: **Settings > General > Apps** shows the installed size. Left and Right choose a larger one and Enter installs it in a terminal window, where sudo can ask for your password. At extras it checks for apps a newer release added.
- From a shell: run the suite installer again with `--size desktop` or `--size extras`.

Going down a size on Debian and Ubuntu: `sudo apt remove tetron-wm-extras` (or `tetron-wm-desktop`); the apps it brought stay until `sudo apt autoremove`.

## Updating

Settings > Updates, or `install.sh` again. On Debian and Ubuntu with the package installed, the update installs the new release `.deb` with apt, and also moves `tetron-wm-desktop` and `tetron-wm-extras` to the release when they are installed, so tools added to a size arrive with the update. A tetron-wm installed without the package is updated in place; the suite installer offers to convert it to the package (`--convert`).

## Removing

| Installed with | Remove with |
|---|---|
| the `.deb` packages | `sudo apt remove tetron-wm-extras tetron-wm-desktop tetron-wm`, then `sudo apt autoremove` for the apps |
| the suite installer or `install.sh` | `install-tetron-wm-suite.sh --uninstall --purge`: console changes, then tetron-wm (through apt when it is the package, with its desktop and extras packages) and `~/.config/tetron-wm`, each confirmed; apps stay installed as ordinary packages |
| source | `cargo uninstall tetron-wm` |

The suite installer records its choices (size, method, console setup) in `~/.config/tetron-wm/suite-state`; Settings > About shows the method and size.
