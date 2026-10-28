//! Acceptance tests for the pure planning rules of stacks (task P5b-5),
//! module `lc_writer::plan`.
//!
//! STATUS: waiting for P5b-5. The whole file is disabled by the crate-level
//! `cfg(any())` below. The developer may ONLY remove that line. Any other
//! change to this file needs the test author.
//!
//! API under test (all pub):
//! - `WriteInput.stacks: Vec<model::Stack>`;
//! - `Step::GroupStack { stack_id: i64, gids: Vec<String> }`;
//! - `Plan::stacks_known() -> usize`: input stacks that are in the mapping
//!   file with the same members in the same order (no command);
//! - `Plan::stack_skips() -> &[Skipped]`: stacks left with fewer than two
//!   members that have a global id; the reason names the stack's Lightroom
//!   id and says "fewer than two";
//! - `Plan::stack_members_removed() -> usize`: recorded members of an input
//!   stack that are not members of it any more.
//!
//! The mapping file keeps `stacks`, keyed by the Lightroom stack id as text:
//! `{members: [global ids, top first]}`; `#[serde(default)]`, so a file
//! without the key still loads; `version` stays 1.
//!
//! Rules:
//! - plan order: all photo steps (`Import`, `Update`), then `GroupStack`
//!   steps (ascending by stack id), then the album steps;
//! - `GroupStack.gids`: the members (top first, Lightroom order) that have a
//!   global id in the input, each once;
//! - a stack gets a step only if it is new or its `gids` differ from the
//!   recorded `members` (list or order); an equal one counts as known;
//! - fewer than two usable members: no step, a skip;
//! - removed members never lead to a remove, ungroup or split of any kind;
//!   they are only counted;
//! - script: before each stack a `library.select` with the placeholder ids
//!   `lr:<global id>` (or the real LightCraft id of a known photo) and
//!   `active` = the first; then `stack.group` with `ids`, `top` = the
//!   first, `collapsed: false`.

#![cfg(any())]

use lc_writer::plan::{Plan, Step, WriteInput, plan};
use lc_writer::state::{PhotoFields, State};
use model::{Collection, CollectionKind, Photo, Rating, Stack};
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn stack(id: i64, members: &[i64]) -> Stack {
    Stack {
        lightroom_id: id,
        members: members.to_vec(),
        collapsed: false,
    }
}

