//! The desktop AI assistant: an agent CLI hosted in a persistent panel or
//! window, opened from the ✦ menubar button.
//!
//! The agent is just another apphost PTY app, so the chat survives detach and
//! frontend reloads like every other window. Its instructions come from the
//! repo's `agent/` pack (embedded at build time), stamped as `AGENTS.md` into a
//! dedicated working directory on every launch with live placeholders filled.
//! That directory is forced as the agent's cwd; `AGENTS.md` is the file the
//! agent CLIs read on startup.
//!
//! Which agents exist is data, not code (WMAST-001): a catalog entry with an
//! `agent` table, then `[[assistant_agent]]` tables in config.toml, merged by
//! id. The AI CLI market changes quickly, so a new agent arrives with the
//! Store's Update list or one config table, never a release. Only the default
//! is named here.

use crate::catalog::McpWiring;
use crate::config::{AgentDef, Config};
use std::path::{Path, PathBuf};

// The instruction pack, embedded from the repo's agent/ folder.
const BRIEFING_MD: &str = include_str!("../agent/BRIEFING.md");
const DESKTOP_MD: &str = include_str!("../agent/DESKTOP.md");
const SYSTEMS_MD: &str = include_str!("../agent/SYSTEMS.md");
const TROUBLESHOOTING_MD: &str = include_str!("../agent/TROUBLESHOOTING.md");
const SUITE_MD: &str = include_str!("../agent/SUITE.md");
const CODE_MD: &str = include_str!("../agent/CODE.md");
const RULES_MD: &str = include_str!("../agent/RULES.md");

/// The agent tetron-wm runs when `assistant_command` is unset.
pub const DEFAULT_AGENT: &str = "opencode";

/// The JSON file handed to agents whose MCP args name `{{MCP_JSON}}`.
const MCP_JSON_FILE: &str = "tetron-wm-mcp.json";

/// The shipped suite parts that install a command, for the briefing's
/// "on this machine" line.
const SUITE_BINS: &[&str] = &[
    "tetron", "tetron-wm", "tetron-browser", "tetron-tui", "tetron-webui", "tetron-systray",
    "tetron-messageboard", "tetron-hosts", "tetron-sync-receiver", "tetron-veilid", "casttui",
];

/// One agent the Assistant can run, after the catalog and config merge.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Agent {
    /// What `assistant_command` stores: the catalog `bin`, or the config `id`.
    pub id: String,
    pub label: String,
    pub command: String,
    pub args: Vec<String>,
    pub context_files: Vec<String>,
    pub env: Vec<(String, String)>,
    pub mcp: Option<McpWiring>,
    /// The catalog app name, for the Store's Install (None: config only).
    pub app: Option<String>,
}

impl Agent {
    /// The command to run: the agent's own, wrapped in `env` when it sets
    /// variables (the app server spawns a plain argv).
    pub fn argv(&self, extra: &[String]) -> (String, Vec<String>) {
        let mut args: Vec<String> = self.args.iter().chain(extra).cloned().collect();
        if self.env.is_empty() {
            return (self.command.clone(), args);
        }
        let mut wrapped: Vec<String> = self.env.iter().map(|(k, v)| format!("{k}={v}")).collect();
        wrapped.push(self.command.clone());
        wrapped.append(&mut args);
        ("env".into(), wrapped)
    }

    pub fn installed(&self) -> bool {
        agent_available(&self.command)
    }
}

