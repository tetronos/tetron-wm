# agent/ — the tetron-wm desktop assistant's instruction pack

This folder is the **source of truth for everything the AI assistant is told**.
It is embedded into the tetron-wm binary at build time (`include_str!`) and stamped
into the assistant's working directory (`~/.local/share/tetron-wm/assistant/`) on
every launch, with live placeholders (host name, saved systems, version)
filled in.

The agent CLI is chosen in **Settings → AI → Agent** (opencode by
default). Agents are data: app catalog entries with an `agent` table
(`lists/tetron.toml`) and `[[assistant_agent]]` tables in config.toml. Every
one of them runs in the assistant's working directory, where the pack is
written as `AGENTS.md`, the file opencode, Codex, pi, Claude Code (2.1.277 and
later) and Antigravity CLI read on startup. An agent that reads another file
name lists it in `context_files` and gets a link to `AGENTS.md`.

The agent starts in `assistant_cwd` (default the code folder, `~/code`). Outside
the AI folder it gets the pack through the `briefing` entry of its catalog
agent table: opencode through `OPENCODE_CONFIG` (a config in the AI folder
listing `AGENTS.md` under `instructions`), Claude Code through
`--append-system-prompt-file`, pi through `--append-system-prompt`. Nothing
is written into the working folder. An agent without such an entry gets the
pack only in the AI folder.

With `assistant_mcp` on, the agent is also told to start `tetron-wm mcp`,
which gives it the desktop as MCP tools.

Editing a file here changes what the agent is told, after a rebuild.

## Files

- `BRIEFING.md` — identity and role (who the agent is, where it runs)
- `DESKTOP.md` — driving the desktop via the `tetron-wm` control CLI
- `SYSTEMS.md` — operating across the user's machines over ssh/scp
- `TROUBLESHOOTING.md` — logs, common failures, fixing tetron-wm itself
- `SUITE.md` — the shipped tetron suite parts and which are installed here
- `CODE.md` — working on suite code: code folder, spec first, own build, reload
- `RULES.md` — ground rules (safety, output width, confirmations)

Placeholders substituted at launch: `{{HOST}}`, `{{VERSION}}`, `{{SHA}}`,
`{{REPO}}`, `{{SYSTEMS}}` (the saved-systems table from systems.toml),
`{{CODE_DIR}}` (`assistant_code_dir`), `{{INSTALL}}` (install method),
`{{SUITE}}` (suite commands installed here).
