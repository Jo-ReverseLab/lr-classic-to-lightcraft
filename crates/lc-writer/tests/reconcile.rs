//! Acceptance tests for the three-way reconciliation (F-022 (a)): the plan
//! compares, per known photo, the last imported value (mapping file), the
//! Lightroom value (model) and the value LightCraft has now (live state).
//!
//! API under test (fixed by these tests):
//! - `WriteInput` gains `live: Option<LiveState>`. `None` is a dry run (no
//!   connection); `Some(live)` is a run with `--apply` where `read_live` has
//!   already read the library. `plan()` keeps its signature; every existing
//!   caller that uses `..WriteInput::default()` keeps working.
//! - `lc_writer::plan::Conflict { photo: i64, field: String }` names only the
//!   Lightroom id and the field, never a value.
//! - `Plan::conflicts() -> &[Conflict]`, sorted ascending by `photo`, then by
//!   field in the fixed order rating, flag, label, title, caption, copyright,
//!   creator.
//! - `Plan::keywords_removed() -> usize`: keyword memberships that left a
//!   photo in Lightroom (`last` has it, `lr` does not). They are kept in
//!   LightCraft and only counted; `removeKeywords` is never sent.
//! - `ApplySummary::conflicts: Vec<Conflict>` and
//!   `ApplySummary::keywords_removed: usize`, filled from the plan. The
//!   `writer` section of `report.json` gets `conflicts`
//!   (`[{"photo": <Lightroom id>, "field": <name>}]`) and the counters
//!   `conflicts_count` and `keywords_removed`.
//!
//! Rules (per field of a photo that is in the mapping file and still in
//! LightCraft):
//! - `lr == last`: nothing, whatever LightCraft has (a change only in
//!   LightCraft is not touched);
//! - `lr != last && lc == last`: write the Lightroom value;
//! - `lr != last && lc == lr`: nothing (the values are already equal);
//! - `lr != last && lc != last && lc != lr`: conflict, nothing written,
//!   reported with photo id and field name only.
//!
//! Keywords, element by element:
//! - in `lr` but not in `last` and not in `lc`: add it;
//! - in `lr` but not in `last` and already in `lc`: nothing;
//! - in `last` but not in `lr`: report (count), never remove;
//! - in `lc` but not in `last`: keep it, send nothing.
//!
//! Never delete anything. A read error of the live state aborts the run
//! before anything is written. A second run reports the same conflicts and
//! sends nothing new.

// `..WriteInput::default()` stays valid when the struct gains fields.
#![allow(clippy::needless_update)]

use lc_writer::apply::{ApplySummary, apply};
use lc_writer::control::{ControlClient, WriteError};
use lc_writer::live::{LivePhoto, LiveState};
use lc_writer::plan::{Conflict, Plan, Step, WriteInput, plan};
use lc_writer::state::{PhotoFields, State, state_path};
use model::{ColorLabel, Flag, Keyword, Photo, Rating};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use fake_lightcraft::{Config, FakeLightCraft, SeedPhoto};
use lc_test_support::fake_lightcraft;

fn kw(parts: &[&str]) -> Keyword {
    Keyword::new(parts.iter().map(|s| s.to_string()).collect()).unwrap()
}

/// A photo of the model with a global id.
fn photo(gid: &str, lightroom_id: i64, rating: u8) -> Photo {
    Photo::new(
        format!("/Users/example/Pictures/{gid}.jpg"),
        lightroom_id,
        Rating::new(rating).unwrap(),
    )
    .with_global_id(gid)
}

/// The last imported values of a fresh import with the given keywords.
fn last(keywords: &[&str]) -> PhotoFields {
    let mut f = PhotoFields::imported_default();
    f.keywords = keywords.iter().map(|k| k.to_string()).collect();
    f
}

