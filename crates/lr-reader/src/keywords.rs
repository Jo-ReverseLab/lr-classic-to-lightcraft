//! Reading keywords (`AgLibraryKeyword`), their synonyms
//! (`AgLibraryKeywordSynonym`) and their assignments to photos
//! (`AgLibraryKeywordImage`).
//!
//! Skip reasons name ids only, never keyword names or synonym texts.

use crate::catalog::{Catalog, ReadError};
use model::{Keyword, Photo, Report, SkipCode, Skipped};
use rusqlite::OptionalExtension;
use rusqlite::types::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Keywords and their assignments read from a catalog.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeywordData {
    /// Converted keywords, ordered by `AgLibraryKeyword.id_local`.
    pub keywords: Vec<Keyword>,
    /// Keywords per Lightroom photo id (`Photo::lightroom_id`), in order of
    /// the assignment rows (`AgLibraryKeywordImage.id_local`). Photos
    /// without keywords have no entry.
    pub assignments: BTreeMap<i64, Vec<Keyword>>,
    /// One read per `AgLibraryKeywordSynonym` row; rows that are not
    /// attached to a keyword are skipped with a reason. Nothing is marked
    /// converted; the converted synonyms are those in
    /// [`Keyword::synonyms`] of [`KeywordData::keywords`].
    pub synonym_report: Report,
    /// Number of converted keywords whose `keywordType` is neither NULL nor
    /// `person`. They are converted without the person attribute.
    pub unknown_type: usize,
}

/// The kind of a keyword (`AgLibraryKeyword.keywordType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeywordType {
    /// NULL: an ordinary keyword.
    Plain,
    /// Exactly `person`.
    Person,
    /// Any other value.
    Unknown,
}

/// The attributes of one `AgLibraryKeyword` row.
#[derive(Debug, Clone, Copy)]
struct Attributes {
    include_on_export: bool,
    export_containing: bool,
    export_synonyms: bool,
    kind: KeywordType,
}

impl Default for Attributes {
    fn default() -> Self {
        Attributes {
            include_on_export: true,
            export_containing: true,
            export_synonyms: true,
            kind: KeywordType::Plain,
        }
    }
}

impl Attributes {
    /// Copies the attributes onto a converted keyword.
    fn apply(&self, keyword: &mut Keyword) {
        keyword.include_on_export = self.include_on_export;
        keyword.export_containing = self.export_containing;
        keyword.export_synonyms = self.export_synonyms;
        keyword.person = self.kind == KeywordType::Person;
    }
}

/// One `AgLibraryKeywordSynonym` row.
#[derive(Debug, Clone)]
struct SynonymRow {
    id: i64,
    keyword: Option<i64>,
    /// `None` if the name is NULL.
    name: Option<String>,
}

/// One `AgLibraryKeyword` row.
#[derive(Debug, Clone)]
struct KeywordRow {
    id: i64,
    /// `None` if the name is NULL.
    name: Option<String>,
    parent: Option<i64>,
    /// `id_global` if it is text; any other value counts as missing.
    global_id: Option<String>,
    attributes: Attributes,
}

/// One `AgLibraryKeywordImage` row.
#[derive(Debug, Clone, Copy)]
struct AssignmentRow {
    id: i64,
    image: Option<i64>,
    tag: Option<i64>,
}

