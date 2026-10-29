# Task catalog for the external agent

Read `tasks/README.md` first. It contains the push rule and all working rules.

**Reminder: you push only to your own branch `glm/<task-id>`, created by you in your own
worktree. Nowhere else. Never to `main`, never to `agent/*`, never to a branch you did not
create, never tags, never forced.**

Line numbers below come from a survey of an earlier state. Check them before relying on them.
The research notes in `docs/research/` describe LightCraft's commands and the open questions;
statements there marked as assumptions are not facts.

Status of each task is **open** unless stated.

**Order of work** (not the order of the numbers; tasks T13 to T23 were added later):

1. T01, T02
2. T13 (shared test support; do it before any other task that touches the fake server)
3. T03, T04, T05, T06
4. T14, T15 (two owner decisions that are now made)
5. T07, T08, T09
6. T16, T17, T18, T19, T20, T21 (large; each is independent of the others)
7. T10, T11
8. T22, T23, T12 (documents only)

Stacking: a task that says "Base: branch of Tnn" is branched from that task's branch.
Everything else is branched from the current `origin/main`. If a base branch is not merged
yet when you start, say so in the report and keep going.

---

## T01 Combined `auto` label set  (branch `glm/t01-auto-label-set`)

Base: this branch (`origin/main` after the task files were merged). The acceptance tests
already exist (commit "Add acceptance tests for the combined auto label set"). 26 of them are
marked `#[ignore = "waiting for ..."]`.

**Goal.** With `--label-set auto` (the default), the standard color-label names of German and
English are recognised together:
Rot/Red → red, Gelb/Yellow → yellow, Grün/Green → green, Blau/Blue → blue, Lila/Purple → purple.
Any other name is reported individually as a custom label (photo id only, never the name).
The comparison is exact, byte for byte: no case folding, no trimming, no Unicode normalisation
(a decomposed "Grün" does not match). `--label-set en|de|<file>` keeps its strict meaning
unchanged.

**Report fields (decided).** In `report.json`, under `photos.labels`:
- `label_set`: `"auto"` when auto is used, otherwise `"en"`, `"de"` or `"file"`.
- `matched_sets`: array of the standard sets that matched at least one name, in the order
  `["en","de"]`; empty for a fixed set. The field is always present.
- The readable summary shows `auto` for the set; the old hint recommending `--label-set`
  disappears in all cases.

**Steps.**
1. Remove the `#[ignore]` markers from the new tests one test file at a time and watch them fail
   for the right reason.
2. Implement in `crates/lr2lc/src/labels.rs` (and wherever the report fields are built).
3. Rewrite the unit tests in `labels.rs` that still describe the old behaviour
   (e.g. `auto_with_mixed_sets_makes_all_custom_and_recommends`).
4. Update README (option description, report fields, known limits) and CHANGELOG.

**Acceptance.** All label tests pass without `#[ignore]`; `fmt` and `clippy --all-targets -D warnings`
clean; no change in behaviour for fixed sets; the numbers still add up (read = converted + skipped);
second run identical; the catalog checksum is unchanged. No custom label name appears anywhere
in output or report.

---

## T02 Report naming and hint  (branch `glm/t02-report-naming`)

Base: branch of T01 (stack on it; say so in the report).