/// A live photo as LightCraft reports it; an empty text is `None`.
#[allow(clippy::too_many_arguments)]
fn live_photo(
    rating: u8,
    flag: &str,
    label: Option<&str>,
    title: Option<&str>,
    caption: Option<&str>,
    copyright: Option<&str>,
    creator: Option<&str>,
    keywords: &[&str],
) -> LivePhoto {
    LivePhoto {
        rating,
        flag: flag.to_string(),
        label: label.map(str::to_string),
        title: title.map(str::to_string),
        keywords: keywords.iter().map(|k| k.to_string()).collect(),
        caption: caption.map(str::to_string),
        copyright: copyright.map(str::to_string),
        creator: creator.map(str::to_string),
    }
}

/// A live state holding exactly these photos, by global id.
fn live_state(photos: &[(&str, LivePhoto)]) -> LiveState {
    LiveState {
        photos: photos
            .iter()
            .map(|(gid, p)| ((*gid).to_string(), p.clone()))
            .collect(),
        ..LiveState::default()
    }
}

fn input(
    photos: Vec<Photo>,
    photo_keywords: BTreeMap<i64, Vec<Keyword>>,
    live: Option<LiveState>,
) -> WriteInput {
    WriteInput {
        photos,
        photo_keywords,
        live,
        ..WriteInput::default()
    }
}

fn run_plan(input: &WriteInput, state: &State) -> Plan {
    plan(input, state, &BTreeSet::new())
}

/// The field update steps of a plan, in order.
fn updates(p: &Plan) -> Vec<(String, Value)> {
    p.steps
        .iter()
        .filter_map(|s| match s {
            Step::Update {
                command, params, ..
            } => Some((command.clone(), params.clone())),
            _ => None,
        })
        .collect()
}

/// True if any step carries a `removeKeywords` parameter.
fn removes_keywords(p: &Plan) -> bool {
    p.steps.iter().any(|s| match s {
        Step::Update { params, .. } => params.get("removeKeywords").is_some(),
        _ => false,
    })
}

fn conflict(photo: i64, field: &str) -> Conflict {
    Conflict {
        photo,
        field: field.to_string(),
    }
}

// --- a) default LightCraft value, Lightroom differs -> exactly one command ---

#[test]
fn a_default_lightcraft_value_gets_the_lightroom_command() {
    let mut state = State::default();
    state.record("g1", 7, json!(1001), last(&[]));
    let lr = Photo::new("a.jpg", 7, Rating::new(3).unwrap()).with_global_id("g1");
    let lc = live_photo(0, "none", None, None, None, None, None, &[]);
    let p = run_plan(
        &input(vec![lr], BTreeMap::new(), Some(live_state(&[("g1", lc)]))),
        &state,
    );
    assert_eq!(
        updates(&p),
        vec![("photo.rate".to_string(), json!({"rating": 3}))]
    );
    assert!(p.conflicts().is_empty());
}

// --- b) only LightCraft changed -> nothing ---

#[test]
fn b_a_change_only_in_lightcraft_is_left_alone() {
    let mut state = State::default();
    state.record("g1", 7, json!(1001), last(&[]));
    let lr = photo("g1", 7, 0);
    let lc = live_photo(
        5,
        "reject",
        Some("red"),
        Some("Only LC"),
        None,
        None,
        None,
        &["K"],
    );
    let p = run_plan(
        &input(vec![lr], BTreeMap::new(), Some(live_state(&[("g1", lc)]))),
        &state,
    );
    assert!(p.steps.is_empty(), "{:?}", p.steps);
    assert!(p.conflicts().is_empty());
}

// --- c) Lightroom changed, LightCraft still on `last` -> command ---

#[test]
fn c_a_lightroom_change_on_the_last_value_is_written() {
    let mut state = State::default();
    state.record("g1", 7, json!(1001), last(&[]));
    let lr = Photo::new("a.jpg", 7, Rating::new(4).unwrap())
        .with_global_id("g1")
        .with_title("New title");
    // LightCraft still has exactly what was last imported.
    let lc = live_photo(0, "none", None, None, None, None, None, &[]);
    let p = run_plan(
        &input(vec![lr], BTreeMap::new(), Some(live_state(&[("g1", lc)]))),
        &state,
    );
    assert_eq!(
        updates(&p),
        vec![
            ("photo.rate".to_string(), json!({"rating": 4})),
            ("photo.setMeta".to_string(), json!({"title": "New title"})),
        ]
    );
    assert!(p.conflicts().is_empty());
}

