//! Reading collections (`AgLibraryCollection`) and their members
//! (`AgLibraryCollectionImage`).
//!
//! Plain collections become albums, collection sets become folders. Smart
//! collections, the Quick Collection and other system collections, and
//! every other kind are skipped with a reason. A collection whose parent is
//! missing, skipped, not a collection set or part of a cycle is skipped as
//! well, and so is everything below it.
//!
//! Reasons name ids and kinds, never collection names.

use crate::catalog::{Catalog, ReadError};
use model::{Collection, CollectionKind, Photo, Report};
use rusqlite::types::ValueRef;
use std::collections::{BTreeMap, HashMap, HashSet};

const PLAIN: &str = "com.adobe.ag.library.collection";
const SET: &str = "com.adobe.ag.library.group";
const SMART: &str = "com.adobe.ag.library.smart_collection";

/// Collections and their members as read from a catalog.
#[derive(Debug, Clone, Default)]
pub struct CollectionData {
    /// Converted collections, ascending by `lightroom_id`.
    pub collections: Vec<Collection>,
    /// One entry per `AgLibraryCollection` row: converted or skipped.
    pub report: Report,
    /// One entry per `AgLibraryCollectionImage` row: converted (a member of
    /// a converted album) or skipped.
    pub members: Report,
}

/// Where a row points with its `parent` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParentRef {
    /// NULL or 0.
    Top,
    Id(i64),
    /// Neither NULL nor a whole number.
    Invalid,
}

/// `(collection, image)` of one `AgLibraryCollectionImage` row; `None`
/// where the value is not a whole number.
type MemberRow = (Option<i64>, Option<i64>);

/// One `AgLibraryCollection` row after its own checks.
#[derive(Debug, Clone)]
struct Row {
    /// The kind and name, or the reason why the row itself is skipped.
    own: Result<(CollectionKind, String), String>,
    parent: ParentRef,
}

impl Catalog {
    /// Reads all collections and their members.
    ///
    /// `photos` are the converted photos: member rows naming any other
    /// photo are skipped. Both reports are complete: every row is counted
    /// as read and then as converted or skipped.
    pub fn collection_data(&self, photos: &[Photo]) -> Result<CollectionData, ReadError> {
        let rows = self.collection_rows()?;
        let status = resolve(&rows);
        let mut data = CollectionData::default();
        let mut index: HashMap<i64, usize> = HashMap::new();
        for (&id, row) in &rows {
            data.report.read();
            match (&row.own, &status[&id]) {
                (Ok((kind, name)), Ok(())) => {
                    data.report.converted();
                    index.insert(id, data.collections.len());
                    data.collections.push(Collection {
                        lightroom_id: id,
                        name: name.clone(),
                        kind: *kind,
                        parent: match row.parent {
                            ParentRef::Id(p) => Some(p),
                            _ => None,
                        },
                        members: Vec::new(),
                    });
                }
                (_, Err(reason)) => data.report.skipped(reason.clone()),
                (Err(_), Ok(())) => unreachable!("own skips are part of the status"),
            }
        }
        let converted: HashSet<i64> = photos.iter().map(|p| p.lightroom_id).collect();
        let mut seen: HashSet<(i64, i64)> = HashSet::new();
        for (collection, image) in self.member_rows()? {
            data.members.read();
            let target = collection.and_then(|c| index.get(&c).copied());
            let outcome = match (collection, image, target) {
                (None, _, _) => Err(match image {
                    Some(p) => format!("photo {p}: collection id is not valid"),
                    None => "collection and photo ids are not valid".to_string(),
                }),
                (Some(c), None, _) => Err(format!("collection {c}: photo id is not valid")),
                (Some(c), Some(p), None) => Err(if rows.contains_key(&c) {
                    format!("photo {p}: collection skipped (collection {c})")
                } else {
                    format!("photo {p}: collection {c} is unknown")
                }),
                (Some(c), Some(p), Some(i)) => member_check(
                    &data.collections[i],
                    p,
                    converted.contains(&p),
                    seen.contains(&(c, p)),
                )
                .map(|()| (c, p, i)),
            };
            match outcome {
                Ok((c, p, i)) => {
                    seen.insert((c, p));
                    data.collections[i].members.push(p);
                    data.members.converted();
                }
                Err(reason) => data.members.skipped(reason),
            }
        }
        Ok(data)
    }

