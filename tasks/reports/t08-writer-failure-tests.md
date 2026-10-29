# T08 — Writer tests for failure paths

Branch: `glm/t08-writer-failure-tests` (base `origin/main` 7f36890).
Scope: tests only. Real defects are noted below for a separate commit; none
are fixed here.

## What was done

### 1. Mapping file write failure (`WriteError::State`)

Test `mapping_file_write_failure_is_reported_and_not_a_success`
(`crates/lc-writer/tests/apply.rs`).

The state file is pointed at a path whose parent directory does not exist,
so the atomic temp-file + rename in `State::save` cannot create the file.
`apply` returns `Err(WriteError::State(_))` — the run is reported as a
failure, never as a success summary. The run stops at the first persistence
(the first keyword creation), before any photo is imported: the test
asserts no `library.import` command was sent.

`State::save` (`crates/lc-writer/src/state.rs`) already maps the io error to
`StateError::Io`, and `WriteError` already has `State(#[from] StateError)`,
so no production change was needed for this point.

### 2. The 75 s I/O timeout

The timeout was a fixed `IO_TIMEOUT` constant with no way to shorten it for
tests. Smallest possible change, production behaviour unchanged:

- `crates/lc-writer/src/control.rs`:
  - `ControlClient` gained a `timeout: Duration` field.
  - New `connect_with_timeout(port, timeout)` constructs a client with a
    custom read/write timeout; `connect(port)` delegates to it with
    `IO_TIMEOUT`, so production code is unaffected.
  - `send` (the reconnect path) and `open` use `self.timeout` / the passed
    `timeout` instead of the constant.

Test `stalled_server_times_out_instead_of_hanging`
(`crates/lc-writer/tests/control.rs`): a fake port that accepts the request
but sleeps longer than the client's read timeout. With a 100 ms timeout the
client surfaces `WriteError::ConnectionLost { command: "photo.rate" }`
within the test's 30 s guard, the request was received exactly once (no
retry), and the test does not hang.

### 3. Two runs against one library in a row

Already covered by the existing
`second_run_with_unchanged_input_sends_only_read_commands`
(`crates/lc-writer/tests/apply.rs`): a second run with an unchanged input
sends only read commands (`write_commands()` is empty) and creates nothing
new. No additional test needed.

### 4. Photo or keyword deleted in LightCraft while still in the mapping file

**Photo — reported and not recreated.**
Test `photo_deleted_in_lightcraft_is_reported_not_recreated`.

First run imports four photos into `fake1`. The second run uses a fresh
library (`fake2`) that kept the keywords but lost every photo, while the
mapping file still records all four ids. The input is changed on `g1`
(rating 3 → 4) so the plan emits an update — and therefore a `library.select`
of the vanished LightCraft id — instead of an import. `library.select` is
rejected with `unknown photo id`; `apply` catches the `WriteError::Command`
in `select`, marks the photo incomplete (`SkipCode::Rejected`) and continues.
The test asserts:

- `photos_imported == 0` — the deleted photo is not re-imported;
- `photos_incomplete` has one entry whose reason names `library.select` and
  contains no path or catalog text;
- no `library.import` command was sent to `fake2`.

**Keyword — not recreated; reporting is a gap.**
Test `keyword_deleted_in_lightcraft_is_not_recreated`.

First run creates `Travel`, `Travel|Rome` and `Café ☕`. The second run uses
a library that dropped `Café ☕`; the mapping file still knows it. The plan
treats a keyword with a mapping-file entry as already created
(`state.keyword(text)` → `Some` → continue, no `keyword.create`, no library
check), so:

- no `keyword.create` command is sent to `fake2`;
- `fake2.keywords()` still does not contain `Café ☕`;
- the photos are unchanged, so `write_commands()` is empty.

**Defect noted for a separate commit:** a keyword that vanished from
LightCraft between runs is not reported. There is no `keywords_incomplete`
or skipped entry for it — the run looks clean although the library no longer
has the keyword. The photo path reports correctly; the keyword path does
not. Fixing this (e.g. letting the plan detect a mapping-file keyword that
is missing from `keyword.list` and reporting it) is left for a separate
commit, per the task's "tests only" rule.

## Verification

- `cargo fmt` applied; `cargo fmt --check` is clean.
- `cargo check`, `cargo clippy` and `cargo test` could not be run: the
  workspace has no local C compiler (`cc` missing), so proc-macro / native
  builds fail to link. CI is the source of truth for compilation and tests.
- No new dependencies were added. The only production change is the
  injectable timeout in `control.rs`.

## Files

- `crates/lc-writer/src/control.rs` — injectable timeout.
- `crates/lc-writer/tests/control.rs` — stalled-server timeout test.
- `crates/lc-writer/tests/apply.rs` — three failure-path tests (mapping
  file, deleted photo, deleted keyword).
- `tasks/reports/t08-writer-failure-tests.md` — this report.