/// Catalog agents, then config tables applied by id (WMAST-001).
pub fn agents(defs: &[AgentDef]) -> Vec<Agent> {
    let mut out: Vec<Agent> = crate::catalog::catalog()
        .iter()
        .filter_map(|a| a.agent.as_ref().map(|s| (a, s)))
        .map(|(a, s)| Agent {
            id: a.bin.clone(),
            label: if s.label.is_empty() { a.name.clone() } else { s.label.clone() },
            command: if s.command.is_empty() { a.bin.clone() } else { s.command.clone() },
            args: s.args.clone(),
            context_files: s.context_files.clone(),
            env: Vec::new(),
            mcp: s.mcp.clone(),
            app: Some(a.name.clone()),
        })
        .collect();
    for d in defs.iter().filter(|d| !d.id.trim().is_empty()) {
        match out.iter_mut().find(|a| a.id == d.id) {
            Some(a) => apply(a, d),
            None => {
                let mut a = Agent { id: d.id.clone(), label: d.id.clone(), command: d.id.clone(), ..Agent::default() };
                apply(&mut a, d);
                out.push(a);
            }
        }
    }
    out
}

/// Override the fields `d` sets.
fn apply(a: &mut Agent, d: &AgentDef) {
    if let Some(v) = &d.label { a.label = v.clone(); }
    if let Some(v) = &d.command { a.command = v.clone(); }
    if let Some(v) = &d.args { a.args = v.clone(); }
    if let Some(v) = &d.context_files { a.context_files = v.clone(); }
    if d.mcp.is_some() { a.mcp = d.mcp.clone(); }
    a.env = d.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
}

/// The agent `assistant_command` names: an agent id, else a command line
/// split on whitespace (a custom agent with no extras).
pub fn selected(cfg: &Config) -> Agent {
    let want = cfg.assistant_command.as_deref().map(str::trim).filter(|c| !c.is_empty()).unwrap_or(DEFAULT_AGENT);
    if let Some(a) = agents(&cfg.assistant_agent).into_iter().find(|a| a.id == want) {
        return a;
    }
    let mut words = want.split_whitespace().map(String::from);
    let command = words.next().unwrap_or_default();
    Agent { id: want.into(), label: format!("custom: {want}"), command, args: words.collect(), ..Agent::default() }
}

/// The agent to start and its argv, or `None` when the chosen agent is a
/// known one that is not installed (the caller opens the Store). A custom
/// command line is trusted as typed.
pub fn resolve(cfg: &Config) -> Option<(Agent, String, Vec<String>)> {
    let a = selected(cfg);
    if a.app.is_some() && !a.installed() {
        return None;
    }
    let (cmd, args) = a.argv(&cfg.assistant_args);
    Some((a, cmd, args))
}

/// Whether the configured agent command looks runnable, for the Settings
/// status marker. An explicit path (`assistant_command` may be a wrapper or an
/// absolute path) must be an existing file — and, on Unix, carry the executable
/// bit so the marker reflects actual launchability; a bare name is looked up on
/// `$PATH`.
pub fn agent_available(command: &str) -> bool {
    if command.contains('/') {
        let p = std::path::Path::new(command);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(p)
                .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        }
        #[cfg(not(unix))]
        {
            p.is_file()
        }
    } else {
        crate::catalog::is_installed(command)
    }
}

/// The code folder (WMAST-004): `assistant_code_dir` with `~` expanded, else `~/code`.
pub fn code_dir(cfg: &Config) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    match cfg.assistant_code_dir.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some("~") => home,
        Some(d) => d.strip_prefix("~/").map(|r| home.join(r)).unwrap_or_else(|| PathBuf::from(d)),
        None => home.join("code"),
    }
}

/// The assistant's working directory (`$XDG_DATA_HOME` aware). The instruction
/// pack lives here and the agent CLI starts here, so its context-file
/// convention picks the briefing up automatically.
pub fn workdir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")))?;
    Some(base.join("tetron-wm").join("assistant"))
}

