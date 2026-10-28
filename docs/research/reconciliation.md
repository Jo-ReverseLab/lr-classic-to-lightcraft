# Three-way reconciliation with LightCraft (groundwork for P5-9 and P5b-8)

Paths are relative to the lr2lc repo (`crates/…`, `docs/…`) or, where marked `LC:`, to the LightCraft source checkout (read only). Statements without file:line are marked *(assumption)*.

Goal: a re-run must only **add**, never overwrite silently. Three states per field: **base** (what lr2lc last wrote, in the mapping file), **Lightroom now** (reader), **LightCraft now** (read through the control port).

## 1. What can be read from LightCraft, and with which command

| Field | `catalog.query` summary | `photo.inspect` | Other |
|---|---|---|---|
| Stars | `rating` (`LC: crates/engine/src/cmd/query.rs:21`) | `rating` (whole `Photo` serialised, `query.rs:161-164`; field `crates/catalog/src/model.rs:243`) | |
| Pick/Reject | `flag`, lowercase `none\|pick\|reject` (`query.rs:22`; enum `model.rs:49-56`) | `flag` | |
| Colour label | `label`, lowercase name or null (`query.rs:23`; enum `model.rs:69-77`, field `:247`) | `label` | |
| Title | `title` (`query.rs:25`) | `meta.title` (`model.rs:182`) | |
| Caption | **not in summary** | `meta.caption` (`model.rs:183`) | |
| Copyright | **not in summary** | `meta.copyright` (`model.rs:187`) | |
| Creator | **not in summary** | `meta.creator` (`model.rs:195`) | |
| Keywords of a photo | `keywords` (`query.rs:26`) | `meta.keywords` (`model.rs:196`) | |
| Keyword tree | | | `keyword.list {}` → array of `{name,path,count,children}`; attributes via `keyword.info` (`docs/research/lightcraft-albums.md` A5, `LC: crates/engine/src/cmd/keywords.rs:313-354`) |
| Album members | `catalog.query {filter:{album:id}}` with paging (`query.rs:82-95`; `docs/research/lightcraft-albums.md` A1) | `albums: [album ids]` of one photo (`query.rs:164`) | `albums.list` gives tree, name, `folder`, `count`, smart `rules`, but **no members and no parent field** (`docs/research/lightcraft-albums.md` A1) |
| Photo existence / deleted | `deleted` (`query.rs:30`) | `deleted` | |

Consequences:

- Rating, flag, label, title and keywords of all photos come cheaply from paged `catalog.query` (default `limit` 200, `query.rs:91`; send `filter: {}` to list all, otherwise it lists the current UI view, `query.rs:82-89`). The summary needs no per-photo call.
- **Caption, copyright and creator need one `photo.inspect` per photo** (the summary lacks them). Only call it for photos where lr2lc has a base value for those fields or a Lightroom value to write; otherwise skip.
- All `photo.inspect`/`catalog.query` commands are `always` (read-only, no selection needed) in their registration (`query.rs:82`, `:161`) *(the `always` flag meaning is an assumption)*; they do not take the selection gate that `photo.*` writers have.
- The summary emits `title` as a plain string; an empty LightCraft title is `""`, while the mapping file stores `None` for empty (`crates/lc-writer/src/state.rs:62-68`). Normalise `""` → `None` before comparing.
- Keyword paths in `meta.keywords` are `a|b|c` text (`docs/research/lightcraft-control-port.md` §5), the same form as `PhotoFields.keywords` (`state.rs:70`). Comparison is case-insensitive in LightCraft (`docs/research/lightcraft-albums.md` A5, `keyword_path` lowercases); compare lower-cased *(design suggestion)*.
- Nothing in these replies gives a modification time or a "who changed it" marker *(assumption: none was found in the summary or `Photo` fields read)*; hence a base value in the mapping file is the only way to tell.

## 2. What the mapping file stores today, and what is missing

Stored (`crates/lc-writer/src/state.rs`):

- Photos: key = Lightroom global id; `PhotoEntry {lightroom_id, lightcraft_id, fields}` (`state.rs:86-94`). `PhotoFields` is exactly the "last imported value" per field: rating, flag, label, title, caption, copyright, creator, sorted keyword paths (`state.rs:54-72`). Saved after being written (`record`, `state.rs:291-306`).
- Keywords: path text → synonyms and the four attributes as created (`state.rs:98-136`).
- Collections: LightCraft album id, kind, name and parent *as created*, members lr2lc added (global ids), recorded smart `rules` (`state.rs:160-177`).
- Stacks and Quick Collection members (`state.rs:180-193`).

So the **base** exists for photo fields, keyword attributes, album name/parent/members and smart rules. `plan_photos` already compares Lightroom-now against base only (`crates/lc-writer/src/plan.rs:607-676`, `field_changes` `:858-897`).

Gaps for a three-way comparison:

