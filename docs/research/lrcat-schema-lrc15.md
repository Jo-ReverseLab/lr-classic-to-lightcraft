# Lightroom Classic 15 catalog schema (relevant tables)

Structure only: table names, column names and declared SQLite types.
Observed on a Lightroom Classic 15 catalog (`Adobe_DBVersion` = `1500000`).
An empty type means the column has no declared type (SQLite affinity NONE).
"NN" = NOT NULL, "PK" = primary key. No catalog values are recorded here.

## Version table: `Adobe_variablesTable`

| Column | Type | Notes |
| --- | --- | --- |
| id_local | INTEGER PK | |
| id_global | (none) NN | |
| name | (none) | key, e.g. `Adobe_DBVersion`, `Adobe_CrsVersion`, `AgLibraryKeyword_rootTagID` |
| type | (none) | |
| value | (none) NN | |

## Images: `Adobe_images`

| Column | Type |
| --- | --- |
| id_local | INTEGER PK |
| id_global | NN |
| aspectRatioCache, bitDepth, colorChannels, colorLabels, colorMode, copyCreationTime | NN, no type |
| captureTime, copyName, copyReason, developSettingsIDCache, fileHeight, fileWidth, orientation, originalCaptureTime, panningDistanceH, panningDistanceV, propertiesCache, pyramidIDCache, rating, sidecarStatus | no type, nullable |
| editLock | INTEGER NN |
| fileFormat, pick, positionInFolder, touchCount, touchTime | NN, no type |
| hasMissingSidecars | INTEGER |
| masterImage | INTEGER (nullable; set on virtual copies, refers to the master image) |
| originalRootEntity | INTEGER |
| rootFile | INTEGER NN (refers to `AgLibraryFile.id_local`) |

`rating` and `pick` have no declared type.

## Files: `AgLibraryFile`

id_local INTEGER PK; id_global NN; baseName NN; extension NN;
folder INTEGER NN (refers to `AgLibraryFolder.id_local`);
idx_filename NN; lc_idx_filename NN; lc_idx_filenameExtension NN; originalFilename NN;
errorMessage, errorTime, externalModTime, importHash, md5, modTime, sidecarExtensions (nullable, no type).

## Folders: `AgLibraryFolder`

id_local INTEGER PK; id_global NN; parentId INTEGER (nullable);
pathFromRoot NN; rootFolder INTEGER NN (refers to `AgLibraryRootFolder.id_local`); visibility INTEGER.

## Root folders: `AgLibraryRootFolder`

id_local INTEGER PK; id_global NN; absolutePath NN; name NN; relativePathFromCatalog (nullable).

## Keywords: `AgLibraryKeyword`

id_local INTEGER PK; id_global NN; dateCreated NN; genealogy NN;
includeOnExport INTEGER NN; includeParents INTEGER NN; includeSynonyms INTEGER NN;
imageCountCache, keywordType, lastApplied, lc_name, name (nullable, no type);
parent INTEGER (nullable; refers to `AgLibraryKeyword.id_local`).

## Keyword assignment: `AgLibraryKeywordImage`

id_local INTEGER PK; image INTEGER NN (refers to `Adobe_images.id_local`);
tag INTEGER NN (refers to `AgLibraryKeyword.id_local`).

## Other tables read by lrcat-extractor (columns it uses exist)

- `Adobe_AdditionalMetadata`: image INTEGER, xmp, embeddedXmp INTEGER, externalXmpIsDirty INTEGER.
- `Adobe_imageProperties`: image, propertiesString (both present).
- `AgFolderContent`: id_local INTEGER, id_global, containingFolder INTEGER, content, name, owningModule.
- `AgLibraryCollection`: id_local, creationId, genealogy, imageCount, name, parent, systemOnly.
- `AgLibraryCollectionContent`: id_local, collection, content, owningModule.
- `AgLibraryCollectionImage`: id_local, collection, image, pick, positionInCollection.
- `AgLibraryTagImage` (Lightroom 2 only) does not exist.

## Relationships

- `Adobe_images.rootFile` -> `AgLibraryFile.id_local`
- `AgLibraryFile.folder` -> `AgLibraryFolder.id_local`
- `AgLibraryFolder.rootFolder` -> `AgLibraryRootFolder.id_local`
- `AgLibraryFolder.parentId` -> `AgLibraryFolder.id_local` (optional)
- `AgLibraryKeywordImage.image` -> `Adobe_images.id_local`; `.tag` -> `AgLibraryKeyword.id_local`
- `AgLibraryKeyword.parent` -> `AgLibraryKeyword.id_local`
- `Adobe_images.masterImage` -> `Adobe_images.id_local` (virtual copies)

## Comparison with lrcat-extractor 0.6.0 (commit a3a59b37)

Line numbers refer to `src/` of that source tree.

- Version detection (`catalog.rs:121-123`, `catalog.rs:128-139`): reads `Adobe_DBVersion`, keeps the first two characters, parses as integer. `1500000` becomes 15. Only 2, 3, 4 and 6 are mapped (`catalog.rs:131-137`); anything else is `CatalogVersion::Unknown`.
  `Catalog::images_for_collection` returns `UnsupportedVersion` for Unknown (`catalog.rs:298-309`).
  Reading code such as `collections.rs:31-62` is selected by version. The version check is the main blocker.
  Not confirmed by running (build was not permitted).
- Column sets (all present with these names in the LrC 15 catalog):
  - folders `folders.rs:49-51`
  - root folders `folders.rs:121-123`
  - keywords `keywords.rs:52-54`
  - files `libraryfiles.rs:51-53`
  - images `images.rs:225-233`, joining `Adobe_AdditionalMetadata` and `Adobe_imageProperties`
- Joins in `images.rs:231-233` (`meta.image = img.id_local and props.image = img.id_local`) are an inner join. Images without a metadata or properties row would be dropped silently.
- Keyword assignment (`AgLibraryKeywordImage`) is not read by lrcat-extractor. Nothing in `src/` references it.
- Collection reads (`AgLibraryCollectionContent`, `AgLibraryCollectionImage`) match the table layout.
- The catalog is opened with `Connection::open` (`catalog.rs:97-102`), which is read-write, so it must be run on a copy.

Conclusion: the schema of the tables needed for folders, files, images, keywords and ratings is compatible with the column names lrcat-extractor expects. The version mapping is what rejects LrC 15. Keyword-to-image assignments are not read at all.
