//! Integration tests that assert the stable `SkipCode` of every reader skip.
//!
//! The other reader tests assert skip reasons by text; none asserted the
//! machine-readable `Skipped::code`. These tests pin the code for every code
//! the reader can produce from a generated (or minimally mutated) catalog.
//!
//! A few codes need a NULL in a column the fixture schema declares `NOT NULL`
//! (`NoRootFolder`, `RootFolderNoPath`, `CollectionIdInvalid`,
//! `CollectionPhotoInvalid`, `IdsInvalid` for collection members). Such a row
//! cannot be stored in the generated catalog, so those codes are covered only
//! by the in-source `#[cfg(test)]` unit tests of the reader modules and are
//! named below as "not fixture-reachable".

use lr_reader::{Catalog, drop_missing_originals};
use lrcat_fixtures::{
    set_keyword_global_id, set_photo_global_id, write_collection_catalog,
    write_keyword_edge_catalog, write_small_catalog, write_small_catalog_with_files,
    write_smart_catalog, write_stack_catalog_with_files, write_video_catalog_with_files,
};
use model::{Report, SkipCode};
use rusqlite::Connection;
use std::collections::BTreeSet;
use std::path::Path;

/// Every code recorded in `report`.
fn codes(report: &Report) -> BTreeSet<SkipCode> {
    report.skipped_items().iter().map(|s| s.code).collect()
}

/// Asserts `report` contains `code`.
fn has(report: &Report, code: SkipCode) {
    assert!(
        codes(report).contains(&code),
        "expected skip code {code:?}, got {:?}",
        codes(report)
    );
}

fn exec(path: &Path, sql: &str) {
    Connection::open(path).unwrap().execute_batch(sql).unwrap();
}

// ---- photos and originals ----

#[test]
fn video_and_virtual_copy_codes() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_video_catalog_with_files(dir.path()).unwrap();
    let catalog = Catalog::open(&path).unwrap();
    let mut report = Report::new();
    catalog.photos(&mut report).unwrap();
    has(&report, SkipCode::Video);
    // The virtual copy of a video keeps the virtual-copy code (T03).
    has(&report, SkipCode::VirtualCopy);
}

#[test]
fn missing_original_code() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("small.lrcat");
    write_small_catalog_with_files(dir.path()).unwrap();
    let catalog = Catalog::open(&path).unwrap();
    let photos = catalog.photos(&mut Report::new()).unwrap();
    let mut report = Report::new();
    let _kept = drop_missing_originals(photos, &mut report);
    has(&report, SkipCode::Missing);
}

#[test]
fn photo_row_defect_codes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("small.lrcat");
    write_small_catalog_with_files(dir.path()).unwrap();
    // Photo 5: its AgLibraryFile row is gone.
    exec(&path, "DELETE FROM AgLibraryFile WHERE id_local = 5;");
    // Photo 7: its file points at a non-existent folder.
    exec(
        &path,
        "UPDATE AgLibraryFile SET folder = 999 WHERE id_local = 7;",
    );
    // Folder 6 (root 2): root folder row missing.
    exec(
        &path,
        "UPDATE AgLibraryFolder SET rootFolder = 999 WHERE id_local = 6;",
    );
    // Photos 8 and 9: rating out of range.
    exec(
        &path,
        "UPDATE Adobe_images SET rating = 7 WHERE id_local = 8;",
    );
    exec(
        &path,
        "UPDATE Adobe_images SET rating = -1 WHERE id_local = 9;",
    );

    let catalog = Catalog::open(&path).unwrap();
    let mut report = Report::new();
    catalog.photos(&mut report).unwrap();
    has(&report, SkipCode::FileRowNotFound);
    has(&report, SkipCode::FolderRowNotFound);
    has(&report, SkipCode::RootFolderNotFound);
    has(&report, SkipCode::InvalidRating);
}

#[test]
fn photo_identity_codes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.lrcat");
    write_small_catalog(&path).unwrap();
    set_photo_global_id(&path, 4, "").unwrap();
    set_photo_global_id(&path, 5, "   ").unwrap();
    let catalog = Catalog::open(&path).unwrap();
    let mut report = Report::new();
    catalog.photos(&mut report).unwrap();
    has(&report, SkipCode::MissingGlobalId);

    let path = dir.path().join("b.lrcat");
    write_small_catalog(&path).unwrap();
    set_photo_global_id(&path, 4, "zyx-dup").unwrap();
    set_photo_global_id(&path, 9, "zyx-dup").unwrap();
    let catalog = Catalog::open(&path).unwrap();
    let mut report = Report::new();
    catalog.photos(&mut report).unwrap();
    has(&report, SkipCode::DuplicateGlobalId);
}

// ---- keywords ----

struct KeywordRead {
    keyword_report: Report,
    assignment_report: Report,
    data: lr_reader::KeywordData,
}

