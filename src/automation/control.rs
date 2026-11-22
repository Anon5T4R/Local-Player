//! The running app's control channel (`local-player-rs --control PORT`): one JSON request per line
//! (`{"id": ., "method": ., "params": .}`), one JSON reply per line
//! (`{"id": ., "ok": true, "result": .}` or `{"id": ., "ok": false, "error": .}`). It is the
//! protocol [`craft_mcp::remote::Remote`] speaks, so `local-player-rs mcp --connect PORT` drives the
//! window through it and an agent sees the same commands the palette has.
//!
//! The listener is loopback only (`127.0.0.1`) and exists only while the app is started with
//! `--control`. A loopback port is **not** authentication: any process of this machine can drive the
//! window while it is open, as with Local Vector's channel. What the commands may touch is
//! confined by `--automation-read-root` when it is given.
//!
//! Requests are answered by the window's own frame ([`Control::pump`]): the listener threads only
//! queue them, so the commands run with the app's state and the window stays responsive.

use std::io::{BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use craft_mcp::jsonrpc::{read_line_capped, write_line};
use serde_json::{Value, json};

/// Longest accepted request line: parameters carry paths, not documents.
pub const MAX_LINE_BYTES: usize = 1024 * 1024;
/// How many connections are served at once.
pub const MAX_CONNECTIONS: usize = 8;
/// Most requests one frame runs: a client flooding the port must not freeze the window.
pub const MAX_PER_FRAME: usize = 64;

/// One request waiting for the window's frame.
pub struct Request {
    pub method: String,
    pub params: Value,
    pub reply: Sender<Result<Value, String>>,
}

/// The listening control channel of a running app.
pub struct Control {
    port: u16,
    requests: Receiver<Request>,
}

impl Control {
    /// Listen on `127.0.0.1:port` (0 picks a free one). Fails when the port is taken, so a second
    /// app started with the same port does not silently steal the requests.
    pub fn start(port: u16) -> Result<Self, String> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
            .map_err(|e| format!("cannot listen on 127.0.0.1:{port}: {e} (choose another port)"))?;
        let port = listener.local_addr().map_err(|e| format!("control listener: {e}"))?.port();
        let (tx, requests) = channel();
        let connections = Arc::new(AtomicUsize::new(0));
        std::thread::Builder::new()
            .name("control-listener".into())
            .spawn(move || accept_loop(listener, tx, connections))
            .map_err(|e| format!("cannot start the control listener: {e}"))?;
        Ok(Self { port, requests })
    }

    /// The port the channel listens on (`--control 0` picks one).
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Answer the requests that arrived, in order, and return how many ran.
    pub fn pump(&mut self, mut dispatch: impl FnMut(&str, &Value) -> Result<Value, String>) -> usize {
        let mut served = 0;
        while served < MAX_PER_FRAME {
            match self.requests.try_recv() {
                Ok(request) => {
                    let outcome = dispatch(&request.method, &request.params);
                    // The caller may have given up (its own timeout) and dropped the receiver:
                    // there is nobody left to tell, which is not this app's error.
                    let _ = request.reply.send(outcome);
                    served += 1;
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
        served
    }
}

fn accept_loop(listener: TcpListener, tx: Sender<Request>, connections: Arc<AtomicUsize>) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        // fetch_add first, so two threads agreeing on the same count cannot both pass the limit.
        if connections.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
            connections.fetch_sub(1, Ordering::SeqCst);
            // No id: the client sent nothing this line answers, and `Remote` reads it as a refusal
            // that ran nothing.
            refuse(stream, "too many control connections");
            continue;
        }
        let tx = tx.clone();
        let counter = Arc::clone(&connections);
        let spawned = std::thread::Builder::new().name("control-client".into()).spawn(move || {
            serve_client(stream, tx);
            counter.fetch_sub(1, Ordering::SeqCst);
        });
        if spawned.is_err() {
            connections.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// Answer one client: one request per line, one reply per line, until it hangs up.
fn serve_client(stream: TcpStream, tx: Sender<Request>) {
    let Ok(read) = stream.try_clone() else { return };
    let mut reader = BufReader::new(read);
    let mut writer = stream;
    let mut line = Vec::new();
    loop {
        line.clear();
        match read_line_capped(&mut reader, &mut line, MAX_LINE_BYTES) {
            Ok(true) => {}
            _ => return,
        }
        if line.len() > MAX_LINE_BYTES {
            refuse(writer, "request line too long");
            return;
        }
        let request = match std::str::from_utf8(&line).map_err(|_| "the request is not UTF-8".to_string()).and_then(parse) {
            Ok(request) => request,
            Err(why) => {
                refuse(writer, &why);
                return;
            }
        };
        let (reply_tx, reply_rx) = channel();
        let queued = Request { method: request.method, params: request.params, reply: reply_tx };
        if tx.send(queued).is_err() {
            refuse(writer, "the app is closing");
            return;
        }
        let outcome = match reply_rx.recv() {
            Ok(outcome) => outcome,
            Err(_) => {
                refuse(writer, "the app closed the request");
                return;
            }
        };
        let body = match outcome {
            Ok(result) => json!({"id": request.id, "ok": true, "result": result}),
            Err(error) => json!({"id": request.id, "ok": false, "error": error}),
        };
        if write_line(&mut writer, &body.to_string()).is_err() {
            return;
        }
    }
}

struct Parsed {
    id: Value,
    method: String,
    params: Value,
}

/// One request line: `{"id": ., "method": ., "params": .}`. The id goes back untouched; without one
/// the client could not match the reply, so it is required.
fn parse(line: &str) -> Result<Parsed, String> {
    let value: Value = serde_json::from_str(line.trim()).map_err(|e| format!("bad request JSON: {e}"))?;
    let Value::Object(mut object) = value else { return Err("a request must be a JSON object".into()) };
    let id = object.remove("id").ok_or("missing `id`")?;
    if !(id.is_string() || id.is_number()) {
        return Err("`id` must be a string or a number".into());
    }
    let method = object.remove("method").and_then(|m| m.as_str().map(str::to_string)).ok_or("missing string `method`")?;
    let params = object.remove("params").unwrap_or(Value::Null);
    Ok(Parsed { id, method, params })
}

/// Tell the client why this line ran nothing, without an id (the connection is then closed).
fn refuse(mut stream: TcpStream, why: &str) {
    let _ = write_line(&mut stream, &json!({"ok": false, "error": why}).to_string());
    let _ = stream.flush();
}

#[cfg(test)]
mod tests {
    use std::io::BufRead;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::automation::session::Session;

    /// Answer requests until the client's calls come back, so a test is not a busy loop.
    fn answer(control: &mut Control, session: &mut Session, until: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !until() {
            assert!(Instant::now() < deadline, "the control channel never answered");
            control.pump(|method, params| session.call(method, params));
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn a_request_is_answered_over_loopback() {
        let mut control = Control::start(0).expect("a free loopback port");
        let mut session = Session::new(None);
        let dir = std::env::temp_dir().join(format!("local-player-control-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp folder");
        std::fs::write(dir.join("a.mp4"), b"not really media").expect("a media name");
        let addr = format!("127.0.0.1:{}", control.port());
        let asked = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = std::sync::Arc::clone(&asked);
        let path = dir.to_string_lossy().to_string();
        let client = std::thread::spawn(move || {
            let mut remote = craft_mcp::remote::Remote::connect(&addr).expect("the channel listens");
            let listed = remote.call("command.list", serde_json::json!({"filter": "playlist"})).expect("command.list");
            let added = remote.call("command.execute", serde_json::json!({"command": "playlist.add_folder", "params": {"path": path}})).expect("add");
            // A method the app has not is an error, not a dropped connection.
            let refused = remote.call("nope.nothing", serde_json::json!({})).expect_err("unknown method");
            flag.store(true, Ordering::SeqCst);
            (listed, added, refused)
        });
        answer(&mut control, &mut session, || asked.load(Ordering::SeqCst));
        let (listed, added, refused) = client.join().expect("the client thread");
        assert_eq!(listed[0]["id"], "playlist.add_file");
        assert_eq!(added["state"]["count"], 1);
        assert!(refused.contains("unknown method"), "{refused}");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_bad_line_is_refused_and_the_channel_keeps_its_port() {
        let control = Control::start(0).expect("a free loopback port");
        let port = control.port();
        assert_ne!(port, 0);
        let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
        stream.write_all(b"not json\n").expect("write");
        let mut reply = String::new();
        BufReader::new(stream.try_clone().expect("clone")).read_line(&mut reply).expect("read");
        assert!(reply.contains("\"ok\":false") && reply.contains("bad request JSON"), "{reply}");
        // The listener is still there for the next client.
        assert!(TcpStream::connect(("127.0.0.1", port)).is_ok());
    }

    #[test]
    fn the_port_is_loopback_only_and_taken_ports_are_reported() {
        let control = Control::start(0).expect("a free loopback port");
        match Control::start(control.port()) {
            Ok(_) => panic!("the port is taken, so a second listener cannot have it"),
            Err(e) => assert!(e.contains("cannot listen on 127.0.0.1"), "{e}"),
        }
    }
}
