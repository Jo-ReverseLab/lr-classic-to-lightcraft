use lrcat_fixtures::{small, write_large_catalog, write_small_catalog};
use rusqlite::{Connection, OpenFlags};

fn open(path: &std::path::Path) -> Connection {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
}

fn count(c: &Connection, sql: &str) -> i64 {
    c.query_row(sql, [], |r| r.get(0)).unwrap()
}

fn small_catalog() -> (tempfile::TempDir, Connection) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("small.lrcat");
    write_small_catalog(&path).unwrap();
    let c = open(&path);
    (dir, c)
}

#[test]
fn small_catalog_has_expected_tables_and_version() {
    let (_d, c) = small_catalog();
    for t in [
        "Adobe_variablesTable",
        "AgLibraryRootFolder",
        "AgLibraryFolder",
        "AgLibraryFile",
        "Adobe_images",
        "AgLibraryKeyword",
        "AgLibraryKeywordImage",
    ] {
        let n = count(
            &c,
            &format!("SELECT count(*) FROM sqlite_master WHERE name = '{t}'"),
        );
        assert_eq!(n, 1, "missing table {t}");
    }
    let v: String = c
        .query_row(
            "SELECT value FROM Adobe_variablesTable WHERE name = 'Adobe_DBVersion'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(v, "1500000");
}

#[test]
fn small_catalog_counts_match_constants() {
    let (_d, c) = small_catalog();
    assert_eq!(
        count(&c, "SELECT count(*) FROM AgLibraryRootFolder"),
        small::ROOT_FOLDERS
    );
    assert_eq!(
        count(&c, "SELECT count(*) FROM AgLibraryFolder"),
        small::FOLDERS
    );
    assert_eq!(
        count(&c, "SELECT count(*) FROM Adobe_images"),
        small::PHOTOS
    );
    assert_eq!(
        count(
            &c,
            "SELECT count(*) FROM Adobe_images WHERE masterImage IS NOT NULL"
        ),
        small::VIRTUAL_COPIES
    );
    // The root keyword row is not counted.
    assert_eq!(
        count(
            &c,
            "SELECT count(*) FROM AgLibraryKeyword WHERE parent IS NOT NULL"
        ),
        small::KEYWORDS
    );
    assert_eq!(
        count(&c, "SELECT count(*) FROM AgLibraryKeywordImage"),
        small::ASSIGNMENTS
    );
}

#[test]
fn small_catalog_ratings_match_constants() {
    let (_d, c) = small_catalog();
    assert_eq!(
        count(&c, "SELECT count(*) FROM Adobe_images WHERE rating IS NULL"),
        small::RATING_NULL
    );
    assert_eq!(
        count(&c, "SELECT count(*) FROM Adobe_images WHERE rating = 0"),
        small::RATING_ZERO
    );
    for (i, expected) in small::RATING_STARS.iter().enumerate() {
        let n = count(
            &c,
            &format!("SELECT count(*) FROM Adobe_images WHERE rating = {}", i + 1),
        );
        assert_eq!(n, *expected, "{} stars", i + 1);
    }
}

#[test]
fn small_catalog_has_special_cases() {
    let (_d, c) = small_catalog();
    assert_eq!(
        count(
            &c,
            "SELECT count(*) FROM AgLibraryFile WHERE baseName = 'IMG_0001'"
        ),
        2
    );
    assert!(count(&c, "SELECT max(length(pathFromRoot)) FROM AgLibraryFolder") > 260);
    assert_eq!(
        count(&c, "SELECT count(*) FROM AgLibraryKeyword WHERE name = ''"),
        1
    );
    assert_eq!(
        count(
            &c,
            "SELECT count(*) FROM AgLibraryKeyword k WHERE name = 'Unused' AND NOT EXISTS (SELECT 1 FROM AgLibraryKeywordImage WHERE tag = k.id_local)"
        ),
        1
    );
    assert_eq!(
        count(
            &c,
            "SELECT count(*) FROM AgLibraryKeyword WHERE name = 'Köln'"
        ),
        1
    );
    let roots = count(
        &c,
        "SELECT count(*) FROM AgLibraryRootFolder WHERE absolutePath LIKE 'C:\\Users\\example\\%' OR absolutePath LIKE '/Users/example/%'",
    );
    assert_eq!(roots, small::ROOT_FOLDERS);
}

#[test]
fn small_catalog_is_deterministic_and_overwritable() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.lrcat");
    let b = dir.path().join("b.lrcat");
    write_small_catalog(&a).unwrap();
    write_small_catalog(&a).unwrap();
    write_small_catalog(&b).unwrap();
    assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
}

#[test]
fn large_catalog_has_exactly_requested_images() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("large.lrcat");
    write_large_catalog(&path, 2500).unwrap();
    let c = open(&path);
    assert_eq!(count(&c, "SELECT count(*) FROM Adobe_images"), 2500);
    assert_eq!(count(&c, "SELECT count(*) FROM AgLibraryFile"), 2500);
    assert_eq!(
        count(&c, "SELECT count(*) FROM AgLibraryKeywordImage"),
        2500
    );
}

#[test]
fn small_catalog_collection_counts_match_constants() {
    let (_d, c) = small_catalog();
    let q = |w: &str| {
        count(
            &c,
            &format!("SELECT count(*) FROM AgLibraryCollection WHERE {w}"),
        )
    };
    assert_eq!(q("1=1"), small::COLLECTIONS);
    assert_eq!(q("systemOnly <> 0"), small::COLLECTIONS_SYSTEM);
    assert_eq!(
        q("systemOnly = 0 AND creationId = 'com.adobe.ag.library.collection'"),
        small::COLLECTIONS_PLAIN
    );
    assert_eq!(
        q("systemOnly = 0 AND creationId = 'com.adobe.ag.library.group'"),
        small::COLLECTION_SETS
    );
    assert_eq!(
        q("systemOnly = 0 AND creationId = 'com.adobe.ag.library.smart_collection'"),
        small::COLLECTIONS_SMART
    );
    assert_eq!(
        q("systemOnly = 0 AND creationId NOT LIKE 'com.adobe.ag.library.%'"),
        small::COLLECTIONS_UNKNOWN
    );
    assert_eq!(
        small::COLLECTIONS,
        small::COLLECTIONS_SYSTEM
            + small::COLLECTIONS_PLAIN
            + small::COLLECTION_SETS
            + small::COLLECTIONS_SMART
            + small::COLLECTIONS_UNKNOWN
    );
    // A plain collection lies inside the collection set.
    assert_eq!(
        count(
            &c,
            "SELECT count(*) FROM AgLibraryCollection c JOIN AgLibraryCollection p ON p.id_local = c.parent WHERE p.creationId = 'com.adobe.ag.library.group' AND c.creationId = 'com.adobe.ag.library.collection'"
        ),
        1
    );
    // A system collection that also carries the plain creationId.
    assert_eq!(
        q("systemOnly <> 0 AND creationId = 'com.adobe.ag.library.collection'"),
        1
    );
    assert_eq!(
        count(&c, "SELECT count(*) FROM AgLibraryCollectionImage"),
        small::COLLECTION_IMAGES
    );
}

#[test]
fn large_catalog_has_collections() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("large.lrcat");
    write_large_catalog(&path, 100).unwrap();
    let c = open(&path);
    assert_eq!(
        count(&c, "SELECT count(*) FROM AgLibraryCollection"),
        lrcat_fixtures::LARGE_COLLECTIONS
    );
    assert!(count(&c, "SELECT count(*) FROM AgLibraryCollectionImage") > 0);
}