// --- d) both changed differently -> conflict, other fields normal ---

#[test]
fn d_a_field_changed_on_both_sides_is_a_conflict_and_not_written() {
    let mut state = State::default();
    state.record("g1", 7, json!(1001), last(&[]));
    let lr = Photo::new("a.jpg", 7, Rating::new(3).unwrap())
        .with_global_id("g1")
        .with_title("Lightroom title")
        .with_flag(Flag::Pick);
    let lc = live_photo(
        5,
        "none",
        None,
        Some("LightCraft title"),
        None,
        None,
        None,
        &[],
    );
    let p = run_plan(
        &input(vec![lr], BTreeMap::new(), Some(live_state(&[("g1", lc)]))),
        &state,
    );
    // rating and title are conflicts; the flag is only changed in Lightroom.
    assert_eq!(
        p.conflicts().to_vec(),
        vec![conflict(7, "rating"), conflict(7, "title")]
    );
    assert_eq!(
        updates(&p),
        vec![("photo.flag".to_string(), json!({"flag": "pick"}))]
    );
    // Nothing in the report may carry a value.
    let text = format!("{:?}", p.conflicts());
    for secret in ["Lightroom title", "LightCraft title", "pick", "5"] {
        assert!(!text.contains(secret), "{text}");
    }
}

#[test]
fn d_every_field_uses_the_field_name_in_the_conflict() {
    struct Case {
        field: &'static str,
        lr: Photo,
        lc: LivePhoto,
    }
    let base = |rating: u8| photo("g1", 7, rating);
    let cases = [
        Case {
            field: "rating",
            lr: base(3),
            lc: live_photo(5, "none", None, None, None, None, None, &[]),
        },
        Case {
            field: "flag",
            lr: base(0).with_flag(Flag::Pick),
            lc: live_photo(0, "reject", None, None, None, None, None, &[]),
        },
        Case {
            field: "label",
            lr: base(0).with_label(ColorLabel::Blue),
            lc: live_photo(0, "none", Some("red"), None, None, None, None, &[]),
        },
        Case {
            field: "title",
            lr: base(0).with_title("LR"),
            lc: live_photo(0, "none", None, Some("LC"), None, None, None, &[]),
        },
        Case {
            field: "caption",
            lr: base(0).with_caption("LR"),
            lc: live_photo(0, "none", None, None, Some("LC"), None, None, &[]),
        },
        Case {
            field: "copyright",
            lr: base(0).with_copyright("LR"),
            lc: live_photo(0, "none", None, None, None, Some("LC"), None, &[]),
        },
        Case {
            field: "creator",
            lr: base(0).with_creator("LR"),
            lc: live_photo(0, "none", None, None, None, None, Some("LC"), &[]),
        },
    ];
    for case in cases {
        let mut state = State::default();
        state.record("g1", 7, json!(1001), last(&[]));
        let p = run_plan(
            &input(
                vec![case.lr],
                BTreeMap::new(),
                Some(live_state(&[("g1", case.lc)])),
            ),
            &state,
        );
        assert!(
            p.steps.is_empty(),
            "{} must not be written: {:?}",
            case.field,
            p.steps
        );
        assert_eq!(
            p.conflicts().to_vec(),
            vec![conflict(7, case.field)],
            "{}",
            case.field
        );
    }
}

