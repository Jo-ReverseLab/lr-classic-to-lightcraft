use rusqlite::Connection;

#[test]
fn removes_table_and_column() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a.lrcat");
    lrcat_fixtures::write_small_catalog_without(&p, "AgLibraryFolder", Some("pathFromRoot"))
        .unwrap();
    let c = Connection::open(&p).unwrap();
    assert!(
        c.prepare("SELECT pathFromRoot FROM AgLibraryFolder")
            .is_err()
    );
    assert!(c.prepare("SELECT rootFolder FROM AgLibraryFolder").is_ok());
    let q = dir.path().join("b.lrcat");
    lrcat_fixtures::write_small_catalog_without(&q, "AgLibraryCollection", None).unwrap();
    let c = Connection::open(&q).unwrap();
    assert!(c.prepare("SELECT 1 FROM AgLibraryCollection").is_err());
}
