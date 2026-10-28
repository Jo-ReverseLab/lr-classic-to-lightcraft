//! A tiny stand-in for the LightCraft control port, for tests only.
//!
//! Listens on `127.0.0.1:0`, speaks JSON lines, records every received line
//! (with the index of the connection it arrived on) and answers through a
//! handler closure. Uses only `std::net` and `serde_json`.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// What the fake port does after receiving one request line.
pub enum Reply {
    /// `{"id":<echoed>,"ok":true,"result":<value>}`
    Ok(Value),
    /// `{"id":<echoed>,"ok":false,"error":<text>}`
    Err(String),
    /// Send this text as the reply line, verbatim.
    Raw(String),
    /// Close the connection without answering.
    Close,
}

/// One received line and the index (from 0) of the connection it came on.
#[derive(Clone, Debug)]
pub struct Received {
    pub conn: usize,
    pub line: String,
}

pub struct FakePort {
    port: u16,
    log: Arc<Mutex<Vec<Received>>>,
    conns: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    accept: Option<JoinHandle<()>>,
}

impl FakePort {
    /// Start listening. `handler(connection_index, parsed_request)` decides
    /// the reply for each request line.
    pub fn start<F>(handler: F) -> FakePort
    where
        F: Fn(usize, &Value) -> Reply + Send + Sync + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake port");
        listener.set_nonblocking(true).expect("nonblocking");
        let port = listener.local_addr().expect("addr").port();
        let log = Arc::new(Mutex::new(Vec::new()));
        let conns = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let handler = Arc::new(handler);

        let accept = {
            let (log, conns, stop) = (log.clone(), conns.clone(), stop.clone());
            thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let index = conns.fetch_add(1, Ordering::SeqCst);
                            let (log, handler) = (log.clone(), handler.clone());
                            thread::spawn(move || serve(stream, index, log, handler));
                        }
                        Err(_) => thread::sleep(Duration::from_millis(5)),
                    }
                }
            })
        };
        FakePort {
            port,
            log,
            conns,
            stop,
            accept: Some(accept),
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Every received line, in arrival order.
    pub fn received(&self) -> Vec<Received> {
        self.log.lock().unwrap().clone()
    }

    /// Every received line parsed as JSON (panics on invalid JSON).
    pub fn requests(&self) -> Vec<Value> {
        self.received()
            .iter()
            .map(|r| serde_json::from_str(&r.line).expect("client sent invalid JSON"))
            .collect()
    }

    /// `(command, params)` of every received request: the inner command of
    /// `engine.execute`, or the method name and params of a request to a
    /// method of the port itself (such as `ui.inspect`).
    pub fn commands(&self) -> Vec<(String, Value)> {
        self.requests().iter().map(request_parts).collect()
    }

    /// The `method` of every received request, in order.
    pub fn methods(&self) -> Vec<String> {
        self.requests()
            .iter()
            .map(|r| r["method"].as_str().unwrap_or("").to_string())
            .collect()
    }

    /// Number of connections accepted so far.
    pub fn connections(&self) -> usize {
        self.conns.load(Ordering::SeqCst)
    }
}

/// `(command, params)` of a request, covering both forms.
pub fn request_parts(request: &Value) -> (String, Value) {
    if request["method"].as_str() == Some("engine.execute") {
        (
            request["params"]["command"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            request["params"]["params"].clone(),
        )
    } else {
        (
            request["method"].as_str().unwrap_or("").to_string(),
            request["params"].clone(),
        )
    }
}

impl Drop for FakePort {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.accept.take() {
            let _ = handle.join();
        }
    }
}

type Handler = Arc<dyn Fn(usize, &Value) -> Reply + Send + Sync>;

fn serve(stream: TcpStream, index: usize, log: Arc<Mutex<Vec<Received>>>, handler: Handler) {
    // Accepted sockets may inherit the listener's non-blocking mode.
    let _ = stream.set_nonblocking(false);
    // Never let a connection thread live forever.
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut writer = match stream.try_clone() {
        Ok(w) => w,
        Err(_) => return,
    };
    let mut reader = BufReader::new(stream);
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let line = line.trim_end_matches(['\r', '\n']).to_string();
        log.lock().unwrap().push(Received {
            conn: index,
            line: line.clone(),
        });
        let request: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let out = match handler(index, &request) {
            Reply::Ok(result) => json!({"id": id, "ok": true, "result": result}).to_string(),
            Reply::Err(text) => json!({"id": id, "ok": false, "error": text}).to_string(),
            Reply::Raw(text) => text,
            Reply::Close => {
                let _ = writer.shutdown(Shutdown::Both);
                return;
            }
        };
        if writeln!(writer, "{out}").is_err() || writer.flush().is_err() {
            return;
        }
    }
}
