# tetron-wm — project memory for coding agents

tetron-wm is a window manager & desktop for the terminal, in Rust. Floating
windows, a top-panel taskbar, cascading launcher, app store (600+ curated
TUIs), file manager, desktop icons, menubar tray, mouse — rendered as cells
over a daemon/client socket, like a graphical tmux.

## Build, test, lint

```sh
cargo build           # MSRV: rust 1.95 (rust-version in Cargo.toml)
cargo test            # 300+ tests; keep them green
cargo clippy --all-targets   # warning-clean; keep it that way
```

There is no rustfmt enforcement; match the style of the file you're editing.
Clippy is currently warning-clean — keep it that way.

Packaging requirements are specs in `spec/` (libspec, pinned in `pyproject.toml` as a development-only dependency). Edit the spec first and review it with `uv run libspec diff` (`uv run libspec diff A B` between commits, `uv run libspec log` for history), then implement; `python3 reconcile.py` must exit 0 before a commit. The app sets live in the "App sets" section of `lists/tetron.toml`; after editing it run `scripts/rebuild-app-sets.sh`, which regenerates `assets/suite-apps.txt`, the recommends of `packaging/<set>.toml` and `assets/catalog.json` and checks them (reconcile fails until you do). Comments, docstrings and specs explain the code and the reason for it; who decided, when, and plans belong in DO-NOT-COMMIT notes and the CHANGELOG. Reconcile (CON-WD06) fails on the owner placeholder, a DO-NOT-COMMIT path, or a date in a comment in any committed file outside its allowlist.

## Versioning & release channels

`Cargo.toml` carries the semver version (currently 0.15.8); update `CHANGELOG.md`
in the same commit as user-visible changes (roll `[Unreleased]` into a new
`[x.y.z]` heading).

**Pre-bump checklist — do ALL of this BEFORE the version bump commit.** A bump
pushed to `main` publishes immediately and a published version can not be
amended (0.14.13 shipped without its `--help` text and needed a follow-up
release). So, for every user-visible change in the release:
1. **`--help` is updated** (`src/cli_help.rs`: the `text()` body, `CONFIG_KEYS`,
   `ENV_VARS`; key descriptions in `src/keymap.rs`). New commands, options,
   config keys, env vars, renamed menu items, and workflows (detach, sessions)
   must all appear. `cli_help` tests check config keys against `struct Config`
   and keep the session/detach topics in the text.
2. `README.md` (features, Controls, Configuration, Roadmap) matches, and so do
   `docs/CONFIG.md` (option table, recipes) and `config.example.toml` (a test
   checks every `--help` config key appears in both).
3. `CHANGELOG.md` has an entry for each change under `[Unreleased]`.
4. `cargo test` and `cargo clippy --all-targets` are clean.
5. Only then: bump `Cargo.toml` + `Cargo.lock`, roll the CHANGELOG, `CLAUDE.md`'s
   "currently x.y.z", and merge/push.

**Releases are automated — cut one by merging a version bump.** Bump
`Cargo.toml` + `Cargo.lock`, roll the CHANGELOG, open a PR, merge it.
`.github/workflows/release.yml` triggers on push to `main`; when the version is
new it tags `vX.Y.Z`, builds the four platform binaries, and publishes the
GitHub Release — no manual tag push or `workflow_dispatch` needed (this matters
because the session's automation can open/merge PRs but **cannot push tags or
dispatch workflows** — both return 403). Tag-push and manual dispatch still
work. A guard step **fails the build if the release tag and `Cargo.toml`
version disagree**, so a version-skewed release (which once shipped 0.2.3
binaries under a `v0.2.4` tag → an in-app update loop) can't ship again.

The in-app updater (`update_command` in `session.rs`) has two channels, chosen
by `config.update_branch`:
- **main** — downloads the latest prebuilt release via `install.sh` (fast),
  falling back to `cargo install --git`.
- **dev** — `cargo install --git --branch dev` (source build, for testing).

The updater reloads via the freshly-installed binary's **absolute path** (a
bare `tetron-wm reload` can miss `$PATH` in the non-interactive `sh -lc` it runs
in) and logs each step (`update: …`) to the log (`~/.local/state/tetron-wm/debug.log`). The debug log now
**appends across reloads** — `dbg_init` used to truncate on every daemon start,
wiping the very update trace needed to debug it. The long-standing "update
from Settings gets stuck" report (investigation log:
`DO-NOT-COMMIT/planning/plans/2026-06-14-update-stuck-investigation.md`) was root-
caused and fixed in 0.2.10: both `install.sh` and `check_for_updates()`
resolved the latest release via the unauthenticated, 60-req/hour
`api.github.com` REST endpoint, which answered a 403 once rate-limited — read
as "no published release yet" and silently dropped Settings' Update & Reload
into a multi-minute source build that looked hung. Both now resolve the tag
from the `github.com/OWNER/REPO/releases/latest` web redirect, which isn't
rate-limited, falling back to the REST API only if that redirect can't be
parsed.

