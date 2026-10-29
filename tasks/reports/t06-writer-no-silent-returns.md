# t06-writer-no-silent-returns report

Status: done (unverified — see Verification)
Branch: glm/t06-writer-no-silent-returns
Last commit: <filled on commit>
Base: origin/main (`7f36890`, "Review GLM task catalog and add large tasks T13 to T23")
Attempt: 1 of 3

## Goal

In `crates/lc-writer/src/apply.rs` three places returned `Ok(())` with a
"not reachable" comment and no skip entry: `create_smart` (the missing input
smart collection), `create_album` (the missing input collection), and the
third early return in `finish_photo` (the photo or its mapping entry missing
after a successful step). Make each visible: a skip entry with a reason when
the case can occur, or an explicit internal error that stops the run with a
clear message when it is a true invariant. Decide per place and write the
decision into a doc comment.

## Decision per place

All three are **true invariants**, not cases that can occur through normal
input or a fake control-port server. They are therefore internal errors that
stop the run, not skip entries (a skip would imply an element was lost from
the catalog, which is not what these guard).

- **`create_smart`** — the plan emits a smart-collection step only for an
  input smart collection, so `collection_id` is always in `self.smart`.
- **`create_album`** — the plan emits a collection step only for an input
  collection, so `collection_id` is always in `self.collections`.
- **`finish_photo` (third early return)** — `current.ok` is set only after a
  successful import or find, which writes the photo into both `self.photos`
  (from the plan) and the mapping file. The first two early returns in
  `finish_photo` (no current photo; current photo already failed) are normal
  no-ops and are left as `Ok(())`.

## What changed

- `crates/lc-writer/src/control.rs` — new `WriteError::Internal { detail:
  String }` variant (`#[error("internal invariant broken: {detail}")]`),
  documented as not reachable through normal input or a fake server; the run
  stops instead of silently returning. `short_reason` returns a fixed text
  ("an internal invariant broke; the run stopped") without the detail, so no
  id leaks into the short reason. The `short_reasons_contain_no_paths...`
  test now also constructs `WriteError::Internal` with a secret detail and
  asserts it does not leak.
- `crates/lc-writer/src/apply.rs` — the three `Ok(())` returns replaced with
  `Err(WriteError::Internal { detail: ... })`, each naming only the id
  (collection id or photo global id). A doc comment on each function states
  the invariant and the decision (true invariant → internal error, not a
  skip). The other early returns in `finish_photo` (no current photo; failed
  photo) stay `Ok(())` and are documented as normal no-ops.

## Acceptance

- **A test for each place if reachable by a fake server; otherwise a note
  why it is not reachable.** None of the three is reachable by a fake server:
  they guard the plan/state invariants, not the app's replies. Forcing them
  would need an `Applier` whose `smart`/`collections`/`photos`/state is
  deliberately inconsistent with the plan it was built from — there is no
  public way to build that, and `create_smart`/`create_album`/`finish_photo`
  are private. The existing fake-server tests cover the normal paths (an
  input collection is created, a photo is finished); the new branches are
  unreachable in those runs.
- **No behaviour change for normal runs.** The three branches are not taken
  on normal input, so the change is invisible to every existing test.

## Verification

- `cargo fmt --check` — clean.
- `cargo check` / `cargo clippy` / `cargo test` — **could not run locally.**
  The workspace pulls build scripts that need a C toolchain (`cc`), which is
  not installed in this seat. CI on this branch is the proof; the seat token
  cannot read Actions runs or PRs from here (403).
- Design verified by reasoning: `WriteError::Internal` is a new enum variant;
  the only exhaustive match on `WriteError` outside `control.rs` is none —
  `crates/lr2lc` maps the error to `anyhow` and does not match exhaustively,
  and the writer tests use it only as a type. `short_reason` and the
  secret-leak test are the two places that enumerate variants; both are
  updated. The `apply_skip_codes` note ("a `WriteError` is not a skip code")
  still holds: `Internal` stops the run, it is not counted as a skip.

## Deviations from the task

- **Internal error, not a skip, for all three.** The task allows either; the
  decision per place is "true invariant → internal error," documented in the
  doc comments. None of the three is a case that "can occur" through input,
  so a skip entry (which would imply lost catalog data) would be the wrong
  signal.
- **No new fake-server test.** The branches are not reachable by a fake
  server; the report states why.

## Questions

- None.

## Risks / things the reviewer should look at first

- **No compile or test run.** CI is the first real proof. Likely failure
  surface: a non-exhaustive match on `WriteError` somewhere I missed (grep
  found only `short_reason` and the test array, both updated), or a
  formatting nit.
- **`WriteError::Internal` is a new public variant.** It widens the public
  error API of `lc-writer`; review whether that is acceptable or whether a
  `#[non_exhaustive]`-style internal channel is preferred.
- **Aborting the run vs. skipping.** If a reviewer believes any of these
  branches could be reached by a subtle plan path (for example a smart
  collection whose input entry was dropped after planning), an internal error
  would stop a run that today silently continues. That is the intended
  change (the task wants the case visible), but it is a behaviour change on
  that edge.
