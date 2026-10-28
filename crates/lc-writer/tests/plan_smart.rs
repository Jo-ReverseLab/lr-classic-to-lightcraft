//! Acceptance tests for the pure planning rules of smart collections (task
//! P5b-4b), module `lc_writer::plan`.
//!
//! API under test (all pub; the translation itself is tested in
//! `smart_translate.rs`):
//! - `model::SmartCollection { lightroom_id: i64, name: String, parent:
//!   Option<i64>, rules: SmartRules }` (Debug, Clone, PartialEq), re-exported
//!   at the crate root of `model`;
//! - `WriteInput.smart: Vec<model::SmartCollection>`;
//! - `Step::CreateSmart { collection_id: i64, params: Value }`;
//! - `Plan::smart_known() -> usize`, `Plan::smart_rules_changed() -> usize`,
//!   `Plan::smart_skips() -> &[Skipped]`.
//!
//! The mapping file keeps a smart album under the same key as every other
//! collection (`collections.<Lightroom id>`), with `kind: "smart"`, an empty
//! `members` and the translated rules that were sent: `{lightcraft_id, kind:
//! "smart", name, parent, members: [], rules: <the `rules` parameter>}`.
//! `rules` is optional when loading (`#[serde(default)]`).
//!
//! Rules:
//! - `CreateSmart.params` is exactly `{name, parent?, rules}`; `rules` is
//!   always present and equals `lc_writer::smart::translate`; `parent` is
//!   absent on top level, the LightCraft id (number) of a parent folder that
//!   is recorded, or the text `lr-collection:<Lightroom id>` for a folder
//!   that this plan creates;
//! - all `CreateSmart` steps come after every `CreateAlbum`, `AddMembers` and
//!   `RemoveMembers` step, in the order of `WriteInput.smart`;
//! - a smart collection that cannot be translated, or whose parent is not a
//!   folder of `WriteInput.collections`, gets no step and one entry in
//!   `smart_skips()`; the reason names the Lightroom id of the collection
//!   and, for a rule, the criterion name - never a collection name, value,
//!   operation or combine text;
//! - a recorded smart collection gets no step, however its name, parent or
//!   rules changed in Lightroom; it counts in `smart_known()`; if its
//!   translation is no longer equal to the recorded `rules` (or no longer
//!   possible) it also counts in `smart_rules_changed()` (reported, never
//!   written);
//! - recorded smart collections are never counted in `collections_removed()`
//!   or any other collection counter, and the rename/move counters
//!   (`collections_renamed()`, `collections_moved()`) also cover them;
//! - `to_script` has one `album.createSmart` line per step, with the same
//!   params.

use lc_writer::plan::{Plan, Step, WriteInput, plan};
use lc_writer::smart::translate;
use lc_writer::state::State;
use model::{
    Collection, CollectionKind, SmartCollection, SmartCombine, SmartRule, SmartRules, SmartValue,
};
use serde_json::{Value, json};
use std::collections::BTreeSet;

const SECRET: &str = "ZyxSecret";

fn rule(criterion: &str, operation: &str, value: Option<SmartValue>) -> SmartRule {
    SmartRule {
        criterion: criterion.to_string(),
        operation: operation.to_string(),
        value,
        value2: None,
        units: None,
    }
}

fn rating(op: &str, stars: f64) -> SmartRule {
    rule("rating", op, Some(SmartValue::Number(stars)))
}

fn rules(combine: SmartCombine, rules: Vec<SmartRule>) -> SmartRules {
    SmartRules { combine, rules }
}

fn good() -> SmartRules {
    rules(SmartCombine::Intersect, vec![rating(">=", 3.0)])
}

fn smart(id: i64, name: &str, parent: Option<i64>, rules: SmartRules) -> SmartCollection {
    SmartCollection {
        lightroom_id: id,
        name: name.to_string(),
        parent,
        rules,
    }
}

fn folder(id: i64, name: &str, parent: Option<i64>) -> Collection {
    Collection {
        lightroom_id: id,
        name: name.to_string(),
        kind: CollectionKind::Folder,
        parent,
        members: Vec::new(),
    }
}

