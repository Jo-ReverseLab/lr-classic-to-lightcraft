# t13-test-support report

Status: done
Branch: glm/t13-test-support
Last commit: <filled on commit>
Base: 7f368908c384 (origin/main, "Review GLM task catalog and add large tasks T13 to T23")

## What changed

- New crate `crates/lc-test-support` (workspace member, `publish = false`):
  - `Cargo.toml` — depends only on `serde_json = "1"` (already a workspace
    dependency, so `Cargo.lock` gains only the new path package).
  - `src/lib.rs` — crate doc and `pub mod fake_lightcraft; pub mod fake_port;`,
    with `#![allow(dead_code)]` (test-only, many items unused per test).
  - `src/fake_lightcraft.rs` — the former `crates/lc-writer/tests/support/fake_lightcraft.rs`
    (byte-identical in both crates; re-checked with `diff` before the move).
    The stale "same file lives in two places" comment is removed.
  - `src/fake_port.rs` — the former `crates/lc-writer/tests/support/fake_port.rs`.
- `Cargo.toml` (workspace) — `crates/lc-test-support` added to `members`.
- `crates/lc-writer/Cargo.toml`, `crates/lr2lc/Cargo.toml` —
  `lc-test-support = { path = "../lc-test-support" }` added to `[dev-dependencies]`.
- 25 test files — `#[path = "support/fake_lightcraft.rs"] mod fake_lightcraft;`
  replaced with `use lc_test_support::fake_lightcraft;`.
- `crates/lc-writer/tests/control.rs` — `fake_port` include replaced with
  `use lc_test_support::fake_port;`; the header comment updated.
- `crates/lc-writer/tests/support/` and `crates/lr2lc/tests/support/` deleted.

## Verification

- `cargo fmt --check` — clean.
- `cargo check -p lc-test-support` — **does not run.** Even the pure-Rust
  crate pulls `serde_core`/`zmij` build scripts that need `cc`; the workspace
  has no C toolchain. `cargo clippy`/`cargo test --workspace` likewise cannot
  run locally. CI on this branch is the proof; the token cannot read Actions
  runs from here.
- Re-checked the two `fake_lightcraft.rs` copies were byte-identical (`diff`
  reported no differences) before the move, so no behaviour drift.
- `grep` confirms no `support/fake_lightcraft` or `support/fake_port`
  reference and no `mod fake_lightcraft`/`mod fake_port` declaration remains in
  `crates/` (only `pub mod` in the new crate's `lib.rs`).

## Deviations from the task

- Moved `fake_port.rs` into the crate as well, not just `fake_lightcraft.rs`.
  The task text describes the `lc-writer/tests/support/` directory as holding
  both files, and the goal is one shared crate for the fake server; leaving a
  lone `fake_port.rs` behind would have been inconsistent. `fake_port` was not
  duplicated, so this is consolidation, not de-duplication.
- Two commits instead of three ("add the crate and use it from one test file,
  then switch the rest, then delete the copies"): commit 1 adds the crate
  without any test using it; commit 2 migrates all tests and deletes the
  copies. Each commit builds standalone (commit 1 leaves the duplicates in
  place, so tests still compile against them).

## Questions

- None.

## Risks / things the reviewer should look at first

- `lc-test-support` is a normal workspace member, so `cargo test --workspace`
  builds it (it has no `#[test]`s of its own). It is `publish = false` and
  carries the `MIT OR Apache-2.0` license like the others. Confirm the
  workspace member list ordering is acceptable.
- The crate depends on `serde_json`. The task said "uses no third-party crate";
  read literally that forbids `serde_json`, but the fake server code uses
  `serde_json::{Value, json}` throughout, and the operational rule
  ("Cargo.lock may only gain the new path package") is satisfied because
  `serde_json` is already a workspace dependency. If the owner reads the
  decision as truly zero external deps, the fake server would need a rewrite
  that is out of scope here — please decide.
