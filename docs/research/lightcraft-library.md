# LightCraft library: storage of folders, photos, keywords and ratings

All references are relative to the LightCraft source root (workspace version 0.14.0, `Cargo.toml:6`). Everything below was read in the sources; statements marked *(assumption)* were not verified in code.

## Storage format

A library is a **directory** (`crates/engine/src/library.rs:1-11`):

```text
catalog.snap  catalog.log   op journal (the catalog)
presets.json  view.json     user presets, last view state
prefs.json                  library preferences (XMP sidecars, import defaults, …)
thumbs/                     preview cache, safe to delete
Originals/                  only for "copy into library" imports
Interop/                    Lightroom import index + recovery archives (see below)
```
(`Interop/` is not in the layout comment; it is created by the Lightroom importer, `crates/engine/src/lightroom_catalog.rs:824`.)

The catalog is **not SQLite and not a table schema**. It is an append-only journal of JSON operations plus periodic snapshots (`crates/catalog/src/journal.rs:1-9`, `docs/architecture.md:29-31`):

- `catalog.snap`: `{"format":"lightcraft-catalog","version":V,"seq":N,"catalog":{…}}`, the whole state after op `N`; replaced atomically via temp + fsync + rename (`journal.rs:3-6`).
- `catalog.log`: JSON lines `{"seq":N,"crc":C,"op":{…}}`, CRC-32 of the op text, fsynced on append (`journal.rs:7-9`).
- Load = snapshot + replay of records with `seq > snapshot seq`; torn tail dropped, damaged log kept as `catalog.log.damaged-<seq>` (`journal.rs:11-27`).
- Format version `VERSION = 9` (`journal.rs:79`); history of versions at `journal.rs:46-58`. Opening an older format rewrites a snapshot in the new format; a newer format is refused untouched (`journal.rs:62-74`).
- A lock file `catalog.lock` allows one writer process (`docs/architecture.md:31`, `docs/mcp.md:22-27`; `crates/catalog/src/lock.rs`).
- Stores are abstracted (`Store` trait, `FsStore` for a directory, `MemStore`, `crates/catalog/src/store.rs:13,61,215`); browsers use OPFS/IndexedDB (`library.rs:13-14`).

Consequence: to write a library without the engine, one would have to produce a valid snapshot (or log records with correct `seq` and CRC) in the current format and respect the lock. The format is versioned but internal; no stability promise for third parties is documented *(assumption)*.

## Catalog structure

`Catalog` (`crates/catalog/src/lib.rs:265-296`) serialises these fields:

| Field | Type | Meaning |
|---|---|---|
| `photos` | `BTreeMap<PhotoId, Arc<Photo>>` | all photo records (`lib.rs:267`) |
| `albums` | `BTreeMap<AlbumId, Album>` | albums and album folders (`lib.rs:268`) |
| `next_photo`, `next_album` | `u64` | id counters (`lib.rs:269-270`); `alloc_photo_id` at `lib.rs:372` |
| `stacks`, `next_stack` | | photo stacks (`lib.rs:272-275`) |
| `label_names` | colour label → custom name | `lib.rs:277-278` |
| `browsed` | folder path → ISO time | Local browsing (`lib.rs:280-282`) |
| `keyword_list` | lower-case path → `ListedKeyword` | keywords listed on their own (`lib.rs:283-285`) |
| `folder_records` | folder key → `FolderRecord` | per-folder data (colour label only) (`lib.rs:286-288`) |

Keys: `PhotoId(u64)`, `AlbumId(u64)`, `StackId(u64)` are transparent integers (`crates/catalog/src/model.rs:10-18`), allocated sequentially. All changes are `Op` variants (`lib.rs:78-…`): `AddPhoto`, `SetRating`, `SetFlag`, `SetLabel`, `SetMeta`, `SetDevelop`, `AddAlbum`, `SetKeyword`, `SetFolderRecord`, and so on (`lib.rs:79-138`, `:249`).

## Photos (file references)

`Photo` (`model.rs:225-300`) holds, among others: `id`, `source`, `file_name`, `kind`, `format`, `width`, `height`, `file_size`, `captured` (ISO 8601 local, `model.rs:236-238`), `imported`, `meta`, `rating: u8`, `flag`, `label`, `develop`, `history`, `deleted`, `local`, `content_hash`, `copy_of`/`copy_name` (virtual copies, `model.rs:280-286`).

- **File reference**: `Source::File { path: String }` (or `Demo { scene }`) (`model.rs:86-94`). The path is a plain string as given at import; no relative form, no root-folder table. A photo is a reference to the original unless imported with `copy`/`move` (`crates/engine/src/import.rs:46-56`).
- **Content hash** `content_hash: Option<String>` (hex of a 128-bit hash of the file bytes), used for duplicate detection and preview-cache keys (`model.rs:273-275`).
- Virtual copy: a separate `Photo` with `copy_of = Some(master)` sharing the master's file (`model.rs:280-286`).
- A record can exist for a missing file (Lightroom import creates these, "catalogued for relinking", `docs/lightroom-catalog-import.md:60-62`); relinking is `photo.relink` (`crates/engine/src/cmd/missing.rs:60`).
- No field holds an external/source id (checked the field list at `model.rs:225-300`; for how provenance is done, see `lightcraft-import.md` in the same folder).

## Folders

Two different things are called "folder":

