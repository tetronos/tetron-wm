# How to cast the tetron-wm desktop to a Chromecast

Casting is an optional addon. tetron-wm only streams its own screen, as lines of JSON, to the standard input of `casttui --cells`. [casttui](https://tetronos.github.io/casttui/) does everything else: it serves a web page that draws the cell grid sharply (no video encoding, almost no CPU), controls the Chromecast, and holds every network facing part, so tetron-wm itself opens no port. Without casttui installed the key, the launcher item and the settings stay hidden. casttui on its own can also cast a pixel desktop (X11, the console framebuffer) and any web page, see casttui `docs/HOWTO_casttui_cast_a_desktop_or_a_web_page_to_a_chromecast.md`.

## What you need

A Chromecast on the same LAN segment as the computer running tetron-wm, and casttui (`sudo apt install casttui` from the tetronos repository). The session needs an attached terminal, because the screen is only composed while a client is attached.

## Cast and stop

| Action | How |
| --- | --- |
| Start | leader then `o`, or the launcher item **Cast desktop**, or `tetron-wm cast` |
| Stop | leader then `o` again, click the red `CAST` marker in the menubar, or `tetron-wm cast off` |
| State | `tetron-wm cast status` (`off` or `casting`) |
| Another device | `tetron-wm cast --device NAME_OR_IP` |
| Watch in a browser | `tetron-wm cast --url` prints a URL for any browser on the LAN, with no TV involved |

`tetron-wm cast` returns in a few seconds, and the TV shows the desktop a few seconds later while the Chromecast loads the page. An idle desktop sends nothing, so only changes (a clock tick, typing, a window moving) appear as movement. `tetron-wm -s NAME cast` targets another session. Stopping quits the app on the TV, so it does not keep showing a frozen picture.

## The red CAST marker

While a cast runs, a red `CAST` stays in the menubar. It is never moved into the Info menu or hidden for space, and a click on it stops the cast. This is deliberate: the page shows the whole desktop, including anything typed or displayed in a terminal window.

## Settings

Both optional, in `config.toml` or `--help`. `cast_command`: the program that serves and casts the desktop, empty uses `casttui` (it must speak `--cells`, see the protocol below). `cast_fps`: at most this many updates per second, default 15. The device is `cast_device` (also used by the right-click file cast), empty uses casttui's remembered device. The leader key is the `cast` action in `[keys]` (default `o`). Full table in `docs/CONFIG.md`.

## Picture quality and size

The page mirrors the attached terminal size and scales the grid to the TV. A terminal of 140 columns or fewer reads best on a 1280x720 screen, and a larger terminal gives finer text and wallpaper. Block and box drawing characters are drawn by the page itself, so they have no gaps on any device.

## The stream protocol (v1)

For other sinks: the program is started as `PROGRAM --cells [-d DEVICE]` (or `--cells --no-cast` for a browser URL) and reads one JSON object per line from stdin. It must print one line on stdout when the cast is up (`casting cells on DEVICE`, or with `--no-cast` the URL) and exit when stdin closes. Events: `{"t":"hello","v":1,"cols":N,"rows":M}`; `{"t":"full","spans":[...]}` and `{"t":"diff","spans":[...]}` where a span is `[x, y, fg, bg, attrs, text]` with colours as `0xRRGGBBAA` and attrs as bits (bold 1, italic 2, underline 4, inverse 8); `{"t":"cursor","x":null,"y":null}` (the cursor is drawn into the cells); `{"t":"status","state":"detached"|"ok"|"stopped"}`. A resize sends a new `hello` and `full`. Unknown events and fields must be ignored.

## Security

Casting is opt in per use: nothing runs until you start it. casttui serves the page only on the LAN address that reaches the Chromecast, under a random 128 bit path, to the Chromecast and this computer, and to at most 4 viewers, and it ends the cast after 2 minutes with nobody watching. tetron-wm itself ends it after 60 seconds with no terminal attached and logs no URL. There is no TLS, so do not cast on a network you do not trust. `--url` prints a secret on purpose and says so.

## Troubleshooting

`no daemon running, or it is too old (run tetron-wm reload)`: start tetron-wm, or reload the running session so it knows the new commands. `casting needs casttui`: install it. A window titled **Cast** with an error: casttui failed, and the window shows its message (usually the Chromecast was not found, try `--device IP`, or casttui is too old to know `--cells`). The TV shows a blank page: wait for the first change on the desktop. The TV shows "tetron-wm is detached": attach a terminal again and the stream resumes. Logs: `tetron-wm` log lines starting with `cast:`.

## See also

`docs/KEYS_tetron-wm_leader_key_table.md` (the `o` key), `docs/CONFIG.md` (the cast settings), and the casttui howto in the casttui repository.
