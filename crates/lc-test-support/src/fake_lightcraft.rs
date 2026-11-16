//! A stateful stand-in for a running LightCraft app, for tests only.
//!
//! Speaks the control-port protocol (JSON lines, `engine.execute` plus the port method `ui.inspect`) on
//! `127.0.0.1:0` and keeps a tiny library: keywords, imported files and the
//! rating of each photo. Records every received line. Built with `std::net`
//! and `serde_json` only.

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
pub const READ_COMMANDS: [&str; 8] = [
    "photo.inspect",
    "library.select",
    "catalog.query",
    "albums.list",
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

/// An error reply for an album command, optionally only for the album with
/// this name (`album.create`: the name sent; `album.addPhotos` and
/// `album.removePhotos`: the name of the album the id refers to).
#[derive(Clone, Debug)]
pub struct AlbumFailure {
    pub command: String,
    pub album_name: Option<String>,
    pub message: String,
}

/// One stack of the fake library: `photos[0]` is the top.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackView {
    pub id: u64,
    pub photos: Vec<i64>,
    pub collapsed: bool,
}

/// One album or album folder of the fake library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlbumView {
    pub id: u64,
    pub name: String,
    pub folder: bool,
    /// The id of the enclosing folder; `None` on top level.
    pub parent: Option<u64>,
    /// The ids of the member photos, in the order they were added.
    pub photos: Vec<i64>,
    /// The rules of a smart album (`album.createSmart`), as received;
    /// `None` for regular albums and folders.
    pub rules: Option<Value>,
    /// Internal mark of the Quick Collection, like the real app: set only
    /// when `album.toggleTarget` creates it (or for a seeded top-level album
    /// of that name), never by `album.create`, and not shown by
    /// `albums.list`.
    quick: bool,
}

/// An album or album folder the fake library already has at start.
#[derive(Clone, Debug)]
pub struct SeedAlbum {
    pub name: String,
    pub folder: bool,
    /// Index of the enclosing folder in `Config::albums` (must be smaller
    /// than the own index); `None` on top level.
    pub parent: Option<usize>,
    /// `Some` makes the seed a smart album with these (translated) rules.
    pub rules: Option<Value>,
}

impl SeedAlbum {
    pub fn smart(name: &str, parent: Option<usize>, rules: Value) -> SeedAlbum {
        SeedAlbum {
            name: name.to_string(),
            folder: false,
            parent,
            rules: Some(rules),
        }
    }

    pub fn album(name: &str, parent: Option<usize>) -> SeedAlbum {
        SeedAlbum {
            name: name.to_string(),
            folder: false,
            parent,
            rules: None,
        }
    }

    pub fn folder(name: &str, parent: Option<usize>) -> SeedAlbum {
        SeedAlbum {
            name: name.to_string(),
            folder: true,
            parent,
            rules: None,
        }
    }
}

/// A photo the fake library already has at start, with its field values as
/// `catalog.query` summaries and `photo.inspect` report them. Its file path
/// is `/Users/example/Pictures/seed-<id>.jpg`.
#[derive(Clone, Debug)]
pub struct SeedPhoto {
    pub id: i64,
    pub rating: i64,
    /// `none`, `pick` or `reject`.
    pub flag: String,
    pub label: Option<String>,
    /// An empty LightCraft title is `""`.
    pub title: String,
    /// Keyword path texts (`a|b`).
    pub keywords: Vec<String>,
    pub caption: Option<String>,
    pub copyright: Option<String>,
    pub creator: Option<String>,
    /// Whether LightCraft has the photo in the trash ("Recently Deleted").
    /// A deleted photo is listed only by `catalog.query {deleted: true}`, but
    /// `photo.inspect` still answers for it.
    pub deleted: bool,
}