1. **Disk folders of photos: derived, not stored.** The folder tree lists folders containing at least one library photo plus their ancestors, with counts, computed from `Source::File.path` (`crates/catalog/src/folders.rs:1-9`; tree function `folder_tree`, `folders.rs:252`, node type `FolderNode`, `folders.rs:49`). A volume is a top-level row decided only by the stored path spelling (`folders.rs:11-16`). Folder identity is `folder_key(path)`: separators `/` and `\` unified, `.`/`..` resolved lexically, drive letter lower-cased, whole path lower-cased on Windows (`crates/catalog/src/query.rs:674-686`). `in_folder(path, dir, deep)` is the membership test (`query.rs:625-630`).
   The only stored per-folder data is `FolderRecord { label: Option<ColorLabel> }` keyed by `folder_key` (`folders.rs:75-80`; `lib.rs:286-288`). It follows renames/moves done inside LightCraft (`folders.rs:30-33`).
2. **Album folders (groups of albums).** `Album { id, name, parent: Option<AlbumId>, folder: bool, photos: Vec<PhotoId>, cover, smart, quick, order }` (`model.rs:516-540`). `folder = true` means a container for albums, "Folders hold albums, not photos" (`model.rs:522-524`); `parent` points at such a folder. Smart albums carry a live filter and an empty `photos` list (`model.rs:529-531`). Albums are the equivalent of Lightroom collections and collection sets; the Lightroom importer maps them that way, using `creationId` containing `group`/`smart` to tell them apart (`lightroom_catalog.rs:1062-1068`).

## Keywords with hierarchy

- Per photo: `Meta.keywords: Vec<String>` (`model.rs:196`). Hierarchy = `|`-separated levels in each string, `parent|child|grandchild` (`crates/catalog/src/keywords.rs:1-2`, `SEP` at `keywords.rs:18`). `clean()` trims each level and drops empty levels (`keywords.rs:21-23`). Keyword names compare case-insensitively; renaming a keyword renames its children (`keywords.rs:7-8`).
- Library-wide: `keyword_list` holds keywords that exist on their own or have attributes: `ListedKeyword { path, info }`, `KeywordInfo { synonyms, include_on_export, export_containing, export_synonyms, person, new_keywords_inside }` (`keywords.rs:174-195`, `:216-219`). "The tree is the keyword list together with the keywords photos carry" (`keywords.rs:10-12`). Created via `Op::SetKeyword` (`lib.rs:249`) / command `keyword.create` (`crates/engine/src/cmd/keywords.rs:356-377`).
- Therefore the hierarchy is **encoded in strings**; there are no parent ids and no keyword ids. A parent keyword need not appear on a photo for the child to exist (the tree derives ancestors from the path).
- The Lightroom importer builds the path by following `AgLibraryKeyword.parent` and writing the joined string into `meta.keywords` (`lightroom_catalog.rs:496-518`, `:970-972`). Lightroom's synonyms/include-on-export flags are *not* read there (that code reads only `name` and `parent`), *(inference from the code read)*.

## Ratings, flags, labels

- `rating: u8`, 0..5 (`model.rs:243`), set by `Op::SetRating`; command `photo.rate` validates 0..5 (`crates/engine/src/cmd/library.rs:797-812`).
- `flag: Flag` (pick / reject / none, `model.rs:51-…`), `label: Option<ColorLabel>` with five colours (`model.rs:71-80`).

## Relationships

```text
Catalog ─┬─ photos[PhotoId] ── Photo ── source: File{path}  (path → disk folder, derived)
         │                           ├─ meta.keywords: ["a|b|c", …]  (strings)
         │                           ├─ rating, flag, label
         │                           └─ copy_of → PhotoId (virtual copy)
         ├─ albums[AlbumId] ── Album ── parent → AlbumId (folder=true), photos → [PhotoId]
         ├─ keyword_list[path] ── ListedKeyword
         └─ folder_records[folder_key] ── FolderRecord{label}
```

## Interop data beside the catalog

`<library>/Interop/lightroom-index.json` maps Lightroom identities to LightCraft ids: `{photos: {identity → photoId}, collections: {"<catalog>:<id_local>" → albumId}}` (`lightroom_catalog.rs:211-215`, `:1030-1037`); recovery archives `lightroom-import-N.lca` hold source settings, history, XMP, keywords and collection definitions, capped at 32 MiB each (`docs/lightroom-catalog-import.md:76-83`, format at `:134-137`).

## Open questions

1. Is the on-disk journal format documented or promised stable for external writers? Only in-code docs were found (`journal.rs`); nothing says third parties may write it.
2. Exact JSON shape of `Photo` and `Album` in `catalog.snap` (field names after `serde` renames) was not extracted; confirm from `model.rs` serde attributes or from a library produced by a test before any direct writing.
3. Whether `Meta` contains a free field that could hold a foreign id: not verified; I only saw the descriptive fields (`model.rs:169-199`, partially read).
4. Path normalisation: LightCraft keeps the path string as given; case and separator handling differ by OS (`query.rs:674-686`). How lr2lc's path mapping interacts with duplicate detection by path is untested.
5. Does `keyword.create` / `SetKeyword` preserve keywords with no photos across `keyword.list`? Likely yes (`keywords.rs:10-12`) but not tested.
6. Product decision: should lr2lc wrap the built-in `library.importLightroom` or reimplement the mapping and write through engine commands?

## Recommendation

Do not write `catalog.snap`/`catalog.log` directly: the format is an internal, versioned, CRC-protected journal with a single-writer lock (`journal.rs:1-79`) and could change with each release (nine versions so far, `journal.rs:46-58`). The model lr2lc must target is small: photo = path + rating + `|`-joined keyword strings; folders are implicit from paths; Lightroom collections map to albums. That maps cleanly onto the neutral model's folders/photos/keywords/ratings, so the writer should translate it into engine commands (see `lightcraft-import.md`). The decision between wrapping `library.importLightroom` and building the mapping ourselves is an open product decision (see Open questions: wrap the built-in importer or reimplement the mapping).
