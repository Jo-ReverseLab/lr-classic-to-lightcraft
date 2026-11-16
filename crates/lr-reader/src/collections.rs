//! Reading collections (`AgLibraryCollection`) and their members
//! (`AgLibraryCollectionImage`).
//!
//! Plain collections become albums, collection sets become folders. The
//! first system collection by id is the Quick Collection; further system
//! collections, smart collections and every other kind are skipped with a
//! reason; the reason of a smart collection names its first criterion,
//! read from `AgLibraryCollectionContent`. A collection whose parent is
//! missing, skipped, not a collection set or part of a cycle is skipped as
//! well, and so is everything below it.
//!
//! Reasons name ids, kinds and criteria, never collection names or rule
//! values.

use crate::catalog::{Catalog, ReadError};
use crate::smart::parse_smart_rules;
use model::{
    Collection, CollectionKind, Photo, QuickCollection, Report, SkipCode, Skipped, SmartCollection,
    SmartRules,
};
use rusqlite::types::ValueRef;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

const PLAIN: &str = "com.adobe.ag.library.collection";
const SET: &str = "com.adobe.ag.library.group";
const SMART: &str = "com.adobe.ag.library.smart_collection";
/// `owningModule` of the content row that holds smart collection rules.
const SMART_MODULE: &str = "ag.library.smart_collection";

/// Collections and their members as read from a catalog.
#[derive(Debug, Clone, Default)]
pub struct CollectionData {
    /// Converted collections, ascending by `lightroom_id`.
    pub collections: Vec<Collection>,
    /// The Quick Collection, if the catalog has one.
    pub quick: Option<QuickCollection>,
    /// One entry per `AgLibraryCollection` row: converted or skipped.
    pub report: Report,
    /// One entry per `AgLibraryCollectionImage` row: converted (a member of
    /// a converted album or of the Quick Collection) or skipped.
    pub members: Report,
    /// Readable rules of smart collections, by collection id. Smart
    /// collections are skipped all the same; the rules are kept for
    /// translation.
    pub smart: BTreeMap<i64, SmartRules>,
    /// The smart collections with readable rules and a name that is valid
    /// text, ascending by id, with their parent as stored (`None` on top
    /// level or for a parent that is not a valid id). They are skipped in
    /// `report` all the same; whether one can be converted is decided by
    /// the writer side.
    pub smart_collections: Vec<SmartCollection>,
    /// Smart collections the reader decided are convertible (translatable
    /// rules and a converted-folder parent, or top level), ascending by id.
    /// The reader books each as converted; every other id in `smart_ids` is
    /// skipped with a reason. The verdict comes from the reader, so the
    /// caller never rewrites the report by reason text.
    pub convertible_smart: Vec<SmartCollection>,
    /// Ids of every smart collection row (`creationId` smart_collection,
    /// system flag not set), whether its rules are readable or not and
    /// whether its name is valid. Never a plain album or set. Independent
    /// of `smart` and `smart_collections`, so `smart.keys()` is a subset.
    pub smart_ids: BTreeSet<i64>,
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
    own: Result<(CollectionKind, String), Skipped>,
    parent: ParentRef,
    /// The name, if it is valid text.
    name: Option<String>,
    /// `systemOnly` is set: a Quick Collection candidate.
    quick: bool,
}

/// Where the members of a collection go.
#[derive(Debug, Clone, Copy)]
enum Target {
    /// `data.collections[i]`.
    Collection(usize),
    Quick,
}