impl SeedPhoto {
    /// A photo as LightCraft creates it on import: no stars, no flag, no
    /// label, empty texts, no keywords, not deleted.
    pub fn plain(id: i64) -> SeedPhoto {
        SeedPhoto {
            id,
            rating: 0,
            flag: "none".to_string(),
            label: None,
            title: String::new(),
            keywords: Vec::new(),
            caption: None,
            copyright: None,
            creator: None,
            deleted: false,
        }
    }
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
    /// When one of these command lines (1-based, counted over the whole run
    /// of the fake) arrives, answer with the app's idle-close notice instead
    /// (no `id`, the command is not run) and close the connection, like an
    /// app whose idle timeout expired just as the request was sent.
    pub idle_close_at: Vec<usize>,
    /// Send the idle notice without the `closed` marker, like apps from
    /// before LightCraft 0.21.
    pub idle_notice_without_marker: bool,
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
    /// Error replies for album commands.
    pub album_failures: Vec<AlbumFailure>,
    /// Albums and folders the library has at start (ids 5001, 5002, ...
    /// in this order; names are kept as given, not trimmed).
    pub albums: Vec<SeedAlbum>,
    /// Answer `albums.list` with an object instead of the nested array.
    pub albums_list_garbage: bool,
    /// `album.create` and `album.createSmart` apply but answer without `id`.
    pub albums_no_id: bool,
    /// `library.import` applies but answers `imported: []` (no id, no
    /// duplicate, not failed).
    pub import_no_id: bool,
    /// `album.create`, `album.createSmart` and `library.import` apply, then
    /// answer `saved in memory but not written to disk` (no id).
    pub not_saved_creates: bool,
    /// `keyword.create` applies, then answers `saved in memory but not
    /// written to disk`. The keyword is not added to the library, so the
    /// next run's `keyword.list` does not return it and it is created again
    /// (modelling an app that lost the unsaved keyword on restart).
    pub not_saved_keyword_creates: bool,
    /// Photos the library has at start (read side: `catalog.query`
    /// summaries and `photo.inspect`).
    pub seed_photos: Vec<SeedPhoto>,
    /// Members of seeded albums: `(index in `albums`, photo id)`. The photo
    /// id need not exist (a member the mapping file does not know).
    pub seed_members: Vec<(usize, i64)>,
    /// `catalog.query` summaries carry `rating` as a text, not a number.
    pub summary_broken: bool,
    /// `catalog.query` ignores the `deleted` filter and lists every photo in
    /// both streams, so a photo can appear in both. Models a library where
    /// the two queries overlap; the reader must merge the ids, not duplicate
    /// them.
    pub ignore_deleted_filter: bool,
    /// The content of a file by path, for duplicate detection by content:
    /// importing a path whose content equals that of a photo already in the
    /// library answers it as a duplicate with reason `content`, like the real
    /// app (which hashes the bytes). Paths not listed have unique content.
    pub file_contents: BTreeMap<String, String>,
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
            idle_close_at: Vec::new(),
            idle_notice_without_marker: false,
            keywords: BTreeSet::new(),
            not_saved_updates: false,
            import_failed_for: None,
            selection_gate: true,
            album_failures: Vec::new(),
            albums: Vec::new(),
            albums_list_garbage: false,
            albums_no_id: false,
            import_no_id: false,
            not_saved_creates: false,
            not_saved_keyword_creates: false,
            seed_photos: Vec::new(),
            seed_members: Vec::new(),
            summary_broken: false,
            ignore_deleted_filter: false,
            file_contents: BTreeMap::new(),
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
    /// Field values of the seeded photos, by photo id.
    seeds: BTreeMap<i64, SeedPhoto>,
    /// The ids of the selected photos (empty: nothing selected).
    selection: Vec<i64>,
    /// album id -> album
    albums: BTreeMap<u64, AlbumView>,
    /// stack id -> stack
    stacks: BTreeMap<u64, StackView>,
    next_stack_id: u64,
    next_id: i64,
    /// Session state of `album.setTarget` (`None`: the Quick Collection).
    target_album: Option<u64>,
    next_album_id: u64,
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
        let mut library = Library {
            keywords: config.keywords.clone(),
            next_id: 1001,
            next_album_id: 5001,
            next_stack_id: 9001,
            ..Library::default()
        };
        for seed in &config.albums {
            let id = library.next_album_id;
            library.next_album_id += 1;
            let parent = seed.parent.map(|i| 5001 + i as u64);
            assert!(parent.is_none_or(|p| p < id), "seed parent must come first");
            library.albums.insert(
                id,
                AlbumView {
                    id,
                    name: seed.name.clone(),
                    folder: seed.folder,
                    parent,
                    photos: Vec::new(),
                    rules: seed.rules.clone(),
                    quick: parent.is_none() && !seed.folder && seed.name == "Quick Collection",
                },
            );
        }
        for seed in &config.seed_photos {
            library.photos.insert(
                format!("/Users/example/Pictures/seed-{}.jpg", seed.id),
                seed.id,
            );
            library.seeds.insert(seed.id, seed.clone());
        }
        for (index, photo) in &config.seed_members {
            let id = 5001 + *index as u64;
            library
                .albums
                .get_mut(&id)
                .expect("seed album")
                .photos
                .push(*photo);
        }
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

    /// `(command, params)` of every received request. An `engine.execute`
    /// request yields its inner command; a request to a method of the port
    /// itself (such as `ui.inspect`) yields the method name and its params.
    pub fn commands(&self) -> Vec<(String, Value)> {
        self.received()
            .iter()
            .map(|r| {
                let v: Value = serde_json::from_str(&r.line).expect("client sent invalid JSON");
                request_parts(&v)
            })
            .collect()
    }

