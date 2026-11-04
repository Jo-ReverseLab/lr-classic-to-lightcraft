# Plan

Working note for the next steps. Findings come from a read-only survey of
`main` at `83d76c2`; items marked *unverified* were not confirmed by a test
run, because nothing compiles in this workspace (see "How to verify").

## Where we stand

- Last CI run on `main` (40705049464, 2026-10-28): **ubuntu passed** in 6m34s,
  **windows failed** in 5m4s, **macos cancelled** after 15m.
- Windows failure: `opens_path_with_special_characters` created a directory
  named `a #b?c%20 ü`. `?` is illegal in a Windows file name, so
  `create_dir` failed at `crates/lr-reader/src/catalog.rs:355`. The product
  code was fine; the test asked for a file that cannot exist. Fixed on this
  branch, `?` stays covered by `encodes_special_characters`, which needs no
  file system.
- macOS: the job reported no runner (`runner_name` empty) and ended
  `cancelled` — not a code failure. Open question: runner availability or a
  manual cancel.
- The Lightroom rules from `CLAUDE.md` hold in the code we read: catalog
  opened `mode=ro&immutable=1` (`catalog.rs:229`), originals never written,
  dry run by default, checksum compared in `dry_run.rs:236` and `:350`.

## How to verify

GitHub Actions is the only proof for now. This workspace has `cargo` and
`rustc` 1.99.1 but **no C toolchain** (`cc`, `ld`, `crt1.o` all missing), so
`cargo check` stops at the first build script: ``error: linker `cc` not
found``. `cargo fmt --check` works locally; clippy and tests do not.

```sh
git checkout -b <branch>
git push -u origin <branch>
gh run list --limit 3
gh run view <id> --log-failed
```

A full matrix run bills roughly 170 Actions minutes (macOS minutes count at a
high multiplier on private repositories), so small, single-purpose commits.

## What the survey found

### P1 — results that can be wrong or quietly lost

1. **`AppliedNotSaved` is handled two different ways.** Field updates and
   `keyword.create` are written into the mapping file even when LightCraft
   answers "saved in memory but not written to disk"
   (`lc-writer/src/apply.rs:1104-1116`, `:986-1002`, `:1278-1305`), while
   `album.createSmart` is skipped and re-planned instead
   (`apply.rs:589-598`, reason: "the next run creates it again"). If the app
   loses that step, one kind of change is never sent again. Decide one rule
   and test it. *unverified behaviour, verified code paths*
2. **Two panics in the reader.** `lr-reader/src/keywords.rs:357` and `:376`
   call `.expect(...)` on an internal invariant of `resolve_keywords`. If it
   ever breaks, the tool aborts instead of skipping with a reason — against
   "nothing is lost silently".
3. **Silent early returns.** `apply.rs:819-822`, `:531-534` and
   `finish_photo` `apply.rs:1285-1290` return `Ok(())` with a "Not reachable"
   comment and no skip entry. Harmless while the invariant holds, invisible
   when it does not.

### P2 — design debt that will cost later

4. **Smart collections rewrite the report by reason text.**
   `lr2lc/src/dry_run.rs:205-219` matches `"collection <id>:"` prefixes
   through `Report::convert_skipped` and `Report::replace_skipped`
   (`model/src/lib.rs:407`, `:418`) — while `model/src/skip.rs:4-6` states a
   code "is set where the reason arises, never derived from the text". Either
   the reader knows the smart verdict, or the prefix matching goes.
5. **Folder identity is the path** (`model/src/lib.rs:54`), photos and
   keywords use global ids. Rule 5 promises global ids; folders are the
   exception. Decide and document it.
6. **`fake_lightcraft.rs` exists twice**, 1,181 byte-identical lines in
   `lc-writer/tests/support/` and `lr2lc/tests/support/`. One change needs
   two edits; move it into a test-support crate member of the workspace.

### P3 — no tests yet (no known bug)

7. `-shm` beside the catalog: `catalog.rs:113` refuses only `-wal` and
   `-journal`, while `lr2lc/src/report_file.rs:28` protects `-shm` as an
   output target. Neither check nor test exists for the read side.
8. `Adobe_DBVersion` as Real or Blob (`catalog.rs:152-153`) is normalised but
   untested; `tests/open.rs` only uses text values.
9. None of the 23 reader integration tests mentions `SkipCode`; codes are
   asserted only in unit tests and `lr2lc/tests/skip_codes.rs`.
10. The Windows branch of `path_bytes` (`catalog.rs:238`) had no real open
    test until this branch; the first push will show whether it works.
11. Writer gaps: mapping file write failure (`WriteError::State`), the 75 s
    I/O timeout, two runs against one library, and a photo or keyword deleted
    in LightCraft while still in the mapping file.
12. `live.rs:56` (`read_live`) is never called from `crates/lr2lc/src`, only
    from `lc-writer/tests/live_state.rs`. Dead entry point or missing feature?
13. The fixture generator builds DBVersion `1500000` only
    (`tests/fixtures/generator/src/lib.rs:726`) and never develop tables.

### P4 — documentation and CI

14. `--report` and `--script` must differ (`main.rs:70-74`) and most options
    may appear once (`args.rs:79,86,93,111,118`): both exit with code 2, both
    missing from README and `USAGE`.
15. CI does not check: `--locked` builds, MSRV, `cargo audit`/`deny`,
    coverage, the `large` fixture as a performance guard, a real LightCraft
    app, or link and prose checks in README and CHANGELOG.
16. `docs/research/` carries about 59 "assumption" markers. Example:
    `albums.list` nests folders "up to a depth limit" with no documented
    number (`albums.md:52`, `lc-writer/src/query.rs:36-46`, README:538-540).

## Next in line

1. This branch: green Windows job.
2. Clarify the macOS cancel, so all three systems really run again.
3. One P1 item at a time, test first, one branch each.
4. P2 as decisions, not as a refactor sweep.
5. Develop settings — the project's differentiator — only after P1 and P2,
   because they change model, reader and writer at once.

## Risks

- Nothing is compiled before a push, so every commit must stand alone and be
  small enough for CI to pin down.
- Actions minutes are paid. If the allowance runs out, CI silently stops
  proving anything; watch `gh run list`.
- The token in this workspace holds **admin** on the repository
  (`gh api repos/<owner>/lr2lc --jq .permissions`). Narrowing it to
  *Contents: Read and write* limits the damage if it leaks.
- The P1 items concern the **LightCraft library**, not the catalog: the
  read-only promise for Lightroom data holds in everything we read.

## Questions for the owner

- Which `AppliedNotSaved` rule should win: record it and risk a lost step, or
  re-plan it and risk a duplicate?
- Should the reader decide smart collections, so the reason text never has to
  be rewritten?
- Is develop settings the next milestone, or wider catalog-version support
  first?