impl Catalog {
    /// Reads all collections and their members.
    ///
    /// `photos` are the converted photos: member rows naming any other
    /// photo are skipped. Both reports are complete: every row is counted
    /// as read and then as converted or skipped.
    pub fn collection_data(&self, photos: &[Photo]) -> Result<CollectionData, ReadError> {
        let contents = self.smart_contents()?;
        let mut smart: BTreeMap<i64, SmartRules> = BTreeMap::new();
        let mut smart_ids: BTreeSet<i64> = BTreeSet::new();
        let rows = self.collection_rows(|id| {
            smart_ids.insert(id);
            let rules = smart_rules(contents.get(&id).map(Vec::as_slice));
            let reason = smart_reason(id, rules.as_ref());
            if let Some(rules) = rules {
                smart.insert(id, rules);
            }
            reason
        })?;
        let status = resolve(&rows);
        let smart_collections = smart_collections(&smart, &rows);
        let mut data = CollectionData {
            smart,
            smart_collections,
            smart_ids,
            ..CollectionData::default()
        };
        let mut index: HashMap<i64, Target> = HashMap::new();
        let first_quick = rows.iter().find(|(_, r)| r.quick).map(|(&id, _)| id);
        for (&id, row) in &rows {
            data.report.read();
            if Some(id) == first_quick {
                data.report.converted();
                index.insert(id, Target::Quick);
                data.quick = Some(QuickCollection {
                    lightroom_id: id,
                    members: Vec::new(),
                });
                continue;
            }
            // Smart collections are booked after the plain ones: their final
            // verdict (translatable rules and a converted-folder parent) needs
            // the converted collections, which are only complete once the loop
            // ends. Only the read is counted here.
            if data.smart_ids.contains(&id) {
                continue;
            }
            match (&row.own, &status[&id]) {
                (Ok((kind, name)), Ok(())) => {
                    data.report.converted();
                    index.insert(id, Target::Collection(data.collections.len()));
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
                // Own skips are part of the status; the second arm only
                // keeps the books balanced should that ever change.
                (_, Err(skip)) | (Err(skip), Ok(())) => data.report.push_skipped(skip.clone()),
            }
        }
        // Book every smart collection with its final verdict. Those with
        // readable rules and a valid name get the rule verdict (and a
        // converted-folder parent check); the rest keep the reason set where
        // they arose (unreadable rules, or the first criterion).
        for s in &data.smart_collections {
            match smart_verdict(s, &data.collections) {
                Ok(()) => {
                    data.report.converted();
                    data.convertible_smart.push(s.clone());
                }
                Err(skipped) => data.report.push_skipped(skipped),
            }
        }
        for &id in &data.smart_ids {
            if data.smart_collections.iter().any(|s| s.lightroom_id == id) {
                continue;
            }
            data.report
                .push_skipped(smart_reason(id, data.smart.get(&id)));
        }
        let converted: HashSet<i64> = photos.iter().map(|p| p.lightroom_id).collect();
        let mut seen: HashSet<(i64, i64)> = HashSet::new();
        for (collection, image) in self.member_rows()? {
            data.members.read();
            let target = collection.and_then(|c| index.get(&c).copied());
            let outcome = match (collection, image, target) {
                (Some(c), Some(p), Some(t)) => {
                    let folder = matches!(t, Target::Collection(i)
                        if data.collections[i].kind == CollectionKind::Folder);
                    member_check(c, folder, p, converted.contains(&p), seen.contains(&(c, p)))
                        .map(|()| (c, p, t))
                }
                _ => Err(unplaced_member(
                    collection,
                    image,
                    collection.is_some_and(|c| rows.contains_key(&c)),
                )),
            };
            match outcome {
                Ok((c, p, t)) => {
                    seen.insert((c, p));
                    // A Quick target exists only together with `data.quick`.
                    let members = match t {
                        Target::Collection(i) => Some(&mut data.collections[i].members),
                        Target::Quick => data.quick.as_mut().map(|q| &mut q.members),
                    };
                    if let Some(members) = members {
                        members.push(p);
                    }
                    data.members.converted();
                }
                Err(skip) => data.members.push_skipped(skip),
            }
        }
        Ok(data)
    }

    /// All collection rows, by id. `smart` gives the reason for a smart
    /// collection with the given id.
    fn collection_rows(
        &self,
        mut smart: impl FnMut(i64) -> Skipped,
    ) -> Result<BTreeMap<i64, Row>, ReadError> {
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
                let own = classify(id, creation, system_only, &mut smart)
                    .and_then(|kind| named(id, kind, name));
                let name = name.map(str::to_owned);
                let parent = parent_ref(row.get_ref(4)?);
                Ok((
                    id,
                    Row {
                        own,
                        parent,
                        name,
                        quick: system_only,
                    },
                ))
            })
            .map_err(ReadError::Query)?;
        rows.collect::<Result<_, _>>().map_err(ReadError::Query)
    }

    /// The `content` of every smart-rule row (`owningModule` is
    /// [`SMART_MODULE`]), by collection id, in row order. `None` stands
    /// for content that is not valid text. A catalog without the table
    /// has no rows.
    fn smart_contents(&self) -> Result<HashMap<i64, Vec<Option<String>>>, ReadError> {
        let exists = self
            .conn
            .prepare(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' \
                 AND name = 'AgLibraryCollectionContent'",
            )
            .and_then(|mut s| s.exists([]))
            .map_err(ReadError::Query)?;
        let mut out: HashMap<i64, Vec<Option<String>>> = HashMap::new();
        if !exists {
            return Ok(out);
        }
        let mut stmt = self
            .conn
            .prepare(
                "SELECT collection, content FROM AgLibraryCollectionContent \
                 WHERE owningModule = ?1 ORDER BY id_local",
            )
            .map_err(ReadError::Query)?;
        let rows = stmt
            .query_map([SMART_MODULE], |row| {
                Ok((
                    as_id(row.get_ref(0)?),
                    text(row.get_ref(1)?).map(str::to_owned),
                ))
            })
            .map_err(ReadError::Query)?;
        for row in rows {
            if let (Some(id), content) = row.map_err(ReadError::Query)? {
                out.entry(id).or_default().push(content);
            }
        }
        Ok(out)
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

/// The smart collections with readable rules whose name is valid text, by
/// id.
fn smart_collections(
    smart: &BTreeMap<i64, SmartRules>,
    rows: &BTreeMap<i64, Row>,
) -> Vec<SmartCollection> {
    smart
        .iter()
        .filter_map(|(&id, rules)| {
            let row = rows.get(&id)?;
            Some(SmartCollection {
                lightroom_id: id,
                name: row.name.clone()?,
                parent: match row.parent {
                    ParentRef::Id(p) => Some(p),
                    _ => None,
                },
                rules: rules.clone(),
            })
        })
        .collect()
}

/// Why a member row is skipped that has no valid ids or no converted
/// collection; `known` is true if its collection has a row.
fn unplaced_member(collection: Option<i64>, image: Option<i64>, known: bool) -> Skipped {
    match (collection, image) {
        (None, Some(p)) => Skipped::new(
            SkipCode::CollectionIdInvalid,
            format!("photo {p}: collection id is not valid"),
        ),
        (None, None) => Skipped::new(
            SkipCode::IdsInvalid,
            "collection and photo ids are not valid",
        ),
        (Some(c), None) => Skipped::new(
            SkipCode::CollectionPhotoInvalid,
            format!("collection {c}: photo id is not valid"),
        ),
        (Some(c), Some(p)) if known => Skipped::new(
            SkipCode::CollectionSkipped,
            format!("photo {p}: collection skipped (collection {c})"),
        ),
        (Some(c), Some(p)) => Skipped::new(
            SkipCode::CollectionUnknown,
            format!("photo {p}: collection {c} is unknown"),
        ),
    }
}

/// Checks one member `photo` of the converted collection `c`; `folder` is
/// true if `c` is a collection set.
fn member_check(
    c: i64,
    folder: bool,
    photo: i64,
    photo_converted: bool,
    repeated: bool,
) -> Result<(), Skipped> {
    if folder {
        Err(Skipped::new(
            SkipCode::CollectionIsSet,
            format!("photo {photo}: collection {c} is a collection set and holds no photos"),
        ))
    } else if !photo_converted {
        Err(Skipped::new(
            SkipCode::PhotoNotConverted,
            format!("photo {photo}: skipped or unknown photo, not added to collection {c}"),
        ))
    } else if repeated {
        Err(Skipped::new(
            SkipCode::DuplicateMember,
            format!("photo {photo}: duplicate member of collection {c}"),
        ))
    } else {
        Ok(())
    }
}

/// The rules of a smart collection from its content rows: exactly one row
/// with readable text, else `None`.
fn smart_rules(contents: Option<&[Option<String>]>) -> Option<SmartRules> {
    match contents? {
        [Some(text)] => parse_smart_rules(text).ok(),
        _ => None,
    }
}

/// The reader's final verdict for a smart collection with readable rules and
/// a valid name: `Ok(())` if every rule translates and its parent is a
/// converted collection set (or it is top level), or the skip entry saying
/// why it is not converted. The reason text matches what the caller used to
/// rewrite by prefix; the code is set here, where the reason arises, never
/// derived from the text.
fn smart_verdict(s: &SmartCollection, collections: &[Collection]) -> Result<(), Skipped> {
    if let Err((code, detail)) = s.rules.verdict() {
        return Err(Skipped::new(
            code,
            format!(
                "collection {}: smart collection is not converted ({detail})",
                s.lightroom_id
            ),
        ));
    }
    if let Some(parent) = s.parent
        && !collections
            .iter()
            .any(|c| c.lightroom_id == parent && c.kind == CollectionKind::Folder)
    {
        return Err(Skipped::new(
            SkipCode::ParentNotSet,
            format!(
                "collection {}: smart collection is not converted (its parent collection \
                 {parent} is not a converted collection set)",
                s.lightroom_id
            ),
        ));
    }
    Ok(())
}

/// The skip reason of smart collection `id`. It names the first criterion
/// only, never a value, operation or combine text.
fn smart_reason(id: i64, rules: Option<&SmartRules>) -> Skipped {
    let (code, detail) = match rules {
        None => (SkipCode::SmartUnreadable, "unreadable rules".to_string()),
        Some(r) => match r.rules.first() {
            None => (SkipCode::SmartNoCriteria, "no criteria".to_string()),
            Some(first) => (
                SkipCode::SmartUntranslatable,
                format!("first criterion: {}", first.criterion),
            ),
        },
    };
    Skipped::new(
        code,
        format!("collection {id}: smart collection is not converted ({detail})"),
    )
}

/// The kind and name of collection `id`, or why it is skipped: its name
/// is not valid text.
fn named(
    id: i64,
    kind: CollectionKind,
    name: Option<&str>,
) -> Result<(CollectionKind, String), Skipped> {
    name.map(|name| (kind, name.to_owned())).ok_or_else(|| {
        Skipped::new(
            SkipCode::NameInvalid,
            format!("collection {id}: name is not valid text"),
        )
    })
}

/// The kind of a collection, or why it is skipped. The system flag wins
/// over the `creationId`; the reason for a system collection applies to
/// all but the first one, which the caller converts as the Quick
/// Collection. `smart` gives the reason for a smart collection.
fn classify(
    id: i64,
    creation_id: Option<&str>,
    system_only: bool,
    smart: impl FnOnce(i64) -> Skipped,
) -> Result<CollectionKind, Skipped> {
    if system_only {
        return Err(Skipped::new(
            SkipCode::QuickDuplicate,
            format!("collection {id}: more than one quick collection; not converted"),
        ));
    }
    match creation_id {
        Some(PLAIN) => Ok(CollectionKind::Album),
        Some(SET) => Ok(CollectionKind::Folder),
        Some(SMART) => Err(smart(id)),
        Some(other) => Err(Skipped::new(
            SkipCode::CollectionKindUnsupported,
            format!("collection {id}: collection type {other:?} is not converted"),
        )),
        None => Err(Skipped::new(
            SkipCode::CollectionKindMissing,
            format!("collection {id}: collection type is missing"),
        )),
    }
}

/// The final status of every row: converted, or the reason why it is
/// skipped (its own reason, or one about its parent).
fn resolve(rows: &BTreeMap<i64, Row>) -> HashMap<i64, Result<(), Skipped>> {
    let mut status: HashMap<i64, Result<(), Skipped>> = HashMap::new();
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
                        Some(Err(Skipped::new(
                            SkipCode::ParentInvalid,
                            format!("collection {cur}: parent is not a valid id"),
                        ))),
                        path.len(),
                    );
                }
                ParentRef::Id(p) => p,
            };
            let problem = match rows.get(&p).map(|r| &r.own) {
                None => Some(Skipped::new(
                    SkipCode::ParentMissing,
                    format!("collection {cur}: parent {p} is missing"),
                )),
                Some(Err(_)) => Some(parent_skipped(cur, p)),
                Some(Ok((CollectionKind::Album, _))) => Some(Skipped::new(
                    SkipCode::ParentNotSet,
                    format!("collection {cur}: parent {p} is not a collection set"),
                )),
                Some(Ok((CollectionKind::Folder, _))) => None,
            };
            if let Some(reason) = problem {
                break (Some(Err(reason)), path.len());
            }
            if let Some(known) = status.get(&p) {
                let mine = match known {
                    Ok(()) => Ok(()),
                    Err(_) => Err(parent_skipped(cur, p)),
                };
                break (Some(mine), path.len());
            }
            cur = p;
        };
        for &id in &path[cycle_from..] {
            status.insert(
                id,
                Err(Skipped::new(
                    SkipCode::ParentCycle,
                    format!("collection {id}: parent chain forms a cycle"),
                )),
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
                Err(_) => Err(parent_skipped(id, parent)),
            };
            status.insert(id, mine);
        }
    }
    status
}

