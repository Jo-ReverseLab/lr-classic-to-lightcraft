# T16 — Hostile catalogs

Task: a generator that writes deliberately damaged `.lrcat` fixtures, and tests
that prove the reader and the CLI never panic or hang on them. Each damaged row
is either skipped with a reason code (`read = converted + skipped` stays true) or
rejected with a typed `ReadError`; nothing crashes.

Branch: `glm/t16-hostile-catalogs` (base `origin/main`, 7f36890).

## What was added

- `tests/fixtures/generator/src/hostile.rs` — new module. `HostileCase` enum
  (29 cases), `slug()`/`group()`/`all()`, `write_hostile_catalog(dir, case)`,
  `apply_case`, `make_nullable`, `make_nullable_keyword_global_id`, plus two
  self-tests (`slugs_are_unique_and_windows_safe`,
  `every_case_writes_a_catalog`).
- `tests/fixtures/generator/src/lib.rs` — `pub mod hostile;` exported.
- `crates/lr-reader/tests/hostile.rs` — `read_pipeline` runs the full pipeline
  (photos, folders, keywords+assignments, collections, stacks) under
  `catch_unwind`; `no_hostile_case_panics_or_hangs` asserts balance
  (`read = converted + skipped`) for every report and rejects any panic.
- `crates/lr2lc/tests/hostile_cli.rs` — `no_hostile_case_crashes_the_cli` runs
  `lr2lc <catalog>` for each case and accepts only clean exit codes
  (`0`, `1`, `2`), never a signal crash.

## Acceptance rule

A damaged catalog may end in one of three outcomes, all acceptable:

1. **skip** — the row is dropped with a `SkipCode`, the report stays balanced;
2. **typed error** — the pipeline returns a `ReadError` (e.g. `MissingSchema`);
3. **open fails** — only for the two `Missing…Required…` structural cases.

A panic or a signal crash (SIGSEGV etc.) is always a defect.

## Per-case table

The behaviour column is read from the reader source (`photos.rs`, `folders.rs`,
`keywords.rs`, `catalog.rs`); CI is the proof since there is no local compiler.
T07's `skip_codes.rs` already exercises many of these mutations with assertions
on the exact skip code; T16 adds the remaining cases, the generator API, and the
"never panics across the whole set" guard.

| case | group | observed behaviour |
| --- | --- | --- |
| null-photo-rating | null | accepted as rating 0 (NULL → 0). |
| null-photo-global-id | null | skipped, `MissingGlobalId`. |
| null-photo-pick | null | accepted as flag none (NULL → none). |
| null-photo-color-label | null | accepted, no label for photo 1. |
| null-keyword-name | null | keyword skipped, empty name. |
| null-keyword-global-id | null | keyword skipped, missing global id. |
| null-root-absolute-path | null | photos/folders under root 1 skipped, `RootFolderNoPath`. |
| null-collection-name | null | collection skipped, invalid name. |
| dangling-folder-root | dangling id | folders under root 987654 skipped, `RootFolderNotFound`. |
| dangling-keyword-parent | dangling id | keyword 2 skipped, `ParentSkipped` (missing parent). |
| dangling-collection-parent | dangling id | collection 4 skipped, missing parent. |
| dangling-collection-image | dangling id | collection image dropped, photo missing. |
| dangling-stack-member | dangling id | stack member dropped, photo missing. |
| keyword-cycle | cycle | one of the two keywords skipped, `ParentSkipped` (cycle broken). |
| collection-cycle | cycle | one collection skipped, cycle broken. |
| duplicate-photo-global-id | duplicate or odd name | second photo skipped, `DuplicateGlobalId`. |
| duplicate-folder-path | duplicate or odd name | both folder rows kept (paths are identity; no reconciliation). |
| embedded-nul-name | duplicate or odd name | keyword skipped, name invalid (NUL). |
| blob-keyword-name | duplicate or odd name | keyword skipped, `value_as_text` returns none. |
| long-keyword-name | duplicate or odd name | accepted (name is valid text of 4096 chars). |
| rating-below-range | number out of range | photo skipped, `InvalidRating`. |
| rating-above-range | number out of range | photo skipped, `InvalidRating`. |
| bad-pick-value | number out of range | accepted as unknown flag. |
| empty-catalog | structural | 0 reads, every report balanced (converted=0, skipped=0). |
| extra-table | structural | ignored; normal read. |
| missing-optional-table | structural | reads fine; titles absent (`Adobe_AdditionalMetadata` optional). |
| missing-optional-column | structural | reads fine; `lc_name` optional. |
| missing-required-table | structural | `Catalog::open` fails, `MissingSchema`. |
| missing-required-column | structural | `Catalog::open` fails, `MissingSchema`. |

## Verification

- `cargo fmt --check` — clean.
- `cargo test` / `cargo clippy` — **not run**: no local C compiler (`cc`) in
  this workspace, so the toolchain cannot build. CI is the proof; the behaviour
  column is predicted from reading the reader source.
- The reader test pins the *acceptance rule* (no panic, balanced reports) rather
  than each exact skip code, because the exact codes are already pinned by T07's
  `skip_codes.rs`. An unexpected `ReadError` on a non-open-fails case is printed
  as a defect to review, not silently ignored.

## Open notes

- `DuplicateFolderPath` is accepted (both kept). Folder identity is the assembled
  path and the mapping file stores no folder ids, so there is no reconciliation
  to do; this matches the T09 folder-identity decision.
- `LongKeywordName` is accepted as valid text. If LightCraft rejects very long
  keyword names, that is a writer-side concern, not a reader defect.
