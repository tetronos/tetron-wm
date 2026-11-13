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

use crate::catalog::Wiring;
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
    /// How it gets the briefing outside the AI folder (None: it does not).
    pub briefing: Option<Wiring>,
    pub mcp: Option<Wiring>,
    /// The catalog app name, for the Store's Install (None: config only).
    pub app: Option<String>,
}

impl Agent {
    /// The command to run: the agent's own args, then `extra`, wrapped in
    /// `env` when it or `extra_env` sets variables (the app server spawns a
    /// plain argv).
    pub fn argv(&self, extra: &[String], extra_env: &[(String, String)]) -> (String, Vec<String>) {
        let mut args: Vec<String> = self.args.iter().chain(extra).cloned().collect();
        let env: Vec<&(String, String)> = self.env.iter().chain(extra_env).collect();
        if env.is_empty() {
            return (self.command.clone(), args);
        }
        let mut wrapped: Vec<String> = env.iter().map(|(k, v)| format!("{k}={v}")).collect();
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
            briefing: s.briefing.clone(),
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
    if d.briefing.is_some() { a.briefing = d.briefing.clone(); }
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

/// The agent to start, or `None` when the chosen agent is a known one that
/// is not installed (the caller opens the Store). A custom command line is
/// trusted as typed.
pub fn resolve(cfg: &Config) -> Option<Agent> {
    let a = selected(cfg);
    (a.app.is_none() || a.installed()).then_some(a)
}

/// Where the agent starts (WMAST-007).
#[derive(Clone, Debug, PartialEq)]
pub enum Cwd {
    /// The AI folder, where AGENTS.md is written.
    Ai,
    /// The folder picker at each launch.
    Ask,
    Dir(PathBuf),
}

/// `assistant_cwd`: unset is the code folder, `ask`, `ai`, else a path.
pub fn cwd_choice(cfg: &Config) -> Cwd {
    match cfg.assistant_cwd.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => Cwd::Dir(code_dir(cfg)),
        Some("ask") => Cwd::Ask,
        Some("ai") => Cwd::Ai,
        Some(p) => Cwd::Dir(expand_home(p)),
    }
}

/// Settings text for the working folder choice.
pub fn cwd_label(cfg: &Config) -> String {
    match (cwd_choice(cfg), cfg.assistant_cwd.is_none()) {
        (Cwd::Ai, _) => "AI folder".into(),
        (Cwd::Ask, _) => "ask each launch".into(),
        (Cwd::Dir(d), true) => format!("code folder ({})", tilde(&d)),
        (Cwd::Dir(d), false) => tilde(&d),
    }
}

fn expand_home(p: &str) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    match p {
        "~" => home,
        _ => p.strip_prefix("~/").map(|r| home.join(r)).unwrap_or_else(|| PathBuf::from(p)),
    }
}

/// `/home/u/code` as `~/code`.
pub fn tilde(p: &Path) -> String {
    match dirs::home_dir().and_then(|h| p.strip_prefix(h).ok().map(Path::to_path_buf)) {
        Some(r) if r.as_os_str().is_empty() => "~".into(),
        Some(r) => format!("~/{}", r.display()),
        None => p.display().to_string(),
    }
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
    match cfg.assistant_code_dir.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(d) => expand_home(d),
        None => expand_home("~/code"),
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

/// One file written in the AI folder: its template and what goes in it.
struct FileDoc<'a> {
    template: &'a str,
    briefing: Option<String>,
    mcp: bool,
}

/// The file in one of the named shapes: opencode's config (instructions and
/// MCP server) or an `mcpServers` object.
fn render_doc(d: &FileDoc, bin: &str) -> Option<String> {
    let server = serde_json::json!({"command": bin, "args": ["mcp"]});
    let v = match d.template {
        "json" => serde_json::json!({"mcpServers": {"tetron-wm": server}}),
        "opencode" => {
            let mut v = serde_json::json!({"$schema": "https://opencode.ai/config.json"});
            if let Some(b) = &d.briefing {
                v["instructions"] = serde_json::json!([b]);
            }
            if d.mcp {
                v["mcp"] = serde_json::json!({"tetron-wm": {"type": "local", "command": [bin, "mcp"], "enabled": true}});
            }
            v
        }
        _ => return None,
    };
    serde_json::to_string_pretty(&v).ok()
}