/// The skip entry of collection `id` whose parent `parent` is skipped.
fn parent_skipped(id: i64, parent: i64) -> Skipped {
    Skipped::new(
        SkipCode::ParentSkipped,
        format!("collection {id}: parent {parent} is skipped"),
    )
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
pub(crate) fn as_id(value: ValueRef<'_>) -> Option<i64> {
    match value {
        ValueRef::Integer(n) => Some(n),
        ValueRef::Real(f) if f.fract() == 0.0 && f.abs() < 9.0e15 => Some(f as i64),
        _ => None,
    }
}

/// True if a `systemOnly` value is anything other than zero or NULL.
/// Numeric text such as `"0"` or `"0.0"` counts as zero.
pub(crate) fn is_nonzero(value: ValueRef<'_>) -> bool {
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

    fn sk(code: SkipCode, reason: &str) -> Skipped {
        Skipped::new(code, reason)
    }

    fn row(kind: Option<CollectionKind>, parent: ParentRef) -> Row {
        Row {
            own: kind
                .map(|k| (k, String::new()))
                .ok_or_else(|| sk(SkipCode::Unspecified, "own")),
            parent,
            name: Some(String::new()),
            quick: false,
        }
    }

    #[test]
    fn smart_ids_include_smart_collections_with_unreadable_rules() {
        use lrcat_fixtures::{RuleDamage, smart_edge as e};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("smart.lrcat");
        lrcat_fixtures::write_smart_catalog(&path).unwrap();
        lrcat_fixtures::break_smart_rules(&path, 101, RuleDamage::NullContent).unwrap();
        let catalog = Catalog::open(&path).unwrap();
        let data = catalog.collection_data(&[]).unwrap();
        assert!(!data.smart.contains_key(&101));
        assert!(data.smart_ids.contains(&101));
        assert!(data.smart_ids.contains(&e::OTHER_MODULE_ONLY));
        assert!(!data.smart_ids.contains(&e::PLAIN_WITH_RULES));
        assert!(data.smart.keys().all(|id| data.smart_ids.contains(id)));
    }

    #[test]
    fn smart_collections_need_rules_and_a_text_name() {
        let named = |name: Option<&str>, parent| Row {
            own: Err(sk(SkipCode::SmartUnreadable, "smart")),
            parent,
            name: name.map(str::to_owned),
            quick: false,
        };
        let rows: BTreeMap<i64, Row> = [
            (1, named(Some("a"), ParentRef::Top)),
            (2, named(Some("b"), ParentRef::Id(7))),
            (3, named(None, ParentRef::Top)),
            (4, named(Some("d"), ParentRef::Invalid)),
            (5, named(Some("e"), ParentRef::Top)),
        ]
        .into_iter()
        .collect();
        let smart: BTreeMap<i64, SmartRules> = [1, 2, 3, 4]
            .into_iter()
            .map(|id| (id, rules(&["rating"])))
            .collect();
        let got: Vec<(i64, String, Option<i64>)> = smart_collections(&smart, &rows)
            .into_iter()
            .map(|s| (s.lightroom_id, s.name, s.parent))
            .collect();
        let want = [(1, "a", None), (2, "b", Some(7)), (4, "d", None)];
        let want: Vec<(i64, String, Option<i64>)> = want
            .iter()
            .map(|(i, n, p)| (*i, n.to_string(), *p))
            .collect();
        assert_eq!(got, want);
    }

    fn smart(id: i64) -> Skipped {
        sk(SkipCode::SmartUnreadable, &format!("smart {id}"))
    }

    fn rules(criteria: &[&str]) -> SmartRules {
        SmartRules {
            combine: model::SmartCombine::Union,
            rules: criteria
                .iter()
                .map(|c| model::SmartRule {
                    criterion: c.to_string(),
                    operation: "zyxOp".into(),
                    value: Some(model::SmartValue::Text("zyxValue".into())),
                    value2: None,
                    units: Some("zyxUnit".into()),
                })
                .collect(),
        }
    }

    #[test]
    fn smart_reasons_name_the_first_criterion_only() {
        assert_eq!(
            smart_reason(7, Some(&rules(&["rating", "pick"]))),
            sk(
                SkipCode::SmartUntranslatable,
                "collection 7: smart collection is not converted (first criterion: rating)"
            )
        );
        assert_eq!(
            smart_reason(7, Some(&rules(&[]))),
            sk(
                SkipCode::SmartNoCriteria,
                "collection 7: smart collection is not converted (no criteria)"
            )
        );
        assert_eq!(
            smart_reason(7, None),
            sk(
                SkipCode::SmartUnreadable,
                "collection 7: smart collection is not converted (unreadable rules)"
            )
        );
    }

    #[test]
    fn smart_rules_need_exactly_one_readable_row() {
        let good =
            "s = { { criteria = \"rating\", operation = \"==\", value = 1 }, combine = \"union\" }";
        let one = [Some(good.to_string())];
        assert_eq!(
            smart_rules(Some(&one)).unwrap().rules[0].criterion,
            "rating"
        );
        assert_eq!(smart_rules(None), None);
        assert_eq!(smart_rules(Some(&[])), None);
        assert_eq!(smart_rules(Some(&[None])), None);
        assert_eq!(smart_rules(Some(&[Some("s = {".into())])), None);
        let two = [Some(good.to_string()), Some(good.to_string())];
        assert_eq!(smart_rules(Some(&two)), None);
    }

    const F: Option<CollectionKind> = Some(CollectionKind::Folder);
    const A: Option<CollectionKind> = Some(CollectionKind::Album);

    #[test]
    fn kinds_and_reasons() {
        assert_eq!(
            classify(2, Some(PLAIN), false, smart),
            Ok(CollectionKind::Album)
        );
        assert_eq!(
            classify(1, Some(SET), false, smart),
            Ok(CollectionKind::Folder)
        );
        assert_eq!(classify(5, Some(SMART), false, smart), Err(smart(5)));
        assert_eq!(
            classify(9, Some("com.example.x"), false, smart),
            Err(sk(
                SkipCode::CollectionKindUnsupported,
                "collection 9: collection type \"com.example.x\" is not converted"
            ))
        );
        assert_eq!(
            classify(10, None, false, smart),
            Err(sk(
                SkipCode::CollectionKindMissing,
                "collection 10: collection type is missing"
            ))
        );
    }

    #[test]
    fn system_wins_over_creation_id() {
        for creation in [Some(PLAIN), Some(SET), Some(SMART), None] {
            assert_eq!(
                classify(8, creation, true, smart),
                Err(sk(
                    SkipCode::QuickDuplicate,
                    "collection 8: more than one quick collection; not converted"
                ))
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
        let reason = |id: i64| s[&id].clone().err().map(|e| e.reason).unwrap_or_default();
        let code = |id: i64| s[&id].clone().err().map(|e| e.code);
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
        assert_eq!(code(4), Some(SkipCode::ParentMissing));
        assert_eq!(code(5), Some(SkipCode::ParentNotSet));
        assert_eq!(code(7), Some(SkipCode::ParentSkipped));
        assert_eq!(code(8), Some(SkipCode::ParentSkipped));
        assert_eq!(code(9), Some(SkipCode::ParentCycle));
        assert_eq!(code(11), Some(SkipCode::ParentSkipped));
        assert_eq!(code(12), Some(SkipCode::ParentCycle));
        assert_eq!(code(13), Some(SkipCode::ParentInvalid));
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
    fn unplaced_members_have_their_codes() {
        let code = |c, p, known| unplaced_member(c, p, known).code;
        assert_eq!(code(None, Some(3), false), SkipCode::CollectionIdInvalid);
        assert_eq!(code(None, None, true), SkipCode::IdsInvalid);
        assert_eq!(code(Some(7), None, true), SkipCode::CollectionPhotoInvalid);
        assert_eq!(code(Some(7), Some(3), true), SkipCode::CollectionSkipped);
        assert_eq!(code(Some(7), Some(3), false), SkipCode::CollectionUnknown);
        assert_eq!(
            unplaced_member(Some(7), Some(3), false).reason,
            "photo 3: collection 7 is unknown"
        );
        assert_eq!(
            unplaced_member(None, None, false).reason,
            "collection and photo ids are not valid"
        );
    }

    #[test]
    fn name_must_be_text() {
        assert_eq!(
            named(4, CollectionKind::Album, Some("a")),
            Ok((CollectionKind::Album, "a".to_string()))
        );
        let skip = named(4, CollectionKind::Album, None).unwrap_err();
        assert_eq!(skip.code, SkipCode::NameInvalid);
        assert_eq!(skip.reason, "collection 4: name is not valid text");
    }

    #[test]
    fn member_checks_in_order() {
        assert_eq!(member_check(7, false, 3, true, false), Ok(()));
        assert_eq!(
            member_check(7, false, 3, false, true),
            Err(sk(
                SkipCode::PhotoNotConverted,
                "photo 3: skipped or unknown photo, not added to collection 7"
            ))
        );
        assert_eq!(
            member_check(7, false, 3, true, true),
            Err(sk(
                SkipCode::DuplicateMember,
                "photo 3: duplicate member of collection 7"
            ))
        );
        assert_eq!(
            member_check(7, true, 3, true, false),
            Err(sk(
                SkipCode::CollectionIsSet,
                "photo 3: collection 7 is a collection set and holds no photos"
            ))
        );
    }
}