fn read_keywords(path: &Path) -> KeywordRead {
    let catalog = Catalog::open(path).unwrap();
    let mut photo_report = Report::new();
    let photos = catalog.photos(&mut photo_report).expect("photos");
    let mut keyword_report = Report::new();
    let mut assignment_report = Report::new();
    let data = catalog
        .keywords(&photos, &mut keyword_report, &mut assignment_report)
        .expect("keywords");
    KeywordRead {
        keyword_report,
        assignment_report,
        data,
    }
}

#[test]
fn keyword_and_assignment_codes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("kw.lrcat");
    write_keyword_edge_catalog(&path).unwrap();
    let r = read_keywords(&path);
    has(&r.keyword_report, SkipCode::NameEmpty);
    has(&r.keyword_report, SkipCode::NameHasBar);
    has(&r.keyword_report, SkipCode::ParentSkipped);
    has(&r.assignment_report, SkipCode::AssignmentPhotoSkipped);
    has(&r.assignment_report, SkipCode::AssignmentKeywordSkipped);
}

#[test]
fn keyword_synonym_codes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("kw.lrcat");
    write_keyword_edge_catalog(&path).unwrap();
    let r = read_keywords(&path);
    has(&r.data.synonym_report, SkipCode::NameEmpty);
    has(&r.data.synonym_report, SkipCode::KeywordSkipped);
    has(&r.data.synonym_report, SkipCode::KeywordUnknown);
}

#[test]
fn keyword_identity_codes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.lrcat");
    write_small_catalog(&path).unwrap();
    set_keyword_global_id(&path, 9, "").unwrap();
    let r = read_keywords(&path);
    has(&r.keyword_report, SkipCode::MissingGlobalId);

    let path = dir.path().join("b.lrcat");
    write_small_catalog(&path).unwrap();
    set_keyword_global_id(&path, 9, "zyx-dup").unwrap();
    set_keyword_global_id(&path, 10, "zyx-dup").unwrap();
    let r = read_keywords(&path);
    has(&r.keyword_report, SkipCode::DuplicateGlobalId);
}

// ---- collections ----

#[test]
fn collection_codes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("coll.lrcat");
    write_collection_catalog(&path).unwrap();
    let catalog = Catalog::open(&path).unwrap();
    let photos = catalog.photos(&mut Report::new()).unwrap();
    let data = catalog.collection_data(&photos).unwrap();
    has(&data.report, SkipCode::CollectionKindUnsupported);
    has(&data.report, SkipCode::QuickDuplicate);
    has(&data.report, SkipCode::SmartUnreadable);
    has(&data.report, SkipCode::ParentMissing);
    has(&data.report, SkipCode::ParentNotSet);
    has(&data.report, SkipCode::ParentCycle);
    has(&data.report, SkipCode::ParentSkipped);
}

#[test]
fn collection_member_codes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("coll.lrcat");
    write_collection_catalog(&path).unwrap();
    let catalog = Catalog::open(&path).unwrap();
    let photos = catalog.photos(&mut Report::new()).unwrap();
    let data = catalog.collection_data(&photos).unwrap();
    has(&data.members, SkipCode::CollectionSkipped);
    has(&data.members, SkipCode::CollectionIsSet);
    has(&data.members, SkipCode::PhotoNotConverted);
    has(&data.members, SkipCode::DuplicateMember);
}

// Mutated collection rows: codes the base fixture does not reach.

#[test]
fn collection_kind_missing_when_creation_id_is_not_text() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("coll.lrcat");
    write_collection_catalog(&path).unwrap();
    // creationId is NOT NULL but stored as an integer here, so the reader's
    // text view is None.
    exec(
        &path,
        "UPDATE AgLibraryCollection SET creationId = 5 WHERE id_local = 2;",
    );
    let catalog = Catalog::open(&path).unwrap();
    let photos = catalog.photos(&mut Report::new()).unwrap();
    let data = catalog.collection_data(&photos).unwrap();
    has(&data.report, SkipCode::CollectionKindMissing);
}

#[test]
fn collection_name_invalid_when_name_is_not_text() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("coll.lrcat");
    write_collection_catalog(&path).unwrap();
    exec(
        &path,
        "UPDATE AgLibraryCollection SET name = 5 WHERE id_local = 3;",
    );
    let catalog = Catalog::open(&path).unwrap();
    let photos = catalog.photos(&mut Report::new()).unwrap();
    let data = catalog.collection_data(&photos).unwrap();
    has(&data.report, SkipCode::NameInvalid);
}

#[test]
fn collection_parent_invalid_when_parent_is_not_an_id() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("coll.lrcat");
    write_collection_catalog(&path).unwrap();
    exec(
        &path,
        "UPDATE AgLibraryCollection SET parent = 'abc' WHERE id_local = 4;",
    );
    let catalog = Catalog::open(&path).unwrap();
    let photos = catalog.photos(&mut Report::new()).unwrap();
    let data = catalog.collection_data(&photos).unwrap();
    has(&data.report, SkipCode::ParentInvalid);
}

