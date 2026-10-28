# LightCraft albums, stacks, Quick Collection and reconciliation (for phase 5b)

> **Status:** These are research notes written before the code. Writing collections as albums and album folders, folder stacks, the Quick Collection and smart albums (strict rating rules only) is implemented in `crates/lc-writer`; `library.select` before photo commands and the array form of `keyword.list` are in place. Reading the live LightCraft state is implemented too. Comparing it with Lightroom and the mapping file (conflict handling) is still open, see `reconciliation.md`. Statements marked as assumptions were not re-verified.

Research for collections → albums and album folders, smart collections → smart albums, stacks, the Quick Collection, reconciliation of removed and renamed items, and the open points of the writer (`crates/lc-writer/src/{control,plan,apply}.rs`).

Conventions, as in `lightcraft-control-port.md`:

- Paths are relative to the LightCraft source root (the read-only LightCraft checkout, commit `bb08a69`) unless prefixed `lrcat-extractor:` (the read-only lrcat-extractor checkout, MPL-2.0; read for understanding only, no code or text taken over).
- Claims without a file:line are marked *(assumption)*.
- Nothing here comes from a real catalog. The Lightroom side (section B) rests on source code and the schema document only; whatever the sources do not say is listed under "Open questions".
- Everything that LightCraft returns through the control port is the `result` of `engine.execute` (`docs/control-protocol.md:7-9`, `crates/ui-egui/src/control.rs:329-334`). The command names below are the `params.command` values.

## 0. Summary of the findings that affect the writer

1. **Selection gate (important, affects existing writer code).** `Session::execute` skips the "is the command enabled" check only for commands marked `explicit_targets` when the call names photos (`crates/engine/src/lib.rs:561-563`; `crates/engine/src/cmd/mod.rs:66-70`, `:192-194`). Only the develop commands (`crates/engine/src/cmd/develop.rs:301`) and `stack.toggle` (`crates/engine/src/cmd/organize.rs:193`) have the flag. `photo.rate`, `photo.flag`, `photo.label`, `photo.setMeta`, `album.addPhotos`, `stack.group` and others are gated by `has_selection` (`crates/engine/src/cmd/library.rs:804`, `:837`, `:912`, `:1038`; `crates/engine/src/cmd/organize.rs:135`) or `has_library_selection` (`library.rs:1376`, with the rule at `cmd/mod.rs:108-116`). With an empty selection they fail with `no photos selected` even when `ids` is passed. The writer never sends `library.select` (no occurrence in `crates/lc-writer/src`). It only works today by accident: after `library.import` the first imported photo is selected (`library.rs:1655-1657`). Fix: before each photo command send `library.select {ids:[…], active:<first id>}` (`library.rs:678-720`, gate `always`; every id must exist, else error and selection unchanged, `:691-693`). `album.addPhotos` needs at least one selected photo that is not Local (`cmd/mod.rs:113-116`).
2. **`library.import` is synchronous, `imported` is a flat id list, `id` is echoed.** See A6.
3. **`keyword.create` on an existing keyword is an error**, also when the keyword exists only as an ancestor of a keyword some photo uses. `keyword.list` returns a bare array. See A5.
4. **Duplicate album names are allowed** (also in the same folder); `album.addPhotos` is idempotent; `stack.group` is idempotent for the same arguments; `album.toggleTarget` is not. See A1, A3, A4.
5. **No dedicated "list members of an album" command.** `albums.list` returns counts only. Members come from `catalog.query` with `filter.album`, which returns summaries without file paths, sorted by a sort key, not in the album's stored order. See A1, A5.
6. LightCraft's own Lightroom importer already maps collections to albums, folders by `creationId` containing `group`, and imports smart collections as plain albums with their current membership (`crates/engine/src/lightroom_catalog.rs:1026-1090`). It does not read stacks (no occurrence in `crates/engine/src/lightroom_*.rs`).

## A. LightCraft side

### A1. Albums and album folders

| Command | Parameters | Result | Source |
|---|---|---|---|
| `album.create` | `{name, parent?: folderId, folder?: bool, addSelected?: bool}` | `{id, skipped}` | `crates/engine/src/cmd/library.rs:1127-1149` |
| `album.rename` | `{id, name}` | `{}` (`ok()`) | `library.rs:1246-1251` |
| `album.move` | `{id, parent?: folderId \| null}` | `{}` | `library.rs:1291-1296` |
| `album.addPhotos` | `{id, ids?}` | `{added, skipped}` | `library.rs:1372-1399` |
| `album.removePhotos` | `{id?, ids?}` | `{removed, album}` | `library.rs:1558-1589` |
| `album.delete` | `{id}` (descendants too; photos stay) | `{}` | `library.rs:1253-1290` |
| `albums.list` | `{}` | nested tree, see below | `crates/engine/src/cmd/query.rs:155-160`, `:41-63` |
| `album.reorder`, `album.sort`, `album.setCover` | ordering and cover | | `library.rs:1298`, `:1348`, `:1590` |

**Create an album.** `name` is trimmed; a missing name becomes `Untitled Album`; an empty name is an error `empty name` (`library.rs:1135-1137`). `parent` is read with `as_u64`: anything that is not a number is silently treated as "top level" (`library.rs:1140`). The engine, not the command, checks the parent: it must be an existing album with `folder = true`, else `parent must be an existing folder` (`crates/catalog/src/lib.rs:785-789`). The id is allocated by the engine and returned (`library.rs:1141`, `:1148`). `ids` is not a parameter of `album.create`; `addSelected: true` would add the current UI selection (`library.rs:1139`), so **never pass it**; add members with `album.addPhotos`.

**Create an album folder (for Lightroom collection sets).** `album.create` with `folder: true` (`library.rs:1138`, `:1144`). Nested folders: create the outer folder first, pass its id as `parent` of the inner folder. A folder holds albums, not photos (`crates/catalog/src/model.rs:522-524`; `Op::SetAlbumPhotos` refuses photos for folders and smart albums, `crates/catalog/src/lib.rs:846-854`). `parent` accepts only folders; an album cannot contain albums.

