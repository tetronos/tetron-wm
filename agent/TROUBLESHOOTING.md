# Troubleshooting & fixing tetron-wm

## First stop: the log

`~/.local/state/tetron-wm/debug.log` — always on, timestamped (ms since epoch), rotated at 4 MiB into `debug.log.1` .. `.3` (16 MiB kept). `log_level` in config.toml (`error`, `warn`, `info`, `debug`) sets how much; `TETRON_WM_DEBUG=1` forces `debug` for one run.
Every subsystem traces here: app launches, installs, ssh/scp transfers,
system switches, notifications, theme changes. `tail -100` it before
theorising. The user can also open it in-app (launcher → tetron-wm → Logs) and
copy it to their clipboard with `c`.

## Common problems

- **App won't install from the Store**: the install runs in a visible shell
  window — read its output. Usually a missing toolchain (Go/Rust/Node/Python);
  the Store offers to install toolchains, or install via the package manager.
- **App needs models/providers configured** (AI agents like opencode, aider):
  they have their own config/onboarding — run their setup in a
  shell window and follow their docs. The Store's detail pane shows a setup
  tip for these apps.
- **Garbled rendering**: ask the user to resize the terminal once (forces a
  full re-baseline) or run `tetron-wm reload`. If it persists, collect the log.
- **Remote system unreachable**: check the Systems menu dot, then
  `ssh -o BatchMode=yes <target> true`; "Permission denied" means the key
  isn't installed — re-run Systems → Add Remote.
- **Mouse doesn't work on a bare Linux console**: gpm must be running
  (`sudo systemctl enable --now gpm`).

## Fixing tetron-wm itself

See "Working on the suite's code" below: clone into the code folder, fix on a
branch with a regression test, run your own build, reload. Released fixes
reach the user through Settings → Updates.
