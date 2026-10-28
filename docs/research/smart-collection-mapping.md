# Smart collection criteria → LightCraft rule fields (preparation)

> **Status:** Implemented: the rule parser and model, and a translation that currently accepts only `rating` with `==` or `>=` (whole number 0 to 5), combined with all or any. Every other smart collection is skipped and reported with a code. The criteria listed as possible later additions are not translated; the live-test questions below are still unanswered.

Scope: research only. Paths are relative to the source roots: `lightcraft:` = the LightCraft sources, `lrcat-extractor:` = the extractor sources (read for understanding only, MPL-2.0, nothing copied), `albums:` = `docs/research/lightcraft-albums.md` in this repo (sections A2, B2, C2). The Lightroom side has **no value vocabulary in the available sources** beyond one synthetic sample; everything about Lightroom's criterion values is marked *(assumption)* and must be confirmed by a live test or a structure-only inventory of the real catalog. Strict reading: a rule is translated only if "same effect" is proven; "no" and "unsure" both mean **not translated**, and the whole smart collection is skipped with a reason (albums:A2 "Strict reading").

## 1. Lightroom format

- Stored in `AgLibraryCollectionContent.content` for rows with `owningModule = "ag.library.smart_collection"` (`lrcat-extractor:src/content.rs:77-103`; albums:B2). Other `owningModule` values are view settings, not membership (`lrcat-extractor:src/content.rs:86-98`).
- Notation: Lua-table-like text, `s = { … }` (`lrcat-extractor:src/lron.rs:8-31`). Synthetic sample (`lrcat-extractor:data/test_lron`): an outer table holding a list of criterion tables, each with `criteria`, `operation`, `value`, `value2`, plus `combine` at the outer level. The sample uses operation `>` and has an extra string-valued key holding an escaped nested table (shape only).
- Structure inventory of the two test catalogs (names and counts only; criteria, operations and keys observed in a real catalog, no contents): keys `combine, criteria, operation, value, value2?, value_units`; criteria `rating, touchTime, captureTime, keywords, labelColor, fileFormat, pick`; operations `>=, ==, inLast, empty, any`; `combine` `intersect` / `union`; `value` absent for `empty`; `value_units` present for `inLast`. One collection may hold several criteria.
- `combine` → LightCraft `ruleSet.match` (`lightcraft:crates/catalog/src/rules.rs:14`, evaluation `:1054-1059` per albums:A2): `intersect` → `all`, `union` → `any` *(assumption: names not documented in the sources; `intersect` is only confirmed as a value in the sample, `union` in the inventory)*. Nothing in the sources shows a "none" combine; `exclude` seen nowhere, so an unknown `combine` skips the collection.
- Each criterion row is one `{field, op, value}` rule (`rules.rs:31`); nested groups are not needed for the flat Lightroom structure (no nesting seen in the inventory).

## 2. Mapping table

LightCraft operators per kind: `rules.rs:602-637` (albums:A2). Number ops `is/isNot/gte/lte/gt/lt/between` (`rules.rs:755-760`, between at `:745-752`). Date ops `is/after/before/between/inLast/notInLast/isEmpty` (`rules.rs:840-870`).

