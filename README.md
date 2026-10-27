# lr2lc

lr2lc converts Adobe Lightroom Classic catalogs (`.lrcat`) into LightCraft
libraries. It reads the catalog read-only, never touches the original photos,
and performs a dry run unless `--apply` is given.

This project is not affiliated with or endorsed by Adobe. "Lightroom" is used
only to describe compatibility.

## Status

Early development. The dry run reads folders and photos from a catalog,
applies path mappings and checks that the original files exist. It also
reads keywords with their hierarchy, their assignments to photos and the
star ratings. Collections are read and counted, but not converted yet: each
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
many folders, photos, collections, keywords and keyword assignments it read,
how many it would convert and how many it skips, with the reasons grouped.
A last line shows how many of the converted photos have 0 to 5 stars
(unrated photos count as 0):

```text
keywords: read 12, would convert 11, skipped 1
keyword assignments: read 30, would convert 25, skipped 5
ratings: 0: 6, 1: 3, 2: 2, 3: 3, 4: 2, 5: 1
```

Keywords are only assigned to photos that are converted, so assignments to
skipped photos are skipped too. Nothing is written except the optional
report file.

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
    "skipped": [{ "reason": "photo 7: original file is missing" }]
  },
  "collections": {
    "read": 1,
    "converted": 0,
    "skipped": [{ "reason": "collection 4: smart collection is not converted" }]
  },
  "keywords": {
    "read": 3,
    "converted": 2,
    "skipped": [{ "reason": "keyword 9: name is empty" }]
  },
  "keyword_assignments": {
    "read": 2,
    "converted": 1,
    "skipped": [{ "reason": "assignment 5: photo 7 is skipped or unknown" }]
  },
  "ratings": { "0": 1, "1": 0, "2": 0, "3": 1, "4": 0, "5": 0 }
}
```

For each area, `read` equals `converted` plus the number of `skipped`
entries. Reasons name the catalog id, never a file path, a collection
name or a keyword. `ratings` always has the keys `"0"` to `"5"`; the
counts add up to `photos.converted`.

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
