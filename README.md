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
their members), smart collections, folder stacks, the Quick Collection and
development settings (sliders, tone curves, crop geometry, Upright mode,
white balance, masks and spots; the stored Lua table is parsed, never
executed).

What `--apply` writes into a LightCraft library, through the control port of
the running LightCraft app: keywords, photos (as references to the existing
files), ratings, flags, labels, titles, captions, copyright and creator,
albums and album folders with their members, smart albums (only when the rule
translates exactly), stacks (always written expanded), the Quick
Collection (only adding photos) and development settings (sliders, tone
curves, crop geometry, Upright mode, white balance, masks and spots).

Photos whose original file cannot be found are skipped; see
[Missing originals](#missing-originals) for what that means and how to
bring them over.

Without `--apply`, lr2lc only reports what it would do. Everything that is
not converted is listed with a reason in the summary and in `report.json`.

CI runs formatting, lint and tests on Linux, macOS and Windows (pull requests on Linux only).

## Desktop window

`lr2lc-gui` is a window for the same conversion: choose a catalog and a
LightCraft library, choose what to transfer, run a dry run or the
transfer, and read the run log. It runs `lr2lc-core` in its own process
and talks to LightCraft over the control port, like the command line.

```sh
cargo run --release -p lr2lc-gui   # needs Rust 1.95 or later
```

- **Transfer**: the catalog with its checks (file found, catalog version,
  open in Lightroom, LightCraft writing XMP automatically, photos without
  original), the target library with LightCraft's connection, path
  mappings (kept per catalog), the parts to transfer (the same as
  [`--skip`](#--skip-areas)), the advanced options (label names, control
  port, continue despite XMP auto-write) and a summary from a read-only
  scan of the catalog. A blocking check disables the buttons and says why.
  Dropping a `.lrcat` file on the window chooses it.
- **Running**: progress per phase in the order lr2lc works (read the
  catalog, read the library when needed, keywords, photos with their
  metadata and develop settings, albums and stacks), counters and the
  last log entries. **Stop** halts at the next photo and keeps what is
  transferred; a new run continues without duplicates. **Cancel** (after a
  confirmation) halts and takes the run back: photos, virtual copies,
  albums, keywords and stacks it added are removed from the library again
  (records only, never the original files), album members, Quick
  Collection photos and fields it changed are set back, and the mapping
  file is restored. Development settings it wrote to photos that were
  already in the library cannot be set back over the control port; the log
  lists them (`develop_not_reset`). `report.json` then has a `rollback`
  section with counts.
- **Log**: every run's [run log](#run-log), filterable by status, area and
  text, with details per entry. Runs are kept in the app's data folder
  (`~/Library/Application Support/lr2lc/runs` on macOS); **Save
  report.json** saves the report with names or, with **Anonymized**,
  without them.

The theme follows the system and can be switched in the top bar. The
window uses the Inter font (SIL Open Font License, see
`crates/lr2lc-gui/assets/fonts`).

Not available yet: **Retry this photo** and **Overwrite with Lightroom
state** in the Log are disabled until lr2lc can run a single photo
([#36](https://github.com/Ben-ReverseLab/lr2lc/issues/36)). There is no
pause, and the spinner does not follow the system's reduced-motion
setting.

## Options

| Option | Purpose |
| --- | --- |
| `--apply` | Write the LightCraft library (default: dry run); needs `--library` |
| `--library DIR` | The LightCraft library to write; also used by the dry run to read the mapping file |
| `--port N` | Control port of the running LightCraft app (default 7980) |
| `--allow-xmp` | Write even if the library writes XMP sidecars next to the originals |
| `--overwrite-develop` | Also rewrite the development settings of known photos that changed in Lightroom |
| `--probe-white-balance` | Read an original file to derive the white-balance scale (not implemented yet: the run stops with exit code 2) |
| `--map OLD=NEW` | Replace a path prefix, repeatable |
| `--skip AREAS` | Leave parts of the catalog out (comma-separated, see below) |
| `--label-set SET` | Names of the colour labels: `en`, `de`, `auto` (default) or a JSON file |
| `--report FILE` | Write `report.json`, with a run log |
| `--anonymize` | Leave every name, path and value of the catalog out of `report.json` |
| `--script FILE` | Write the planned commands as JSON lines |

Each option is described below.

Every option except `--map` may be given at most once. Repeating one of
`--library`, `--script`, `--report`, `--port` or `--label-set` ends with
exit code 2 before anything is read. `--report` and `--script` must not
name the same file either; if they do, lr2lc stops with exit code 2 so the
two outputs cannot overwrite each other.

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
example a missing original), if it repeats a photo
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
row id. A member is skipped if its photo is not converted (a video, a missing
original or an unknown id) or if the photo is already in a
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

A virtual copy is converted together with its master: it keeps its own
rating, flag, label, keywords, title and caption, and the writer creates it
once in LightCraft with `photo.virtualCopy` (recorded in the mapping file, so
a later run never creates it twice). A copy whose master is not converted (a
video, a missing original, ...) is skipped with the reason
`virtual copy of ... cannot be created`; its collection and stack memberships
are skipped with it.

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

Folders are the exception: a folder is identified by its absolute path
(the root folder's `absolutePath` followed by `pathFromRoot`), not by a
global id. Folders are read fresh every run and are not kept in the mapping
file. A folder moved or renamed in Lightroom therefore simply appears under
its new path in the next report; the old path is gone and nothing is
reconciled or reported about it. (Photos inside keep their global id, so a
moved folder does not make lr2lc re-import its photos.)

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

### Missing originals

lr2lc imports each photo into LightCraft as a reference to its original
file, so the file has to be there. Before anything is planned it checks
every photo's original at its path (after `--map`). A photo whose file is
not found, or cannot be checked, is skipped with the code `missing` and the
reason `photo N: original file is missing`. Everything that belongs to the
photo stays behind with it: rating, flag, label, keywords, title and
caption, album and stack memberships, virtual copies and development
settings. The summary counts them in the line `N x missing` under
`skip codes photos:`, and `report.json` lists them under `photos.skipped`
with the code `missing`; no path is printed.

Typical causes are an external disk or network share that is not connected,
photos that moved to another disk or computer, and originals that were
deleted.

To bring them over:

1. Make the originals reachable: connect the disk or mount the share at the
   path Lightroom knows. If they now live somewhere else, give the old and
   the new location with [`--map OLD=NEW`](#--map-oldnew).
2. Run a dry run and check that the `missing` count dropped to the photos
   you expect.
3. Run `--apply` again with the same `--library`. Photos converted earlier
   are recognised through the [mapping file](#mapping-file) and not
   imported twice; the photos that were missing before are added with their
   metadata, keywords, albums and development settings, as on a first run.

Photos whose originals are gone for good cannot be converted by lr2lc.
LightCraft's own Lightroom import (File › Import Lightroom Catalog…) keeps
such photos as offline records that can be relinked later, but it has no
re-sync, report or mapping of its own. lr2lc cannot create offline records
yet: LightCraft's control port has no command for it. Support has been
requested from the LightCraft project
([storytold/lightcraft#4392](https://github.com/storytold/lightcraft/issues/4392));
once it exists, lr2lc will offer it as an option.

### `--skip AREAS`

Leaves parts of the catalog out of the run. `AREAS` is a comma-separated
list of `keywords`, `ratings`, `labels`, `flags`, `texts` (title, caption,
copyright and creator), `collections` (with smart collections), `stacks`
(with the Quick Collection), `virtual-copies` and `develop`. Folders and
photos are always converted.

```sh
lr2lc --skip develop,keywords --library ~/Pictures/Lib --apply catalog.lrcat
```

A part that is left out sends no command and is not compared. Its fields
keep the value of the last import in the mapping file (a new photo keeps
LightCraft's import defaults), and items recorded earlier are not reported
as removed. A later run without `--skip` therefore writes every Lightroom
value of that part. The summary starts the writer part with `not selected:
…` and `report.json` lists the names under `not_selected`; both are absent
when nothing is left out. An unknown name is a command-line error (exit
code 2).

### `--label-set SET`

Chooses the names of the colour labels: `en` (Red, Yellow, Green, Blue,
Purple), `de` (Rot, Gelb, Grün, Blau, Lila), `auto` (default) or the path
of a JSON file such as `{"My red": "red", "Todo": "yellow"}` whose values
are `red`, `yellow`, `green`, `blue` or `purple`. Names are compared exactly,
including letter case, spaces and Unicode form.

With `auto`, lr2lc shows what Lightroom shows. Lightroom gives a label its
colour only when the stored text equals a name of the label set that is
active in Lightroom; every other text appears without colour (for example
`Red` in a German Lightroom). The active set is not stored in the catalog,
so `auto` detects it: the English or German standard set, whichever names
label more converted photos, English on a tie. Only that set's names map to
colours; every other non-empty label, a name of the other set included, is
a custom label of its own photo (converted without a label, reported by id
only). The summary names the detected set (`set auto (detected de)`). If
the detection is wrong for your catalog, pass `--label-set en` or `de`. A
missing or invalid file is a command-line error (exit code 2).

French and Spanish standard names are not in `auto` yet: they are pending a
citable Adobe source for each name. See `docs/research/label-names.md` for
the candidates, the sources attempted and what is still open. A guard test
(`no_standard_name_maps_to_two_colors`) keeps any future set from clashing
with an existing name.

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
      "none": 0, "custom": 1, "label_set": "en", "matched_sets": [], "detected_set": null,
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
    "unused": 1,
    "trimmed": 0
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
one name in `matched_sets` (order `en`, `de`; empty for a fixed set) and
the set it used in `detected_set` (`en`, `de` or `null`); it
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

### Run log

`report.json` ends with the run log: `entry_counts` (entries per status)
and `entries`, one entry per thing that was done, adjusted, skipped, in
conflict or failed: what the catalog read found first, then what the run
did in the order it did it, then the failures and skips by area. The summary counts them in a line
`log entries (one per photo, field, keyword and album): adjusted N, …`,
after a line that counts each photo once: `photos: N in the catalog,
transferred N, unchanged N, skipped N, conflict N, error N` (also in
`report.json` as `photo_counts`).

| Status | Meaning |
| --- | --- |
| `transferred` | Done as in Lightroom (in a dry run: would be done) |
| `adjusted` | Done, but changed on purpose to fit LightCraft (for example a relative white balance converted to LightCraft's scale) |
| `skipped` | Deliberately not done: not supported yet, not selected (`--skip`), or nothing to do; has a code and a reason |
| `conflict` | Changed in Lightroom and in LightCraft since the last import; LightCraft's state is kept |
| `error` | Should have been done but failed; has a code, a reason and a recommendation |

Each entry has `status`, `area` (`folders`, `photos`, `keywords`,
`ratings`, `labels`, `flags`, `metadata`, `collections`, `stacks`,
`virtual-copies`, `develop`), `stage` (`read`, `keywords`, `photos`,
`organize`), `item` (a file, keyword or album name), `message`, and where
they apply `code` (the skip code), `reason`, `recommendation`, `source`
(the value in Lightroom), `result` (the value in LightCraft) and `photo`
(the photo's Lightroom id). Messages, reasons and recommendations never
contain a name, a path or a value; those are only in `item`, `source` and
`result`.

The report holds names, paths and values by default. `--anonymize` leaves
them out: `item` becomes an id such as `photo 7` or `collection 3`,
`source` and `result` are dropped, and `anonymized` is `true`. Use it before
sharing a report. The readable summary never holds a name, path or value.

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
`stack_members_removed`), `photos_skipped`, `keywords_not_created`,
`members_skipped` and `stacks_skipped` (each a list of `{"reason": ...}`;
`members_skipped` holds album members whose photo is not converted or has
no global id). `keywords_not_created` is the writer-side set: keywords the
run did not create (their parent failed, LightCraft rejected them, or their
synonyms or attributes changed after creation). It is distinct from
`keywords.skipped` in the reader area, which counts keywords skipped while
reading the catalog.
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

### Development settings

lr2lc writes the development settings that have an exact LightCraft
equivalent. Slider values (Light, HDR/SDR, colour, mixer, effects, detail,
optics, geometry and calibration) go as one `develop.set` per photo; that
command names its photo, so no selection is needed. Structured parts — the
colour/B&W treatment, `hdr.enabled`, `optics.remove_ca` and the white-balance
mode — go as one `develop.merge` directly after a `library.select`, because
LightCraft applies it to the selection. A photo that needs both gets the
`develop.set` first.

A field lr2lc cannot carry over exactly is never written; it is collected
and reported by name in `unmapped`.

White balance follows LightCraft's own Lightroom import. A named mode
(As Shot, Auto, Daylight, …) is carried by the mode alone; Lightroom's stored
numbers for it are not written. A custom white balance carries its numbers:
a relative pair (`IncrementalTemperature`/`IncrementalTint`, stored for
JPEG, TIFF and other rendered files) is converted to LightCraft's relative
scale (0.8 mired per step around 6500 K) and written. A Kelvin pair
(`Temperature`/`Tint`, stored for raws and DNG) is written as is only when
LightCraft edits the photo in Kelvin: after the import lr2lc asks
`photo.inspect`, and only a raw developed from its sensor data with
`wb.scale` `kelvin` qualifies (DNG files and cameras LightCraft knows a
Kelvin scale for). On any other photo the Kelvin value would mean something
else, so it is not carried and only counted. lr2lc never opens an original
photo for this; `--probe-white-balance` stays refused with exit code 2. The
`develop` area counts `white_balance_custom` (converted photos with a custom
pair), `white_balance_to_check` (dry run: Kelvin pairs whose transfer
LightCraft decides), `white_balance_written` and `white_balance_not_carried`
(applied run); only numbers, never a value or a photo id.

Tone curves and crop go in the same `develop.merge`. The 2012 curve fields
become `curve.master`, `curve.red`, `curve.green` and `curve.blue` as arrays
of `{"x", "y"}` points in 0..1; a channel list is written only when all three
channels and the master exist, and a straight identity line counts as no
curve. The legacy `ToneCurve` is used only when no 2012 curve is present.
The crop becomes `crop.geometry` with its rectangle and angle, transformed
into the displayed frame through the stored orientation and image size the
same way LightCraft's catalog import does. The angle appears only inside
`crop.geometry`, never as a control.

Upright goes in the same `develop.merge` as `geometry.upright`: Lightroom's
Auto, Level, Vertical and Full become `auto`, `level`, `vertical` and
`full`, and LightCraft computes the correction from the photo itself, as its
own importer does. Guided Upright is not carried over and is reported as
`PerspectiveUpright`; its guide lines would have to be converted into
LightCraft's frame. The other `Upright*` fields only matter while Upright is
on, so they are reported only for photos that use it.

Masks (local corrections) go in the same `develop.merge` as LightCraft's
`masks` array: each mask carries an id, the Lightroom correction name or
`Mask N`, its visibility, inversion, typed components (linear, radial, brush,
luminance range, depth range, subject, sky, background) with their blend
operation, and a
sparse `adjust` object holding the local sliders that were set plus `amount`
(scaled to 0..200). The four Lightroom mask containers are read in a fixed
order, so the ids and default names are stable. Lightroom's current brush
masks (a group of painted strokes, including erased parts) become brush
strokes; the AI masks Subject, Sky and Background become `subject`, `sky`
and `background`. A mask LightCraft cannot express — a colour range, an AI
object or people part (`Mask: AI mask (N)`; LightCraft would need the
selection itself, not just its type), a grain slider, an empty brush, a
brush group with other parts (`Mask: Aggregate (…)`) — is reported under a
stable name such as `Mask: colour range` in `unmapped`, never written as a
partial mask. Masks travel only through
`develop.merge`, never `develop.set`, and the develop hash covers them, so a
second run plans nothing.

Spot removal and healing go in the same `develop.merge` as LightCraft's
`spots` array. Each spot carries its mode (`heal`, `clone`, `remove` or
`fill`), its target points (one point for an ellipse or circle, the brush
path for a painted spot), a size in long-edge units, feather and opacity on
LightCraft's 0..100 scale and an optional source offset. The current
`RetouchAreas` container is read first; the older `RetouchInfo` lines are
used only when it yields no spot, with `size = radius × min(aspect, 1)` and
no feather. A spot LightCraft cannot express is reported under a stable name
(`Spot: <kind>`, `Spot: no mask`, `Spot: empty brush`, `Spot: legacy line`)
in `unmapped`, never written as a partial spot. Spots travel only through
`develop.merge`, never `develop.set`, and the develop hash covers them too.
Lightroom's generative Remove (`RemoveAreas`) cannot be reproduced and is
reported by name.

The camera profile (`CameraProfile`) is kept byte for byte in the model but
never written: there is no profile or LUT converter, and a photo that only
carries a profile plans no develop command. The report groups converted
photos by profile name (a missing or empty name is no profile) and the
summary counts them.

The mapping file records `develop_hash`, the SHA-256 of the settings text
last written for a photo. On a later run the settings are compared by that
hash:

- a new photo, or a known photo without a hash (development left out with
  `--skip develop`, or written by an older lr2lc), is written, unless
  LightCraft shows that photo as edited: then it is left alone and counted
  as `not_applied` like a changed hash, so an edit made in LightCraft is
  never overwritten silently. A photo whose earlier write lr2lc began but
  did not finish (the mapping file marks it with `develop_started`) is
  written again;
- an unchanged hash sends nothing;
- a changed hash of a known photo is left alone by default and counted as
  `not_applied`, because the settings may have been edited in Lightroom
  after the import. `--overwrite-develop` writes them and counts the photo
  as `overwritten`.

The dry run plans the same commands. The summary has one line
`develop: photos with settings N, commands planned N, written N, unmapped
fields N (N listed), changed and not applied N, overwritten N, photos with
camera profile N, photos with masks N, photos with spots N, custom white
balance N (to check N, written N, not carried N)`, and `report.json`
carries the same numbers in its top-level `develop` area (`photos`, `planned`,
`written`, `unmapped`, `unmapped_count`, `not_applied`, `overwritten`,
`camera_profiles`, `camera_profiles_count`, `photos_with_profile`, `masks`,
`spots`, `white_balance_custom`, `white_balance_to_check`,
`white_balance_written`, `white_balance_not_carried`). `written` is 0 in a
dry run.
`unmapped` lists at most 20 distinct field names, sorted; `unmapped_count`
is the full count. `camera_profiles` maps each profile name to the number of
converted photos, sorted, at most 20 entries (empty with `--anonymize`,
since the names are catalog values); `camera_profiles_count` is the
full number of distinct names and `photos_with_profile` the number of
converted photos with a profile. Neither the report nor the summary ever
holds a rendering value.

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

Then it creates the missing keywords (parents first), imports each photo as
a reference (`mode: add`, the file stays where it is; from 100 photos on in
batches of 64, each photo still recorded on its own) and sets the fields
that differ. LightCraft applies rating, flag, label and metadata
commands only to selected photos, so lr2lc selects each photo before its
first update; this changes the selection in LightCraft, which afterwards
holds the last updated photo. A select is not counted as an update.

#### Second run and reconciliation

On a second run lr2lc compares three values for every field of a photo that
is in the mapping file and still in LightCraft: the value it last imported
(`last`), the value in the Lightroom catalog (`lr`) and the value LightCraft
holds now (`lc`). It reads the live values from the app before it writes
anything; a failed read stops the run with nothing written.

- `lr == last`: nothing is written, whatever LightCraft holds. A value
  changed only in LightCraft is never overwritten.
- `lr != last` and `lc == last`: the Lightroom value is written.
- `lr != last` and `lc == lr`: nothing is written; both sides already agree.
- `lr != last`, `lc != last` and `lc != lr`: a conflict. Nothing is written,
  and the report names only the photo's Lightroom id and the field
  (`rating`, `flag`, `label`, `title`, `caption`, `copyright` or `creator`),
  never a value.

Keywords are compared element by element: a keyword new in Lightroom is
added unless LightCraft already has it; a keyword that left the photo in
Lightroom is kept in LightCraft and only counted (`keywords_removed`, in
the report and the summary); a keyword only in LightCraft stays. lr2lc
never removes a keyword from a photo. A field in conflict keeps its last
imported value in the mapping file, so a later run reports it again until
one side changes. The live values are read only when a known photo actually
changed in Lightroom, so a run with nothing to do sends no extra command. A
dry run never connects to LightCraft and therefore reports no conflicts.

lr2lc also reports items in the mapping file that changed or disappeared in
Lightroom. It never deletes or renames anything in LightCraft, and the lists
below carry Lightroom ids only, never names or paths.

- A photo no longer in the Lightroom model is counted in `photos_removed` and
  listed in `removed_photos`. lr2lc takes it only out of the albums it created
  itself (recorded in the mapping file), never out of an album the user made.
- A photo whose file path changed while its global id stayed is counted in
  `photos_moved` and listed in `moved_photos`; it is not imported again. The
  mapping file keeps the path of the first import as the comparison base, so
  a later change of the `--map` prefix also makes a photo appear as moved, and
  the move is reported on every later run.
- A keyword whose global id stayed but whose path text changed in Lightroom
  (renamed, or moved under a different parent keyword) is counted in
  `keywords_renamed` and listed in `renamed_keywords`; no keyword is created
  or renamed, and the new name is never added to a photo. A renamed parent
  also changes the path text of its descendants, so those are reported too.

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
- `photos_same_content` (part of `photos_imported`): photos whose file has
  the same bytes as another photo of the catalog, for example the same file
  copied into two folders. LightCraft's import would merge such files into
  one record, so lr2lc gives each its own record: a copy of the first one,
  linked to its own file and reset to import defaults. LightCraft shows it
  as a copy named after the file;
- `photos_updated` (known from the mapping file, at least one update sent)
  and `photos_unchanged` (known, no command needed);
- `updates` (field updates applied) and `not_saved` (a step LightCraft
  applied but did not save to disk; the step is not recorded as done and the
  next run sends it again);
- `photos_skipped`, `photos_incomplete` (an update or the select before it
  failed, or a step was applied but not saved; the next run sends the updates
  again) and `keywords_not_created` (keywords the run did not create;
  distinct from the reader's `keywords.skipped`), each with reasons;
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
written. A record the next run could not find again (a created keyword,
album or virtual copy) is saved at once; imports and field values, which
the next run finds again by path or compares anew, are saved at most once a
second and on every exit, so an interrupted run loses nothing. A step LightCraft applied but did not save (`AppliedNotSaved`) is
not recorded: the next run sends it again (field updates are idempotent; a
keyword is looked up with `keyword.list` before it is created, so no
duplicate is made). A second run sends only what changed in Lightroom since
then and creates no duplicates. If the file is lost, the next run finds the
photos again as duplicates and the keywords in the library's keyword list.

A photo entry also gains `develop_hash` once its development settings were
written (see [Development settings](#development-settings)). Files from an
older lr2lc without the field still load; such a known photo is written
once, which records the hash, unless LightCraft shows it as edited.
`develop_started: true` marks a photo whose development write began and
has no hash yet; it is cleared when the hash is recorded.

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
- A virtual copy is converted with its master; a copy whose master is not
  converted (for example a video) is skipped with its memberships.
- Smart collections become smart albums only when every rule translates
  exactly, which currently means star-rating rules; all others are only
  reported, with a code.
- The collapsed or expanded state of stacks is only counted, not written;
  stacks are always written expanded.
- Photos whose original file is missing are skipped with everything that
  belongs to them; LightCraft offers no way yet to add them as offline
  records over the control port (see [Missing originals](#missing-originals)).
- Development settings are written as sliders, tone curves, crop geometry,
  Upright mode, white balance, masks and spots. Not carried over, only
  reported by name: the camera profile and look, Guided Upright, colour range
  masks, AI object and people-part masks, generative Remove, a Kelvin white
  balance on a photo LightCraft does not edit in Kelvin, and every other
  unmapped field (see [Development settings](#development-settings)).
  `--probe-white-balance` (reading the original file) is not implemented and
  is refused with exit code 2 before the catalog is opened.
- A known photo whose development settings changed in Lightroom is left alone
  unless `--overwrite-develop` is given (see
  [Development settings](#development-settings)).
- A second run reconciles every field of a known photo with what LightCraft
  holds now (see [Second run and reconciliation](#second-run-and-reconciliation)):
  a value changed differently on both sides is reported as a conflict with
  the photo id and field name only and is not written.
- Renaming, moving and deleting collections, photos that left a stack, photos
  that left the Quick Collection, photos that disappeared from Lightroom,
  photos whose file moved and keywords that left a photo or were renamed in
  Lightroom are only reported, never applied. A moved photo is not imported
  again, and a renamed keyword is neither created nor added. Photos that left
  a collection are removed from the album, but only what lr2lc wrote itself.
- The CI workflow runs format, lint and tests on Linux, macOS and Windows; pull requests run on Linux only.
- Only `127.0.0.1` is contacted; a LightCraft app on another machine is not
  supported.
- LightCraft reads an XMP sidecar next to an original when it imports the
  file (rating, label, keywords, development settings). lr2lc records a new
  photo with LightCraft's import defaults and writes only the Lightroom
  values that differ from them, so a value from a sidecar that Lightroom
  does not have (for example stars in the sidecar, none in the catalog) can
  stay in LightCraft. Turn off “Automatically write changes into XMP” in
  Lightroom, or remove stale sidecars, before the first transfer.

### Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Success (also for `--help` and `--version`) |
| 1 | The run failed: catalog cannot be opened or read, unsupported version, required table or column missing, pending `-wal`/`-journal` changes, mapping file damaged, report or script cannot be written, catalog changed during the run, or with `--apply`: no connection to LightCraft, a pre-flight check failed (wrong or temporary library, library problem, XMP auto-write) or the connection was lost |
| 2 | Invalid command line (unknown option, missing value, invalid `--map`, `--label-set` or `--port`, `--apply` without `--library`, an option given more than once, `--report` and `--script` naming the same file, refused `--report` or `--script` file); nothing is read or written |

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
