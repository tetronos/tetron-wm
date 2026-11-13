"""✦ AI requirements for tetron-wm (WMAST-*): which agent CLI runs,
how it is chosen, what it is told, the code folder, and the optional MCP
server."""
from libspec import Requirement


class AgentsAreData(Requirement):
    """REQUIREMENT-ID: WMAST-001

    No agent CLI is named in the code except the default (`opencode`). An app
    in the catalog is an agent when its entry carries an `agent` table
    (`label`, `command`, `args`, `context_files`, `mcp`); `lists/tetron.toml`
    sets it in `[app.agent]` and `scripts/merge_catalog.py` checks the keys and
    carries it into `assets/catalog.json`, so an agent added on the lists
    reaches installed versions through the Store's Update list (WMCAT-004).
    Fields a binary does not know are ignored. `[[assistant_agent]]` tables in
    `config.toml` (same fields plus `id` and `env`) add agents or override a
    catalog agent field by field when `id` matches. `assistant_command` holds
    an agent id; a value that matches no id runs as a command line split on
    whitespace (values saved before this, such as `hermes` or a wrapper path,
    keep working). `assistant_args` is appended to the agent's own args. No
    flag that skips the agent's permission prompts is passed by default.
    """


class AgentPicker(Requirement):
    """REQUIREMENT-ID: WMAST-002

    Settings > AI > Agent opens a picker list instead of cycling:
    installed agents first, then agents not installed (choosing one opens the
    Store), then "Custom command…", which opens a text box and stores the
    command line in `assistant_command`. Up and Down move, typed text filters
    by label and command, Enter chooses, Esc closes, the list scrolls when
    longer than the space, and a click chooses the row under it (render and
    hit-test share one geometry). The row shows the current agent's label, or
    `custom: <command>`.
    """


class OneContextFile(Requirement):
    """REQUIREMENT-ID: WMAST-003

    At each launch of the AI agent the briefing is written as `AGENTS.md` in
    the AI folder (`~/.local/share/tetron-wm/assistant`). An agent whose
    `context_files` names other files gets each as a symbolic link to
    `AGENTS.md` (a copy where links fail); names are plain file names, never
    paths. No `CLAUDE.md` is written.
    """


class SuiteAndCodeBriefing(Requirement):
    """REQUIREMENT-ID: WMAST-004

    The briefing has a suite section (the shipped components: purpose,
    repository, how each is installed here) and a code section (where code
    lives, read the repository's own `AGENTS.md`, spec first, branch, test,
    install an own build with `install.sh` from the clone, `tetron-wm reload`,
    never push or bump versions). Placeholders are filled from local facts
    only (PATH, dpkg diversions, the install method), never the network.
    The code folder is `assistant_code_dir`, default `~/code`.
    """


class CodeFolder(Requirement):
    """REQUIREMENT-ID: WMAST-005

    `install.sh` and the suite installer create the code folder (`~/code`)
    when missing, and never touch it when present. The Files sidebar lists it
    as Code, after Home, at `assistant_code_dir` when set.
    """


class McpServer(Requirement):
    """REQUIREMENT-ID: WMAST-006

    `tetron-wm mcp` is an MCP server over stdio (JSON-RPC 2.0, newline
    delimited) that forwards to the running desktop through the control
    socket. It is never resident: the agent starts it and it exits with its
    stdin. `assistant_mcp` (default false, a Settings > AI row) turns
    the wiring on: at launch the agent's `mcp` entry says where it is told
    about the server, a file in the assistant working directory written from
    a named template (`json` for an `mcpServers` object, `opencode`)
    or args in which the words `MCP_JSON` and `MCP_BIN` in double braces are
    replaced by the path of a generated `json` file and of this binary. Only the assistant working directory is written, never the agent's
    own global config. With `assistant_mcp` off those files are removed at the
    next launch. An agent without an `mcp` entry is started without it. Every
    tool call is logged.
    """


class WorkingFolder(Requirement):
    """REQUIREMENT-ID: WMAST-007

    The Settings section and the agent's window are named AI. Settings > AI >
    Working folder picks where the agent starts (`assistant_cwd`): the code
    folder (unset, the default, created when missing), `ask` (the folder
    picker at each launch, rooted at the code folder), `ai` (the AI folder),
    or a path; a missing path falls back to the AI folder with a log line.
    Outside the AI folder the agent gets the briefing through the `briefing`
    entry of its agent table (file in the AI folder from a template, args, env,
    with `BRIEFING` and `AI_DIR` in double braces filled), and with MCP on
    through its `mcp` entry, briefing args before MCP args. Nothing is written
    into the working folder. An agent without a `briefing` entry starts there
    without the briefing, and Settings says so.
    """
