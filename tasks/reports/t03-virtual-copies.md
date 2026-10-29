# t03-virtual-copies report

Status: done (unverified — see Verification)
Branch: glm/t03-virtual-copies
Last commit: <filled on commit>
Base: 5aa462d (glm/t13-test-support, "Add report for t13-test-support")
Attempt: 1 of 3

## Goal

A virtual copy shares its master's file but has its own stars, colour label,
pick/reject flag and keywords. LightCraft's `photo.virtualCopy {ids: [master id]}`
is not idempotent, so the writer creates each copy exactly once and records it
in the mapping file with the master's global id. A copy whose master is unknown
(master was skipped at read time, or its import failed so it has no LightCraft
id) is skipped with the `virtual_copy` code and never created blindly.

## What changed

- `crates/model/src/lib.rs` — `Photo` gains `pub master: Option<i64>` (the
  Lightroom id of the master, `None` for a master). `Photo::new` defaults it to
  `None`; new builder `pub fn with_master(mut self, master: i64) -> Self`. No
  external struct literals construct `Photo` (all callers use `Photo::new`), so
  there is no ripple.
- `crates/model/src/skip.rs` — `SkipCode::VirtualCopy` already existed with code
  `"virtual_copy"` and is in `ALL`; added a variant doc comment stating it is
  now emitted by both the reader (master not converted) and the writer (master
  has no LightCraft id yet), never created blindly.
- `crates/lr-reader/src/photos.rs` — `photo_from_row` signature gains
  `converted: &mut HashSet<i64>`. Master check first: if `row.master` is
  `Some(m)` and `m` is not in `converted`, the copy is skipped with
  `SkipCode::VirtualCopy` ("virtual copy of photo {master} cannot be created,
  the master was not converted"). Otherwise the photo is built with
  `with_master(m)` and its id is inserted into `converted` alongside the
  `used` set. Caller passes a fresh `converted` set. ~20 test call sites
  updated. New reader test
  `virtual_copy_of_a_converted_master_is_converted_with_master`.
- `crates/lc-writer/src/state.rs` — `PhotoEntry` gains
  `#[serde(default, skip_serializing_if = "Option::is_none")] pub copy_of:
  Option<String>`. `record()` preserves an existing `copy_of` across a field
  refresh (read before insert). New `record_copy(gid, lr_id, id,
  master_global_id, fields)` sets `copy_of: Some(master_global_id)`.
- `crates/lc-writer/src/plan.rs` — new `Step::VirtualCopy { global_id,
  master_global_id }`. `plan_photos` builds `master_gid: BTreeMap<i64, &str>`
  (Lightroom id → global id) and sorts masters (`master == None`) before copies
  so the master's import writes its LightCraft id before the copy step reads it.
  A copy whose master is absent from the input is skipped at plan time with
  `SkipCode::VirtualCopy`. `to_script` emits `photo.virtualCopy {ids:
  [master_id]}` from the master's recorded LightCraft id.
- `crates/lc-writer/src/apply.rs` — `step()` handles `Step::VirtualCopy`
  (finish_photo, set current, call `virtual_copy`). New method
  `virtual_copy(global_id, master_global_id)`: if the copy is already in state,
  count existing; if the master has no LightCraft id in state, skip
  `VirtualCopy` and fail current; otherwise send `photo.virtualCopy`, and on
  `Outcome::Applied` extract the new id (`virtual_copy_id`), record the copy via
  `record_copy`, save state. `Outcome::AppliedNotSaved` and
  `WriteError::Command` skip (`AppliedNotSaved` / `Rejected`) and fail current —
  no retry within the run. New `fn virtual_copy_id(result: &Value) ->
  Option<Value>` reads `result["ids"][0]` via `valid_id`, falling back to
  `id`/`photoId` fields.
- `crates/lc-test-support/src/fake_lightcraft.rs` — new `"photo.virtualCopy"`
  arm: reads `ids`, finds each master's path by LightCraft id, mints a new id,
  inserts it under a distinct `{master_path}#vc{id}` key (so the master keeps
  its entry), gates `lib.selection`, returns `{ids: [new_ids]}`. Respects
  `not_saved_creates`. Not idempotent, like the real app.
- `crates/lr2lc/src/writer.rs` — `planned` JSON counting: `Step::VirtualCopy`
  counts as `out.photos += 1`.
- `crates/lc-writer/tests/virtual_copies.rs` (new) — 6 acceptance tests against
  the fake control-port server (see Acceptance).

## Acceptance (6 tests)

1. `first_run_creates_one_copy` — master imported + copy created
   (`photos_imported == 2`), exactly one `photo.virtualCopy` naming the master's
   LightCraft id, `copy.copy_of == Some("gm")`, copy id differs from master id.
2. `second_run_creates_none` — second run imports nothing, sends no
   `photo.virtualCopy` and no `library.import`.
3. `copy_carries_own_stars_label_and_keywords` — copy rated 4, master not rated;
   `photo.label` and `photo.setMeta` sent only for the copy.
4. `master_with_two_copies_creates_both` — `photos_imported == 3`, two
   `photo.virtualCopy` calls, both copies `copy_of == Some("gm")`.
5. `failed_command_is_not_retried` — `Failure` on `photo.virtualCopy` → master
   still imported (`photos_imported == 1`), `rejected` skip, exactly one
   attempt, copy not recorded.
6. `copy_whose_master_has_no_lightcraft_id_is_skipped` — master import failed →
   `photos_imported == 0`, `virtual_copy` skip whose reason contains
   "virtual copy of", no `photo.virtualCopy` sent, copy not recorded.

## Verification

- `cargo fmt --check` — clean.
- `cargo check` / `cargo clippy` / `cargo test --workspace` — **could not run
  locally.** The workspace pulls build scripts that need a C toolchain (`cc`),
  which is not installed in this seat. CI on this branch is the proof; the seat
  token cannot read Actions runs or PRs from here (403).
- Design verified by reasoning: every new code path was checked against the
  existing builder/enum/state shapes (`Photo::new`, `PhotoEntry` literals only
  inside `state.rs`, `valid_id` accepts bare numbers which the fake returns,
  `Outcome`/`WriteError` variants match the import path). `cargo fmt --check`
  clean.
- Ripple minimization (see Deviations) keeps the existing reader/fixture
  numbers intact.

## Deviations from the task

- **Reader-skip when the master is not converted, not "convert all copies".**
  The task says virtual copies carry their own stars/label/flag/keywords and
  share the master's file. Rather than convert every copy (which would change
  the `video_edge`/`stack_edge` fixture balances — copies 20/21 have a missing
  or video master and would flip `CONVERTED` 15→17 / `SKIPPED` 6→4 and break
  ~14 reader tests), a copy is converted only when its master was converted;
  otherwise it stays a reader skip `virtual_copy`. This keeps
  `skip_codes.rs by_code["virtual_copy"] == 2`, `video_edge.CONVERTED == 15 /
  SKIPPED == 6`, and every "virtual copy" substring assertion unchanged. The
  positive path (master converted → copy converted with `master` set) is covered
  by the new reader unit test and the writer acceptance tests.
