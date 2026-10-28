//! Reading folder stacks (`AgLibraryFolderStack`) and their members
//! (`AgLibraryFolderStackImage`).
//!
//! A stack keeps the members that are converted photos and not already in
//! an earlier stack (smaller id). With fewer than two such members it is
//! skipped. Members are ordered top first: the photo at `position` 1, then
//! by `position`, then by row id.
//!
//! Both tables are optional: without them the catalog has no stacks.
//! Reasons name ids only.

use crate::catalog::{Catalog, ReadError};
use crate::collections::{as_id, is_nonzero};
use model::{Photo, Report, Stack};
use rusqlite::types::ValueRef;
use std::collections::{BTreeMap, HashMap, HashSet};

/// Stacks and their members as read from a catalog.
#[derive(Debug, Clone, Default)]
pub struct StackData {
    /// Converted stacks, ascending by `lightroom_id`.
    pub stacks: Vec<Stack>,
    /// One entry per `AgLibraryFolderStack` row: converted or skipped.
    pub report: Report,
    /// One entry per `AgLibraryFolderStackImage` row: converted (a member of
    /// a converted stack) or skipped.
    pub members: Report,
    /// Converted stacks that are collapsed in Lightroom.
    pub collapsed: usize,
    /// Converted stacks without a kept member at position 1; their first
    /// member by position is the top.
    pub no_top: usize,
}

/// One `AgLibraryFolderStackImage` row.
#[derive(Debug, Clone, Copy, PartialEq)]
struct MemberRow {
    row: i64,
    stack: Option<i64>,
    image: Option<i64>,
    /// True if `position` is exactly 1.
    at_one: bool,
}

impl Catalog {
    /// Reads all folder stacks and their members.
    ///
    /// `photos` are the converted photos: member rows naming any other
    /// photo are skipped. Both reports are complete: every row is counted
    /// as read and then as converted or skipped.
    pub fn stack_data(&self, photos: &[Photo]) -> Result<StackData, ReadError> {
        if !self.has_columns("AgLibraryFolderStack", &["id_local", "collapsed"])? {
            return Ok(StackData::default());
        }
        let stacks = self.stack_rows()?;
        let members = if self.has_columns(
            "AgLibraryFolderStackImage",
            &["id_local", "image", "position", "stack"],
        )? {
            self.stack_member_rows()?
        } else {
            Vec::new()
        };
        let converted: HashSet<i64> = photos.iter().map(|p| p.lightroom_id).collect();
        Ok(build(&stacks, &members, &converted))
    }

    /// Whether `table` exists with all `columns`.
    fn has_columns(&self, table: &str, columns: &[&str]) -> Result<bool, ReadError> {
        let mut stmt = self
            .conn
            .prepare("SELECT name FROM pragma_table_info(?1)")
            .map_err(ReadError::Query)?;
        let present = stmt
            .query_map([table], |row| row.get::<_, String>(0))
            .map_err(ReadError::Query)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(ReadError::Query)?;
        Ok(columns.iter().all(|c| present.iter().any(|p| p == c)))
    }

    /// `id_local -> collapsed` of every stack row.
    fn stack_rows(&self) -> Result<BTreeMap<i64, bool>, ReadError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id_local, collapsed FROM AgLibraryFolderStack ORDER BY id_local")
            .map_err(ReadError::Query)?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, is_nonzero(row.get_ref(1)?)))
            })
            .map_err(ReadError::Query)?;
        rows.collect::<Result<_, _>>().map_err(ReadError::Query)
    }

    /// Every member row, ordered by stack, position 1 first, then by
    /// position (NULL last), then by row id.
    fn stack_member_rows(&self) -> Result<Vec<MemberRow>, ReadError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id_local, stack, image, position = 1 FROM AgLibraryFolderStackImage \
                 ORDER BY stack, position = 1 DESC, position IS NULL, position, id_local",
            )
            .map_err(ReadError::Query)?;
        let rows = stmt
            .query_map([], |row| {
                Ok(MemberRow {
                    row: row.get(0)?,
                    stack: as_id(row.get_ref(1)?),
                    image: as_id(row.get_ref(2)?),
                    at_one: matches!(row.get_ref(3)?, ValueRef::Integer(1)),
                })
            })
            .map_err(ReadError::Query)?;
        rows.collect::<Result<_, _>>().map_err(ReadError::Query)
    }
}

