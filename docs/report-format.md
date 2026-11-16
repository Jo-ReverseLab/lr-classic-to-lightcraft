# `report.json` format reference

This document lists every key of `report.json` that `lr2lc --report FILE`
writes: its JSON type, when it appears, and the guarantees that hold. It is a
reference, not a specification: keys are not added or renamed here. Any
inconsistency found while writing this document is listed under
[Questions](#questions).

The file is pretty-printed JSON with a trailing newline. It is written only
when `--report FILE` is given, after the run, and `FILE` is checked before
anything is read (see the README section on `--report`).

## How the object is built

The top-level object is built in `crates/lr2lc/src/output.rs`
(`report_json_with`) and written by `report_file::write` in
`crates/lr2lc/src/main.rs`. The `writer` value comes from
`crates/lr2lc/src/writer.rs` and is an untagged enum: in a dry run it holds
the planned section; with `--apply` it holds the applied section. Because it
is untagged, the JSON carries the inner fields directly, with no
discriminator. `writer` is always present in any report file that is written.

Notation below:

- **always** — the key is serialized in every report (no `skip_serializing_if`
  on the field); an empty collection renders as `[]` or `{}`.
- **dry run** — present only when `--apply` is absent.
- **apply** — present only with `--library` and `--apply`.
- **on abort** — present only when an apply run stopped early; absent after a
  complete run.

Every skip entry, in every list, has the shape `{"reason": string, "code":
string}`. `reason` is human-readable text, never empty (empty becomes
`"unspecified"`). `code` is a fixed snake_case key that never holds a value
from the catalog. Every `by_code`/`*_by_code` map is `code string -> integer`;
only codes whose count is non-zero appear; the counts sum to the length of the
matching skipped list.

## Top-level keys

| Key | Type | When | Notes |
| --- | --- | --- | --- |
| `tool_version` | string | always | the lr2lc version |
| `catalog_version` | string | always | the Lightroom catalog version string |
| `dry_run` | boolean | always | `true` unless `--apply` ran |
| `catalog_unchanged` | boolean | always | checksum equal before and after the run |
| `folders` | object | always | see [Area](#areas) |
| `photos` | object | always | see [photos](#photos) |
| `collections` | object | always | see [collections](#collections) |
| `collection_members` | object | always | area |
| `stacks` | object | always | see [stacks](#stacks) |
| `stack_members` | object | always | area |
| `keywords` | object | always | see [keywords](#keywords) |
| `keyword_assignments` | object | always | area |
| `keyword_synonyms` | object | always | area |
| `ratings` | object | always | see [ratings](#ratings) |
| `develop` | object | always | see [develop](#develop) |
| `skipped_by_code` | object | always, may be `{}` | sums the nine catalog areas only (not `writer`) |
| `writer` | object | always | see [writer](#writer) |

## Areas

The keys `folders`, `collection_members`, `stack_members`,
`keyword_assignments` and `keyword_synonyms` are plain area objects. `photos`,
`collections`, `stacks` and `keywords` flatten an area into themselves, so they
also carry these four fields.

| Key | Type | When | Notes |
| --- | --- | --- | --- |
| `<area>.read` | integer | always | elements read from the catalog |
| `<area>.converted` | integer | always | elements converted |
| `<area>.skipped` | array | always, may be `[]` | each `{"reason","code"}` |
| `<area>.by_code` | object | always, may be `{}` | per-area skip counts |

**Guarantee.** For every area, `read == converted + len(skipped)`.

The nine catalog areas, in output order: `folders`, `photos`, `collections`,
`collection_members`, `stacks`, `stack_members`, `keywords`,
`keyword_assignments`, `keyword_synonyms`. The top-level `skipped_by_code`
sums the `by_code` of these nine only; writer skips are not included.

## photos

Carries the area fields (`read`, `converted`, `skipped`, `by_code`) plus:

| Key | Type | When | Notes |
| --- | --- | --- | --- |
| `photos.videos` | integer | always | skipped photos whose code is `video`; `videos == by_code.video` |
| `photos.flags` | object | always | see below |
| `photos.labels` | object | always | see below |
| `photos.captions` | integer | always | converted photos with a caption; text never reported |
| `photos.copyrights` | integer | always | converted photos with a copyright; text never reported |
| `photos.creators` | integer | always | converted photos with a creator; name never reported |
| `photos.titles` | integer | always | converted photos with a title; text never reported |
| `photos.titles_unreadable` | integer | always | converted photos whose title blob could not be read |

`photos.videos` is a subset of `photos.skipped` (videos are also listed there).

### photos.flags

| Key | Type | Notes |
| --- | --- | --- |
| `photos.flags.pick` | integer | converted photos flagged pick |
| `photos.flags.reject` | integer | flagged reject |
| `photos.flags.none` | integer | no flag (does not include `unknown`) |
| `photos.flags.unknown` | integer | stored flag value not recognised; converted without a flag |

**Guarantee.** `pick + reject + none + unknown == photos.converted`.

### photos.labels

| Key | Type | Notes |
| --- | --- | --- |
| `photos.labels.red` | integer | |
| `photos.labels.yellow` | integer | |
| `photos.labels.green` | integer | |
| `photos.labels.blue` | integer | |
| `photos.labels.purple` | integer | |
| `photos.labels.none` | integer | empty or NULL label |
| `photos.labels.custom` | integer | label not in the chosen set; converted without a label |
| `photos.labels.label_set` | string | `auto`, `en`, `de` or `file` |
| `photos.labels.matched_sets` | array | under `auto`, the standard sets that matched at least one name, in the order `en`, `de`; empty for a fixed set |
| `photos.labels.detected_set` | string or null | under `auto`, the set used: `en` or `de`, whichever names label more converted photos (`en` on a tie); only its names map to colours, like Lightroom with that set active. `null` when no standard name matched and for a fixed set |
| `photos.labels.custom_reasons` | array | one entry per custom photo, ascending by Lightroom id |

Each `custom_reasons` entry is `{"reason": string}`. The reason is the fixed
text `photo <id>: custom label not in label set`; it carries only the
Lightroom id, never the label text.

**Guarantee.**
`red + yellow + green + blue + purple + none + custom == photos.converted`,
and `len(custom_reasons) == custom`.

## ratings

A top-level object (not nested under `photos`). Always has exactly the six
keys `"0"` .. `"5"`, each an integer count of converted photos with that star
rating (unrated counts as 0).

**Guarantee.** The six counts sum to `photos.converted`.

## develop

Development settings: counts and field or profile names only, never a
rendering value.

| Key | Type | When | Notes |
| --- | --- | --- | --- |
| `develop.photos` | integer | always | converted photos that carry development settings |
| `develop.planned` | integer | always | photos the plan sends `develop.*` commands for |
| `develop.written` | integer | always | photos whose develop commands all succeeded (0 in a dry run) |
| `develop.unmapped` | array of strings | always | distinct field names that could not be carried over, sorted, at most 20 |
| `develop.unmapped_count` | integer | always | full number of distinct unmapped names, also above the cap |
| `develop.unmapped_photos` | object | always | every unmapped field name (sorted, not capped) with the number of converted photos that carry it; names and counts only |
| `develop.not_applied` | integer | always | known photos whose settings changed in Lightroom and were left alone |
| `develop.overwritten` | integer | always | photos rewritten because `--overwrite-develop` was given |
| `develop.camera_profiles` | object | always, may be `{}` | profile name → number of converted photos, sorted, at most 20 entries |
| `develop.camera_profiles_count` | integer | always | full number of distinct profile names, also above the cap |
| `develop.photos_with_profile` | integer | always | converted photos with a non-empty profile name |
| `develop.masks` | integer | always | converted photos with at least one mask (local correction) |
| `develop.spots` | integer | always | converted photos with at least one spot (heal/clone/remove/fill) |
| `develop.white_balance_custom` | integer | always | converted photos with a custom white balance (temperature or tint with mode `custom`); a named mode is carried by the mode and not counted |
| `develop.white_balance_to_check` | integer | always | dry run: planned Kelvin white balances whose transfer depends on LightCraft's scale for the photo; 0 in an applied run |
| `develop.white_balance_written` | integer | always | applied run: photos whose custom white balance was written; 0 in a dry run |
| `develop.white_balance_not_carried` | integer | always | applied run: photos whose Kelvin white balance was not written because LightCraft edits them on a relative scale; 0 in a dry run |

**Guarantee.** A camera profile name is reported byte for byte and only when
it is non-empty; it is never a rendering value and never triggers a develop
command. Profile names and unmapped field names are independent: `Look` is
an unmapped field name, not a profile. A mask that cannot be converted is
named in `develop.unmapped` (`Mask: colour range`, `Mask: AI mask (3)`),
never replaced by a partial mask; mask values never appear in the report.
The same holds for spots: an unconvertible spot is named (`Spot: Gradient`,
`Spot: empty brush`, `Spot: legacy line`, `Spot: no mask`) and its values
never appear in the report.

## collections

Carries the area fields plus:

| Key | Type | Notes |
| --- | --- | --- |
| `collections.albums` | integer | converted collections of kind album |
| `collections.folders` | integer | converted collections of kind folder |
| `collections.duplicate_names` | integer | converted collections that repeat an earlier sibling's exact name under the same parent (two equal names count as 1) |
| `collections.quick` | integer | 1 if a Quick Collection was read and converted, else 0 |
| `collections.smart` | integer | smart collections converted as smart albums |

**Guarantees.**
`albums + folders == converted`; `duplicate_names <= converted - 1`; `quick`
is 0 or 1; `smart <= converted`.

## stacks

Carries the area fields plus:

| Key | Type | Notes |
| --- | --- | --- |
| `stacks.collapsed` | integer | converted stacks collapsed in Lightroom (written expanded) |
| `stacks.no_top` | integer | converted stacks whose first photo is not at position 1 |

**Guarantees.** `collapsed <= converted`; `no_top <= converted`.

## keywords

Carries the area fields plus:

| Key | Type | Notes |
| --- | --- | --- |
| `keywords.persons` | integer | converted keywords that name a person |
| `keywords.not_exported` | integer | keywords not written into exported photos |
| `keywords.no_parents_on_export` | integer | exported without their parents |
| `keywords.no_synonyms_on_export` | integer | exported without their synonyms |
| `keywords.unknown_type` | integer | type neither plain keyword nor person |
| `keywords.unused` | integer | converted keywords without a converted assignment |
| `keywords.trimmed` | integer | converted keywords whose own name starts or ends with white space; LightCraft trims keyword names, so they arrive without it (assignments still match) |

These seven are overlapping subsets of `keywords.converted` (a keyword can be in
several).

## writer

`writer` is always present. Its shape depends on the run.

### writer — dry run (`planned` section)

| Key | Type | When | Notes |
| --- | --- | --- | --- |
| `writer.planned` | object | dry run | see below |
| `writer.photos_skipped` | array | dry run | each `{"reason","code"}` |
| `writer.photos_skipped_by_code` | object | dry run | |
| `writer.keywords_not_created` | array | dry run | |
| `writer.keywords_not_created_by_code` | object | dry run | |
| `writer.members_skipped` | array | dry run | album members |
| `writer.members_skipped_by_code` | object | dry run | |
| `writer.stacks_skipped` | array | dry run | |
| `writer.stacks_skipped_by_code` | object | dry run | |
| `writer.quick_skipped` | array | dry run | Quick Collection |
| `writer.quick_skipped_by_code` | object | dry run | |
| `writer.smart_skipped` | array | dry run | smart collections |
| `writer.smart_skipped_by_code` | object | dry run | |

#### writer.planned

Every field is an integer.

| Key | Notes |
| --- | --- |
| `writer.planned.keywords` | keywords to create |
| `writer.planned.photos` | photos to import |
| `writer.planned.updates` | field-update commands |
| `writer.planned.photos_updated` | known photos needing at least one update |
| `writer.planned.photos_unchanged` | known photos needing no command |
| `writer.planned.folders` | album folders to create |
| `writer.planned.albums` | albums to create (not folders) |
| `writer.planned.members_added` | photos to add to albums |
| `writer.planned.members_removed` | photos to remove from albums |
| `writer.planned.collections_known` | input collections already in the mapping file |
| `writer.planned.collections_removed` | recorded collections no longer in Lightroom (counted only) |
| `writer.planned.stacks` | stacks to group (new or changed) |
| `writer.planned.stacks_known` | input stacks already recorded with same members and order |
| `writer.planned.stack_members_removed` | recorded stack members that left their stack in Lightroom |
| `writer.planned.quick_add` | Quick Collection members to add |
| `writer.planned.quick_already` | Quick Collection members already in the mapping file |
| `writer.planned.quick_removed` | recorded Quick Collection members that left it in Lightroom |
| `writer.planned.smart_albums` | smart albums to create |
| `writer.planned.smart_known` | smart collections already in the mapping file |
| `writer.planned.smart_rules_changed` | known smart collections whose rules changed in Lightroom |
| `writer.planned.smart_no_longer_translatable` | recorded smart albums whose collection still exists but no longer translates |

**Guarantees (dry run).**
- `photos.converted == planned.photos + planned.photos_updated + planned.photos_unchanged + len(writer.photos_skipped)`.
- `collections.converted == planned.folders + planned.albums + planned.collections_known` (no collection is skipped by the writer in a dry run).
- smart: `collections.smart == planned.smart_albums + planned.smart_known + len(writer.smart_skipped)`.
- stacks: `stacks.converted == planned.stacks + planned.stacks_known + len(writer.stacks_skipped)`.
- `members_added`, `members_removed` and `quick_add` count photos, not collections.

### writer — apply (`applied` section)

All integer counters, all `*_skipped`/`*_incomplete` arrays and the three id
arrays (`removed_photos`, `moved_photos`, `renamed_keywords`) are always
present (arrays may be `[]`, maps may be `{}`). `aborted` is the only optional
key. Conflict data appears only here: a dry run does not connect to
LightCraft, so the planned section carries none.

| Key | Type | When | Notes |
| --- | --- | --- | --- |
| `writer.keywords_created` | integer | apply | |
| `writer.photos_imported` | integer | apply | |
| `writer.photos_existing` | integer | apply | found as duplicates on import |
| `writer.photos_same_content` | integer | apply | imported photos whose file has the same bytes as another photo of the catalog; each became its own record (a copy linked to its own file). Part of `photos_imported` |
| `writer.photos_updated` | integer | apply | known photos, at least one update sent |
| `writer.photos_unchanged` | integer | apply | known photos, no command |
| `writer.updates` | integer | apply | field updates applied |
| `writer.not_saved` | integer | apply | applied in memory but not yet saved to disk |
| `writer.conflicts` | array | apply | each `{"photo": <Lightroom id>, "field": <name>}`: a field changed differently in Lightroom and LightCraft since the last import; nothing was written |
| `writer.conflicts_count` | integer | apply | `len(conflicts)` |
| `writer.keywords_removed` | integer | apply | keyword memberships that left the photo in Lightroom; kept in LightCraft and only counted |
| `writer.photos_removed` | integer | apply | recorded photos no longer in the Lightroom model; nothing was deleted in LightCraft |
| `writer.removed_photos` | array | apply | each `{"photo": <Lightroom id>}`, ascending by id; `len(removed_photos) == photos_removed` |
| `writer.photos_moved` | integer | apply | recorded photos whose original moved while their global id stayed; they are not imported again |
| `writer.moved_photos` | array | apply | each `{"photo": <Lightroom id>}`, ascending by id; `len(moved_photos) == photos_moved` |
| `writer.keywords_renamed` | integer | apply | recorded keywords renamed in Lightroom (same global id, new path text); nothing was created or renamed in LightCraft |
| `writer.renamed_keywords` | array | apply | each `{"keyword": <Lightroom id>}`, ascending by id; `len(renamed_keywords) == keywords_renamed` |
| `writer.photos_skipped` | array | apply | |
| `writer.photos_skipped_by_code` | object | apply | |
| `writer.photos_incomplete` | array | apply | a photo whose update or the select before it failed (resent next run) |
| `writer.photos_incomplete_by_code` | object | apply | |
| `writer.keywords_not_created` | array | apply | |
| `writer.keywords_not_created_by_code` | object | apply | |
| `writer.folders_created` | integer | apply | |
| `writer.albums_created` | integer | apply | |
| `writer.collections_known` | integer | apply | already in the mapping file |
| `writer.collections_existing` | integer | apply | missing from the file but found in the library and taken over |
| `writer.members_added` | integer | apply | photos added to albums |
| `writer.members_removed` | integer | apply | photos removed (only ones lr2lc added before) |
| `writer.collections_renamed` | integer | apply | known collections renamed in Lightroom (left as is in LightCraft) |
| `writer.collections_moved` | integer | apply | known collections moved in Lightroom (left as is) |
| `writer.collections_removed` | integer | apply | recorded collections gone from Lightroom (not deleted in LightCraft) |
| `writer.collections_skipped` | array | apply | not created |
| `writer.collections_skipped_by_code` | object | apply | |
| `writer.collections_incomplete` | array | apply | a select or member change failed (resent next run) |
| `writer.collections_incomplete_by_code` | object | apply | |
| `writer.members_skipped` | array | apply | photo not converted, no global id, or no LightCraft id |
| `writer.members_skipped_by_code` | object | apply | |
| `writer.stacks_grouped` | integer | apply | |
| `writer.stacks_known` | integer | apply | in the mapping file with same members and order |
| `writer.stack_members_removed` | integer | apply | members that left a stack in Lightroom and stay in LightCraft |
| `writer.stacks_skipped` | array | apply | fewer than two photos with a LightCraft id |
| `writer.stacks_skipped_by_code` | object | apply | |
| `writer.stacks_incomplete` | array | apply | select or group rejected (resent next run) |
| `writer.stacks_incomplete_by_code` | object | apply | |
| `writer.quick_added` | integer | apply | |
| `writer.quick_already` | integer | apply | already in the mapping file |
| `writer.quick_removed` | integer | apply | recorded members that left the Quick Collection |
| `writer.quick_skipped` | array | apply | |
| `writer.quick_skipped_by_code` | object | apply | |
| `writer.quick_incomplete` | array | apply | Quick Collection command rejected (resent next run) |
| `writer.quick_incomplete_by_code` | object | apply | |
| `writer.smart_created` | integer | apply | |
| `writer.smart_known` | integer | apply | already in the mapping file |
| `writer.smart_existing` | integer | apply | taken over from the library |
| `writer.smart_rules_changed` | integer | apply | known smart collections whose rules changed in Lightroom (left as is) |
| `writer.smart_no_longer_translatable` | integer | apply | recorded smart albums whose collection still exists but no longer translates |
| `writer.smart_skipped` | array | apply | |
| `writer.smart_skipped_by_code` | object | apply | |
| `writer.aborted` | string | on abort | short reason, fixed texts and command names only; never a path, name or app message |

**Guarantees (apply, complete run).**
- `photos.converted == photos_imported + photos_existing + photos_updated + photos_unchanged + len(photos_skipped)`. (`photos_incomplete` is a subset of imported/existing/updated, not part of this partition.)
- `keywords.converted == keywords_created + len(keywords_not_created)` only on a first apply into a library and mapping file without keywords. It does not hold on later runs for keywords that are silently left alone: a keyword already in the mapping file that is an implicit ancestor (added only for a parent path) or whose attributes and synonyms are unchanged (`crates/lc-writer/src/plan.rs:814-823`), a keyword already in the library (`:830-832`) and a keyword renamed in Lightroom (`:827-829`) is neither created nor listed in `keywords_not_created`, while `keywords_created` counts only successful `keyword.create` calls (`crates/lc-writer/src/apply.rs:1219`). Those keywords are still in `keywords.converted` (the reader's count) and in no writer counter. An explicit keyword already in the mapping file whose attributes or synonyms changed is different: it is skipped with `KeywordChanged` and does appear in `keywords_not_created` (`plan.rs:815-822`, `:614`; `apply.rs:296`), so it keeps the equation balanced.
- `collections.converted == folders_created + albums_created + collections_known + collections_existing + len(collections_skipped)`. (`collections_incomplete` is a subset of created/existing.)
- `stacks.converted == stacks_grouped + stacks_known + len(stacks_skipped)`. (`stacks_incomplete` is a subset of grouped.)
- smart: `collections.smart == smart_created + smart_known + smart_existing + len(smart_skipped)`.
- `collections_renamed`, `collections_moved`, `collections_removed`, `smart_rules_changed` and `smart_no_longer_translatable` are subsets of `*_known` (changed in Lightroom, left as is in LightCraft).
- `removed_photos`, `moved_photos` and `renamed_keywords` carry only Lightroom ids; no path, file name, keyword name or global id.
- `members_added`, `members_removed`, `stack_members_removed`, `quick_added` and `quick_removed` count photos.
- For an aborted run the counters reflect what was done up to the stop; a command without a reply counts nowhere; `aborted` is then present.

## What never appears

Reasons never expose user data. They carry fixed text plus Lightroom ids only.

- No file paths.
- No collection names, keyword names or synonym names.
- No caption, copyright, creator or title text — only counts.
- No conflict values: `writer.conflicts` carries only a Lightroom id and the
  fixed field name; the Lightroom and LightCraft values are never reported.
- No paths, file names, keyword names or global ids in `writer.removed_photos`,
  `writer.moved_photos` and `writer.renamed_keywords`: only Lightroom ids.
- No colour-label text — only counts, the set name, and the Lightroom id in
  `custom_reasons`.
- No LightCraft ids — they live in the mapping file, never in the report.
- No app error messages (which may quote paths) are copied into reasons.

## Skip codes

Codes are stable snake_case keys that never hold a catalog value. The full set
(defined in `crates/model/src/skip.rs`) follows. An absent code is omitted
from every `by_code`/`*_by_code` map and from the top-level `skipped_by_code`.

Reader — folders and photos: `no_root_folder`, `root_folder_not_found`,
`root_folder_no_path`, `virtual_copy`, `video`, `missing`, `missing_global_id`,
`duplicate_global_id`, `file_row_not_found`, `folder_row_not_found`,
`invalid_rating`.

Reader — collections: `collection_kind_unsupported`, `collection_kind_missing`,
`quick_duplicate`, `smart_unreadable`, `smart_no_criteria`,
`smart_untranslatable`, `name_invalid`, `parent_invalid`, `parent_missing`,
`parent_skipped`, `parent_not_set`, `parent_cycle`.

Reader — collection members: `collection_id_invalid`,
`collection_photo_invalid`, `collection_skipped`, `collection_unknown`,
`photo_not_converted`, `duplicate_member`, `collection_is_set`.

Reader — stacks and stack members: `too_few_photos`, `stack_unknown`,
`stack_id_invalid`, `ids_invalid`, `photo_invalid`, `already_in_stack`,
`stack_skipped`.

Reader — keywords, assignments, synonyms: `name_empty`, `name_has_bar`,
`assignment_photo_skipped`, `assignment_keyword_skipped`, `keyword_skipped`,
`keyword_unknown`.

Reader and writer — paths: `path_not_utf8` (a catalog path or file name
that cannot be used as text — bytes that are not valid UTF-8, or a
non-text value — and is skipped by the reader, or a model path the writer
cannot send as UTF-8).

Writer: `keyword_changed`, `no_global_id`,
`no_lightcraft_id`, `ambiguous`, `parent_not_created`, `parent_failed`,
`no_id_returned`, `applied_not_saved`, `import_failed`, `rejected`.

Catch-all (kept for callers that skip without a specific code): `unspecified`.

## Questions

These are observations from writing the document, not changes made.

1. **`smart_no_longer_translatable` declaration order.** `ApplySummary`
   declares this field between `collections_removed` and `collections_skipped`
   in `crates/lc-writer/src/apply.rs`, but in the applied `writer` JSON it is
   serialized near the smart block (`smart_known`, `smart_existing`, ...). The
   JSON key set is correct and consistent with the guarantees; only the Rust
   field order in the summary struct differs from the JSON order. No action
   needed unless the struct order should mirror the JSON order.