/// Markdown table of the user's saved systems for the SYSTEMS guide, or a
/// pointer to Add Remote when none are saved yet.
fn systems_table(systems: &[crate::systems::RemoteSystem]) -> String {
    if systems.is_empty() {
        return "(none saved yet — the user adds machines via the power menu's \
Systems → Add Remote, which installs SSH keys and tetron-wm on them)"
            .to_string();
    }
    let mut t = String::from("| name | ssh target | port |\n|---|---|---|\n");
    for s in systems {
        t.push_str(&format!(
            "| {} | {} | {} |\n",
            s.name,
            s.host,
            s.port.map(|p| p.to_string()).unwrap_or_else(|| "22".into())
        ));
    }
    t
}

/// "deb package; installed here: tetron-wm, casttui; not installed: …".
fn suite_line() -> String {
    let (have, miss): (Vec<&str>, Vec<&str>) = SUITE_BINS.iter().partition(|b| crate::catalog::is_installed(b));
    let list = |v: &[&str]| if v.is_empty() { "none".to_string() } else { v.join(", ") };
    format!("Suite commands installed here: {}. Not installed: {}.", list(&have), list(&miss))
}

/// Fill the pack's placeholders in `text`.
fn fill(text: &str, host: &str, systems: &[crate::systems::RemoteSystem], code_dir: &Path) -> String {
    text.replace("{{HOST}}", host)
        .replace("{{VERSION}}", crate::VERSION)
        .replace("{{SHA}}", crate::GIT_SHA)
        .replace("{{REPO}}", crate::REPO_URL)
        .replace("{{SYSTEMS}}", &systems_table(systems))
        .replace("{{CODE_DIR}}", &code_dir.display().to_string())
        .replace("{{INSTALL}}", crate::packaging::install_method())
        .replace("{{SUITE}}", &suite_line())
}

/// The fully-assembled briefing: every section of the pack, placeholders
/// filled. Written as one file for single-context-file conventions.
pub fn briefing(host: &str, systems: &[crate::systems::RemoteSystem], code_dir: &Path) -> String {
    let joined = [BRIEFING_MD, DESKTOP_MD, SYSTEMS_MD, TROUBLESHOOTING_MD, SUITE_MD, CODE_MD, RULES_MD].join("\n");
    fill(&joined, host, systems, code_dir)
}

/// Stamp the instruction pack into `dir` as `AGENTS.md`, the context file
/// the agent CLIs read from their cwd on startup. Refreshed on every launch so
/// the host name, machine list, and version never go stale.
pub fn write_briefing(
    dir: &Path,
    host: &str,
    systems: &[crate::systems::RemoteSystem],
    code_dir: &Path,
) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    // Best-effort: clear artifacts from the retired multi-framework stamping so
    // upgraders don't keep stale context files with outdated placeholders. We
    // leave `.env` alone — it may hold the user's provider secrets.
    let _ = std::fs::remove_file(dir.join("CLAUDE.md"));
    let _ = std::fs::remove_file(dir.join("HERMES.md"));
    let _ = std::fs::remove_dir_all(dir.join("knowledge"));
    std::fs::write(dir.join("AGENTS.md"), briefing(host, systems, code_dir))
}

/// Give `agent` each of its `context_files` as a link to `AGENTS.md` (a copy
/// where links fail). Names with a path part are skipped (WMAST-003).
pub fn link_context(dir: &Path, agent: &Agent) {
    for name in &agent.context_files {
        if name.is_empty() || name.contains('/') || name == "AGENTS.md" || name == "." || name == ".." {
            crate::log_warn(&format!("assistant: context file '{name}' skipped (plain file names only)"));
            continue;
        }
        let p = dir.join(name);
        let _ = std::fs::remove_file(&p);
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink("AGENTS.md", &p).is_ok();
        #[cfg(not(unix))]
        let linked = false;
        if !linked {
            if let Err(e) = std::fs::copy(dir.join("AGENTS.md"), &p) {
                crate::log_warn(&format!("assistant: context file {} not written: {e}", p.display()));
            }
        }
    }
}

