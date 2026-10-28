//! Acceptance tests for writing stacks (task P5b-5), module
//! `lc_writer::apply`, against the stateful fake LightCraft (which has the
//! real `stack.group` rules: gate `no photos selected`, at least two photos,
//! merging of touched stacks, `collapsed` defaulting to true, equal
//! arguments giving the same state).
//!
//! STATUS: waiting for P5b-5. The whole file is disabled by the crate-level
//! `cfg(any())` below. The developer may ONLY remove that line. Any other
//! change to this file needs the test author.
//!
//! API under test (see also `plan_stacks.rs`):
//! - `WriteInput.stacks`;
//! - `ApplySummary { stacks_grouped, stacks_known, stack_members_removed:
//!   usize; stacks_skipped, stacks_incomplete: Vec<Skipped> }`;
//! - the mapping file entry `stacks.<Lightroom stack id>` = `{members:
//!   [global ids, top first]}`, written as soon as the group succeeded.
//!
//! Rules:
//! - plan order: photos, then stacks (ascending by id), then albums;
//! - per stack: `library.select {ids, active: <top>}` directly before
//!   `stack.group {ids, top: <top>, collapsed: false}`; `collapsed` is always
//!   sent and always `false`, whatever the model says (LightCraft would
//!   collapse by default and F-019 point 9 says not to map the state);
//! - members without a LightCraft id (import failed) are not sent; fewer
//!   than two left: the stack is in `stacks_skipped`, nothing is sent;
//! - no change, no `stack.*` command and no `library.select`; a changed
//!   member list or order is sent once more as one `stack.group`;
//! - members that left a stack in Lightroom are only counted
//!   (`stack_members_removed`); never `stack.remove`, `stack.ungroup`,
//!   `stack.split`;
//! - `select` or `group` rejected: `stacks_incomplete`, nothing recorded for
//!   that stack, the run goes on; a lost connection aborts;
//! - balance: `input.stacks.len() == stacks_grouped + stacks_known +
//!   stacks_skipped.len() + stacks_incomplete.len()`, in every run (without
//!   rejected commands this is the sum of the task description);
//! - reasons name the stack's Lightroom id, never a path.

#![cfg(any())]

use lc_writer::apply::{ApplySummary, apply, apply_partial};
use lc_writer::control::{ControlClient, WriteError};
use lc_writer::plan::WriteInput;
use lc_writer::state::State;
use model::{Photo, Rating, Stack};
use serde_json::{Value, json};
use std::path::PathBuf;

#[path = "support/fake_lightcraft.rs"]
mod fake_lightcraft;
use fake_lightcraft::{Config, Failure, FakeLightCraft, StackView};

fn stack(id: i64, members: &[i64]) -> Stack {
    Stack {
        lightroom_id: id,
        members: members.to_vec(),
        collapsed: false,
    }
}

/// Photos 101..=106 (global ids g101..g106, no rating, so imports need no
/// updates and no selects), imported as 1001..1006.
fn photos() -> Vec<Photo> {
    (101..=106)
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

fn input_with(stacks: Vec<Stack>) -> WriteInput {
    WriteInput {
        photos: photos(),
        stacks,
        ..WriteInput::default()
    }
}

struct Env {
    _dir: tempfile::TempDir,
    library: PathBuf,
    state_file: PathBuf,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("Test.lightcraft");
    let state_file = lc_writer::state::state_path(&library);
    Env {
        _dir: dir,
        library,
        state_file,
    }
}

type Commands = Vec<(String, Value)>;

fn run(
    fake: &FakeLightCraft,
    env: &Env,
    input: &WriteInput,
) -> (Result<ApplySummary, WriteError>, Commands) {
    let before = fake.commands().len();
    let mut state = State::load(&env.state_file).unwrap();
    let mut client = ControlClient::connect(fake.port()).unwrap();
    let result = apply(&mut client, input, &mut state, &env.state_file);
    (result, fake.commands()[before..].to_vec())
}

fn ok(fake: &FakeLightCraft, env: &Env, input: &WriteInput) -> (ApplySummary, Commands) {
    let (result, commands) = run(fake, env, input);
    let summary = result.unwrap();
    assert_balance(input, &summary);
    (summary, commands)
}

fn assert_balance(input: &WriteInput, s: &ApplySummary) {
    assert_eq!(
        input.stacks.len(),
        s.stacks_grouped + s.stacks_known + s.stacks_skipped.len() + s.stacks_incomplete.len(),
        "{s:?}"
    );
}

fn state(env: &Env) -> Value {
    serde_json::from_str(&std::fs::read_to_string(&env.state_file).unwrap()).unwrap()
}

fn of<'a>(commands: &'a Commands, name: &str) -> Vec<&'a Value> {
    commands
        .iter()
        .filter(|(c, _)| c == name)
        .map(|(_, p)| p)
        .collect()
}

