# How photos get into a LightCraft library

Scope: import paths, keywords/ratings, duplicate detection, provenance, effects on originals.
All references are relative to the LightCraft source root (version 0.14.0, `Cargo.toml:6`, license `Cargo.toml:8`) and were read there.

## Headline finding

LightCraft already ships a native Lightroom Classic catalog importer (`library.importLightroom`, `crates/engine/src/cmd/library.rs:1605-1616`, docs `docs/lightroom-catalog-import.md:1-5`). It maps ratings, flags, labels, hierarchical keywords, collections, virtual copies and develop settings, is idempotent, and references originals in place (`docs/lightroom-catalog-import.md:59-74`). lr2lc therefore overlaps with a built-in feature. Its differentiator can only be what LightCraft's importer does not do well (see Open questions); this affects project scope, not just the writer design.

## Import paths

There are four ways in. All go through the same command registry in `lightcraft-engine`.

1. **Engine command `library.import {paths, mode, album, albumName, keywords, ...}`** (`crates/engine/src/cmd/library.rs:1634-1670`). Takes files or folders (recursive); returns `{imported: [photoId…], duplicates, restored, failed, warnings, album}`. Parameters are parsed in `import_params` (`library.rs:51-100`). `mode` is `add|copy|move` (`library.rs:52-58`; semantics `crates/engine/src/import.rs:47-56`).
2. **Engine command `library.importLightroom {path, updateExisting?}`** (`library.rs:1605-1616`), plus the read-only `library.inspectLightroom {path}` (`library.rs:1597-1604`). Implementation: `crates/engine/src/lightroom_catalog.rs` (`import` at :810, `apply_prepared` at :863).
3. **CLI** `lightcraft-cli run --library DIR [--import PATH] <command> key=value…` or `--script FILE` with one JSON `{"command","params"}` per line (`apps/lightcraft-cli/src/main.rs:520-580`; usage shown in `docs/lightroom-catalog-import.md:47-50`). Output is one JSON line per command (`main.rs:620`). `--library` opens or creates a persistent library headless (`main.rs:586-590`; `docs/mcp.md:20-25`). `lightcraft-cli commands --json` lists every command id (`main.rs:937`).
4. **MCP server** `lightcraft-cli mcp [--library DIR]` exposing the same commands over stdio JSON-RPC (`docs/mcp.md:1-26`), or `--connect` to a running desktop app (`docs/control-protocol.md`).

Rust embedding is also possible: `lightcraft-engine` is a workspace crate (`crates/engine/Cargo.toml:2`) with `Session::execute(id, &json)` (e.g. `crates/engine/src/files.rs:2357`). Whether it is published on crates.io is not shown in the sources I read (Open question 5).

Concurrency: a library is open in one process at a time (`catalog.lock`); while the desktop app holds it, `--library` fails (`docs/mcp.md:22-27`).

Source files are only supported media types (`EXTENSIONS`, `import.rs:37-44`); non-media catalog originals are refused (`lightroom_catalog.rs:654`, call at :866).

## Keywords and ratings