impl Catalog {
    /// Reads all keywords and their assignments to `photos`.
    ///
    /// Every keyword row except the hidden root keyword
    /// (`AgLibraryKeyword_rootTagID`) is counted with [`Report::read`] on
    /// `keyword_report`; every assignment row on `assignment_report`. Rows
    /// that cannot be converted are recorded with [`Report::skipped`].
    /// Synonyms are attached to their converted keyword and counted in
    /// [`KeywordData::synonym_report`].
    /// Nothing is marked converted here; the writer does that.
    pub fn keywords(
        &self,
        photos: &[Photo],
        keyword_report: &mut Report,
        assignment_report: &mut Report,
    ) -> Result<KeywordData, ReadError> {
        let root = self.root_keyword_id()?;
        let rows = self.keyword_rows()?;
        let resolved = resolve_keywords(&rows, root);

        let known: HashSet<i64> = rows.iter().map(|r| r.id).collect();
        let attributes: HashMap<i64, Attributes> =
            rows.iter().map(|r| (r.id, r.attributes)).collect();
        let mut synonym_report = Report::new();
        let mut synonyms = group_synonyms(
            &self.synonym_rows()?,
            &known,
            &resolved,
            &mut synonym_report,
        );

        let mut keywords = Vec::new();
        let mut by_id = HashMap::new();
        let mut unknown_type = 0;
        for (id, result) in resolved {
            keyword_report.read();
            match result {
                Ok(mut keyword) => {
                    keyword.synonyms = synonyms.remove(&id).unwrap_or_default();
                    let attrs = attributes[&id];
                    attrs.apply(&mut keyword);
                    if attrs.kind == KeywordType::Unknown {
                        unknown_type += 1;
                    }
                    by_id.insert(id, keyword.clone());
                    keywords.push(keyword);
                }
                Err(skip) => keyword_report.push_skipped(skip),
            }
        }

        let photo_ids: HashSet<i64> = photos.iter().map(|p| p.lightroom_id).collect();
        let assignments = assign(
            &self.assignment_rows()?,
            &photo_ids,
            &by_id,
            assignment_report,
        );
        Ok(KeywordData {
            keywords,
            assignments,
            synonym_report,
            unknown_type,
        })
    }

    /// The id of the hidden root keyword, or `None` if the catalog does not
    /// name one (then no row is treated as the root).
    fn root_keyword_id(&self) -> Result<Option<i64>, ReadError> {
        let value: Option<Value> = self
            .conn
            .query_row(
                "SELECT value FROM Adobe_variablesTable \
                 WHERE name = 'AgLibraryKeyword_rootTagID' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(ReadError::Query)?;
        Ok(value.as_ref().and_then(value_as_id))
    }

    fn keyword_rows(&self) -> Result<Vec<KeywordRow>, ReadError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id_local, name, parent, includeOnExport, includeParents, \
                 includeSynonyms, keywordType, id_global FROM AgLibraryKeyword ORDER BY id_local",
            )
            .map_err(ReadError::Query)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(KeywordRow {
                    id: row.get(0)?,
                    name: value_as_text(row.get(1)?),
                    parent: row.get(2)?,
                    global_id: match row.get(7)? {
                        Value::Text(t) => Some(t),
                        _ => None,
                    },
                    attributes: Attributes {
                        include_on_export: value_as_flag(&row.get(3)?),
                        export_containing: value_as_flag(&row.get(4)?),
                        export_synonyms: value_as_flag(&row.get(5)?),
                        kind: value_as_type(&row.get(6)?),
                    },
                })
            })
            .map_err(ReadError::Query)?;
        rows.collect::<Result<_, _>>().map_err(ReadError::Query)
    }

    fn synonym_rows(&self) -> Result<Vec<SynonymRow>, ReadError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id_local, keyword, name FROM AgLibraryKeywordSynonym ORDER BY id_local",
            )
            .map_err(ReadError::Query)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(SynonymRow {
                    id: row.get(0)?,
                    keyword: row.get(1)?,
                    name: value_as_text(row.get(2)?),
                })
            })
            .map_err(ReadError::Query)?;
        rows.collect::<Result<_, _>>().map_err(ReadError::Query)
    }

    fn assignment_rows(&self) -> Result<Vec<AssignmentRow>, ReadError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id_local, image, tag FROM AgLibraryKeywordImage ORDER BY id_local")
            .map_err(ReadError::Query)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(AssignmentRow {
                    id: row.get(0)?,
                    image: row.get(1)?,
                    tag: row.get(2)?,
                })
            })
            .map_err(ReadError::Query)?;
        rows.collect::<Result<_, _>>().map_err(ReadError::Query)
    }
}

/// Reads an integer id stored as integer, integral real or numeric text.
fn value_as_id(value: &Value) -> Option<i64> {
    match value {
        Value::Integer(n) => Some(*n),
        Value::Real(f) if f.fract() == 0.0 => Some(*f as i64),
        Value::Text(t) => t.trim().parse().ok(),
        _ => None,
    }
}

/// Reads an export flag: zero is false; any other number and NULL are true,
/// as is a value that is not a number.
fn value_as_flag(value: &Value) -> bool {
    match value {
        Value::Integer(n) => *n != 0,
        Value::Real(f) => *f != 0.0,
        Value::Text(t) => t.trim().parse::<f64>() != Ok(0.0),
        Value::Null | Value::Blob(_) => true,
    }
}

