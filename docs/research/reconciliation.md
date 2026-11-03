# Three-way reconciliation with LightCraft (groundwork)

> **Status:** The read-only snapshot of the LightCraft state (step a below) and the three-way decision for photo fields and photo keywords (step b) are implemented (`crates/lc-writer/src/live.rs`, `crates/lc-writer/src/plan.rs:1353-1444`). The handling of albums, removed and renamed items (step c) is partly implemented: photos removed from the Lightroom model, moved photo files and renamed keywords are detected and reported without changing LightCraft (§4); the album rules of §3 are still planned. Today, collection renames, moves and deletions, and photos that left a stack or the Quick Collection, are only counted; photos that left a collection are removed from the album only when lr2lc had added them, and keywords removed in Lightroom are kept in LightCraft and only counted. *(Corrected 2026-11-04: an earlier version said a photo's keywords removed in Lightroom are removed from the photo; the code never did that — see §2 item 3.)* Planned, not implemented yet: only values that still have their default (0 stars, no flag, no label, empty text) may be overwritten in a photo that LightCraft already had before the first run; any other difference is reported as a conflict and nothing is written.

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

- Rating, flag, label, title and keywords of all photos come cheaply from paged `catalog.query` (default `limit` 200, `query.rs:91`; `filter: {}` lists the library instead of the current UI view, `query.rs:82-89` — but not Local or deleted photos, corrected 2026-11-04, `LC: crates/catalog/src/query.rs:353-360`). The summary needs no per-photo call.
- **Caption, copyright and creator need one `photo.inspect` per photo** (the summary lacks them). Only call it for photos where lr2lc has a base value for those fields or a Lightroom value to write; otherwise skip.
- All `photo.inspect`/`catalog.query` commands are `always` (read-only, no selection needed) in their registration (`query.rs:82`, `:161`) *(the `always` flag meaning is an assumption)*; they do not take the selection gate that `photo.*` writers have.
- The summary emits `title` as a plain string; an empty LightCraft title is `""`, while the mapping file stores `None` for empty (`crates/lc-writer/src/state.rs:62-68`). Normalise `""` → `None` before comparing.
- Keyword paths in `meta.keywords` are `a|b|c` text (`docs/research/lightcraft-control-port.md` §5), the same form as `PhotoFields.keywords` (`state.rs:70`). Comparison is case-insensitive in LightCraft (`docs/research/lightcraft-albums.md` A5, `keyword_path` lowercases); compare lower-cased *(design suggestion)*.
- Nothing in these replies gives a modification time or a "who changed it" marker *(assumption: none was found in the summary or `Photo` fields read)*; hence a base value in the mapping file is the only way to tell.

## 2. What the mapping file stores today, and what is missing

Stored (`crates/lc-writer/src/state.rs`):

- Photos: key = Lightroom global id; `PhotoEntry {lightroom_id, lightcraft_id, fields, path, copy_of}` (`state.rs:86-105`; `path` was added by T11 and is the comparison base for a moved file, `state.rs:94-98`). `PhotoFields` is exactly the "last imported value" per field: rating, flag, label, title, caption, copyright, creator, sorted keyword paths (`state.rs:54-72`). Saved after being written (`record`, `state.rs:306-332`).
- Keywords: path text → global id, Lightroom id, synonyms and the four attributes as created (`state.rs:109-128`).
- Collections: LightCraft album id, kind, name and parent *as created*, members lr2lc added (global ids), recorded smart `rules` (`state.rs:177-194`).
- Stacks and Quick Collection members (`state.rs:196-210`).

So the **base** exists for photo fields, keyword attributes, album name/parent/members and smart rules. `plan_photos` compares Lightroom-now against base and LightCraft-now for every mapped photo (`crates/lc-writer/src/plan.rs:853-993`, `reconcile` `:1353-1444`; without live state `field_changes` `:1286-1321` is used).

Gaps for a three-way comparison:

1. **No read of LightCraft-now.** *(Corrected 2026-11-04: this is no longer true. `read_live` reads `catalog.query`, `photo.inspect`, `albums.list` and `keyword.list` read-only (`crates/lc-writer/src/live.rs:72-141`), and `plan_photos` decides each photo field with `decide(last, lightroom, live)` (`plan.rs:1336-1346`, used at `:1353-1444`). Only mapped photos are inspected. Album members and `missing_photos`/`missing_albums` are read but not yet used by the plan.)* The historical gap: the plan never queried the live values; `photo.rate`, `photo.flag`, `photo.label` and `photo.setMeta` set absolute values, so if the user changed stars in LightCraft and Lightroom also changed since the last run, the LightCraft value was overwritten silently.
2. **Base may not equal "what LightCraft holds".** The base records what lr2lc *sent*; if a command failed or was skipped, the file may claim a value that LightCraft does not have *(assumption; the apply path was not re-read here)*. The live read removes this doubt for photo fields; a field whose live value differs from the base and from Lightroom is reported as a conflict and not written (`plan.rs:1332-1345`).
3. **Keyword removal.** *(Corrected 2026-11-04: the code never sends `removeKeywords`.)* Keyword memberships in base but not in Lightroom-now are counted in `keywords_removed` and kept in LightCraft (`plan.rs:295-300`, `:949-955`; the plan states "`removeKeywords` is never sent", `plan.rs:45`). Memberships the user *added* in LightCraft are kept (`plan.rs:1424-1435`), which fits "only add".
4. **Album members**: base `members` is stored (`state.rs:187-189`), and live members are read per mapped album into `LiveAlbum.members` (`lc-writer/src/live.rs:119-139`); the comparison is not implemented yet — the plan does not read `LiveState.albums` at all (only additions from Lightroom are made; removed members are counted per `plan.rs` Quick/Stack handling, not deleted).
5. **Album name/parent**: base has name and parent (`state.rs:184-186`); `plan.rs:747-752` compares Lightroom against base only, and counts renamed/moved. `read_live` now reads the live album name (`live.rs:132-138`) but the plan does not use it.
6. **Photos gone from Lightroom.** *(Corrected 2026-11-04: detected by T11. `plan_removed_and_moved` iterates the mapping file, not only the input, and reports each mapped photo missing from the Lightroom model as `photos_removed` / `removed_photos`; the photo is taken only out of albums lr2lc created (`plan.rs:1017-1048`, `:1224-1260`).)* The historical gap: `plan_photos` iterated only the photos of the input, so a mapped photo that disappeared was not detected at all.
7. **Keyword rename.** *(Corrected 2026-11-04: renamed keywords are detected by global id and reported by T11; nothing is renamed in LightCraft.)* `renamed_keywords` matches a wanted keyword's global id against the recorded one and reports every changed path text as `RenamedKeyword {keyword: <Lightroom id>}`, descendants included (`plan.rs:763-803`); a renamed keyword is never created and never added to a photo (`plan.rs:827-829`, `:995-1015`). The historical gap: keywords are keyed by path text (`state.rs:216`, accessors `:426-440`), so a rename looked like a new keyword plus an old one that is no longer used.
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

## 4. Remaining scope (removed and renamed)

Already counted today (`crates/lc-writer/src/apply.rs:130-138`, `plan.rs:166-173`, `:475-478`, `:747-764`): `collections_renamed`, `collections_moved`, `collections_removed`, `smart_no_longer_translatable`; also `quick_removed` (`plan.rs:287`) and `stack_members_removed` (`plan.rs:312`), `smart_rules_changed` (`plan.rs:229`). These compare Lightroom with base only and apply nothing for removal.

Missing:

| Item | State today | Needed |
|---|---|---|
| Photos removed from the Lightroom catalog | **done (T11):** detected and reported, nothing deleted (`plan.rs:1017-1048`, `:1224-1253`); was "not detected" | optional later prune via `photo.delete` (recoverable, `docs/research/lightcraft-albums.md` C5) |
| Photo files moved/renamed | **done (T11):** compared with the recorded `PhotoEntry.path` and reported, not imported again (`plan.rs:1034-1043`, `state.rs:94-98`); was "not detected" and "`PhotoEntry` has no path" | LightCraft cannot re-point via any command found *(assumption, `lightcraft-albums.md` C5)*; report only |
| Photo's membership removed from a collection | only counted as removed members? Not applied | report per album (needs the live-members comparison of §3) |
| Keywords removed from the catalog | keyword stays in state; no report | report `keywords_removed` (state keys without a Lightroom keyword) |
| Keywords renamed | **partly done (T11):** detected by `global_id`, same id with a different path text is reported (`plan.rs:763-803`); still looks like new + orphan in the plan and nothing is renamed in LightCraft | `keyword.rename {from,to}` (`LC: keywords.rs:516-530`) if `C` still has the old path; otherwise report |
| Keyword attributes changed | skipped with a reason (`plan.rs:12`, `KeywordEntry::same_attributes` `state.rs:146-152`) | optional `keyword.edit` (`lightcraft-albums.md` A5); only when LightCraft's `keyword.info` still equals base |
| Album/folder name or parent changed in LightCraft | not read | covered by §3 |
| Deleted in LightCraft (album deleted, photo in Recently Deleted) | not detected; a re-run would not recreate the album (state says known) and `album.addPhotos` fails *(assumption)* | read `albums.list` ids and `deleted`; report "removed in LightCraft", never recreate silently |

## 5. Proposed split

**Step a – Read LightCraft state (no behaviour change).**
Add a read-only snapshot in `lc-writer`: `LiveState {photos: id → {rating, flag, label, title, keywords, caption?, copyright?, creator?}, albums: id → {name, parent, members}, keywords}` from paged `catalog.query {filter:{}}`, `photo.inspect` (only where needed), `albums.list`, `catalog.query {filter:{album}}`, `keyword.list`. Plan/apply unchanged.
Acceptance: unit tests against a fake control-port server for parsing (empty values, paging over 200, deleted photos, folders without members); `--dry-run` prints how many photos/albums were read; a read of N photos takes only the expected number of requests (counter in test); no write command is ever sent (test asserts).

**Step b – Three-way decision for photo fields and keywords.**
Pure function `decide(base, lightroom, live) -> Keep | Write | Converged | Conflict` per field and the set variant for keywords; `plan_photos` uses it; conflicts and "changed in LightCraft only" go into the report with codes (e.g. `conflict`, `changed_in_lightcraft`), numbers add up (principle 4).
Acceptance: table test of all rows of §3 for each of the 8 fields; keyword set cases (added, removed, extra in LightCraft); integration test with generated catalog and fake server: only-Lightroom change writes, only-LightCraft change writes nothing and is reported, both-changed writes nothing and reports a conflict; second run reports the same and sends no command; no overwrite occurs in any test (assert on command log).

**Step c – Albums, collections and the rest of the removed and renamed items.**
Apply §3 to album name/parent/members, add removed-photo, removed-keyword and renamed-keyword detection (§4), store the photo path in the mapping file (version stays 1 with `#[serde(default)]` as done for later fields, `state.rs:206-218`), report only; no deletion.
Acceptance: each item of §4 has a test and a report code; renamed keyword by global id calls `keyword.rename` once and only if LightCraft still has the old path; album deleted in LightCraft is reported, not recreated; old mapping files without the new field still load; `read = converted + skipped` still holds.

## Open questions

1. Behaviour for photos already in LightCraft before the first lr2lc run (base unknown): treat defaults as writable and everything else as conflict, or ask? (Open decision.)
2. Should Lightroom-side removals (keyword removed from a photo, photo removed from an album) ever be applied, e.g. with an explicit `--prune`? Default here: report only.
3. May reports contain field values, or only field names and ids?
4. ~~Is `catalog.query` with `filter: {}` complete (Local-only and deleted photos)? Not verified (`lightcraft-albums.md` A5 assumption).~~ **Answered 2026-11-04:** no. `Filter::matches_in` rejects every photo whose `deleted` differs from the filter's `deleted` (default `false`) and, without a folder filter, also every `local` photo that is not deleted (`LC: crates/catalog/src/query.rs:353-360`). So `filter: {}` lists neither Local nor deleted photos; deleted ones need `filter:{deleted:true}`. `read_live` uses `filter: {}` (`crates/lc-writer/src/live.rs:82-87`) and therefore cannot see photos in Recently Deleted.
5. Cost: `photo.inspect` per photo for caption/copyright/creator on large libraries; measure on a scratch library (60 s per request limit, `docs/research/lightcraft-control-port.md` §1).
6. Does LightCraft add or normalise values on its own (trimmed titles, keyword case), which would make `C != B` without a user edit? Needs a live test; compare normalised values to limit false conflicts.
7. Whether LightCraft's own XMP/metadata reload (sidecars win at import, `lightcraft-control-port.md` §4) can change fields between runs.

## Recommendation

Build it in the order a → b → c. Treat the mapping file's `PhotoFields` as the base, read LightCraft live (summary for most fields, `photo.inspect` only for caption/copyright/creator), and apply the five-row rule of §3: write only when LightCraft still equals the base, otherwise keep and report. Removals and renames stay report-only, with the single exception of keyword and album renames by stable id when LightCraft still holds the old name. Decide questions 1–3 before Step b starts.

*(2026-11-04: steps a and b are implemented as described; step c is partly done (§4). The remaining work is the album part of §3 — live album name and members are already read but unused — and reading deleted photos with `filter:{deleted:true}` before claiming that a mapped photo is gone; see question 4.)*
