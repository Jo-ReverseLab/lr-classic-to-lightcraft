# T18 — Writer tests: stacks, quick collection, albums

Task: pin the behaviour of the writer (`lc_writer::apply`) for stacks, the
quick collection and albums against the stateful fake LightCraft, which speaks
the real rules of `album.toggleTarget`, `stack.group`, `albums.list` and
`album.addPhotos`. Defects found are fixed in separate commits and noted.

Branch: `glm/t18-writer-album-stack-tests` (base `origin/main`, 7f36890).

## What was added / changed

- `crates/lc-writer/src/plan.rs` — `plan_stacks` now tracks the global ids
  already used by an earlier stack (grouped or already known). A later stack
  whose members overlap that set is reported with `SkipCode::AlreadyInStack`
  and not grouped, instead of emitting a second `stack.group` that would merge
  or steal the shared photo. Stacks are planned in ascending Lightroom id
  order, so the first stack to use a photo wins. The balance still holds
  (`input.stacks.len() == stacks_grouped + stacks_known +
  stacks_skipped.len() + stacks_incomplete.len()`).
- `crates/lc-writer/tests/apply_stacks.rs` —
  `two_stacks_sharing_a_photo_report_the_second_and_write_nothing_half`.
- `crates/lc-writer/tests/apply_quick.rs` —
  `a_member_whose_photo_left_lightcraft_is_incomplete_and_records_nothing`.
- `crates/lc-writer/tests/apply_albums_existing.rs` —
  `a_lightcraft_side_rename_is_not_detected`.

## Defect found and fixed

1. **Two stacks sharing a photo were both grouped (half-written).** A photo
   can be in only one stack, but the plan did not check across stacks, so two
   input stacks that share a photo both produced a `GroupStack` step. The
   fake (like the real app) merges stacks that any member is in: grouping the
   second stack would move the shared photo out of the first, leaving the
   first stack incomplete in LightCraft while the mapping file recorded both.
   Now the later stack is reported (`already_in_stack`) and not sent at all;
   the first stack is grouped in full, so nothing is half-written.

## Per-case verdict

Cases are the ones listed in `tasks/TASKS.md` under T18. "covered" = an
existing test pins the behaviour; "new test" = added here; "note" = the fake
cannot show it, see Open notes.

### Quick collection

| case | verdict | test |
| --- | --- | --- |
| all in | covered | `members_that_lightcraft_already_has_are_never_sent` |
| none in | covered | `first_run_adds_the_members_with_select_and_toggle` |
| some in | covered | `a_new_member_is_the_only_one_sent` |
| empty collection | covered | `no_quick_collection_means_no_quick_command` (empty members) |
| a photo that vanished from LightCraft | new test | `a_member_whose_photo_left_lightcraft_is_incomplete_and_records_nothing` |
| a repeated run | covered | `second_run_sends_no_quick_command` |

The "vanished photo" case is shown by pre-recording the photo in the mapping
file with a LightCraft id the fake library does not have (the user deleted it
between runs). `library.select [id]` is rejected, so `quick_command` reports
`Rejected`, nothing is toggled and nothing is recorded; the next run tries
again (no blind repeat).

### Stacks

| case | verdict | test |
| --- | --- | --- |
| a stack of one | covered | `fewer_than_two_members_with_a_lightcraft_id_is_skipped` |
| a member missing in LightCraft | covered | `a_member_without_a_lightcraft_id_is_left_out` |
| two stacks that share a photo (reported, nothing half-written) | new test + fix | `two_stacks_sharing_a_photo_report_the_second_and_write_nothing_half` |
| a stack already present in LightCraft | covered | `a_new_member_merges_into_the_existing_stack`, `a_second_run_sends_no_stack_command` |
| a second run | covered | `a_second_run_sends_no_stack_command` |

### Albums and album folders

| case | verdict | test |
| --- | --- | --- |
| nested folders several levels deep | covered | `lost_state_file_adopts_every_collection_and_creates_nothing`, `partly_existing_tree_creates_only_the_missing_collections` |
| unknown depth limit of `albums.list` | note | see Open notes |
| same-named albums in one folder (names unchanged, duplicates only counted) | covered | `two_equal_albums_in_the_same_folder_are_reported_not_guessed`, `three_equal_albums_say_how_many` |
| a member that is not in the library | covered | `member_whose_photo_was_not_imported_is_skipped_with_its_id` |
| an empty album | covered | `tree_is_created_with_folder_flags_and_parent_ids` (album "Empty") |
| a second run | covered | `normal_second_run_does_not_read_albums_list`, `second_run_sends_no_album_command_and_no_select` |
| an album the user renamed in LightCraft | new test (pins the gap) | `a_lightcraft_side_rename_is_not_detected` |

### A command that fails in the middle of an album

| case | verdict | test |
| --- | --- | --- |
| what is recorded / what the next run does (no blind repeat) | covered | `rejected_add_makes_the_collection_incomplete_and_the_run_goes_on`, `rejected_create_skips_the_collection_and_its_descendants`, `rejected_select_makes_the_collection_incomplete`, `lost_connection_during_members_loses_nothing_and_adds_nothing_twice` |

## Verification

- `cargo fmt --check` — clean.
- `cargo test` / `cargo clippy -- -D warnings` — **not run**: no local C
  compiler in this workspace; CI is the proof. The new tests and the
  `plan_stacks` change were derived by reading `apply.rs` (`quick_add`,
  `quick_command`, `change_members`), `plan.rs` (`plan_stacks`,
  `plan_collections`), and the fake's `library.select`, `stack.group` and
  `album.addPhotos` handlers, and tracing each case through them.

## Open notes

- **`albums.list` depth limit (case "unknown depth limit").** The fake's
  `albums_nested` builds the full tree with no depth limit, and the writer
  has no way to learn that the real app's `albums.list` cuts deep trees:
  `albums.list` carries no truncation flag (research
  `docs/research/lightcraft-albums.md`). A silently cut tree would make a
  deep existing album invisible to adoption, so the writer would create a
  duplicate instead of reporting it. Detecting this needs an app-side signal
  that does not exist, so the fake cannot show the case without first
  inventing one; reconciliation against the live library tree is an open
  item of the safe re-sync. Noted here per the acceptance rule.
- **LightCraft-side album rename (case "an album the user renamed in
  LightCraft").** The new test pins the current behaviour: the writer trusts
  the recorded LightCraft id, sends no `album.create`, does not re-read
  `albums.list`, and adds members to the id by id. `collections_renamed`
  counts only Lightroom-side renames (input name vs. recorded name), never a
  library-side rename, so a rename done in LightCraft is not detected.
  Reconciling library-side renames is an open item of the safe re-sync; the
  test documents the gap rather than asserting a fix.
- The shared-photo fix is deliberately conservative: the first stack wins
  and the later one is skipped. Lightroom's catalog does not let a photo be
  in two stacks, so two input stacks sharing a photo is a plan/reader
  anomaly; reporting it and writing nothing of the later stack is safer than
  guessing which stack the photo belongs to.
