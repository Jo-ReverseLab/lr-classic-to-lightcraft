//! Acceptance tests for recorded smart albums whose Lightroom collection
//! still exists but can no longer be translated, modules
//! `lc_writer::plan` and `lc_writer::apply`.
//!
//! API under test:
//! - `WriteInput.smart_untranslatable_ids: Vec<i64>`: the Lightroom ids of
//!   every smart collection that was read but is NOT in `WriteInput.smart`
//!   (untranslatable, or its parent is not a collection set). The caller
//!   fills it; the default is empty;
//! - `Plan::smart_no_longer_translatable() -> usize` and
//!   `ApplySummary.smart_no_longer_translatable: usize`.
//!
//! Rules:
//! - a mapping entry of kind `smart` whose Lightroom id is not in
//!   `input.smart` but is in `input.smart_untranslatable_ids` counts in
//!   `smart_no_longer_translatable`, NOT in `collections_removed`;
//! - `collections_removed` counts only entries whose collection is in
//!   neither list (really gone from Lightroom), regular or smart;
//! - an entry that is in `input.smart` still counts as `smart_known` (and,
//!   when the rules changed, `smart_rules_changed`), never in the new
//!   counter;
//! - a regular (non-smart) entry whose id is in `smart_untranslatable_ids`
//!   is still `collections_removed` (the list only explains smart entries);
//! - nothing is deleted, created or removed from the mapping file; no album
//!   command is sent for such an entry;
//! - the existing balance (`smart.len() == smart_created + smart_known +
//!   smart_existing + smart_skipped.len()`) is unchanged: untranslatable
//!   collections stay out of `input.smart` and out of the balance.

use lc_writer::apply::{ApplySummary, apply};
use lc_writer::control::ControlClient;
use lc_writer::plan::{Plan, WriteInput, plan};
use lc_writer::smart::translate;
use lc_writer::state::State;
use model::{SmartCollection, SmartCombine, SmartRule, SmartRules, SmartValue};
use serde_json::{Value, json};
use std::collections::BTreeSet;

#[path = "support/fake_lightcraft.rs"]
mod fake_lightcraft;
use fake_lightcraft::{Config, FakeLightCraft};

fn good() -> SmartRules {
    SmartRules {
        combine: SmartCombine::Intersect,
        rules: vec![SmartRule {
            criterion: "rating".into(),
            operation: ">=".into(),
            value: Some(SmartValue::Number(3.0)),
            value2: None,
            units: None,
        }],
    }
}

fn smart(id: i64, name: &str) -> SmartCollection {
    SmartCollection {
        lightroom_id: id,
        name: name.into(),
        parent: None,
        rules: good(),
    }
}

fn recorded(id: u64, kind: &str, name: &str) -> Value {
    json!({
        "lightcraft_id": id,
        "kind": kind,
        "name": name,
        "parent": null,
        "members": [],
        "rules": if kind == "smart" { translate(&good()).unwrap() } else { Value::Null },
    })
}

fn state_file(dir: &tempfile::TempDir, collections: Value) -> std::path::PathBuf {
    let file = dir.path().join("x.lr2lc.json");
    let text = json!({"version": 1, "photos": {}, "keywords": {}, "collections": collections});
    std::fs::write(&file, text.to_string()).unwrap();
    file
}

fn input(smart_list: Vec<SmartCollection>, untranslatable: Vec<i64>) -> WriteInput {
    WriteInput {
        smart: smart_list,
        smart_untranslatable_ids: untranslatable,
        ..WriteInput::default()
    }
}

/// Entries: 7 smart (still translatable), 8 smart (no longer
/// translatable), 9 smart (gone), 10 regular album (gone, but its id is in
/// the untranslatable list by accident of the input).
fn entries() -> Value {
    json!({
        "7": recorded(7, "smart", "Keep"),
        "8": recorded(8, "smart", "Untranslatable"),
        "9": recorded(9, "smart", "Gone"),
        "10": recorded(10, "album", "Plain"),
    })
}

