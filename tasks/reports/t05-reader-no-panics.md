# t05-reader-no-panics report

Status: done (unverified — see Verification)
Branch: glm/t05-reader-no-panics
Last commit: <filled on commit>
Base: origin/main (`7f36890`, "Review GLM task catalog and add large tasks T13 to T23")
Attempt: 1 of 3

## Goal

`crates/lr-reader/src/keywords.rs` called `.expect(...)` twice in
`resolve_keywords` (around lines 357 and 376). A broken internal invariant
would abort the tool instead of skipping with a reason. Replace both with a
handled outcome: the affected keyword is skipped with a reason code and an
id, counted so read = converted + skipped still holds.

## What changed

- `crates/model/src/skip.rs` — new `SkipCode::KeywordUnresolved`
  (`"keyword_unresolved"`), documented as a defensive skip emitted only when
  the resolution invariant breaks; added to `ALL` (length 55 → 56) and
  `as_str`.
- `crates/lr-reader/src/keywords.rs` — both `.expect(...)` calls in
  `resolve_keywords` replaced:
  - The `ids.remove(&current.id).expect(...)` (the global-id result) is now a
    `match`: `Some(id_result) => id_result.and_then(...)` (unchanged for
    normal input), `None => Err(Skipped::new(KeywordUnresolved, ...))`.
  - The `done.remove(&r.id).expect(...)` (the resolution entry) is now
    `unwrap_or_else(|| Err(Skipped::new(KeywordUnresolved, ...)))`.
  Both defensive branches name the keyword id in the reason. The existing
  `Some(Err(...))` path (duplicate or missing global id) is unchanged, so
  the existing skip codes (`missing_global_id`, `duplicate_global_id`,
  `parent_skipped`, …) keep their meaning and counts.
- `crates/lr-reader/src/keywords.rs` (tests) — new test
  `every_non_root_row_is_resolved_or_skipped` pins the invariant the
  defensive branches guard: every non-root row produces exactly one entry
  (Ok or Err) in row order, so the defensive branch is not reached by normal
  input. A missing parent and a two-node cycle are both handled as skips,
  not panics.

## Cycle handling (the task's open question)

A cycle in the keyword parent chain **is** handled, and already tested:
`cycles_are_skipped_without_looping` walks the chain until it revisits a node
on the current chain (`on_chain.contains(&p)` → `Top::Broken(p)`), then skips
the whole subtree with `ParentSkipped`. The `expect` sites were not reachable
through a cyclic or broken parent chain.

## Acceptance

- **No `expect`/`unwrap` on catalog-derived data in the reader's keyword
  resolver.** Both expects in `resolve_keywords` are gone.
- **Remaining panic-prone calls in the reader, with justification.** Product
  code only (test code is out of scope for T05; `unwrap_or*` is not a panic):
  - `collections.rs:149` — `unreachable!("own skips are part of the status")`.
    A collection's own skip is recorded in its status before the member
    check runs, so the `(own-err, member-ok)` combination cannot arise from
    catalog data. Internal invariant of the collection status machine.
  - `collections.rs:176` — `unreachable!("quick target without quick")`.
    `Target::Quick` is only constructed for the Quick Collection row, which
    also sets `data.quick`; the two are built together. Internal invariant.
  - `stacks.rs:196` — `unreachable!("rows without a photo id have a reason")`.
    `reason` is `Some(...)` whenever `m.image` is `None` (the first match
    arm), so `(reason None, image None)` cannot arise. Internal invariant.
  - `smart.rs:162/237/286` — `self.expect(byte)` is a parser method
    (`fn expect`), not a panic; it returns `Result<_, SmartParseError>`.
  - Every other `unwrap()`/`expect()`/`panic!` in `crates/lr-reader/src/` is
    inside `#[cfg(test)]` modules (catalog, collections, originals, photos,
    smart, xmp, keywords tests). Test code is out of scope.
- **New skip code documented** — `KeywordUnresolved` (`"keyword_unresolved"`)
  in `skip.rs` with a variant doc comment.
- **read = converted + skipped still holds** — the defensive branches emit a
  `Skipped` (counted), never silently drop a row.

## Verification

- `cargo fmt --check` — clean.
- `cargo check` / `cargo clippy` / `cargo test` — **could not run locally.**
  The workspace pulls build scripts that need a C toolchain (`cc`), which is
  not installed in this seat. CI on this branch is the proof; the seat token
  cannot read Actions runs or PRs from here (403).
- Design verified by reasoning: the `Some` arm of the new `match` is the
  exact previous `.and_then` body, so existing behaviour and skip codes are
  unchanged; the `None` arm is the only new path and it is unreachable in
  normal input (pinned by the new test). `SkipCode::ALL` length and `as_str`
  match the new variant; the `codes_are_unique_snake_case` test covers
  uniqueness.

## Deviations from the task

- **No fixture-based test that triggers the defensive skip.** The case is
  not reachable through `resolve_keywords` with any `KeywordRow` input:
  `global_ids` populates an entry for every non-root row, and the loop
  inserts every non-root row into `done`. A test that forced the branch would
  need `resolve_keywords` refactored to accept injectable maps (a signature
  change with ripple I will not make blind). Instead the new test pins the
  invariant the branch guards, and the report states the unreachability.
- **The three `unreachable!` calls are left in place** with justifications
  above; they are true internal invariants, not catalog-derived panics. T16
  (hostile catalogs) and T21 (audit) can revisit them.

## Questions

- None.

## Risks / things the reviewer should look at first

- **No compile or test run.** CI is the first real proof. Likely failure
  surface is small: a typo in the new `match`/`unwrap_or_else`, or the
  `SkipCode::ALL` length not matching the count.
- **`KeywordUnresolved` is unreachable in normal input.** If the reviewer
  prefers a reachable test, `resolve_keywords` would need to take the `ids`
  map as a parameter so a test can pass a broken one; that is a deliberate
  signature change left for the owner.
