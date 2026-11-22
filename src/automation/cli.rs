//! The agent-native command line of Local Player: `mcp` (the MCP server on stdio), `run` (a script
//! of commands, headless), `commands` (the catalogue) and their usage. Everything else belongs to
//! the window or to the diagnostic CLI (`--probe`, `--bench`, `--help`, `--version`).
//!
//! The plumbing is the suite's `craft-mcp`: the server, the script steps with `"$N"` references and
//! the `--automation-read-root`/`--automation-write-root` flags. The commands are Local Player's
//! ([`super::catalogue`]), so the CLI, the MCP server and the palette reach the same ids.

use std::io::Write;

use craft_mcp::automation::RootFlags;
use craft_mcp::remote::Remote;
use serde_json::{Value, json};

use super::catalogue;
use super::mcp::{Automation, Backend};
use super::session::Session;
use crate::cli::output;

/// How the agent-native command line is used (the window's own help is `--help` of [`crate::cli`]).
pub const USAGE: &str = "\
local-player-rs — Local Player automation

USAGE:
  local-player-rs mcp [--connect PORT|HOST:PORT | --headless] [--automation-read-root DIR]...
                      [--automation-write-root DIR]...
      Run the MCP server on stdio. Default: drive a running app at 127.0.0.1:7979, falling back to
      a headless session. --connect takes PORT (loopback) or HOST:PORT; a host that isn't loopback
      is refused. The root flags confine the files the commands read (a kind left out grants none of
      its access) and imply a headless session: a running app enforces its own.
  local-player-rs run [--cmd ID [--params JSON] | --script FILE|-]...
      Headless: run the commands in order and print one JSON result per step. --script (or
      --commands) reads a JSON array of {\"command\": ID, \"params\": {…}} objects, or one per line;
      a parameter that is exactly \"$N.path\" takes that value from step N's result.
  local-player-rs commands [--search TEXT]
      Print the command catalogue as JSON; --search keeps the ids, labels or menus containing TEXT.

Playing needs the window: start it with `local-player-rs --control PORT` and point an MCP client at
`local-player-rs mcp --connect PORT`. A headless session manages the playlist and probes media.
";

/// Run the agent-native command line when `args` ask for it; `None` means the window or the
/// diagnostic CLI (the default).
pub fn run(args: &[String]) -> Option<i32> {
    let outcome = match args.first().map(String::as_str) {
        Some("mcp") => mcp(&args[1..]),
        Some("run") => batch(&args[1..]),
        Some("commands") => commands(&args[1..]),
        _ => return None,
    };
    Some(match outcome {
        Ok(()) => 0,
        Err(e) => {
            let mut out = output();
            let _ = writeln!(out, "local-player-rs: {e}");
            1
        }
    })
}

/// The agent-native usage.
fn help() -> Result<(), String> {
    let mut out = output();
    let _ = write!(out, "{USAGE}");
    Ok(())
}

/// `mcp [--connect …|--headless] [roots…]`: the MCP server on stdio.
fn mcp(args: &[String]) -> Result<(), String> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return help();
    }
    let (flags, rest) = RootFlags::take(args)?;
    let mut connect: Option<String> = None;
    let mut headless = false;
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--connect" => connect = Some(craft_mcp::remote::normalize_addr(it.next().ok_or("--connect needs an address")?)?),
            other if other.starts_with("--connect=") => connect = Some(craft_mcp::remote::normalize_addr(other.trim_start_matches("--connect="))?),
            "--headless" => headless = true,
            other => return Err(format!("unknown mcp option `{other}`")),
        }
    }
    let roots = flags.roots()?;
    if headless && connect.is_some() {
        return Err("use either --connect or --headless".into());
    }
    if roots.is_some() && connect.is_some() {
        return Err("--automation-read-root and --automation-write-root confine the headless server; a running app enforces its own roots: start it with `local-player-rs --control PORT --automation-read-root DIR`".into());
    }
    let backend: Box<dyn Backend> = if let Some(roots) = roots {
        Box::new(Session::new(Some(roots)))
    } else if headless {
        Box::new(Session::new(None))
    } else if let Some(addr) = connect {
        // An explicit address fails loudly: the agent asked for that app.
        Box::new(Remote::connect(&addr).map_err(|e| format!("cannot connect to {addr}: {e} (start the app with `local-player-rs --control PORT`)"))?)
    } else {
        match Remote::connect(craft_mcp::remote::DEFAULT_ADDR) {
            Ok(remote) => Box::new(remote),
            Err(_) => Box::new(Session::new(None)),
        }
    };
    let mut err = std::io::stderr();
    let _ = writeln!(err, "local-player-rs: MCP server on stdio ({})", backend.describe());
    let mut server = craft_mcp::Server::new(Automation::new(backend));
    server.serve(std::io::stdin().lock(), std::io::stdout().lock()).map_err(|e| e.to_string())
}

