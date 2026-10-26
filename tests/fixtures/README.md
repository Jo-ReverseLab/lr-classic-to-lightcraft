# Generated test catalogs

All test catalogs are generated; none is copied from a real catalog. Every name,
path and value below is invented. Windows and macOS paths always use the user
name `example`.

The generator (a small crate, planned as `tests/generator`) writes a SQLite file
with the Lightroom Classic 15 layout (`Adobe_DBVersion` = `1500000`). Output is
deterministic: the same parameters and seed give a byte-identical file.

## Tables and columns

Only what the reader needs. Columns that are NOT NULL in the real schema get
fixed dummy values.

| Table | Columns filled |
| --- | --- |
| `Adobe_variablesTable` | `name`/`value` rows for `Adobe_DBVersion`, `AgLibraryKeyword_rootTagID` |
| `AgLibraryRootFolder` | `id_local`, `id_global`, `absolutePath`, `name`, `relativePathFromCatalog` |
| `AgLibraryFolder` | `id_local`, `id_global`, `parentId`, `pathFromRoot`, `rootFolder`, `visibility` |
| `AgLibraryFile` | `id_local`, `id_global`, `baseName`, `extension`, `folder`, `idx_filename`, `lc_idx_filename`, `lc_idx_filenameExtension`, `originalFilename` |
| `Adobe_images` | `id_local`, `id_global`, `rootFile`, `rating` (NULL or 0-5), `pick`, `masterImage` (virtual copies), plus NOT NULL dummies |
| `AgLibraryKeyword` | `id_local`, `id_global`, `name`, `lc_name`, `parent`, `genealogy`, `dateCreated`, include* flags |
| `AgLibraryKeywordImage` | `id_local`, `image`, `tag` |
| `Adobe_AdditionalMetadata`, `Adobe_imageProperties` | one row per image (the reference reader inner-joins them) |

Relationships follow the research notes: image -> file -> folder -> root folder;
keyword `parent` chain; keyword assignments via `AgLibraryKeywordImage`.

## Small fixture (fixed counts)

Committed generator output is not needed; tests call the generator. Defaults:

- 2 root folders (one macOS-style `/Users/example/Pictures`, one Windows-style
  `C:\Users\example\Pictures`), 6 folders in total
- 20 photos (including 1 virtual copy)
- 12 keywords, max depth 4 (e.g. `Travel` > `Italy` > `Rome` > `Colosseum`)
- 30 keyword assignments
- ratings: 5 photos with NULL, 3 with 0, and the rest spread over 1-5
- the same file name (`IMG_0001.jpg`) in two different folders
- 2 photos whose files do not exist on disk
- 1 photo with a path longer than 260 characters
- keywords with umlauts and emoji, a synonym-like pair, one keyword with no
  assigned photos (empty), one keyword with an empty name

Tests assert these exact numbers: read = converted + skipped, e.g. 20 photos
read, 17 converted, 3 skipped (with reasons). The exact split is defined in the
generator and kept in one constant table used by all tests.

## Parameters for large catalogs

| Parameter | Default | Meaning |
| --- | --- | --- |
| `--photos` | 20 | number of photos (e.g. 50000 for performance tests) |
| `--folders` | 6 | number of folders |
| `--keywords` | 12 | number of keywords |
| `--max-depth` | 4 | maximum keyword depth |
| `--keywords-per-photo` | 1.5 | average assignments per photo |
| `--seed` | 1 | seed for the deterministic generator |
| `--out` | required | output `.lrcat` path |

Large catalogs are written to a temporary directory and never committed.

## Checks every test set performs

- checksum of the catalog file is identical before and after a run
- read = converted + skipped
- a second run adds no duplicate photos or keywords
- without `--apply` nothing is written
