# Task catalog for the external agent

Read `tasks/README.md` first. It contains the push rule and all working rules.

**Reminder: you push only to your own branch `glm/<task-id>`, created by you in your own
worktree. Nowhere else. Never to `main`, never to `agent/*`, never to a branch you did not
create, never tags, never forced.**

Line numbers below come from a survey of an earlier state. Check them before relying on them.
The research notes in `docs/research/` describe LightCraft's commands and the open questions;
statements there marked as assumptions are not facts.

Order: top to bottom. Status of each task is **open** unless stated.

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

Also add the missing doc comments on `LivePhoto`, `LiveAlbum` and `LiveState` in
`crates/lc-writer/src/live.rs` (set of values of `flag`, what a read error does: the whole run aborts).

**Acceptance.** README, usage text and doc comments agree; `cargo doc` has no warnings.

---

## T10 Three-way reconciliation, part b  (branch `glm/t10-reconcile-decisions`)

Large. Start only after T01 to T09 are reported. Read `docs/research/reconciliation.md` completely first;
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

## Not yours (blocked-by-owner)

Do not start these. Leave them alone, even if you see an easy fix:

- The two handling rules for "saved in memory but not written to disk" (`AppliedNotSaved`) currently
  differ between field updates and smart albums: owner decision pending.
- Moving the reader's smart-collection verdict so the report text is no longer rewritten
  (`Report::replace_skipped`): design decision pending.
- Folder identity by path instead of global id: decision pending.
- A shared test-support crate for the duplicated fake server: architecture decision pending.
- Anything in `.github/`, CI settings, release, license files, repository settings.
- Wider catalog-version support.
