# Triage of the assumptions in `docs/research/`

> **Status:** Documentation only. No product code was changed. Evidence was read
> on 2026-11-04 from the read-only LightCraft checkout at
> `~/lr2lc-ref/lightcraft` (workspace version 0.14.0, commit `bb08a69`) and from
> this repository at `agent/t20-performance` (3edac05).

## Scope and method

Every statement in `docs/research/` that is marked as an assumption, "not
verified", "open", "unverified", "presumably", "inference" or "not read" was
collected with `grep`, read in context and listed **once**. Statements that
appear in more than one file are merged into one row; the row names the primary
location, and a second location is added in the statement cell where it adds
information. File-header notes
of the form "anything without a file:line is an assumption" are the rule of the
notes, not an assumption themselves, and are not rows. Pure design suggestions
(`*(design suggestion)*`) are decisions, not assumptions; they are not rows
either. The `(Open question)` marks that assert a fact about Lightroom or
LightCraft (rather than asking a product question) are included.

Two statements are included although the marker scan does not catch them,
because they are load-bearing for the writer and one is the example named in the
task: the `albums.list` depth limit (`lightcraft-albums.md:52`) and the
selection gate (`lightcraft-albums.md:16`).

Result: **77 rows** (A01–A77). The task estimated about 59; the difference comes
from splitting claims that share one sentence (for example the two assumed
column sets of the stack tables) and from including the fact-asserting open
questions. A second pass after the completeness review added the five
statements the marker scan had not caught: the `camera`, `fileName` and
`stacked` smart-collection criteria (`smart-collection-mapping.md:32`, `:33`;
`lightcraft-albums.md:101`), the cover-image table
(`lightcraft-albums.md:195`) and the post-creation synonym question
(`lightcraft-control-port.md:77`). Every claim still appears only once.

### Evidence classes

| Tag | Meaning |
|---|---|
| **fact** | Settled by code in this repository or in the LightCraft checkout. The cell names `file:line`. |
| **sourced** | Settled by a public source; URL and retrieval date are given. |
| **corrected** | The assumption is contradicted by the code. The correction is the new fact. |
| **stale** | The research note is out of date because the lr2lc code has moved on. |
| **open** | Still unverified. A live check (`L…`) or a catalog structure check (`C…`) is named. |

Priority: **high** = can produce wrong data, lose data or breach a project
principle in the MVP; **medium** = affects a feature that is implemented but not
yet enabled, or only produces a worse message; **low** = later phase, internal
format we do not write, or cosmetic.

Risk uses the task's three classes: **wrong data**, **lost data**, **only a
worse message**.

## Summary

| Priority | Rows | Settled part (fact / corrected / stale / partly) |
|---|---:|---:|
| high | 21 | 11 |
| medium | 38 | 20 |
| low | 18 | 8 |
| **Total** | **77** | **39 settled or partly settled, 38 fully open** |

The settled column counts a row when its evidence cell carries a class,
including `partly`. Of the 39, **31 are fully settled** (fact, sourced,
corrected or stale, without the `partly` qualifier) and **8 are partly
settled**; the remaining **38** are fully open.

Per file (primary location):

| File | Rows |
|---|---:|
| `lightcraft-control-port.md` | 9 |
| `lightcraft-library.md` | 3 |
| `lightcraft-import.md` | 3 |
| `lightcraft-develop.md` | 2 |
| `label-names.md` | 2 |
| `lrcat-schema-lrc15.md` | 2 |
| `reconciliation.md` | 9 |
| `smart-collection-mapping.md` | 21 |
| `lightcraft-albums.md` | 26 |

## The table

