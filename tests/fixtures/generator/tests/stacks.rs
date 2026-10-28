//! Checks of the stack fixture itself.

use lrcat_fixtures::stack_edge as e;
use lrcat_fixtures::{drop_stack_tables, write_stack_catalog_with_files};
use rusqlite::{Connection, OpenFlags};

fn open(p: &std::path::Path) -> Connection {
    Connection::open_with_flags(p, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
}

fn count(c: &Connection, sql: &str) -> i64 {
    c.query_row(sql, [], |r| r.get(0)).unwrap()
}

#[test]
fn row_counts_match_the_constants() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_stack_catalog_with_files(dir.path()).unwrap();
    let c = open(&cat);
    assert_eq!(
        count(&c, "SELECT count(*) FROM AgLibraryFolderStack"),
        e::STACKS
    );
    assert_eq!(
        count(&c, "SELECT count(*) FROM AgLibraryFolderStackImage"),
        e::MEMBER_ROWS
    );
    assert_eq!(
        count(&c, "SELECT count(*) FROM AgLibraryFolderStackData"),
        e::STACKS
    );
    assert_eq!(e::CONVERTED + e::SKIPPED, e::STACKS);
    assert_eq!(e::MEMBERS_CONVERTED + e::MEMBERS_SKIPPED, e::MEMBER_ROWS);
    assert_eq!(e::STACK_MEMBERS.len() as i64, e::CONVERTED);
    let kept: usize = e::STACK_MEMBERS.iter().map(|(_, m)| m.len()).sum();
    assert_eq!(kept as i64, e::MEMBERS_CONVERTED);
}

#[test]
fn position_has_real_and_integer_forms_and_collapsed_is_set_once() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_stack_catalog_with_files(dir.path()).unwrap();
    let c = open(&cat);
    let ty = |stack: i64| -> String {
        c.query_row(
            "SELECT typeof(position) FROM AgLibraryFolderStackImage WHERE stack = ?1 LIMIT 1",
            [stack],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(ty(1), "real");
    assert_eq!(ty(2), "integer");
    assert_eq!(
        count(
            &c,
            "SELECT count(*) FROM AgLibraryFolderStack WHERE collapsed = 1"
        ),
        e::COLLAPSED
    );
}

#[test]
fn drop_stack_tables_removes_all_three() {
    let dir = tempfile::tempdir().unwrap();
    let cat = write_stack_catalog_with_files(dir.path()).unwrap();
    drop_stack_tables(&cat).unwrap();
    let c = open(&cat);
    assert_eq!(
        count(
            &c,
            "SELECT count(*) FROM sqlite_master WHERE name LIKE 'AgLibraryFolderStack%'"
        ),
        0
    );
}