#[test]
fn collection_member_points_at_unknown_collection() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("coll.lrcat");
    write_collection_catalog(&path).unwrap();
    // collection 999 has no AgLibraryCollection row.
    exec(
        &path,
        "INSERT INTO AgLibraryCollectionImage (collection, image, pick, positionInCollection) \
         VALUES (999, 1, 0, 0);",
    );
    let catalog = Catalog::open(&path).unwrap();
    let photos = catalog.photos(&mut Report::new()).unwrap();
    let data = catalog.collection_data(&photos).unwrap();
    has(&data.members, SkipCode::CollectionUnknown);
}

// ---- smart collections ----

#[test]
fn smart_codes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("smart.lrcat");
    write_smart_catalog(&path).unwrap();
    let catalog = Catalog::open(&path).unwrap();
    let photos = catalog.photos(&mut Report::new()).unwrap();
    let data = catalog.collection_data(&photos).unwrap();
    has(&data.report, SkipCode::SmartUnreadable);
    has(&data.report, SkipCode::SmartNoCriteria);
    has(&data.report, SkipCode::SmartUntranslatable);
}

// ---- stacks ----

#[test]
fn stack_codes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stacks.lrcat");
    write_stack_catalog_with_files(dir.path()).unwrap();
    let catalog = Catalog::open(&path).unwrap();
    let photos = catalog.photos(&mut Report::new()).unwrap();
    let data = catalog.stack_data(&photos).unwrap();
    has(&data.report, SkipCode::TooFewPhotos);
    has(&data.members, SkipCode::StackSkipped);
    has(&data.members, SkipCode::PhotoNotConverted);
    has(&data.members, SkipCode::AlreadyInStack);
}

// Mutated stack member rows: codes the base fixture does not reach.
// AgLibraryFolderStackImage.image and .stack are nullable, so all five are
// fixture-reachable through a minimal INSERT.

#[test]
fn stack_member_unknown_stack() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stacks.lrcat");
    write_stack_catalog_with_files(dir.path()).unwrap();
    exec(
        &path,
        "INSERT INTO AgLibraryFolderStackImage (image, stack) VALUES (1, 999);",
    );
    let catalog = Catalog::open(&path).unwrap();
    let photos = catalog.photos(&mut Report::new()).unwrap();
    let data = catalog.stack_data(&photos).unwrap();
    has(&data.members, SkipCode::StackUnknown);
}

#[test]
fn stack_member_invalid_stack_id() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stacks.lrcat");
    write_stack_catalog_with_files(dir.path()).unwrap();
    exec(
        &path,
        "INSERT INTO AgLibraryFolderStackImage (image, stack) VALUES (1, NULL);",
    );
    let catalog = Catalog::open(&path).unwrap();
    let photos = catalog.photos(&mut Report::new()).unwrap();
    let data = catalog.stack_data(&photos).unwrap();
    has(&data.members, SkipCode::StackIdInvalid);
}

#[test]
fn stack_member_both_ids_invalid() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stacks.lrcat");
    write_stack_catalog_with_files(dir.path()).unwrap();
    exec(
        &path,
        "INSERT INTO AgLibraryFolderStackImage (image, stack) VALUES (NULL, NULL);",
    );
    let catalog = Catalog::open(&path).unwrap();
    let photos = catalog.photos(&mut Report::new()).unwrap();
    let data = catalog.stack_data(&photos).unwrap();
    has(&data.members, SkipCode::IdsInvalid);
}

#[test]
fn stack_member_invalid_photo_id() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stacks.lrcat");
    write_stack_catalog_with_files(dir.path()).unwrap();
    // stack 1 exists; its member has no photo id.
    exec(
        &path,
        "INSERT INTO AgLibraryFolderStackImage (image, stack) VALUES (NULL, 1);",
    );
    let catalog = Catalog::open(&path).unwrap();
    let photos = catalog.photos(&mut Report::new()).unwrap();
    let data = catalog.stack_data(&photos).unwrap();
    has(&data.members, SkipCode::PhotoInvalid);
}

#[test]
fn stack_member_duplicate_within_stack() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stacks.lrcat");
    write_stack_catalog_with_files(dir.path()).unwrap();
    // Photo 1 is converted; stack 1 is processed before stack 8 (which also
    // holds photo 1). Two identical rows in stack 1 -> the second is a
    // duplicate member of that stack.
    exec(
        &path,
        "INSERT INTO AgLibraryFolderStackImage (image, stack) VALUES (1, 1);",
    );
    exec(
        &path,
        "INSERT INTO AgLibraryFolderStackImage (image, stack) VALUES (1, 1);",
    );
    let catalog = Catalog::open(&path).unwrap();
    let photos = catalog.photos(&mut Report::new()).unwrap();
    let data = catalog.stack_data(&photos).unwrap();
    has(&data.members, SkipCode::DuplicateMember);
}

// Not fixture-reachable (need a NULL in a NOT NULL column); covered by the
// in-source unit tests of the reader modules:
//   NoRootFolder, RootFolderNoPath, CollectionIdInvalid,
//   CollectionPhotoInvalid, IdsInvalid (collection member).