- **Rating 0–5**: command `photo.rate {rating, ids}` — whole number 0..5, else error (`library.rs:797-812`); the op is `Op::SetRating` (`:810`). `ids` is optional but without it the command acts on the selection and may advance it (`library.rs:803`); lr2lc must always pass `ids`.
- **Keywords with hierarchy**: stored as `Meta.keywords: Vec<String>` per photo (`crates/catalog/src/model.rs:196`). Hierarchy is encoded in the string: `a|b|c` is the path `a > b > c` (`crates/engine/src/cmd/keywords.rs:313` "`a|b|c` keywords are hierarchical"; rename/merge split on `|`, `keywords.rs:394-399`). Per-photo strings are the source of truth, but the library also has an optional standalone keyword list (`Catalog.keyword_list`, `Op::SetKeyword`, command `keyword.create`, `crates/engine/src/cmd/keywords.rs:356`; `crates/catalog/src/keywords.rs:10-12`), so a keyword can exist without photos; the tree is derived from both (`keyword.list`, `keywords.rs:313`).
- Set via `photo.setMeta {ids, keywords | addKeywords | removeKeywords, …}` (`library.rs:1033-1037`, `addKeywords` handling :1106-1118), or at import time with `library.import {keywords: […]}` — but that applies the same list to every file in the call (`import.rs:96-97`), so per-photo keywords need a follow-up `photo.setMeta`.
- The Lightroom importer itself builds the `|`-joined path by walking `AgLibraryKeyword.parent` up (depth cap 64, cycle check) and stores it in `meta.keywords` (`lightroom_catalog.rs:496-518`, :970-972; tested `:1245`). Rating comes from `AgLibraryImage…rating` clamped 0–5 (`:960`). Also imported: pick/reject flag (`:961-965`), color label (`:966`), capture time (`:967`).
- Folder assignment: LightCraft has **no stored folder entity for photos**: the folder tree is derived from the photo paths (`crates/catalog/src/folders.rs:1-9`), only a colour label per folder is stored (`FolderRecord`, `folders.rs:75-80`). A "folder" view is a path-prefix filter on `Source::File{path}` (`crates/catalog/src/query.rs:625-630`; `Source` at `model.rs:89-94`). Albums are separate (`model.rs:516-540`): an `Album` has `parent`, a `folder: bool` flag (grouping of albums, "Folders hold albums, not photos") and an ordered `photos` list. The Lightroom importer maps Lightroom collections to albums and collection sets to album folders (`lightroom_catalog.rs:1052-1075`, `album.folder` at :1065). Via commands: `album.create` (`library.rs:1127`), `album.addPhotos` (used at `library.rs:129`), and `library.import {album | albumName}`. Consequence: the "folder" of a Lightroom photo is preserved only as its file path; recreating the Lightroom folder tree as albums is an lr2lc design choice, not something LightCraft does.

## Duplicate detection

