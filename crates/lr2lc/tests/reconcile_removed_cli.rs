//! Acceptance tests for T11 (reconciliation, part c): items that were removed
//! or renamed in Lightroom are reported, never deleted or renamed in
//! LightCraft.
//!
//! These tests fix the contract; the implementation follows.
//!
//! `report.json`, section `writer` (an `--apply` run), gains six keys,
//! always present, next to the existing counters:
//! - `photos_removed` (integer) and `removed_photos`
//!   (`[{"photo": <Lightroom id>}]`): recorded photos that are no longer in
//!   the Lightroom model. `len(removed_photos) == photos_removed`, ascending
//!   by id. Nothing is deleted in LightCraft; a photo is taken only out of
//!   albums recorded in `state.collections` (`album.removePhotos`).
//! - `photos_moved` (integer) and `moved_photos`
//!   (`[{"photo": <Lightroom id>}]`): recorded photos whose file path
//!   changed while their global id stayed. Nothing is imported again.
//! - `keywords_renamed` (integer) and `renamed_keywords`
//!   (`[{"keyword": <Lightroom id>}]`): recorded keywords whose `id_global`
//!   is the same but whose name changed. No `keyword.create` and no
//!   `keyword.rename` is sent, and the new name is not added to any photo.
//!
//! The mapping file stores the original's absolute path per photo as `path`.
//! It is the base of the move comparison and is not changed by a move, so a
//! move (like a removal and a rename) is reported again on every later run
//! with the same numbers. A second run after the reporting run sends no
//! write command and leaves the mapping file byte-identical.
//!
//! Never: `photo.delete`, `album.delete`, `keyword.rename`, `removeKeywords`;
//! never a second `library.import` of a moved photo.

use lc_writer::control::{ControlClient, Outcome};
use lrcat_fixtures::{
    move_photo_file, photo_file_path, remove_photo, rename_keyword, write_small_catalog_with_files,
};
use serde_json::{Value, json};
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use fake_lightcraft::{Config, FakeLightCraft, READ_COMMANDS};
use lc_test_support::fake_lightcraft;

struct Setup {
    photos: tempfile::TempDir,
    work: tempfile::TempDir,
    catalog: PathBuf,
    library: PathBuf,
}

impl Setup {
    fn new() -> Setup {
        let photos = tempfile::tempdir().unwrap();
        let catalog = write_small_catalog_with_files(photos.path()).unwrap();
        let work = tempfile::tempdir().unwrap();
        let library = work.path().join("Test.lightcraft");
        Setup {
            photos,
            work,
            catalog,
            library,
        }
    }

    fn state_file(&self) -> PathBuf {
        let mut name = self.library.file_name().unwrap().to_os_string();
        name.push(".lr2lc.json");
        self.work.path().join(name)
    }

    fn state_json(&self) -> Value {
        serde_json::from_str(&fs::read_to_string(self.state_file()).unwrap()).unwrap()
    }

    /// `lr2lc <catalog> --library <lib> --port <port> --report <file>
    /// [--apply]`.
    fn run(&self, port: u16, apply: bool) -> Output {
        let port_text = port.to_string();
        let report = self.work.path().join("report.json");
        let mut args: Vec<&OsStr> = vec![
            self.catalog.as_os_str(),
            OsStr::new("--library"),
            self.library.as_os_str(),
            OsStr::new("--port"),
            OsStr::new(&port_text),
            OsStr::new("--report"),
            report.as_os_str(),
        ];
        if apply {
            args.push(OsStr::new("--apply"));
        }
        Command::new(env!("CARGO_BIN_EXE_lr2lc"))
            .current_dir(self.work.path())
            .args(args)
            .output()
            .unwrap()
    }

    /// A run that must succeed and leave the catalog byte-identical.
    fn ok(&self, port: u16, apply: bool) -> Run {
        let before = fs::read(&self.catalog).unwrap();
        let o = self.run(port, apply);
        let stderr = String::from_utf8_lossy(&o.stderr).into_owned();
        assert_eq!(o.status.code(), Some(0), "{stderr}");
        assert_eq!(before, fs::read(&self.catalog).unwrap(), "catalog changed");
        let raw = fs::read_to_string(self.work.path().join("report.json")).unwrap();
        Run {
            report: serde_json::from_str(&raw).unwrap(),
            raw,
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        }
    }
}

struct Run {
    report: Value,
    raw: String,
    stdout: String,
}