/// Builds the stacks from their rows. `members` must be in the order of
/// [`Catalog::stack_member_rows`].
fn build(
    stacks: &BTreeMap<i64, bool>,
    members: &[MemberRow],
    converted: &HashSet<i64>,
) -> StackData {
    let mut by_stack: HashMap<i64, Vec<MemberRow>> = HashMap::new();
    let mut data = StackData::default();
    for m in members {
        match m.stack {
            Some(s) if stacks.contains_key(&s) => by_stack.entry(s).or_default().push(*m),
            _ => {
                data.members.read();
                data.members.skipped(match (m.image, m.stack) {
                    (Some(p), Some(s)) => format!("photo {p}: stack {s} is unknown"),
                    (Some(p), None) => format!("photo {p}: stack id is not valid"),
                    (None, _) => format!("stack member row {}: ids are not valid", m.row),
                });
            }
        }
    }
    // Photo id -> the converted stack that holds it.
    let mut taken: HashMap<i64, i64> = HashMap::new();
    for (&id, &collapsed) in stacks {
        data.report.read();
        let rows = by_stack.remove(&id).unwrap_or_default();
        let mut kept: Vec<(i64, bool)> = Vec::new();
        let mut skipped: Vec<String> = Vec::new();
        // Reasons of rows that would be kept if the stack were converted.
        let mut pending: Vec<i64> = Vec::new();
        for m in &rows {
            let reason = match m.image {
                None => Some(format!(
                    "stack member row {}: photo id is not valid (stack {id})",
                    m.row
                )),
                Some(p) if !converted.contains(&p) => Some(format!(
                    "photo {p}: skipped or unknown photo, not added to stack {id}"
                )),
                Some(p) => {
                    if let Some(first) = taken.get(&p) {
                        Some(format!("photo {p}: already in stack {first}"))
                    } else if kept.iter().any(|&(k, _)| k == p) {
                        Some(format!("photo {p}: duplicate member of stack {id}"))
                    } else {
                        None
                    }
                }
            };
            match (reason, m.image) {
                (Some(r), _) => skipped.push(r),
                (None, Some(p)) => {
                    kept.push((p, m.at_one));
                    pending.push(p);
                }
                (None, None) => unreachable!("rows without a photo id have a reason"),
            }
        }
        for _ in &rows {
            data.members.read();
        }
        if kept.len() < 2 {
            data.report
                .skipped(format!("stack {id}: fewer than two converted photos"));
            for p in pending {
                data.members
                    .skipped(format!("photo {p}: stack {id} is skipped"));
            }
        } else {
            for &(p, _) in &kept {
                taken.insert(p, id);
                data.members.converted();
            }
            data.report.converted();
            if collapsed {
                data.collapsed += 1;
            }
            if !kept[0].1 {
                data.no_top += 1;
            }
            data.stacks.push(Stack {
                lightroom_id: id,
                members: kept.into_iter().map(|(p, _)| p).collect(),
                collapsed,
            });
        }
        for r in skipped {
            data.members.skipped(r);
        }
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(row: i64, stack: i64, image: i64, at_one: bool) -> MemberRow {
        MemberRow {
            row,
            stack: Some(stack),
            image: Some(image),
            at_one,
        }
    }

    #[test]
    fn small_stack_is_skipped_with_its_rows() {
        let stacks = [(1, false)].into_iter().collect();
        let rows = [m(1, 1, 10, true), m(2, 1, 11, false)];
        let d = build(&stacks, &rows, &[10].into_iter().collect());
        assert!(d.stacks.is_empty());
        assert_eq!(d.report.skipped_count(), 1);
        assert_eq!(d.members.read_count(), 2);
        assert_eq!(d.members.skipped_count(), 2);
    }

    #[test]
    fn orphan_and_duplicate_rows_are_skipped() {
        let stacks = [(1, true)].into_iter().collect();
        let rows = [
            m(1, 1, 10, true),
            m(2, 1, 11, false),
            m(3, 1, 11, false),
            m(4, 9, 12, true),
        ];
        let d = build(&stacks, &rows, &[10, 11, 12].into_iter().collect());
        assert_eq!(d.stacks[0].members, [10, 11]);
        assert_eq!(d.collapsed, 1);
        assert_eq!(d.no_top, 0);
        assert_eq!(d.members.read_count(), 4);
        assert_eq!(d.members.skipped_count(), 2);
        let reasons: Vec<_> = d
            .members
            .skipped_items()
            .iter()
            .map(|s| s.reason.clone())
            .collect();
        assert!(reasons.contains(&"photo 12: stack 9 is unknown".to_string()));
        assert!(reasons.contains(&"photo 11: duplicate member of stack 1".to_string()));
    }

    #[test]
    fn photo_freed_by_a_skipped_stack_can_join_a_later_one() {
        let stacks = [(1, false), (2, false)].into_iter().collect();
        let rows = [m(1, 1, 10, true), m(2, 2, 10, true), m(3, 2, 11, false)];
        let d = build(&stacks, &rows, &[10, 11].into_iter().collect());
        assert_eq!(d.stacks.len(), 1);
        assert_eq!(d.stacks[0].members, [10, 11]);
    }
}