fn album(id: i64, name: &str) -> Collection {
    Collection {
        lightroom_id: id,
        name: name.to_string(),
        kind: CollectionKind::Album,
        parent: None,
        members: Vec::new(),
    }
}

fn input(collections: Vec<Collection>, smart: Vec<SmartCollection>) -> WriteInput {
    WriteInput {
        collections,
        smart,
        ..WriteInput::default()
    }
}

fn state_with(collections: Value) -> State {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("x.lr2lc.json");
    let text = json!({"version": 1, "photos": {}, "keywords": {}, "collections": collections});
    std::fs::write(&file, text.to_string()).unwrap();
    State::load(&file).unwrap()
}

fn run(input: &WriteInput, state: &State) -> Plan {
    plan(input, state, &BTreeSet::new())
}

fn smart_steps(p: &Plan) -> Vec<(i64, &Value)> {
    p.steps
        .iter()
        .filter_map(|s| match s {
            Step::CreateSmart {
                collection_id,
                params,
            } => Some((*collection_id, params)),
            _ => None,
        })
        .collect()
}

fn recorded_smart(id: u64, name: &str, parent: Option<i64>, rules: &SmartRules) -> Value {
    json!({
        "lightcraft_id": id,
        "kind": "smart",
        "name": name,
        "parent": parent,
        "members": [],
        "rules": translate(rules).unwrap(),
    })
}

#[test]
fn top_level_smart_collection_gets_name_and_rules_only() {
    let p = run(
        &input(vec![], vec![smart(7, "Best", None, good())]),
        &State::default(),
    );
    let steps = smart_steps(&p);
    assert_eq!(steps.len(), 1);
    assert_eq!(steps[0].0, 7);
    assert_eq!(
        steps[0].1,
        &json!({"name": "Best", "rules": translate(&good()).unwrap()})
    );
    assert!(p.smart_skips().is_empty());
    assert_eq!((p.smart_known(), p.smart_rules_changed()), (0, 0));
}

#[test]
fn parent_is_a_placeholder_for_a_folder_created_by_the_plan() {
    let p = run(
        &input(
            vec![folder(1, "Set", None)],
            vec![smart(7, "Best", Some(1), good())],
        ),
        &State::default(),
    );
    let steps = smart_steps(&p);
    assert_eq!(steps[0].1["parent"], "lr-collection:1");
    assert_eq!(steps[0].1.as_object().unwrap().len(), 3);
}

#[test]
fn parent_is_the_lightcraft_id_of_a_recorded_folder() {
    let state = state_with(json!({"1": {
        "lightcraft_id": 5001, "kind": "folder", "name": "Set",
        "parent": null, "members": [],
    }}));
    let p = run(
        &input(
            vec![folder(1, "Set", None)],
            vec![smart(7, "Best", Some(1), good())],
        ),
        &state,
    );
    assert_eq!(smart_steps(&p)[0].1["parent"], json!(5001));
}

#[test]
fn smart_steps_come_after_every_album_and_member_step() {
    let mut col = vec![folder(1, "Set", None), album(2, "Plain")];
    col[1].members = vec![101];
    let mut inp = input(
        col,
        vec![
            smart(8, "Second", None, good()),
            smart(7, "First", Some(1), good()),
        ],
    );
    inp.photos = vec![
        model::Photo::new(
            "/Users/example/Pictures/101.jpg",
            101,
            model::Rating::default(),
        )
        .with_global_id("g101"),
    ];
    let p = run(&inp, &State::default());
    let last_other = p
        .steps
        .iter()
        .rposition(|s| !matches!(s, Step::CreateSmart { .. }))
        .unwrap();
    let first_smart = p
        .steps
        .iter()
        .position(|s| matches!(s, Step::CreateSmart { .. }))
        .unwrap();
    assert!(last_other < first_smart, "{:?}", p.steps);
    // Input order is kept.
    let ids: Vec<i64> = smart_steps(&p).iter().map(|(id, _)| *id).collect();
    assert_eq!(ids, [8, 7]);
}