/// Reads `keywordType`: NULL is a plain keyword, exactly `person` a person,
/// anything else unknown.
fn value_as_type(value: &Value) -> KeywordType {
    match value {
        Value::Null => KeywordType::Plain,
        Value::Text(t) if t == "person" => KeywordType::Person,
        _ => KeywordType::Unknown,
    }
}

/// Reads a name column; NULL becomes `None`.
fn value_as_text(value: Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::Text(t) => Some(t),
        Value::Integer(n) => Some(n.to_string()),
        Value::Real(f) => Some(f.to_string()),
        Value::Blob(b) => Some(String::from_utf8_lossy(&b).into_owned()),
    }
}

/// Where the walk up the parent chain of a keyword ended.
enum Top {
    /// A keyword directly below the root (or without parent).
    Root,
    /// A parent that was resolved earlier.
    Known(i64),
    /// A parent id that has no row, or that closes a cycle.
    Broken(i64),
}

/// Builds the full path of every keyword row except `root`, in row order.
///
/// A keyword is skipped if its parent is skipped, missing or part of a
/// cycle, if its global id is missing or a duplicate (see
/// [`global_ids`]), or if its name is empty or contains `|`; checked in
/// this order. A converted keyword carries its global id.
fn resolve_keywords(
    rows: &[KeywordRow],
    root: Option<i64>,
) -> Vec<(i64, Result<Keyword, Skipped>)> {
    let index: HashMap<i64, &KeywordRow> = rows.iter().map(|r| (r.id, r)).collect();
    let mut done: HashMap<i64, Result<Keyword, Skipped>> = HashMap::new();
    let mut ids = global_ids(rows, root);

    for row in rows {
        if Some(row.id) == root || done.contains_key(&row.id) {
            continue;
        }
        // Walk up until the root, a resolved keyword, a missing row or a cycle.
        let mut chain = vec![row];
        let mut on_chain = HashSet::from([row.id]);
        let top = loop {
            let current = chain[chain.len() - 1];
            match current.parent {
                None => break Top::Root,
                Some(p) if Some(p) == root => break Top::Root,
                Some(p) if done.contains_key(&p) => break Top::Known(p),
                Some(p) if on_chain.contains(&p) => break Top::Broken(p),
                Some(p) => match index.get(&p) {
                    Some(parent) => {
                        chain.push(parent);
                        on_chain.insert(p);
                    }
                    None => break Top::Broken(p),
                },
            }
        };
        // Resolve from the topmost row down.
        let mut parent: Result<Vec<String>, i64> = match top {
            Top::Root => Ok(Vec::new()),
            Top::Known(p) => match &done[&p] {
                Ok(k) => Ok(k.path().to_vec()),
                Err(_) => Err(p),
            },
            Top::Broken(p) => Err(p),
        };
        for current in chain.into_iter().rev() {
            let result = match parent {
                Err(p) => Err(Skipped::new(
                    SkipCode::ParentSkipped,
                    format!("keyword {}: parent keyword {p} is skipped", current.id),
                )),
                Ok(mut path) => match ids.remove(&current.id) {
                    Some(id_result) => id_result.and_then(|global_id| {
                        let name = own_name(current)?;
                        path.push(name);
                        Keyword::new(path)
                            .map(|k| k.with_global_id(global_id))
                            .map_err(|_| name_empty(current.id))
                    }),
                    // Defensive: global_ids covers every non-root row, so this
                    // branch is not reached by normal input. If the invariant
                    // ever breaks, the keyword is skipped instead of panicking.
                    None => Err(Skipped::new(
                        SkipCode::KeywordUnresolved,
                        format!("keyword {}: could not resolve its global id", current.id),
                    )),
                },
            };
            parent = match &result {
                Ok(k) => Ok(k.path().to_vec()),
                Err(_) => Err(current.id),
            };
            done.insert(current.id, result);
        }
    }

    rows.iter()
        .filter(|r| Some(r.id) != root)
        .map(|r| {
            (
                r.id,
                done.remove(&r.id).unwrap_or_else(|| {
                    // Defensive: the loop above inserts every non-root row
                    // into `done`, so this is not reached by normal input. If
                    // the invariant ever breaks, the keyword is skipped with a
                    // reason instead of panicking.
                    Err(Skipped::new(
                        SkipCode::KeywordUnresolved,
                        format!("keyword {}: was not resolved", r.id),
                    ))
                }),
            )
        })
        .collect()
}

