//! A stateful stand-in for a running LightCraft app, for tests only.
//!
//! Speaks the control-port protocol (JSON lines, `engine.execute`) on
//! `127.0.0.1:0` and keeps a tiny library: keywords, imported files and the
//! rating of each photo. Records every received line. Built with `std::net`
//! and `serde_json` only. The same file lives in `crates/lc-writer/tests/support`
//! and `crates/lr2lc/tests/support`; keep both copies identical.

#![allow(dead_code)]

use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// The commands that only read or only change the selection; everything else
/// may change the library.
pub const READ_COMMANDS: [&str; 5] = [
    "library.select",
    "library.info",
    "ui.inspect",
    "library.xmpPreferences",
    "keyword.list",
];

/// How `keyword.list` answers.
#[derive(Clone, Debug)]
pub enum KeywordListFormat {
    /// `["a", "a|b"]`
    Texts,
    /// `[{"name":"a","children":[{"name":"b","children":[]}]}]`
    Nested,
    /// `{"keywords": [...texts...]}`
    WrappedTexts,
    /// `{"keywords": [...nested...]}`
    WrappedNested,
    /// The real format: a bare array of `{name, path, count, children}`.
    Real,
    /// An answer no format accepts.
    Garbage,
}

/// How `library.import` names the new id in `imported`.
#[derive(Clone, Debug)]
pub enum ImportIdStyle {
    /// `"imported": [1001]`
    Number,
    /// `"imported": [{"id": 1001}]`
    ObjectId,
    /// `"imported": [{"photoId": 1001}]`
    ObjectPhotoId,
}

/// An error reply for a command, optionally only for one photo id.
#[derive(Clone, Debug)]
pub struct Failure {
    pub command: String,
    pub photo_id: Option<i64>,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct Config {
    /// What `library.info` reports as `path`.
    pub open_library: String,
    pub persistent: bool,
    pub library_problem: Option<String>,
    pub auto_write: bool,
    pub keyword_list: KeywordListFormat,
    pub import_ids: ImportIdStyle,
    pub failures: Vec<Failure>,
    /// Close the connection without a reply when the n-th command line
    /// (1-based, counted over the whole run of the fake) arrives.
    pub close_at: Option<usize>,
    /// Whether the command that gets no reply takes effect anyway.
    pub close_applies: bool,
    /// Keywords the library already has.
    pub keywords: BTreeSet<String>,
    /// Reply `saved in memory but not written to disk` to every update.
    pub not_saved_updates: bool,
    /// `library.import` of a path containing this text answers `failed: 1`.
    pub import_failed_for: Option<String>,
    /// Behave like the real app: `photo.rate`, `photo.flag`, `photo.label`
    /// and `photo.setMeta` fail with `no photos selected` while the selection
    /// is empty, `library.select` sets it and `library.import` selects the
    /// first imported photo. On by default.
    pub selection_gate: bool,
}

impl Config {
    pub fn new(library: &std::path::Path) -> Config {
        Config {
            open_library: library.to_string_lossy().into_owned(),
            persistent: true,
            library_problem: None,
            auto_write: false,
            keyword_list: KeywordListFormat::Real,
            import_ids: ImportIdStyle::Number,
            failures: Vec::new(),
            close_at: None,
            close_applies: false,
            keywords: BTreeSet::new(),
            not_saved_updates: false,
            import_failed_for: None,
            selection_gate: true,
        }
    }
}

/// One received line and the index (from 0) of the connection it came on.
#[derive(Clone, Debug)]
pub struct Received {
    pub conn: usize,
    pub line: String,
}

#[derive(Default)]
struct Library {
    keywords: BTreeSet<String>,
    /// file path -> photo id
    photos: BTreeMap<String, i64>,
    ratings: BTreeMap<i64, i64>,
    /// The ids of the selected photos (empty: nothing selected).
    selection: Vec<i64>,
    next_id: i64,
    seen: usize,
    closed_once: bool,
}

struct Shared {
    config: Config,
    library: Mutex<Library>,
    log: Mutex<Vec<Received>>,
    conns: AtomicUsize,
    stop: AtomicBool,
}

pub struct FakeLightCraft {
    port: u16,
    shared: Arc<Shared>,
    accept: Option<JoinHandle<()>>,
}

impl FakeLightCraft {
    pub fn start(config: Config) -> FakeLightCraft {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fake LightCraft");
        listener.set_nonblocking(true).expect("nonblocking");
        let port = listener.local_addr().expect("addr").port();
        let library = Library {
            keywords: config.keywords.clone(),
            next_id: 1001,
            ..Library::default()
        };
        let shared = Arc::new(Shared {
            config,
            library: Mutex::new(library),
            log: Mutex::new(Vec::new()),
            conns: AtomicUsize::new(0),
            stop: AtomicBool::new(false),
        });
        let accept = {
            let shared = shared.clone();
            thread::spawn(move || {
                while !shared.stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let index = shared.conns.fetch_add(1, Ordering::SeqCst);
                            let shared = shared.clone();
                            thread::spawn(move || serve(stream, index, &shared));
                        }
                        Err(_) => thread::sleep(Duration::from_millis(5)),
                    }
                }
            })
        };
        FakeLightCraft {
            port,
            shared,
            accept: Some(accept),
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn connections(&self) -> usize {
        self.shared.conns.load(Ordering::SeqCst)
    }

    pub fn received(&self) -> Vec<Received> {
        self.shared.log.lock().unwrap().clone()
    }

    /// `(command, params)` of every received `engine.execute` request.
    pub fn commands(&self) -> Vec<(String, Value)> {
        self.received()
            .iter()
            .map(|r| {
                let v: Value = serde_json::from_str(&r.line).expect("client sent invalid JSON");
                (
                    v["params"]["command"].as_str().unwrap_or("").to_string(),
                    v["params"]["params"].clone(),
                )
            })
            .collect()
    }

    /// The received commands that are not read-only.
    pub fn write_commands(&self) -> Vec<(String, Value)> {
        self.commands()
            .into_iter()
            .filter(|(c, _)| !READ_COMMANDS.contains(&c.as_str()))
            .collect()
    }

    /// The selection at this moment.
    pub fn selection(&self) -> Vec<i64> {
        self.shared.library.lock().unwrap().selection.clone()
    }

    /// Keyword path texts in the fake library.
    pub fn keywords(&self) -> BTreeSet<String> {
        self.shared.library.lock().unwrap().keywords.clone()
    }

    /// Imported files and their ids.
    pub fn photos(&self) -> BTreeMap<String, i64> {
        self.shared.library.lock().unwrap().photos.clone()
    }

    /// Rating of each photo, by photo id (only photos that were rated).
    pub fn ratings(&self) -> BTreeMap<i64, i64> {
        self.shared.library.lock().unwrap().ratings.clone()
    }
}