#[test]
fn conflicts_are_sorted_by_photo_and_field() {
    let mut state = State::default();
    state.record("a", 9, json!(2001), last(&[]));
    state.record("b", 4, json!(2002), last(&[]));
    // `a` (Lightroom id 9) comes first in global-id order but last in the
    // report; within one photo the fixed field order applies.
    let a = Photo::new("a.jpg", 9, Rating::new(1).unwrap())
        .with_global_id("a")
        .with_title("LR a");
    let b = Photo::new("b.jpg", 4, Rating::new(2).unwrap())
        .with_global_id("b")
        .with_title("LR b");
    let live = live_state(&[
        (
            "a",
            live_photo(3, "none", None, Some("LC a"), None, None, None, &[]),
        ),
        (
            "b",
            live_photo(5, "none", None, Some("LC b"), None, None, None, &[]),
        ),
    ]);
    let p = run_plan(&input(vec![a, b], BTreeMap::new(), Some(live)), &state);
    assert_eq!(
        p.conflicts().to_vec(),
        vec![
            conflict(4, "rating"),
            conflict(4, "title"),
            conflict(9, "rating"),
            conflict(9, "title"),
        ]
    );
}

// --- e) both sides have the same new value -> nothing ---

#[test]
fn e_equal_new_values_need_no_command() {
    let mut state = State::default();
    state.record("g1", 7, json!(1001), last(&[]));
    let lr = Photo::new("a.jpg", 7, Rating::new(3).unwrap())
        .with_global_id("g1")
        .with_title("Same");
    let lc = live_photo(3, "none", None, Some("Same"), None, None, None, &[]);
    let p = run_plan(
        &input(vec![lr], BTreeMap::new(), Some(live_state(&[("g1", lc)]))),
        &state,
    );
    assert!(p.steps.is_empty(), "{:?}", p.steps);
    assert!(p.conflicts().is_empty());
}

// --- f) keywords, element by element ---

#[test]
fn f_a_keyword_new_in_lightroom_is_added() {
    let mut state = State::default();
    state.record("g1", 7, json!(1001), last(&["A"]));
    let lr = photo("g1", 7, 0);
    let lc = live_photo(0, "none", None, None, None, None, None, &["A"]);
    let mut photo_keywords = BTreeMap::new();
    photo_keywords.insert(7, vec![kw(&["A"]), kw(&["B"])]);
    let p = run_plan(
        &input(vec![lr], photo_keywords, Some(live_state(&[("g1", lc)]))),
        &state,
    );
    assert_eq!(
        updates(&p),
        vec![("photo.setMeta".to_string(), json!({"addKeywords": ["B"]}))]
    );
    assert!(!removes_keywords(&p));
    assert_eq!(p.keywords_removed(), 0);
}

#[test]
fn f_a_keyword_new_in_lightroom_but_already_in_lightcraft_is_not_sent() {
    let mut state = State::default();
    state.record("g1", 7, json!(1001), last(&["A"]));
    let lr = photo("g1", 7, 0);
    let lc = live_photo(0, "none", None, None, None, None, None, &["A", "B"]);
    let mut photo_keywords = BTreeMap::new();
    photo_keywords.insert(7, vec![kw(&["A"]), kw(&["B"])]);
    let p = run_plan(
        &input(vec![lr], photo_keywords, Some(live_state(&[("g1", lc)]))),
        &state,
    );
    assert!(updates(&p).is_empty(), "{:?}", p.steps);
}

#[test]
fn f_a_keyword_removed_in_lightroom_is_only_reported_and_never_removed() {
    let mut state = State::default();
    state.record("g1", 7, json!(1001), last(&["A", "B"]));
    let lr = photo("g1", 7, 0);
    // LightCraft still has both keywords.
    let lc = live_photo(0, "none", None, None, None, None, None, &["A", "B"]);
    let mut photo_keywords = BTreeMap::new();
    photo_keywords.insert(7, vec![kw(&["A"])]);
    let p = run_plan(
        &input(vec![lr], photo_keywords, Some(live_state(&[("g1", lc)]))),
        &state,
    );
    assert!(updates(&p).is_empty(), "{:?}", p.steps);
    assert!(!removes_keywords(&p), "never send removeKeywords");
    assert_eq!(p.keywords_removed(), 1);
}

#[test]
fn f_a_keyword_only_in_lightcraft_is_kept() {
    let mut state = State::default();
    state.record("g1", 7, json!(1001), last(&[]));
    let lr = photo("g1", 7, 0);
    let lc = live_photo(0, "none", None, None, None, None, None, &["C"]);
    let p = run_plan(
        &input(vec![lr], BTreeMap::new(), Some(live_state(&[("g1", lc)]))),
        &state,
    );
    assert!(p.steps.is_empty(), "{:?}", p.steps);
    assert!(!removes_keywords(&p));
    assert_eq!(p.keywords_removed(), 0);
}