/// `run [--cmd ID [--params JSON] | --script FILE|-]…`: a headless script of commands.
fn batch(args: &[String]) -> Result<(), String> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return help();
    }
    let (flags, rest) = RootFlags::take(args)?;
    let mut session = Session::new(flags.roots()?);
    let mut steps: Vec<(String, Value)> = vec![];
    // The step of the `--cmd` just given, which the next `--params` fills.
    let mut params_for: Option<usize> = None;
    let mut stdin_read = false;
    let mut it = rest.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--cmd" | "-c" => {
                steps.push((it.next().cloned().ok_or("--cmd needs a command id")?, json!({})));
                params_for = Some(steps.len() - 1);
            }
            "--params" | "-p" => {
                let raw = it.next().cloned().ok_or("--params needs JSON")?;
                let params: Value = serde_json::from_str(&raw).map_err(|e| format!("--params {raw}: {e}"))?;
                if !params.is_object() {
                    return Err(format!("--params must be a JSON object, got {raw}"));
                }
                match params_for.and_then(|i| steps.get_mut(i)) {
                    Some((_, slot)) => *slot = params,
                    None => return Err("--params must follow a --cmd".into()),
                }
            }
            "--script" | "--commands" => {
                let flag = arg.clone();
                let source = it.next().cloned().ok_or(format!("{flag} needs a file, or - for standard input"))?;
                if source == "-" && std::mem::replace(&mut stdin_read, true) {
                    return Err(format!("{flag} - is given twice: standard input is read once"));
                }
                let text = craft_mcp::script::read_source(&source, craft_mcp::script::MAX_SOURCE_BYTES).map_err(|e| format!("{flag} {e}"))?;
                let parsed = craft_mcp::script::parse_commands(&format!("{flag} {source}"), &text, steps.len())?;
                steps.extend(parsed.into_iter().map(|c| (c.id, c.params)));
                params_for = None;
            }
            other => return Err(format!("unknown run option `{other}`")),
        }
    }
    let mut out = output();
    let mut results: Vec<Value> = vec![];
    for (n, (id, params)) in steps.into_iter().enumerate() {
        let params = craft_mcp::script::resolve(&params, &results).map_err(|e| format!("step {n} ({id}): {e}"))?;
        let result = session.call("command.execute", &json!({"command": id, "params": params})).map_err(|e| format!("step {n} ({id}): {e}"))?;
        let _ = writeln!(out, "{}", json!({"step": "cmd", "command": id, "result": result}));
        results.push(result);
    }
    Ok(())
}

/// `commands [--search TEXT]`: the catalogue as JSON.
fn commands(args: &[String]) -> Result<(), String> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return help();
    }
    let mut search: Option<String> = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--search" => search = Some(it.next().cloned().ok_or("--search needs text")?),
            other => return Err(format!("unknown commands option `{other}`")),
        }
    }
    let list = Value::Array(catalogue::search(search.as_deref(), false));
    let mut out = output();
    let _ = writeln!(out, "{}", serde_json::to_string_pretty(&list).unwrap_or_default());
    Ok(())
}