    /// All collection rows, by id.
    fn collection_rows(&self) -> Result<BTreeMap<i64, Row>, ReadError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id_local, creationId, systemOnly, name, parent \
                 FROM AgLibraryCollection ORDER BY id_local",
            )
            .map_err(ReadError::Query)?;
        let rows = stmt
            .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let creation = text(row.get_ref(1)?);
                let system_only = is_nonzero(row.get_ref(2)?);
                let name = text(row.get_ref(3)?);
                let own = classify(id, creation, system_only).and_then(|kind| {
                    name.map(|name| (kind, name.to_owned()))
                        .ok_or_else(|| format!("collection {id}: name is not valid text"))
                });
                let parent = parent_ref(row.get_ref(4)?);
                Ok((id, Row { own, parent }))
            })
            .map_err(ReadError::Query)?;
        rows.collect::<Result<_, _>>().map_err(ReadError::Query)
    }

    /// `(collection, image)` of every member row, ordered by collection,
    /// then `positionInCollection` (NULL last, otherwise SQLite's order:
    /// numbers before text), then row id.
    fn member_rows(&self) -> Result<Vec<MemberRow>, ReadError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT collection, image FROM AgLibraryCollectionImage \
                 ORDER BY collection, positionInCollection IS NULL, positionInCollection, id_local",
            )
            .map_err(ReadError::Query)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((as_id(row.get_ref(0)?), as_id(row.get_ref(1)?)))
            })
            .map_err(ReadError::Query)?;
        rows.collect::<Result<_, _>>().map_err(ReadError::Query)
    }
}

/// Checks one member `photo` of a converted `collection`.
fn member_check(
    collection: &Collection,
    photo: i64,
    photo_converted: bool,
    repeated: bool,
) -> Result<(), String> {
    let c = collection.lightroom_id;
    if collection.kind == CollectionKind::Folder {
        Err(format!(
            "photo {photo}: collection {c} is a collection set and holds no photos"
        ))
    } else if !photo_converted {
        Err(format!(
            "photo {photo}: skipped or unknown photo, not added to collection {c}"
        ))
    } else if repeated {
        Err(format!("photo {photo}: duplicate member of collection {c}"))
    } else {
        Ok(())
    }
}

/// The kind of a collection, or why it is skipped. The system flag wins
/// over the `creationId`.
fn classify(
    id: i64,
    creation_id: Option<&str>,
    system_only: bool,
) -> Result<CollectionKind, String> {
    if system_only {
        return Err(format!(
            "collection {id}: quick collection (system only) is not converted"
        ));
    }
    match creation_id {
        Some(PLAIN) => Ok(CollectionKind::Album),
        Some(SET) => Ok(CollectionKind::Folder),
        Some(SMART) => Err(format!(
            "collection {id}: smart collection is not converted"
        )),
        Some(other) => Err(format!(
            "collection {id}: collection type {other:?} is not converted"
        )),
        None => Err(format!("collection {id}: collection type is missing")),
    }
}

/// The final status of every row: converted, or the reason why it is
/// skipped (its own reason, or one about its parent).
fn resolve(rows: &BTreeMap<i64, Row>) -> HashMap<i64, Result<(), String>> {
    let mut status: HashMap<i64, Result<(), String>> = HashMap::new();
    for (&id, row) in rows {
        if let Err(reason) = &row.own {
            status.insert(id, Err(reason.clone()));
        }
    }
    for &start in rows.keys() {
        if status.contains_key(&start) {
            continue;
        }
        // Walk up until the top, a known row, a problem or a repeat.
        let mut path: Vec<i64> = Vec::new();
        let mut on_path: HashSet<i64> = HashSet::new();
        let mut cur = start;
        // The status of the last row on `path`, and where the cycle starts.
        let (last, cycle_from) = loop {
            if !on_path.insert(cur) {
                let from = path.iter().position(|&x| x == cur).unwrap_or(0);
                break (None, from);
            }
            path.push(cur);
            let p = match rows[&cur].parent {
                ParentRef::Top => break (Some(Ok(())), path.len()),
                ParentRef::Invalid => {
                    break (
                        Some(Err(format!("collection {cur}: parent is not a valid id"))),
                        path.len(),
                    );
                }
                ParentRef::Id(p) => p,
            };
            let problem = match rows.get(&p).map(|r| &r.own) {
                None => Some(format!("collection {cur}: parent {p} is missing")),
                Some(Err(_)) => Some(format!("collection {cur}: parent {p} is skipped")),
                Some(Ok((CollectionKind::Album, _))) => Some(format!(
                    "collection {cur}: parent {p} is not a collection set"
                )),
                Some(Ok((CollectionKind::Folder, _))) => None,
            };
            if let Some(reason) = problem {
                break (Some(Err(reason)), path.len());
            }
            if let Some(known) = status.get(&p) {
                let mine = match known {
                    Ok(()) => Ok(()),
                    Err(_) => Err(format!("collection {cur}: parent {p} is skipped")),
                };
                break (Some(mine), path.len());
            }
            cur = p;
        };
        for &id in &path[cycle_from..] {
            status.insert(
                id,
                Err(format!("collection {id}: parent chain forms a cycle")),
            );
        }
        let mut below = cycle_from;
        if let Some(last) = last {
            below -= 1;
            status.insert(path[below], last);
        }
        // Rows before `below` take their status from their parent.
        for i in (0..below).rev() {
            let (id, parent) = (path[i], path[i + 1]);
            let mine = match status[&parent] {
                Ok(()) => Ok(()),
                Err(_) => Err(format!("collection {id}: parent {parent} is skipped")),
            };
            status.insert(id, mine);
        }
    }
    status
}

