# tetron-wm leader key table

The leader key exists so tetron-wm shortcuts never collide with your terminal, the operating system, or the focused app. Press the leader (default `Ctrl+Space`), release it, then press one key. The key is consumed by tetron-wm and never reaches the app. Source of truth: the `DEFAULTS` table in `src/keymap.rs`; this page is kept in sync with it by hand, so if the two differ the code wins.

## How it works

- Press `Ctrl+Space`, release, then the key. It is a two-step sequence, not a chord, so no modifier needs to be held with the second key.
- Any key that is not bound after the leader does nothing, and the leader state is cleared.
- Uppercase keys mean Shift plus the letter after the leader, for example leader then `C` is Shift+c.
- Rebind the leader with `leader = "ctrl+b"` in `config.toml` (only `ctrl+<key>` chords are accepted; a bare key would steal normal typing). Rebind an action with the `[keys]` table, for example `launcher = "l"` or `launcher = ["l", "enter"]`. A change needs `tetron-wm reload`. Details: `docs/CONFIG.md`.
- If a key you set is invalid, the default stays and a warning is logged. If two actions claim the same key, the earlier row in the table below keeps it.

## Table

| Key after leader | Action name (for `[keys]`) | What it does |
| --- | --- | --- |
| `space` | `spotlight` | Spotlight search overlay: type to filter apps, up and down to move, Enter to launch |
| `l`, `enter` | `launcher` | Open the app menu (the cascading launcher, the same as clicking the top-left button) |
| `m`, `M` | `maximize` | Maximize or restore the focused window |
| `n`, `N` | `minimize` | Minimize the focused window to the taskbar |
| `tab` | `focus_next` | Focus and raise the next window in taskbar order, wrapping. Minimized windows are skipped (if all are minimized, one is restored). After it, bare `Tab` / `Shift+Tab` keep cycling without the leader; Enter or Esc ends the cycle, any other key ends it and goes to the app |
| `backtab` | `focus_prev` | Same as `focus_next`, backwards (leader then `Shift+Tab`) |
| `f` | `windows` | Window picker: a numbered list of every window, minimized ones included. Up/Down, `Tab`/`Shift+Tab` or `j`/`k` move, Enter or a digit `1` to `9` focuses (and restores) that window, Esc or `q` closes, a click picks a row |
| `w` | `move` | Keyboard move mode for the focused window: the title shows a hint, arrows move it one cell (`Shift`+arrow five, where the terminal reports Shift), `r` switches to resizing (arrows move the right and bottom edges) and back, Enter keeps the result, Esc puts the window back exactly as it was (a maximized window comes back maximized), a click also ends it. Other keys are ignored while it is on |
| `=` | `volume` | Open the menubar volume popover (leader `=` again closes it). Left / Right (or `-` / `=` / `+`) change the volume in 5% steps, `m` or Space mutes, Down then Enter opens the mixer program, Esc closes. The same keys work when the popover was opened with a click. It changes the volume of the machine tetron-wm runs on (over ssh, the remote one) |
| `-` | `brightness` | Open the menubar brightness popover (leader `-` again closes it). Left / Right (or `-` / `=` / `+`) change the screen brightness in 5% steps, Enter or Esc closes. Only on a machine with a backlight (a laptop panel); elsewhere there is no brightness segment and the key does nothing. Settings > Display has the same control |
| `i` | `info` | Info menu: session, version, tray details; arrows and Enter open Sessions, Screensaver, Logs, Shortcuts, Settings; Esc closes |
| `esc` | `close` | Close the focused window (app windows ask for confirmation first; built-in panels close at once) |
| `[`, `left` | `snap_left` | Snap the focused window to the left half of the screen |
| `]`, `right` | `snap_right` | Snap the focused window to the right half of the screen |
| `t` | `tile` | Tile all windows into the grid once |
| `.` | `context_menu` | Right-click menu for the selected file in Files, or on the desktop when no window is focused; arrows and Enter pick, Esc closes |
| `T` | `auto_tile` | Toggle auto-tile mode (new windows re-tile the grid automatically) |
| `1` to `9` | none (fixed) | Send the focused window to grid cell N (row-major, 1-based) |
| `r`, `R` | `rename` | Rename the focused window |
| `S` | `store` | Open the app store |
| `,` | `settings` | Open Settings |
| `A` | `activity` | Open the activity monitor |
| `C` | `clipboard` | Open the clipboard history popover |
| `x`, `y` | `select` | Select mode: the next mouse drag selects text even in apps that capture the mouse (one shot) |
| `c` | `copy` | Copy the mouse selection to the clipboard (same as Ctrl+Shift+C where the terminal allows it) |
| `v`, `p` | `paste` | Paste the clipboard into the focused app (same as Ctrl+Shift+V) |
| `z` | `screensaver` | Start the screensaver (any key or mouse wakes it; effect from `screensaver` in the config) |
| `s` | `screenshot` | Save a PNG screenshot of the whole composited screen to `screenshot_dir` (default `~/Pictures`, else `$HOME`) |
| `?`, `h` | `help` | Show the shortcut help screen |
| `q` | `detach` | Detach the client; the daemon and all apps keep running |
| `Q` | `shutdown` | Shut down the daemon (all apps close) |

## Planned

| Key after leader | Action name | What it will do |
| --- | --- | --- |
| `d` | `screen_dump` | Save a cell dump of the screen (`.ansi` and `.cells.json.gz`) |

## Free keys

These keys are unused after the leader and safe to bind in `[keys]` or to claim for new features (defaults only; your own `[keys]` overrides change this).

| Type | Free |
| --- | --- |
| lowercase | `a b d e g j k o u` |
| uppercase | `B D E F G H I J K L O P U V W X Y Z` |
| digit | `0` |
| symbols | `` ` ; ' / \ ! @ # $ % ^ & * ( ) _ + { } | : " < > ~ `` |

`d` is reserved for the planned screen dump above, and `g` is reserved for the planned grid prefix (`g` then `1` to `9` sends a window to a cell).

## Other rules

- tetron-wm never binds terminal control keys in its own UI: no Ctrl+C, Ctrl+D, Ctrl+P and no function keys. Direct shortcuts use Alt+letter, Insert and Delete, or the leader.
- Leader then `Esc` closes the focused window, but while a dialog or popover is open it is that dialog's key instead (cancel or close), and it never closes a window then.
- Dialogs have their own keys while open: Enter or `y` confirms and Esc or `n` cancels (power and forget dialogs); the clipboard history popover takes typing to filter, then `Alt+c` or leader `c` to copy without pasting, `Alt+p` or leader `p` or `Insert` to pin, and `Alt+d` or leader `d` or `Delete` to remove an entry.
- Settings: `Tab` moves focus between the section list and the rows (like mc's panels). In the list, Up / Down pick a section and Right or Enter opens it; in the rows, Up / Down pick a row, Left / Right change it, Enter or Space toggles or activates. PageDown / PageUp switch section from either pane. Esc closes.
