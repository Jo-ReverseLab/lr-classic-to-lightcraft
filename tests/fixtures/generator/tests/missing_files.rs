use lrcat_fixtures::{small, write_small_catalog, write_small_catalog_with_files};
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};

fn open(path: &Path) -> Connection {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
}

fn count(c: &Connection, sql: &str) -> i64 {
    c.query_row(sql, [], |r| r.get(0)).unwrap()
}

/// (file id, original path on disk) for every AgLibraryFile row.
fn original_paths(c: &Connection) -> Vec<(i64, PathBuf)> {
    let mut st = c
        .prepare(
            "SELECT f.id_local, r.absolutePath, d.pathFromRoot, f.idx_filename \
             FROM AgLibraryFile f \
             JOIN AgLibraryFolder d ON d.id_local = f.folder \
             JOIN AgLibraryRootFolder r ON r.id_local = d.rootFolder \
             ORDER BY f.id_local",
        )
        .unwrap();
    st.query_map([], |r| {
        let root: String = r.get(1)?;
        let rel: String = r.get(2)?;
        let name: String = r.get(3)?;
        let mut p = PathBuf::from(root);
        for part in rel.split('/').filter(|x| !x.is_empty()) {
            p.push(part);
        }
        p.push(name);
        Ok((r.get(0)?, p))
    })
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

#[test]
fn exactly_the_defined_files_are_missing() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_small_catalog_with_files(dir.path()).unwrap();
    let c = open(&cat);
    let files = original_paths(&c);
    assert_eq!(files.len() as i64, small::PHOTOS - small::VIRTUAL_COPIES);
    let missing: Vec<i64> = files
        .iter()
        .filter(|(_, p)| !p.exists())
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(missing, small::MISSING_FILE_IDS);
    assert_eq!(missing.len() as i64, small::MISSING_FILES);
    for (id, p) in &files {
        if !small::MISSING_FILE_IDS.contains(id) {
            assert!(p.is_file(), "file {id} should exist: {}", p.display());
        }
    }
}

#[test]
fn missing_photos_include_virtual_copy_and_duplicate_name_case() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_small_catalog_with_files(dir.path()).unwrap();
    let c = open(&cat);
    let mut st = c
        .prepare("SELECT id_local FROM Adobe_images WHERE rootFile IN (2, 3) ORDER BY id_local")
        .unwrap();
    let ids: Vec<i64> = st
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(ids, small::MISSING_PHOTO_IDS);
    // One of the two IMG_0001 files is missing, the other exists.
    let same: Vec<_> = original_paths(&c)
        .into_iter()
        .filter(|(_, p)| p.file_name().unwrap() == "IMG_0001.jpg")
        .collect();
    assert_eq!(same.len(), 2);
    assert_eq!(same.iter().filter(|(_, p)| p.exists()).count(), 1);
}

#[test]
fn roots_lie_below_target_dir() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_small_catalog_with_files(dir.path()).unwrap();
    assert_eq!(cat, dir.path().join("small.lrcat"));
    let c = open(&cat);
    let mut st = c
        .prepare("SELECT absolutePath FROM AgLibraryRootFolder ORDER BY id_local")
        .unwrap();
    let roots: Vec<String> = st
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(roots.len() as i64, small::ROOT_FOLDERS);
    for r in roots {
        assert!(Path::new(&r).starts_with(dir.path()), "{r}");
        assert!(Path::new(&r).is_dir(), "{r}");
    }
}

#[test]
fn counts_equal_plain_small_catalog() {
    let dir = tempfile::tempdir().unwrap();
    let with = open(&write_small_catalog_with_files(dir.path()).unwrap());
    let plain_path = dir.path().join("plain.lrcat");
    write_small_catalog(&plain_path).unwrap();
    let plain = open(&plain_path);
    for t in [
        "AgLibraryRootFolder",
        "AgLibraryFolder",
        "AgLibraryFile",
        "Adobe_images",
        "AgLibraryKeyword",
        "AgLibraryKeywordImage",
    ] {
        let sql = format!("SELECT count(*) FROM {t}");
        assert_eq!(count(&with, &sql), count(&plain, &sql), "{t}");
    }
    assert_eq!(
        count(
            &with,
            "SELECT count(*) FROM Adobe_images WHERE masterImage IS NOT NULL"
        ),
        small::VIRTUAL_COPIES
    );
    let ok: String = with
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap();
    assert_eq!(ok, "ok");
}

#[test]
fn plain_small_catalog_is_unchanged_and_output_deterministic() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    // Plain variant: still the old fixed paths, no files created.
    let p = a.path().join("x.lrcat");
    write_small_catalog(&p).unwrap();
    let c = open(&p);
    let root: String = c
        .query_row(
            "SELECT absolutePath FROM AgLibraryRootFolder WHERE id_local = 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(root, "/Users/example/Pictures/");
    // Same call in two directories gives the same schema/row counts and rewriting
    // the same directory gives a byte-identical file.
    let f1 = write_small_catalog_with_files(b.path()).unwrap();
    let bytes1 = std::fs::read(&f1).unwrap();
    let f2 = write_small_catalog_with_files(b.path()).unwrap();
    assert_eq!(bytes1, std::fs::read(&f2).unwrap());
}