#[test]
fn f_keywords_are_never_removed_even_without_live_state() {
    // A dry run has no live state, but the plan must still never propose a
    // removal: LightCraft keeps every keyword it has.
    let mut state = State::default();
    state.record("g1", 7, json!(1001), last(&["A", "B"]));
    let lr = photo("g1", 7, 0);
    let mut photo_keywords = BTreeMap::new();
    photo_keywords.insert(7, vec![kw(&["A"])]);
    let p = run_plan(&input(vec![lr], photo_keywords, None), &state);
    assert!(!removes_keywords(&p), "{:?}", p.steps);
    assert_eq!(p.keywords_removed(), 1);
}

// --- g) second run: same conflicts, nothing new ---

struct Env {
    _dir: tempfile::TempDir,
    library: PathBuf,
    state_file: PathBuf,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("Test.lightcraft");
    let state_file = state_path(&library);
    Env {
        _dir: dir,
        library,
        state_file,
    }
}

fn apply_once(
    fake: &FakeLightCraft,
    env: &Env,
    input: &WriteInput,
) -> Result<ApplySummary, WriteError> {
    let mut state = State::load(&env.state_file).unwrap();
    let mut client = ControlClient::connect(fake.port()).unwrap();
    apply(&mut client, input, &mut state, &env.state_file)
}

#[test]
fn g_a_second_run_reports_the_same_conflicts_and_writes_nothing() {
    let env = env();
    let mut state = State::default();
    state.record("g1", 7, json!(1001), last(&[]));
    state.save(&env.state_file).unwrap();

    // LightCraft already has 5 stars; the mapping file says 0; Lightroom now
    // says 3: both sides changed, differently.
    let mut config = Config::new(&env.library);
    config.seed_photos = vec![SeedPhoto {
        rating: 5,
        ..SeedPhoto::plain(1001)
    }];
    let fake = FakeLightCraft::start(config);
    let input = input(vec![photo("g1", 7, 3)], BTreeMap::new(), None);

    let first = apply_once(&fake, &env, &input).unwrap();
    assert_eq!(first.conflicts, vec![conflict(7, "rating")]);
    assert_eq!(first.updates, 0);
    assert!(fake.write_commands().is_empty(), "{:?}", fake.commands());

    let second = apply_once(&fake, &env, &input).unwrap();
    assert_eq!(second.conflicts, first.conflicts);
    assert_eq!(second.updates, 0);
    assert!(fake.write_commands().is_empty(), "{:?}", fake.commands());
    // The seeded value is still there: nothing was overwritten.
    let mut client = ControlClient::connect(fake.port()).unwrap();
    let state = State::load(&env.state_file).unwrap();
    let live = lc_writer::live::read_live(&mut client, &state).unwrap();
    assert_eq!(live.photos["g1"].rating, 5);
}

// --- h) a read error aborts before anything is written ---

#[test]
fn h_a_live_read_error_aborts_without_a_partial_write() {
    let env = env();
    let mut state = State::default();
    state.record("g1", 7, json!(1001), last(&[]));
    state.save(&env.state_file).unwrap();
    let before = std::fs::read(&env.state_file).unwrap();

    let mut config = Config::new(&env.library);
    config.seed_photos = vec![SeedPhoto::plain(1001)];
    config.summary_broken = true;
    let fake = FakeLightCraft::start(config);
    let input = input(vec![photo("g1", 7, 3)], BTreeMap::new(), None);

    let err = apply_once(&fake, &env, &input).unwrap_err();
    assert!(matches!(err, WriteError::Protocol(_)), "{err:?}");
    assert!(fake.write_commands().is_empty(), "{:?}", fake.commands());
    assert_eq!(
        std::fs::read(&env.state_file).unwrap(),
        before,
        "the mapping file is not changed"
    );
}

// --- i) converged values are recorded even without a step ---