/// The MCP server entry in one of the named file shapes.
fn mcp_doc(template: &str, bin: &str) -> Option<String> {
    let v = match template {
        "json" => serde_json::json!({"mcpServers": {"tetron-wm": {"command": bin, "args": ["mcp"]}}}),
        "opencode" => serde_json::json!({
            "$schema": "https://opencode.ai/config.json",
            "mcp": {"tetron-wm": {"type": "local", "command": [bin, "mcp"], "enabled": true}}
        }),
        _ => return None,
    };
    serde_json::to_string_pretty(&v).ok()
}

/// A file in `dir` that tetron-wm wrote for MCP (it names the server), so
/// turning MCP off never deletes a file of the user's own.
fn ours(p: &Path) -> bool {
    std::fs::read_to_string(p).is_ok_and(|t| t.contains("\"tetron-wm\""))
}

/// Remove the MCP files of every known agent from `dir` (WMAST-006).
fn clear_mcp(dir: &Path, all: &[Agent]) {
    let files = all.iter().filter_map(|a| a.mcp.as_ref()).map(|m| m.file.as_str()).filter(|f| !f.is_empty());
    for f in files.chain([MCP_JSON_FILE]) {
        let p = dir.join(f);
        if ours(&p) && std::fs::remove_file(&p).is_ok() {
            crate::dbg_log(&format!("assistant: mcp off, removed {}", p.display()));
        }
    }
}

/// Wire `tetron-wm mcp` into `agent` when `on`: write its file into `dir`
/// and return the extra args it needs. Files of earlier launches are removed
/// first, so off really is off and a switched agent leaves nothing behind.
pub fn wire_mcp(dir: &Path, agent: &Agent, all: &[Agent], on: bool, bin: &str) -> Vec<String> {
    clear_mcp(dir, all);
    let Some(m) = agent.mcp.as_ref().filter(|_| on) else {
        if on {
            crate::dbg_log(&format!("assistant: mcp on, but {} has no mcp entry; started without it", agent.label));
        }
        return Vec::new();
    };
    let write = |name: &str, template: &str| {
        let p = dir.join(name);
        let res = match mcp_doc(template, bin) {
            Some(doc) => p.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(&p, doc)),
            None => Err(std::io::Error::other(format!("unknown template '{template}'"))),
        };
        match res {
            Ok(()) => crate::dbg_log(&format!("assistant: mcp wired for {} in {}", agent.label, p.display())),
            Err(e) => crate::log_warn(&format!("assistant: mcp file {} not written: {e}", p.display())),
        }
    };
    if !m.file.is_empty() && !m.file.starts_with('/') && !m.file.split('/').any(|c| c == "..") {
        write(&m.file, &m.template);
    }
    if m.args.iter().any(|a| a.contains("{{MCP_JSON}}")) {
        write(MCP_JSON_FILE, "json");
    }
    let json = dir.join(MCP_JSON_FILE).display().to_string();
    m.args.iter().map(|a| a.replace("{{MCP_JSON}}", &json).replace("{{MCP_BIN}}", bin)).collect()
}

