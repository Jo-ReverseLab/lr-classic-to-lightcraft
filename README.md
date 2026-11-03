# lr2lc

lr2lc converts Adobe Lightroom Classic catalogs (`.lrcat`) into LightCraft
libraries. It reads the catalog read-only, never touches the original photos,
and performs a dry run unless `--apply` is given.

This project is not affiliated with or endorsed by Adobe. "Lightroom" is used
only to describe compatibility.

## Status

Early development; the version is 0.x and the command line may still
change. Not every part of a catalog is converted yet (see
[Known limits](#known-limits)).

What lr2lc reads from a catalog, read-only: folders and photos (with a check
that the original files exist, after an optional path mapping), keywords with
their hierarchy, synonyms and attributes and their assignments to photos, star
ratings, pick/reject flags, colour labels, titles, captions, copyright and
creator, collections as albums and collection sets as album folders (with
their members), smart collections, folder stacks and the Quick Collection.

What `--apply` writes into a LightCraft library, through the control port of
the running LightCraft app: keywords, photos (as references to the existing
files), ratings, flags, labels, titles, captions, copyright and creator,
albums and album folders with their members, smart albums (only when the rule
translates exactly), stacks (always written expanded) and the Quick
Collection (only adding photos). Development settings are not converted yet.

Without `--apply`, lr2lc only reports what it would do. Everything that is
not converted is listed with a reason in the summary and in `report.json`.

CI runs formatting, lint and tests on Linux, macOS and Windows.

## Options

| Option | Purpose |
| --- | --- |
| `--apply` | Write the LightCraft library (default: dry run); needs `--library` |
| `--library DIR` | The LightCraft library to write; also used by the dry run to read the mapping file |
| `--port N` | Control port of the running LightCraft app (default 7980) |
| `--allow-xmp` | Write even if the library writes XMP sidecars next to the originals |
| `--map OLD=NEW` | Replace a path prefix, repeatable |
| `--label-set SET` | Names of the colour labels: `en`, `de`, `auto` (default) or a JSON file |
| `--report FILE` | Write `report.json` |
| `--script FILE` | Write the planned commands as JSON lines |

Each option is described below.

## Usage

```sh
lr2lc [OPTIONS] <CATALOG>
cargo run -p lr2lc -- --help
```

Close Lightroom Classic before running lr2lc. If a non-empty `-wal` or
`-journal` file sits next to the catalog, Lightroom may still hold unsaved
changes and lr2lc stops with an error. The catalog is opened read-only and a
checksum before and after the run proves it was not changed.

### Dry run (default)

Without `--apply`, lr2lc only reads the catalog and prints a summary: how
many folders, photos, collections, collection members, stacks, stack
members, keywords, keyword
assignments and keyword synonyms it read,
how many it would convert and how many it skips, with the reasons grouped.
A line `videos: skipped N` follows, then a line with keyword attribute
counts, a line shows how many of the converted photos have 0 to 5
stars (unrated photos count as 0), and a last line counts their pick/reject
flags:

```text
keywords: read 12, would convert 11, skipped 1
keyword assignments: read 30, would convert 25, skipped 5
keyword synonyms: read 4, would convert 3, skipped 1
videos: skipped 2
keyword attributes: persons 0, not exported 0, no parents on export 0, no synonyms on export 0, unknown type 0, unused 1
ratings: 0: 6, 1: 3, 2: 2, 3: 3, 4: 2, 5: 1
flags: pick 4, reject 2, none 11, unknown 0
```

Collections become albums and collection sets become album folders, with
their nesting and names unchanged. Web, print and other kinds of
collections are skipped with a reason. Smart collections become smart
albums (see [Smart collections](#smart-collections)) only when every rule
translates exactly; otherwise they are skipped. A collection whose parent
is missing, skipped, not a collection set or part of a cycle is skipped,
and so is everything below it. Empty collections are kept. Members keep
Lightroom's order; a member is skipped if its photo is not converted (for
example a missing original or a virtual copy), if it repeats a photo
already in the collection, if its collection is skipped, or if it belongs
to a collection set, which holds no photos. Collection names never appear
in the output or in the report.

The first system collection (`systemOnly` not zero) by id is the Quick
Collection. It counts as converted (`collections.quick` is 1, and
`collections.converted` is folders + albums + quick); every further one is
skipped with `more than one quick collection`. Its members follow the album
rules above. With `--apply` lr2lc runs, after the albums (only smart
albums come later) and only when
some member is not yet in the mapping file: `album.setTarget {id: null}`
(so a target album you chose is not used), `albums.list`, then reads the
members of the one top-level album (not a smart album) named
`Quick Collection` with
`catalog.query` (200 per page). Only photos it does not hold yet are sent,
with `library.select` and `album.toggleTarget`; LightCraft creates the
album when it is missing. Two such albums skip the step as ambiguous
(`quick_skipped`). lr2lc never removes photos from it and never empties it:
members that left the Quick Collection in Lightroom are only counted
(`quick_removed`). The writer section reports `quick_added`,
`quick_already`, `quick_removed`, `quick_skipped` and `quick_incomplete`;
the dry run reports `planned.quick_add`. The mapping file keeps the added
members under `quick` as `{"members": [...]}` (sorted global ids).

Folder stacks are read with their members, the top photo first. Members
are ordered by Lightroom's position (the top photo has position 1), then by
row id. A member is skipped if its photo is not converted (a virtual copy, a
video, a missing original or an unknown id) or if the photo is already in a
stack with a smaller id; a stack left with fewer than two photos is skipped
with the reason `fewer than two converted photos`, together with its
members. A line `stack details: collapsed N (written expanded), no top
photo N` counts the converted stacks that are collapsed in Lightroom and
those whose first photo is not at position 1. Catalogs without the stack
tables have no stacks. Reasons name ids only.

LightCraft cannot read videos, so they are skipped with the reason
`video files are not supported by LightCraft`. A photo counts as a video if
Lightroom stores its format as `VIDEO` or its file extension is one of mov,
mp4, m4v, avi, mts, m2ts, 3gp, mpg, mpeg, wmv or mkv (any letter case).
Virtual copies keep their own reason, even of a video.

Each photo keeps its flag: picked, rejected or none. A stored flag value
other than pick, reject or none is converted as no flag and counted as
`unknown`; the photo itself is still converted. The four flag counts add up
to the converted photos.

Colour labels are counted on their own line, for example
`labels: red 2, yellow 1, green 1, blue 1, purple 1, none 11, custom 0, set en`.
Lightroom stores a label as text in the language of its interface, so the
names are mapped with a label set (see `--label-set`). A label that is not
in the set is a custom label: the photo is still converted, without a
label, and counted as `custom`. The seven counts add up to the converted
photos. Label names never appear in the output or in the report.

Photos with a caption are counted on the line `captions: N` and as
`photos.captions` in the report; the caption text itself never appears.

Photos with a copyright notice are counted on the line `copyrights: N` and
as `photos.copyrights`; photos with a creator on the line `creators: N` and
as `photos.creators`. The texts themselves never appear. Catalogs without
the creator tables report `creators: 0`.

Photos with a title (read from the XMP data the catalog stores per photo)
are counted on the line `titles: N` and as `photos.titles`. Photos whose
stored XMP data cannot be read have no title; they are converted anyway and
counted on the line `titles unreadable: N` and as `photos.titles_unreadable`.
Titles never appear in the output. Catalogs without XMP data report
`titles: 0`.

Photos and keywords are identified by Lightroom's global id, which, unlike the
internal row id, does not depend on the order in which rows were stored. This keeps repeated runs from creating duplicates. A photo or keyword
without a global id is skipped with the reason `missing global id`; if
several rows share one, the row with the lowest catalog id is kept and the
others are skipped with `duplicate global id` (a skipped photo does not
claim its global id). A skipped keyword takes the keywords below it along.
Global ids are used internally only and never appear in the output or in
the report.

Keywords are only assigned to photos that are converted, so assignments to
skipped photos are skipped too. Synonyms are converted together with their
keyword; a synonym of a skipped or unknown keyword, or one without text, is
skipped. Each keyword keeps its attributes: whether it is included on
export, exported with its parents and with its synonyms, and whether it names
a person. A keyword of another type is converted as an ordinary keyword and
counted as `unknown type`. `unused` counts converted keywords without a
converted assignment; they are still converted. Nothing is written except
the optional report file.

### `--map OLD=NEW`

Replaces the path prefix `OLD` with `NEW`, for example when photos moved to
another disk or computer. The option can be given several times. Original
files are checked at the mapped path.

```sh
lr2lc --map /Users/example/Pictures=/mnt/photos catalog.lrcat
lr2lc --map 'D:\Photos=/mnt/photos' catalog.lrcat
lr2lc --map '/Users/example/Pictures=D:\Photos' catalog.lrcat
```

- Prefixes match only at path component boundaries: `/a/b` matches `/a/b`
  and `/a/b/c`, but not `/a/bc`.
- `/` and `\` are equivalent when matching. Case matters, except for a
  drive letter such as `C:`. A trailing separator makes no difference.
- If several rules match, the longest `OLD` prefix wins. Among rules of the
  same length (for example `/a` and `/a/`), the first one given wins.
- The rest of the path uses the separator style of `NEW`: `\` if `NEW`
  contains `\` or starts with a drive letter, `/` otherwise.
- The rest must not climb out of `NEW` via `..`. A `..` that would pop to the
  parent of `NEW` or above is refused: the element keeps its original path and
  is reported with the `path_mapping_escaped` code, never silently rewritten
  to a location outside `NEW`.
- If two photos land on the same path after mapping, the later one is reported
  with the `path_mapping_collision` code and is not converted, so nothing is
  merged silently.
- The value is split at the first `=`. An empty `OLD` and an empty or
  blank `NEW` are rejected.

Matching works on the text only and never touches the file system, so it
behaves the same on every platform. Names are not Unicode-normalized: a
composed and a decomposed accent stay two different paths.

### `--label-set SET`

Chooses the names of the colour labels: `en` (Red, Yellow, Green, Blue,
Purple), `de` (Rot, Gelb, Grün, Blau, Lila), `auto` (default) or the path
of a JSON file such as `{"My red": "red", "Todo": "yellow"}` whose values
are `red`, `yellow`, `green`, `blue` or `purple`. Names are compared exactly,
including letter case, spaces and Unicode form.

With `auto`, the English and German standard names count together: a label
that equals one of them exactly maps to its colour, whatever the other
labels are. Every other non-empty label is a custom label of its own photo
(converted without a label, reported by id only). Nothing else is guessed:
there is no recommendation. A missing or invalid file is a command-line
error (exit code 2).

### `--report FILE`

Writes a machine-readable report to `FILE` after the run:

```json
{
  "tool_version": "0.1.0",
  "catalog_version": "1500000",
  "dry_run": true,
  "catalog_unchanged": true,
  "folders": { "read": 2, "converted": 2, "skipped": [] },
  "photos": {
    "read": 3,
    "converted": 2,
    "skipped": [{ "reason": "photo 7: original file is missing" }],
    "videos": 0,
    "flags": { "pick": 1, "reject": 0, "none": 1, "unknown": 0 },
    "labels": {
      "red": 1, "yellow": 0, "green": 0, "blue": 0, "purple": 0,
      "none": 0, "custom": 1, "label_set": "en", "matched_sets": [],
      "custom_reasons": [{ "reason": "photo 5: custom label not in label set" }]
    }
  },
  "collections": {
    "read": 3,
    "converted": 2,
    "skipped": [{
      "reason": "collection 4: smart collection is not converted (criterion \"pick\" has no exact LightCraft equivalent)",
      "code": "smart_untranslatable"
    }],
    "by_code": { "smart_untranslatable": 1 },
    "albums": 1,
    "folders": 1,
    "duplicate_names": 0,
    "quick": 0,
    "smart": 0
  },
  "collection_members": {
    "read": 2,
    "converted": 1,
    "skipped": [{ "reason": "photo 7: skipped or unknown photo, not added to collection 2" }]
  },
  "stacks": {
    "read": 3,
    "converted": 2,
    "skipped": [{ "reason": "stack 3: fewer than two converted photos" }],
    "collapsed": 1,
    "no_top": 0
  },
  "stack_members": {
    "read": 7,
    "converted": 5,
    "skipped": [{ "reason": "photo 20: stack 3 is skipped" }, { "reason": "photo 21: skipped or unknown photo, not added to stack 3" }]
  },
  "keywords": {
    "read": 3,
    "converted": 2,
    "skipped": [{ "reason": "keyword 9: name is empty" }],
    "persons": 1,
    "not_exported": 0,
    "no_parents_on_export": 0,
    "no_synonyms_on_export": 0,
    "unknown_type": 0,
    "unused": 1
  },
  "keyword_assignments": {
    "read": 2,
    "converted": 1,
    "skipped": [{ "reason": "assignment 5: photo 7 is skipped or unknown" }]
  },
  "keyword_synonyms": {
    "read": 2,
    "converted": 1,
    "skipped": [{ "reason": "synonym 3: keyword 9 is skipped" }]
  },
  "ratings": { "0": 1, "1": 0, "2": 0, "3": 1, "4": 0, "5": 0 }
}
```

The other areas are shortened here: every area, and every skip list in
the `writer` section, has entries `{"reason": ..., "code": ...}` and a
`by_code` (`<list>_by_code` in `writer`) count per code. The top-level
`skipped_by_code` sums the codes over all catalog areas (not `writer`).
A code is a fixed snake_case key such as `missing`, `video`,
`smart_untranslatable` or `ambiguous`; it never holds a value from the
catalog. The summary prints the codes per area in `skip codes <area>:`
blocks after the reasons.

For each area, `read` equals `converted` plus the number of `skipped`
entries. Reasons name the catalog id, never a file path, a collection
name, a keyword or a synonym. `ratings` always has the keys `"0"` to `"5"`; the
counts add up to `photos.converted`. `photos.videos` is the number of
skipped videos; they are also listed in `photos.skipped`. `photos.flags`
counts the converted photos per flag; `none` does not include `unknown`, so
the four counts add up to `photos.converted`. `photos.labels` counts the
converted photos per colour label, names the label set (`auto`, `en`, `de`
or `file`), and under `auto` lists the standard sets that matched at least
one name in `matched_sets` (order `en`, `de`; empty for a fixed set); it
lists each photo with a custom label; its seven counts add
up to `photos.converted`. The six attribute
counts in `keywords` cover converted keywords only, like the summary line.
`collections.albums` and `collections.folders` count the converted
collections by kind; `collections.duplicate_names` counts converted
collections that repeat the exact name of an earlier sibling below the same
parent (two equal names count as 1). `stacks.collapsed` and
`stacks.no_top` count converted stacks only.

To protect the catalog and your files, `FILE` is checked before anything is
read, and lr2lc stops with exit code 2 if:

- `FILE` is the catalog or one of its `-wal`, `-journal` or `-shm` files,
  also when given through another path, a symbolic link or a hard link.
  On Unix (Linux, macOS) hard links are detected by file identity; on
  Windows only by comparing paths without regard to letter case;
- `FILE` already exists and is not an earlier lr2lc report (a JSON object
  with the key `tool_version`). An earlier report is overwritten; any other
  file, such as a photo, is left untouched.

### Writer plan in the dry run

The dry run also prints what the writer would send, as one line:

```text
writer: keywords to create 11, photos to import 17, updates 23, albums to create 4, album members to add 30
```

`albums to create` counts albums only, not album folders. A second line
`writer stacks: to group N, already in the library N, members left in
LightCraft N` follows.

`report.json` then has a `writer` section with `planned` (`keywords`,
`photos`, `updates`, `photos_updated`, `photos_unchanged`, `folders`,
`albums`, `members_added`, `members_removed`, `collections_known`,
`collections_removed`, `stacks`, `stacks_known`,
`stack_members_removed`), `photos_skipped`, `keywords_skipped`,
`members_skipped` and `stacks_skipped` (each a list of `{"reason": ...}`;
`members_skipped` holds album members whose photo is not converted or has
no global id).
`planned.photos` counts the imports, `photos_updated` the photos from the
mapping file with at least one update and `photos_unchanged` those that
need no command. Every converted photo counts once:
`photos.converted` equals `planned.photos` plus `planned.photos_updated`
plus `planned.photos_unchanged` plus the photos skipped by the writer.
Every converted collection counts once: `collections.converted` equals
`planned.folders` plus `planned.albums` plus `planned.collections_known`.
Every converted stack counts once: `stacks.converted` equals
`planned.stacks` plus `planned.stacks_known` plus the entries of
`stacks_skipped`.
With `--library DIR` the dry run reads the mapping file of that library
(see below), so it only plans what is still missing; it never connects to
LightCraft and writes nothing next to the library.

`--script FILE` also writes the plan as JSON lines, one `engine.execute`
request per command, with a `library.select` line before the updates of
each photo (see `--apply`); photos that are not imported yet are named
`lr:<global id>` in `ids`. The number of updates in the summary does not
count the selects. Album commands follow the photo commands; an album or
folder created by the same script is named `lr-collection:<Lightroom id>`
(as `parent` of `album.create` and as `id` of `album.addPhotos`), and each
member change has a `library.select` line of exactly its photos before it.
Stack commands come between the photo and the album commands: for each
stack a `library.select` of its photos with the top one active, then
`stack.group` with `ids`, `top` and `collapsed: false`.
The Quick Collection is not in the script: `album.toggleTarget` removes
photos that are all in it already, and only `--apply` reads its members
first.
The same protection as for `--report` applies:
the catalog and its companion files are refused, and an existing `FILE` is
only overwritten if it is empty or an earlier lr2lc script (every non-empty
line a JSON object with `"method":"engine.execute"`).

### `--apply`

```sh
lr2lc catalog.lrcat --library ~/Pictures/Photos.lightcraft --apply
```

Before the first real run, try it on a throw-away or test library: start
LightCraft with `--control <port>` (for example `--control 7980`) and a
library you can delete, run lr2lc with `--apply` against it and look at the
result. `--apply` changes the selection in LightCraft (see below), and the
photos, keywords and albums it creates stay in the library.

`--apply` needs `--library DIR`, the LightCraft library to write; without
it lr2lc stops with exit code 2. LightCraft must be running with that
library open and its control port enabled (start the app with `--control N`;
lr2lc uses `--port N`, default 7980, and contacts only `127.0.0.1`).
lr2lc reads the catalog as in a dry run, then checks the app before it writes anything and stops with exit code 1 if:

- the app runs a temporary in-memory session or has another library open;
- the app reports a problem with its library;
- the library writes XMP sidecars next to the originals. lr2lc never
  touches the originals, so it refuses this unless you turn XMP auto-write
  off in LightCraft or allow it explicitly with `--allow-xmp`.

Then it creates the missing keywords (parents first), imports each photo on
its own as a reference (`mode: add`, the file stays where it is) and sets
the fields that differ. LightCraft applies rating, flag, label and metadata
commands only to selected photos, so lr2lc selects each photo before its
first update; this changes the selection in LightCraft, which afterwards
holds the last updated photo. A select is not counted as an update.

After all photos, lr2lc creates the missing album folders (for collection
sets, parents first) and albums (for collections) with `album.create`,
sending only the name, `folder` and the parent folder; before the first
of them it reads `albums.list` once to find albums that already exist
(see [Mapping file](#mapping-file)). Then it adds the
members of each album with `album.addPhotos`, in Lightroom's order, and
removes photos that it added earlier but that are no longer in the
collection with `album.removePhotos`. LightCraft changes album members only
for a selection, so each of these calls comes directly after a
`library.select` of exactly its photos. lr2lc never removes photos it did
not add, and never renames, moves or deletes an album: a collection
renamed, moved or deleted in Lightroom is only counted in the report.
Albums with equal names are allowed and kept as they are.

Between the photos and the albums, lr2lc groups the stacks, ascending by
their Lightroom id: for each new stack, or one whose members or order
changed in Lightroom, it selects exactly its photos with the top photo
active and sends `stack.group` with the photos, the top photo and
`collapsed: false`. Stacks are always written expanded: LightCraft would
collapse a new stack, and the collapsed state of Lightroom is not taken
over. Members without a LightCraft id (for example a failed import) are
left out; a stack with fewer than two photos left is skipped. A photo that
left a stack in Lightroom stays in the LightCraft stack and is only
counted: lr2lc never ungroups, splits or removes anything from a stack.
A rejected select or group leaves the stack incomplete, and the next run
sends it again; an unchanged stack gets no command.

A command LightCraft rejects skips only that photo, keyword or collection,
with a reason in the report. A rejected `album.create` also skips every
collection below it; a rejected select or member change leaves the album
incomplete, and the next run sends its members again. If the connection is lost, lr2lc
stops at once and never repeats the command: its outcome is unknown, so run
lr2lc again; it re-checks the state and finds photos LightCraft already
has. The catalog checksum is checked again after writing.

The summary and `report.json` (`dry_run: false`) show what was written in
the `writer` section:

- `keywords_created`;
- `photos_imported`, and `photos_existing` (found in the library as
  duplicates on import);
- `photos_updated` (known from the mapping file, at least one update sent)
  and `photos_unchanged` (known, no command needed);
- `updates` (field updates applied) and `not_saved` (a step LightCraft
  applied but did not save to disk; the step is not recorded as done and the
  next run sends it again);
- `photos_skipped`, `photos_incomplete` (an update or the select before it
  failed, or a step was applied but not saved; the next run sends the updates
  again) and `keywords_skipped`, each with reasons;
- `folders_created`, `albums_created` and `collections_known` (already in
  the mapping file, no `album.create`), and `collections_existing` (missing
  from the mapping file but found in the library and taken over, see
  [Mapping file](#mapping-file));
- `members_added` and `members_removed` (photos added to or removed from
  albums);
- `collections_renamed`, `collections_moved` and `collections_removed`
  (changed in Lightroom since lr2lc created them; nothing is changed in
  LightCraft);
- `collections_skipped` (not created), `collections_incomplete` (a select
  or member change failed) and `members_skipped` (the photo is not
  converted, has no global id or has no LightCraft id), each with reasons.
  Reasons name Lightroom ids, never a collection name;
- `stacks_grouped`, `stacks_known` (in the mapping file with the same
  members in the same order, no command) and `stack_members_removed`
  (members that left a stack in Lightroom and stay in LightCraft);
- `stacks_skipped` (fewer than two photos with a LightCraft id) and
  `stacks_incomplete` (select or group rejected), each with reasons that
  name the Lightroom stack id.

Every converted photo counts once, in every run: `photos.converted` equals
`photos_imported` plus `photos_existing` plus `photos_updated` plus
`photos_unchanged` plus the photos skipped by the writer. A photo imported
or found as a duplicate in this run counts only there. The incomplete
photos are among the imported, existing or updated ones. Every converted
collection counts once, too: `collections.converted` equals
`folders_created` plus `albums_created` plus `collections_known` plus
`collections_existing` plus the entries of `collections_skipped`. The
incomplete collections are among the created, known or taken over ones.
Every converted stack counts once: `stacks.converted` equals
`stacks_grouped` plus `stacks_known` plus the entries of `stacks_skipped`
and `stacks_incomplete`.

If the run stops after it started (no connection, a failed pre-flight
check, a lost connection, an unexpected reply, or the mapping file cannot
be written), lr2lc still writes `report.json` when `--report` is given and
exits with code 1. The `writer` section then holds the numbers up to the
stop (a command without a reply counts nowhere; all are 0 if the
connection or the pre-flight check failed) and `aborted`, a short reason
without paths, e.g. `connection lost during library.import; outcome
unknown`. After a complete run `aborted` is absent.

### Mapping file

lr2lc remembers what it wrote in `<library folder name>.lr2lc.json` next to
the library folder, e.g. `Photos.lightcraft.lr2lc.json`. It maps each
photo's Lightroom global id to its LightCraft id and keeps the values last
written. It is saved after every finished step, so an interrupted run loses
nothing. A step LightCraft applied but did not save (`AppliedNotSaved`) is
not recorded: the next run sends it again (field updates are idempotent; a
keyword is looked up with `keyword.list` before it is created, so no
duplicate is made). A second run sends only what changed in Lightroom since
then and creates no duplicates. If the file is lost, the next run finds the
photos again as duplicates and the keywords in the library's keyword list.

Collections are kept under `collections`, keyed by the Lightroom collection
id: `{"lightcraft_id": ..., "kind": "album" | "folder", "name": ...,
"parent": <Lightroom id> | null, "members": [...]}`, with the name and
parent at creation and the sorted global ids of the photos lr2lc added.
Files from older versions without `collections` still load; the file
version stays 1. If the file is lost, the next run reads the library's
album tree (`albums.list`) once before it would create an album, and looks
for each collection missing from the file for an album or album folder
with the same name (trimmed as LightCraft does on create, otherwise byte
for byte), the same kind and the same parent (top level, or the folder
lr2lc knows or just found or created for the Lightroom parent). Smart
albums and albums already in the mapping file never match. With exactly
one match for exactly one collection, lr2lc takes that album over
(`collections_existing`), creates nothing and adds the members as usual
(adding is idempotent). Without a match it creates the album. Otherwise
(LightCraft allows equal names, so several albums can match, or several
collections can want the same album) lr2lc does not guess: the
collections are skipped with the reason `ambiguous: N albums with the same
name and parent in LightCraft`, together with everything below them, and
nothing is created for them. Known limitation: `albums.list` nests
folders only up to a depth limit, so an album below that depth is not
found and is created again.

Stacks are kept under `stacks`, keyed by the Lightroom stack id:
`{"members": [...]}`, the global ids of the grouped photos with the top
first, followed by members that left the stack in Lightroom (they are
still in the LightCraft stack). The entry is written as soon as the group
succeeded. Files without `stacks` still load.

### Smart collections

A smart collection becomes a smart album (`album.createSmart`) only if
every rule translates exactly; lr2lc never writes half a rule set.
Currently that holds only for `rating` with `==` or `>=` and a whole
number from 0 to 5, without a second value or units, combined with "all"
or "any", and placed at top level or below a collection set
(LightCraft nests smart albums only in folders). Any other smart
collection is skipped (`smart_untranslatable`, `smart_no_criteria` or
`smart_unreadable`). The reason never contains a value: it names the first
criterion without an exact equivalent; if all criteria translate but the
combine mode does not, it reads "the combine mode has no exact LightCraft
equivalent"; without any criteria it reads "no criteria".

With `--apply`, smart albums are written as the very last step. The
mapping file keeps them, so a second run creates nothing again. Rules
changed in Lightroom since lr2lc wrote the album are only counted
(`smart_rules_changed`); the smart album is not changed. If the mapping
file is lost, lr2lc takes over exactly one existing smart album with the
same name, parent and rules (`smart_existing`), also one you created by
hand. With several matches it does not guess: the collection is skipped
as `ambiguous` and nothing is created. A smart collection lr2lc wrote
earlier whose rules no longer translate or can no longer be read counts
as `smart_no_longer_translatable` and stays unchanged in LightCraft.

The dry run prints a line `writer smart albums: smart albums to create N,
already in the library N, rules changed in Lightroom N`; `report.json`
has `planned.smart_albums`, `planned.smart_known`,
`planned.smart_rules_changed` and `planned.smart_no_longer_translatable`.
After `--apply` the `writer` section has `smart_created`, `smart_known`,
`smart_existing`, `smart_rules_changed`, `smart_no_longer_translatable`
and `smart_skipped` (with `smart_skipped_by_code`). `collections.smart`
counts the smart collections converted as smart albums.

### Known limits

- Videos are skipped (`video files are not supported by LightCraft`).
- Virtual copies are skipped; so are their collection and stack memberships.
- Smart collections become smart albums only when every rule translates
  exactly, which currently means star-rating rules; all others are only
  reported, with a code.
- The collapsed or expanded state of stacks is only counted, not written;
  stacks are always written expanded.
- Development settings (the Lightroom edit state) are not converted yet.
- A second run sends what changed in Lightroom since the last run, compared
  with the mapping file. lr2lc does not yet compare these values with what
  LightCraft holds now, so a value you changed in LightCraft may be
  overwritten if the same value also changed in Lightroom.
- Renaming, moving and deleting collections, photos that left a stack and
  photos that left the Quick Collection are only counted, never applied.
  Photos that left a collection are removed from the album, and keywords
  removed in Lightroom are removed from the photo, but only what lr2lc wrote
  itself.
- The CI workflow runs format, lint and tests on Linux, macOS and Windows.
- Only `127.0.0.1` is contacted; a LightCraft app on another machine is not
  supported.

### Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Success (also for `--help` and `--version`) |
| 1 | The run failed: catalog cannot be opened or read, unsupported version, required table or column missing, pending `-wal`/`-journal` changes, mapping file damaged, report or script cannot be written, catalog changed during the run, or with `--apply`: no connection to LightCraft, a pre-flight check failed (wrong or temporary library, library problem, XMP auto-write) or the connection was lost |
| 2 | Invalid command line (unknown option, missing value, invalid `--map`, `--label-set` or `--port`, `--apply` without `--library`, refused `--report` or `--script` file); nothing is read or written |

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