/// The global id of every row except `root`, or the reason it cannot be
/// used: NULL, not text, empty or whitespace-only is missing; a value
/// already held by an earlier row in `id_local` order is a duplicate.
///
/// Unlike photos, the first row with a usable value owns it even if that
/// row is later skipped for another reason (its name or its parent): a
/// keyword's fate depends on its ancestors, which may come later in
/// `id_local` order, so ownership is decided on the rows alone.
fn global_ids(rows: &[KeywordRow], root: Option<i64>) -> HashMap<i64, Result<String, Skipped>> {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut out = HashMap::new();
    let mut sorted: Vec<&KeywordRow> = rows.iter().filter(|r| Some(r.id) != root).collect();
    sorted.sort_by_key(|r| r.id);
    for row in sorted {
        let result = match row.global_id.as_deref() {
            Some(g) if !g.trim().is_empty() => {
                if seen.insert(g) {
                    Ok(g.to_owned())
                } else {
                    Err(Skipped::new(
                        SkipCode::DuplicateGlobalId,
                        format!("keyword {}: duplicate global id", row.id),
                    ))
                }
            }
            _ => Err(Skipped::new(
                SkipCode::MissingGlobalId,
                format!("keyword {}: missing global id", row.id),
            )),
        };
        out.insert(row.id, result);
    }
    out
}

/// The keyword's own name, or the reason it cannot be used.
fn own_name(row: &KeywordRow) -> Result<String, Skipped> {
    match row.name.as_deref() {
        None => Err(name_empty(row.id)),
        Some(n) if n.trim().is_empty() => Err(name_empty(row.id)),
        Some(n) if n.contains('|') => Err(Skipped::new(
            SkipCode::NameHasBar,
            format!("keyword {}: name contains a vertical bar", row.id),
        )),
        Some(n) => Ok(n.to_owned()),
    }
}

/// The skip entry of keyword `id` with an empty name.
fn name_empty(id: i64) -> Skipped {
    Skipped::new(SkipCode::NameEmpty, format!("keyword {id}: name is empty"))
}

/// Groups the synonym texts per keyword id, in row order. Every row is
/// counted as read; a row is skipped if its keyword has no row in `known`,
/// if its keyword is not converted in `resolved` (including the hidden
/// root), or if its text is NULL, empty or whitespace-only, checked in
/// this order.
fn group_synonyms(
    rows: &[SynonymRow],
    known: &HashSet<i64>,
    resolved: &[(i64, Result<Keyword, Skipped>)],
    report: &mut Report,
) -> HashMap<i64, Vec<String>> {
    let converted: HashSet<i64> = resolved
        .iter()
        .filter(|(_, r)| r.is_ok())
        .map(|(id, _)| *id)
        .collect();
    let mut out: HashMap<i64, Vec<String>> = HashMap::new();
    for row in rows {
        report.read();
        let keyword = id_text(row.keyword);
        match row.keyword {
            Some(k) if converted.contains(&k) => match row.name.as_deref() {
                Some(n) if !n.trim().is_empty() => out.entry(k).or_default().push(n.to_owned()),
                _ => report.skip(
                    SkipCode::NameEmpty,
                    format!("synonym {}: name is empty", row.id),
                ),
            },
            Some(k) if known.contains(&k) => report.skip(
                SkipCode::KeywordSkipped,
                format!("synonym {}: keyword {keyword} is skipped", row.id),
            ),
            _ => report.skip(
                SkipCode::KeywordUnknown,
                format!("synonym {}: keyword {keyword} is unknown", row.id),
            ),
        }
    }
    out
}