fn stack_commands(commands: &Commands) -> usize {
    commands
        .iter()
        .filter(|(c, _)| c.starts_with("stack."))
        .count()
}

/// Every `stack.group` directly follows a `library.select` with exactly its
/// ids and the top one active.
fn assert_selected_before_groups(commands: &Commands) {
    for (i, (command, params)) in commands.iter().enumerate() {
        if command == "stack.group" {
            let (prev, select) = &commands[i - 1];
            assert_eq!(prev, "library.select", "before {command}");
            assert_eq!(select["ids"], params["ids"], "{select} {params}");
            assert_eq!(select["active"], params["top"]);
            assert_eq!(params["top"], params["ids"][0]);
        }
    }
}

fn views(fake: &FakeLightCraft) -> Vec<Vec<i64>> {
    fake.stacks().into_iter().map(|s| s.photos).collect()
}

#[test]
fn a_stack_is_selected_then_grouped_with_the_top_first_and_expanded() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let (s, commands) = ok(&fake, &env, &input_with(vec![stack(1, &[103, 101, 102])]));
    assert_eq!(s.stacks_grouped, 1);
    assert_eq!((s.stacks_known, s.stack_members_removed), (0, 0));
    assert!(s.stacks_skipped.is_empty() && s.stacks_incomplete.is_empty());
    assert_eq!(of(&commands, "stack.group").len(), 1);
    assert_eq!(
        of(&commands, "stack.group")[0],
        &json!({"ids": [1003, 1001, 1002], "top": 1003, "collapsed": false})
    );
    assert_selected_before_groups(&commands);
    assert_eq!(
        fake.stacks(),
        [StackView {
            id: fake.stacks()[0].id,
            photos: vec![1003, 1001, 1002],
            collapsed: false,
        }]
    );
    assert_eq!(
        state(&env)["stacks"]["1"],
        json!({"members": ["g103", "g101", "g102"]})
    );
}

#[test]
fn collapsed_false_is_sent_even_if_the_model_says_collapsed() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let mut st = stack(1, &[101, 102]);
    st.collapsed = true;
    let (_, commands) = ok(&fake, &env, &input_with(vec![st]));
    for params in of(&commands, "stack.group") {
        assert_eq!(params["collapsed"], json!(false), "{params}");
    }
    assert!(!fake.stacks()[0].collapsed);
}

#[test]
fn several_stacks_are_grouped_in_ascending_order_each_after_its_select() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let (s, commands) = ok(
        &fake,
        &env,
        &input_with(vec![stack(2, &[105, 104]), stack(1, &[102, 101, 103])]),
    );
    assert_eq!(s.stacks_grouped, 2);
    assert_selected_before_groups(&commands);
    let groups = of(&commands, "stack.group");
    assert_eq!(groups[0]["top"], 1002);
    assert_eq!(groups[1]["top"], 1005);
    assert_eq!(views(&fake), [vec![1002, 1001, 1003], vec![1005, 1004]]);
}

#[test]
fn a_second_run_sends_no_stack_command_and_no_select() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let input = input_with(vec![stack(1, &[103, 101, 102]), stack(2, &[105, 104])]);
    ok(&fake, &env, &input);
    let before = fake.stacks();
    let (s, commands) = ok(&fake, &env, &input);
    assert_eq!(stack_commands(&commands), 0, "{commands:?}");
    assert!(of(&commands, "library.select").is_empty(), "{commands:?}");
    assert_eq!((s.stacks_grouped, s.stacks_known), (0, 2));
    assert_eq!(fake.stacks(), before);
}