| ID | File:line | Statement (core) | Why it matters for lr2lc | What makes it a fact | Risk if wrong | Pri |
|---|---|---|---|---|---|---|
| A01 | `docs/research/lightcraft-control-port.md:13` | The request `id` is echoed in the reply by the transport. | The client matches replies to requests by `id`; a missing echo would desynchronise a batch. | **fact** — request id is stored (`lightcraft:apps/lightcraft/src/control_server.rs:167`) and inserted into every reply (`:216`); asserted by the test at `:329`. Also **sourced**: <https://raw.githubusercontent.com/storytold/lightcraft/bb08a69/docs/control-protocol.md> (retrieved 2026-11-04). | only a worse message | low |
| A02 | `docs/research/lightcraft-control-port.md:19` | `library.import` through the control port is synchronous, subject to the 60 s reply limit. | The writer imports one file per call and must know when the reply arrives; batching affects runtime. | **fact** at engine level — the command body calls `import_with` inline (`lightcraft:crates/engine/src/cmd/library.rs:1646`), which runs `prepare` and `commit_prepared` in the same call (`lightcraft:crates/engine/src/import.rs:800-806`). Response time on a slow volume stays open (L8). | only a worse message | medium |
| A03 | `docs/research/lightcraft-control-port.md:34` | XMP auto-write also fires for commands run through `engine.execute` (the `lib.rs:603` path). | If true, any write can create sidecars next to originals, breaching principle 2. | **fact** — the auto-write check is in `Session::execute` at depth 0 (`lightcraft:crates/engine/src/lib.rs:603-606`), the same path the control port uses (`lightcraft:crates/ui-egui/src/control.rs:329-333`). The writer also aborts unless `--allow-xmp` (`crates/lr2lc/src/args.rs:27`). | lost data | high |
| A04 | `docs/research/lightcraft-control-port.md:55` | `photo.rate` sets an absolute value, so it is idempotent. | Re-runs must not toggle stars. | **fact** — `Op::SetRating { rating: r as u8 }` with `r` validated to 0..5 (`lightcraft:crates/engine/src/cmd/library.rs:799-812`). | wrong data | low |
| A05 | `docs/research/lightcraft-control-port.md:58` | `addKeywords` is a set-union; duplicate keywords are not created. | Keyword re-sync needs idempotent adds. | **fact** — `add` cleans each keyword and skips a case-insensitive duplicate (`lightcraft:crates/engine/src/cmd/library.rs:1091-1097`). | wrong data | medium |
| A06 | `docs/research/lightcraft-control-port.md:60` | A second `keyword.create` of an existing keyword errors rather than updates; the merge hint in `keywords.rs:47-54` is misleading. | Creating a keyword twice must not fail the run or duplicate a node. | **fact** — `create_keyword_ops` returns `KeywordExists` (`lightcraft:crates/catalog/src/keywords.rs:690-697`); `has_keyword` is true for a listed keyword **or** one on a library photo, itself or below (`:608-615`); the command turns it into a rejection (`lightcraft:crates/engine/src/cmd/keywords.rs:355-384`). The writer looks up `keyword.list` before creating (`crates/lc-writer/src/apply.rs:55`, `:1229`). | wrong data | high |
| A07 | `docs/research/lightcraft-control-port.md:67` | `photo.inspect` returns the photo's source under a stable key, including the file path. | Path→id reconciliation needs the path from `photo.inspect`. | **fact in code** — `photo.inspect` serialises the whole `Photo` (`lightcraft:crates/engine/src/cmd/query.rs:161-166`); `Source` is tagged `type`/lowercase (`lightcraft:crates/catalog/src/model.rs:87-94`), so the shape is `{"source":{"type":"file","path":…}}`, snake_case (`Photo` has no rename, `model.rs:224-286`). The actual reply is still worth one live look (L2). | only a worse message | medium |
| A08 | `docs/research/lightcraft-control-port.md:68` | `library.import.imported` may be plain ids or `{path,id}` objects. | The writer pairs each import with its path; an object shape would break the mapping. | **fact** — `ImportReport.imported: Vec<u64>` (`lightcraft:crates/engine/src/import.rs:195-196`); the command reads it back as integers (`lightcraft:crates/engine/src/cmd/library.rs:1649`). Elements carry no path, so one file per call stays right. | wrong data | medium |
| A09 | `docs/research/lightcraft-library.md:30` | The on-disk journal format is internal; no stability promise for third parties is documented. | Justifies never writing `catalog.snap`/`catalog.log` directly. | **fact (negative)** — the format is versioned, CRC-protected and documented only in code (`lightcraft:crates/catalog/src/journal.rs:1-79`; `lightcraft:docs/architecture.md:29-31`); no third-party promise found in the checkout. | lost data | low |
| A10 | `docs/research/lightcraft-library.md:99` | `Meta` might contain a free field that could hold a Lightroom id. | Provenance could live in the photo instead of a side file. | **fact** — the full `Meta` field list is descriptive (`lightcraft:crates/catalog/src/model.rs:166-208`: camera…focus); no external-id field. `Photo` likewise has none (`model.rs:224-300`). The mapping file stays the only place for identity. | wrong data | high |
| A11 | `docs/research/lightcraft-library.md:101` | Keywords with no photos survive in `keyword.list` (likely, untested). | The writer may create an attribute-only keyword; a re-run must still see it. | **fact** — `keyword_tree` seeds every listed keyword before walking photos (`lightcraft:crates/catalog/src/keywords.rs:262-268`); `KeywordNode` carries every level (`:229-242`). | wrong data | medium |
| A12 | `docs/research/lightcraft-import.md:49` | `Interop/lightroom-index.json` is an internal format with no compatibility promise. | Discourages writing LightCraft's own index; lr2lc keeps its state file instead. | **fact (negative)** — the map is written by the importer (`lightcraft:crates/engine/src/lightroom_catalog.rs:211-215`, `:824-825`); no stability note found. lr2lc does not write it (`crates/lc-writer/src/state.rs`). | lost data | low |
| A13 | `docs/research/lightcraft-import.md:78` | It was unverified whether `library.import` with `keywords` applies them to duplicates already in the library. | If yes, a second run could change existing photos via `library.import` alone. | **fact: no** — duplicate items are returned in `report.duplicates` (`lightcraft:crates/engine/src/import.rs:1427-1446`); the `opts.keywords` loop only runs while a **new** photo record is built (`:1487-1490`). The writer never sends `keywords` with an import anyway (`crates/lc-writer/src/plan.rs:477`). | wrong data | medium |
| A14 | `docs/research/lightcraft-import.md:58` | "No writes in add mode" follows from documented behaviour, not an exhaustive audit of `import.rs`. | Principle 2 depends on it. | **partly fact** — mode semantics are documented and parsed (`lightcraft:crates/engine/src/import.rs:47-56`; `lightcraft:docs/lightroom-catalog-import.md:59`), and probing opens files read-only. A byte-level before/after check remains the belt-and-braces proof (L8). | lost data | high |
| A15 | `docs/research/lightcraft-develop.md:26` | White-balance writes are normalised against as-shot data, so a raw must be probed for Kelvin values to be read correctly. | Only matters once lr2lc maps develop settings itself. | **open** — inference from `lightcraft:crates/engine/src/cmd/develop.rs:197-200`; not run. | wrong data | low |
| A16 | `docs/research/lightcraft-develop.md:28` | `develop.merge` is the likely public command for an arbitrary partial-settings JSON (masks, spots). | Needed only if lr2lc stops using `library.importLightroom`. | **open** — the importer builds the partial (`lightcraft:crates/engine/src/crs.rs:202`, `:472-495`); which command accepts it was not confirmed. | only a worse message | low |
| A17 | `docs/research/label-names.md:25` | The English/German label names are the long-standing defaults but were not re-fetched from an Adobe URL. | They drive `--label-set auto`; a wrong name silently maps to no label. | **open** — the names are in `crates/model/src/label.rs` and guarded by `no_standard_name_maps_to_two_colors`; the three Adobe pages tried returned HTTP 403 (`label-names.md:52-63`). | wrong data | low |
| A18 | `docs/research/label-names.md:30` | French/Spanish names (Rouge…, Rojo…, Púrpura…) are the Lightroom defaults; no citable Adobe source exists. | They are deliberately not in the code; adding uncited names could mis-detect a label set. | **open** — candidates only (`label-names.md:36-50`); needs a `TranslatedStrings_<LOCALE>.txt` excerpt or a reachable Adobe page. | wrong data | medium |
| A19 | `docs/research/lrcat-schema-lrc15.md:66` | `lc_name` is presumably the lower-case form of `name`. | Synonym/keyword matching in the reader relies on the normalised column. | **open** — schema observation only; settle with a count-only query on a catalog copy (C2). | wrong data | medium |
| A20 | `docs/research/lrcat-schema-lrc15.md:69` | Several synonym rows per keyword exist, presumably one per synonym; row semantics are inferred from the schema. | The reader must join synonyms without multiplying photos. | **open** — no uniqueness on `keyword`, row semantics unconfirmed (`lrcat-schema-lrc15.md:63-70`); C2. | wrong data | medium |
| A21 | `docs/research/reconciliation.md:29` | The `always` flag means a command needs no selection and takes no selection gate. | Decides which read commands are safe without `library.select`. | **fact** — `always` returns `Ok(())` (`lightcraft:crates/engine/src/cmd/mod.rs:102-104`); only `explicit_targets` commands skip the gate (`lightcraft:crates/engine/src/lib.rs:561-563`). | only a worse message | medium |
| A22 | `docs/research/reconciliation.md:32` | No reply gives a modification time or a "who changed it" marker. | Three-way reconciliation can only work from lr2lc's base values. | **mostly fact** — `catalog.query` summaries have no timestamp (`lightcraft:crates/engine/src/cmd/query.rs:12-34`); `Photo.edited` exists but timestamps develop edits (`lightcraft:crates/catalog/src/model.rs:250`), not metadata fields. | wrong data | high |
| A23 | `docs/research/reconciliation.md:48` | The base may not equal what LightCraft holds if a command failed or was skipped. | A wrong base makes a re-run overwrite user edits or mis-report conflicts. | **partly fact** — a live read now exists (`crates/lc-writer/src/live.rs:1-60`) and `AppliedNotSaved` is handled (`crates/lc-writer/src/apply.rs:1229-1240`); the remaining gap is a command that was never sent. | wrong data | high |
| A24 | `docs/research/reconciliation.md:88` | LightCraft cannot re-point a photo to a moved/renamed file through any command found. | A moved original can only be reported, not fixed. | **open** — no relocate-like command was found; the research did not search exhaustively (`lightcraft-albums.md:273`). Check the command list live (L10). | only a worse message | high |
| A25 | `docs/research/reconciliation.md:94` | A re-run would not recreate a deleted album, and `album.addPhotos` on it fails. | Reconciliation must report "removed in LightCraft", never recreate silently. | **open** — the failure is plausible (`lightcraft:crates/catalog/src/lib.rs:846-848` errors on `NoAlbum`), but the re-run behaviour was not run. L9. | only a worse message | medium |
| A26 | `docs/research/reconciliation.md:115` | It was unverified whether `catalog.query {filter:{}}` is complete (Local-only and deleted photos). | The live reader uses exactly this filter; an incomplete read can report photos as missing. | **corrected fact** — `filter:{}` is **not** complete: `matches_in` excludes `local` photos and treats `deleted` as `false` unless asked (`lightcraft:crates/catalog/src/query.rs:353-360`). Deleted photos need `filter:{deleted:true}`; Local ones appear only in folder views. | wrong data | high |
| A27 | `docs/research/reconciliation.md:117` | LightCraft may normalise values on its own (trimmed titles, keyword case), causing false conflicts. | False conflicts would block writes on every run. | **partly fact** — keyword set values are cleaned (`lightcraft:crates/engine/src/cmd/library.rs:1091-1097`; `lightcraft:crates/catalog/src/keywords.rs:21-23`); title/caption are stored as given in the code read. Live check L6. | only a worse message | medium |
| A28 | `docs/research/reconciliation.md:118` | An XMP/metadata reload can change fields between runs. | A sidecar change could look like a user edit and block a write. | **open** — sidecars win at import (`lightcraft:crates/engine/src/import.rs:15-17`); whether the app reloads them outside import was not read. | wrong data | medium |
| A29 | `docs/research/reconciliation.md:88` | `PhotoEntry` has no path field (`state.rs:86-94`). | The note proposes storing the path as future work. | **stale** — `PhotoEntry` now stores `path: Option<String>` (`crates/lc-writer/src/state.rs:87-99`), so the proposed addition is done. | only a worse message | low |
| A30 | `docs/research/smart-collection-mapping.md:12` | `intersect`→`all`, `union`→`any`, `exclude`→`none` are not documented; only the first two were seen. | Wrong combine semantics changes every smart-collection membership. | **partly fact** — `intersect`/`union` are implemented (`crates/lr-reader/src/smart.rs:99-112`, `:383-403`); `exclude` was never seen and the reader must keep it unknown. Catalog check C3. | wrong data | high |
| A31 | `docs/research/smart-collection-mapping.md:21` | Lightroom rating operations `>`, `<`, `<=` exist (only `==` and `>=` were inventoried). | The translation allowlist is rating-only; a missed operator silently skips collections. | **open** — C3 histogram of `operation` strings. | only a worse message | medium |
| A32 | `docs/research/smart-collection-mapping.md:21` | Lightroom "unrated" and 0 stars are both 0 in the model. | `rating is 0` could match unrated photos that the user meant to exclude. | **open** — live check L11 on a throwaway library. | wrong data | high |
| A33 | `docs/research/smart-collection-mapping.md:22` | `pick` values are 1 / -1 / 0. | Enabling `pick` translation depends on it. | **open** — the note itself says "per task brief, not in sources"; C3. | wrong data | high |
| A34 | `docs/research/smart-collection-mapping.md:23` | `labelColor` stores the colour name, or the label-set text. | A custom label set would make a colour rule unmappable. | **open** — LightCraft accepts custom names too (`lightcraft:crates/catalog/src/rules.rs:413`, `:964-966`); C3. | wrong data | high |
| A35 | `docs/research/smart-collection-mapping.md:24` | Keyword operation names are `any`/`all`/`none`/`words` (only `any` and `empty` were inventoried). | Keyword rules are not translated; the open names could change that. | **open** — C3. | only a worse message | medium |
| A36 | `docs/research/smart-collection-mapping.md:25` | LightCraft's per-photo keyword list holds the same set as Lightroom's (no implied parents or hidden keywords). | `keywords isEmpty` would otherwise disagree. | **open** — LightCraft stores exactly the strings on the photo (`lightcraft:crates/catalog/src/model.rs:196`); the Lightroom side is unknown. L11. | wrong data | medium |
| A37 | `docs/research/smart-collection-mapping.md:26` | `value_units` for `captureTime inLast` are days/weeks/months/years. | Date rules are not translated until the units and arithmetic are known. | **open** — C3. | wrong data | medium |
| A38 | `docs/research/smart-collection-mapping.md:26` | The capture timestamp carries no time zone in either application. | Time-zone offsets would shift `inLast` boundaries. | **open** — schema/read check; C3 plus a live count. | wrong data | medium |
| A39 | `docs/research/smart-collection-mapping.md:26` | Lightroom's month/year arithmetic is unknown; LightCraft uses 30.4375/365.25 days. | Boundary photos differ between the two date maths. | **fact for LightCraft** — `rules.rs:806-807` (`lightcraft:crates/catalog/src/rules.rs`); Lightroom remains open. | wrong data | medium |
| A40 | `docs/research/smart-collection-mapping.md:27` | Lightroom date comparisons' inclusive/exclusive ends and day granularity are unknown. | `after`/`before`/`between` cannot be translated safely. | **open** — live check L11 with one photo on each boundary day. | wrong data | high |
| A41 | `docs/research/smart-collection-mapping.md:28` | `touchTime` means "last modified" in Lightroom; LightCraft's `editDate` is `p.edited`. | The two could mean different events. | **fact for LightCraft** — `editDate` reads `p.edited` (`lightcraft:crates/catalog/src/rules.rs:1025`); Lightroom's meaning is open. | wrong data | medium |
| A42 | `docs/research/smart-collection-mapping.md:29` | Lightroom `fileFormat` tokens are RAW/DNG/JPG/TIFF/PSD/VIDEO, and LightCraft's `format` string content is unchecked. | A format translation needs a two-way name table. | **open** — LightCraft `format` is a free string (`lightcraft:crates/catalog/src/model.rs:231`); C3. | wrong data | medium |
| A43 | `docs/research/smart-collection-mapping.md:30` | Lightroom's "contains" differs from "contains words"; LightCraft's `contains` is all-words, case-insensitive. | Text rules are not translated; enabling them on a false equivalence changes membership. | **fact for LightCraft** — `contains` splits at whitespace and requires every word (`lightcraft:crates/catalog/src/rules.rs:705-713`); the Lightroom distinction is open. | wrong data | high |
| A44 | `docs/research/smart-collection-mapping.md:30` | `title isEmpty` is likely equal but unverified (whitespace-only text). | Whitespace-only titles could match on one side only. | **partly fact** — LightCraft trims before `isEmpty` (`lightcraft:crates/catalog/src/rules.rs:705-711`); Lightroom is open. L11. | wrong data | low |
| A45 | `docs/research/smart-collection-mapping.md:45` | Lightroom keyword/text criteria are case-insensitive in the UI. | LightCraft compares lower-cased; a case-sensitive Lightroom would make rules unequal. | **open** — only LightCraft is proven (`lightcraft:crates/catalog/src/rules.rs:705-707`, `:954`). L11. | wrong data | medium |
| A46 | `docs/research/smart-collection-mapping.md:61` | LightCraft's `keywords` rule field holds names/paths only, not synonyms. | A Lightroom search that matches synonyms would not be reproducible. | **fact for LightCraft** — `names_op` runs over `m.keywords` (`lightcraft:crates/catalog/src/rules.rs:980`, `:722-729`); synonyms are separate keyword info. | wrong data | medium |
| A47 | `docs/research/smart-collection-mapping.md:61` | Lightroom's keyword search may match synonyms. | Same as A46 on the Lightroom side. | **open** — C3/L11. | wrong data | medium |
| A48 | `docs/research/smart-collection-mapping.md:36` | Lightroom `hasAdjustments` is a boolean. | The corresponding LightCraft `edited` means something different. | **open** — not translated; low value. | wrong data | low |
| A49 | `docs/research/lightcraft-albums.md:52` | `albums.list` nests children only up to a depth limit. | A collection set deeper than the limit would be read incompletely; the writer could create a duplicate parent. | **fact** — `MAX_ALBUM_DEPTH = 64`; children are listed only while `depth < 64` (`lightcraft:crates/engine/src/cmd/query.rs:39-46`), roots start at depth 0 (`:157-160`). Deeper nodes are silently absent from the reply. | only a worse message | medium |
| A50 | `docs/research/lightcraft-albums.md:50` | The stored album member order is not used to display members; there is no manual sort key. | lr2lc keeps `positionInCollection` order but must not promise it in the report. | **open** — only the sort-key list was read (`lightcraft:crates/catalog/src/query.rs:75-87`); live check L3. | only a worse message | medium |
| A51 | `docs/research/lightcraft-albums.md:60` | Unknown fields in `rules` are a parse error in `merge_filter`. | A bad rule must fail the album creation, never create a match-everything album. | **corrected** — `Filter` has no `deny_unknown_fields`, so unknown JSON keys are ignored (`lightcraft:crates/catalog/src/query.rs:19-21`); an unknown **rule field** becomes a `check_for` problem `unknown field …` (`lightcraft:crates/catalog/src/rules.rs:261`, `:1095`), and `createSmart` refuses to create the album (`lightcraft:crates/engine/src/cmd/library.rs:1162-1171`). The conclusion holds; the mechanism is different. | wrong data | medium |
| A52 | `docs/research/lightcraft-albums.md:65` | An empty rule set matches everything except under `match:any` (`all`/`none` true). | The writer always passes rules; a malformed empty group could select nothing. | **fact** — `if self.rules.is_empty() { return self.mode != Match::Any }` (`lightcraft:crates/catalog/src/rules.rs:1052-1054`). | wrong data | low |
| A53 | `docs/research/lightcraft-albums.md:145` | There is no "list all stacks" query command; only `photo.inspect` shows a stack. | Stack reconciliation needs one inspect per stacked photo. | **fact for 0.14.0** — no `query "stack…` spec exists in `lightcraft:crates/engine/src/cmd/organize.rs`; `photo.inspect` adds `stack` (`query.rs:166-172`). | only a worse message | medium |
| A54 | `docs/research/lightcraft-albums.md:157` | `album.setTarget` only sets session state and is not persisted. | The writer must set `null` before `toggleTarget`, and cannot rely on it after a restart. | **fact** — it assigns `s.target_album` and returns it, with no `Op` (`lightcraft:crates/engine/src/cmd/library.rs:1441-1450`). | wrong data | medium |
| A55 | `docs/research/lightcraft-albums.md:158` | The Quick Collection can only be found by its name because `albums.list` has no `quick` flag. | A user album with the same name would receive Quick Collection photos. | **fact for the flag** — `album_json` emits no `quick` key (`lightcraft:crates/engine/src/cmd/query.rs:47-54`); the name-collision consequence is open (L4). | wrong data | high |
| A56 | `docs/research/lightcraft-albums.md:192` | Saved web/print/book/slideshow collections are not `systemOnly`, so LightCraft's importer would turn them into albums. | lr2lc maps only `collection`/`group`/`smart_collection` and reports the rest. | **open** — LightCraft skips only `systemOnly` (`lightcraft:crates/engine/src/lightroom_catalog.rs:1026-1028`); the Lightroom values need a structure-only catalog check (C1). | wrong data | medium |
| A57 | `docs/research/lightcraft-albums.md:193` | `genealogy` is a path-like string of ancestor ids. | lr2lc currently uses `parent`, so this is informational. | **open** — the extractor doc says only "the hierarchy" (`lrcat-extractor:doc/lrcat_format.md:193`); C1. | only a worse message | low |
| A58 | `docs/research/lightcraft-albums.md:194` | Real `positionInCollection` values are floating-point-looking or lexicographic, not plain integers. | Reading and writing album order and stack order depend on the type. | **open** — LightCraft sorts it as text (`lightcraft:crates/engine/src/lightroom_catalog.rs:1045-1048`); the reader now orders by `positionInCollection` in SQLite (`crates/lr-reader/src/collections.rs:297-304`). C4. | wrong data | high |
| A59 | `docs/research/lightcraft-albums.md:194` | An empty `positionInCollection` should sort first; text sorting is acceptable. | Order of unpositioned members could differ from Lightroom. | **open / implementation choice** — C4 plus a live comparison. | only a worse message | low |
| A60 | `docs/research/lightcraft-albums.md:196` | The same name twice in one collection set is possible in Lightroom; the UI's uniqueness rule is unverified. | Duplicate names make a name-based re-find ambiguous; lr2lc uses ids. | **open** — C1 counts duplicates per parent. | wrong data | medium |
| A61 | `docs/research/lightcraft-albums.md:204` | It is unclear whether Lightroom keeps `AgLibraryCollectionImage` rows for smart collections. | If not, LightCraft's importer creates empty albums and lr2lc's count check is meaningless. | **open** — C6 counts rows per smart collection. | wrong data | high |
| A62 | `docs/research/lightcraft-albums.md:211` | `AgLibraryFolderStack` has columns `id_local, id_global, collapsed, text`. | The stack reader depends on the columns it actually uses (`crates/lr-reader/src/stacks.rs:53`, `:87`). | **open** — no source documents the table (`lrcat-schema-lrc15.md`); C7. | wrong data | high |
| A63 | `docs/research/lightcraft-albums.md:212` | `AgLibraryFolderStackImage` has columns `id_local, collapsed, image, position, stack`; `position = 1` is the top photo; collapsed repeats per member. | The stack reader uses `image`, `stack` and `position = 1` (`crates/lr-reader/src/stacks.rs:58`, `:103`). | **open** — C7. | wrong data | high |
| A64 | `docs/research/lightcraft-albums.md:212` | Collapsed state is stored per stack and presumably repeated per member. | Decides whether the collapsed flag is read once or per row. | **open** — C7 (count equality between the two tables). | only a worse message | low |
| A65 | `docs/research/lightcraft-albums.md:213` | A further table of folder-stack position entries may exist for sorting within a folder. | If it exists, stack order may come from there instead. | **open, no evidence** — C7 lists all tables matching `%Stack%`. | only a worse message | low |
| A66 | `docs/research/lightcraft-albums.md:220` | The Quick Collection's `creationId` and name in Lightroom Classic 15 are not established. | The reader must identify it among `systemOnly` rows; MVP includes the Quick Collection. | **open** — the extractor doc lists `.unsaved` ids but none for the Quick Collection (`lrcat-extractor:doc/lrcat_format.md:187-199`); C1. | wrong data | high |
| A67 | `docs/research/lightcraft-albums.md:222` | "Target collection" is stored in a preference, not in the catalog. | The reader must not look for it in a table. | **open** — no source shows it; low impact. | only a worse message | low |
| A68 | `docs/research/lightcraft-albums.md:243` | Chunking `album.addPhotos` at 500 ids keeps each call within the 60 s limit. | Large collections could exceed the reply limit. | **open / not implemented** — the writer sends all ids in one call (`crates/lc-writer/src/apply.rs:1080-1090`); no measurement exists. L3 with a large album. | only a worse message | medium |
| A69 | `docs/research/lightcraft-albums.md:46` | `SetAlbumPhotos` does not reject unknown photo ids. | The writer relies on its own map, so this only affects error handling. | **fact** — the op checks album existence and folder/smart status, not photo ids (`lightcraft:crates/catalog/src/lib.rs:846-857`). | wrong data | medium |
| A70 | `docs/research/lightcraft-albums.md:280` | Creating a deep keyword path implicitly creates its ancestor levels in the tree. | The writer creates only the full path, not each ancestor. | **fact** — `create_keyword_ops` writes one `SetKeyword` for the full path (`lightcraft:crates/catalog/src/keywords.rs:690-699`); the tree derives ancestor levels from the string (`:258-270`). | wrong data | medium |
| A71 | `docs/research/lightcraft-albums.md:16` | Photo commands are gated by the UI selection; they fail with `no photos selected` even when `ids` is passed, unless `library.select` ran first. | This is the difference between a working writer and one that fails on the first photo command. | **fact** — the gate is skipped only for `explicit_targets && names_photos` (`lightcraft:crates/engine/src/lib.rs:561-563`); `has_selection` reads only the selection (`lightcraft:crates/engine/src/cmd/mod.rs:108-110`). The writer now sends `library.select` (`crates/lc-writer/src/apply.rs:9`, `:1080`, `:1435`). | wrong data | high |
| A72 | `docs/research/lightcraft-albums.md:178` | The UI thread is blocked while `library.import` runs, so no frames are drawn. | A blocked UI could trigger the 5 s no-frame rejection for the next request. | **open** — the call chain was read (`lightcraft:crates/ui-egui/src/control.rs:329-333`); not run. L2/L12. | only a worse message | medium |
| A73 | `docs/research/smart-collection-mapping.md:32` | The `camera` criterion (`==`) may compare Lightroom's camera model rather than make + model, and LightCraft's field content was unread. | A camera rule is not translated today; enabling it on a false assumption would select the wrong photos. | **partly fact** — LightCraft's field is `Meta.camera` (`lightcraft:crates/catalog/src/rules.rs:1014`), built as Exif Make, a space and Model (`lightcraft:crates/engine/src/files.rs:18`, stored at `:20`; field `lightcraft:crates/catalog/src/model.rs:169-170`). What Lightroom compares remains open (C3/L11). | wrong data | medium |
| A74 | `docs/research/smart-collection-mapping.md:33` | Lightroom's `fileName` value may or may not include the file extension. | A literal `==`/`startsWith`/`endsWith` translation would match nothing or the wrong files if the two sides differ. | **partly fact** — LightCraft matches `Photo.file_name` (`lightcraft:crates/catalog/src/rules.rs:1004`), which carries the extension: `extension()` splits it off at the last dot (`lightcraft:crates/catalog/src/model.rs:455-459`). The Lightroom side is open (C3/L11). | wrong data | medium |
| A75 | `docs/research/lightcraft-albums.md:101` | The `stacked` bool is the counterpart of Lightroom's "Stacking state" criterion. | A stack rule can be translated only if both sides mean "is a member of a stack". | **partly fact** — `stacked` is true iff `catalog.stack_of(photo)` returns a stack (`lightcraft:crates/catalog/src/rules.rs:974`; `lightcraft:crates/catalog/src/stacks.rs:23`), i.e. LightCraft's own membership. Lightroom's criterion semantics were not read (C3/L11). Separate from A53, which is only the missing list-all-stacks command. | wrong data | medium |
| A76 | `docs/research/lightcraft-albums.md:195` | `AgLibraryCollectionCoverImage {collection, collectionImage}` exists in Lightroom 6 but is not in the LrC 15 schema document, so its presence there is unverified. | A cover reader would fail if the table is absent; a cover is not in the MVP. | **open** — no source in the reading list documents it for LrC 15; C5 checks the table name. | only a worse message | low |
| A77 | `docs/research/lightcraft-control-port.md:77` | It was unverified whether a keyword's synonyms can be set after creation (the rows at `keywords.rs:385-546` were not read). | Keyword attributes are in the MVP; a re-sync must be able to add or correct synonyms without recreating the keyword. | **fact** — `keyword.edit` accepts `synonyms?` and keeps omitted attributes (`lightcraft:crates/engine/src/cmd/keywords.rs:386-405`); `info_from` replaces the list only when the parameter is present (`:45-58`); `edit_keyword_ops` applies it (`lightcraft:crates/catalog/src/keywords.rs:735-755`). The writer deliberately reports changed attributes instead of editing (`crates/lc-writer/src/plan.rs:803-811`) and sends synonyms at creation (`:829-839`). | only a worse message | low |

