use lrcat_fixtures::{small, write_large_catalog, write_small_catalog};
use rusqlite::{Connection, OpenFlags};

fn open(path: &std::path::Path) -> Connection {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
}

fn typeof_dbversion(c: &Connection) -> String {
    c.query_row(
        "SELECT typeof(value) FROM Adobe_variablesTable WHERE name = 'Adobe_DBVersion'",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn dbversion_type_can_be_changed_to_real_and_blob() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("small.lrcat");
    write_small_catalog(&path).unwrap();

    // The generator writes the version as text by default.
    let c = open(&path);
    assert_eq!(typeof_dbversion(&c), "text");
    drop(c);

    lrcat_fixtures::set_dbversion_type(&path, lrcat_fixtures::DbVersionType::Real).unwrap();
    let c = open(&path);
    assert_eq!(typeof_dbversion(&c), "real");
    // The logical value is still 1500000.
    let v: f64 = c
        .query_row(
            "SELECT value FROM Adobe_variablesTable WHERE name = 'Adobe_DBVersion'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(v, 1500000.0);
    drop(c);

    lrcat_fixtures::set_dbversion_type(&path, lrcat_fixtures::DbVersionType::Blob).unwrap();
    let c = open(&path);
    assert_eq!(typeof_dbversion(&c), "blob");
    let v: Vec<u8> = c
        .query_row(
            "SELECT value FROM Adobe_variablesTable WHERE name = 'Adobe_DBVersion'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(v, b"1500000");
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

#[test]
fn keyword_edge_catalog_has_documented_counts() {
    use lrcat_fixtures::{keyword_edge, write_keyword_edge_catalog};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("kw.lrcat");
    write_keyword_edge_catalog(&path).unwrap();
    let c = open(&path);
    assert_eq!(
        count(
            &c,
            "SELECT COUNT(*) FROM AgLibraryKeyword WHERE id_local <> 1"
        ),
        keyword_edge::KEYWORDS
    );
    assert_eq!(
        count(&c, "SELECT COUNT(*) FROM AgLibraryKeywordImage"),
        keyword_edge::ASSIGNMENTS
    );
    assert_eq!(
        count(
            &c,
            "SELECT COUNT(*) FROM AgLibraryKeyword WHERE includeOnExport = 0"
        ),
        1
    );
}

// --- Proportional performance catalogs (task T20) -------------------------

use lrcat_fixtures::{
    CatalogShape, performance, write_performance_catalog, write_proportional_catalog_with_files,
};

#[test]
fn proportional_shape_is_fixed_and_documented() {
    // One folder per 20 photos, one keyword per 10, one collection per 500.
    assert_eq!(
        CatalogShape::proportional(2_000),
        CatalogShape {
            photos: 2_000,
            folders: 100,
            keywords: 200,
            collections: 4,
        }
    );
    assert_eq!(
        CatalogShape::proportional(10_000),
        CatalogShape {
            photos: 10_000,
            folders: 500,
            keywords: 1_000,
            collections: 20,
        }
    );
    assert_eq!(
        CatalogShape::proportional(100_000),
        CatalogShape {
            photos: 100_000,
            folders: 5_000,
            keywords: 10_000,
            collections: 200,
        }
    );
    // Caps take effect above 200 000 / 500 000 photos.
    let largest = CatalogShape::proportional(500_000);
    assert_eq!(largest.photos, 500_000);
    assert_eq!(largest.folders, performance::MAX_FOLDERS);
    assert_eq!(largest.keywords, performance::MAX_KEYWORDS);
    assert_eq!(largest.collections, performance::MAX_COLLECTIONS);
    // Never zero, even for a tiny catalog.
    assert_eq!(
        CatalogShape::proportional(0),
        CatalogShape {
            photos: 1,
            folders: 1,
            keywords: 1,
            collections: 1,
        }
    );
}

#[test]
fn performance_catalog_has_exact_row_counts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("performance.lrcat");
    let shape = CatalogShape {
        photos: 250,
        folders: 12,
        keywords: 25,
        collections: 20,
    };
    write_performance_catalog(&path, shape).unwrap();
    let c = open(&path);
    assert_eq!(count(&c, "SELECT COUNT(*) FROM Adobe_images"), 250);
    assert_eq!(count(&c, "SELECT COUNT(*) FROM AgLibraryFile"), 250);
    assert_eq!(count(&c, "SELECT COUNT(*) FROM AgLibraryFolder"), 12);
    assert_eq!(
        count(&c, "SELECT COUNT(*) FROM AgLibraryRootFolder"),
        1,
        "one root folder"
    );
    assert_eq!(
        count(
            &c,
            "SELECT COUNT(*) FROM AgLibraryKeyword WHERE id_local <> 1"
        ),
        25
    );
    assert_eq!(
        count(&c, "SELECT COUNT(*) FROM AgLibraryKeywordImage"),
        250,
        "one assignment per photo"
    );
    assert_eq!(count(&c, "SELECT COUNT(*) FROM AgLibraryCollection"), 20);
    // Every tenth photo (starting at 1) is in a plain album: 25 of 250. The
    // set, the smart collection and the Quick Collection receive no members.
    assert_eq!(
        count(&c, "SELECT COUNT(*) FROM AgLibraryCollectionImage"),
        25
    );
    assert_eq!(
        count(
            &c,
            "SELECT COUNT(*) FROM AgLibraryCollectionImage ci \
             JOIN AgLibraryCollection c ON c.id_local = ci.collection \
             WHERE c.creationId = 'com.adobe.ag.library.collection'"
        ),
        25
    );
}

#[test]
fn performance_catalog_is_deterministic() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.lrcat");
    let b = dir.path().join("b.lrcat");
    let shape = CatalogShape::proportional(300);
    write_performance_catalog(&a, shape).unwrap();
    write_performance_catalog(&b, shape).unwrap();
    assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
}

#[test]
fn proportional_catalog_with_files_writes_every_original() {
    let dir = tempfile::tempdir().unwrap();
    let catalog = write_proportional_catalog_with_files(dir.path(), 60).unwrap();
    assert_eq!(catalog.file_name().unwrap(), "performance.lrcat");
    let pictures = dir.path().join(performance::PICTURES_DIR);
    let mut files = Vec::new();
    let mut todo = vec![pictures];
    while let Some(d) = todo.pop() {
        for entry in std::fs::read_dir(&d).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                todo.push(p);
            } else {
                files.push(p);
            }
        }
    }
    assert_eq!(files.len(), 60, "one placeholder per photo");
    assert!(
        files
            .iter()
            .all(|p| std::fs::metadata(p).unwrap().is_file())
    );
}
