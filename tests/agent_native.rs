//! The three agent-native paths of the real binary: the MCP server on stdio, the headless script
//! (`run --script`, with `"$N"` references) and `mcp --connect PORT` bridged to a running app.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

/// A folder with two media names, fresh per test (the playlist only looks at the extension).
fn folder(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("local-player-cli-{tag}-{nanos}"));
    std::fs::create_dir_all(&dir).expect("temp folder");
    for name in ["a.mp4", "b.mkv"] {
        std::fs::write(dir.join(name), b"not really media").expect("a media name");
    }
    dir
}

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_local-player-rs"))
}

/// The MCP server on stdio: one JSON-RPC line in, one reply line out.
#[test]
fn the_stdio_server_lists_the_tools_and_adds_a_folder() {
    let dir = folder("stdio");
    let mut child = binary().args(["mcp", "--headless"]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().expect("the app starts");
    let mut stdin = child.stdin.take().expect("stdin");
    writeln!(stdin, r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{}}}}"#).expect("write");
    writeln!(stdin, r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#).expect("write");
    writeln!(stdin, r#"{{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{{}}}}"#).expect("write");
    let added = json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {
        "name": "command_run",
        "arguments": {"id": "playlist.add_folder", "params": {"path": dir.to_string_lossy()}},
    }});
    writeln!(stdin, "{added}").expect("write");
    stdin.flush().expect("flush");
    drop(stdin);
    let output = child.wait_with_output().expect("the server exits at end of input");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let lines: Vec<Value> = String::from_utf8_lossy(&output.stdout).lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    assert_eq!(lines.len(), 3, "a notification gets no reply: {}", String::from_utf8_lossy(&output.stdout));
    assert_eq!(lines[0]["result"]["serverInfo"]["name"], "local-player-rs");
    assert_eq!(lines[1]["result"]["tools"][0]["name"], "command_list");
    let text = lines[2]["result"]["content"][0]["text"].as_str().unwrap_or_default();
    let state: Value = serde_json::from_str(text).expect("the tool answers JSON");
    assert_eq!(state["state"]["count"], 2, "{state}");
    std::fs::remove_dir_all(dir).ok();
}

/// `run --script`: a JSON array of steps, one result per line, `"$N.path"` taken from a step.
#[test]
fn a_script_runs_steps_and_shares_results_by_reference() {
    let dir = folder("script");
    let script = dir.join("job.json");
    let steps = json!([
        {"command": "playlist.add_folder", "params": {"path": dir.to_string_lossy()}},
        {"id": "playlist.add_file", "params": {"path": "$0.state.files.1"}},
        {"id": "playlist.clear"},
    ]);
    std::fs::write(&script, steps.to_string()).expect("the script");
    let output = binary().args(["run", "--script", script.to_string_lossy().as_ref()]).output().expect("the app runs");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let lines: Vec<Value> = String::from_utf8_lossy(&output.stdout).lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
    assert_eq!(lines.len(), 3, "one line per step: {}", String::from_utf8_lossy(&output.stdout));
    assert_eq!(lines[0]["result"]["state"]["count"], 2);
    assert_eq!(lines[1]["result"]["added"], false, "it was already in the playlist");
    assert_eq!(lines[2]["result"]["cleared"], 2);
    // A step that fails stops the script, named by its index.
    std::fs::write(&script, json!([{"id": "playlist.add_file", "params": {"path": dir.join("nope.mp4").to_string_lossy()}}]).to_string())
        .expect("the script");
    let failed = binary().args(["run", "--script", script.to_string_lossy().as_ref()]).output().expect("the app runs");
    assert!(!failed.status.success());
    assert!(
        String::from_utf8_lossy(&failed.stdout).contains("local-player-rs: step 0 (playlist.add_file)"),
        "{}",
        String::from_utf8_lossy(&failed.stdout)
    );
    std::fs::remove_dir_all(dir).ok();
}

/// `mcp --connect PORT`: the MCP server bridged to the app behind a loopback control channel.
#[test]
fn connect_bridges_the_mcp_server_to_a_running_app() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a free port");
    let port = listener.local_addr().expect("local addr").port();
    // The stand-in app: answers every request with the same body, stamped with its own method.
    let app = std::thread::spawn(move || {
        let (stream, _) = listener.accept().expect("the bridge connects");
        let mut reader = BufReader::new(stream.try_clone().expect("clone"));
        let mut writer = stream;
        let mut seen = vec![];
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                return seen;
            }
            let request: Value = serde_json::from_str(line.trim()).expect("a request line");
            seen.push(request["method"].as_str().unwrap_or_default().to_string());
            let reply = json!({"id": request["id"], "ok": true, "result": {"reached": request["method"]}});
            writeln!(writer, "{reply}").expect("reply");
            writer.flush().expect("flush");
        }
    });
    let mut child = binary()
        .args(["mcp", "--connect", &port.to_string()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the app starts");
    let mut stdin = child.stdin.take().expect("stdin");
    writeln!(stdin, r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"command_run","arguments":{{"id":"playback.toggle"}}}}}}"#)
        .expect("write");
    stdin.flush().expect("flush");
    drop(stdin);
    let mut stdout = String::new();
    child.stdout.take().expect("stdout").read_to_string(&mut stdout).expect("read");
    let _ = child.wait();
    let reply: Value = serde_json::from_str(stdout.lines().next().unwrap_or_default()).expect("a reply");
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    let text = reply["result"]["content"][0]["text"].as_str().unwrap_or_default();
    assert!(text.contains("command.execute"), "the call reached the app: {text}");
    let seen = app.join().expect("the stand-in app");
    assert_eq!(seen, ["command.execute"]);
}

/// A host that isn't loopback never gets a connection.
#[test]
fn connect_refuses_a_host_that_is_not_loopback() {
    let output = binary().args(["mcp", "--connect", "192.168.1.5:7979"]).stdin(Stdio::null()).output().expect("the app runs");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("the control channel is loopback only"), "{}", String::from_utf8_lossy(&output.stdout));
}
