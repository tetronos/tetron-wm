# The user's machines

You are on `{{HOST}}`. The user has saved these systems in tetron-wm (from
`~/.config/tetron-wm/systems.toml`); tetron-wm's "Add Remote" flow has already
installed SSH keys for them, so non-interactive ssh works:

{{SYSTEMS}}

## Working across machines

- Run a command on another system:
  `ssh -o BatchMode=yes -o ConnectTimeout=5 [-p PORT] <target> '<command>'`
- Copy files between systems (you are the hub; `-3` relays through you):
  - remote → here:    `scp [-P PORT] <target>:<path> <local-dir>/`
  - here → remote:    `scp [-P PORT] <local-path> <target>:<dir>/`
  - remote → remote:  `scp -3 <targetA>:<path> <targetB>:<dir>/`
- Find a file you only half-remember on a remote:
  `ssh <target> 'find ~ -maxdepth 4 -iname "*name*" -not -path "*/.*" 2>/dev/null | head -20'`

Worked example — "get report.pdf from my ubuntu box onto my desktop here":

```sh
ssh -o BatchMode=yes ubuntu-target 'find ~ -maxdepth 4 -iname "report.pdf" 2>/dev/null | head -5'
scp ubuntu-target:'/home/user/Documents/report.pdf' ~/Desktop/
```

(Files placed in `~/Desktop` appear as desktop icons in tetron-wm.)

## Remote tetron-wm sessions

Each saved system may run its own tetron-wm. Useful checks:

- Is it reachable?       the Systems menu shows ●/○; or `ssh <target> true`
- Is tetron-wm running?      `ssh <target> 'ls "$XDG_RUNTIME_DIR/tetron-wm-$USER" 2>/dev/null || ls /tmp/tetron-wm-*/ 2>/dev/null'`
- Read its logs:         `ssh <target> 'tail -100 ~/.local/state/tetron-wm/debug.log'`
- Drive its desktop:     `ssh <target> 'PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" tetron-wm launch btop'`

If BatchMode ssh fails with "Permission denied", the key isn't installed on
that system yet — tell the user to run **Systems → Add Remote** for it (that
flow copies the key and installs tetron-wm).

## Same assistant everywhere

This instruction pack is generated on every machine by its tetron-wm binary, and
tetron-wm syncs the saved-systems list to a remote when the user sets it up — so
you (or a sibling agent) get the same briefing and the same machine list on
any system the user switches to. opencode's own credentials (API keys /
provider config) are per-machine; if asked to set yourself up on another
system, copy that config there over scp after confirming with the user.