## Live-check checklist

Run against a **throw-away library** with three throw-away image files. Start the
app with `lightcraft --control 7980` and drive it with
`lightcraft-cli run --connect 127.0.0.1:7980 <command> '<json>'`. Record only
numbers, booleans, key names and error codes. **Never** record a file name, a
path, a keyword, a title or any other value from the library or the catalog.
Write the answers into a scratch note, not into this repository.

The checks are ordered so that each one only needs state the previous one made.

| # | Command | Expected answer shape | Record |
|---|---|---|---|
| L1 | `library.info '{}'` | `{persistent:true, path, photos:N, albums:N, seq:N, unsavedOps:N}` | `persistent`, `photos`, `albums`, `seq` |
| L2 | `catalog.query '{"filter":{},"limit":1}'` then `photo.inspect '{"id":<that id>}'` | query: `{total:N, photos:[{id,fileName,rating,flag,label,title,keywords,deleted,copyOf,…}]}`; inspect: snake_case object with `source:{type:"file",path}`, `meta.keywords`, `content_hash`, `copy_of`, `stack?` | which keys exist; `source.type`; `total`; whether the next request within 5 s answered (A02, A07, A72) |
| L3 | `albums.list '{}'`; create one album folder, one child album, add 3 photos, repeat `album.addPhotos`, re-list | `[{id,name,folder,count,cover,children:[…]}]`; smart adds `smart,rules,rulesText,problems`; **no** `parent`, **no** `quick`; `album.addPhotos` → `{added,skipped}` with `added:0` on repeat | key names present/absent; `count`; `added` on repeat; whether member order matches insertion (A49, A50, A68) |
| L4 | select a photo, `album.toggleTarget '{"ids":[ID]}'`; `albums.list`; create a user album named exactly `Quick Collection` (do not record the name), `albums.list` | toggle → `{album,name,added,count,skipped}`; list shows two indistinguishable entries | `album` id; number of albums with that name; whether `quick` exists (A55) |
| L5 | `library.xmpPreferences '{}'`; `photo.rate '{"rating":1,"ids":[ID]}'`; list the library folder for sidecar files | `{autoWrite:bool, naming:"stem"\|"full"}`; count of `.xmp` files created | `autoWrite`, `naming`, sidecar count (A03) |
| L6 | `keyword.create '{"name":"LcT22Probe","parent":null}'`; repeat; `keyword.list '{}'`; `keyword.info '{"keyword":"LcT22Probe"}'`; `photo.setMeta '{"ids":[ID],"title":"  LcT22Probe  "}'` then `photo.inspect` | first `{keyword}`; second `ok:false` with an "exists" error; list is a **bare array** of `{name,path,count,children}`; info has `path,listed,count,synonyms,includeOnExport,exportContaining,exportSynonyms,person`; inspect title shows whether the spaces survived | `ok` of the repeat; error mentions existing; array shape; whether `title` is trimmed (A06, A11, A27) |
| L7 | import two files, `photo.delete` one, then `catalog.query '{"filter":{}}'`; `catalog.query '{"filter":{"deleted":true}}'`; `library.info` | first query `total` equals `photos` minus the deleted photo; second `total:1` | both totals; whether the deleted photo is absent from the first query (A26) |
| L8 | `library.import '{"paths":["…/a.jpg"],"mode":"add"}'`; repeat with `keywords:["LcT22Probe"]`; repeat after renaming the file and importing the new path | first `{imported:[N], duplicates:[], restored?, failed, scanned, sidecars, warnings?, album?}`; repeat `duplicates:[{path,existing,reason,existingDeleted?}]`; moved file: duplicate with `reason:"content"` | shape of `imported`; `reason`; whether the existing photo gained the keyword; wall time; hashes of the two originals before/after (A02, A08, A13, A14) |
| L9 | `album.delete '{"id":<album>}'`; repeat `album.addPhotos '{"id":<same>,"ids":[ID]}'` | `ok:false` with an album-missing error; nothing recreated | error code/text presence; album count unchanged (A25) |
| L10 | `album.addPhotos '{"id":<album>,"ids":[999999]}'`; rename a throwaway original on disk and `library.import` the new path; `photo.inspect` the old record | add: `{added,skipped}` or an error; import: duplicate with `reason:"content"` and `existing`; inspect: old `source.path` unchanged | `ok`, `added`; `reason`; whether the old path still points at the old name (A24, A69) |
| L11 | `album.createSmart` with an unknown rule field; with `rating is 0`; with `rating gte 1`; with an empty ruleSet and `match:"any"`; with `match:"all"`; `album.ruleFields '{}'` | unknown field: `ok:false`, error contains `unknown field`; rating rules: `{id,count}`; empty `any` → `count:0`, empty `all` → all photos; ruleFields lists fields with operators | `ok`; `count`s; field/operator names present (A32, A36, A40, A43, A45, A51, A52) |
| L12 | `stack.group '{"ids":[ID1,ID2],"top":ID1,"collapsed":false}'`; repeat; `photo.inspect` one member | `{stack,count}`; second call same `stack`; inspect adds `stack:{id,photos:[ids],position,collapsed}` | stack id stable; `count`; key names (A53, A72) |