1. **No read of LightCraft-now.** The plan never queries the live values; `photo.rate`, `photo.flag`, `photo.label` and `photo.setMeta` set absolute values (`plan.rs:858-873`), so if the user changed stars in LightCraft and Lightroom also changed since the last run, the LightCraft value is overwritten silently. If only Lightroom changed, this is correct. If Lightroom did **not** change, nothing is sent (`old == new`), so LightCraft-only edits survive today by accident; but they are not reported.
2. **Base may not equal "what LightCraft holds".** The base records what lr2lc *sent*; if a command failed or was skipped, the file may claim a value that LightCraft does not have *(assumption; the apply path was not re-read here)*. A live read removes this doubt.
3. **Keyword removal.** `removeKeywords` is sent for paths in base but not in Lightroom-now (`plan.rs:883-893`); it does not check whether the user removed or kept it in LightCraft. Keywords the user *added* in LightCraft are never touched (set difference only), which fits "only add".
4. **Album members**: base `members` is stored (`state.rs:170-172`), live members can be read (§1); the comparison is not implemented (only additions from Lightroom are made; removed members are counted as removed per `plan.rs` Quick/Stack handling, not deleted).
5. **Album name/parent**: base has name and parent (`state.rs:166-169`); `plan.rs:747-752` compares Lightroom against base only, and counts renamed/moved, with no live name from `albums.list`.
6. **Photos gone from Lightroom**: `plan_photos` iterates only the photos of the input (`plan.rs:607-632`), so a mapped photo that disappeared is not detected at all.
7. **Keyword rename**: keywords are keyed by path text (`state.rs:310-313`), so a rename in Lightroom looks like a new keyword plus an old one that is no longer used; there is no link between them. Keyword global id is stored (`state.rs:101`) and could serve as identity *(design suggestion)*.
8. Nothing records the last *reported* conflict, so the same conflict would be reported again on every run (acceptable; arguably desired).

Minimum additions to the file: none for photo fields (base exists). A `format version` bump is only needed if conflicts are stored (not recommended at first). Keyword global id → path is already present.

## 3. Conflict rules

Principle: only add, never overwrite without being asked. For each field, with `B` = base, `L` = Lightroom now, `C` = LightCraft now (all normalised):

| L vs B | C vs B | Action | Report |
|---|---|---|---|
| unchanged | unchanged | nothing | – |
| changed | unchanged (`C == B`) | **write** `L` to LightCraft, update base | counted as updated |
| unchanged | changed | **keep** `C`; do not change base | report "changed in LightCraft only" |
| changed | changed, `C == L` | nothing to write; update base to `L` | none (converged) |
| changed | changed, `C != L` | **conflict**: write nothing, base unchanged | report both values' *kind* (field, photo id), reason `conflict` |

Notes:

- No base (photo never recorded, e.g. file from an older run): treat as first import, base = `imported_default` (`state.rs:77-82`), as today. A photo that already existed in LightCraft before lr2lc (found as duplicate by path, `docs/research/lightcraft-control-port.md` §4) has an unknown base: use "write only where `C` is the default value (0 stars, no flag, no label, empty text); otherwise report as conflict" *(design suggestion; needs user decision, see Open)*.
- Keywords are a set per photo; reconcile per path: added in Lightroom (`∉B`, `∈L`) → add if absent; removed in Lightroom (`∈B`, `∉L`) → remove only if `C` still has it **and** the user opted in, else report; paths only in `C` → keep, report as "added in LightCraft". Removal is the one place where "only add" and "mirror Lightroom" collide, so default to **report only** (consistent with `docs/research/lightcraft-albums.md` C5).
- Album members: same set logic with base `members` (`state.rs:170`); additions from Lightroom are applied; removal in Lightroom is reported, not applied; extra members in LightCraft are kept and reported.
- Album name/parent: Lightroom renamed and `C.name == B.name` → `album.rename`/`album.move` (idempotent, `docs/research/lightcraft-albums.md` A1); `C` differs from `B` → keep and report (conflict if both changed). Parent of `C` cannot be read from `albums.list` directly; derive it from the tree nesting.
- The report must name only field, photo identifier (Lightroom id) and rule; not the values themselves *(design suggestion; keeps user data such as titles out of report files that might be shared)*. Values may appear in a `--verbose` console mode only *(open)*.
- Idempotence: running again without changes on either side yields `C == L` for every field, so no output.

## 4. Remaining scope of P5b-8 (removed and renamed)

Already counted today (`crates/lc-writer/src/apply.rs:130-138`, `plan.rs:166-173`, `:475-478`, `:747-764`): `collections_renamed`, `collections_moved`, `collections_removed`, `smart_no_longer_translatable`; also `quick_removed` (`plan.rs:287`) and `stack_members_removed` (`plan.rs:312`), `smart_rules_changed` (`plan.rs:229`). These compare Lightroom with base only and apply nothing for removal.

Missing:

| Item | State today | Needed |
|---|---|---|
| Photos removed from the Lightroom catalog | not detected (`plan.rs:607-632` iterates input only) | iterate `state.photos` not in input; report (`photos_removed`), optional later prune via `photo.delete` (recoverable, `docs/research/lightcraft-albums.md` C5) |
| Photo files moved/renamed | not detected | compare mapped path with new path; LightCraft cannot re-point via any command found *(assumption, `lightcraft-albums.md` C5)*; report only. Needs the base path stored (`PhotoEntry` has no path, `state.rs:86-94`): small file addition |
| Photo's membership removed from a collection | only counted as removed members? Not applied | report per album (needs the live-members comparison of §3) |
| Keywords removed from the catalog | keyword stays in state; no report | report `keywords_removed` (state keys without a Lightroom keyword) |
| Keywords renamed | looks like new + orphan | match by `global_id` (`state.rs:101`): same id, different path → `keyword.rename {from,to}` (`LC: keywords.rs:516-530`) if `C` still has the old path; else report |
| Keyword attributes changed | skipped with a reason (`plan.rs:12`, `KeywordEntry::same_attributes` `state.rs:129`) | optional `keyword.edit` (`lightcraft-albums.md` A5); only when LightCraft's `keyword.info` still equals base |
| Album/folder name or parent changed in LightCraft | not read | covered by §3 |
| Deleted in LightCraft (album deleted, photo in Recently Deleted) | not detected; a re-run would not recreate the album (state says known) and `album.addPhotos` fails *(assumption)* | read `albums.list` ids and `deleted`; report "removed in LightCraft", never recreate silently |

## 5. Proposed split of P5-9

**P5-9a – Read LightCraft state (no behaviour change).**
Add a read-only snapshot in `lc-writer`: `LiveState {photos: id → {rating, flag, label, title, keywords, caption?, copyright?, creator?}, albums: id → {name, parent, members}, keywords}` from paged `catalog.query {filter:{}}`, `photo.inspect` (only where needed), `albums.list`, `catalog.query {filter:{album}}`, `keyword.list`. Plan/apply unchanged.
Acceptance: unit tests against a fake control-port server for parsing (empty values, paging over 200, deleted photos, folders without members); `--dry-run` prints how many photos/albums were read; a read of N photos takes only the expected number of requests (counter in test); no write command is ever sent (test asserts).

**P5-9b – Three-way decision for photo fields and keywords.**
Pure function `decide(base, lightroom, live) -> Keep | Write | Converged | Conflict` per field and the set variant for keywords; `plan_photos` uses it; conflicts and "changed in LightCraft only" go into the report with codes (e.g. `conflict`, `changed_in_lightcraft`), numbers add up (principle 4).
Acceptance: table test of all rows of §3 for each of the 8 fields; keyword set cases (added, removed, extra in LightCraft); integration test with generated catalog and fake server: only-Lightroom change writes, only-LightCraft change writes nothing and is reported, both-changed writes nothing and reports a conflict; second run reports the same and sends no command; no overwrite occurs in any test (assert on command log).

**P5-9c – Albums, collections and the rest of P5b-8.**
Apply §3 to album name/parent/members, add removed-photo, removed-keyword and renamed-keyword detection (§4), store the photo path in the mapping file (version stays 1 with `#[serde(default)]` as done for later fields, `state.rs:206-218`), report only; no deletion.
Acceptance: each item of §4 has a test and a report code; renamed keyword by global id calls `keyword.rename` once and only if LightCraft still has the old path; album deleted in LightCraft is reported, not recreated; old mapping files without the new field still load; `read = converted + skipped` still holds.

## Open questions

1. Behaviour for photos already in LightCraft before the first lr2lc run (base unknown): treat defaults as writable and everything else as conflict, or ask? (User decision; also written to QUESTIONS.)
2. Should Lightroom-side removals (keyword removed from a photo, photo removed from an album) ever be applied, e.g. with an explicit `--prune`? Default here: report only.
3. May reports contain field values, or only field names and ids?
4. Is `catalog.query` with `filter: {}` complete (Local-only and deleted photos)? Not verified (`lightcraft-albums.md` A5 assumption).
5. Cost: `photo.inspect` per photo for caption/copyright/creator on large libraries; measure on a scratch library (60 s per request limit, `docs/research/lightcraft-control-port.md` §1).
6. Does LightCraft add or normalise values on its own (trimmed titles, keyword case), which would make `C != B` without a user edit? Needs a live test; compare normalised values to limit false conflicts.
7. Whether LightCraft's own XMP/metadata reload (sidecars win at import, `lightcraft-control-port.md` §4) can change fields between runs.

## Recommendation

Build P5-9 in the order a → b → c. Treat the mapping file's `PhotoFields` as the base, read LightCraft live (summary for most fields, `photo.inspect` only for caption/copyright/creator), and apply the five-row rule of §3: write only when LightCraft still equals the base, otherwise keep and report. Removals and renames stay report-only, with the single exception of keyword and album renames by stable id when LightCraft still holds the old name. Decide questions 1–3 before P5-9b starts.