/// Commands sent from `from` on must all be read-only.
fn assert_only_reads(fake: &FakeLightCraft, from: usize) {
    let sent: Vec<String> = fake.commands()[from..]
        .iter()
        .map(|(c, _)| c.clone())
        .collect();
    assert!(
        sent.iter().all(|c| READ_COMMANDS.contains(&c.as_str())),
        "a later run sent write commands: {sent:?}"
    );
}

/// The write commands sent from `from` on, with their parameters.
fn writes_since(fake: &FakeLightCraft, from: usize) -> Vec<(String, Value)> {
    fake.commands()[from..]
        .iter()
        .filter(|(c, _)| !READ_COMMANDS.contains(&c.as_str()))
        .cloned()
        .collect()
}

/// A LightCraft id recorded in the mapping file.
fn lightcraft_id(state: &Value, area: &str, key: &str) -> i64 {
    state[area][key]["lightcraft_id"]
        .as_i64()
        .unwrap_or_else(|| panic!("no {area}.{key}.lightcraft_id in the mapping file"))
}

// --- 1) a photo removed in Lightroom ---

#[test]
fn removed_photo_is_reported_and_only_taken_out_of_own_albums() {
    let s = Setup::new();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    s.ok(fake.port(), true);

    // Photo 4 (`image-00000004`) was imported and is a member of the album
    // lr2lc created for Lightroom collection 3.
    let state = s.state_json();
    let gid = "image-00000004";
    assert_eq!(state["photos"][gid]["lightroom_id"], 4);
    let photo_id = lightcraft_id(&state, "photos", gid);
    let own_album = lightcraft_id(&state, "collections", "3");
    let recorded: Vec<&str> = state["collections"]["3"]["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m.as_str().unwrap())
        .collect();
    assert!(recorded.contains(&gid), "{recorded:?}");

    // An album the user created in LightCraft, unknown to the mapping file;
    // it holds the same photo.
    {
        let mut client = ControlClient::connect(fake.port()).unwrap();
        client
            .execute(
                "library.select",
                json!({"ids": [photo_id], "active": photo_id}),
            )
            .unwrap();
        let created = match client
            .execute(
                "album.create",
                json!({"name": "Foreign Album", "folder": false}),
            )
            .unwrap()
        {
            Outcome::Applied(value) => value,
            other => panic!("unexpected album.create reply: {other:?}"),
        };
        let foreign = created["id"].as_u64().unwrap();
        match client
            .execute("album.addPhotos", json!({"id": foreign, "ids": [photo_id]}))
            .unwrap()
        {
            Outcome::Applied(_) => {}
            other => panic!("unexpected album.addPhotos reply: {other:?}"),
        }
    }

    // The user deletes photo 4 in Lightroom.
    remove_photo(&s.catalog, 4).unwrap();
    let after_first = fake.commands().len();
    let second = s.ok(fake.port(), true);

    let writer = &second.report["writer"];
    assert_eq!(writer["photos_removed"], 1, "{writer}");
    assert_eq!(writer["removed_photos"], json!([{"photo": 4}]));

    // Nothing is deleted, and the photo leaves only the album lr2lc wrote.
    let writes = writes_since(&fake, after_first);
    for (command, _) in &writes {
        assert!(
            !matches!(command.as_str(), "photo.delete" | "album.delete"),
            "removed photo triggered {command}"
        );
    }
    let removals: Vec<&(String, Value)> = writes
        .iter()
        .filter(|(c, _)| c == "album.removePhotos")
        .collect();
    assert_eq!(removals.len(), 1, "{writes:?}");
    assert_eq!(removals[0].1["id"], own_album);
    let albums = fake.albums();
    let foreign = albums.iter().find(|a| a.name == "Foreign Album").unwrap();
    assert!(
        foreign.photos.contains(&photo_id),
        "the user's own album was changed: {foreign:?}"
    );
    let own = albums.iter().find(|a| a.id as i64 == own_album).unwrap();
    assert!(!own.photos.contains(&photo_id), "{own:?}");

    // No path, no global id, no file name in the report or on stdout.
    for needle in [gid, "IMG_0004", "/Users/example"] {
        assert!(!second.raw.contains(needle), "report leaks {needle:?}");
        assert!(!second.stdout.contains(needle), "stdout leaks {needle:?}");
    }

    // The next run reports the same and sends only reads.
    let state_after_second = fs::read(s.state_file()).unwrap();
    let commands_after_second = fake.commands().len();
    let third = s.ok(fake.port(), true);
    assert_eq!(third.report["writer"]["photos_removed"], 1);
    assert_eq!(
        third.report["writer"]["removed_photos"],
        json!([{"photo": 4}])
    );
    assert_only_reads(&fake, commands_after_second);
    assert_eq!(
        fs::read(s.state_file()).unwrap(),
        state_after_second,
        "the mapping file changed without a write"
    );
}

// --- 2) a keyword renamed in Lightroom ---

#[test]
fn renamed_keyword_is_reported_and_neither_created_nor_renamed() {
    let s = Setup::new();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    s.ok(fake.port(), true);

    let keywords_before = fake.keywords();
    assert!(keywords_before.contains("Family"), "{keywords_before:?}");

    // Keyword 13 (`kw-00000013`) is a leaf: only its own name changes.
    rename_keyword(&s.catalog, 13, "Kin").unwrap();
    let after_first = fake.commands().len();
    let second = s.ok(fake.port(), true);

    let writer = &second.report["writer"];
    assert_eq!(writer["keywords_renamed"], 1, "{writer}");
    assert_eq!(writer["renamed_keywords"], json!([{"keyword": 13}]));

    // No second keyword is created and nothing is renamed in LightCraft.
    assert_only_reads(&fake, after_first);
    assert_eq!(fake.keywords(), keywords_before, "keyword set changed");

    // Neither the old nor the new name may appear in the report or on stdout.
    for needle in ["Family", "Kin", "kw-00000013"] {
        assert!(!second.raw.contains(needle), "report leaks {needle:?}");
        assert!(!second.stdout.contains(needle), "stdout leaks {needle:?}");
    }

    // The next run reports the same and changes nothing.
    let commands_after_second = fake.commands().len();
    let third = s.ok(fake.port(), true);
    assert_eq!(third.report["writer"]["keywords_renamed"], 1);
    assert_eq!(
        third.report["writer"]["renamed_keywords"],
        json!([{"keyword": 13}])
    );
    assert_only_reads(&fake, commands_after_second);
    assert_eq!(fake.keywords(), keywords_before);
}

// --- 3) a file moved in Lightroom ---

#[test]
fn moved_file_is_reported_and_not_imported_again() {
    let s = Setup::new();
    let fake = FakeLightCraft::start(Config::new(&s.library));
    s.ok(fake.port(), true);

    // The mapping file holds the path of the original.
    let old_path = photo_file_path(&s.catalog, 1).unwrap();
    let old_text = old_path.to_string_lossy().into_owned();
    let state = s.state_json();
    let entry = &state["photos"]["image-00000001"];
    assert_eq!(entry["lightroom_id"], 1);
    assert_eq!(entry["path"], old_text, "the mapping file stores the path");
    let photos_before = fake.photos();

    // The same photo (same global id) now lives in another folder; its file
    // is created there so the reader sees a present original.
    move_photo_file(&s.catalog, 1, 3).unwrap();
    let new_path = photo_file_path(&s.catalog, 1).unwrap();
    assert_ne!(new_path, old_path);
    fs::create_dir_all(new_path.parent().unwrap()).unwrap();
    fs::write(&new_path, b"placeholder\n").unwrap();

    let after_first = fake.commands().len();
    let second = s.ok(fake.port(), true);

    let writer = &second.report["writer"];
    assert_eq!(writer["photos_moved"], 1, "{writer}");
    assert_eq!(writer["moved_photos"], json!([{"photo": 1}]));

    // No second import, no duplicate, no write at all.
    assert_only_reads(&fake, after_first);
    assert_eq!(fake.photos(), photos_before, "a duplicate was imported");

    // The recorded path stays the old one, so the next run reports the move
    // again; it sends only reads.
    assert_eq!(
        s.state_json()["photos"]["image-00000001"]["path"],
        old_text,
        "the move must not silently update the base"
    );
    let commands_after_second = fake.commands().len();
    let third = s.ok(fake.port(), true);
    assert_eq!(third.report["writer"]["photos_moved"], 1);
    assert_eq!(
        third.report["writer"]["moved_photos"],
        json!([{"photo": 1}])
    );
    assert_only_reads(&fake, commands_after_second);

    // The old and the new path never reach the report or stdout.
    let new_text = new_path.to_string_lossy().into_owned();
    for needle in [old_text.as_str(), new_text.as_str(), "IMG_0001"] {
        assert!(!second.raw.contains(needle), "report leaks {needle:?}");
        assert!(!second.stdout.contains(needle), "stdout leaks {needle:?}");
    }
    // Keep the temp dirs alive until here.
    assert!(s.photos.path().exists());
}