#[test]
fn a_changed_order_sends_exactly_one_new_group() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    ok(
        &fake,
        &env,
        &input_with(vec![stack(1, &[101, 102, 103]), stack(2, &[105, 104])]),
    );
    let id = fake.stacks()[0].id;
    let (s, commands) = ok(
        &fake,
        &env,
        &input_with(vec![stack(1, &[103, 101, 102]), stack(2, &[105, 104])]),
    );
    assert_eq!(of(&commands, "stack.group").len(), 1);
    assert_eq!(
        of(&commands, "stack.group")[0],
        &json!({"ids": [1003, 1001, 1002], "top": 1003, "collapsed": false})
    );
    assert_selected_before_groups(&commands);
    assert_eq!((s.stacks_grouped, s.stacks_known), (1, 1));
    // Same stack, not a second one, with the new order.
    assert_eq!(fake.stacks().len(), 2);
    assert_eq!(fake.stacks()[0].id, id);
    assert_eq!(fake.stacks()[0].photos, [1003, 1001, 1002]);
    assert_eq!(
        state(&env)["stacks"]["1"],
        json!({"members": ["g103", "g101", "g102"]})
    );
    // And now it is stable.
    let (_, commands) = ok(
        &fake,
        &env,
        &input_with(vec![stack(1, &[103, 101, 102]), stack(2, &[105, 104])]),
    );
    assert_eq!(stack_commands(&commands), 0);
}

#[test]
fn a_new_member_merges_into_the_existing_stack() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    ok(&fake, &env, &input_with(vec![stack(1, &[101, 102])]));
    let id = fake.stacks()[0].id;
    let (s, commands) = ok(&fake, &env, &input_with(vec![stack(1, &[101, 102, 103])]));
    assert_eq!(s.stacks_grouped, 1);
    assert_eq!(of(&commands, "stack.group").len(), 1);
    assert_eq!(fake.stacks().len(), 1);
    assert_eq!(fake.stacks()[0].id, id);
    assert_eq!(fake.stacks()[0].photos, [1001, 1002, 1003]);
}

#[test]
fn a_removed_member_is_only_counted() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    ok(&fake, &env, &input_with(vec![stack(1, &[101, 102, 103])]));
    let (s, commands) = ok(&fake, &env, &input_with(vec![stack(1, &[101, 102])]));
    assert_eq!(s.stack_members_removed, 1);
    for (c, _) in &commands {
        assert!(
            !["stack.remove", "stack.ungroup", "stack.split"].contains(&c.as_str()),
            "{c}"
        );
    }
    // The photo stays in the LightCraft stack.
    assert_eq!(fake.stacks().len(), 1);
    assert!(fake.stacks()[0].photos.contains(&1003));
    // Counted again until Lightroom and LightCraft agree; no failure.
    assert!(s.stacks_incomplete.is_empty() && s.stacks_skipped.is_empty());
}

#[test]
fn a_stack_gone_from_lightroom_is_left_alone() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    ok(&fake, &env, &input_with(vec![stack(1, &[101, 102])]));
    let (_, commands) = ok(&fake, &env, &input_with(Vec::new()));
    assert_eq!(stack_commands(&commands), 0);
    assert_eq!(views(&fake), [vec![1001, 1002]]);
}

#[test]
fn a_member_without_a_lightcraft_id_is_left_out() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.import_failed_for = Some("103.jpg".into());
    let fake = FakeLightCraft::start(config);
    let (s, commands) = ok(&fake, &env, &input_with(vec![stack(1, &[103, 101, 102])]));
    assert_eq!(s.stacks_grouped, 1);
    assert_eq!(
        of(&commands, "stack.group")[0],
        &json!({"ids": [1001, 1002], "top": 1001, "collapsed": false})
    );
    assert_eq!(views(&fake), [vec![1001, 1002]]);
    assert_eq!(
        state(&env)["stacks"]["1"],
        json!({"members": ["g101", "g102"]})
    );
}