/// Photos with Lightroom ids 101.. and global ids `g101`..
fn photos(n: i64) -> Vec<Photo> {
    (101..101 + n)
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

fn input(stacks: Vec<Stack>, photo_count: i64) -> WriteInput {
    WriteInput {
        photos: photos(photo_count),
        stacks,
        ..WriteInput::default()
    }
}

/// The state from a mapping file text with the given `stacks` and with the
/// photos `g101..` recorded as LightCraft ids 1001...
fn state_with(stacks: Value, photo_count: i64) -> State {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("x.lr2lc.json");
    let text = json!({"version": 1, "photos": {}, "keywords": {}, "stacks": stacks});
    std::fs::write(&file, text.to_string()).unwrap();
    let mut state = State::load(&file).unwrap();
    for i in 0..photo_count {
        state.record(
            format!("g{}", 101 + i),
            101 + i,
            json!(1001 + i),
            PhotoFields::imported_default(),
        );
    }
    state
}

fn run(input: &WriteInput, state: &State) -> Plan {
    plan(input, state, &BTreeSet::new())
}

fn groups(p: &Plan) -> Vec<(i64, Vec<&str>)> {
    p.steps
        .iter()
        .filter_map(|s| match s {
            Step::GroupStack { stack_id, gids } => {
                Some((*stack_id, gids.iter().map(String::as_str).collect()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn new_stacks_are_grouped_top_first_in_lightroom_order() {
    let p = run(
        &input(vec![stack(2, &[104, 103]), stack(1, &[102, 101, 105])], 5),
        &State::default(),
    );
    // Ascending by stack id.
    assert_eq!(
        groups(&p),
        [(1, vec!["g102", "g101", "g105"]), (2, vec!["g104", "g103"])]
    );
    assert_eq!(p.stacks_known(), 0);
    assert!(p.stack_skips().is_empty());
    assert_eq!(p.stack_members_removed(), 0);
}

#[test]
fn group_steps_come_after_photo_steps_and_before_album_steps() {
    let mut i = input(vec![stack(1, &[101, 102])], 2);
    i.collections = vec![Collection {
        lightroom_id: 9,
        name: "A".into(),
        kind: CollectionKind::Album,
        parent: None,
        members: vec![101],
    }];
    let p = run(&i, &State::default());
    let last_photo = p
        .steps
        .iter()
        .rposition(|s| matches!(s, Step::Import { .. } | Step::Update { .. }))
        .unwrap();
    let first_group = p
        .steps
        .iter()
        .position(|s| matches!(s, Step::GroupStack { .. }))
        .unwrap();
    let last_group = p
        .steps
        .iter()
        .rposition(|s| matches!(s, Step::GroupStack { .. }))
        .unwrap();
    let first_album = p
        .steps
        .iter()
        .position(|s| matches!(s, Step::CreateAlbum { .. } | Step::AddMembers { .. }))
        .unwrap();
    assert!(last_photo < first_group);
    assert!(last_group < first_album);
}

#[test]
fn an_unchanged_stack_gets_no_step() {
    let state = state_with(json!({"1": {"members": ["g102", "g101"]}}), 2);
    let p = run(&input(vec![stack(1, &[102, 101])], 2), &state);
    assert!(groups(&p).is_empty(), "{:?}", p.steps);
    assert!(p.steps.is_empty(), "{:?}", p.steps);
    assert_eq!(p.stacks_known(), 1);
}

#[test]
fn a_changed_order_gives_one_new_group() {
    let state = state_with(json!({"1": {"members": ["g101", "g102", "g103"]}}), 3);
    let p = run(&input(vec![stack(1, &[103, 101, 102])], 3), &state);
    assert_eq!(groups(&p), [(1, vec!["g103", "g101", "g102"])]);
    assert_eq!(p.stacks_known(), 0);
    assert_eq!(p.stack_members_removed(), 0);
}

#[test]
fn a_new_member_gives_one_new_group_with_the_whole_list() {
    let state = state_with(json!({"1": {"members": ["g101", "g102"]}}), 3);
    let p = run(&input(vec![stack(1, &[101, 102, 103])], 3), &state);
    assert_eq!(groups(&p), [(1, vec!["g101", "g102", "g103"])]);
}

#[test]
fn removed_members_are_counted_and_never_removed() {
    let state = state_with(json!({"1": {"members": ["g101", "g102", "g103"]}}), 3);
    let p = run(&input(vec![stack(1, &[101, 102])], 3), &state);
    assert_eq!(p.stack_members_removed(), 1);
    // Whatever the plan does, it has no way to take a photo out of a stack:
    // the only stack step type is GroupStack.
    for (_, gids) in groups(&p) {
        assert_eq!(gids, ["g101", "g102"]);
    }
}

#[test]
fn a_member_gone_from_lightroom_altogether_is_counted_as_removed() {
    // g103 is no longer in the input photos at all.
    let state = state_with(json!({"1": {"members": ["g101", "g102", "g103"]}}), 3);
    let p = run(&input(vec![stack(1, &[101, 102, 103])], 2), &state);
    assert_eq!(p.stack_members_removed(), 1);
}

#[test]
fn a_recorded_stack_missing_from_the_input_is_left_alone() {
    let state = state_with(json!({"1": {"members": ["g101", "g102"]}}), 2);
    let p = run(&input(Vec::new(), 2), &state);
    assert!(p.steps.is_empty(), "{:?}", p.steps);
    assert_eq!(p.stacks_known(), 0);
}

#[test]
fn members_without_a_global_id_or_photo_fall_away() {
    let mut ph = photos(3);
    ph.push(Photo::new(
        "/Users/example/Pictures/x.jpg",
        777,
        Rating::default(),
    ));
    let i = WriteInput {
        photos: ph,
        stacks: vec![stack(1, &[101, 777, 888, 102])],
        ..WriteInput::default()
    };
    let p = run(&i, &State::default());
    assert_eq!(groups(&p), [(1, vec!["g101", "g102"])]);
    assert!(p.stack_skips().is_empty());
}

#[test]
fn fewer_than_two_usable_members_is_a_skip_with_the_stack_id() {
    let i = input(vec![stack(41, &[101, 888]), stack(42, &[888, 889])], 2);
    let p = run(&i, &State::default());
    assert!(groups(&p).is_empty());
    let reasons: Vec<String> = p
        .stack_skips()
        .iter()
        .map(|s| s.reason.to_lowercase())
        .collect();
    assert_eq!(reasons.len(), 2, "{reasons:?}");
    for (r, id) in reasons.iter().zip(["41", "42"]) {
        assert!(r.contains("fewer than two"), "{r}");
        assert!(
            r.split(|c: char| !c.is_ascii_digit()).any(|t| t == id),
            "{r}"
        );
    }
}

#[test]
fn a_photo_listed_twice_counts_once() {
    let p = run(
        &input(vec![stack(1, &[101, 102, 101])], 2),
        &State::default(),
    );
    assert_eq!(groups(&p), [(1, vec!["g101", "g102"])]);
    let p = run(&input(vec![stack(1, &[101, 101])], 2), &State::default());
    assert!(groups(&p).is_empty());
    assert_eq!(p.stack_skips().len(), 1);
}

#[test]
fn a_state_file_without_stacks_still_loads() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("x.lr2lc.json");
    std::fs::write(
        &file,
        json!({"version": 1, "photos": {}, "keywords": {}}).to_string(),
    )
    .unwrap();
    let state = State::load(&file).unwrap();
    let p = run(&input(vec![stack(1, &[101, 102])], 2), &state);
    assert_eq!(groups(&p).len(), 1);
    assert_eq!(p.stacks_known(), 0);
}

#[test]
fn no_stacks_means_no_stack_steps() {
    let p = run(&input(Vec::new(), 3), &State::default());
    assert!(groups(&p).is_empty());
    assert_eq!(p.stacks_known(), 0);
    assert!(p.stack_skips().is_empty());
    assert_eq!(p.stack_members_removed(), 0);
}

#[test]
fn balance_of_the_plan() {
    // Known 1, new 2 (one changed), skipped 1: every input stack is exactly
    // one of known, grouped or skipped.
    let state = state_with(
        json!({
            "1": {"members": ["g101", "g102"]},
            "2": {"members": ["g103", "g104"]},
        }),
        6,
    );
    let i = input(
        vec![
            stack(1, &[101, 102]),
            stack(2, &[104, 103]),
            stack(3, &[105, 106]),
            stack(4, &[105, 999]),
        ],
        6,
    );
    let p = run(&i, &state);
    assert_eq!(
        i.stacks.len(),
        p.stacks_known() + groups(&p).len() + p.stack_skips().len()
    );
    assert_eq!(
        (p.stacks_known(), groups(&p).len(), p.stack_skips().len()),
        (1, 2, 1)
    );
}

fn script_lines(p: &Plan, state: &State) -> Vec<(String, Value)> {
    p.to_script(state)
        .lines()
        .map(|l| {
            let v: Value = serde_json::from_str(l).unwrap();
            assert_eq!(v["method"], "engine.execute");
            (
                v["params"]["command"].as_str().unwrap().to_string(),
                v["params"]["params"].clone(),
            )
        })
        .collect()
}

#[test]
fn script_has_select_then_group_with_placeholders_and_collapsed_false() {
    let state = State::default();
    let p = run(&input(vec![stack(1, &[102, 101])], 2), &state);
    let lines = script_lines(&p, &state);
    let names: Vec<&str> = lines.iter().map(|(c, _)| c.as_str()).collect();
    assert_eq!(
        names,
        [
            "library.import",
            "library.import",
            "library.select",
            "stack.group"
        ]
    );
    assert_eq!(
        lines[2].1,
        json!({"ids": ["lr:g102", "lr:g101"], "active": "lr:g102"})
    );
    assert_eq!(
        lines[3].1,
        json!({"ids": ["lr:g102", "lr:g101"], "top": "lr:g102", "collapsed": false})
    );
}

#[test]
fn script_uses_real_ids_for_known_photos() {
    let state = state_with(json!({}), 2);
    let p = run(&input(vec![stack(1, &[102, 101])], 2), &state);
    let lines = script_lines(&p, &state);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert_eq!(lines[0].0, "library.select");
    assert_eq!(lines[0].1, json!({"ids": [1002, 1001], "active": 1002}));
    assert_eq!(
        lines[1].1,
        json!({"ids": [1002, 1001], "top": 1002, "collapsed": false})
    );
}

#[test]
fn script_of_a_known_unchanged_stack_has_no_stack_command() {
    let state = state_with(json!({"1": {"members": ["g102", "g101"]}}), 2);
    let p = run(&input(vec![stack(1, &[102, 101])], 2), &state);
    assert!(
        script_lines(&p, &state)
            .iter()
            .all(|(c, _)| !c.starts_with("stack."))
    );
}

#[test]
fn script_never_contains_remove_ungroup_or_split() {
    let state = state_with(json!({"1": {"members": ["g101", "g102", "g103"]}}), 3);
    let p = run(&input(vec![stack(1, &[101, 102])], 3), &state);
    for (c, _) in script_lines(&p, &state) {
        assert!(
            !["stack.remove", "stack.ungroup", "stack.split"].contains(&c.as_str()),
            "{c}"
        );
    }
}
