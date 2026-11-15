# How to cast the tetron-wm desktop to a Chromecast

Casting is split between two programs. tetron-wm serves its own desktop as a live web page that draws the cell grid sharply (no video encoding, almost no CPU), and it asks [casttui](https://tetronos.github.io/casttui/) (or catt) to put that page on the TV. casttui on its own can also cast a pixel desktop (X11, the console framebuffer) and any web page. That side is in casttui `docs/HOWTO_casttui_cast_a_desktop_or_a_web_page_to_a_chromecast.md`.

## What you need

A Chromecast on the same LAN segment as the computer running tetron-wm, and either casttui or [catt](https://github.com/skorokithakis/catt) installed. With `cast_command = ""` tetron-wm uses casttui when it is installed, else catt. The session needs an attached terminal, because the screen is only composed while a client is attached.

## Cast and stop

| Action | How |
| --- | --- |
| Start | leader then `o`, or the launcher item **Cast desktop**, or `tetron-wm cast` |
| Stop | leader then `o` again, click the red `CAST n` marker in the menubar, or `tetron-wm cast off` |
| State | `tetron-wm cast status` (`off`, or `casting, N viewer(s)`) |
| Another device | `tetron-wm cast --device NAME_OR_IP` |
| Watch in a browser | `tetron-wm cast --url` prints a URL for any browser on the LAN, with no TV involved |

`tetron-wm cast` returns in a few seconds, and the TV shows the desktop about 5 to 10 seconds later while the Chromecast loads the page. The TV stays up while the screen is idle. An idle desktop sends nothing, so only changes (a clock tick, typing, a window moving) appear as movement. `tetron-wm -s NAME cast` targets another session.

## The red CAST n marker

While a cast runs, a red `CAST n` (n = viewers) stays in the menubar. It is never moved into the Info menu or hidden for space, and a click on it stops the cast. This is deliberate: the page shows the whole desktop, including anything typed or displayed in a terminal window.

## Settings

All optional, in `config.toml` or `--help`. `cast_device`: the device name or IP (as `casttui -d` or `catt -d` take it), empty uses the tool's own default. `cast_command`: `casttui`, `catt`, or a path to either; empty picks casttui if installed, else catt. `cast_port`: the viewer port, 0 for a random free one. `cast_fps`: at most this many updates per second, default 15. `cast_idle_stop`: seconds with no viewer before the cast stops, default 120, 0 never. The leader key is the `cast` action in `[keys]` (default `o`). Full table in `docs/CONFIG.md`.

## Picture quality and size

The page mirrors the attached terminal size and scales the grid to the TV. A terminal of 140 columns or fewer reads best on a 1280x720 screen, and a larger terminal gives finer text and wallpaper. Block and box drawing characters are drawn by the page itself, so they have no gaps on any device.

## Security

Casting is opt in per use: nothing listens until you start it. The viewer binds only the LAN address that routes to the device, answers GET only, has one random 128 bit token in the path (any other path returns 404 with no body), allows at most 4 viewers, and refuses other addresses when the device IP is known. It stops when you stop it, after 60 seconds with no terminal attached, and after `cast_idle_stop` seconds with no viewer. The token and the full URL are never logged. There is no TLS, so do not cast on a network you do not trust.

## Troubleshooting

`no daemon running, or it is too old (run tetron-wm reload)`: start tetron-wm, or reload the running session so it knows the new commands. `casttui or catt is needed to cast`: install casttui (`sudo apt install casttui` from the tetronos repository) or `uv tool install catt`. A window titled **Cast** with an error: the cast command failed, and the window shows its message (usually the device was not found, try `--device IP`). The TV shows a blank page: wait for the first change on the desktop, or check `tetron-wm cast status` shows a viewer. The screen shows "tetron-wm is detached": attach a terminal again, and the stream resumes. Logs: `tetron-wm` log lines starting with `cast:`, never containing the URL.

## See also

`docs/KEYS_tetron-wm_leader_key_table.md` (the `o` key), `docs/CONFIG.md` (the cast settings), and the casttui howto in the casttui repository.