impl Drop for FakeLightCraft {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.accept.take() {
            let _ = handle.join();
        }
    }
}

fn serve(stream: TcpStream, index: usize, shared: &Shared) {
    let _ = stream.set_nonblocking(false);
    // Never let a connection thread live forever.
    let _ = stream.set_read_timeout(Some(Duration::from_secs(20)));
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let line = line.trim_end_matches(['\r', '\n']).to_string();
        shared.log.lock().unwrap().push(Received {
            conn: index,
            line: line.clone(),
        });
        let request: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let command = request["params"]["command"].as_str().unwrap_or("");
        let params = &request["params"]["params"];

        let mut lib = shared.library.lock().unwrap();
        lib.seen += 1;
        let close_now = shared.config.close_at == Some(lib.seen) && !lib.closed_once;
        if close_now {
            lib.closed_once = true;
            if shared.config.close_applies {
                let _ = run(shared, &mut lib, command, params);
            }
            drop(lib);
            let _ = writer.shutdown(Shutdown::Both);
            return;
        }
        let out = match run(shared, &mut lib, command, params) {
            Ok(result) => json!({"id": id, "ok": true, "result": result}),
            Err(text) => json!({"id": id, "ok": false, "error": text}),
        };
        drop(lib);
        if writeln!(writer, "{out}").is_err() || writer.flush().is_err() {
            return;
        }
    }
}