| LR criterion | LR operation → value | LightCraft field / op / value | Conversion | Same effect? |
|---|---|---|---|---|
| `rating` | `==`, `>=`, `>`, `<`, `<=` (last three *(assumption)*) → integer 0–5 | `rating` `is` / `gte` / `gt` / `lt` / `lte` (`rules.rs:958`, `:755-760`) | Stars 1:1 (LightCraft ratings are 0–5: `lightcraft:crates/engine/src/cmd/library.rs:808` accepts only values `<= 5` and `:198` clamps to `0..=5`; `u8` field at `lightcraft:crates/catalog/src/model.rs:243`). | **yes** for `==` and `>=` (inventoried). Residual risk: LR "unrated" vs 0 stars (both 0 in the model, *assumption*); verify live once. |
| `pick` | `==` → 1 / -1 / 0 *(values per task brief, not in sources)* | `flag` `is` pick / reject / none (`rules.rs:959-962`; choices `rules.rs` field list "Pick Flag") | 1 → `pick`, -1 → `reject`, 0 → `none`. LightCraft also accepts "picked"/"rejected" (`rules.rs:411`). | **unsure** until the value vocabulary is confirmed in an inventory; expected yes. Translate only 1/-1/0 with `==`; anything else skip. |
| `labelColor` | `==` → colour *(assumption: colour name, or custom label text)* | `label` `is` red/yellow/green/blue/purple/none (`rules.rs:963-967`) | Name 1:1 for the five colours. | **unsure**: LightCraft also matches *custom label names* (`rules.rs:413`, `:964-966`) and Lightroom's value may be the label-set text, which differs per label set. Translate only if the value is one of the five colour names; else skip. Needs live test. |
| `keywords` | `any` / `all` / `none` / `words`… → text *(assumption on names, only `any` and `empty` inventoried)* | `keywords` `contains` / `is` / `isEmpty` or flat `keyword` | see §3 | **no** for text operators (see §3); **unsure** for `empty` (below). |
| `keywords` | `empty` (no value) | `keywords` `isEmpty` (`rules.rs:725`) | Direct. | **unsure**: depends on whether LightCraft's photo keyword list holds the same set as Lightroom (e.g. no implied parents, no hidden/private keywords). Live test. |
| `captureTime` | `inLast` + `value_units` (days/weeks/months/years *(assumption)*) | `captureDate` `inLast` `{n, unit}` (`rules.rs:385-392`, `:859-864`) | Unit names accepted: hour(s), day(s), week(s), month(s), year(s) (`rules.rs:803-807`). | **no / unsure**: LightCraft month = 30.4375 days, year = 365.25 days (`rules.rs:806-807`); Lightroom's calendar arithmetic is unknown; the capture timestamp has no time zone in either *(assumption)*. Boundary photos may differ. Not translated until a live test shows the sets agree. |
| `captureTime` | `==`, `>=`, `<=`, `>`, `<`, `in range` → date | `captureDate` `is` / `after` / `before` / `between` (`rules.rs:840-858`) | ISO prefix dates; `is` is a prefix match (`rules.rs:840-843`), `after` excludes the same day (`:844-847`), `between` includes the end day (`:852-857`). | **unsure**: inclusive/exclusive ends and day granularity in Lightroom unknown. Do not translate until live-tested. |
| `touchTime` | `inLast` / comparisons → date | `editDate` (`rules.rs:1025`) | as `captureTime` | **unsure**: `touchTime` is "last modified" in Lightroom *(assumption)*; LightCraft `editDate` is `p.edited` (`rules.rs:1025`), and what sets it was not read. Not translated. |
| `fileFormat` | `==` → format token *(assumption: e.g. RAW, DNG, JPG, TIFF, PSD, VIDEO)* | `format` (text, `rules.rs:1007`) or `kind` image/raw/video (`rules.rs:969`) | Would need a name table. | **no / unsure**: LightCraft's `format` string content was not checked (`model.rs:231`), and `kind` is coarser than Lightroom's formats (DNG, JPEG, TIFF all "image" or "raw"?). Not translated. |
| `title` | `contains`, `==`, `empty`… | `title` text ops (`rules.rs:1008`) | — | **no**: `contains` is "all words" (`rules.rs:708`), case-insensitive (`rules.rs:705-707`); Lightroom's "contains" vs "contains words" differ *(assumption)*. `empty` (`isEmpty`) is likely equal but unverified: **unsure**. |
| `caption` | as `title` | `caption` (`rules.rs:1009`) | — | same as `title` |
| `camera` | `==`, `contains` | `camera` text (`rules.rs:1014`) | — | **unsure** for `==` (Lightroom may compare the model string with make; LightCraft field content unread); **no** for `contains`. |
| `fileName` | `==`, `contains`, `startsWith`, `endsWith` | `fileName` (`rules.rs:1004`) | — | **unsure**: `startsWith`/`endsWith`/`==` are literal lower-cased compares (`rules.rs:710-713`), but Lightroom's field may include the extension or not *(assumption)*. `contains` **no** (word split). |
| `folder` | path match | `filePath` (field declared at `rules.rs:77`; evaluated by `file_path_matches`, dispatched at `rules.rs:950-951`, defined at `:907`; path ops `:919-926`; it has no line in the generic text-field match, where `format` is at `:1007`) or flat `folder` | — | **no**: paths change under path mapping; LightCraft folder semantics differ (flat `folder` + `subfolders`, `query.rs:20-73`). |
| `collection` | membership of a collection | `album` `is` (id, `rules.rs` Kind::Album, `:320-336`) | needs id of target album already created | **no** for now: requires id mapping, ordering and a non-smart target (`catalog/src/lib.rs:531-535`, albums:A2 "Order of operations"). |
| `hasAdjustments` | bool *(assumption)* | `edited` `is` (`rules.rs:970`) | — | **no**: LightCraft `is_edited` compares against LightCraft import defaults (`model.rs:501-507`); Lightroom's flag is different. |
| anything else, unknown `combine` or operation | — | — | — | skip whole collection with reason `criterion "<name>" has no exact LightCraft equivalent`. |

