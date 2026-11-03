# T09 — Documentation of command-line limits

Branch: `glm/t09-cli-docs` (base `origin/main` 7f36890).
Scope: documentation, `--help` text and doc comments; tests pin the two
command-line behaviours.

## 1. `--report` and `--script` must not name the same file

The check already exists in `crates/lr2lc/src/main.rs::check_targets`: when
both targets resolve to the same path it returns
`Err("--report and --script name the same file")`, and `main` maps that to
exit code 2 (before anything is read).

Now documented:

- `USAGE` / `--help` text (`crates/lr2lc/src/args.rs`) ends with the line
  "`--report` and `--script` must not name the same file."
- README `Options` section and the exit-code 2 row state it.

Tests added in `crates/lr2lc/src/main.rs` (new `#[cfg(test)] mod tests`):

- `report_and_script_pointing_at_the_same_file_is_rejected` — same path for
  both is `Err` with the documented message.
- `report_and_script_pointing_at_different_files_are_accepted` — different
  paths are accepted.

## 2. Most options may appear only once

`crates/lr2lc/src/args.rs::parse_args` already rejects a repeated
`--library`, `--script`, `--report`, `--port` or `--label-set` with
"`<option> given more than once`"; `--map` is repeatable by design. `main`
maps a parse error to exit code 2.

Now documented in `USAGE` / `--help` ("Every option except `--map` may be
given at most once …"), the README `Options` section and the exit-code 2 row.

Test added in `crates/lr2lc/src/args.rs`:
`single_valued_options_rejected_when_repeated` pins all five once-only
options (repeating each is an error) and that `--map` stays repeatable. The
existing `invalid_arguments_are_errors` and `bad_ports_are_errors` tests
already covered `--report`, `--port` and `--label-set` individually.

## 3. Folder identity is the folder path, not a global id

Checked the code:

- `model::Folder` (`crates/model/src/lib.rs`) holds only `path: PathBuf` —
  no id, no global id.
- `Catalog::folders` (`crates/lr-reader/src/folders.rs`) reads `id_local`
  only to name skip reasons; the returned `Folder` carries the assembled
  path (`absolutePath` + `pathFromRoot`).
- The mapping file (`crates/lc-writer/src/state.rs`) stores photos,
  keywords, collections, stacks and the Quick Collection — **not folders**.
  Folders are read fresh every run.

So a folder moved or renamed in Lightroom changes its path and is seen as a
new folder; the old path is simply gone from the next report. Nothing is
reconciled or reported about the old path. Photos inside keep their global
id, so a moved folder does not cause photos to be re-imported.

Documented in README, in the paragraph that follows the photo/keyword
global-id section.

Test added in `crates/lr-reader/tests/folders.rs`:
`folder_identity_is_the_path_not_an_id` renames a folder's `pathFromRoot`
in the catalog (same row id, new path), re-reads, and asserts that exactly
one path disappeared and exactly one appeared (ending in `renamed`), with
the total count unchanged — pinning that the row id does not carry identity,
only the path does.

## 4. Doc comments on `LivePhoto`, `LiveAlbum`, `LiveState`

`crates/lc-writer/src/live.rs`:

- `LivePhoto`: documents the values of `flag` (`none`, `pick`, `reject`),
  `rating` (0–5), `label` (colour name or `None`), and that empty texts are
  `None`.
- `LiveAlbum`: documents that `members` holds mapped global ids and that
  folder members are absent.
- `LiveState`: documents the commands sent (`catalog.query`,
  `photo.inspect`, `albums.list`, `keyword.list`) and that a read error is
  returned as a `WriteError` and aborts the whole run (a partial live state
  is never acted on).

## Verification

- `cargo fmt` applied; `cargo fmt --check` is clean.
- `cargo doc` could not be run to completion (no local C compiler, so the
  workspace cannot build); the doc comments are plain `///` on public items
  with no intra-doc links that could warn. CI is the source of truth.
- No new dependencies; no production logic changed (only doc comments and
  `USAGE` text).

## Files

- `crates/lr2lc/src/args.rs` — `USAGE` text; once-only-options test.
- `crates/lr2lc/src/main.rs` — `check_targets` unit tests (same-file).
- `crates/lr-reader/tests/folders.rs` — folder-identity test.
- `crates/lc-writer/src/live.rs` — doc comments on the three structs.
- `README.md` — command-line limits and folder identity.
- `tasks/reports/t09-cli-docs.md` — this report.