fn run(shared: &Shared, lib: &mut Library, command: &str, params: &Value) -> Result<Value, String> {
    let config = &shared.config;
    let photo_id = params["ids"][0].as_i64();
    for f in &config.failures {
        if f.command == command && (f.photo_id.is_none() || f.photo_id == photo_id) {
            return Err(f.message.clone());
        }
    }
    match command {
        "library.info" => Ok(json!({
            "persistent": config.persistent,
            "path": config.open_library,
        })),
        "ui.inspect" => Ok(json!({"libraryProblem": config.library_problem})),
        "library.xmpPreferences" => Ok(json!({"autoWrite": config.auto_write})),
        "library.select" => {
            let ids = params["ids"].as_array().ok_or("ids is missing")?;
            let ids: Vec<i64> = ids.iter().filter_map(Value::as_i64).collect();
            if ids.len() != params["ids"].as_array().map_or(0, Vec::len) {
                return Err("ids must be numbers".into());
            }
            if let Some(unknown) = ids.iter().find(|i| !lib.photos.values().any(|p| p == *i)) {
                return Err(format!("unknown photo id {unknown}"));
            }
            lib.selection = ids;
            Ok(json!({}))
        }
        "keyword.list" => Ok(keyword_list(&config.keyword_list, &lib.keywords)),
        "keyword.create" => {
            let name = params["name"].as_str().ok_or("name is missing")?;
            let path = match params["parent"].as_str() {
                Some(parent) if !lib.keywords.contains(parent) => {
                    return Err("parent keyword does not exist".into());
                }
                Some(parent) => format!("{parent}|{name}"),
                None => name.to_string(),
            };
            if !lib.keywords.insert(path) {
                return Err("keyword already exists".into());
            }
            Ok(json!({}))
        }
        "library.import" => {
            let expected = params.as_object().is_some_and(|m| m.len() == 2)
                && params["mode"] == "add"
                && params["paths"].as_array().is_some_and(|a| a.len() == 1);
            if !expected {
                return Err("unexpected import parameters".into());
            }
            let path = params["paths"][0].as_str().ok_or("path is not text")?;
            if config
                .import_failed_for
                .as_deref()
                .is_some_and(|t| path.contains(t))
            {
                return Ok(json!({"imported": [], "duplicates": [], "failed": 1}));
            }
            if let Some(existing) = lib.photos.get(path) {
                return Ok(json!({
                    "imported": [],
                    "duplicates": [{"path": path, "existing": existing, "reason": "same file"}],
                    "failed": 0,
                }));
            }
            let id = lib.next_id;
            lib.next_id += 1;
            lib.photos.insert(path.to_string(), id);
            if config.selection_gate {
                lib.selection = vec![id];
            }
            let imported = match config.import_ids {
                ImportIdStyle::Number => json!(id),
                ImportIdStyle::ObjectId => json!({"id": id}),
                ImportIdStyle::ObjectPhotoId => json!({"photoId": id}),
            };
            Ok(json!({"imported": [imported], "duplicates": [], "failed": 0}))
        }
        "photo.rate" | "photo.flag" | "photo.label" | "photo.setMeta" => {
            if config.selection_gate && lib.selection.is_empty() {
                return Err("no photos selected".into());
            }
            let id = photo_id.filter(|id| lib.photos.values().any(|p| p == id));
            let id = id.ok_or("unknown photo id")?;
            if command == "photo.rate" {
                lib.ratings
                    .insert(id, params["rating"].as_i64().unwrap_or(-1));
            }
            if config.not_saved_updates {
                return Err("saved in memory but not written to disk: disk full".into());
            }
            Ok(json!({}))
        }
        other => Err(format!("unknown command {other}")),
    }
}

fn keyword_list(format: &KeywordListFormat, keywords: &BTreeSet<String>) -> Value {
    let texts: Vec<&String> = keywords.iter().collect();
    match format {
        KeywordListFormat::Texts => json!(texts),
        KeywordListFormat::Nested => json!(nested(keywords, "")),
        KeywordListFormat::WrappedTexts => json!({"keywords": texts}),
        KeywordListFormat::WrappedNested => json!({"keywords": nested(keywords, "")}),
        KeywordListFormat::Real => json!(real_nested(keywords, "")),
        KeywordListFormat::Garbage => json!("no keywords here"),
    }
}

/// The keywords below `prefix` (path text, empty for the top level).
fn nested(keywords: &BTreeSet<String>, prefix: &str) -> Vec<Value> {
    let depth = if prefix.is_empty() {
        0
    } else {
        prefix.split('|').count()
    };
    keywords
        .iter()
        .filter(|k| {
            let parts: Vec<&str> = k.split('|').collect();
            parts.len() == depth + 1 && (prefix.is_empty() || k.starts_with(&format!("{prefix}|")))
        })
        .map(|k| {
            let name = k.rsplit('|').next().unwrap_or(k);
            json!({"name": name, "children": nested(keywords, k)})
        })
        .collect()
}

/// The real `keyword.list` nodes below `prefix`: `{name, path, count,
/// children}`. Ancestors of a keyword appear as levels of their own.
fn real_nested(keywords: &BTreeSet<String>, prefix: &str) -> Vec<Value> {
    let depth = if prefix.is_empty() {
        0
    } else {
        prefix.split('|').count()
    };
    let mut levels = BTreeSet::new();
    for k in keywords {
        let parts: Vec<&str> = k.split('|').collect();
        if parts.len() > depth {
            let level = parts[..=depth].join("|");
            if prefix.is_empty() || level.starts_with(&format!("{prefix}|")) {
                levels.insert(level);
            }
        }
    }
    levels
        .iter()
        .map(|level| {
            let name = level.rsplit('|').next().unwrap_or(level);
            json!({
                "name": name,
                "path": level,
                "count": 0,
                "children": real_nested(keywords, level),
            })
        })
        .collect()
}
