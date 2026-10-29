# t14-applied-not-saved report

Status: done (unverified — see Verification)
Branch: glm/t14-applied-not-saved
Last commit: <filled on commit>
Base: glm/t13-test-support (`5aa462d`, "Add report for t13-test-support")
Attempt: 1 of 3

## Goal

An `AppliedNotSaved` answer from LightCraft means a step was applied in
memory but not written to disk. Today the writer treats it like a success:
it records the step as done (so the next run skips it) and, for field
updates, counts it in `not_saved` only. That loses the step: the next run
never sends it again, and the on-disk library never catches up.

The rule T14 asks for, applied to **field updates** and **`keyword.create`**:

- the step is **not recorded** in the mapping file;
- the report lists it as **not saved** (and the photo as incomplete, or the
  keyword subtree as skipped);
- the **next run sends it again** (field updates are idempotent; a keyword is
  looked up with `keyword.list` before it is created, so no duplicate is
  made).

## What changed

- `crates/lc-writer/src/apply.rs`
  - `update()` — the `Ok(Outcome::AppliedNotSaved(_))` arm no longer counts
    the update as a finished `updates` step. Instead it bumps `not_saved`,
    pushes an `AppliedNotSaved` entry to `photos_incomplete` (so the photo is
    reported incomplete), and calls `fail_current()` so `finish_photo` does
    **not** write the field snapshot. The photo keeps its import-default
    fields and the next run re-plans every field that differs from them.
    `count_updated` still fires once for the photo (the update command was
    sent), but `updates` (the count of *saved* field updates) stays at 0 for
    that photo.
  - `create_keyword()` — the `Ok(Outcome::AppliedNotSaved(_))` arm no longer
    records the keyword. It bumps `not_saved`, inserts the path into
    `failed_keywords` (so the whole subtree is skipped as `ParentFailed`,
    nothing in the unsaved branch is recorded), pushes an `AppliedNotSaved`
    entry to `keywords_skipped`, and returns `Ok(())`. The next run's
    `keyword.list` does not return the keyword, so it is created again.
  - module doc comment — two rules added: an AppliedNotSaved update is not
    recorded (photo incomplete, re-sent next run); an AppliedNotSaved
    `keyword.create` is not recorded and its subtree is skipped (looked up
    before creation next run, so no duplicate).

- `README.md`
  - `not_saved` now reads "a step LightCraft applied but did not save to
    disk; the step is not recorded as done and the next run sends it again".
  - `photos_incomplete` now includes "or a step was applied but not saved".
  - Mapping-file section: a step applied but not saved is not recorded; the
    next run sends it again (field updates idempotent; a keyword is looked up
    with `keyword.list` before it is created, so no duplicate is made).

