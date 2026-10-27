//! Counter-check for P5-7: keywords without any converted assignment stay in
//! the reader result. Uses only the existing API, so it runs today.

use lr_reader::Catalog;
use lrcat_fixtures::{keyword_attr, write_keyword_attr_catalog};
use model::Report;

#[test]
fn unused_keywords_are_kept_in_the_result() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("attrs.lrcat");
    write_keyword_attr_catalog(&path).unwrap();
    let catalog = Catalog::open(&path).unwrap();
    let photos = catalog.photos(&mut Report::new()).unwrap();
    let data = catalog
        .keywords(&photos, &mut Report::new(), &mut Report::new())
        .unwrap();
    assert_eq!(data.keywords.len() as i64, keyword_attr::KEYWORDS_CONVERTED);
    let used: Vec<_> = data.assignments.values().flatten().collect();
    let unused: Vec<_> = data
        .keywords
        .iter()
        .filter(|k| !used.iter().any(|u| u.path() == k.path()))
        .collect();
    assert_eq!(unused.len() as i64, keyword_attr::UNUSED);
    // Keyword 27 is only assigned to a skipped photo; it is still present.
    assert!(
        data.keywords
            .iter()
            .any(|k| k.path() == ["Zyx-Attr-27".to_string()])
    );
}