For A68 (chunk size), extend L3 with an album of several thousand photos and
record the wall time of one `album.addPhotos` call; 60 s is the ceiling.

### Catalog structure checks (read-only copy, aggregate only)

Copy the `.lrcat` first; never open the original. Run with `sqlite3` in
read-only mode (`file:copy.lrcat?mode=ro`). Record **counts, type names, table
and column names only**. No value, path, name or keyword may leave the machine.

| # | Query | Expected shape | Record |
|---|---|---|---|
| C1 | `SELECT creationId, COUNT(*) FROM AgLibraryCollection GROUP BY 1;` plus `SELECT COUNT(*) FROM AgLibraryCollection WHERE systemOnly<>0;` plus duplicate names per parent: `SELECT COUNT(*) FROM (SELECT parent,name,COUNT(*) c FROM AgLibraryCollection GROUP BY 1,2 HAVING c>1);` | rows of `(creationId, count)`; one count; one count | which `creationId` strings exist and their counts; `systemOnly` count; duplicate-name groups (A56, A57, A60, A66) |
| C2 | `SELECT SUM(lc_name = lower(name)), COUNT(*) FROM AgLibraryKeyword;` and `SELECT COUNT(*) FROM (SELECT keyword FROM AgLibraryKeywordSynonym GROUP BY keyword HAVING COUNT(*)>1);` | two counts; one count | equality count, total keywords, keywords with >1 synonym (A19, A20) |
| C3 | Smart-collection inventory: `SELECT COUNT(*) FROM AgLibraryCollectionContent WHERE owningModule='ag.library.smart_collection';`; then a read-only script over `content` that prints a histogram of `criteria`, `operation`, `combine` and `value_units` **names** (reuse the repo parser or a small script; print names and counts only) | `(name, count)` rows | criterion names, operation names, combine names, unit names and counts (A30–A47, A73–A75) |
| C4 | `SELECT typeof(positionInCollection), COUNT(*) FROM AgLibraryCollectionImage GROUP BY 1;` plus `SELECT COUNT(*) FROM AgLibraryCollectionImage WHERE positionInCollection IS NULL OR positionInCollection='';` plus `SELECT COUNT(*) FROM AgLibraryCollectionImage WHERE positionInCollection GLOB '*[^0-9.]*';` | `(type, count)` rows; one count; one count | types present; empty count; non-numeric count (A58, A59) |
| C5 | `SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='AgLibraryCollectionCoverImage';` | 0 or 1 | table present (A76) |
| C6 | `SELECT COUNT(*) FROM AgLibraryCollection c JOIN AgLibraryCollectionImage i ON i.collection=c.id_local WHERE c.creationId LIKE '%smart%';` and the number of smart collections | two counts | whether smart collections have member rows (A61) |
| C7 | `SELECT name FROM sqlite_master WHERE type='table' AND name LIKE '%Stack%';`; `PRAGMA table_info(AgLibraryFolderStack);`; `PRAGMA table_info(AgLibraryFolderStackImage);`; `SELECT COUNT(*) FROM AgLibraryFolderStackImage WHERE position=1;` and `SELECT COUNT(*) FROM AgLibraryFolderStack;` | table names; column names; two counts | actual table/column names; whether each stack has exactly one position-1 row (A62–A65) |