Net effect under the strict reading: of the seven criteria seen in the inventory only `rating` (`==`, `>=`) is **provably** translatable today; `pick` and a colour-name-only `labelColor` and `keywords empty` become translatable after one live test each. In practice most real collections will be skipped and reported; that is the intended behaviour.

## 3. Keywords and case

LightCraft side (albums:A2; `rules.rs:705-728`, `keywords.rs:32-36`, `query.rs:450-452`):

- All text compares are lower-cased on both sides, i.e. **case-insensitive** (`rules.rs:705-707`, `:957`). Lightroom keyword and text criteria are case-insensitive in the UI *(assumption, not in the sources)*. Equal in intent, but not proven.
- `keywords contains`: substring over the full `a|b|c` path (`rules.rs:708`, `:722-724`), words split at whitespace, **all** words required. A multi-word keyword search therefore means "all words anywhere in one keyword path", not "this phrase". False positives: `travel` matches `xtravel` and every descendant path.
- `keywords is`: whole path equal **or any single level** (`rules.rs:728`): `is italy` matches `travel|italy|rome` and also `places|italy`, and `is travel` matches `travel|italy` (the parent). No prefix match for multi-level values.
- `notContains`: no keyword path contains any of the words (`rules.rs:709`, `:727`).
- Only the flat `keyword` filter is exact "keyword or descendants" (`is_under`, `keywords.rs:32-36`; `query.rs:450-452`), one keyword at a time, AND-combined with everything else (not usable inside `any`).

Comparison with Lightroom *(assumption, names from general knowledge: `any`, `all`, `none`, `words`, `beginsWith`, `endsWith`, `empty`; only `any` and `empty` seen in the inventory)*:

| LR keyword operation | LightCraft candidate | Same effect? |
|---|---|---|
| `any` (photo has any of the listed keywords) | several `keywords is` rules under `match: any`, or `keyword` flat | **no**: `is` matches any level (false positives); flat `keyword` cannot be OR-ed. Whether Lightroom's `any` includes descendants is **unknown**. |
| `all` | several `is` rules under `all` | **no**, same reason. |
| `none` | `notContains` or `match: none` | **no**: `contains` is a substring (false negatives of the exclusion). |
| `words` / contains | `contains` | **no**: word-split vs Lightroom semantics unverified. |
| `empty` | `isEmpty` | **unsure** (§2). |

