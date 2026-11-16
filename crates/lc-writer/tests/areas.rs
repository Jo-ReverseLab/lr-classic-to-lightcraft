//! Leaving parts of a catalog out (`WriteInput.areas`): a part that is not
//! selected sends no command and is not compared, its fields keep their last
//! imported value in the mapping file, and selecting it again later writes
//! every Lightroom value. Items recorded earlier are not reported as removed
//! while their part is left out.

use lc_test_support::fake_lightcraft::{Config, FakeLightCraft};
use lc_writer::apply::{ApplySummary, apply};
use lc_writer::control::ControlClient;
use lc_writer::plan::{Areas, WriteInput, plan};
use lc_writer::state::State;
use model::{Collection, CollectionKind, Develop, Flag, Keyword, Photo, Rating};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

fn keyword(name: &str) -> Keyword {
    Keyword::new(vec![name.to_string()]).unwrap()
}

/// Photo 1 with rating, flag, caption, a keyword and develop settings;
/// photo 2 a virtual copy of photo 1; one album holding photo 1.
fn input(areas: Areas) -> WriteInput {
    let mut develop = Develop::default();
    develop.controls.insert("light.exposure".to_string(), 0.5);
    develop.source_hash = Some([7; 32]);
    let mut master = Photo::new("/Users/example/Pictures/1.jpg", 1, Rating::new(4).unwrap())
        .with_global_id("g1")
        .with_flag(Flag::Pick)
        .with_caption("Caption");
    master.develop = Some(develop);
    let copy = Photo::new("/Users/example/Pictures/1.jpg", 2, Rating::default())
        .with_global_id("g2")
        .with_master(1);
    let mut photo_keywords = BTreeMap::new();
    photo_keywords.insert(1, vec![keyword("Harbour")]);
    WriteInput {
        photos: vec![master, copy],
        keywords: vec![keyword("Harbour")],
        photo_keywords,
        collections: vec![Collection {
            lightroom_id: 50,
            name: "Album".into(),
            kind: CollectionKind::Album,
            parent: None,
            members: vec![1],
        }],
        areas,
        ..WriteInput::default()
    }
}

struct Env {
    _dir: tempfile::TempDir,
    fake: FakeLightCraft,
    state_file: PathBuf,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("Test.lightcraft");
    Env {
        state_file: lc_writer::state::state_path(&library),
        fake: FakeLightCraft::start(Config::new(&library)),
        _dir: dir,
    }
}

impl Env {
    /// Applies `input` and returns the summary and the commands it sent.
    fn run(&self, input: &WriteInput) -> (ApplySummary, BTreeSet<String>) {
        let before = self.fake.commands().len();
        let mut state = State::load(&self.state_file).unwrap();
        let mut client = ControlClient::connect(self.fake.port()).unwrap();
        let summary = apply(&mut client, input, &mut state, &self.state_file).unwrap();
        let sent = self.fake.commands()[before..]
            .iter()
            .map(|(c, _)| c.clone())
            .collect();
        (summary, sent)
    }
}

fn only(names: &[&str]) -> Areas {
    let mut areas = Areas::ALL;
    for name in Areas::NAMES {
        areas.set(name, names.contains(&name));
    }
    areas
}

#[test]
fn nothing_but_photos_imports_the_files_alone() {
    let env = env();
    let (summary, sent) = env.run(&input(only(&[])));
    assert_eq!(summary.photos_imported, 1);
    for command in [
        "photo.rate",
        "photo.flag",
        "photo.setMeta",
        "keyword.create",
        "album.create",
        "photo.virtualCopy",
        "develop.set",
    ] {
        assert!(!sent.contains(command), "{command} sent: {sent:?}");
    }
    let state = State::load(&env.state_file).unwrap();
    let fields = &state.photo("g1").unwrap().fields;
    assert_eq!(fields.rating, 0);
    assert!(fields.keywords.is_empty());
    assert_eq!(fields.caption, None);
}

#[test]
fn selecting_an_area_later_writes_its_values() {
    let env = env();
    env.run(&input(only(&[])));
    let (summary, sent) = env.run(&input(Areas::ALL));
    // Only the virtual copy is new; the file is not imported again.
    assert_eq!(summary.photos_imported, 1);
    assert!(!sent.contains("library.import"));
    for command in [
        "photo.rate",
        "photo.flag",
        "photo.setMeta",
        "keyword.create",
        "album.create",
        "photo.virtualCopy",
        "develop.set",
    ] {
        assert!(sent.contains(command), "{command} missing: {sent:?}");
    }
    // The master gets its stars; the new copy is reset to none.
    assert_eq!(
        env.fake.ratings().values().copied().collect::<Vec<_>>(),
        [4, 0]
    );
}

#[test]
fn leaving_an_area_out_later_neither_writes_nor_reports_it_as_removed() {
    let env = env();
    env.run(&input(Areas::ALL));
    let state = State::load(&env.state_file).unwrap();
    let p = plan(&input(only(&[])), &state, &BTreeSet::new());
    assert_eq!(p.collections_removed(), 0);
    assert!(p.removed_photos().is_empty());
    assert_eq!(p.keywords_removed(), 0);
    assert!(p.steps.is_empty(), "{:?}", p.steps);
}

#[test]
fn area_names_parse_and_list_in_a_fixed_order() {
    let areas = Areas::skipping("develop, keywords").unwrap();
    assert_eq!(areas.not_selected(), ["keywords", "develop"]);
    assert!(
        Areas::skipping("faces")
            .unwrap_err()
            .contains("unknown area `faces`")
    );
    assert!(Areas::ALL.not_selected().is_empty());
}