**Goal.**
1. `writer.keywords_skipped` in the report counts a different set than the reader's keyword
   numbers, which is misleading. Rename it so the name says what it counts (or document it in
   README and in the field's doc comment, if a rename would break the schema). Prefer a rename
   with a short note in CHANGELOG.
2. The label-set note must be in `report.json`, not only in the text output (check that T01
   fully covers this; add what is missing).

**Acceptance.** A test that pins the report key names; README documents each of the counters;
no other behaviour change.

---

## T03 Virtual copies  (branch `glm/t03-virtual-copies`)

Base: branch of T13.

**Goal.** Virtual copies are currently skipped with the reason code `virtual_copy`. Implement them
as follows:
- `photo.virtualCopy` is not idempotent in LightCraft, so a copy is created **exactly once**
  and recorded in the mapping file (state file next to the library, see `crates/lc-writer/src/state.rs`).
- If the mapping file is missing or has no entry, but the copy might already exist, do **not**
  create it again: skip it with a reason (nothing is created blindly).
- Stars, color label, pick/reject flag and keywords of the copy are written to the copy (not to the master).
- Never retry a failed `photo.virtualCopy` blindly. After an ambiguous answer, query the state
  first (read commands are listed in `docs/research/lightcraft-control-port.md`).
- Development settings of the copy are out of scope (later phase).

**Steps.** First extend the fixture generator with a virtual copy (a photo row that points to a
master); write tests against the fake control-port server (`crates/lc-writer/tests/support/` and
`crates/lr2lc/tests/support/`); then the reader/model/writer changes.

**Acceptance.** Tests for: first run creates one copy; second run creates none; missing mapping
file creates none and reports a reason; copy carries its own stars/label/keywords; a master with
two copies; a failed command is not retried. Report numbers still add up (the skip reason code
`virtual_copy` is replaced by the new outcome where applicable; update the documented code list).

---

## T04 French and Spanish label names  (branch `glm/t04-label-names-fr-es`)

Base: branch of T01.

**Goal.** Extend `auto` with the standard color-label names of Lightroom Classic in French and
Spanish.

**Rules.**
- Only names that you can **cite** from a public source (official Adobe documentation or
  Adobe-published language resources). Record each name with its source URL and the retrieval date
  in `docs/research/label-names.md`. A name without a citation does not go into the code.
- If you cannot find a source for a name, leave it out and list it under "Questions" in your report.
  Do not guess spellings or accents; matching is exact.
- No name may mean two different colors across all languages. Add a collision test over the whole table.
- Tests per language, including non-ASCII characters (exact comparison without normalisation).

**Acceptance.** Table, research document with sources, collision test, per-language tests, README
and CHANGELOG updated.

---

## T05 Reader robustness: no panics  (branch `glm/t05-reader-no-panics`)

**Goal.** `crates/lr-reader/src/keywords.rs` calls `.expect(...)` twice in `resolve_keywords` (around
lines 357 and 376). A broken internal invariant would abort the tool instead of skipping with a reason.
Replace both with a handled outcome: the affected keyword is skipped with a reason code and an id, and it
is counted, so read = converted + skipped still holds.

**Steps.** First a test that reaches the case through a crafted fixture (a keyword whose parent chain is
broken or cyclic), then the change. Check whether a cycle in the parent chain is handled at all.

**Acceptance.** No `expect`/`unwrap` on catalog-derived data in the reader (grep the crate and list the
remaining ones in the report with a one-line justification each); new skip code documented.

---

## T06 Writer: no silent early returns  (branch `glm/t06-writer-no-silent-returns`)

**Goal.** In `crates/lc-writer/src/apply.rs` there are three places that return `Ok(())` with a
"not reachable" comment and no skip entry (around lines 531, 819 and 1285, the last one in `finish_photo`).
Make each of them visible: either a skip entry with a reason (preferred when the case can occur), or an
explicit internal error that stops the run with a clear message (when it is a true invariant). Decide per
place and write the decision into a doc comment.

**Acceptance.** A test for each place if reachable by a fake server; otherwise a note in the report why it
is not reachable. No behaviour change for normal runs.

---

## T07 Reader tests for edge cases  (branch `glm/t07-reader-edge-tests`)

Tests only, no product code unless a test shows a real defect (then fix it in a separate commit and say so).

1. `Adobe_DBVersion` stored as a real number and as a blob (see `crates/lr-reader/src/catalog.rs`
   around lines 152 to 153): the generator builds only text values today. Add variants to the generator
   and tests in `crates/lr-reader/tests/open.rs`.
2. A `-shm` file beside the catalog: today only `-wal` and `-journal` are refused. Write a test that
   pins the **current** behaviour and describe in the report whether you think that is right. Do not change
   the behaviour (that is a decision for the owner).
3. Skip codes: none of the reader integration tests asserts a `SkipCode`. Add assertions for every code the
   reader can produce.
4. The fixture generator builds one catalog version only. Add a second, older supported version if the
   reader claims to support it (check `docs/research/lrcat-schema-lrc15.md` and the reader's version check).

**Acceptance.** All new tests pass on Linux and on Windows (no illegal file names such as `?` in test
directories, no reliance on case-sensitive paths).

---

## T08 Writer tests for failure paths  (branch `glm/t08-writer-failure-tests`)

Tests only (fix real defects in a separate commit, noted in the report).

- Writing the mapping file fails (`WriteError::State`): the run reports it and does not claim success.
- The 75 s I/O timeout: make the timeout injectable for tests if it is not (smallest possible change,
  note it in the report) and test that a stalled server leads to a clear error, not a hang.
- Two runs against one library in a row: the second creates nothing new.
- A photo or keyword was deleted in LightCraft while it is still in the mapping file: the run reports
  it and does not recreate it silently.

**Acceptance.** Tests pass against the fake control-port server; no real LightCraft needed.

---

## T09 Documentation of command-line limits  (branch `glm/t09-cli-docs`)

**Goal.** Document, in README and in the `--help` text:
- `--report` and `--script` must not point to the same file (`crates/lr2lc/src/main.rs`, exit code 2);
- most options may appear only once; repeating them ends with exit code 2 (`crates/lr2lc/src/args.rs`).
Add tests that pin both behaviours if they are missing.

Also document, in README, that folder identity is the folder **path** (not a global id, unlike photos and
keywords) and what that means after a folder is moved or renamed in Lightroom (it is seen as a new folder;
check the code and describe what really happens, with a test if none exists).

Also add the missing doc comments on `LivePhoto`, `LiveAlbum` and `LiveState` in
`crates/lc-writer/src/live.rs` (set of values of `flag`, what a read error does: the whole run aborts).

**Acceptance.** README, usage text and doc comments agree; `cargo doc` has no warnings.

---

## T10 Three-way reconciliation, part b  (branch `glm/t10-reconcile-decisions`)

Large. Start only after T01 to T09 and T13 to T21 are reported (it builds on the shared test support,
the single `AppliedNotSaved` rule and the reader-side smart verdict). Read `docs/research/reconciliation.md` completely first;
its section on the planner contains line references to `plan.rs` that you must **re-check** before use.

**Decision rule (decided by the owner).** For a photo that already exists in LightCraft:
- A Lightroom value is written only where LightCraft still holds the default value
  (0 stars, no flag, no color label, empty text).
- Any other difference is a **conflict**: it is reported and **not** written.
- Nothing is ever deleted in LightCraft.
- Applies per field (stars, flag, label, title, caption, copyright, creator) and per element for keywords
  and album members.
- The report names fields and ids only, never values.

**Notes.**
- `read_live` in `crates/lc-writer/src/live.rs` is not called from the CLI yet. This task is the first
  caller. `photo.inspect` per photo is expensive: read only what is needed, only for photos that exist in
  both worlds, and note the cost in the report.
- Idempotence: a second run must report the same conflicts and write nothing new.

**Steps.** Tests first against the fake server: default value (written), non-default (conflict), both
equal (nothing), idempotent second run, keyword added in LightCraft only (kept), keyword removed in
Lightroom (reported, not removed).

**Acceptance.** All of the above, counters add up, report fields documented in README and CHANGELOG.

---

## T11 Reconciliation, part c  (branch `glm/t11-reconcile-removed-renamed`)

Start only after T10 has been reviewed. Branch from the T10 branch.

**Goal.** Removed and renamed items, report only (nothing is deleted or renamed in LightCraft):
- a photo removed from Lightroom: report it, and take it out only of albums that lr2lc created;
- a keyword renamed in Lightroom: recognised by its global id, reported as renamed, no second keyword is created;
- the file path of a photo is recorded in the mapping file, so a moved file is recognised.

**Acceptance.** Tests for each case; second run identical; nothing destructive anywhere.

---

## T12 Research only: development settings  (branch `glm/t12-develop-research`)

Documents only, no Rust code. Phase 6 (development settings) is the project's main differentiator.
Using `docs/research/lightcraft-develop.md`, public documentation and public descriptions of the Lightroom
format, write `docs/research/develop-mapping.md`:
- which develop settings exist on both sides, with a mapping table (Lightroom setting → LightCraft control);
- what has no counterpart, and how it would be reported;
- the structure of the data in a catalog as far as it is publicly documented (no values from any real catalog;
  you have none);
- a proposed model extension and a proposed test plan with generated fixtures.
Mark each statement as **sourced** (with URL) or **assumption**. Do not write assumptions as facts.

**Acceptance.** Every row has a source or the assumption marker; the open decisions for the owner are
listed in a final section.

---

## T13 Shared test support for the fake LightCraft server  (branch `glm/t13-test-support`)

**Decided by the owner:** yes, a shared test-support crate, as a new **workspace member** that uses no
third-party crate.

**Goal.** The fake control-port server exists twice: `crates/lc-writer/tests/support/` (two files,
`fake_lightcraft.rs` and `fake_port.rs`) and `crates/lr2lc/tests/support/fake_lightcraft.rs`
(the survey found the two `fake_lightcraft.rs` byte-identical; re-check). Move it into one crate
(suggested name `lc-test-support`, directory `crates/lc-test-support`, `publish = false`), used only as a
`[dev-dependencies]` path dependency by `lc-writer` and `lr2lc`.

**Rules.**
- No new external crate. `Cargo.lock` may only gain the new path package.
- Behaviour of every existing test stays the same. Do the move in small commits: add the crate and use it
  from one test file, then switch the rest, then delete the copies.
- The new crate carries `MIT OR Apache-2.0` like the others and a short `//!` doc comment.
- The crate must build on Linux, macOS and Windows (no platform-specific code).

**Acceptance.** All existing tests pass unchanged in meaning; no duplicate of the fake server is left;
`cargo test --workspace`, `fmt`, `clippy --all-targets -D warnings` clean; CHANGELOG not needed (internal).

---

## T14 One rule for "saved in memory but not written to disk"  (branch `glm/t14-applied-not-saved`)

Base: branch of T13.

**Decided by the owner (on the lead's recommendation):** when LightCraft answers that a change was
applied but not saved (`AppliedNotSaved`), lr2lc does **not** record it as done in the mapping file.
It re-plans it, so the next run sends it again.

**Goal.** Make field updates (stars, flag, label, title, caption, copyright, creator) and `keyword.create`
follow the same rule as `album.createSmart` already does (`crates/lc-writer/src/apply.rs`, around lines 589
to 598, 986 to 1002, 1104 to 1116 and 1278 to 1305; re-check the numbers).

**Why this is safe.** Field updates are idempotent, and a keyword is looked up with `keyword.list` before
it is created. The only step that can produce a duplicate is `album.createSmart`; the report must say so
in that case (by id, no names).

**Steps.** Tests first against the fake server: an `AppliedNotSaved` answer for each kind of step leaves
the mapping file without that entry, the report lists the step as not saved, and the next run sends it
again. Then the change. Update the doc comments that describe the old rule and the README section on
re-runs.

**Acceptance.** One rule, documented in one place; tests for every step kind; second run behaves as above;
numbers still add up.

---

## T15 The reader decides the smart-collection verdict  (branch `glm/t15-smart-verdict`)

**Decided by the owner:** yes.

**Goal.** `crates/lr2lc/src/dry_run.rs` (around lines 205 to 219) rewrites report entries by matching the
prefix `"collection <id>:"` through `Report::convert_skipped` and `Report::replace_skipped`
(`crates/model/src/lib.rs`). That contradicts the rule in `crates/model/src/skip.rs` (a code is set where
the reason arises, never derived from the text). Let the reader produce the final verdict for each smart
collection (translatable or not, with the reason code), so the CLI never edits the report by reason text.
Then remove `convert_skipped` and `replace_skipped` if nothing else uses them.

**Rules.** The report output (keys, codes, numbers) must stay **identical** for every existing test
unless a test shows the old output was wrong (then say so in the report). The layering stays
reader → model → writer; the reader must not depend on the writer.

**Acceptance.** No reason-text matching remains (grep and show it); existing smart-collection tests pass;
a test that the verdict comes from the reader; read = converted + skipped still holds.

---

## T16 Hostile catalogs: no panic, numbers add up  (branch `glm/t16-hostile-catalogs`)

Tests first. Product code only to fix a defect a test proves (separate commit, noted in the report).

**Goal.** Extend the fixture generator (`tests/fixtures/generator`) with a way to make deliberately
damaged catalogs, and write reader and CLI tests that run each one and assert: no panic, no hang, a clear
error or a skip with a reason code, and read = converted + skipped. Cases (each its own test):

- NULL where a value is expected (names, paths, ids, ratings, flags, labels) in every table the reader uses;
- an id that points to a row that does not exist (folder, root folder, keyword parent, collection parent,
  image of a collection, stack member);
- cycles: keyword parent chain, collection-set chain, stack that contains itself;
- duplicate global ids; two rows with the same path; an empty name; names with very long text,
  embedded NUL, a lone surrogate-like byte sequence stored as a blob in a text column;
- numbers out of range: rating below 0 or above 5, negative ids, huge ids, a pick value other than -1, 0, 1;
- a catalog with all tables present but empty; a catalog with an unknown extra table;
- a missing optional table or column (older or newer layout than the supported one).

**Rules.** Everything is generated, never taken from a real catalog. The generator's new options have
tests of their own. Test directories and file names must be valid on Windows.

**Acceptance.** All cases pass or are reported as defects with a minimal failing test; a table in the
report that lists each case and the observed behaviour (error, skip code, or accepted).

---

## T17 Path mapping test matrix  (branch `glm/t17-path-mapping`)

**Goal.** `--map OLD=NEW` (see `crates/lr2lc/src/args.rs` and where the mapping is applied) is a core
feature of "runs everywhere". Pin its behaviour with tests and fix real defects in separate commits.

Cases: prefix boundary (`/a/b` must not rewrite `/a/bc/x`); trailing separators on either side; several
`--map` options and which wins (longest prefix? first? decide by reading the code, then test and
document the actual rule); Windows drive letters and UNC paths (`C:\data\x`, `\\server\share\x`) in the
catalog while running on Linux; mixed separators; case differences on Windows-style paths; non-ASCII names
(composed and decomposed, no normalisation: they stay different); empty NEW; relative paths; `..`
segments (the mapping must not climb out of the new prefix silently: report it); a mapping that makes two
photos land on the same path (reported, nothing merged silently).

**Rules.** The pure mapping function must be testable without a file system. Do not touch originals; the
tool must never check existence by modifying anything. Paths in tests are neutral (`C:\data\...`).

**Acceptance.** A table-driven test file; README documents the exact rule in a short section; defects
fixed with tests.

---

## T18 Writer tests: stacks, quick collection, albums  (branch `glm/t18-writer-album-stack-tests`)

Tests only against the fake server (defects fixed in separate commits and noted).

Read `docs/research/lightcraft-albums.md` first. Cases:

- **Quick collection:** `album.toggleTarget` removes **all** photos if all are already in. The writer must
  read the members first and send only the missing ones. Test: all in, none in, some in, empty collection,
  a photo that vanished from LightCraft, a repeated run.
- **Stacks:** top photo and order are taken over, collapsed state is only counted. Test: a stack of one,
  a stack whose member is missing in LightCraft, two stacks that share a photo (reported, nothing
  half-written), a stack already present in LightCraft, a second run.
- **Albums and album folders:** nested folders several levels deep (and the unknown depth limit of
  `albums.list`: test that a deeper tree than the fake server returns is detected and reported, not
  silently cut), same-named albums in one folder (names unchanged, duplicates only counted), a member
  that is not in the library, an empty album, a second run, an album that the user renamed in LightCraft.
- A command that fails in the middle of a album: what is recorded in the mapping file, what the next
  run does (never blind repeat).

**Acceptance.** Every case has a test or a note in the report on why a fake server cannot show it; the
report states which cases found a defect.

---

## T19 End-to-end and golden-report tests  (branch `glm/t19-end-to-end`)

Tests only (defects fixed in separate commits and noted).

**Goal.**
1. One generated catalog that uses **every** supported feature (folders, photos, keywords with hierarchy,
   synonyms, attributes, ratings, flags, labels, collections, collection sets, smart collections both
   translatable and not, stacks, quick collection, titles, captions, copyright, creator, a video, a
   missing file, a virtual copy once T03 exists). Run the whole tool: dry run, then `--apply` against the
   fake server, then `--apply` again. Assert: the second run changes nothing in the fake server's state;
   the catalog checksum is the same before and after all three runs; read = converted + skipped for
   every counter group; the dry-run numbers equal the numbers of the first apply run.
2. **Golden files** for `report.json` (and the readable summary): a stored expected output for that one
   catalog, compared in a test. The test must print a readable diff on failure and must be easy to
   re-generate (document the one command). Golden files contain only generated data.
3. A **report format document** `docs/report-format.md`: every key of `report.json`, its type, when it
   appears, and the guarantees (numbers add up; no values from a catalog, only names of fields and ids).
   Do not add or rename keys here; if you find an inconsistency, list it under "Questions".

**Acceptance.** All of the above; the golden test fails if a key is renamed; works on Windows (line endings
and path separators in golden files are handled, say how).

---

## T20 Performance and complexity guard  (branch `glm/t20-performance`)

**Goal.** The generator can produce a large catalog (`write_large_catalog` in
`tests/fixtures/generator/src/lib.rs`). Measure the reader, the planner and the whole dry run with
catalogs of 10 000, 100 000 and 500 000 photos (with keywords and collections in proportion; extend the
generator if needed, deterministic seed). Find and remove accidental quadratic behaviour (a `Vec`
searched inside a loop, repeated string building, repeated `clone` of big collections, one SQL query per
photo instead of one per table, an unbounded memory copy of all photos).

**Rules.**
- Report numbers (time, peak memory if you can measure it, number of SQL statements) in the task report
  in a table. If your workspace cannot run `cargo`, say so and instead list the suspected hot spots with
  file and line plus the reasoning; do not invent measurements.
- A change must not change any output. Prove it with the existing tests plus one test that compares the
  report of a medium catalog before and after.
- Performance tests must not run in the normal `cargo test` (mark them `#[ignore]` with a clear reason
  and document the one command that runs them). A small version (about 2 000 photos) may run normally
  with a generous time limit that cannot fail on a slow CI runner.
- The live writer: count how many control-port requests a run needs per photo, and list which could be
  batched. Do not change the batching in this task (design decision); write the findings down.

**Acceptance.** Measurements or reasoned hot-spot list; at least the clear wins fixed in small commits;
no change in output; the guard test exists and is documented.

---

## T21 Audit: lossy casts, panics, indexing  (branch `glm/t21-audit-casts-panics`)

**Goal.** T05 covers `.expect` in the reader's keywords. Do the same audit for the whole workspace:

- every `unwrap`, `expect`, `panic!`, `unreachable!`, `todo!`, array or slice index and range slice
  (`[a..b]`) on data that comes from a catalog, from a LightCraft answer or from the command line;
- every `as` cast between integer types (the survey counted about six `as u8/u16/u32/i32/usize`), and
  arithmetic that could overflow on a big or hostile value;
- every `String::from_utf8_lossy`, `to_string_lossy` and similar that silently changes data (a name must
  stay byte for byte, with no Unicode normalisation).

For each finding: fix it (test first) or list it in `tasks/reports/t21-audit-casts-panics.md` with a
one-line justification why it is safe. Test code is out of scope.

**Acceptance.** A table of all findings by file and line with the decision; no panic left on external
data; fixes have tests; `fmt` and `clippy` clean.

---

## T22 Triage of the assumptions in the research notes  (branch `glm/t22-assumption-triage`)

Documents only, no Rust code.

**Goal.** `docs/research/` contains about 59 statements marked as assumptions (for example the depth limit
of `albums.list`). Produce `docs/research/assumptions.md`: one row per assumption with the file and line,
the statement, why it matters for the tool, what would make it a fact (a source to cite, a read of the
code in this repository, or a live check against the app), the **risk if wrong** (wrong data, lost data,
only a worse message) and a priority.

Then derive a **live-check checklist**: the smallest list of commands, in order, that an owner with a
running app and a throw-away library can run to settle the high-priority rows, each with the exact
expected answer shape and what to record (numbers and field names only, never user content).

**Rules.** Never turn an assumption into a fact without a citation. If you can cite a public source for a
row, add the URL and the retrieval date and mark it **sourced**.

**Acceptance.** Every assumption is listed once; the checklist is runnable as written; a short section
"decisions for the owner" at the end.

---

## T23 CI proposal (document only)  (branch `glm/t23-ci-proposal`)

Documents only. You must **not** change anything under `.github/`.

**Goal.** Write `docs/ci-proposal.md`: what the CI should additionally check and what it would cost.
Candidates: `--locked` builds, the minimum supported Rust version, `cargo audit` or `cargo deny` (note
that these are extra tools, which need the owner's approval), a coverage report, the performance guard
from T20 on a schedule instead of on every push, link and prose checks on README and CHANGELOG, a check
that the `tasks/` directory is gone before a release, a check for personal paths and names in the diff,
and a job that builds the three systems with a cache. For each: benefit, run time, cost in Actions
minutes (macOS minutes count at a high multiplier on private repositories: use the numbers that
`PLAN.md` gives, mark guesses as guesses), risk, and the exact change you would make to the workflow
file, written as a snippet in the document, not as a change.

**Acceptance.** Ranked list with a recommendation; open decisions for the owner at the end.

---

## Not yours (blocked-by-owner)

Do not start these. Leave them alone, even if you see an easy fix:

- Folder identity: the owner decided to **keep** identity by path, no change. (Task T09 documents it.)
- Anything in `.github/`, CI settings, release, license files, repository settings.
- Wider catalog-version support.
- Develop settings code (Phase 6). T12 is research only.
- Changing how many control-port requests are batched (T20 only reports).
- Any new dependency, even a "small" one. Write the question into the report.