- **No new fixture-generator catalog.** The generator already supports
  `add_photo(..., master, ...)` and the existing `video_edge` copies exercise the
  skip path; the positive path is covered by synthetic `WriteInput` in the
  writer tests and the reader unit test. Adding a new positive fixture would
  risk the fixture balances for no extra coverage, so it was not done.
- **`record()` signature unchanged; `copy_of` preserved by read-before-insert.**
  A new `record_copy` handles the initial copy record. This avoids touching the
  5 existing `record` callers.
- **Apply-skip reason worded to contain "virtual copy of"** so it maps to the
  `expected_code` table ("virtual copy of" → `virtual_copy`) if cross-checked.

## Questions

- None.

## Risks / things the reviewer should look at first

- **No compile or test run.** CI is the first real proof. If `cargo check`
  fails, the most likely cause is a signature/literal mismatch in
  `photo_from_row` test call sites or a missing import; `cargo fmt --check` is
  clean, so formatting is not the issue.
- **`Step::VirtualCopy` ordering.** Masters are sorted before copies by
  `master.is_some()`. Within each group the order is the `by_gid` BTreeMap order
  (sorted by global id string). If two copies share a master, both are planned
  after the master; the fake mints distinct ids for each. Confirm the real app
  accepts two `photo.virtualCopy` calls for the same master in one run.
- **`virtual_copy_id` shape.** It reads `result["ids"][0]` as a bare id (number
  or string) and falls back to `id`/`photoId` fields. The fake returns
  `{ids: [number]}`. Confirm the real `photo.virtualCopy` result shape matches.
- **`copy_of` is `Option<String>` (global id), not the master's LightCraft id.**
  This is deliberate: the mapping is keyed by global id, and a re-run resolves
  the master's current LightCraft id from state, not from a stale stored value.