- `crates/lc-test-support/src/fake_lightcraft.rs`
  - new `Config.not_saved_keyword_creates` flag. When set, `keyword.create`
    answers `saved in memory but not written to disk` **and does not add the
    keyword to the library**, modelling an app that lost the unsaved keyword
    on restart (so the next run's `keyword.list` does not return it). This
    mirrors the existing `not_saved_creates` flag for `album.create` /
    `album.createSmart` / `library.import`. Needed because the fake's
    `keyword.create` arm previously always returned `Ok({})`.

- `crates/lc-writer/tests/apply.rs`
  - replaced `not_saved_replies_count_as_success` (which encoded the old
    "counts as success and is recorded" rule) with two tests:
    - `not_saved_updates_are_not_recorded_and_rerun_resends`: with
      `not_saved_updates = true`, no update is counted in `updates`
      (`updates == 0`), each photo with an update is in `photos_incomplete`
      with code `AppliedNotSaved` (`len == 3`: g1, g3, g4), the photos keep
      `PhotoFields::imported_default()` (the snapshot was not written), and a
      second run sends `photo.*` commands again.
    - `not_saved_keyword_create_is_not_recorded_and_rerun_resends`: with
      `not_saved_keyword_creates = true`, no keyword is created
      (`keywords_created == 0`), `not_saved == 2` (Travel, Café ☕),
      `keywords_skipped.len() == 3` (2 `AppliedNotSaved` + 1 `ParentFailed`
      for Travel|Rome), the mapping file holds no keywords, and a second run
      sends `keyword.create` again.
  - test-module doc comment updated from "AppliedNotSaved counts as success
    (and in not_saved)" to the new rule for updates and keyword.create.

## Acceptance

- **An AppliedNotSaved answer for each kind of step leaves the mapping file
  without that entry, the report lists the step as not saved, and the next
  run sends it again.** Covered by the two tests above for field updates and
  `keyword.create`.
- **Scope: field updates and `keyword.create` only**, as the task says. The
  same record-on-AppliedNotSaved pattern exists in `add_to_quick` (line ~740)
  and `update_stack` (line ~1060), which still call `count_not_saved` and
  then record. These are **out of T14 scope** and left unchanged; listed
  under Risks.

## Verification

- `cargo fmt --check` — clean.
- `cargo check` / `cargo clippy` / `cargo test` — **could not run locally.**
  The workspace pulls build scripts that need a C toolchain (`cc`), which is
  not installed in this seat. CI on this branch is the proof; the seat token
  cannot read Actions runs or PRs from here (403).
- Design verified by reasoning:
  - `update()`'s match on `Outcome` is exhaustive (`Applied` and
    `AppliedNotSaved` are the only `Ok` arms; `Err` has `Command` and a
    catch-all).
  - `fail_current()` is the same mechanism the rejected-update path uses
    (`reject`), so `finish_photo` already honours `current.ok == false` by
    skipping the snapshot (line ~1319).
  - `failed_keywords` already drives `ParentFailed` for a rejected
    `keyword.create`; reusing it for AppliedNotSaved gives the same
    subtree-skip behaviour, so the next run re-plans the whole branch.
  - The fake's `keyword.create` arm, when `not_saved_keyword_creates` is set,
    returns the exact `saved in memory but not written to disk` text that
    `control.rs::parse_reply` maps to `Outcome::AppliedNotSaved` (the same
    text the update and album arms use).

## Deviations from the task

- **Added a fake flag (`not_saved_keyword_creates`).** The task is about the
  writer's rule, but the existing fake could not produce an AppliedNotSaved
  answer for `keyword.create` (the arm always returned `Ok({})`). Without it
  the keyword half of the acceptance ("an AppliedNotSaved answer for each
  kind of step … the next run sends it again") could not be tested end-to-end
  against the stateful fake. The flag mirrors the existing
  `not_saved_creates` and is test-only infrastructure.

## Questions

- None.

## Risks / things the reviewer should look at first

- **No compile or test run.** CI is the first real proof. Likely failure
  surface: a typo in the new test or the fake arm, or an assertion count that
  is off by one. The counts were derived by hand from the step loop
  (`Step::Update` is skipped once `current.ok` is false, so only the first
  update of each photo is sent → `not_saved == 3`, not 5).
- **`add_to_quick` and `update_stack` still record on AppliedNotSaved.**
  Same bug pattern as the one T14 fixes, but outside the task's stated scope
  (field updates + `keyword.create`). They still call `count_not_saved` and
  then record the quick members / the stack as done. A follow-up task should
  apply the same rule there (do not record; re-send next run).
- **`not_saved_keyword_creates` models "lost on restart" by not persisting
  the keyword.** The existing `not_saved_creates` for albums *does* persist
  the album in the fake's memory before answering. The two flags therefore
  differ in their disk model. For the keyword test the non-persisting model
  is what makes "the next run sends it again" provable (the planner keys off
  `keyword.list`); for albums the existing tests apparently did not need
  that. Review whether the inconsistency is acceptable or whether albums
  should be aligned.