/// Groups the converted keywords per photo. Every row is counted as read;
/// a row whose photo is not in `photos` or whose keyword is not in
/// `keywords` is skipped. The photo reason wins if both apply.
fn assign(
    rows: &[AssignmentRow],
    photos: &HashSet<i64>,
    keywords: &HashMap<i64, Keyword>,
    report: &mut Report,
) -> BTreeMap<i64, Vec<Keyword>> {
    let mut out: BTreeMap<i64, Vec<Keyword>> = BTreeMap::new();
    for row in rows {
        report.read();
        let image = row.image.filter(|i| photos.contains(i));
        let keyword = row.tag.and_then(|t| keywords.get(&t));
        match (image, keyword) {
            (None, _) => report.skip(
                SkipCode::AssignmentPhotoSkipped,
                format!(
                    "assignment {}: photo {} is skipped or unknown",
                    row.id,
                    id_text(row.image)
                ),
            ),
            (Some(_), None) => report.skip(
                SkipCode::AssignmentKeywordSkipped,
                format!(
                    "assignment {}: keyword {} is skipped",
                    row.id,
                    id_text(row.tag)
                ),
            ),
            (Some(image), Some(keyword)) => out.entry(image).or_default().push(keyword.clone()),
        }
    }
    out
}

fn id_text(id: Option<i64>) -> String {
    id.map_or_else(|| "NULL".to_owned(), |i| i.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: i64, name: Option<&str>, parent: Option<i64>) -> KeywordRow {
        KeywordRow {
            id,
            name: name.map(str::to_owned),
            parent,
            global_id: Some(format!("g{id}")),
            attributes: Attributes::default(),
        }
    }

    fn kw(parts: &[&str]) -> Keyword {
        Keyword::new(parts.iter().map(|s| s.to_string()).collect()).unwrap()
    }

    fn with_gid(mut r: KeywordRow, g: Option<&str>) -> KeywordRow {
        r.global_id = g.map(str::to_owned);
        r
    }

    fn reasons(resolved: &[(i64, Result<Keyword, Skipped>)]) -> Vec<String> {
        resolved
            .iter()
            .filter_map(|(_, r)| r.as_ref().err().map(|e| e.reason.clone()))
            .collect()
    }

    #[test]
    fn builds_paths_and_ignores_root() {
        let rows = [
            row(1, None, None),
            row(3, Some("Child"), Some(2)),
            row(2, Some("Top"), Some(1)),
        ];
        let resolved = resolve_keywords(&rows, Some(1));
        assert_eq!(resolved.len(), 2);
        assert_eq!(
            resolved[0],
            (3, Ok(kw(&["Top", "Child"]).with_global_id("g3")))
        );
        assert_eq!(resolved[1], (2, Ok(kw(&["Top"]).with_global_id("g2"))));
    }

    #[test]
    fn keyword_without_parent_is_top_level() {
        let resolved = resolve_keywords(&[row(5, Some("A"), None)], Some(1));
        assert_eq!(resolved, vec![(5, Ok(kw(&["A"]).with_global_id("g5")))]);
    }

    #[test]
    fn without_root_id_every_row_counts() {
        let resolved = resolve_keywords(&[row(1, None, None), row(2, Some("A"), Some(1))], None);
        assert_eq!(
            reasons(&resolved),
            vec![
                "keyword 1: name is empty",
                "keyword 2: parent keyword 1 is skipped"
            ]
        );
    }

    #[test]
    fn subtree_of_vertical_bar_keyword_is_skipped() {
        let rows = [
            row(1, None, None),
            row(2, Some("a|b"), Some(1)),
            row(3, Some("Child"), Some(2)),
            row(4, Some("Grandchild"), Some(3)),
        ];
        let resolved = resolve_keywords(&rows, Some(1));
        assert_eq!(
            reasons(&resolved),
            vec![
                "keyword 2: name contains a vertical bar",
                "keyword 3: parent keyword 2 is skipped",
                "keyword 4: parent keyword 3 is skipped",
            ]
        );
    }

    #[test]
    fn missing_global_id_skips_keyword_and_subtree() {
        let rows = [
            with_gid(row(2, Some("A"), Some(1)), None),
            with_gid(row(3, Some("B"), Some(1)), Some(" \t")),
            with_gid(row(4, Some("C"), Some(1)), Some("")),
            row(5, Some("Below"), Some(2)),
        ];
        assert_eq!(
            reasons(&resolve_keywords(&rows, Some(1))),
            vec![
                "keyword 2: missing global id",
                "keyword 3: missing global id",
                "keyword 4: missing global id",
                "keyword 5: parent keyword 2 is skipped",
            ]
        );
    }

    #[test]
    fn duplicate_global_id_follows_id_order_not_row_order() {
        // Row 3 comes first in the slice but row 2 has the lower id.
        let rows = [
            with_gid(row(3, Some("B"), Some(1)), Some("same")),
            with_gid(row(2, Some("A"), Some(1)), Some("same")),
        ];
        let resolved = resolve_keywords(&rows, Some(1));
        assert_eq!(reasons(&resolved), vec!["keyword 3: duplicate global id"]);
        assert_eq!(
            resolved[1].1.as_ref().unwrap().global_id.as_deref(),
            Some("same")
        );
    }

    #[test]
    fn skipped_keyword_still_owns_its_global_id() {
        let rows = [
            with_gid(row(2, Some(""), Some(1)), Some("same")),
            with_gid(row(3, Some("B"), Some(1)), Some("same")),
        ];
        assert_eq!(
            reasons(&resolve_keywords(&rows, Some(1))),
            vec!["keyword 2: name is empty", "keyword 3: duplicate global id"]
        );
    }

    #[test]
    fn keyword_check_order() {
        // Parent reason before global id, global id before the name.
        let rows = [
            with_gid(row(2, Some("A"), Some(99)), None),
            with_gid(row(3, Some("a|b"), Some(1)), None),
        ];
        assert_eq!(
            reasons(&resolve_keywords(&rows, Some(1))),
            vec![
                "keyword 2: parent keyword 99 is skipped",
                "keyword 3: missing global id",
            ]
        );
    }

    #[test]
    fn root_global_id_is_ignored() {
        let rows = [
            with_gid(row(1, None, None), Some("same")),
            with_gid(row(2, Some("A"), Some(1)), Some("same")),
        ];
        let resolved = resolve_keywords(&rows, Some(1));
        assert!(resolved[0].1.is_ok());
    }

    #[test]
    fn null_and_whitespace_names_are_empty() {
        let rows = [row(2, None, Some(1)), row(3, Some("  "), Some(1))];
        let resolved = resolve_keywords(&rows, Some(1));
        assert_eq!(
            reasons(&resolved),
            vec!["keyword 2: name is empty", "keyword 3: name is empty"]
        );
    }

    #[test]
    fn missing_parent_skips_keyword_and_subtree() {
        let rows = [row(2, Some("A"), Some(99)), row(3, Some("B"), Some(2))];
        let resolved = resolve_keywords(&rows, Some(1));
        assert_eq!(
            reasons(&resolved),
            vec![
                "keyword 2: parent keyword 99 is skipped",
                "keyword 3: parent keyword 2 is skipped",
            ]
        );
    }

    #[test]
    fn cycles_are_skipped_without_looping() {
        let rows = [
            row(2, Some("A"), Some(3)),
            row(3, Some("B"), Some(2)),
            row(4, Some("Self"), Some(4)),
            row(5, Some("Below"), Some(2)),
        ];
        let resolved = resolve_keywords(&rows, Some(1));
        assert_eq!(
            reasons(&resolved),
            vec![
                "keyword 2: parent keyword 3 is skipped",
                "keyword 3: parent keyword 2 is skipped",
                "keyword 4: parent keyword 4 is skipped",
                "keyword 5: parent keyword 2 is skipped",
            ]
        );
    }

    #[test]
    fn every_non_root_row_is_resolved_or_skipped() {
        // The defensive `KeywordUnresolved` skips in `resolve_keywords`
        // exist so a broken internal invariant is reported instead of
        // panicking. This pins the invariant they guard: every non-root row
        // produces exactly one entry (Ok or Err), in row order, so the
        // defensive branch is not reached by normal input. A missing parent
        // and a two-node cycle are both handled as skips, not panics.
        let rows = [
            row(1, None, None),
            row(2, Some("A"), Some(1)),
            row(3, Some("B"), Some(2)),
            row(4, Some("C"), Some(9)),
            row(5, Some("D"), Some(6)),
            row(6, Some("E"), Some(5)),
        ];
        let resolved = resolve_keywords(&rows, Some(1));
        assert_eq!(resolved.len(), rows.len() - 1);
        assert_eq!(
            resolved.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
            vec![2, 3, 4, 5, 6]
        );
        for (_, r) in &resolved {
            assert!(r.is_ok() || r.is_err(), "no entry is absent");
        }
        assert!(
            reasons(&resolved)
                .iter()
                .all(|r| !r.contains("could not resolve") && !r.contains("was not resolved"))
        );
    }

    #[test]
    fn deep_chain_is_resolved_iteratively() {
        let mut rows = vec![row(1, None, None)];
        for id in 2..=10_000 {
            rows.push(row(id, Some("k"), Some(id - 1)));
        }
        let resolved = resolve_keywords(&rows, Some(1));
        let last = resolved.last().unwrap().1.as_ref().unwrap();
        assert_eq!(last.depth(), 9_999);
    }

    #[test]
    fn reasons_never_contain_names() {
        let rows = [
            row(2, Some("Secret|Name"), Some(1)),
            row(3, Some("Hidden"), Some(2)),
            row(4, Some("Lost"), Some(77)),
        ];
        for reason in reasons(&resolve_keywords(&rows, Some(1))) {
            for name in ["Secret", "Hidden", "Lost"] {
                assert!(!reason.contains(name), "{reason}");
            }
        }
    }

    fn codes(report: &Report) -> Vec<SkipCode> {
        report.skipped_items().iter().map(|s| s.code).collect()
    }

    #[test]
    fn keyword_skips_have_their_codes() {
        let rows = [
            row(1, None, None),
            with_gid(row(2, Some("A"), Some(1)), None),
            with_gid(row(3, Some("B"), Some(1)), Some("g4")),
            row(4, Some("C"), Some(1)),
            row(5, Some(""), Some(1)),
            row(6, Some("a|b"), Some(1)),
            row(7, Some("D"), Some(2)),
        ];
        let got: Vec<SkipCode> = resolve_keywords(&rows, Some(1))
            .into_iter()
            .filter_map(|(_, r)| r.err().map(|e| e.code))
            .collect();
        assert_eq!(
            got,
            [
                SkipCode::MissingGlobalId,
                SkipCode::DuplicateGlobalId,
                SkipCode::NameEmpty,
                SkipCode::NameHasBar,
                SkipCode::ParentSkipped,
            ]
        );
    }

    fn assignment(id: i64, image: i64, tag: i64) -> AssignmentRow {
        AssignmentRow {
            id,
            image: Some(image),
            tag: Some(tag),
        }
    }

    #[test]
    fn assignments_follow_row_order_and_skip_with_reasons() {
        let photos = HashSet::from([10, 11]);
        let keywords = HashMap::from([(2, kw(&["A"])), (3, kw(&["B"]))]);
        let rows = [
            assignment(1, 10, 3),
            assignment(2, 10, 2),
            assignment(3, 11, 1),  // root keyword
            assignment(4, 11, 50), // unknown keyword
            assignment(5, 12, 2),  // unknown photo
            assignment(6, 12, 50), // both: photo reason wins
        ];
        let mut report = Report::new();
        let out = assign(&rows, &photos, &keywords, &mut report);
        assert_eq!(out.len(), 1);
        assert_eq!(out[&10], vec![kw(&["B"]), kw(&["A"])]);
        assert_eq!(report.read_count(), 6);
        assert_eq!(report.converted_count(), 0);
        let reasons: Vec<_> = report
            .skipped_items()
            .iter()
            .map(|s| s.reason.as_str())
            .collect();
        assert_eq!(
            codes(&report),
            [
                SkipCode::AssignmentKeywordSkipped,
                SkipCode::AssignmentKeywordSkipped,
                SkipCode::AssignmentPhotoSkipped,
                SkipCode::AssignmentPhotoSkipped,
            ]
        );
        assert_eq!(
            reasons,
            vec![
                "assignment 3: keyword 1 is skipped",
                "assignment 4: keyword 50 is skipped",
                "assignment 5: photo 12 is skipped or unknown",
                "assignment 6: photo 12 is skipped or unknown",
            ]
        );
    }

    #[test]
    fn null_ids_in_assignments_are_skipped() {
        let rows = [AssignmentRow {
            id: 1,
            image: None,
            tag: None,
        }];
        let mut report = Report::new();
        let out = assign(&rows, &HashSet::new(), &HashMap::new(), &mut report);
        assert!(out.is_empty());
        assert_eq!(
            report.skipped_items()[0].reason,
            "assignment 1: photo NULL is skipped or unknown"
        );
    }

    fn synonym(id: i64, keyword: Option<i64>, name: Option<&str>) -> SynonymRow {
        SynonymRow {
            id,
            keyword,
            name: name.map(str::to_owned),
        }
    }

    #[test]
    fn synonyms_are_grouped_and_skipped_in_order() {
        let keyword_rows = [
            row(1, None, None),
            row(2, Some("A"), Some(1)),
            row(3, Some(""), Some(1)),
        ];
        let resolved = resolve_keywords(&keyword_rows, Some(1));
        let known: HashSet<i64> = keyword_rows.iter().map(|r| r.id).collect();
        let rows = [
            synonym(1, Some(2), Some("Second")),
            synonym(2, Some(2), Some("First")),
            synonym(3, Some(1), Some("Root")), // hidden root: skipped
            synonym(4, Some(3), None),         // skipped keyword wins over empty
            synonym(5, Some(50), Some("")),    // unknown wins over empty
            synonym(6, None, Some("Orphan")),  // NULL keyword is unknown
            synonym(7, Some(2), Some(" \t")),  // whitespace-only
            synonym(8, Some(2), None),         // NULL text
        ];
        let mut report = Report::new();
        let out = group_synonyms(&rows, &known, &resolved, &mut report);
        assert_eq!(out.len(), 1);
        assert_eq!(out[&2], vec!["Second".to_string(), "First".to_string()]);
        assert_eq!(report.read_count(), 8);
        assert_eq!(report.converted_count(), 0);
        let reasons: Vec<_> = report
            .skipped_items()
            .iter()
            .map(|s| s.reason.as_str())
            .collect();
        assert_eq!(
            codes(&report),
            [
                SkipCode::KeywordSkipped,
                SkipCode::KeywordSkipped,
                SkipCode::KeywordUnknown,
                SkipCode::KeywordUnknown,
                SkipCode::NameEmpty,
                SkipCode::NameEmpty,
            ]
        );
        assert_eq!(
            reasons,
            vec![
                "synonym 3: keyword 1 is skipped",
                "synonym 4: keyword 3 is skipped",
                "synonym 5: keyword 50 is unknown",
                "synonym 6: keyword NULL is unknown",
                "synonym 7: name is empty",
                "synonym 8: name is empty",
            ]
        );
        for reason in reasons {
            for text in ["Second", "First", "Root", "Orphan"] {
                assert!(!reason.contains(text), "{reason}");
            }
        }
    }

    #[test]
    fn flag_values() {
        assert!(!value_as_flag(&Value::Integer(0)));
        assert!(value_as_flag(&Value::Integer(1)));
        assert!(value_as_flag(&Value::Integer(-1)));
        assert!(value_as_flag(&Value::Integer(7)));
        assert!(!value_as_flag(&Value::Real(0.0)));
        assert!(value_as_flag(&Value::Real(0.5)));
        assert!(!value_as_flag(&Value::Text(" 0 ".into())));
        assert!(value_as_flag(&Value::Text("2".into())));
        assert!(value_as_flag(&Value::Text("x".into())));
        assert!(value_as_flag(&Value::Null));
    }

    #[test]
    fn type_values() {
        assert_eq!(value_as_type(&Value::Null), KeywordType::Plain);
        assert_eq!(
            value_as_type(&Value::Text("person".into())),
            KeywordType::Person
        );
        for other in [
            Value::Text("Person".into()),
            Value::Text(" person".into()),
            Value::Text(String::new()),
            Value::Integer(1),
        ] {
            assert_eq!(value_as_type(&other), KeywordType::Unknown);
        }
    }

    #[test]
    fn attributes_are_applied() {
        let mut k = kw(&["A"]);
        Attributes {
            include_on_export: false,
            export_containing: true,
            export_synonyms: false,
            kind: KeywordType::Person,
        }
        .apply(&mut k);
        assert_eq!(
            (
                k.include_on_export,
                k.export_containing,
                k.export_synonyms,
                k.person
            ),
            (false, true, false, true)
        );
        Attributes {
            kind: KeywordType::Unknown,
            ..Attributes::default()
        }
        .apply(&mut k);
        assert!(!k.person && k.include_on_export && k.export_synonyms);
    }

    #[test]
    fn root_id_values() {
        assert_eq!(value_as_id(&Value::Integer(7)), Some(7));
        assert_eq!(value_as_id(&Value::Text(" 7 ".into())), Some(7));
        assert_eq!(value_as_id(&Value::Real(7.0)), Some(7));
        assert_eq!(value_as_id(&Value::Real(7.5)), None);
        assert_eq!(value_as_id(&Value::Text("x".into())), None);
        assert_eq!(value_as_id(&Value::Null), None);
    }
}