## Decisions for the owner

1. **Keep the live checks out of CI.** They need a running app with a GUI on a
   machine that has a display; the Pi has none. They are a one-off owner task,
   not an automated test. Decide who runs them (see `QUESTIONS.md`).
2. **Do not enable any further smart-collection translation** (`pick`,
   `labelColor`, `keywords empty`, dates) before L11/C3 answer A32–A47 and
   A73–A75. This is
   already the code's strict-allowlist behaviour.
3. **Treat `albums.list` depth 64 as a report condition.** A collection set
   nested deeper than 64 levels is invisible to the reader; recommend a
   `depth_limit` warning in the report rather than silence (A49).
4. **Keep duplicate album names** (already decided in `lightcraft-albums.md:309`)
   and keep the id map as the identity source; A60 only changes the report.
5. **Correct the stale research notes** in a separate docs task; at minimum
   `reconciliation.md:88` (path now stored) and `lightcraft-albums.md:60`
   (mechanism of the unknown-field error). Do not rewrite them as part of T22.

## Open questions

1. Which smart-collection value vocabularies (A31–A47, A73–A75) does the owner's real
   catalog actually use? Only a structure-only histogram can answer it.
2. Is the Quick Collection in Lightroom Classic 15 a `systemOnly` row with a
   fixed `creationId`, and what is its name? (A66)
3. Do the stack tables and `position = 1` behave as assumed? The reader is
   already written against them, so this is the highest-risk open row (A62–A63).
4. Is `positionInCollection` numeric, text or mixed? (A58)
5. Is XMP auto-write off in the app's default new library, and does a sidecar
   reload change fields between runs? (A03, A28)
6. Does `library.import` on a slow volume stay within the 60 s reply limit, and
   does it block the UI thread long enough to reject the next request? (A02, A72)

## Recommendation

Treat the **31 fully settled rows** (fact, sourced, corrected or stale, without
the `partly` qualifier) as facts and fix the two stale notes; they no longer
need live checks. Only the **8 partly settled rows** keep the check named in
their evidence cell for the open part. Before the next smart-collection or
stack milestone, run the catalog structure checks C1–C7 on a copy (they are
cheap and answer the highest number of high-priority rows). Run the app checks L1–L12 once on a throw-away
library, record only shapes and counts, and close the rows they settle. Do not
add a public source or a "fact" mark for any row that still lacks a citation:
the two corrections (A26, A51) show that the intuition in the notes was not
always right.