Only exception worth a live test: a **single** keyword with `any`/`==` mapped to flat `keyword: "<full path>"` (strict descendant semantics); it is exact only if Lightroom also includes descendants and uses synonyms the same way. Synonyms are a further unknown: Lightroom's search may match synonyms, LightCraft's `keywords` field holds names only (`rules.rs:980`, *assumption*).

## 4. Recommendation for splitting the work

**Smart collections, part a: parser + model (no LightCraft calls).**
- Fresh lron/Lua-table parser (grammar from `lrcat-extractor:src/lron.rs:8-53`, written anew) turning `content` into `SmartRules { combine: Intersect|Union|Other(String), rules: Vec<SmartRule { criterion: String, operation: String, value: Option<Value>, value2: Option<Value>, units: Option<String> }> }`. Keep unknown names verbatim; the report shows names only, never values (privacy).
- Reader test fixtures generated with every combination of the inventoried criteria/operations plus unknown ones; truncated or odd tables become a skip with reason, never a panic.
- Counting rule: collections read = converted + skipped (principle 4); the skip reason is per collection, naming the first untranslatable criterion.

**Smart collections, part b: translation + writer.**
- Pure function `translate(SmartRules) -> Result<RuleSet, Vec<Reason>>`, a table-driven whitelist from §2 (initially `rating ==/>=`; `pick`, `labelColor` colour names, `keywords empty` enabled only after the live test). Any untranslatable rule → whole collection skipped (a dropped rule changes membership both for `all` and `any`, albums:A2).
- `album.createSmart {name, parent?, rules}` (always pass `rules`; albums:A2); runs after regular albums. Not idempotent: store the Lightroom collection global id → album id in the mapping state, and never call again for a known id; rule changes only reported (first version).
- Compare `count` from `album.createSmart` with Lightroom's current membership where readable (`AgLibraryCollectionImage`, albums:B2, albums:C2); warn on mismatch.
- Dry run default: print the translated rule set; `--apply` creates albums.

**Live-test questions (LightCraft + Lightroom, structure only):**
1. Which `operation` strings and `combine` strings exist in the real catalog (histogram only)?
2. `pick`: do values 1/-1/0 correspond to picked/rejected/unflagged exactly?
3. `labelColor`: stored value = colour name or label-set text? Behaviour with a renamed label set versus LightCraft `label` (custom names, `rules.rs:964-966`).
4. `rating`: does Lightroom's "unrated" equal rating 0; boundaries of `>=`.
5. Keywords: does `any`/`==` include descendants and synonyms in Lightroom; does LightCraft's `keywords` field equal Lightroom's assigned keywords (no implied parents)?
6. Date criteria: inclusive ends, time zones, month/year arithmetic for `inLast` (30.4375-day month, `rules.rs:806`), `touchTime` ↔ `editDate`.
7. `fileFormat`: token vocabulary and LightCraft `format` / `kind` values for DNG, JPEG, TIFF, PSD, RAW.
8. Does `album.createSmart` with a rule set referencing a missing field or bad value fail with `rule N.M:` errors and create nothing (`rules.rs:155-160`, albums:A2)?
9. Does Lightroom keep `AgLibraryCollectionImage` rows for smart collections (for the count check)?
10. Are `title`/`caption` `is empty` equal on both sides (whitespace-only text: LightCraft trims, `rules.rs:703`)?

## Open questions

- Value vocabularies for all seven criteria (see live-test list); none is proven by the available sources.
- Whether the user wants a fallback regular album with current membership for skipped smart collections (albums:C2 "Fallback").
- Whether `hasAdjustments`, `folder`, `collection`, `camera`, `fileName` should ever be translated, or always reported.

## Recommendation

Implement the parser and model first (report names only). Implement the translation with a whitelist that starts with `rating` only, and enable `pick`, colour-name `labelColor`, and `keywords empty` one by one after the live tests. Skip every other collection whole, with a reason. Do not translate keyword, date or text rules until their semantics are verified against both applications.