    /// The methods of the received requests, in order (`engine.execute` or
    /// the name of a port method).
    pub fn methods(&self) -> Vec<String> {
        self.received()
            .iter()
            .map(|r| {
                let v: Value = serde_json::from_str(&r.line).expect("client sent invalid JSON");
                v["method"].as_str().unwrap_or("").to_string()
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

    /// Empties the selection, like an app that was restarted or whose
    /// user deselected everything.
    pub fn clear_selection(&self) {
        self.shared.library.lock().unwrap().selection.clear();
    }

    /// Keyword path texts in the fake library.
    pub fn keywords(&self) -> BTreeSet<String> {
        self.shared.library.lock().unwrap().keywords.clone()
    }

    /// Imported files and their ids.
    pub fn photos(&self) -> BTreeMap<String, i64> {
        self.shared.library.lock().unwrap().photos.clone()
    }

    /// Every album and album folder, in creation order.
    pub fn albums(&self) -> Vec<AlbumView> {
        self.shared
            .library
            .lock()
            .unwrap()
            .albums
            .values()
            .cloned()
            .collect()
    }

    /// The target album set by `album.setTarget` (`None`: Quick Collection).
    pub fn target_album(&self) -> Option<u64> {
        self.shared.library.lock().unwrap().target_album
    }

    /// Sets the target album like a user would, without a received command.
    pub fn set_target_album(&self, id: Option<u64>) {
        self.shared.library.lock().unwrap().target_album = id;
    }

    /// The Quick Collection (top-level album of this name), if any.
    pub fn quick_collection(&self) -> Option<AlbumView> {
        let lib = self.shared.library.lock().unwrap();
        quick_id(&lib).and_then(|id| lib.albums.get(&id).cloned())
    }

    /// Every stack, ascending by id; `photos[0]` of each is its top.
    pub fn stacks(&self) -> Vec<StackView> {
        let lib = self.shared.library.lock().unwrap();
        lib.stacks.values().cloned().collect()
    }

    /// The member photo ids of one album, in the order they were added.
    pub fn album_members(&self, album_id: u64) -> Vec<i64> {
        self.shared
            .library
            .lock()
            .unwrap()
            .albums
            .get(&album_id)
            .map(|a| a.photos.clone())
            .unwrap_or_default()
    }

    /// Rating of each photo, by photo id (only photos that were rated).
    pub fn ratings(&self) -> BTreeMap<i64, i64> {
        self.shared.library.lock().unwrap().ratings.clone()
    }

    /// The current field values of one photo, as the read side reports them.
    pub fn photo_fields(&self, id: i64) -> Option<SeedPhoto> {
        self.shared.library.lock().unwrap().seeds.get(&id).cloned()
    }

    /// The file path a photo is linked to.
    pub fn path_of(&self, id: i64) -> Option<String> {
        let lib = self.shared.library.lock().unwrap();
        lib.photos
            .iter()
            .find(|(_, v)| **v == id)
            .map(|(p, _)| p.clone())
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

/// Methods the port answers itself, outside `engine.execute`. The real app
/// does not know these as engine commands ("unknown command").
const PORT_METHODS: [&str; 1] = ["ui.inspect"];

/// `(command, params)` of a request: the inner command of `engine.execute`,
/// or the method name and params of a port method.
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
        let (command, params) = request_parts(&request);
        let (command, params) = (command.as_str(), &params);
        let is_engine = request["method"].as_str() == Some("engine.execute");
        let known = if is_engine {
            !PORT_METHODS.contains(&command)
        } else {
            PORT_METHODS.contains(&command)
        };

        let mut lib = shared.library.lock().unwrap();
        lib.seen += 1;
        if shared.config.idle_close_at.contains(&lib.seen) {
            drop(lib);
            let mut notice = json!({
                "ok": false,
                "error": "control connection idle read timed out; closing the connection",
                "closed": "idle",
            });
            if shared.config.idle_notice_without_marker {
                notice.as_object_mut().map(|o| o.remove("closed"));
            }
            let _ = writeln!(writer, "{notice}");
            let _ = writer.flush();
            let _ = writer.shutdown(Shutdown::Both);
            return;
        }
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
        let result = if known {
            run(shared, &mut lib, command, params)
        } else if is_engine {
            Err(format!("unknown command: {command}"))
        } else {
            Err(format!("unknown method: {command}"))
        };
        let out = match result {
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
        "album.create" | "album.createSmart" | "album.addPhotos" | "album.removePhotos"
        | "albums.list" => album_command(config, lib, command, params),
        "album.setTarget" => {
            let target = match params.get("id") {
                None | Some(Value::Null) => None,
                Some(v) => {
                    let id = v.as_u64().ok_or("id must be a number or null")?;
                    if !lib.albums.get(&id).is_some_and(|a| !a.folder) {
                        return Err("unknown album id".into());
                    }
                    Some(id)
                }
            };
            lib.target_album = target;
            Ok(json!({}))
        }
        "album.toggleTarget" => toggle_target(config, lib, params),
        "catalog.query" => catalog_query(config, lib, params),
        "photo.inspect" => {
            let id = params["ids"][0]
                .as_i64()
                .or_else(|| params["id"].as_i64())
                .ok_or("id is missing")?;
            if !lib.photos.values().any(|p| *p == id) {
                return Err("unknown photo id".into());
            }
            let seed = lib
                .seeds
                .get(&id)
                .cloned()
                .unwrap_or_else(|| SeedPhoto::plain(id));
            let albums: Vec<u64> = lib
                .albums
                .values()
                .filter(|a| a.photos.contains(&id))
                .map(|a| a.id)
                .collect();
            let stack = lib
                .stacks
                .values()
                .find(|st| st.photos.contains(&id))
                .map(|st| json!({"id": st.id, "photos": st.photos, "collapsed": st.collapsed}));
            Ok(json!({
                "id": id,
                "albums": albums,
                "stack": stack,
                "meta": {
                    "caption": seed.caption,
                    "copyright": seed.copyright,
                    "creator": seed.creator,
                    "keywords": seed.keywords,
                },
            }))
        }
        "stack.group" => stack_group(config, lib, params),
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
            if config.not_saved_keyword_creates {
                return Err("saved in memory but not written to disk: disk full".into());
            }
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
                    "duplicates": [{"path": path, "existing": existing, "reason": "path"}],
                    "failed": 0,
                }));
            }
            if let Some(content) = config.file_contents.get(path) {
                let twin = lib
                    .photos
                    .iter()
                    .filter(|(other, _)| config.file_contents.get(*other) == Some(content))
                    .map(|(_, id)| *id)
                    .min();
                if let Some(existing) = twin {
                    return Ok(json!({
                        "imported": [],
                        "duplicates": [{"path": path, "existing": existing, "reason": "content"}],
                        "failed": 0,
                    }));
                }
            }
            let id = lib.next_id;
            lib.next_id += 1;
            lib.photos.insert(path.to_string(), id);
            // An imported photo starts with LightCraft's import defaults and
            // is visible to the read side from now on.
            lib.seeds.entry(id).or_insert_with(|| SeedPhoto::plain(id));
            if config.selection_gate {
                lib.selection = vec![id];
            }
            if config.not_saved_creates {
                return Err("saved in memory but not written to disk: disk full".into());
            }
            if config.import_no_id {
                return Ok(json!({"imported": [], "duplicates": [], "failed": 0}));
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
            if config.not_saved_updates {
                // The app lost the change on restart: the next run must not
                // see it through the read side either.
                return Err("saved in memory but not written to disk: disk full".into());
            }
            // Keep the read side (`catalog.query`, `photo.inspect`) in sync
            // with what was applied, so a later run sees the live values.
            let seed = lib.seeds.entry(id).or_insert_with(|| SeedPhoto::plain(id));
            match command {
                "photo.rate" => {
                    let rating = params["rating"].as_i64().unwrap_or(-1);
                    lib.ratings.insert(id, rating);
                    seed.rating = rating;
                }
                "photo.flag" => {
                    if let Some(flag) = params["flag"].as_str() {
                        seed.flag = flag.to_string();
                    }
                }
                "photo.label" => {
                    seed.label = params["label"]
                        .as_str()
                        .filter(|label| *label != "none")
                        .map(str::to_string);
                }
                _ => {
                    if let Some(title) = params["title"].as_str() {
                        seed.title = title.to_string();
                    }
                    for (key, field) in [
                        ("caption", &mut seed.caption),
                        ("copyright", &mut seed.copyright),
                        ("creator", &mut seed.creator),
                    ] {
                        if let Some(text) = params[key].as_str() {
                            *field = (!text.is_empty()).then(|| text.to_string());
                        }
                    }
                    if let Some(all) = params["keywords"].as_array() {
                        seed.keywords = all
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect();
                    }
                    if let Some(removed) = params["removeKeywords"].as_array() {
                        seed.keywords
                            .retain(|have| !removed.iter().any(|r| r.as_str() == Some(have)));
                    }
                    if let Some(added) = params["addKeywords"].as_array() {
                        for keyword in added.iter().filter_map(Value::as_str) {
                            if !seed.keywords.iter().any(|have| have == keyword) {
                                seed.keywords.push(keyword.to_string());
                            }
                        }
                    }
                }
            }
            Ok(json!({}))
        }
        "develop.set" => {
            // Slider values name their photos directly and need no selection.
            let values = params
                .get("values")
                .and_then(Value::as_object)
                .ok_or("values is missing")?;
            if values.values().any(|v| !v.is_number()) {
                return Err("values must be numbers".into());
            }
            let ids = params
                .get("ids")
                .and_then(Value::as_array)
                .ok_or("ids is missing")?;
            let ids: Vec<i64> = ids.iter().filter_map(Value::as_i64).collect();
            if ids.len() != params["ids"].as_array().map_or(0, Vec::len) {
                return Err("ids must be numbers".into());
            }
            if let Some(unknown) = ids.iter().find(|i| !lib.photos.values().any(|p| p == *i)) {
                return Err(format!("unknown photo id {unknown}"));
            }
            Ok(json!({}))
        }
        "develop.merge" => {
            // Structured settings act on the selection, like `photo.*`, and
            // carry no `ids`.
            if config.selection_gate && lib.selection.is_empty() {
                return Err("no photos selected".into());
            }
            if params.get("ids").is_some() {
                return Err("develop.merge takes no ids".into());
            }
            if params.get("settings").and_then(Value::as_object).is_none() {
                return Err("settings is missing".into());
            }
            Ok(json!({}))
        }
        "photo.virtualCopy" => {
            // `photo.virtualCopy {ids: [master id]}` -> `{ids: [new id]}`.
            // Every call makes a new copy (not idempotent), like the real app.
            // The copy shares the master's file but gets its own id, so it is
            // known to `library.select`, `photo.rate` and friends.
            let ids = params
                .get("ids")
                .and_then(Value::as_array)
                .ok_or("ids is missing")?;
            let master_ids: Vec<i64> = ids.iter().filter_map(Value::as_i64).collect();
            if master_ids.len() != ids.len() {
                return Err("ids must be numbers".into());
            }
            let mut new_ids: Vec<Value> = Vec::new();
            for master_id in master_ids {
                let master_path = lib
                    .photos
                    .iter()
                    .find(|(_, v)| **v == master_id)
                    .map(|(p, _)| p.clone())
                    .ok_or("unknown photo id")?;
                let id = lib.next_id;
                lib.next_id += 1;
                // Distinct path key so the master keeps its own entry.
                lib.photos.insert(format!("{master_path}#vc{id}"), id);
                // A virtual copy starts with the master's fields.
                let seed = lib
                    .seeds
                    .get(&master_id)
                    .cloned()
                    .unwrap_or_else(|| SeedPhoto::plain(id));
                lib.seeds.insert(id, SeedPhoto { id, ..seed });
                // Like the real app: the copy joins the master's regular
                // albums and is stacked right after the master.
                for album in lib.albums.values_mut() {
                    if album.rules.is_none()
                        && !album.folder
                        && let Some(at) = album.photos.iter().position(|p| *p == master_id)
                    {
                        album.photos.insert(at + 1, id);
                    }
                }
                match lib
                    .stacks
                    .values_mut()
                    .find(|st| st.photos.contains(&master_id))
                {
                    Some(st) => {
                        let at = st.photos.iter().position(|p| *p == master_id).unwrap_or(0);
                        st.photos.insert(at + 1, id);
                        st.collapsed = false;
                    }
                    None => {
                        let sid = lib.next_stack_id;
                        lib.next_stack_id += 1;
                        lib.stacks.insert(
                            sid,
                            StackView {
                                id: sid,
                                photos: vec![master_id, id],
                                collapsed: false,
                            },
                        );
                    }
                }
                if config.selection_gate {
                    lib.selection = vec![id];
                }
                new_ids.push(json!(id));
            }
            if config.not_saved_creates {
                return Err("saved in memory but not written to disk: disk full".into());
            }
            Ok(json!({"ids": new_ids}))
        }
        "photo.relink" => {
            // `{id, path}`: points one photo at another file.
            let id = params["id"].as_i64().ok_or("id is missing")?;
            let path = params["path"].as_str().ok_or("missing `path`")?;
            let old = lib
                .photos
                .iter()
                .find(|(_, v)| **v == id)
                .map(|(p, _)| p.clone())
                .ok_or("unknown photo id")?;
            if lib.photos.get(path).is_some_and(|other| *other != id) {
                return Err("another photo already uses that file".into());
            }
            lib.photos.remove(&old);
            lib.photos.insert(path.to_string(), id);
            Ok(json!({"id": id, "path": path}))
        }
        "stack.remove" => {
            let ids: Vec<i64> = params["ids"]
                .as_array()
                .ok_or("ids is missing")?
                .iter()
                .filter_map(Value::as_i64)
                .collect();
            let mut changed = 0;
            for st in lib.stacks.values_mut() {
                let before = st.photos.len();
                st.photos.retain(|p| !ids.contains(p));
                changed += before - st.photos.len();
            }
            if changed == 0 {
                return Err("the photos are not in a stack".into());
            }
            lib.stacks.retain(|_, st| st.photos.len() >= 2);
            Ok(json!({"changed": changed}))
        }
        "photo.delete" => {
            // Moves photos to Recently Deleted; the fake simply forgets them.
            let ids: Vec<i64> = params["ids"]
                .as_array()
                .ok_or("ids is missing")?
                .iter()
                .filter_map(Value::as_i64)
                .collect();
            lib.photos.retain(|_, id| !ids.contains(id));
            for id in &ids {
                lib.seeds.remove(id);
            }
            for album in lib.albums.values_mut() {
                album.photos.retain(|p| !ids.contains(p));
            }
            for st in lib.stacks.values_mut() {
                st.photos.retain(|p| !ids.contains(p));
            }
            lib.stacks.retain(|_, st| st.photos.len() >= 2);
            Ok(json!({}))
        }
        "develop.reset" => {
            let ids = params["ids"].as_array().ok_or("ids is missing")?;
            if ids
                .iter()
                .filter_map(Value::as_i64)
                .any(|i| !lib.photos.values().any(|p| *p == i))
            {
                return Err("unknown photo id".into());
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

/// The album commands, with the rules of the real app: `album.create` never
/// takes `addSelected` here (the writer must not send it), names are
/// trimmed, an empty name and a parent that is not an existing folder are
/// errors, the same name may exist twice; `album.addPhotos` needs a
/// non-empty selection, refuses folders, appends and is idempotent;
/// `album.removePhotos` needs a non-empty selection too.
fn album_command(
    config: &Config,
    lib: &mut Library,
    command: &str,
    params: &Value,
) -> Result<Value, String> {
    let album_name = match command {
        "album.create" | "album.createSmart" => params["name"].as_str().map(str::to_string),
        _ => params["id"]
            .as_u64()
            .and_then(|id| lib.albums.get(&id))
            .map(|a| a.name.clone()),
    };
    for f in &config.album_failures {
        if f.command == command
            && (f.album_name.is_none() || f.album_name.as_deref() == album_name.as_deref())
        {
            return Err(f.message.clone());
        }
    }
    match command {
        "album.create" => {
            if params.get("addSelected").is_some() {
                return Err("addSelected must not be sent".into());
            }
            let name = params["name"].as_str().ok_or("name is missing")?.trim();
            if name.is_empty() {
                return Err("empty name".into());
            }
            let parent = match params.get("parent") {
                None | Some(Value::Null) => None,
                Some(v) => {
                    let id = v.as_u64().ok_or("parent must be a number")?;
                    if !lib.albums.get(&id).is_some_and(|a| a.folder) {
                        return Err("parent must be an existing folder".into());
                    }
                    Some(id)
                }
            };
            let folder = match params.get("folder") {
                None | Some(Value::Null) => false,
                Some(v) => v.as_bool().ok_or("folder must be true or false")?,
            };
            let id = lib.next_album_id;
            lib.next_album_id += 1;
            lib.albums.insert(
                id,
                AlbumView {
                    id,
                    name: name.to_string(),
                    folder,
                    parent,
                    photos: Vec::new(),
                    rules: None,
                    quick: false,
                },
            );
            if config.not_saved_creates {
                return Err("saved in memory but not written to disk: disk full".into());
            }
            if config.albums_no_id {
                return Ok(json!({"skipped": 0}));
            }
            Ok(json!({"id": id, "skipped": 0}))
        }
        "album.createSmart" => {
            // Like the real app, but `rules` is mandatory here: without it
            // the real command would save the current view.
            let name = params["name"].as_str().ok_or("name is missing")?.trim();
            if name.is_empty() {
                return Err("empty name".into());
            }
            let rules = params.get("rules").ok_or("rules is missing")?;
            if !rules.is_object() {
                return Err("rules must be an object".into());
            }
            if let Some(set) = rules.get("ruleSet") {
                let mode = set["match"].as_str().unwrap_or("");
                if !["all", "any", "none"].contains(&mode) || !set["rules"].is_array() {
                    return Err("rule 1: bad rule set".into());
                }
            }
            let parent = match params.get("parent") {
                None | Some(Value::Null) => None,
                Some(v) => {
                    let id = v.as_u64().ok_or("parent must be a number")?;
                    if !lib.albums.get(&id).is_some_and(|a| a.folder) {
                        return Err("parent must be an existing folder".into());
                    }
                    Some(id)
                }
            };
            let id = lib.next_album_id;
            lib.next_album_id += 1;
            lib.albums.insert(
                id,
                AlbumView {
                    id,
                    name: name.to_string(),
                    folder: false,
                    parent,
                    photos: Vec::new(),
                    rules: Some(rules.clone()),
                    quick: false,
                },
            );
            if config.not_saved_creates {
                return Err("saved in memory but not written to disk: disk full".into());
            }
            if config.albums_no_id {
                return Ok(json!({"count": 0}));
            }
            Ok(json!({"id": id, "count": 0}))
        }
        "album.addPhotos" | "album.removePhotos" => {
            if params.get("addSelected").is_some() {
                return Err("addSelected must not be sent".into());
            }
            if config.selection_gate && lib.selection.is_empty() {
                return Err("no photos selected".into());
            }
            let id = params["id"].as_u64().ok_or("id must be a number")?;
            let ids = params["ids"].as_array().ok_or("ids is missing")?;
            let ids: Vec<i64> = ids.iter().filter_map(Value::as_i64).collect();
            if ids.len() != params["ids"].as_array().map_or(0, Vec::len) {
                return Err("ids must be numbers".into());
            }
            if let Some(unknown) = ids.iter().find(|i| !lib.photos.values().any(|p| p == *i)) {
                return Err(format!("unknown photo id {unknown}"));
            }
            let album = lib.albums.get_mut(&id).ok_or("unknown album id")?;
            if album.folder || album.rules.is_some() {
                return Err("smart albums and folders can't hold photos".into());
            }
            if command == "album.addPhotos" {
                let mut added = 0;
                for photo in ids {
                    if !album.photos.contains(&photo) {
                        album.photos.push(photo);
                        added += 1;
                    }
                }
                Ok(json!({"added": added, "skipped": 0}))
            } else {
                let before = album.photos.len();
                album.photos.retain(|p| !ids.contains(p));
                Ok(json!({"removed": before - album.photos.len(), "album": id}))
            }
        }
        _ if config.albums_list_garbage => Ok(json!({"albums": "none"})),
        _ => Ok(json!(albums_nested(lib, None))),
    }
}

/// `albums.list`: the nested tree `{id, name, folder, count, children}`.
fn albums_nested(lib: &Library, parent: Option<u64>) -> Vec<Value> {
    lib.albums
        .values()
        .filter(|a| a.parent == parent)
        .map(|a| {
            let mut node = json!({
                "id": a.id,
                "name": a.name,
                "folder": a.folder,
                "count": a.photos.len(),
                "cover": null,
                "children": albums_nested(lib, Some(a.id)),
            });
            if let Some(rules) = &a.rules {
                node["smart"] = json!(true);
                node["rules"] = rules.clone();
            }
            node
        })
        .collect()
}

/// `stack.group` with the rules of the real app: gated by a selection
/// (`no photos selected`), `ids` default to the selection, the new order is
/// `top` first, then `ids` without repeats, at least two photos, stacks that
/// any of them are in are merged (the first touched keeps its id, the others
/// disappear, their other members are appended), `collapsed` defaults to
/// true, equal arguments give the same state, the selection becomes the top
/// photo.
fn stack_group(config: &Config, lib: &mut Library, params: &Value) -> Result<Value, String> {
    if config.selection_gate && lib.selection.is_empty() {
        return Err("no photos selected".into());
    }
    let ids: Vec<i64> = match params.get("ids") {
        Some(v) => {
            let list = v.as_array().ok_or("ids must be an array")?;
            let ids: Vec<i64> = list.iter().filter_map(Value::as_i64).collect();
            if ids.len() != list.len() {
                return Err("ids must be numbers".into());
            }
            ids
        }
        None => lib.selection.clone(),
    };
    let known = |i: &i64| lib.photos.values().any(|p| p == i);
    let top = match params.get("top") {
        Some(v) => Some(v.as_i64().ok_or("top must be a number")?),
        None => None,
    };
    let top = top.or_else(|| ids.first().copied());
    let mut order: Vec<i64> = Vec::new();
    for id in top.into_iter().chain(ids.iter().copied()) {
        if known(&id) && !order.contains(&id) {
            order.push(id);
        }
    }
    if order.len() < 2 {
        return Err("select at least two photos".into());
    }
    let touched: Vec<u64> = lib
        .stacks
        .values()
        .filter(|s| s.photos.iter().any(|p| order.contains(p)))
        .map(|s| s.id)
        .collect();
    let mut photos = order.clone();
    for id in &touched {
        for p in &lib.stacks[id].photos {
            if !photos.contains(p) {
                photos.push(*p);
            }
        }
    }
    let id = match touched.first() {
        Some(id) => *id,
        None => {
            let id = lib.next_stack_id;
            lib.next_stack_id += 1;
            id
        }
    };
    for other in touched.iter().skip(1) {
        lib.stacks.remove(other);
    }
    let count = photos.len();
    let collapsed = params["collapsed"].as_bool().unwrap_or(true);
    lib.stacks.insert(
        id,
        StackView {
            id,
            photos,
            collapsed,
        },
    );
    lib.selection = vec![order[0]];
    Ok(json!({"stack": id, "count": count}))
}

/// The id of the Quick Collection: the first album marked `quick`.
fn quick_id(lib: &Library) -> Option<u64> {
    lib.albums.values().find(|a| a.quick).map(|a| a.id)
}

/// `album.toggleTarget {ids?}` with the real rules (research A4): gated by a
/// selection, `ids` default to the selection, the target is the album set by
/// `album.setTarget` or else the Quick Collection (created when missing);
/// when all photos are in it they are removed, otherwise the missing ones are
/// appended.
fn toggle_target(config: &Config, lib: &mut Library, params: &Value) -> Result<Value, String> {
    if config.selection_gate && lib.selection.is_empty() {
        return Err("no photos selected".into());
    }
    let ids: Vec<i64> = match params.get("ids") {
        Some(v) => {
            let list = v.as_array().ok_or("ids must be an array")?;
            let ids: Vec<i64> = list.iter().filter_map(Value::as_i64).collect();
            if ids.len() != list.len() {
                return Err("ids must be numbers".into());
            }
            ids
        }
        None => lib.selection.clone(),
    };
    if let Some(unknown) = ids.iter().find(|i| !lib.photos.values().any(|p| p == *i)) {
        return Err(format!("unknown photo id {unknown}"));
    }
    let album_id = match lib.target_album {
        Some(id) => id,
        None => match quick_id(lib) {
            Some(id) => id,
            None => {
                let id = lib.next_album_id;
                lib.next_album_id += 1;
                lib.albums.insert(
                    id,
                    AlbumView {
                        id,
                        name: "Quick Collection".into(),
                        folder: false,
                        parent: None,
                        photos: Vec::new(),
                        rules: None,
                        quick: true,
                    },
                );
                id
            }
        },
    };
    let album = lib.albums.get_mut(&album_id).ok_or("unknown album id")?;
    let all_in = !ids.is_empty() && ids.iter().all(|i| album.photos.contains(i));
    let mut added = 0;
    if all_in {
        album.photos.retain(|p| !ids.contains(p));
    } else {
        for id in ids {
            if !album.photos.contains(&id) {
                album.photos.push(id);
                added += 1;
            }
        }
    }
    Ok(json!({
        "album": album.id,
        "name": album.name,
        "added": added,
        "count": album.photos.len(),
        "skipped": 0,
    }))
}

/// `catalog.query {filter?, offset?, limit?}` -> `{total, photos:[{id,
/// fileName}]}`; `filter.album` lists the members of that album, sorted by
/// photo id (not by insertion order, like the real app); without it all
/// photos. `filter.deleted: true` lists only photos in the trash, a missing
/// or false `deleted` only photos outside it. The default `limit` is 200.
fn catalog_query(config: &Config, lib: &Library, params: &Value) -> Result<Value, String> {
    let deleted = params["filter"]["deleted"].as_bool().unwrap_or(false);
    let mut ids: Vec<i64> = match params["filter"].get("album") {
        Some(v) => {
            let id = v.as_u64().ok_or("album must be a number")?;
            let album = lib.albums.get(&id).ok_or("unknown album id")?;
            album.photos.clone()
        }
        None => lib.photos.values().copied().collect(),
    };
    // A photo without a seed is a plain, non-deleted photo.
    ids.retain(|id| {
        config.ignore_deleted_filter || lib.seeds.get(id).is_none_or(|seed| seed.deleted == deleted)
    });
    ids.sort_unstable();
    let offset = params["offset"].as_u64().unwrap_or(0) as usize;
    let limit = params["limit"].as_u64().unwrap_or(200) as usize;
    let photos: Vec<Value> = ids
        .iter()
        .skip(offset)
        .take(limit)
        .map(|id| {
            let mut v = json!({"id": id, "fileName": format!("{id}.jpg")});
            let seed = lib
                .seeds
                .get(id)
                .cloned()
                .unwrap_or_else(|| SeedPhoto::plain(*id));
            v["rating"] = json!(seed.rating);
            v["flag"] = json!(seed.flag);
            v["label"] = json!(seed.label);
            v["title"] = json!(seed.title);
            v["keywords"] = json!(seed.keywords);
            v["deleted"] = json!(seed.deleted);
            if config.summary_broken {
                v["rating"] = json!("many");
            }
            v
        })
        .collect();
    Ok(json!({"total": ids.len(), "photos": photos}))
}
