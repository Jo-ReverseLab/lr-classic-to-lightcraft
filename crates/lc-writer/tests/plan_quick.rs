//! Acceptance tests for planning the quick collection (task P5b-6), module
//! `lc_writer::plan`.
//!
//! STATUS: waiting for P5b-6. Disabled by the crate-level `cfg(any())`; the
//! developer may ONLY remove that line.
//!
//! API: `WriteInput.quick: Option<model::QuickCollection>`,
//! `Step::QuickAdd { gids: Vec<String> }`, the mapping file key
//! `quick = {members: [gids]}` (the state is built by loading a file with
//! `version`, empty `photos` and `keywords` (required keys) and `quick`).
//!
//! Rules: one `QuickAdd` at the very end of the plan (after album steps)
//! with the members, in Lightroom order, that are not in `state.quick`;
//! none when there is nothing to add.

use lc_writer::plan::{Step, WriteInput, plan};
use lc_writer::state::State;
use model::{Collection, CollectionKind, Photo, QuickCollection, Rating};
use serde_json::json;
use std::collections::BTreeSet;

fn photos() -> Vec<Photo> {
    (101..=104)
        .map(|i| {
            Photo::new(
                format!("/Users/example/Pictures/{i}.jpg"),
                i,
                Rating::default(),
            )
            .with_global_id(format!("g{i}"))
        })
        .collect()
}

fn input(members: &[i64]) -> WriteInput {
    WriteInput {
        photos: photos(),
        collections: vec![Collection {
            lightroom_id: 4,
            name: "Album".into(),
            kind: CollectionKind::Album,
            parent: None,
            members: vec![102],
        }],
        quick: Some(QuickCollection {
            lightroom_id: 50,
            members: members.to_vec(),
        }),
        ..WriteInput::default()
    }
}

fn state_with(quick: &[&str]) -> State {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    std::fs::write(
        &path,
        json!({"version": 1, "photos": {}, "keywords": {}, "quick": {"members": quick}})
            .to_string(),
    )
    .unwrap();
    State::load(&path).unwrap()
}

fn quick_steps(input: &WriteInput, state: &State) -> Vec<Vec<String>> {
    plan(input, state, &BTreeSet::new())
        .steps
        .iter()
        .filter_map(|s| match s {
            Step::QuickAdd { gids } => Some(gids.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn quick_add_lists_the_members_in_lightroom_order() {
    let steps = quick_steps(&input(&[103, 101]), &state_with(&[]));
    assert_eq!(steps, vec![vec!["g103".to_string(), "g101".to_string()]]);
}

#[test]
fn quick_add_is_the_last_step() {
    let p = plan(&input(&[103]), &state_with(&[]), &BTreeSet::new());
    assert!(matches!(p.steps.last(), Some(Step::QuickAdd { .. })));
    assert!(p.steps.len() > 1);
}

#[test]
fn members_in_the_mapping_file_are_left_out() {
    let steps = quick_steps(&input(&[103, 101, 104]), &state_with(&["g101"]));
    assert_eq!(steps, vec![vec!["g103".to_string(), "g104".to_string()]]);
}

#[test]
fn nothing_to_add_means_no_step() {
    assert!(quick_steps(&input(&[103, 101]), &state_with(&["g101", "g103"])).is_empty());
    assert!(quick_steps(&input(&[]), &state_with(&[])).is_empty());
    let mut i = input(&[]);
    i.quick = None;
    assert!(quick_steps(&i, &state_with(&[])).is_empty());
}