## Architecture (the three processes)

```
client (tetron-wm)  ←socket→  daemon (tetron-wm --daemon)  ←socket→  apphost (tetron-wm --apphost)
thin renderer              owns SessionCore (all UI state)    owns the PTY apps
real terminal              composites frames, routes input    survives UI reloads
```

- **client** (`src/client.rs`, `src/main.rs`): sets up the terminal, turns
  `FrameMsg` diffs into ANSI, forwards input as `ClientMsg`. Holds NO session
  state — it routes keys using the `Flags` the daemon sends each frame.
  `main.rs` is also where ssh "system switches" run (the client owns the real
  terminal, so interactive prompts work there and nowhere else).
- **daemon** (`src/daemon.rs`): serves one client at a time; out-of-band
  control socket accepts any `ClientMsg` (the `tetron-wm launch/tile/theme/msg`
  CLI and the AI assistant use it).
- **session** (`src/session.rs`, the big one): `SessionCore` is the
  `ClientMsg`-in / frame-out boundary. All UI widgets live here as fields.
- **apphost** (`src/apphost/`, `src/ptyhost.rs`): PTY children behind the
  `AppHost` trait; `LocalAppHost` in-process, `RemoteAppHost` over a socket.
  Apps survive frontend reloads because this process never restarts. The
  emulator keeps 10k lines of scrollback; `AppInstance::scroll` moves the
  display offset and `snapshot()` reflects it automatically (the wheel routes
  here via `ClientMsg::ScrollAt`).

## Key seams and conventions

- **Protocol changes**: every new `FrameMsg`/`Flags`/apphost-`Frame` field
  must be `#[serde(default)]` so version skew (old daemon ↔ new client, old
  apphost ↔ new frontend) degrades gracefully. Prefer new fields on existing
  messages over new enum variants (unknown variants fail the whole parse).
  Bump `PROTO_VERSION` (apphost/proto.rs) on any apphost protocol change; if
  a change genuinely breaks older apphosts, ALSO bump `MIN_COMPAT` — that
  arms the post-update safety dialog ("restart the app server, closes your
  apps") instead of letting users' sessions break silently. Never bump
  `MIN_COMPAT` casually: it forces users through an app-closing restart.
- **New windowed widgets** follow the Store/Settings/Logs pattern:
  a `WinContent` variant + `focused_is_X()` flag + `ClientMsg::X*` variants +
  a routing branch in `client.rs`. Widgets are pure state+render; they never
  touch sockets.
- **Shelling out**: only via `crate::system::run_capped` (hard timeout).
  Never block the render loop on the network — see the poller's separate
  thread for slow probes.
- **Logging**: `crate::dbg_log` (`info`), `log_debug`, `log_warn`, `log_error` (src/logfile.rs: `~/.local/state/tetron-wm/debug.log`, 4 x 4 MiB rotation, `log_level` config). Test binaries never write to it unless `TETRON_WM_LOG_DIR` is set.
  Log every external effect (ssh, scp, installs, spawns) with enough detail
  to debug from the log alone; users paste it via the in-app Logs viewer.
- **Geometry/hit-testing**: render and hit-test must share the same rect
  functions (see `powermenu.rs`'s "Geometry" section) so they can never drift.
- **Secrets**: passwords ride env vars (`SSHPASS`) for one-shot use, never
  argv, never disk, never the log.

## The AI assistant (✦)

`src/assistant.rs` + the `agent/` folder (Settings section and window: AI).
`agent/*.md` is the single source of truth for everything the in-app agent is
told — embedded via `include_str!`, stamped into
`~/.local/share/tetron-wm/assistant/` (the AI folder) at launch as `AGENTS.md`.
Agents are data, never code: catalog entries with an `[app.agent]` table in
`lists/tetron.toml` plus `[[assistant_agent]]` config tables, merged by id
(`assistant::agents`); only `DEFAULT_AGENT` (opencode) is named in Rust. Each
agent's `briefing` and `mcp` wiring (file from a template in the AI folder,
args, env) says how it gets the pack outside the AI folder and the
`tetron-wm mcp` server (`src/mcp.rs`, off by default); nothing is ever written
into the agent's working folder (`assistant_cwd`, default `~/code`). Add a new
agent CLI by editing the list, not the code.

## Tests

Integration tests live in `tests/*_tests.rs`, unit tests inline. UI tests
drive widgets through their public state machines (clicks at computed rects,
messages applied to `SessionCore`) — no terminal needed. When you change a
menubar/taskbar layout, check `tests/chrome_tests.rs` for byte-vs-char column
assumptions (the bar contains multi-byte glyphs).

## Docs

- `README.md` — user-facing; keep "What works today", Controls, Configuration,
  and Roadmap in sync with shipped features.
- `agent/` — what the in-app assistant is told (see its README).
- `DO-NOT-COMMIT/planning/{specs,plans}` — dated historical design records (kept
  out of the public repo); append new ones, don't retrofit old ones.