Two layers (`import.rs:1-24`):
1. **By path**: a file whose path is already a catalog record is skipped, reported as `reason: "path"` (`import.rs:638`, `:1048`). Lightroom importer path keys are `\`→`/` and lower-cased on Windows only (`lightroom_catalog.rs:36-39`).
2. **By content**: the probe computes a 128-bit hash of the file bytes (`Photo.content_hash`, "Hash of the file's bytes (hex), for duplicate detection", `model.rs:273-275`; `lightcraft_preview::Hash128`, `import.rs:601`); a file whose hash matches a library photo or an earlier file in the batch is a duplicate (`import.rs:648-663`, :748-776, :918-927, :1108-1110). Reported in `duplicates: [{path, existing, reason}]` (`library.rs:1634` doc string).
- A duplicate of a trashed photo is restored by default (`OnDeleted::Restore`, `import.rs:58-86`).
- `library.importPreview {paths}` runs the same detection without adding anything (`library.rs:1617-1632`; `import.rs:22-24`).
- The Lightroom importer deliberately keeps distinct originals with identical bytes as separate records (they may have independent edits): it clones the existing record under a new id (`lightroom_catalog.rs:918-925`). Hence in that path content-hash is *not* used to merge.

## Provenance and idempotency

- `Photo` has **no field for an external id** (fields `model.rs:225-300`; `Source` is only `File{path}` or `Demo{scene}`, `model.rs:89-94`). A Lightroom id cannot be stored on a photo through `library.import`/`photo.setMeta` as far as I found (searched the command list for `library.import`, `photo.setMeta`; `Meta` fields at `model.rs:166-199` are descriptive metadata only — I did not enumerate every field, so a free-text field such as caption could be abused, but that would pollute user data).
- The native Lightroom importer keeps identity **outside the photo**, in `<library>/Interop/lightroom-index.json` (`lightroom_catalog.rs:824-825`; `lightroom_job.rs:263`, :312), a JSON map `ImportIndex { photos: {identity → photoId}, collections: {…} }` (`lightroom_catalog.rs:211-215`). Identity is Lightroom's `id_global` UUID (`:602`, :248-250), falling back to `<catalog path>:<id_local>`. On re-run, a photo matches if its index entry exists **and** the stored record still has the same path, else by path for non-copies (`:899-904`). Reimport reports zero additions when all records exist (`docs/lightroom-catalog-import.md:70-71`; test `lightroom_catalog.rs:1525`, :1794). Collections map the same way, `<catalog>:<id_local>` (`:1030-1037`).
- Existing edits are kept unless `updateExisting=true` (`:944`, :956-957; docs :71-72). On reimport, user album names/hierarchy are preserved, only new membership is merged (`:1076-1090`).
- For lr2lc via the engine/CLI: idempotency can rely on (a) path + content-hash duplicate detection of `library.import` (works without ids, as long as files stay at the same path/bytes), plus (b) lr2lc's own index file `lightroom id → LightCraft photoId` kept *outside* the library (e.g. next to the report). Writing into LightCraft's `Interop/lightroom-index.json` is possible in principle but is an internal format with no stability promise (**assumption**: no compatibility guarantee is documented).

## Effects on original files

- `mode=add` (default): the photo points at the file; nothing is copied or moved (`import.rs:47-49`; `docs/lightroom-catalog-import.md:59`).
- `copy`/`move` write into the library's `Originals/YYYY/YYYY-MM-DD/`; `move` removes sources after verified copy (`import.rs:11-14`, :51-55). lr2lc must never pass these (project principle 2). The default is safe.
- **XMP sidecars**: import *reads* a photo's `.xmp` sidecar (or embedded XMP); the sidecar wins over catalog metadata (`import.rs:15-17`; `docs/xmp-interop.md:13`). So re-import into an existing directory can alter ratings/keywords in LightCraft from sidecars, not the other way round.
- LightCraft writes sidecars only when `auto_write` is on (`crates/engine/src/sidecar.rs:57-61`, `Default` derive → `false`; applied at `crates/engine/src/lib.rs:603-606`) or on explicit `photo.saveMetadataToFile` (`docs/xmp-interop.md:12`). Default off, so a plain run writes nothing beside the originals. lr2lc must not enable `library.xmpPreferences {autoWrite: true}` (`cmd/xmp.rs:103`) and the library preference must be verified (Open question 3), since it is persisted per library.
- The Lightroom importer opens the `.lrcat` read-only, rejects a catalog that changes during the read, and never overwrites source catalog or originals (`docs/lightroom-catalog-import.md:3-5`, :83-84, :91-92). Missing originals remain catalogued for relinking (`:60-63`).
- Originals are opened read-only for probing (hash, dimensions). I did not audit every file operation in `import.rs`; "no writes in add mode" follows from the documented behavior, not from an exhaustive code audit.

## Recommendation

**Use the Engine/CLI route (`lightcraft-cli run --library … --script …`, JSON lines), not direct library access.**

Reasons:
1. The on-disk library is an append-only journal of operations plus snapshots, with a lock file and versioned format (`docs/architecture.md:29-31`; `crates/catalog/src/journal.rs` version table). Writing it directly would mean reimplementing that format and tracking its migrations; the engine/CLI gives crash-safe, undoable, format-version-aware writes. This also matches principle 8 of `CLAUDE.md`.
2. Every needed operation exists as a stable, documented command: `library.import` (mode `add`), `photo.rate`, `photo.setMeta` (keywords with `|`), `album.create`/`album.addPhotos`, `library.importPreview` for a dry run. Output is JSON per line, easy to turn into `report.json`.
3. Duplicate detection (path + content hash) comes for free.
Gaps that lr2lc must fill itself: per-photo provenance (keep a `lightroom id → photoId` map beside the library; no field exists), missing originals (`library.import` needs files that exist; only `library.importLightroom` creates offline records), and folder-tree-as-albums.
Process cost: spawning `lightcraft-cli` needs the binary on the user's machine and the app closed (single-writer lock). Batching through `--script` avoids per-photo process starts.

## Open questions

1. **Scope vs. built-in importer** (user decision): `library.importLightroom` already does folders-as-paths, ratings, keywords, collections, virtual copies and develop settings. Should lr2lc call it (thin wrapper with dry-run/report/checksum) or reimplement the mapping and feed `library.import`/`photo.*`?
2. How does the native importer handle keyword synonyms, `includeOnExport`, and keywords with no photos? Not examined (it reads only `AgLibraryKeyword` name/parent, `lightroom_catalog.rs:496-518`).
3. Is the XMP auto-write preference ever on by default in a new library or in the desktop app? Only the struct default (`false`) was read, not the app's library defaults.
4. Can a photo whose original is missing be created through the public commands other than `library.importLightroom`? No such command found; `Op::AddPhoto` is an internal op.
5. Is `lightcraft-engine` published/embeddable as a crate, and is the command set stable across versions (0.x)? Not stated in the sources read. No new crates may be added without approval anyway.
6. Does `library.import` with `keywords` apply them to duplicates already in the library (second run)? Not verified; matters for idempotency.
7. Folder semantics: should Lightroom folders become albums (folder-flag albums) or stay implicit as path prefixes? Product decision.
