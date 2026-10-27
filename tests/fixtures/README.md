# Generated test catalogs

All test catalogs are generated; none is copied from a real catalog. Every name,
path and value below is invented. Windows and macOS paths always use the user
name `example`.

The generator is the workspace crate `lrcat-fixtures` in
`tests/fixtures/generator`. It writes a SQLite file with the Lightroom Classic 15
layout (`Adobe_DBVersion` = `1500000`). Output is deterministic: the same call
gives a byte-identical file. It is used as a library:

- `write_small_catalog(path)` writes the small fixture with fixed counts.
- `write_small_catalog_with_files(dir)` writes the same small catalog as
  `dir/small.lrcat` and returns its path. Both root folders lie below `dir`
  (`macos-pictures/`, `windows-pictures/`, built with `std::path`), and every
  original gets a tiny placeholder file (not a real image) except the files in
  `small::MISSING_FILE_IDS` (`AgLibraryFile.id_local` 2 and 3). Missing: one of
  the two `IMG_0001` files (Windows-style root) and a file shared with a
  virtual copy. The affected photos are `small::MISSING_PHOTO_IDS` (2, 3, 20).
- `write_large_catalog(path, images)` writes a catalog with exactly `images`
  photos for load tests.

There is no command-line interface (no `main.rs`) yet.

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
| `AgLibraryCollection` | `id_local`, `creationId`, `genealogy`, `imageCount`, `name`, `parent`, `systemOnly` |
| `AgLibraryCollectionImage` | `id_local`, `collection`, `image`, `pick`, `positionInCollection` |
| `Adobe_AdditionalMetadata`, `Adobe_imageProperties` | one row per image (the reference reader inner-joins them) |

Relationships follow the research notes: image -> file -> folder -> root folder;
keyword `parent` chain; keyword assignments via `AgLibraryKeywordImage`.

## Small fixture (fixed counts)

The generator is called by the tests; no generated file is committed. The
authoritative counts are the constants in the `small` module of the crate
(root folders, folders, photos, virtual copies, keywords, keyword depth,
assignments, ratings). Tests assert these constants. Contents:

- two root folders (one macOS-style `/Users/example/Pictures`, one Windows-style
  `C:\Users\example\Pictures`)
- one virtual copy
- keywords with a hierarchy up to depth 4 (e.g. `Travel` > `Italy` > `Rome` >
  `Colosseum`), plus a hidden root keyword row that is not counted
- ratings: NULL, 0 and 1-5 stars, counted per value
- the same file name (`IMG_0001`) in two different folders
- one folder path longer than 260 characters
- keywords with umlauts and emoji, a synonym-like pair, one keyword with no
  assigned photos, one keyword with an empty name

## Collections in the small fixture

Names are invented placeholders. Constants live in the `small` module.

| Kind | Rows | Ids | Notes |
| --- | --- | --- | --- |
| system (`systemOnly` = 1) | 2 | 7, 8 | id 8 also has the plain `creationId`; system wins |
| plain (`...library.collection`) | 3 | 2, 3, 4 | id 2 is inside set 1 |
| collection set (`...library.group`) | 1 | 1 | parent of id 2 |
| smart (`...library.smart_collection`) | 2 | 5, 6 | |
| unknown `creationId` | 1 | 9 | |
| total | 9 | 1-9 | `small::COLLECTIONS` |

`AgLibraryCollectionImage` has 7 rows (`small::COLLECTION_IMAGES`). Large
catalogs get 4 collections (`LARGE_COLLECTIONS`: set, plain, smart, system) and
up to 50 assignments.

## Large catalogs

`write_large_catalog` creates the requested number of photos spread over 100
folders, with 200 keywords and one assignment per photo. Large catalogs are
written to a temporary directory and never committed.

## Known gaps

- Missing originals are modelled only by `write_small_catalog_with_files`;
  `write_small_catalog` and large catalogs create no files on disk.
- The small fixture has no skipped-photo split; the read = converted + skipped
  expectations are defined by the tests of the reader and writer.

## Checks every test set performs

- checksum of the catalog file is identical before and after a run
- read = converted + skipped
- a second run adds no duplicate photos or keywords
- without `--apply` nothing is written