/// This binary's path for agents to start `tetron-wm mcp` with (after an
/// update the running file may show as deleted; the path is what counts).
pub fn self_bin() -> String {
    std::env::current_exe()
        .map(|p| p.display().to_string().trim_end_matches(" (deleted)").to_string())
        .unwrap_or_else(|_| "tetron-wm".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::RemoteSystem;

    fn sys() -> Vec<RemoteSystem> {
        vec![RemoteSystem { name: "ubuntu".into(), host: "me@10.0.0.7".into(), port: Some(2222), theme: None }]
    }

    #[test]
    fn briefing_contains_the_essentials() {
        let b = briefing("mini", &sys(), Path::new("/home/u/code"));
        for needle in [
            "tetron-wm desktop assistant",
            "mini",
            crate::REPO_URL,
            "tetron-wm launch",
            "tetron-wm tile",
            "tetron-wm theme",
            "tetron-wm msg",
            "tetron-wm-debug.log",
            "config.toml",
            "systems.toml",
            "pull request",
            "Never run `tetron-wm kill`",
            // Cross-machine operations: the saved system appears with its port,
            // and the scp/ssh recipes are present.
            "me@10.0.0.7",
            "2222",
            "scp -3",
            "BatchMode=yes",
        ] {
            assert!(b.contains(needle), "briefing must mention {needle:?}");
        }
        assert!(!b.contains("{{"), "all placeholders must be filled");
        for needle in ["The tetron suite", "tetron-browser", "/home/u/code", "TETRON_WM_FROM_CLONE", "AGENTS.md"] {
            assert!(b.contains(needle), "suite and code sections must mention {needle:?}");
        }
    }

    #[test]
    fn empty_systems_points_at_add_remote() {
        let b = briefing("mini", &[], Path::new("/home/u/code"));
        assert!(b.contains("Add Remote"), "no machines yet → tell the agent how they get added");
    }

    fn cfg(cmd: Option<&str>, defs: Vec<AgentDef>) -> Config {
        Config { assistant_command: cmd.map(String::from), assistant_agent: defs, ..Config::default() }
    }

    #[test]
    fn agents_come_from_the_catalog() {
        let ids: Vec<String> = agents(&[]).into_iter().map(|a| a.id).collect();
        for id in ["opencode", "hermes", "pi", "codex", "claude", "agy"] {
            assert!(ids.iter().any(|i| i == id), "{id} must be a catalog agent: {ids:?}");
        }
    }

    #[test]
    fn saved_ids_resolve_and_unknown_values_run_as_command_lines() {
        // Values saved by the old opencode/hermes switch keep working.
        assert_eq!(selected(&cfg(Some("hermes"), vec![])).label, "Hermes");
        assert_eq!(selected(&cfg(None, vec![])).id, DEFAULT_AGENT);
        assert_eq!(selected(&cfg(Some("   "), vec![])).id, DEFAULT_AGENT, "blank means unset");
        let a = selected(&cfg(Some("my-agent --flag"), vec![]));
        assert_eq!((a.command.as_str(), a.args.clone(), a.app.clone()), ("my-agent", vec!["--flag".to_string()], None));
        // A custom command line is trusted as typed, installed or not.
        let (_, cmd, args) = resolve(&Config { assistant_args: vec!["-x".into()], ..cfg(Some("my-agent --flag"), vec![]) }).unwrap();
        assert_eq!((cmd.as_str(), args), ("my-agent", vec!["--flag".to_string(), "-x".to_string()]));
    }

    #[test]
    fn config_tables_override_by_id_and_add_agents() {
        let over = AgentDef { id: "codex".into(), args: Some(vec!["--oss".into()]), ..AgentDef::default() };
        let new = AgentDef {
            id: "newthing".into(),
            label: Some("NewThing".into()),
            env: [("K".to_string(), "v".to_string())].into(),
            ..AgentDef::default()
        };
        let all = agents(&[over, new]);
        let codex = all.iter().find(|a| a.id == "codex").unwrap();
        assert_eq!((codex.label.as_str(), codex.args.clone()), ("Codex", vec!["--oss".to_string()]), "label kept, args overridden");
        assert!(codex.mcp.is_some(), "catalog mcp kept");
        let n = all.iter().find(|a| a.id == "newthing").unwrap();
        assert_eq!((n.label.as_str(), n.command.as_str(), n.app.clone()), ("NewThing", "newthing", None));
        assert_eq!(n.argv(&["a".into()]), ("env".to_string(), vec!["K=v".into(), "newthing".into(), "a".into()]));
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tetron-wm-assist-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("AGENTS.md"), "pack").unwrap();
        d
    }

    #[test]
    fn context_files_link_to_agents_md_and_paths_are_refused() {
        let d = tmp("ctx");
        let a = Agent { context_files: vec!["GEMINI.md".into(), "../evil.md".into()], ..Agent::default() };
        link_context(&d, &a);
        assert_eq!(std::fs::read_to_string(d.join("GEMINI.md")).unwrap(), "pack");
        assert!(!d.parent().unwrap().join("evil.md").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn mcp_on_writes_the_agents_file_and_off_removes_it() {
        let d = tmp("mcp");
        let all = agents(&[]);
        let oc = all.iter().find(|a| a.id == "opencode").unwrap();
        assert!(wire_mcp(&d, oc, &all, true, "/bin/tetron-wm").is_empty());
        let f = std::fs::read_to_string(d.join("opencode.json")).unwrap();
        assert!(f.contains("/bin/tetron-wm") && f.contains("\"tetron-wm\""));
        let claude = all.iter().find(|a| a.id == "claude").unwrap();
        let args = wire_mcp(&d, claude, &all, true, "/bin/tetron-wm");
        assert_eq!(args, vec!["--mcp-config".to_string(), d.join(MCP_JSON_FILE).display().to_string()]);
        assert!(!d.join("opencode.json").exists(), "switching agent clears the other agent's file");
        assert!(wire_mcp(&d, claude, &all, false, "/bin/tetron-wm").is_empty());
        assert!(!d.join(MCP_JSON_FILE).exists(), "off removes the file");
        // A file of the user's own (no tetron-wm server in it) is never removed.
        std::fs::write(d.join("opencode.json"), "{}").unwrap();
        wire_mcp(&d, oc, &all, false, "x");
        assert!(d.join("opencode.json").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn code_dir_defaults_to_home_code_and_expands_tilde() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(code_dir(&Config::default()), home.join("code"));
        let c = Config { assistant_code_dir: Some("~/src/x".into()), ..Config::default() };
        assert_eq!(code_dir(&c), home.join("src/x"));
    }

    #[test]
    fn pack_written_as_agents_md_and_cleans_retired_files() {
        let dir = std::env::temp_dir().join(format!("tetron-wm-assist-test-{}", std::process::id()));
        // Simulate an upgrade: a workdir left by the old multi-framework stamping.
        std::fs::create_dir_all(dir.join("knowledge")).unwrap();
        std::fs::write(dir.join("CLAUDE.md"), "stale").unwrap();
        std::fs::write(dir.join("HERMES.md"), "stale").unwrap();
        std::fs::write(dir.join("knowledge/old.md"), "stale").unwrap();
        std::fs::write(dir.join(".env"), "SECRET=keep").unwrap();

        write_briefing(&dir, "host", &sys(), Path::new("/c")).unwrap();

        let a = std::fs::read_to_string(dir.join("AGENTS.md")).unwrap();
        assert!(a.contains("desktop assistant"));
        assert!(!a.contains("{{"), "placeholders filled");
        // Retired conventions are cleaned up on write…
        assert!(!dir.join("CLAUDE.md").exists());
        assert!(!dir.join("HERMES.md").exists());
        assert!(!dir.join("knowledge").exists());
        // …but a user's .env (possible secrets) is left untouched.
        assert!(dir.join(".env").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn agent_available_handles_paths_and_names() {
        // A bare name that surely isn't installed reports unavailable.
        assert!(!agent_available("definitely-not-a-real-binary-xyz"));
        // An explicit path is checked on disk (and, on Unix, must be executable).
        let f = std::env::temp_dir().join(format!("tetron-wm-agent-{}", std::process::id()));
        std::fs::write(&f, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // A regular file without the exec bit is NOT runnable…
            assert!(!agent_available(&f.display().to_string()), "non-executable path is unavailable");
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        // …once executable (or on non-Unix), an existing path is available.
        assert!(agent_available(&f.display().to_string()), "an executable path is available");
        let _ = std::fs::remove_file(&f);
        assert!(!agent_available("/no/such/path/opencode"), "a missing path is unavailable");
    }
}
