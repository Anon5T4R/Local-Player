//! The MCP server of Local Player: the suite's [`craft_mcp::Server`] in front of the app's
//! commands, which run either headless ([`session::Session`]) or in the running window
//! ([`craft_mcp::remote::Remote`] over its control channel, `local-player-rs --control PORT`).
//!
//! Three tools, one per way in: `command_list` discovers the catalogue, `command_run` runs one
//! command and `command_batch` runs steps in order with `"$N.path"` references between them. The
//! commands themselves are the app's (`catalogue`), so a new command is reachable the moment its
//! id exists.

use craft_mcp::{Handler, ServerInfo, ToolResult};
use serde_json::{Map, Value, json};

use super::catalogue;
use super::session::Session;

/// Where a tool call runs.
pub trait Backend {
    /// One control-channel method, as the app answers them (`command.execute`, `command.list`,
    /// `app.info`).
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String>;
    /// Where calls go, for the instructions ("headless", "remote 127.0.0.1:7979").
    fn describe(&self) -> String;
}

impl Backend for Session {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        Session::call(self, method, &params)
    }
    fn describe(&self) -> String {
        Session::describe(self)
    }
}

impl Backend for craft_mcp::remote::Remote {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        craft_mcp::remote::Remote::call(self, method, params)
    }
    fn describe(&self) -> String {
        format!("remote {}", self.addr())
    }
}

impl Backend for Box<dyn Backend> {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        (**self).call(method, params)
    }
    fn describe(&self) -> String {
        (**self).describe()
    }
}

/// The app's [`Handler`] over one backend.
pub struct Automation<B: Backend> {
    backend: B,
}

impl<B: Backend> Automation<B> {
    pub fn new(backend: B) -> Self {
        Self { backend }
    }

    fn list(&mut self, args: &Map<String, Value>) -> Result<Value, String> {
        let filter = args.get("filter").and_then(Value::as_str);
        let brief = args.get("brief").and_then(Value::as_bool).unwrap_or(false);
        let mut params = json!({});
        if let Some(filter) = filter {
            params["filter"] = json!(filter);
        }
        params["brief"] = json!(brief);
        self.backend.call("command.list", params)
    }

    fn run(&mut self, args: &Map<String, Value>) -> Result<Value, String> {
        let id = args.get("id").and_then(Value::as_str).ok_or("missing string `id`")?;
        if !catalogue::has(id) {
            return Err(format!("unknown command `{id}`: command_list lists what this app has"));
        }
        let params = match args.get("params") {
            None | Some(Value::Null) => json!({}),
            Some(p) if p.is_object() => p.clone(),
            Some(_) => return Err("`params` must be a JSON object".into()),
        };
        self.backend.call("command.execute", json!({"command": id, "params": params}))
    }

    fn batch(&mut self, args: &Map<String, Value>) -> Result<Value, String> {
        let steps = args.get("steps").and_then(Value::as_array).ok_or("missing array `steps`")?;
        if steps.len() > craft_mcp::script::MAX_BATCH_STEPS {
            return Err(format!("`steps` holds {} entries; at most {}", steps.len(), craft_mcp::script::MAX_BATCH_STEPS));
        }
        let stop = args.get("stop_on_error").and_then(Value::as_bool).unwrap_or(true);
        // Each step's result, for the `"$N"` references of the later ones.
        let mut results: Vec<Value> = Vec::with_capacity(steps.len());
        let mut failed: Option<usize> = None;
        for (n, step) in steps.iter().enumerate() {
            let id = step.get("id").and_then(Value::as_str).ok_or_else(|| format!("step {n}: missing string `id`"))?;
            let params = match step.get("params") {
                None | Some(Value::Null) => json!({}),
                Some(p) if p.is_object() => p.clone(),
                Some(_) => return Err(format!("step {n} ({id}): `params` must be a JSON object")),
            };
            let params = craft_mcp::script::resolve(&params, &results).map_err(|e| format!("step {n} ({id}): {e}"))?;
            match self.backend.call("command.execute", json!({"command": id, "params": params})) {
                Ok(value) => results.push(value),
                Err(e) => {
                    if stop {
                        return Err(format!("step {n} ({id}): {e}"));
                    }
                    if failed.is_none() {
                        failed = Some(n);
                    }
                    results.push(json!({"error": e}));
                }
            }
        }
        Ok(json!({"completed": results.len(), "failed": failed, "results": results}))
    }
}