#[test]
fn fewer_than_two_members_with_a_lightcraft_id_is_skipped() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.import_failed_for = Some("103.jpg".into());
    let fake = FakeLightCraft::start(config);
    let input = input_with(vec![stack(77, &[103, 101]), stack(78, &[104, 105])]);
    let (s, commands) = ok(&fake, &env, &input);
    assert_eq!((s.stacks_grouped, s.stacks_skipped.len()), (1, 1));
    assert_eq!(of(&commands, "stack.group").len(), 1);
    assert_eq!(views(&fake), [vec![1003, 1004]]);
    let reason = &s.stacks_skipped[0].reason;
    assert!(
        reason
            .split(|c: char| !c.is_ascii_digit())
            .any(|t| t == "77"),
        "{reason}"
    );
    assert!(!reason.contains("jpg") && !reason.contains('/'), "{reason}");
    assert!(state(&env)["stacks"].get("77").is_none());
    assert!(state(&env)["stacks"].get("78").is_some());
}

#[test]
fn a_rejected_group_is_incomplete_and_the_run_goes_on() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.failures.push(Failure {
        command: "stack.group".into(),
        photo_id: Some(1001),
        message: "boom".into(),
    });
    let fake = FakeLightCraft::start(config);
    let input = input_with(vec![stack(1, &[101, 102]), stack(2, &[104, 103])]);
    let (s, commands) = ok(&fake, &env, &input);
    assert_eq!((s.stacks_grouped, s.stacks_incomplete.len()), (1, 1));
    assert_eq!(of(&commands, "stack.group").len(), 2);
    assert_eq!(views(&fake), [vec![1004, 1003]]);
    let st = state(&env);
    assert!(st["stacks"].get("1").is_none(), "nothing recorded: {st}");
    assert!(st["stacks"].get("2").is_some());
    let reason = &s.stacks_incomplete[0].reason;
    assert!(
        reason
            .split(|c: char| !c.is_ascii_digit())
            .any(|t| t == "1"),
        "{reason}"
    );
}

#[test]
fn a_rejected_select_sends_no_group_for_that_stack() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.failures.push(Failure {
        command: "library.select".into(),
        photo_id: Some(1001),
        message: "boom".into(),
    });
    let fake = FakeLightCraft::start(config);
    let input = input_with(vec![stack(1, &[101, 102]), stack(2, &[104, 103])]);
    let (s, commands) = ok(&fake, &env, &input);
    assert_eq!((s.stacks_grouped, s.stacks_incomplete.len()), (1, 1));
    assert_eq!(of(&commands, "stack.group").len(), 1);
    assert_eq!(views(&fake), [vec![1004, 1003]]);
    assert!(state(&env)["stacks"].get("1").is_none());
}

#[test]
fn an_incomplete_stack_is_sent_again_by_the_next_run() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.failures.push(Failure {
        command: "stack.group".into(),
        photo_id: Some(1001),
        message: "boom".into(),
    });
    let fake = FakeLightCraft::start(config);
    let input = input_with(vec![stack(1, &[101, 102])]);
    let (first, _) = ok(&fake, &env, &input);
    assert_eq!(first.stacks_incomplete.len(), 1);
    // The fake keeps failing for photo 1001, so the stack is incomplete
    // again, and it is sent again: one command per run, no recorded state.
    let (second, commands) = ok(&fake, &env, &input);
    assert_eq!(second.stacks_incomplete.len(), 1);
    assert_eq!(of(&commands, "stack.group").len(), 1);
    assert!(fake.stacks().is_empty());
}

