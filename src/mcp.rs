//! `tetron-wm mcp`: the desktop as MCP tools for the ✦ Assistant's agent
//! (WMAST-006). MCP over stdio is newline-delimited JSON-RPC 2.0; this
//! answers `initialize`, `ping`, `tools/list` and `tools/call` and forwards
//! each tool to the running daemon through the control socket, as the
//! `tetron-wm launch/tile/theme` commands do. It is never resident: the agent
//! starts it and it ends with its stdin.

use crate::session::ClientMsg;
use serde_json::{json, Value};
use std::io::{BufRead, Write};

/// The MCP revision answered when the client asks for none.
const PROTOCOL: &str = "2025-06-18";

/// (name, description, input schema properties, required) for tools/list.
fn tools() -> Value {
    let none = json!({"type": "object", "properties": {}});
    json!([
        {"name": "status", "description": "Whether a client is attached, how many windows are open and their titles.", "inputSchema": none},
        {"name": "list_windows", "description": "Titles of the open windows, in taskbar order.", "inputSchema": none},
        {"name": "launch", "description": "Open a program in a new tetron-wm window.", "inputSchema": {
            "type": "object", "required": ["command"],
            "properties": {"command": {"type": "string"}, "args": {"type": "array", "items": {"type": "string"}}}}},
        {"name": "tile", "description": "Tile all windows into the grid.", "inputSchema": none},
        {"name": "focus_next", "description": "Focus the next window.", "inputSchema": none},
        {"name": "maximize_focused", "description": "Maximize or restore the focused window.", "inputSchema": none},
        {"name": "minimize_focused", "description": "Minimize the focused window.", "inputSchema": none},
        {"name": "close_focused", "description": "Close the focused window (the user may be asked to confirm).", "inputSchema": none},
        {"name": "list_themes", "description": "Names of the installed themes.", "inputSchema": none},
        {"name": "set_theme", "description": "Switch the desktop theme.", "inputSchema": {
            "type": "object", "required": ["name"], "properties": {"name": {"type": "string"}}}},
        {"name": "screenshot", "description": "Save a PNG of the whole screen; returns its path.", "inputSchema": {
            "type": "object", "properties": {"path": {"type": "string", "description": "absolute .png path; default: the current folder"}}}},
    ])
}

/// Send `msg` to the daemon; the tool's text result or an error.
fn send(msg: &ClientMsg, done: &str) -> Result<String, String> {
    match crate::protocol::send_control(msg) {
        Ok(true) => Ok(done.to_string()),
        Ok(false) => Err("no tetron-wm desktop is running".into()),
        Err(e) => Err(format!("control socket: {e}")),
    }
}

fn arg_str(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

/// Run one tool; `Ok(text)` or `Err(message)` (both go back as content).
fn call(name: &str, args: &Value) -> Result<String, String> {
    match name {
        "status" | "list_windows" => {
            let st = crate::protocol::query_status().ok_or("no tetron-wm desktop is running (or it is too old to answer)")?;
            if name == "list_windows" {
                return Ok(if st.titles.is_empty() { "(no windows)".into() } else { st.titles.join("\n") });
            }
            Ok(json!({"attached": st.attached, "windows": st.windows, "titles": st.titles}).to_string())
        }
        "launch" => {
            let command = arg_str(args, "command").ok_or("launch needs a command")?;
            let argv: Vec<String> = args.get("args").and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
            let name = command.rsplit('/').next().unwrap_or(&command).to_string();
            send(&ClientMsg::Launch { name, command: command.clone(), args: argv }, &format!("launched {command}"))
        }
        "tile" => send(&ClientMsg::TileAll, "tiled"),
        "focus_next" => send(&ClientMsg::FocusNext, "focused the next window"),
        "maximize_focused" => send(&ClientMsg::MaximizeFocused, "maximized or restored"),
        "minimize_focused" => send(&ClientMsg::MinimizeFocused, "minimized"),
        "close_focused" => send(&ClientMsg::CloseFocused, "asked the focused window to close"),
        "list_themes" => Ok(crate::theme::preset_names().join("\n")),
        "set_theme" => {
            let t = arg_str(args, "name").ok_or("set_theme needs a name")?;
            if !crate::theme::preset_names().contains(&t) {
                return Err(format!("no theme '{t}' (list_themes names them)"));
            }
            send(&ClientMsg::SetTheme(t.clone()), &format!("theme {t}"))
        }
        "screenshot" => {
            let path = match arg_str(args, "path") {
                Some(p) => std::path::absolute(p).map_err(|e| e.to_string())?,
                None => {
                    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                    std::env::current_dir().map_err(|e| e.to_string())?.join(format!("tetron-wm-screenshot-{ts}.png"))
                }
            };
            let p = path.to_string_lossy().into_owned();
            send(&ClientMsg::Screenshot(p.clone()), &format!("{p} (written by the desktop within a moment)"))
        }
        _ => Err(format!("unknown tool '{name}'")),
    }
}

/// The JSON-RPC answer to one request, or `None` for a notification.
pub fn handle(req: &Value) -> Option<Value> {
    let id = req.get("id").cloned()?;
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => json!({
            "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL),
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "tetron-wm", "version": crate::VERSION},
            "instructions": "Control the tetron-wm terminal desktop: windows, apps, themes, screenshots."
        }),
        "ping" => json!({}),
        "tools/list" => json!({"tools": tools()}),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let out = call(name, &args);
            crate::log_debug(&format!("mcp: {name} {args} -> {}", match &out { Ok(_) => "ok", Err(e) => e }));
            let (text, error) = match out { Ok(t) => (t, false), Err(e) => (e, true) };
            json!({"content": [{"type": "text", "text": text}], "isError": error})
        }
        _ => return Some(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("method not found: {method}")}})),
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

/// Serve stdin to stdout until stdin closes.
pub fn run() -> std::io::Result<()> {
    crate::dbg_log("mcp: started");
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(req) => handle(&req),
            Err(e) => Some(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("parse error: {e}")}})),
        };
        if let Some(r) = reply {
            writeln!(out, "{r}")?;
            out.flush()?;
        }
    }
    crate::dbg_log("mcp: stdin closed, exiting");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_and_tools_list() {
        let r = handle(&json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26"}})).unwrap();
        assert_eq!(r["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(r["result"]["serverInfo"]["name"], "tetron-wm");
        let r = handle(&json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).unwrap();
        let names: Vec<&str> = r["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        for n in ["status", "list_windows", "launch", "tile", "set_theme", "screenshot"] {
            assert!(names.contains(&n), "{n}");
        }
    }

    #[test]
    fn notifications_get_no_reply_and_unknowns_are_errors() {
        assert!(handle(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).is_none());
        let r = handle(&json!({"jsonrpc": "2.0", "id": 3, "method": "nope"})).unwrap();
        assert_eq!(r["error"]["code"], -32601);
        let r = handle(&json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "zzz"}})).unwrap();
        assert_eq!(r["result"]["isError"], true);
        let r = handle(&json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "set_theme", "arguments": {"name": "no-such-theme"}}})).unwrap();
        assert_eq!(r["result"]["isError"], true);
        assert!(r["result"]["content"][0]["text"].as_str().unwrap().contains("list_themes"));
    }
}