impl<B: Backend> Handler for Automation<B> {
    fn server_info(&self) -> ServerInfo {
        ServerInfo { name: "local-player-rs", title: "Local Player", version: env!("CARGO_PKG_VERSION") }
    }

    fn instructions(&self) -> String {
        "Local Player is a local, offline media player. A headless session manages the playlist \
         (playlist.add_file, playlist.add_folder, playlist.clear) and reads container metadata \
         (media.probe); playing needs the desktop window, which serves the same commands through \
         `local-player-rs --control PORT` and `mcp --connect PORT`. Every action is a command: \
         command_list discovers the ids, their labels and their parameters; command_run runs one \
         with its JSON params; command_batch runs steps in order, where a string that is exactly \
         \"$N.path\" takes that value from step N's result. Commands marked appOnly run only in the \
         desktop window."
            .to_string()
    }

    fn describe(&self) -> String {
        self.backend.describe()
    }

    fn tools(&self) -> Vec<Value> {
        vec![
            json!({
                "name": "command_list",
                "title": "List commands",
                "description": "The commands this app offers: id, label, menu, whether it needs the window (appOnly) and its parameter schema. Optional filter keeps the ids, labels or menus containing the text (case-insensitive); brief keeps only id and label.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "filter": {"type": "string", "description": "Case-insensitive id, label or menu substring"},
                        "brief": {"type": "boolean", "description": "Return only id and label"},
                    },
                    "additionalProperties": false,
                },
                "annotations": {"title": "List commands", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false},
            }),
            json!({
                "name": "command_run",
                "title": "Run a command",
                "description": "Run one command by id with its JSON params (command_list describes them). The result is the command's own JSON.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string", "description": "Command id, as command_list returns it"},
                        "params": {"type": "object", "description": "The command's parameters"},
                    },
                    "required": ["id"],
                    "additionalProperties": false,
                },
                "annotations": {"title": "Run a command", "readOnlyHint": false, "destructiveHint": false, "openWorldHint": false},
            }),
            json!({
                "name": "command_batch",
                "title": "Run several commands",
                "description": "Run steps in order: [{id, params?}]. A string that is exactly \"$N.path\" (N: 0-based step index) or \"$last.path\" takes that value from an earlier step's result; \"$$\" starts a literal \"$\". Stops on the first error unless stop_on_error is false. At most 256 steps.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "steps": {
                            "type": "array",
                            "maxItems": 256,
                            "items": {
                                "type": "object",
                                "properties": {"id": {"type": "string"}, "params": {"type": "object"}},
                                "required": ["id"],
                                "additionalProperties": false,
                            },
                        },
                        "stop_on_error": {"type": "boolean", "default": true},
                    },
                    "required": ["steps"],
                    "additionalProperties": false,
                },
                "annotations": {"title": "Run several commands", "readOnlyHint": false, "destructiveHint": false, "openWorldHint": false},
            }),
        ]
    }

    fn call_tool(&mut self, name: &str, arguments: &Value) -> ToolResult {
        let args = match tool_arguments(name, arguments) {
            Ok(args) => args,
            Err(e) => return ToolResult::error(e),
        };
        let outcome = match name {
            "command_list" => self.list(&args),
            "command_run" => self.run(&args),
            "command_batch" => self.batch(&args),
            other => return ToolResult::error(format!("unknown tool `{other}`: this server has command_list, command_run and command_batch")),
        };
        match outcome {
            Ok(value) => ToolResult::json(&value),
            Err(e) => ToolResult::error(e),
        }
    }
}

