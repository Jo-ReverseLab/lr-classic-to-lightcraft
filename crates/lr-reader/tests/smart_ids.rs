//! Acceptance test for the id list of ALL smart collections.
//!
//! API: `CollectionData` gets one more field
//!
//! ```ignore
//! /// Ids of every smart collection row (creationId smart_collection),
//! /// whether its rules are readable, empty or not, or whether the name is
//! /// valid. Independent of `smart` and `smart_collections`.
//! pub smart_ids: BTreeSet<i64>,
//! ```
//!
//! Rules:
//! - `smart_ids` = ids of all smart collections of the catalog: the readable
//!   ones, the ones with unreadable rules (broken text, NULL, missing row,
//!   missing table) and the ones with only another module's content row;
//! - never a plain album or set;
//! - `smart` and `smart_collections` are unchanged (readable rules only), so
//!   `smart.keys() ⊆ smart_ids`;
//! - reading leaves the catalog unchanged.

use lr_reader::{Catalog, CollectionData};
use lrcat_fixtures::{RuleDamage, smart_edge as e};
use model::Report;
use std::collections::BTreeSet;
use std::path::Path;

fn read(path: &Path) -> CollectionData {
    let catalog = Catalog::open(path).unwrap();
    let photos = catalog.photos(&mut Report::new()).unwrap();
    catalog.collection_data(&photos).expect("read collections")
}

/// Smart collections of the smart fixture: the edge cases plus ids 5 and 6
/// of the small fixture (smart, without a content row).
fn all_smart() -> BTreeSet<i64> {
    e::READABLE
        .iter()
        .map(|(id, _)| *id)
        .chain(e::UNREADABLE.iter().map(|(id, _)| *id))
        .chain([e::OTHER_MODULE_ONLY, 5, 6])
        .collect()
}

#[test]
fn ids_cover_readable_and_unreadable_smart_collections() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("smart.lrcat");
    lrcat_fixtures::write_smart_catalog(&path).unwrap();
    let before = std::fs::read(&path).unwrap();
    let d = read(&path);
    assert_eq!(d.smart_ids, all_smart());
    assert!(!d.smart_ids.contains(&e::PLAIN_WITH_RULES), "plain album");
    for id in d.smart.keys() {
        assert!(d.smart_ids.contains(id), "{id}");
    }
    assert_eq!(d.smart.len() as i64, e::READABLE_COUNT, "unchanged");
    assert_eq!(before, std::fs::read(&path).unwrap());
}

#[test]
fn a_collection_whose_rules_break_stays_in_the_id_list() {
    for damage in [
        RuleDamage::Truncated,
        RuleDamage::RowMissing,
        RuleDamage::NullContent,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("smart.lrcat");
        lrcat_fixtures::write_smart_catalog(&path).unwrap();
        lrcat_fixtures::break_smart_rules(&path, 101, damage).unwrap();
        let d = read(&path);
        assert!(!d.smart.contains_key(&101), "{damage:?}");
        assert!(d.smart_ids.contains(&101), "{damage:?}");
        assert_eq!(d.smart_ids, all_smart(), "{damage:?}");
    }
}

#[test]
fn without_a_content_table_all_smart_ids_are_still_listed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("smart.lrcat");
    lrcat_fixtures::write_smart_catalog(&path).unwrap();
    lrcat_fixtures::break_smart_rules(&path, 101, RuleDamage::TableMissing).unwrap();
    let d = read(&path);
    assert!(d.smart.is_empty());
    assert_eq!(d.smart_ids, all_smart());
}