fn the_input() -> WriteInput {
    input(vec![smart(7, "Keep")], vec![8, 10])
}

fn do_plan(inp: &WriteInput, state: &State) -> Plan {
    plan(inp, state, &BTreeSet::new())
}

#[test]
fn plan_counts_no_longer_translatable_apart_from_removed() {
    let dir = tempfile::tempdir().unwrap();
    let state = State::load(&state_file(&dir, entries())).unwrap();
    let p = do_plan(&the_input(), &state);
    assert_eq!(p.smart_no_longer_translatable(), 1, "entry 8");
    assert_eq!(p.collections_removed(), 2, "entry 9 and the regular 10");
    assert_eq!(p.smart_known(), 1);
    assert!(p.steps.is_empty());
}

#[test]
fn without_the_list_the_entry_counts_as_removed_as_before() {
    let dir = tempfile::tempdir().unwrap();
    let state = State::load(&state_file(&dir, entries())).unwrap();
    let p = do_plan(&input(vec![smart(7, "Keep")], vec![]), &state);
    assert_eq!(p.smart_no_longer_translatable(), 0);
    assert_eq!(p.collections_removed(), 3);
}

#[test]
fn a_translatable_entry_never_counts_as_no_longer_translatable() {
    let dir = tempfile::tempdir().unwrap();
    let state = State::load(&state_file(&dir, entries())).unwrap();
    // 7 is in both lists by mistake: input.smart wins.
    let p = do_plan(&input(vec![smart(7, "Keep")], vec![7, 8]), &state);
    assert_eq!((p.smart_known(), p.smart_no_longer_translatable()), (1, 1));
}

#[test]
fn an_untranslatable_collection_without_an_entry_counts_nowhere() {
    let state = State::default();
    let p = do_plan(&input(vec![], vec![44]), &state);
    assert_eq!(p.smart_no_longer_translatable(), 0);
    assert_eq!(p.collections_removed(), 0);
    assert!(p.steps.is_empty());
}

#[test]
fn apply_reports_the_new_counter_and_leaves_everything_alone() {
    let dir = tempfile::tempdir().unwrap();
    let file = state_file(&dir, entries());
    let before = std::fs::read_to_string(&file).unwrap();
    let fake = FakeLightCraft::start(Config::new(&dir.path().join("Test.lightcraft")));
    let mut state = State::load(&file).unwrap();
    let mut client = ControlClient::connect(fake.port()).unwrap();
    let inp = the_input();
    let s: ApplySummary = apply(&mut client, &inp, &mut state, &file).unwrap();
    assert_eq!(s.smart_no_longer_translatable, 1);
    assert_eq!(s.collections_removed, 2);
    assert_eq!((s.smart_created, s.smart_known), (0, 1));
    assert_eq!(
        inp.smart.len(),
        s.smart_created + s.smart_known + s.smart_skipped.len()
    );
    assert!(
        fake.commands()
            .iter()
            .all(|(c, _)| !c.starts_with("album.")),
        "{:?}",
        fake.commands()
    );
    assert!(fake.albums().is_empty());
    // The mapping file keeps every entry.
    let after: Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap_or(before.clone())).unwrap();
    assert_eq!(
        after["collections"].as_object().unwrap().len(),
        4,
        "{after}"
    );
}

#[test]
fn a_second_apply_gives_the_same_counters() {
    let dir = tempfile::tempdir().unwrap();
    let file = state_file(&dir, entries());
    let fake = FakeLightCraft::start(Config::new(&dir.path().join("Test.lightcraft")));
    let mut counters = Vec::new();
    for _ in 0..2 {
        let mut state = State::load(&file).unwrap();
        let mut client = ControlClient::connect(fake.port()).unwrap();
        let s = apply(&mut client, &the_input(), &mut state, &file).unwrap();
        counters.push((s.smart_no_longer_translatable, s.collections_removed));
    }
    assert_eq!(counters[0], counters[1]);
    assert_eq!(counters[0], (1, 2));
}