#[test]
fn untranslatable_collections_are_skipped_with_the_criterion_name() {
    let bad = rules(
        SmartCombine::Intersect,
        vec![
            rating(">=", 3.0),
            rule(
                "labelColor",
                "==",
                Some(SmartValue::Text(format!("red {SECRET}"))),
            ),
        ],
    );
    let p = run(
        &input(
            vec![],
            vec![
                smart(7, &format!("Name {SECRET}"), None, bad),
                smart(8, "Fine", None, good()),
            ],
        ),
        &State::default(),
    );
    assert_eq!(smart_steps(&p).len(), 1);
    assert_eq!(p.smart_skips().len(), 1);
    let reason = &p.smart_skips()[0].reason;
    assert!(
        reason.contains('7') && reason.contains("labelColor"),
        "{reason}"
    );
    assert!(
        !reason.contains(SECRET) && !reason.contains("red"),
        "{reason}"
    );
}

#[test]
fn parent_that_is_not_a_converted_folder_skips_the_collection() {
    let p = run(
        &input(
            vec![album(2, "Plain")],
            vec![
                smart(7, &format!("A {SECRET}"), Some(2), good()),
                smart(8, "B", Some(99), good()),
            ],
        ),
        &State::default(),
    );
    assert!(smart_steps(&p).is_empty());
    assert_eq!(p.smart_skips().len(), 2);
    for s in p.smart_skips() {
        assert!(s.reason.contains("parent"), "{}", s.reason);
        assert!(!s.reason.contains(SECRET), "{}", s.reason);
    }
}

#[test]
fn recorded_smart_collection_gets_no_step_even_if_changed() {
    let state = state_with(json!({
        "7": recorded_smart(7, "Best", None, &good()),
        "8": recorded_smart(8, "Old name", None, &good()),
        "9": recorded_smart(9, "Moved", None, &good()),
    }));
    let changed = rules(SmartCombine::Intersect, vec![rating(">=", 4.0)]);
    let p = run(
        &input(
            vec![folder(1, "Set", None)],
            vec![
                smart(7, "Best", None, good()),
                smart(8, "New name", None, good()),
                smart(9, "Moved", Some(1), changed),
            ],
        ),
        &state,
    );
    assert!(smart_steps(&p).is_empty());
    assert!(p.smart_skips().is_empty());
    assert_eq!(p.smart_known(), 3);
    assert_eq!(p.smart_rules_changed(), 1);
    assert_eq!(p.collections_renamed(), 1);
    assert_eq!(p.collections_moved(), 1);
    assert_eq!(p.collections_removed(), 0);
}

#[test]
fn recorded_smart_collection_that_is_no_longer_translatable_counts_as_changed() {
    let state = state_with(json!({"7": recorded_smart(7, "Best", None, &good())}));
    let now = rules(
        SmartCombine::Intersect,
        vec![rule("pick", "==", Some(SmartValue::Number(1.0)))],
    );
    let p = run(&input(vec![], vec![smart(7, "Best", None, now)]), &state);
    assert!(smart_steps(&p).is_empty());
    assert_eq!((p.smart_known(), p.smart_rules_changed()), (1, 1));
    assert!(p.smart_skips().is_empty());
}

#[test]
fn recorded_smart_collection_missing_from_the_input_is_only_counted_as_removed() {
    let state = state_with(json!({"7": recorded_smart(7, "Best", None, &good())}));
    let p = run(&input(vec![], vec![]), &state);
    assert!(p.steps.is_empty());
    assert_eq!(p.collections_removed(), 1);
    assert_eq!(p.smart_known(), 0);
}

#[test]
fn script_has_one_create_smart_line_per_step() {
    let p = run(
        &input(
            vec![folder(1, "Set", None)],
            vec![smart(7, "Best", Some(1), good())],
        ),
        &State::default(),
    );
    let script = p.to_script(&State::default());
    let lines: Vec<Value> = script
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .filter(|v: &Value| v["params"]["command"] == "album.createSmart")
        .collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["params"]["params"], *smart_steps(&p)[0].1);
    // Dry run only: the plan never changes the state.
    assert_eq!(State::default(), State::default());
}