/// The recorded fields of one photo in the mapping file on disk.
fn recorded(env: &Env, global_id: &str) -> PhotoFields {
    State::load(&env.state_file)
        .unwrap()
        .photo(global_id)
        .unwrap()
        .fields
        .clone()
}

#[test]
fn i_a_value_equal_on_both_sides_updates_the_base_so_a_later_change_is_written() {
    let env = env();
    let mut state = State::default();
    state.record("g1", 7, json!(1001), last(&[]));
    state.save(&env.state_file).unwrap();

    // Lightroom and LightCraft both moved to 3 stars: nothing to write, but
    // the base must become 3 so that a later Lightroom change is not read as
    // a conflict.
    let mut config = Config::new(&env.library);
    config.seed_photos = vec![SeedPhoto {
        rating: 3,
        ..SeedPhoto::plain(1001)
    }];
    let fake = FakeLightCraft::start(config);
    let first = apply_once(
        &fake,
        &env,
        &input(vec![photo("g1", 7, 3)], BTreeMap::new(), None),
    )
    .unwrap();
    assert_eq!(first.updates, 0, "{first:?}");
    assert!(first.conflicts.is_empty(), "{first:?}");
    assert!(fake.write_commands().is_empty(), "{:?}", fake.commands());
    assert_eq!(
        recorded(&env, "g1").rating,
        3,
        "the base follows the converged value"
    );

    // Only Lightroom moves now (3 -> 5); LightCraft still has 3: write it.
    let second = apply_once(
        &fake,
        &env,
        &input(vec![photo("g1", 7, 5)], BTreeMap::new(), None),
    )
    .unwrap();
    assert_eq!(second.updates, 1, "{second:?}");
    assert!(second.conflicts.is_empty(), "{second:?}");
    assert_eq!(fake.ratings()[&1001], 5);
}

#[test]
fn i_a_keyword_removed_in_lightroom_is_counted_once_and_stops_being_recorded() {
    let env = env();
    let mut state = State::default();
    state.record("g1", 7, json!(1001), last(&["A", "B"]));
    state.save(&env.state_file).unwrap();

    // Lightroom removed B and added C; LightCraft already has C, so there is
    // no command at all. The removal must still be reported exactly once.
    let mut config = Config::new(&env.library);
    config.keywords = ["A", "B", "C"].iter().map(|k| k.to_string()).collect();
    config.seed_photos = vec![SeedPhoto {
        keywords: vec!["A".into(), "B".into(), "C".into()],
        ..SeedPhoto::plain(1001)
    }];
    let fake = FakeLightCraft::start(config);
    let mut photo_keywords = BTreeMap::new();
    photo_keywords.insert(7, vec![kw(&["A"]), kw(&["C"])]);
    let input = input(vec![photo("g1", 7, 0)], photo_keywords, None);

    let first = apply_once(&fake, &env, &input).unwrap();
    assert_eq!(first.keywords_removed, 1, "{first:?}");
    assert_eq!(first.updates, 0, "{first:?}");
    assert!(fake.write_commands().is_empty(), "{:?}", fake.commands());
    assert_eq!(
        recorded(&env, "g1").keywords,
        ["A".to_string(), "C".to_string()],
        "B is no longer the recorded base"
    );

    let second = apply_once(&fake, &env, &input).unwrap();
    assert_eq!(second.keywords_removed, 0, "{second:?}");
    assert_eq!(second.updates, 0, "{second:?}");
}

// --- no live state: the dry run keeps the two-way behaviour ---

#[test]
fn without_live_state_a_lightroom_change_is_still_planned() {
    let mut state = State::default();
    state.record("g1", 7, json!(1001), last(&[]));
    let lr = Photo::new("a.jpg", 7, Rating::new(3).unwrap()).with_global_id("g1");
    let p = run_plan(&input(vec![lr], BTreeMap::new(), None), &state);
    assert_eq!(
        updates(&p),
        vec![("photo.rate".to_string(), json!({"rating": 3}))]
    );
    assert!(p.conflicts().is_empty());
}