/// A file in the AI folder that tetron-wm wrote (it names tetron-wm), so a
/// file of the user's own there is never removed.
fn ours(p: &Path) -> bool {
    std::fs::read_to_string(p).is_ok_and(|t| t.contains("tetron-wm"))
}

/// Remove the files every known agent's wiring writes (WMAST-006, WMAST-007).
fn clear_wired(ai: &Path, all: &[Agent]) {
    let named = all.iter().flat_map(|a| [a.briefing.as_ref(), a.mcp.as_ref()]).flatten().map(|w| w.file.as_str());
    for f in named.filter(|f| !f.is_empty()).chain([MCP_JSON_FILE]) {
        let p = ai.join(f);
        if ours(&p) && std::fs::remove_file(&p).is_ok() {
            crate::dbg_log(&format!("assistant: removed {}", p.display()));
        }
    }
}

/// What the launch adds to the agent's argv and environment.
#[derive(Debug, Default, PartialEq)]
pub struct Prepared {
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// The agent gets the briefing (it starts in the AI folder, or has a way).
    pub briefed: bool,
}

/// Wire `agent` for a start in `cwd`: outside the AI folder `ai` it gets the
/// briefing through its `briefing` wiring, and with `mcp_on` the MCP server
/// through its `mcp` wiring. Files go in `ai` only, never in `cwd`; files of
/// earlier launches are removed first, so off is off and a switched agent
/// leaves nothing behind. Briefing args come before MCP args, which go last
/// (some take a variable number of values).
pub fn prepare(ai: &Path, cwd: &Path, agent: &Agent, all: &[Agent], mcp_on: bool, bin: &str) -> Prepared {
    clear_wired(ai, all);
    let in_ai = cwd == ai;
    let brief = agent.briefing.as_ref().filter(|_| !in_ai);
    let mcp = agent.mcp.as_ref().filter(|_| mcp_on);
    if mcp_on && agent.mcp.is_none() {
        crate::dbg_log(&format!("assistant: mcp on, but {} has no mcp entry; started without it", agent.label));
    }
    if !in_ai && brief.is_none() {
        crate::dbg_log(&format!("assistant: {} has no briefing entry; outside the AI folder it starts without the briefing", agent.label));
    }
    let agents_md = ai.join("AGENTS.md").display().to_string();
    let mut docs: Vec<(String, FileDoc)> = Vec::new();
    let mut add = |file: &str, template: &'static str, briefing: bool, mcp_part: bool| {
        if file.is_empty() || file.starts_with('/') || file.split('/').any(|c| c == "..") {
            return;
        }
        let i = docs.iter().position(|(f, _)| f == file).unwrap_or_else(|| {
            docs.push((file.to_string(), FileDoc { template, briefing: None, mcp: false }));
            docs.len() - 1
        });
        if briefing {
            docs[i].1.briefing = Some(agents_md.clone());
        }
        docs[i].1.mcp |= mcp_part;
    };
    let tpl = |w: &Wiring| -> &'static str { if w.template == "opencode" { "opencode" } else if w.template == "json" { "json" } else { "" } };
    if let Some(w) = brief {
        add(&w.file, tpl(w), true, false);
    }
    if let Some(w) = mcp {
        add(&w.file, tpl(w), false, true);
        if w.args.iter().chain(w.env.values()).any(|a| a.contains("{{MCP_JSON}}")) {
            add(MCP_JSON_FILE, "json", false, true);
        }
    }
    for (file, doc) in &docs {
        let p = ai.join(file);
        let res = match render_doc(doc, bin) {
            Some(text) => p.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(&p, text)),
            None => Err(std::io::Error::other(format!("unknown template '{}'", doc.template))),
        };
        match res {
            Ok(()) => crate::dbg_log(&format!("assistant: wired {} in {}", agent.label, p.display())),
            Err(e) => crate::log_warn(&format!("assistant: {} not written: {e}", p.display())),
        }
    }
    let fill = |t: &str| {
        t.replace("{{BRIEFING}}", &agents_md)
            .replace("{{AI_DIR}}", &ai.display().to_string())
            .replace("{{MCP_JSON}}", &ai.join(MCP_JSON_FILE).display().to_string())
            .replace("{{MCP_BIN}}", bin)
    };
    let mut out = Prepared { briefed: in_ai || brief.is_some(), ..Prepared::default() };
    for w in brief.into_iter().chain(mcp) {
        out.args.extend(w.args.iter().map(|a| fill(a)));
        for (k, v) in &w.env {
            if !out.env.iter().any(|(e, _)| e == k) {
                out.env.push((k.clone(), fill(v)));
            }
        }
    }
    out
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
        let a = resolve(&cfg(Some("my-agent --flag"), vec![])).unwrap();
        assert_eq!(a.argv(&["-x".into()], &[]), ("my-agent".to_string(), vec!["--flag".to_string(), "-x".to_string()]));
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
        let extra_env = [("E".to_string(), "1".to_string())];
        assert_eq!(n.argv(&["a".into()], &extra_env), ("env".to_string(), vec!["K=v".into(), "E=1".into(), "newthing".into(), "a".into()]));
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
        let p = prepare(&d, &d, oc, &all, true, "/bin/tetron-wm");
        assert!(p.args.is_empty() && p.briefed);
        let f = std::fs::read_to_string(d.join("opencode.json")).unwrap();
        assert!(f.contains("/bin/tetron-wm") && f.contains("\"tetron-wm\""));
        assert!(!f.contains("instructions"), "in the AI folder AGENTS.md is read directly");
        let claude = all.iter().find(|a| a.id == "claude").unwrap();
        let p = prepare(&d, &d, claude, &all, true, "/bin/tetron-wm");
        assert_eq!(p.args, vec!["--mcp-config".to_string(), d.join(MCP_JSON_FILE).display().to_string()]);
        assert!(!d.join("opencode.json").exists(), "switching agent clears the other agent's file");
        assert!(prepare(&d, &d, claude, &all, false, "/bin/tetron-wm").args.is_empty());
        assert!(!d.join(MCP_JSON_FILE).exists(), "off removes the file");
        // A file of the user's own (no tetron-wm in it) is never removed.
        std::fs::write(d.join("opencode.json"), "{}").unwrap();
        prepare(&d, &d, oc, &all, false, "x");
        assert!(d.join("opencode.json").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn outside_the_ai_folder_the_briefing_comes_by_flags_and_nothing_lands_in_cwd() {
        let d = tmp("brief");
        let proj = d.join("proj");
        std::fs::create_dir_all(&proj).unwrap();
        let all = agents(&[]);
        let md = d.join("AGENTS.md").display().to_string();
        // opencode: OPENCODE_CONFIG points at a file in the AI folder with the
        // instructions, and the MCP server in the same file.
        let oc = all.iter().find(|a| a.id == "opencode").unwrap();
        let p = prepare(&d, &proj, oc, &all, true, "/b");
        assert!(p.briefed);
        assert_eq!(p.env, vec![("OPENCODE_CONFIG".to_string(), d.join("opencode.json").display().to_string())]);
        let f = std::fs::read_to_string(d.join("opencode.json")).unwrap();
        assert!(f.contains(&md) && f.contains("\"mcp\""), "{f}");
        // Claude Code: the briefing file by flag, before the MCP args.
        let claude = all.iter().find(|a| a.id == "claude").unwrap();
        let p = prepare(&d, &proj, claude, &all, true, "/b");
        assert_eq!(p.args[..2], ["--append-system-prompt-file".to_string(), md.clone()]);
        assert_eq!(p.args[2], "--mcp-config");
        // An agent without a briefing entry is started without it.
        let hermes = all.iter().find(|a| a.id == "hermes").unwrap();
        assert!(!prepare(&d, &proj, hermes, &all, false, "/b").briefed);
        assert_eq!(std::fs::read_dir(&proj).unwrap().count(), 0, "nothing is written into the working folder");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn cwd_choice_reads_the_config() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(cwd_choice(&Config::default()), Cwd::Dir(home.join("code")));
        let c = |v: &str| Config { assistant_cwd: Some(v.into()), ..Config::default() };
        assert_eq!(cwd_choice(&c("ask")), Cwd::Ask);
        assert_eq!(cwd_choice(&c("ai")), Cwd::Ai);
        assert_eq!(cwd_choice(&c("~/p")), Cwd::Dir(home.join("p")));
        assert_eq!(tilde(&home.join("p")), "~/p");
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