/// The arguments of one tool as an object, refusing a key it does not read.
fn tool_arguments(tool: &str, arguments: &Value) -> Result<Map<String, Value>, String> {
    let map = match arguments {
        Value::Null => Map::new(),
        Value::Object(m) => m.clone(),
        _ => return Err(format!("{tool}: arguments must be a JSON object")),
    };
    let allowed: &[&str] = match tool {
        "command_list" => &["filter", "brief"],
        "command_run" => &["id", "params"],
        "command_batch" => &["steps", "stop_on_error"],
        _ => &[],
    };
    if let Some(key) = map.keys().find(|k| !allowed.contains(&k.as_str())) {
        return Err(format!(
            "{tool}: unknown argument `{key}` (it takes {})",
            if allowed.is_empty() { "none".to_string() } else { allowed.join(", ") }
        ));
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> craft_mcp::Server<Automation<Session>> {
        craft_mcp::Server::new(Automation::new(Session::new(None)))
    }

    fn call(s: &mut craft_mcp::Server<Automation<Session>>, id: u64, method: &str, params: Value) -> Value {
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string();
        let reply = s.handle_line(&line).expect("a request gets a reply");
        serde_json::from_str(&reply).expect("the reply is JSON")
    }

    fn text_of(v: &Value) -> String {
        v["result"]["content"][0]["text"].as_str().unwrap_or_default().to_string()
    }

    fn folder(tag: &str) -> std::path::PathBuf {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("local-player-mcp-{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["a.mp4", "b.mkv"] {
            std::fs::write(dir.join(name), b"not really media").unwrap();
        }
        dir
    }

    #[test]
    fn the_lifecycle_lists_the_three_tools_and_runs_a_command() {
        let mut s = server();
        let init = call(&mut s, 1, "initialize", json!({}));
        assert_eq!(init["result"]["serverInfo"]["name"], "local-player-rs");
        assert!(init["result"]["instructions"].as_str().unwrap_or_default().contains("command_run"));
        assert!(init["result"]["instructions"].as_str().unwrap_or_default().ends_with("Backend: headless."));
        assert_eq!(s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#), None);

        let tools = call(&mut s, 2, "tools/list", json!({}));
        let names: Vec<&str> = tools["result"]["tools"].as_array().map(|a| a.iter().filter_map(|t| t["name"].as_str()).collect()).unwrap_or_default();
        assert_eq!(names, ["command_list", "command_run", "command_batch"]);

        let listed = call(&mut s, 3, "tools/call", json!({"name": "command_list", "arguments": {"filter": "playlist"}}));
        assert_eq!(listed["result"]["isError"], false);
        let commands: Value = serde_json::from_str(&text_of(&listed)).unwrap_or(Value::Null);
        assert_eq!(commands[0]["id"], "playlist.add_file");

        let dir = folder("run");
        let added =
            call(&mut s, 4, "tools/call", json!({"name": "command_run", "arguments": {"id": "playlist.add_folder", "params": {"path": dir}}}));
        assert_eq!(added["result"]["isError"], false, "{}", text_of(&added));
        let state: Value = serde_json::from_str(&text_of(&added)).unwrap_or(Value::Null);
        assert_eq!(state["state"]["count"], 2);

        // A window-only command is refused with where it runs.
        let play = call(&mut s, 5, "tools/call", json!({"name": "command_run", "arguments": {"id": "playback.toggle"}}));
        assert_eq!(play["result"]["isError"], true);
        assert!(text_of(&play).contains("runs in the window"), "{}", text_of(&play));

        // Unknown tools, commands and arguments are refused, and the server keeps serving.
        let unknown_tool = call(&mut s, 6, "tools/call", json!({"name": "nope"}));
        assert!(text_of(&unknown_tool).contains("unknown tool `nope`"));
        let unknown = call(&mut s, 7, "tools/call", json!({"name": "command_run", "arguments": {"id": "playlist.shuffle"}}));
        assert!(text_of(&unknown).contains("unknown command"));
        let typo = call(&mut s, 8, "tools/call", json!({"name": "command_run", "arguments": {"id": "playlist.clear", "parms": {}}}));
        assert!(text_of(&typo).contains("unknown argument `parms`"), "{}", text_of(&typo));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_batch_runs_in_order_and_shares_results_by_reference() {
        let dir = folder("batch");
        let mut s = server();
        let _ = call(&mut s, 1, "initialize", json!({}));
        let batch = json!({
            "steps": [
                {"id": "playlist.add_folder", "params": {"path": dir.to_string_lossy()}},
                {"id": "playlist.add_file", "params": {"path": "$0.state.files.1"}},
                {"id": "playlist.clear"},
            ]
        });
        let ran = call(&mut s, 2, "tools/call", json!({"name": "command_batch", "arguments": batch}));
        assert_eq!(ran["result"]["isError"], false, "{}", text_of(&ran));
        let out: Value = serde_json::from_str(&text_of(&ran)).unwrap_or(Value::Null);
        assert_eq!(out["completed"], 3);
        assert_eq!(out["results"][1]["added"], false, "the file is already in the playlist");
        assert_eq!(out["results"][2]["cleared"], 2);

        // A step that fails is named, and nothing after it runs.
        let bad = json!({"steps": [{"id": "playlist.add_file", "params": {"path": "$0.state.files.0"}}, {"id": "playlist.clear"}]});
        let e = call(&mut s, 3, "tools/call", json!({"name": "command_batch", "arguments": bad}));
        assert!(text_of(&e).contains("step 0 (playlist.add_file)"), "{}", text_of(&e));
        std::fs::remove_dir_all(dir).ok();
    }
}
