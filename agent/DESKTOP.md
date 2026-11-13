# Driving the desktop

These commands talk to the running tetron-wm daemon (same user, local socket).
They are how you open windows and arrange the user's desktop.

- `tetron-wm launch <command> [args…]`   open a new app window running <command>
  - Launching a catalog-tagged **CLI tool** (gum, himalaya, khal, dust, …)
    with **no args** automatically opens a shell with the tool's `--help`
    printed first, instead of a window that prints usage and dies. Passing
    args runs the command exactly as given.
- `tetron-wm tile`                       tile all windows into the configured grid
- `tetron-wm theme <name>`               switch theme (newmoon|midnight|nord|gruvbox|dracula|pastoral)
- `tetron-wm reload`                     reload the UI (apps keep running)
- `tetron-wm msg '<json>'`               raw control message (ClientMsg JSON), e.g.

```sh
tetron-wm msg '"MaximizeFocused"'
tetron-wm msg '{"SnapFocused":"Left"}'
tetron-wm msg '{"SendToCell":3}'
tetron-wm msg '"FocusNext"'          # focus the next window (also "FocusPrev")
tetron-wm msg '{"Launch":{"name":"btop","command":"btop","args":[]}}'
```

Examples of arranging a workspace:

```sh
tetron-wm launch btop          # system monitor
tetron-wm launch lazygit       # git UI
tetron-wm tile                 # arrange everything into the grid
```

Apps are installed via the in-app Store (600+ curated TUIs), or you can
install them yourself with the user's package manager and then `tetron-wm launch`.

Other things worth knowing when helping the user:
- Keyboard-only use: the leader is `Ctrl+Space` (press, release, then a key).
  Leader `Tab` / `Shift+Tab` cycles window focus, leader `f` opens a numbered
  window picker, leader `w` moves the focused window with the arrows (`r`
  resizes, Enter keeps, Esc undoes). Leader `=` opens the volume popover.
  Leader `?` shows every shortcut.
- Sound: Settings → Sound sets volume, mute and the output device (HDMI,
  headphones, speakers) through PipeWire's `wpctl`.
- Scroll the mouse wheel over any app window to read its scrollback; typing
  jumps back to the live bottom.
- Updating tetron-wm is **Settings → Updates** (default channel downloads the
  latest prebuilt release; a "dev" channel builds the dev branch from source).
- The version is in `Cargo.toml`; release notes are in `CHANGELOG.md`.

## Desktop tools over MCP

When the user turns on Settings → Assistant → Desktop tools (MCP), you also
get `tetron-wm` MCP tools (list windows, read a window's text, launch, focus,
close, tile, theme, notify, screenshot). Prefer them when present: they
answer in structured form. Without them, use the `tetron-wm` commands above.