**Duplicates.** There is **no check for equal names**: neither `Op::AddAlbum` (`crates/catalog/src/lib.rs:781-798`) nor `Op::RenameAlbum` (`lib.rs:810-813`) compares names, and `album.create` allocates a new id each call (`library.rs:1141`). So two albums with the same name in the same folder are allowed, and **`album.create` is not idempotent**: calling it twice creates two albums. Consequence for the writer: (a) an earlier decision is not needed to satisfy LightCraft; it is a lr2lc design choice to keep names distinguishable for the idempotence index (Open question 2); (b) the writer must remember `Lightroom collection id → LightCraft album id` itself (like the photo index) or re-find albums by (parent, name) via `albums.list` before creating. With duplicate names allowed, re-finding by name is ambiguous, so the id mapping file is the reliable key (LightCraft's own importer does the same with `Interop/lightroom-index.json`, `lightroom_catalog.rs:1030-1037`, `lightcraft-library.md:91`).

**`ids` and the selection.** `album.addPhotos` takes `ids`; without `ids` it uses the UI selection (`library.rs:1380`). It is **not** `explicit_targets`, so the selection must be non-empty and contain a non-Local photo (section 0, point 1). The engine's own import path works around this by selecting the first imported photo before calling it (`library.rs:126-129`, comment "album.addPhotos wants a selection"), which confirms the gate.

**Add photos.** Smart albums and folders are refused (`smart albums and folders can't hold photos`, `library.rs:1382-1384`). Local photos (browsed, not in the library) are skipped and counted in `skipped`; a call with only Local photos is an error (`library.rs:153-167`). Photos already in the album are not added twice (`library.rs:1387-1393`): **idempotent**, `added` is the number of new members. New members are appended in the order given (`library.rs:1390-1391`). Unknown photo ids are not checked here *(the `SetAlbumPhotos` arm, `catalog/src/lib.rs:846-856`, checks only that the album exists and is not a folder or smart album, so unknown photo ids are **not** rejected there; confirm in a live test)*; pass only ids returned by LightCraft. The first member becomes the cover when the album has none (`library.rs:1394`).

**Remove photos.** `album.removePhotos {id, ids}`: only takes the photos out of the album (`library.rs:1558-1589`). Gate is `has_selection` (`library.rs:1564` region) so the same selection rule applies. Refuses smart albums and folders.

**Order of members (positionInCollection).** `Album.photos` is a `Vec<PhotoId>` in insertion order (`model.rs:526`; `album.addPhotos` appends, `library.rs:1390-1391`). The available sort keys are capture date, import date, edit date, file name, rating, file size and random (`crates/catalog/src/query.rs:75-87`); there is **no "manual order" key**, so the stored order is not used to display members *(assumption: not verified against the UI code; only the sort keys were read)*. Album-level manual order exists for albums among each other (`Album.order`, `model.rs:536-539`, `album.reorder`), not for photos in an album. Recommendation: still add members in Lightroom's `positionInCollection` order (cheap, and the order is kept in the library if LightCraft ever uses it), but do not promise the order in the report.

**List albums.** `albums.list {}` returns an array of top-level albums, each `{id, name, folder, count, cover, children:[…]}` and, for smart albums, `smart:true, rules, rulesText, problems` (`query.rs:41-63`). Children are nested up to a depth limit (`query.rs:36-46`). There is **no `parent` field** (the parent is the enclosing node), **no member list**, and **no `quick` flag**: the Quick Collection appears as an ordinary album named `Quick Collection` (`library.rs:1414`, creation in `album.toggleTarget`), indistinguishable from a user album of that name *(the Album struct has the `quick` flag, `model.rs:534-535`, but `album_json` does not output it, `query.rs:47-54`)*. `count` for a regular album is the number of members; for a smart album it is the live match count (`catalog/src/lib.rs:514-517`).

**Read members of one album.** `catalog.query {filter:{album:<id>}, sort?, offset?, limit?}` → `{total, photos:[summary]}` (`query.rs:82-95`). `Filter.album` is a field of the filter (`crates/catalog/src/query.rs:45`). The summary has `id, fileName, format, kind, width, height, captured, rating, flag, label, edited, title, keywords, camera, deleted, copyOf, copyName, previewOnly` (`query.rs:12-34`), **no path**. The default `limit` is 200 (`query.rs:91`), so page with `offset`. The order is the sort order, not the stored member order. Alternative: `photo.inspect {id}` returns `albums: [album ids]` for one photo, including smart albums (`query.rs:164`, `catalog/src/lib.rs:436-439`).

**Empty albums.** Allowed: `album.create` without members is the normal path (`library.rs:1141-1146`), and `album.removePhotos` may leave an album empty. Lightroom collections with no photos can therefore be mapped one to one.

### A2. `album.createSmart`

`{name, rules?, parent?: folderId}` → `{id, count}` (`library.rs:1151-1171`; description string at `:1155`). Without `rules` it saves the **current view**, so **always pass `rules`**. `rules` is a partial `Filter` (`crates/catalog/src/query.rs:20-73`) merged onto an empty filter (`library.rs:1741-1761`, called at `:1163`). Problems in the rule set are reported as an error with positions like `rule 2.1: …` (`rules.rs` `Problem`, display at `:155-160`); the album is then not created. Unknown fields in `rules` are a parse error in `merge_filter` *(assumption: not read)*. Each call creates a new album (`library.rs:1165-1169`): **not idempotent**, same remedy as in A1.

**Rule format.** Two layers, combined with AND:

1. Flat `Filter` fields (`query.rs:20-73`): `text`, `rating`, `ratingOp` (`atLeast|exactly|atMost`, `query.rs:11-17`), `flag`, `label`, `labels`, `kind`, `edited`, `album`, `date` (prefix), `libraryFolder`, `keyword`, `person`, `camera`, `lens`, `dateFrom`, `dateTo`, `imported`, `importedFrom`, `merged`, `folder`, `subfolders`, `only`, `onlyNone`, `deleted` (refused for smart albums, `catalog/src/lib.rs:528-530`). Of these `album` must not name another smart album (`lib.rs:531-535`).
2. `ruleSet`: `{match: "all"|"any"|"none", rules:[{field, op, value} | {group: ruleSet}]}`, nested groups allowed (`crates/catalog/src/rules.rs:12-38`). Evaluation: `all` = every rule, `any` = at least one, `none` = no rule (`rules.rs:1054-1059`). An empty rule set matches everything (`rules.rs:311`, `query.rs:311`) *(assumption on exact behaviour: `rules.rs:1054` returns `self.mode != Match::Any` for an empty list, i.e. true for `all` and `none`, false for `any`)*. `album.ruleFields {}` lists the fields with operators at runtime (`library.rs:1173-1195`), so the writer can check the live field list.

**Operators by kind** (`rules.rs:602-637`):

| Kind | Operators |
|---|---|
| text | `contains`, `notContains`, `is`, `isNot`, `startsWith`, `endsWith`, `isEmpty`, `isNotEmpty` |
| keywords / people | `contains`, `notContains`, `is`, `startsWith`, `isEmpty`, `isNotEmpty` |
| number | `is`, `isNot`, `gte`, `lte`, `gt`, `lt`, `between` (value `[a,b]`) |
| date | `is`, `after`, `before`, `between`, `inLast`, `notInLast`, `isEmpty` |
| choice, album | `is`, `isNot` |
| bool | `is` |

**Case.** Text comparison lower-cases both sides: `have` is lower-cased in `text_op` (`rules.rs:705-707`) and `want` by `lower(value)` (`rules.rs:957`). The module header says so too (`rules.rs:3-4`). So all text rules are **case-insensitive**.

**`contains` is "all words".** `contains` splits the wanted text at whitespace and requires every word to occur (`rules.rs:708`); `notContains` is "none of the words" (`:709`). Lightroom's "contains" / "contains words" distinction is therefore not reproduced one to one *(assumption about Lightroom)*.

**Keywords and sub-keywords.** Keywords are stored as `a|b|c` path strings (`rules.rs:722-724`, `keywords.rs:32-36`). In a rule set, for a keywords field:

- `contains` is a substring test on each full path, so `travel` also matches `travel|italy` and also `xtravel` (`rules.rs:708`, `:728`). Includes sub-keywords, but with false positives.
- `is` matches the whole path equal to the value **or any one level** of a path (`rules.rs:728`), so `is italy` matches `travel|italy|rome` (descendant of the named level) but also any other keyword that has a level called `italy` elsewhere in the tree, and it matches a **parent** level, `is travel` matches `travel|italy`. It does not match a multi-level path as a prefix (`is travel|italy` matches only that exact keyword).
- The flat `Filter.keyword` is strict: a keyword matches if it equals the value or lies below it (`is_under`, `keywords.rs:32-36`; use at `query.rs:450-452`). This is the only exact "keyword including descendants" test, for one keyword at a time and AND-combined with everything else.

So, for the strict reading (exact equivalents only): Lightroom "keywords contain X (including sub-keywords)" maps exactly only to the flat `keyword: "<full path>"` field; any/none or several keywords have no exact counterpart (`is` and `contains` have false positives, see above) and should go into the report.

**All fields** (`rules.rs:60-126`; the list of 46 matches the list). Field, kind, and whether it could correspond to a Lightroom smart-collection criterion *(Lightroom column is from general knowledge of Lightroom's criteria and unverified, see Open questions)*:

| Group | Field (kind) | Possible Lightroom counterpart |
|---|---|---|
| top | `rating` (number) | Rating (`is/≥/≤/>/<`) – direct |
| top | `flag` (choice pick/reject/none) | Pick flag – direct (`flagged`/`unflagged`/`rejected`) |
| top | `label` (choice red/yellow/green/blue/purple/none) | Color label – only the five colours (see the colour label sets in the README) |
| top | `text` (text) | Searchable text – only loosely (different field set) |
| Source | `album` (album id) | "Collection" criterion – only via id mapping, and the target album must exist first |
| Source | `virtualCopy` (bool) | Copy status (virtual copy) |
| Source | `copyName` (text) | Copy name |
| Source | `stacked` (bool) | Stacking state *(assumption)* |
| File | `fileName` (text) | File name |
| File | `extension` (text) | File extension |
| File | `filePath` (text) | Folder / file path – paths differ after path mapping, not safely translatable |
| File | `kind` (choice image/raw/video) | File type |
| File | `format` (text) | File format (RAW/DNG/JPEG) |
| File | `duration` (number) | Video duration |
| Date | `captureDate`, `importDate`, `editDate` (date) | Capture date, Added date, Edit date |
| Keywords | `keywords` (keywords), `keywordCount` (number) | Keywords, keyword count *(Lightroom "has keywords" yes/no)* |
| Keywords | `person`, `personCount` | People (face names) – no Lightroom 1:1 |
| Description | `title`, `caption`, `altText`, `creator`, `copyright`, `copyrightStatus` | Title, Caption, Creator, Copyright, Copyright status |
| Camera | `camera`, `lens` (text), `focalLength`, `aperture`, `shutterSpeed`, `iso` (number) | Camera, Lens, Focal length, Aperture, Shutter speed, ISO |
| Location | `location`, `city`, `state`, `country` (text), `hasGps` (bool) | Location fields, GPS |
| Size | `longEdge`, `shortEdge`, `megapixels` (number), `aspect` (choice) | Dimensions, aspect ratio |
| Develop | `edited` (bool), `cropped` (bool), `treatment` (choice color/monochrome) | Develop: has adjustments, cropped, treatment |
| Culling | `sharpness`, `bestOfGroup` | none (LightCraft specific) |

**Strict reading (recommendation).** Translate only criteria where operator and value semantics are provably the same:

- exact: `rating` (all operators), `flag` (`is`), `label` (`is` for the five colours), numeric camera fields with `is/gte/lte/between`, `captureDate` with `after/before/between/is` *(date semantics in Lightroom, e.g. inclusive/exclusive end, not verified, so prefer a live test first)*, `kind`, `virtualCopy`, `edited` ;
- conditional (report with reason): keyword rules (see above), text rules with `contains` (word-split semantics), `fileName`/`filePath`/`folder` (path mapping), `album` (requires id mapping and refuses smart-album references in the flat field);
- everything else, plus any rule whose Lightroom criterion or operator is unknown: skip the **whole smart collection** with reason, because a rule set with a dropped rule would silently change the result. (A group with `match: any` and one untranslatable rule is wrong without it; with `all` it is a superset. Both change membership.)

**Order of operations.** A smart album in `ruleSet` can reference another smart album by id (`rules.rs:320-336`, `Kind::Album`), but the flat `album` field cannot (`catalog/src/lib.rs:531-535`). Loops are refused (`rules.rs:328-330`). Create regular albums and mapping first, then smart albums.

### A3. Stacks

| Command | Parameters | Result | Source |
|---|---|---|---|
| `stack.group` | `{ids?, top?: photoId, collapsed?: bool (default true)}` | `{stack, count}` | `crates/engine/src/cmd/organize.rs:128-153` |
| `stack.ungroup` | `{ids?}` | `{ungrouped}` | `organize.rs:155-158` |
| `stack.remove` | `{ids?}` (take photos out) | `{changed}` | `organize.rs:159-162` |
| `stack.setTop` | `{id?}` | `{changed}` | `organize.rs:163-167` |
| `stack.moveUp/moveDown` | `{id?}` | `{changed}` | `organize.rs:168-177` |
| `stack.split` | `{id?}` | `{changed}` | `organize.rs:178-192` |
| `stack.toggle` | `{ids?, collapsed?}` | | `organize.rs:193-211` (`explicit_targets: true`) |
| `stack.expandAll`, `collapseAll` | `{}` | | `organize.rs:212-219` |

**Model.** `Stack {id, photos: Vec<PhotoId>, collapsed}`; `photos[0]` is the top, at least two members (`crates/catalog/src/model.rs:20-29`; `crates/catalog/src/stacks.rs:38-52` `validate_stack`: at least two, no duplicates, photos must exist, a photo is in at most one stack).

**`stack.group`.** The order in the new stack is: `top` first, then `ids` in the given order, de-duplicated and restricted to photos that exist (`stacks.rs:53-60`). `top` defaults to the active photo if it is among the targets, else the first id (`organize.rs:138-143`). A call with fewer than two resulting photos is an error `select at least two photos` (`organize.rs:148`; `stacks.rs:79`). Stacks that any of these photos are already in are **merged**: the other members of those stacks are appended in their order; the first touched stack keeps its id, others are removed (`stacks.rs:61-98`). `collapsed` applies to the resulting stack, default **true** (`organize.rs:131`, `:149`). `ids` is optional, but the gate is `has_selection` (`organize.rs:135`), so a **selection is required** (section 0, point 1): send `library.select` first. The command also replaces the selection with the top photo afterwards (`organize.rs:151`).

**Idempotence.** Re-running with the same `ids` and `top` computes the same order and issues `SetStack` on the existing stack with identical content (`stacks.rs:90-94`), so the state does not change; no error. With a different `top` or order the stack is **re-ordered** (top first, then given order, then former extra members). With a subset of an existing stack, the stack keeps the extra members at the end (they are appended from the touched stack, `stacks.rs:70-77`). Hence, for "Lightroom stack = exact set" the writer should compare `photo.inspect.stack.photos` first, and call `stack.group` only if different.

**Read state.** `photo.inspect {id}` adds `stack: {id, photos:[ids in order], position, collapsed}` when the photo is stacked (`query.rs:166-172`). There is no "list all stacks" query command in the files read *(assumption; `stack.*` queries other than `photo.inspect` were not found in `organize.rs`)*; `catalog.stats` does not list them (`query.rs:98-111`).

**Collapsed / expanded.** Settable at group time (`collapsed`), later with `stack.toggle {ids, collapsed}` (explicit targets, no selection needed, `organize.rs:193-211`). Not mapping the state is thus a choice, not a limit: it could be mapped with a single parameter. Default `true` would collapse every imported stack, so if lr2lc does not map the state, pass the value explicitly.

**Virtual copies.** `photo.virtualCopy` already stacks the copy with its original (`organize.rs:110`, `:93-97`). A Lightroom stack that contains a master and its virtual copy therefore may already exist in LightCraft after the copy is created; `stack.group` merges into it rather than failing. `stacked` is also a smart rule field (`rules.rs:73`).

**Photos in Recently Deleted** are removed from stacks (`library.rs:169-180`), so stack building should skip deleted photos.

### A4. Quick Collection

- Model: a regular album with `quick: true`, "one per library" (`model.rs:533-535`; lookup `catalog/src/lib.rs:431-433`). Nothing creates it except `album.toggleTarget`: it adds an album named `Quick Collection` with `quick: true` when none exists and no target album is set (`library.rs:1412-1418`). **There is no command to create or find it explicitly.**
- `album.toggleTarget {ids?}` (`library.rs:1402-1440`): target = the target album if one is set (`album.setTarget`), else the Quick Collection. If **all** given photos are already in it, they are **removed**; otherwise the missing ones are appended. Returns `{album, name, added, count, skipped}` (`library.rs:1436`). **Not idempotent** (second identical call removes). Gate `has_library_selection` (selection rule, section 0).
- `album.setTarget {id | null}`: `null` means the Quick Collection (`library.rs:1441-1454`); it only sets session state (`s.target_album`), not persisted *(assumption: field is a session field, not part of the catalog; the command has no `Op`)*. A set target album changes what `toggleTarget` does, so the writer must call `album.setTarget {id:null}` first, otherwise photos would go into a user-chosen target album.
- Reading members: `albums.list` shows it as album `Quick Collection` with `count` but without a `quick` flag (see A1); members via `catalog.query {filter:{album:<id>}}`. To find its id without the flag: it is the album named `Quick Collection` *(assumption; breaks if the user has another album of that name; `photo.inspect.albums` has the same limit)*. Alternative: after `album.toggleTarget` the result gives `album` (its id) directly (`library.rs:1436`), but that call changes state.
- Safe add procedure (the writer's): (1) `album.setTarget {id:null}`; (2) read members of the existing Quick Collection if found; (3) compute photos missing; (4) `library.select` them and `album.toggleTarget {ids: missing}`; because none of them is in it, they are all added. Never send photos that are already members (they would be removed when all are in it). For a mixed batch (some in, some out) the call adds the missing ones and keeps the others (`library.rs:1424-1428`: `all_in` false → append those not held), so even a mixed batch is safe; only a batch entirely inside removes. Sending only the missing ones avoids the case.
- `album.clearQuick` empties it (`library.rs:1455-1462`): do not use.

### A5. Reading commands for reconciliation and the writer's open questions

**`catalog.query`** (`query.rs:82-95`): `{filter?: Filter, sort?: Sort, offset?, limit?}` → `{total, photos:[summary]}`. Without `filter`/`sort` it lists the **current view** (what the UI shows, `:85-89`); to list all photos send `filter: {}`. Default `limit` 200. Summary fields: see A1; **no path**, no Lightroom id. Whether `filter: {}` includes Local-only and deleted photos is not read *(assumption: Local photos are excluded by the default query, see `Photo::in_library`; not checked in `query.rs:…`; test)*. `deleted` summary field exists, so deleted ones may appear with `filter.deleted`.

**`photo.inspect {id}`** (`query.rs:161-175`): serialises the whole `Photo` with serde defaults, i.e. **snake_case** field names (the test reads `preview_only`, `crates/engine/src/files.rs:2590`; `Photo` has no `rename_all`, `model.rs:224-285`), then adds `albums`, `history` (labels) and, when stacked, `stack`. The file path is in `source`: `Source` is an internally tagged enum (`#[serde(tag="type", rename_all="lowercase")]`, `model.rs:88-94`), so for a file photo: `"source": {"type": "file", "path": "<string>"}` *(shape derived from the serde attributes, not seen in a real reply)*. Other useful fields: `file_name`, `rating`, `flag`, `label`, `meta` (with `keywords`), `deleted`, `local`, `copy_of`, `copy_name`, `content_hash` (`model.rs:225-285`). Path comparison is by exact string; the engine's duplicate check keys by path string (`import.rs:824-827`), so give it the same absolute path.

**Path → id map.** Needs one `photo.inspect` per photo (ids from `catalog.query` paging). There is no bulk path listing. Cheaper for files being imported: `library.import` returns `duplicates[].{path, existing, reason}` for known files (`import.rs:182-192`), so "import then read duplicates" doubles as a lookup (`reason` is `"path"` or `"content"`; `existingDeleted` if in Recently Deleted).

**`keyword.list {}`** (`crates/engine/src/cmd/keywords.rs:313-315`): `result` is a **bare JSON array** of nodes (`keyword_tree()` serialised directly, `crates/catalog/src/keywords.rs:245`), each node `{"name": <level name>, "path": <full `a|b|c` path>, "count": <photos below, not deleted>, "children": [nodes]}` (`catalog/src/keywords.rs:232-241`, struct `KeywordNode` derives `Serialize` with no renames). Sorted by name case-insensitively; levels are one per lower-case path with the spelling first written. Top-level array = roots. It contains every level (ancestors too), whether the keyword is "listed" or only used by a photo (`keywords.rs:262-268`: listed keywords first, then photos). Answers writer open question 3: the writer should accept **only the array form** (`[{name,path,count,children}]`); the other two assumed forms do not exist in this version. For attributes (synonyms, flags) use `keyword.info {keyword}` → `{path, listed, count, synonyms, includeOnExport, exportContaining, exportSynonyms, person}` (`keywords.rs:335-354`); `listed` says whether the keyword has its own list entry.

**`keyword.create` on an existing keyword** (writer open question 4): **error**, not update. `create_keyword_ops` returns `KeywordExists` when `has_keyword(path)` (`catalog/src/keywords.rs:695-697`), and the command turns it into `there is a keyword “<path>” already` (`crates/engine/src/cmd/keywords.rs:355-384`, message built at `:69`). `has_keyword` is true for a keyword that is listed **or used by a library photo, itself or one below it** (`catalog/src/keywords.rs:611-615`). So: (a) creating `travel` when `travel|italy` is in use fails; (b) the check is case-insensitive (`keyword_path` lowercases, `lib.rs:544-550`); (c) it is not an update. To change attributes of an existing keyword use `keyword.edit {keyword, name?, synonyms?, includeOnExport?, exportContaining?, exportSynonyms?, person?}` ("what isn't given keeps its value", `keywords.rs:386-405`; also refuses a name that is taken, `:398`). Detect "already exists" either before (`keyword.list`/`keyword.info.listed`) or by treating this error message as "exists" and moving on. The error is a plain string; there are no error codes (`lightcraft-control-port.md` §1). A subtle point: `keyword.create` with `parent` absent places the keyword inside the **default parent** (`keywords.rs:363-368`); always send `parent` explicitly (`null` for top level).

**`albums.list`**: see A1.

### A6. Open questions 1, 2, 7 of `lightcraft-control-port.md`

1. **Does `library.import` run synchronously through the control port?** Yes at the engine level: the command body calls `import_with` inline, then `import_batch_done` and returns the report (`crates/engine/src/cmd/library.rs:1633-1660`; `import_with` runs `prepare` and `commit_prepared` in the same call, `crates/engine/src/import.rs:800-806`). The control port runs `engine.execute` via `app.run(...)` → `session.execute(...)` (`crates/ui-egui/src/control.rs:329-333`; `crates/ui-egui/src/lib.rs:850-875`) on the UI thread, so the reply comes after the import. It is therefore subject to the 60 s reply limit (`apps/lightcraft/src/control_server.rs:70`); a started request still gets the full 60 s (`docs/control-protocol.md:19`). Background import with progress is a separate UI path (`ui.inspect.import`); not used here. Residual risk: the UI thread is blocked during the import, no frames are drawn *(assumption)*; a one-file import is short. **Live check recommended** only to confirm timing on slow volumes.
2. **Shape of `imported`.** A flat array of **photo ids (unsigned integers)**: `ImportReport.imported: Vec<u64>` (`import.rs:195-197`); the command reads it back as such (`library.rs:1639-1641`, `as_u64`). It is not `{path,id}`. Pushed in processing order (`import.rs:1425`, `:1500`). Elements are not paired with paths, so for a batch the pairing is not given; **import one file per call** (as the writer already plans, an earlier decision) or read the id back by `duplicates`/`photo.inspect`. Full result: `{imported:[id], duplicates:[{path, existing, reason, existingDeleted?}], restored?, moved?, kept?, failed:[[path, error]], scanned, sidecars, warnings?, album?}` (`import.rs:182-217`; `album` set by `import_batch_done`, `library.rs:134-137`). `failed` entries are two-element arrays `[path, error]` (tuple serialisation, `import.rs:209`).
3. **Is `id` echoed?** Yes: the server stores `obj.get("id")` of the request (`apps/lightcraft/src/control_server.rs:167`) and puts it into the reply (`:216`); the test asserts `{"id": id, "ok": true, "result": …}` (`:329`). A request without `id` gets `null` (`:167`). The writer may rely on matching ids.

**Additional finding for the writer:** import also sets the selection to the first imported photo (`library.rs:1655-1657`); with `restored` ids when nothing new was imported (`:1648-1654`). Not relied upon: see section 0.

## B. Lightroom side (schema)

Sources: `docs/research/lrcat-schema-lrc15.md` (structure observed earlier on a Lightroom Classic 15 catalog: names and types only), lrcat-extractor (`lrcat-extractor:doc/lrcat_format.md`, `src/collections.rs`, `src/content.rs`, `src/lron.rs`; read for understanding), LightCraft's own importer (`crates/engine/src/lightroom_catalog.rs`). Nothing was read from a real catalog in this task. What none of these sources show is marked.

### B1. Collections and collection sets

- `AgLibraryCollection`: `id_local, creationId, genealogy, imageCount, name, parent, systemOnly` (`lrcat-schema-lrc15.md`, "Other tables read by lrcat-extractor"; the extractor selects `id_local, genealogy, name, parent, systemOnly`, `lrcat-extractor:src/collections.rs:71-75`). `parent` is the `id_local` of the parent set; NULL/0 means top level (`lrcat-extractor:doc/lrcat_format.md:196`; the extractor treats an unreadable parent as 0, `src/collections.rs:34`; LightCraft treats 0 as none, `lightroom_catalog.rs:1062`, `:1048-1056`).
- `creationId` values documented by lrcat-extractor (Lightroom 3 to 6): `com.adobe.ag.library.group` (collection set), `com.adobe.ag.library.collection` (regular collection), `com.adobe.ag.library.smart_collection` (smart collection), plus `com.adobe.ag.webGallery`, `com.adobe.ag.print.unsaved`, `com.adobe.ag.webGallery.unsaved`, and a slideshow `.unsaved` entry (misspelled in the source) (`lrcat-extractor:doc/lrcat_format.md:187-195`). LightCraft's importer does not compare exact strings: **folder if `creationId` contains `group`, smart if it contains `smart`** (`lightroom_catalog.rs:1063-1064`). Whether Lightroom Classic 15 uses the same `creationId` strings, and which other values exist (e.g. for print, book, slideshow, web collections; these are **not** photo collections), is **not established** by these sources: Open question.
- Exclusion of non-photo containers: LightCraft skips rows with `systemOnly != 0` (`lightroom_catalog.rs:1026-1028`). The extractor notes `systemOnly` applies to the Quick Collection and the `.unsaved` entries (`lrcat-extractor:doc/lrcat_format.md:199`). `webGallery`, print, book and slideshow collections that are saved (not `.unsaved`) are *not* `systemOnly`, so LightCraft would import them as albums *(assumption)*; lr2lc should map only `collection` / `group` / `smart_collection` by exact `creationId` and report others as skipped with reason.
- `genealogy` is a path-like string of ancestor ids *(assumption; `lrcat-extractor:doc/lrcat_format.md:193` says only "the hierarchy")*; the `parent` column is enough. LightCraft guards against cycles and depth (`lightroom_catalog.rs:267-273`).
- `AgLibraryCollectionImage`: `id_local, collection, image, pick, positionInCollection` (`lrcat-schema-lrc15.md`; `lrcat-extractor:doc/lrcat_format.md:201-207`). `image` is `Adobe_images.id_local`. `positionInCollection` is **not a plain integer**: LightCraft sorts it as **text** (`lightroom_catalog.rs:1045-1048`; its test uses the strings `"a"`, `"b"`, `:1135-1136`). Real values are a floating-point-looking or lexicographic position *(assumption)*; sort as text, or as number if all parse, and let an empty value sort first *(assumption)*.
- Cover images: `AgLibraryCollectionCoverImage {collection, collectionImage}` exists in Lr6 (`lrcat-extractor:doc/lrcat_format.md:209-212`); not in the schema document for LrC 15, so unverified there. LightCraft has `album.setCover` (A1) but a cover is not in scope for the MVP.
- Same name twice in one set is possible in Lightroom *(assumption: Lightroom enforces unique names within a set in the UI; unverified)*.
- Virtual copies are rows of `Adobe_images` with their own `id_local` (`lrcat-schema-lrc15.md`, `masterImage`), so `AgLibraryCollectionImage.image` can name a copy; the neutral model must map copy ids too.

### B2. Smart collections

- Where: the rules are **not** columns of `AgLibraryCollection`. They are a row in `AgLibraryCollectionContent` (`id_local, collection, content, owningModule`) with `owningModule = "ag.library.smart_collection"`; the extractor reads `content` of that row with `lron::Object::from_string` (`lrcat-extractor:src/content.rs:77-103`; table layout `lrcat-schema-lrc15.md`). Other `owningModule` keys in the same table: `com.adobe.ag.library.filter`, `.sortType`, `.sortDirection` (`content.rs:86-98`): view settings, not membership (`lrcat-extractor:doc/lrcat_format.md:84-107`).
- Format: Lightroom's own text notation "lron" (Lua-table-like: `name = { key = value, … }`, nested braces, strings, integers, floats, booleans; `lrcat-extractor:src/lron.rs:8-31`, value kinds at `:46-53`). The extractor's test file shows the shape of a smart collection: an outer table holding a list of criterion tables, each with `criteria`, `operation`, `value` and `value2`, plus a `combine` key at the outer level, and an embedded escaped sub-object in a further key (`lrcat-extractor:data/test_lron`, a synthetic sample). So per rule: criterion name + operator + one or two values; combination across rules by `combine`. The values of `combine` are *(assumption from general knowledge, not in the sources: `intersect` = all, `union` = any, `exclude` = none)*; the sample only shows `intersect`.
- Which criteria are common, and the exact value vocabulary (e.g. which `operation` strings exist, how keyword criteria express "any / all / none" and sub-keywords): **not in the sources**. Needs an inventory on the real catalog (Open question, structure-only: a histogram of `criteria`/`operation` names and their counts, no values; to be done by the katalog-tester role).
- LightCraft itself does not translate them: smart collections become plain albums with the **current membership** read from `AgLibraryCollectionImage` (`lightroom_catalog.rs:1064-1075`; `docs/lightroom-catalog-import.md:105-107`). Whether Lightroom keeps `AgLibraryCollectionImage` rows for a smart collection is unclear (the importer assumes it can read members; with no rows the album would be empty) *(Open question)*.
- Sibling tables: none needed (`AgLibraryCollectionContent` has `collection` only, `lrcat-schema-lrc15.md`).

### B3. Stacks

**No source in the reading list documents stack tables.** A search for `Stack` in lrcat-extractor, in LightCraft's Lightroom importer (`crates/engine/src/lightroom_*.rs`) and in `docs/research/lrcat-schema-lrc15.md` finds nothing. The following is from general knowledge of Lightroom Classic catalogs and **unverified**; confirm structurally on a real catalog (table and column names only) before use:

- `AgLibraryFolderStack`: one row per stack *(assumed columns: `id_local, id_global, collapsed, text`; `text` is used as an in-stack marker/label)*.
- `AgLibraryFolderStackImage`: membership *(assumed columns: `id_local, collapsed, image, position, stack`)*; `image` → `Adobe_images.id_local`, `stack` → `AgLibraryFolderStack.id_local`, `position` = order within the stack, **position 1 is the top photo** *(assumption)*. Collapsed state is stored per stack, presumably also repeated per member *(assumption)*.
- Related: a table of folder-stack position entries may exist for sorting within a folder *(assumption, no evidence)*.

Neutral-model implications: a stack needs the ordered list of member image ids; the first in `position` order is the top; a stack of one photo should be reported, not written (LightCraft needs two, `stacks.rs:38-41`). A photo in two stacks is impossible in LightCraft (`stacks.rs:49-51`), so a conflict must be reported.

### B4. Quick Collection

- Stored as a row in `AgLibraryCollection` with `systemOnly != 0` (`lrcat-extractor:src/collections.rs:35` (flag), `doc/lrcat_format.md:199`); members in `AgLibraryCollectionImage` like any collection. For Lightroom 2 it is the `AgLibraryTag` row of kind `AgQuickCollectionTagKind` (`lrcat-extractor:src/collections.rs:41-53`, `doc/lrcat_format.md:216-221`); irrelevant for LrC 15.
- How to tell it from the `.unsaved` entries and which `creationId` it has in LrC 15: **not established** by the sources (the format document lists `.unsaved` ids but no id for the Quick Collection; `lrcat-extractor:doc/lrcat_format.md:187-199`). Name: the extractor falls back to the string `Quick Collection` only for Lightroom 2 when the name is missing (`src/collections.rs:40-47`); LrC 15 name is unknown *(Open question)*.
- LightCraft's importer **skips** all `systemOnly` rows (`lightroom_catalog.rs:1026-1028`), so the native importer drops the Quick Collection.
- The Quick Collection may also be the *target collection* marker in the catalog; where "target collection" is stored (a preference, not a table) is not shown in any source *(assumption: preference file, not catalog)*.

## C. Recommendations per task

### C1. Collections → albums / album folders

Mapping:

| Lightroom | LightCraft |
|---|---|
| collection set (`creationId` ends in `group`) | `album.create {name, folder:true, parent?}` |
| collection (`…library.collection`) | `album.create {name, parent?}` + `album.addPhotos {id, ids}` |
| `parent` | `parent` = LightCraft id of the mapped set (create parents first, ordered by depth as LightCraft does, `lightroom_catalog.rs:1049-1058`) |
| `AgLibraryCollectionImage.image` | LightCraft photo id via the existing photo index |
| `positionInCollection` | order of `ids` in `album.addPhotos` (kept only as a courtesy, A1) |

Rules:

- Skip with reason: `systemOnly` rows, web/print/book/slideshow, unknown `creationId`; members whose image is not converted (counted per collection); collections whose set is skipped.
- Idempotence: keep `Lightroom collection id_local (or id_global) → LightCraft album id` in the mapping file (same file as photos). On a rerun: id known and still present in `albums.list` → only `album.addPhotos` (idempotent) for missing members; id known but album gone (deleted by the user in LightCraft) → report, do not recreate (reconciliation); id unknown → create. Never find by name.
- Duplicate names: LightCraft allows them (A1), so lr2lc may keep names as they are. Adding a suffix such as " (2)" is therefore only a readability choice; recommend **not** renaming (less loss of information) and listing duplicates in the report as a notice. Ask the user (QUESTIONS, point 2).
- Photo commands need `library.select` first (section 0). Group `album.addPhotos` calls in chunks (the whole id list at once is linear, `library.rs:1386-1393`), but keep each call within the 60 s limit; chunk size 500 *(assumption)*.
- Empty collections: create them (empty albums are allowed).
- Neutral model draft: `model::Collection { id: CollectionId(i64), name, parent: Option<CollectionId>, kind: CollectionKind::{Set, Regular, Smart, Quick}, members: Vec<PhotoRef> /* in position order */ }`, plus `Report` entries for skipped ones. Keep `Smart` and `Quick` as kinds so one reader pass serves P5b-3/4/6.

### C2. Smart collections → smart albums

- Reader: parse `AgLibraryCollectionContent` rows with `owningModule = "ag.library.smart_collection"` into a neutral `SmartRule { criterion, operation, value, value2 }` list with a `combine` mode; keep unknown criteria as raw names so the report can name them (names only, not values, to keep user data out of the report text *(design suggestion)*). The `lron` text needs a small parser written fresh (no code from lrcat-extractor, MPL-2.0): Lua-table-like grammar from `lrcat-extractor:src/lron.rs:8-53`.
- Writer: `album.createSmart {name, parent?, rules:{…}}` with `rules` limited to the exactly equivalent set of A2 (rating, flag, label, numeric camera data, kind, virtual copy, edited, keyword **only** as flat `keyword` with full path). Everything else: skip the whole collection with the reason `criterion "<name>" has no exact LightCraft equivalent`. Create after all regular albums (rule `album` needs ids). After creation compare the live count `count` from `album.createSmart` with Lightroom's current membership count when the latter is readable, and warn on mismatch.
- Idempotence: same mapping file; a re-run must not call `album.createSmart` again for a known id; for rule changes use `album.setRules {id, rules, replace:true}` (`library.rs:1197-1245`) *(optional; first version: report only)*.
- Fallback that matches LightCraft's own importer: if no rule is translatable, optionally create a **regular** album with the current membership (read from `AgLibraryCollectionImage`), clearly marked in the report (user choice). Recommendation: do not do this silently.
- Model: `Collection.kind = Smart` plus `smart: Option<SmartRules>`; `SmartRules { combine: All|Any|None, rules: Vec<SmartRule> }`.

### C3. Stacks

- Reader (needs the structural confirmation of B3): `model::Stack { id, members: Vec<PhotoRef> /* top first */, collapsed: bool }` (collapsed read and counted; mapping it is one parameter, A3).
- Writer: per stack: map members to LightCraft ids; drop unconverted members (report); fewer than two left → skip with reason; check `photo.inspect.stack` of the top photo: equal member list in equal order → nothing to do; else `library.select {ids}` then `stack.group {ids, top, collapsed}`. Pass `collapsed` explicitly. If a member is already in another stack (e.g. by a virtual copy auto-stack, or the user), `stack.group` merges stacks; report this as a notice instead of failing.
- Order of operations: after photos **and virtual copies** exist.
- Idempotence: A3 (same arguments leave the state unchanged).

### C4. Quick Collection

- Reader: members of the `systemOnly` quick collection (once identified, B4).
- Writer: procedure in A4, in short: `album.setTarget {id:null}`, find existing `Quick Collection` album via `albums.list` (by name, no flag), read members via `catalog.query {filter:{album}}`, send only missing ids to `album.toggleTarget {ids}`; if the album does not exist yet it is created by that very call. Report: count added, count already in, count skipped.
- Caveats: (1) `albums.list` has no `quick` flag; a user album named `Quick Collection` would be mistaken for it; the project may choose to treat a name clash as "report and skip". (2) Never use `album.clearQuick`. (3) Order: last, after photos.

### C5. Reconciliation of removed and renamed items

What LightCraft offers: `albums.list` (names, tree, counts), `catalog.query` + filter `album` (members), `photo.inspect` (path, albums, stack, `deleted`, `rating`, `meta.keywords`), `keyword.list`/`keyword.info`, `library.import` duplicates (existing id by path). Matching key: lr2lc's own mapping file (Lightroom id → LightCraft id), because LightCraft has no external-id field (`lightcraft-import.md:44`).

- **Removed in Lightroom** (collection gone, member removed from a collection, photo gone from the catalog, keyword removed): with the mapping file the diff can be computed (mapped ids no longer in the new read). Recommended default: **report only**, do not delete anything in LightCraft automatically (principles: "Nothing is lost silently", user data). Optional `--prune` later: `album.removePhotos`, `album.delete` (descendants too, `library.rs:1253-1290`; careful), `photo.delete` (moves to Recently Deleted, recoverable, `library.rs:990`).
- **Renamed in Lightroom** (collection or keyword renamed, photo file renamed/moved): identity by mapped id, so a collection rename → `album.rename {id, name}` (idempotent, `library.rs:1246-1251`); a keyword rename has no id in LightCraft (keywords are path strings in photos' metadata, `lightcraft-library.md`) → `keyword.rename {from, to}` exists (`keywords.rs:516-530`), but only if the old path is still the one lr2lc wrote; otherwise report. A moved/renamed file: the photo's `source.path` differs from the new path; LightCraft cannot re-point a photo through any command read here *(assumption; `library.relocate`-like commands were not searched)*: report, so reconciliation should be limited to reporting renamed files.
- **Edited in LightCraft** after the first run (user changed name or members): with only the mapping file lr2lc cannot tell who changed it. Recommended rule: the first run's state is stored (a hash of what lr2lc wrote) in the mapping file; on conflict, do not overwrite, report. This needs a design decision (QUESTIONS).
- The needed reads (`albums.list`, `catalog.query` per album, one `photo.inspect` per photo) are linear in library size; for large catalogs budget with paging and the 60 s limit per request.

### C6. Writer fixes derived from this research

1. Send `library.select {ids, active}` before every photo command (section 0). Affected: `crates/lc-writer/src/plan.rs` (command list at `:303-336`), `apply.rs`.
2. `keyword.create` result handling: treat the error text `there is a keyword` as "exists" (open question 4); create a keyword only if it is not in the `keyword.list` tree (any level), and do not create ancestors separately: creating a deep path creates the levels implicitly *(assumption: `create_keyword_ops` writes one `SetKeyword` for the full path and levels appear in the tree through the path, `catalog/src/keywords.rs:699`, `:262-285`)*.
3. `keyword.list`: parse only the array form (A5).
4. `imported`: array of integers (A6). Pair with the path by calling per file.
5. Pass `parent` explicitly in `keyword.create` (default-parent behaviour, A5).
6. `photo.inspect` path: read `source.path` (A5), guarding for `source.type != "file"`.

## Open questions

Needing a live test against a running LightCraft (Mac test):

1. Does the control port reply to `library.select` followed by `photo.rate {ids}` as described (gate semantics read from source, not run)? Also whether any command via `app.run` pre-empts `session.execute` (`menus::run_ui_command`, `crates/ui-egui/src/lib.rs:860`) for album and stack commands.
2. Real reply of `photo.inspect`: exact JSON of `source` (expected `{"type":"file","path":…}`), snake_case names.
3. Does `catalog.query {filter:{}}` list all library photos (not Local-only, not deleted)? Page size limits.
4. Do album members keep the `positionInCollection` order in the UI (is there a manual sort)? Does `album.addPhotos` verify photo ids exist (error text)?
5. Smart album correctness: create a smart album with each translated rule and compare `count` to expectations; especially keyword `keyword` field with a full path, date `between` boundaries, `contains` word semantics.
6. Does `album.setTarget` persist or is it session state; does `toggleTarget` create the Quick Collection when `setTarget {id:null}` was called first; does `albums.list` show it.
7. Is the UI thread blocked (no frames) during `library.import`, and does that trigger the 5 s no-frame rejection of the next request?
8. `stack.group` with `collapsed:true` for many stacks: speed; behaviour with deleted or Local members.

Needing the real Lightroom catalog (structure-only, role katalog-tester; no values):

9. Which `creationId` strings exist in LrC 15 and what are their counts (collection, set, smart, quick, others); how the Quick Collection is recognised.
10. Names and columns of the stack tables (B3) and whether `position` 1 is the top.
11. Smart collection inventory: histogram of `criteria` and `operation` names and `combine` values; whether members are stored in `AgLibraryCollectionImage` for smart collections.
12. `positionInCollection` value type (text or number, empty values).
13. Whether `AgLibraryCollectionCoverImage` exists.

Decisions taken or still open:

14. Duplicate album names: keep, or add a suffix such as " (2)"? Decided: keep. LightCraft allows duplicates (A1).
15. Collapsed state of stacks: map it (one parameter) or not? Decided: not mapped, written expanded.
16. Smart collections without an exact rule translation: skip (recommended, implemented) or import as a regular album with current membership (what LightCraft's own importer does)?
17. Removed/renamed items: report only (recommended) or prune? Currently report only.
18. Handling of web, print, book, slideshow collections: skip with reason (recommended).

## Recommendation

1. **Before any further writer work, add `library.select`** before every photo command and accept only the array form of `keyword.list`. These are the two places where the code as written can fail on a real app (A5, section 0). The control-port document's advice "always pass `ids`" is right but not sufficient.
2. **Build collections first** (regular collections and sets). It needs no new commands beyond `album.create` and `album.addPhotos`, both idempotent per member, and a persisted `collection id → album id` map. Do not rename duplicates; report them.
3. **Stacks second**, once the table names are confirmed structurally (B3): `stack.group` is idempotent for equal arguments; pass `collapsed` explicitly.
4. **Quick Collection** with the "set target null, read, send only missing" procedure; low risk, small value.
5. **Smart collections** last, with strict translation (A2) and a structural inventory of criteria first (open question 11). Skip whole collections that cannot be translated exactly, and report them.
6. **Reconciliation of removed and renamed items** as a report-only diff based on the mapping file; deletions and renames in LightCraft only behind an explicit option, and only for items whose id is in the map.
7. Model extensions, as drafted in C1 to C4: `Collection { id, name, parent, kind, members, smart }`, `Stack { id, members (top first), collapsed }`; the `Report` should count per collection and per stack: read, converted, skipped with reason.