#[test]
fn lost_connection_at_the_second_group_repeats_nothing_after_the_next_run() {
    let env = env();
    let mut config = Config::new(&env.library);
    // 1 keyword.list, 2-5 imports, 6 select, 7 first group, 8 select, 9 the
    // second group.
    config.close_at = Some(9);
    let fake = FakeLightCraft::start(config);
    let input = input_with(vec![stack(1, &[101, 102]), stack(2, &[104, 103])]);
    let mut state_in = State::load(&env.state_file).unwrap();
    let mut client = ControlClient::connect(fake.port()).unwrap();
    let aborted = apply_partial(&mut client, &input, &mut state_in, &env.state_file).unwrap_err();
    assert!(
        matches!(&aborted.error, WriteError::ConnectionLost { command } if command == "stack.group"),
        "{:?}",
        aborted.error
    );
    assert_eq!(aborted.summary.stacks_grouped, 1);
    let st = state(&env);
    assert!(st["stacks"].get("1").is_some());
    assert!(st["stacks"].get("2").is_none());
    assert_eq!(views(&fake), [vec![1001, 1002]]);

    let (s, commands) = ok(&fake, &env, &input);
    // Only the missing stack is sent.
    assert_eq!(of(&commands, "stack.group").len(), 1);
    assert_eq!(of(&commands, "stack.group")[0]["top"], 1004);
    assert_eq!((s.stacks_grouped, s.stacks_known), (1, 1));
    assert_eq!(views(&fake), [vec![1001, 1002], vec![1004, 1003]]);
}

#[test]
fn lost_connection_at_a_select_makes_no_duplicate() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.close_at = Some(8);
    let fake = FakeLightCraft::start(config);
    let input = input_with(vec![stack(1, &[101, 102]), stack(2, &[104, 103])]);
    let (result, _) = run(&fake, &env, &input);
    assert!(matches!(result, Err(WriteError::ConnectionLost { .. })));
    let (s, commands) = ok(&fake, &env, &input);
    assert_eq!((s.stacks_grouped, s.stacks_known), (1, 1));
    assert_eq!(of(&commands, "stack.group").len(), 1);
    assert_eq!(views(&fake), [vec![1001, 1002], vec![1004, 1003]]);
}

#[test]
fn a_group_that_took_effect_without_an_answer_is_repeated_without_harm() {
    let env = env();
    let mut config = Config::new(&env.library);
    config.close_at = Some(7);
    config.close_applies = true;
    let fake = FakeLightCraft::start(config);
    let input = input_with(vec![stack(1, &[101, 102])]);
    let (result, _) = run(&fake, &env, &input);
    assert!(matches!(
        result,
        Err(WriteError::ConnectionLost { ref command }) if command == "stack.group"
    ));
    assert_eq!(views(&fake), [vec![1001, 1002]]);
    let (s, _) = ok(&fake, &env, &input);
    assert_eq!(s.stacks_grouped, 1);
    // Equal arguments give the same state: still one stack.
    assert_eq!(views(&fake), [vec![1001, 1002]]);
    assert!(!fake.stacks()[0].collapsed);
}

#[test]
fn no_stacks_means_no_stack_commands() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let (s, commands) = ok(&fake, &env, &input_with(Vec::new()));
    assert_eq!(stack_commands(&commands), 0);
    assert_eq!(
        (s.stacks_grouped, s.stacks_known, s.stack_members_removed),
        (0, 0, 0)
    );
    assert!(fake.stacks().is_empty());
}

#[test]
fn stacks_are_written_after_all_photos() {
    let env = env();
    let fake = FakeLightCraft::start(Config::new(&env.library));
    let (_, commands) = ok(&fake, &env, &input_with(vec![stack(1, &[101, 102])]));
    let last_import = commands
        .iter()
        .rposition(|(c, _)| c == "library.import")
        .unwrap();
    let first_stack = commands
        .iter()
        .position(|(c, _)| c == "stack.group")
        .unwrap();
    assert!(last_import < first_stack);
}

#[test]
fn the_selection_gate_makes_the_select_necessary() {
    // With the gate on (the default of the fake) a group without a select
    // fails, so the successful runs above prove the select. Here the gate is
    // off: the result must be the same, select or not.
    let env = env();
    let mut config = Config::new(&env.library);
    config.selection_gate = false;
    let fake = FakeLightCraft::start(config);
    let (s, commands) = ok(&fake, &env, &input_with(vec![stack(1, &[101, 102])]));
    assert_eq!(s.stacks_grouped, 1);
    assert_selected_before_groups(&commands);
}
