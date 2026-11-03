# T07 — Reader tests for edge cases

**Branch:** `glm/t07-reader-edge-tests`
**Base:** `origin/main` (7f36890)
**Status:** done (unverified — no local compiler, CI is the proof)
**Attempt:** 1 of 3

Tests only, no product code changed. (If a test below proves a defect, it is
noted; none changed behaviour.)

## 1. `Adobe_DBVersion` stored as a real number and as a blob

`crates/lr-reader/src/catalog.rs` already handles every SQLite storage type for
the version value (`Text`, `Integer`, `Real`, `Blob` → lossy UTF-8, `Null` →
missing). The generator only wrote text, so the non-text arms were untested.

Added to the generator (`tests/fixtures/generator/src/lib.rs`):

- `pub enum DbVersionType { Text, Real, Blob }`
- `pub fn set_dbversion_type(path, kind)` — rewrites the `Adobe_DBVersion` value
  with the given SQLite storage type while keeping the logical value `1500000`.

Tests:

- `crates/lr-reader/tests/open.rs`: `opens_when_dbversion_is_stored_as_real`,
  `opens_when_dbversion_is_stored_as_blob` — open succeeds and reports
  `1500000` for both variants.
- `tests/fixtures/generator/tests/generator.rs`:
  `dbversion_type_can_be_changed_to_real_and_blob` — asserts `typeof(value)`
  is `text` / `real` / `blob` and the logical value stays `1500000`, so the
  generator's own contract is pinned.

## 2. A `-shm` file beside the catalog

Today only `-wal` and `-journal` are refused (`SIDE_FILE_SUFFIXES`,
`catalog.rs`). A `-shm` (shared-memory) file is ignored. Test
`non_empty_shm_is_not_rejected` in `crates/lr-reader/tests/pending_changes.rs`
pins the current behaviour: a non-empty `<catalog>-shm` does **not** block open,
the catalog opens with version `1500000`, and nothing is written (catalog and
`-shm` stay byte-identical).

**Is this right?** Yes. A `-shm` file only exists alongside a live `-wal` in WAL
mode; a lone `-shm` is a stale leftover, and the existing `-wal` check already
catches uncommitted WAL changes. Refusing a lone `-shm` would reject harmless
stale files. Behaviour is left unchanged (a decision for the owner).

## 3. Skip codes

No reader integration test asserted the stable `Skipped::code`; they asserted
the reason text only. `crates/lr-reader/tests/skip_codes.rs` adds `.code`
assertions for every code the reader can produce from a generated catalog.

A helper collects the code set of a `Report`:

```rust
fn codes(report: &Report) -> BTreeSet<SkipCode> {
    report.skipped_items().iter().map(|s| s.code).collect()
}
```

Each test reads the relevant fixture (or a minimally mutated one) and asserts
the expected code is present.

**Codes asserted from the existing fixtures** (no mutation):
`VirtualCopy`, `Video`, `Missing`, `MissingGlobalId`, `DuplicateGlobalId`,
`FileRowNotFound`, `FolderRowNotFound`, `RootFolderNotFound`, `InvalidRating`
(photos / originals); `NameEmpty`, `NameHasBar`, `ParentSkipped`,
`AssignmentPhotoSkipped`, `AssignmentKeywordSkipped`, `KeywordSkipped`,
`KeywordUnknown` (keywords / assignments / synonyms, including the synonym
report); `CollectionKindUnsupported`, `QuickDuplicate`, `SmartUnreadable`,
`ParentMissing`, `ParentNotSet`, `ParentCycle`, `ParentSkipped`,
`CollectionSkipped`, `CollectionIsSet`, `PhotoNotConverted`, `DuplicateMember`,
`SmartNoCriteria`, `SmartUntranslatable` (collections and smart); `TooFewPhotos`,
`StackSkipped`, `PhotoNotConverted`, `AlreadyInStack` (stacks).

**Codes asserted from a minimal SQL mutation** (the base fixture does not
reach them, but a generated catalog can still store the triggering row):

| code | mutation |
|---|---|
| `CollectionKindMissing` | `creationId` stored as an integer (not text) |
| `NameInvalid` | collection `name` stored as an integer (not text) |
| `ParentInvalid` | collection `parent` set to non-id text `'abc'` |
| `CollectionUnknown` | a member row pointing at collection id `999` (no row) |
| `StackUnknown` | a stack member pointing at stack id `999` (no row) |
| `StackIdInvalid` | a stack member with `stack = NULL` |
| `IdsInvalid` (stack) | a stack member with both ids `NULL` |
| `PhotoInvalid` | a stack member of an existing stack with `image = NULL` |
| `DuplicateMember` (stack) | two identical member rows in one stack |

`AgLibraryFolderStackImage.image` and `.stack` are nullable, so the five stack
mutations are real rows. `creationId` and `name` are `NOT NULL` but accept a
non-text value, which the reader's `ValueRef`-based read sees as "no text" /
"no valid id".

**Codes not fixture-reachable** (need a `NULL` in a `NOT NULL` column; covered
only by the in-source `#[cfg(test)]` unit tests of the reader modules):
`NoRootFolder` (`AgLibraryFolder.rootFolder` is `NOT NULL`),
`RootFolderNoPath` (`AgLibraryRootFolder.absolutePath` is `NOT NULL`, and an
empty string is `Some("")` not `None`), `CollectionIdInvalid`,
`CollectionPhotoInvalid`, `IdsInvalid` for collection members
(`AgLibraryCollectionImage.collection` and `.image` are `NOT NULL`). These
cannot arise from a catalog that satisfies the schema, so no integration test
can produce them; the unit tests build the malformed row by hand.

## 4. A second, older supported version

Not applicable. The reader accepts exactly one version,
`pub const SUPPORTED_VERSION: &str = "1500000"` (`catalog.rs:11`), by exact
string match (`if version != SUPPORTED_VERSION`). It does **not** claim to
support any older version. `docs/research/lrcat-schema-lrc15.md` mentions a
version mapping (2, 3, 4, 6) but that describes lrcat-extractor's behaviour
during research, not this reader. There is no older supported version to add.

## Acceptance

- New tests use neutral file names (no `?`, no case-only differences) and run on
  Linux and Windows.
- `cargo fmt --check` passes. `cargo check` / clippy / tests need a C compiler
  (`cc`), not installed here, so they are left to CI.
