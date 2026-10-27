# lr2lc

lr2lc converts Adobe Lightroom Classic catalogs (`.lrcat`) into LightCraft
libraries. It reads the catalog read-only, never touches the original photos,
and performs a dry run unless `--apply` is given.

This project is not affiliated with or endorsed by Adobe. "Lightroom" is used
only to describe compatibility.

## Status

Early development. The dry run reads folders and photos from a catalog,
applies path mappings and checks that the original files exist. It also
reads keywords with their hierarchy and synonyms, their assignments to
photos, the star ratings and the pick/reject flags. Collections are read and counted, but not converted yet: each
one is listed as skipped.
Writing to LightCraft (`--apply`) is not implemented yet.

Once the repository is on GitHub, CI runs formatting, lint and tests on
Linux, macOS and Windows.

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
many folders, photos, collections, keywords, keyword assignments and keyword
synonyms it read,
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

LightCraft cannot read videos, so they are skipped with the reason
`video files are not supported by LightCraft`. A photo counts as a video if
Lightroom stores its format as `VIDEO` or its file extension is one of mov,
mp4, m4v, avi, mts, m2ts, 3gp, mpg, mpeg, wmv or mkv (any letter case).
Virtual copies keep their own reason, even of a video.

Each photo keeps its flag: picked, rejected or none. A stored flag value
other than pick, reject or none is converted as no flag and counted as
`unknown`; the photo itself is still converted. The four flag counts add up
to the converted photos.

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
- If several rules match, the longest `OLD` prefix wins.
- The rest of the path uses the separator style of `NEW`: `\` if `NEW`
  contains `\` or starts with a drive letter, `/` otherwise.
- The value is split at the first `=`. An empty `OLD` and an empty or
  blank `NEW` are rejected.

Matching works on the text only and never touches the file system, so it
behaves the same on every platform.

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
    "flags": { "pick": 1, "reject": 0, "none": 1, "unknown": 0 }
  },
  "collections": {
    "read": 1,
    "converted": 0,
    "skipped": [{ "reason": "collection 4: smart collection is not converted" }]
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

For each area, `read` equals `converted` plus the number of `skipped`
entries. Reasons name the catalog id, never a file path, a collection
name, a keyword or a synonym. `ratings` always has the keys `"0"` to `"5"`; the
counts add up to `photos.converted`. `photos.videos` is the number of
skipped videos; they are also listed in `photos.skipped`. `photos.flags`
counts the converted photos per flag; `none` does not include `unknown`, so
the four counts add up to `photos.converted`. The six attribute
counts in `keywords` cover converted keywords only, like the summary line.

To protect the catalog and your files, `FILE` is checked before anything is
read, and lr2lc stops with exit code 2 if:

- `FILE` is the catalog or one of its `-wal`, `-journal` or `-shm` files,
  also when given through another path, a symbolic link or a hard link.
  On Unix (Linux, macOS) hard links are detected by file identity; on
  Windows only by comparing paths without regard to letter case;
- `FILE` already exists and is not an earlier lr2lc report (a JSON object
  with the key `tool_version`). An earlier report is overwritten; any other
  file, such as a photo, is left untouched.

### `--apply`

Will write the LightCraft library. Not implemented yet: lr2lc exits with an
error and reads or writes nothing.

### Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Success (also for `--help` and `--version`) |
| 1 | The run failed: catalog cannot be opened or read, unsupported version, required table or column missing, pending `-wal`/`-journal` changes, report cannot be written, catalog changed during the run, or `--apply` given |
| 2 | Invalid command line (unknown option, missing value, invalid `--map`, refused `--report` file); nothing is read or written |

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