/// Where a `parent` value points. NULL and 0 both mean top level, as in
/// Lightroom.
fn parent_ref(value: ValueRef<'_>) -> ParentRef {
    match value {
        ValueRef::Null => ParentRef::Top,
        v => match as_id(v) {
            Some(0) => ParentRef::Top,
            Some(p) => ParentRef::Id(p),
            None => ParentRef::Invalid,
        },
    }
}

/// The text of a value, if it is valid UTF-8 text.
fn text(value: ValueRef<'_>) -> Option<&str> {
    match value {
        ValueRef::Text(t) => std::str::from_utf8(t).ok(),
        _ => None,
    }
}

/// A row id stored as INTEGER or as a whole REAL.
fn as_id(value: ValueRef<'_>) -> Option<i64> {
    match value {
        ValueRef::Integer(n) => Some(n),
        ValueRef::Real(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => Some(f as i64),
        _ => None,
    }
}

/// True if a `systemOnly` value is anything other than zero or NULL.
/// Numeric text such as `"0"` or `"0.0"` counts as zero.
fn is_nonzero(value: ValueRef<'_>) -> bool {
    match value {
        ValueRef::Null => false,
        ValueRef::Integer(n) => n != 0,
        ValueRef::Real(f) => f != 0.0,
        ValueRef::Text(t) => {
            std::str::from_utf8(t)
                .ok()
                .and_then(|t| t.trim().parse::<f64>().ok())
                != Some(0.0)
        }
        ValueRef::Blob(b) => !b.is_empty(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(kind: Option<CollectionKind>, parent: ParentRef) -> Row {
        Row {
            own: kind
                .map(|k| (k, String::new()))
                .ok_or_else(|| "own".to_string()),
            parent,
        }
    }

    const F: Option<CollectionKind> = Some(CollectionKind::Folder);
    const A: Option<CollectionKind> = Some(CollectionKind::Album);

    #[test]
    fn kinds_and_reasons() {
        assert_eq!(classify(2, Some(PLAIN), false), Ok(CollectionKind::Album));
        assert_eq!(classify(1, Some(SET), false), Ok(CollectionKind::Folder));
        assert_eq!(
            classify(5, Some(SMART), false),
            Err("collection 5: smart collection is not converted".into())
        );
        assert_eq!(
            classify(9, Some("com.example.x"), false),
            Err("collection 9: collection type \"com.example.x\" is not converted".into())
        );
        assert_eq!(
            classify(10, None, false),
            Err("collection 10: collection type is missing".into())
        );
    }

    #[test]
    fn system_wins_over_creation_id() {
        for creation in [Some(PLAIN), Some(SET), Some(SMART), None] {
            assert_eq!(
                classify(8, creation, true),
                Err("collection 8: quick collection (system only) is not converted".into())
            );
        }
    }

    #[test]
    fn system_only_values() {
        assert!(!is_nonzero(ValueRef::Null));
        assert!(!is_nonzero(ValueRef::Integer(0)));
        assert!(is_nonzero(ValueRef::Integer(1)));
        assert!(!is_nonzero(ValueRef::Real(0.0)));
        assert!(is_nonzero(ValueRef::Real(1.0)));
        assert!(!is_nonzero(ValueRef::Text(b"0")));
        assert!(!is_nonzero(ValueRef::Text(b"0.0")));
        assert!(is_nonzero(ValueRef::Text(b"1")));
        assert!(is_nonzero(ValueRef::Text(b"yes")));
        assert!(is_nonzero(ValueRef::Text(b"\xff")));
    }

    #[test]
    fn ids_are_integers_or_whole_reals() {
        assert_eq!(as_id(ValueRef::Integer(4)), Some(4));
        assert_eq!(as_id(ValueRef::Real(4.0)), Some(4));
        assert_eq!(as_id(ValueRef::Real(4.5)), None);
        assert_eq!(as_id(ValueRef::Real(f64::NAN)), None);
        assert_eq!(as_id(ValueRef::Text(b"4")), None);
        assert_eq!(as_id(ValueRef::Null), None);
    }

    #[test]
    fn parent_null_and_zero_are_top_level() {
        assert_eq!(parent_ref(ValueRef::Null), ParentRef::Top);
        assert_eq!(parent_ref(ValueRef::Integer(0)), ParentRef::Top);
        assert_eq!(parent_ref(ValueRef::Real(0.0)), ParentRef::Top);
        assert_eq!(parent_ref(ValueRef::Integer(3)), ParentRef::Id(3));
        assert_eq!(parent_ref(ValueRef::Real(3.0)), ParentRef::Id(3));
        assert_eq!(parent_ref(ValueRef::Real(0.5)), ParentRef::Invalid);
        assert_eq!(parent_ref(ValueRef::Text(b"0")), ParentRef::Invalid);
    }

    #[test]
    fn invalid_utf8_is_no_text() {
        assert_eq!(text(ValueRef::Text(b"a\xffb")), None);
        assert_eq!(text(ValueRef::Text("S\u{fc}".as_bytes())), Some("S\u{fc}"));
        assert_eq!(text(ValueRef::Integer(1)), None);
    }

    #[test]
    fn tree_is_resolved_with_reasons() {
        use ParentRef::*;
        let rows: BTreeMap<i64, Row> = [
            (1, row(F, Top)),
            (2, row(F, Id(1))),
            (3, row(A, Id(2))),
            (4, row(A, Id(99))),
            (5, row(A, Id(3))),
            (6, row(None, Top)),
            (7, row(F, Id(6))),
            (8, row(A, Id(7))),
            (9, row(F, Id(10))),
            (10, row(F, Id(9))),
            (11, row(A, Id(9))),
            (12, row(F, Id(12))),
            (13, row(A, Invalid)),
        ]
        .into_iter()
        .collect();
        let s = resolve(&rows);
        let reason = |id: i64| s[&id].clone().err().unwrap_or_default();
        for id in [1, 2, 3] {
            assert_eq!(s[&id], Ok(()), "{id}");
        }
        assert_eq!(reason(4), "collection 4: parent 99 is missing");
        assert_eq!(reason(5), "collection 5: parent 3 is not a collection set");
        assert_eq!(reason(6), "own");
        assert_eq!(reason(7), "collection 7: parent 6 is skipped");
        assert_eq!(reason(8), "collection 8: parent 7 is skipped");
        assert_eq!(reason(9), "collection 9: parent chain forms a cycle");
        assert_eq!(reason(10), "collection 10: parent chain forms a cycle");
        assert_eq!(reason(11), "collection 11: parent 9 is skipped");
        assert_eq!(reason(12), "collection 12: parent chain forms a cycle");
        assert_eq!(reason(13), "collection 13: parent is not a valid id");
        assert_eq!(s.len(), rows.len());
    }

    #[test]
    fn deep_chain_does_not_recurse() {
        let n = 100_000;
        let mut rows: BTreeMap<i64, Row> = (2..=n)
            .map(|id| (id, row(F, ParentRef::Id(id - 1))))
            .collect();
        rows.insert(1, row(F, ParentRef::Top));
        let s = resolve(&rows);
        assert!(s.values().all(Result::is_ok));
    }

    #[test]
    fn member_checks_in_order() {
        let mut c = Collection {
            lightroom_id: 7,
            name: String::new(),
            kind: CollectionKind::Album,
            parent: None,
            members: vec![],
        };
        assert_eq!(member_check(&c, 3, true, false), Ok(()));
        assert_eq!(
            member_check(&c, 3, false, true),
            Err("photo 3: skipped or unknown photo, not added to collection 7".into())
        );
        assert_eq!(
            member_check(&c, 3, true, true),
            Err("photo 3: duplicate member of collection 7".into())
        );
        c.kind = CollectionKind::Folder;
        assert_eq!(
            member_check(&c, 3, true, false),
            Err("photo 3: collection 7 is a collection set and holds no photos".into())
        );
    }
}
