//! Reads the current state of a LightCraft library, read-only.
//!
//! Only `catalog.query`, `photo.inspect`, `albums.list` and `keyword.list`
//! are sent. Photos the mapping file does not know are counted, never
//! stored. An empty text becomes `None`, as the mapping file never stores
//! `""`. Error texts never contain a value from a reply.

use crate::albums::parse_album_list;
use crate::apply::{QUERY_PAGE, id_text, parse_keyword_list, parse_query_page, same_id};
use crate::control::{ControlClient, Outcome, WriteError};
use crate::state::State;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// The fields of one photo as LightCraft has them now.
///
/// `flag` is LightCraft's spelling of the pick/reject flag: `none`, `pick`
/// or `reject`. `rating` is a whole number from 0 to 5. `label` is a colour
/// name (`red`, `yellow`, `green`, `blue`, `purple`) or `None`. Text fields
/// are `None` when LightCraft has the empty string, since the mapping file
/// never stores `""`.
#[derive(Debug, Clone, PartialEq)]
pub struct LivePhoto {
    pub rating: u8,
    pub flag: String,
    pub label: Option<String>,
    pub title: Option<String>,
    /// Keyword path texts, sorted.
    pub keywords: Vec<String>,
    pub caption: Option<String>,
    pub copyright: Option<String>,
    pub creator: Option<String>,
}

/// One mapped album or album folder as LightCraft has it now.
///
/// `members` holds the global ids of the photos LightCraft lists as members
/// that the mapping file also knows; members the mapping file does not know
/// are dropped. Album folders have no members.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveAlbum {
    pub name: String,
    /// Members as global ids; members the mapping file does not know are
    /// dropped.
    pub members: BTreeSet<String>,
}

/// A snapshot of the library, limited to what the mapping file knows.
///
/// Built by [`read_live`], which sends only the read commands `catalog.query`,
/// `photo.inspect`, `albums.list` and `keyword.list`. A read error (a rejected
/// command, a lost connection or an unexpected reply) is returned as a
/// [`WriteError`] and aborts the whole run: live state is read to decide what
/// to reconcile, so a partial picture is never acted on.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LiveState {
    /// Mapped photos LightCraft still has, by global id.
    pub photos: BTreeMap<String, LivePhoto>,
    /// Mapped photos LightCraft no longer lists.
    pub missing_photos: BTreeSet<String>,
    /// Photos LightCraft lists that the mapping file does not know.
    pub foreign_photos: usize,
    /// Mapped collections still present, by Lightroom collection id.
    pub albums: BTreeMap<i64, LiveAlbum>,
    /// Mapped collections that `albums.list` no longer shows.
    pub missing_albums: BTreeSet<i64>,
    /// All keyword path texts of the library.
    pub keywords: BTreeSet<String>,
}

/// Reads the library state for the photos and collections of `state`.
pub fn read_live(client: &mut ControlClient, state: &State) -> Result<LiveState, WriteError> {
    // LightCraft id (as text) -> global id.
    let by_id: HashMap<String, String> = state
        .photos()
        .filter_map(|(gid, entry)| id_text(&entry.lightcraft_id).map(|t| (t, gid.to_string())))
        .collect();

    let mut live = LiveState::default();
    let mut present: Vec<(String, Value, LivePhoto)> = Vec::new();
    let mut offset = 0;
    loop {
        let value = read(
            client,
            "catalog.query",
            json!({"filter": {}, "offset": offset, "limit": QUERY_PAGE}),
        )?;
        let (total, items) = parse_summary_page(&value)?;
        let count = items.len();
        for (id, photo) in items {
            match id_text(&id).and_then(|t| by_id.get(&t)) {
                Some(gid) => present.push((gid.clone(), id, photo)),
                None => live.foreign_photos += 1,
            }
        }
        offset += count;
        if count == 0 || count < QUERY_PAGE || offset >= total {
            break;
        }
    }

    for (gid, id, mut photo) in present {
        let value = read(client, "photo.inspect", json!({"id": id}))?;
        let (caption, copyright, creator) = parse_inspect(&value)?;
        photo.caption = caption;
        photo.copyright = copyright;
        photo.creator = creator;
        live.photos.insert(gid, photo);
    }
    live.missing_photos = state
        .photos()
        .map(|(gid, _)| gid.to_string())
        .filter(|gid| !live.photos.contains_key(gid))
        .collect();

    let nodes = parse_album_list(&read(client, "albums.list", json!({}))?)?;
    live.keywords = parse_keyword_list(&read(client, "keyword.list", json!({}))?)?;

    for (lightroom_id, entry) in state.collections() {
        let Some(node) = nodes.iter().find(|n| same_id(&n.id, &entry.lightcraft_id)) else {
            live.missing_albums.insert(lightroom_id);
            continue;
        };
        let mut members = BTreeSet::new();
        if !node.folder {
            for id in album_members(client, &node.id)? {
                if let Some(gid) = id_text(&id).and_then(|t| by_id.get(&t)) {
                    members.insert(gid.clone());
                }
            }
        }
        live.albums.insert(
            lightroom_id,
            LiveAlbum {
                name: node.name.clone(),
                members,
            },
        );
    }
    Ok(live)
}

/// Sends a read command; an answer like a writing command is a protocol
/// error.
fn read(client: &mut ControlClient, command: &str, params: Value) -> Result<Value, WriteError> {
    match client.execute(command, params)? {
        Outcome::Applied(value) => Ok(value),
        Outcome::AppliedNotSaved(_) => Err(WriteError::Protocol(format!(
            "{command} answered like a writing command"
        ))),
    }
}

/// The photo ids of an album, read page by page.
fn album_members(client: &mut ControlClient, album: &Value) -> Result<Vec<Value>, WriteError> {
    let mut held: Vec<Value> = Vec::new();
    loop {
        let value = read(
            client,
            "catalog.query",
            json!({"filter": {"album": album}, "offset": held.len(), "limit": QUERY_PAGE}),
        )?;
        let page = parse_query_page(&value)?;
        let done = page.ids.is_empty()
            || page.ids.len() < QUERY_PAGE
            || held.len() + page.ids.len() >= page.total;
        held.extend(page.ids);
        if done {
            return Ok(held);
        }
    }
}

fn summary_error() -> WriteError {
    WriteError::Protocol("catalog.query answered with an unknown photo summary".into())
}

fn inspect_error() -> WriteError {
    WriteError::Protocol("photo.inspect answered in an unknown shape".into())
}

/// Reads `{total, photos: [{id, rating, flag, label, title, keywords}]}`.
/// Caption, copyright and creator are left `None`.
fn parse_summary_page(value: &Value) -> Result<(usize, Vec<(Value, LivePhoto)>), WriteError> {
    let total = value["total"].as_u64().ok_or_else(summary_error)?;
    // Refuse a count that does not fit `usize` instead of truncating it.
    let total = usize::try_from(total).map_err(|_| summary_error())?;
    let items = value["photos"].as_array().ok_or_else(summary_error)?;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let id = match &item["id"] {
            v @ Value::Number(_) => v.clone(),
            Value::String(s) if !s.is_empty() => Value::String(s.clone()),
            _ => return Err(summary_error()),
        };
        let rating = item["rating"]
            .as_u64()
            .filter(|r| *r <= 5)
            .ok_or_else(summary_error)? as u8;
        let flag = item["flag"].as_str().ok_or_else(summary_error)?.to_string();
        let label = optional_text(&item["label"]).ok_or_else(summary_error)?;
        let title = optional_text(&item["title"]).ok_or_else(summary_error)?;
        let mut keywords = match &item["keywords"] {
            Value::Null => Vec::new(),
            Value::Array(list) => list
                .iter()
                .map(|k| k.as_str().map(str::to_string).ok_or_else(summary_error))
                .collect::<Result<Vec<_>, _>>()?,
            _ => return Err(summary_error()),
        };
        keywords.sort();
        out.push((
            id,
            LivePhoto {
                rating,
                flag,
                label,
                title,
                keywords,
                caption: None,
                copyright: None,
                creator: None,
            },
        ));
    }
    Ok((total, out))
}

type Credits = (Option<String>, Option<String>, Option<String>);

/// Caption, copyright and creator of a `photo.inspect` answer, from `meta`
/// or from the top level.
fn parse_inspect(value: &Value) -> Result<Credits, WriteError> {
    if !value.is_object() {
        return Err(inspect_error());
    }
    let source = match value.get("meta") {
        Some(meta @ Value::Object(_)) => meta,
        None | Some(Value::Null) => value,
        Some(_) => return Err(inspect_error()),
    };
    let field = |name: &str| optional_text(&source[name]).ok_or_else(inspect_error);
    Ok((field("caption")?, field("copyright")?, field("creator")?))
}

/// `null`, a missing field or `""` is `None`; another text is `Some`; any
/// other value is unusable (outer `None`).
fn optional_text(value: &Value) -> Option<Option<String>> {
    match value {
        Value::Null => Some(None),
        Value::String(s) if s.is_empty() => Some(None),
        Value::String(s) => Some(Some(s.clone())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_page_parses_values_and_empty_texts() {
        let value = json!({"total": 2, "photos": [
            {"id": 1, "rating": 3, "flag": "pick", "label": "red", "title": "T",
             "keywords": ["b", "a|c"]},
            {"id": "x2", "rating": 0, "flag": "none", "label": null, "title": ""},
        ]});
        let (total, items) = parse_summary_page(&value).unwrap();
        assert_eq!(total, 2);
        assert_eq!(items[0].0, json!(1));
        assert_eq!(items[0].1.rating, 3);
        assert_eq!(items[0].1.label.as_deref(), Some("red"));
        assert_eq!(items[0].1.title.as_deref(), Some("T"));
        assert_eq!(items[0].1.keywords, ["a|c", "b"]);
        assert_eq!(items[1].0, json!("x2"));
        assert_eq!(items[1].1.title, None);
        assert_eq!(items[1].1.label, None);
        assert!(items[1].1.keywords.is_empty());
    }

    #[test]
    fn summary_page_total_is_never_truncated() {
        let v = json!({"total": 2, "photos": []});
        assert_eq!(parse_summary_page(&v).unwrap().0, 2);
        // A count that does not fit `usize` must be refused, not truncated.
        if usize::BITS < 64 {
            let v = json!({"total": u64::MAX, "photos": []});
            assert!(matches!(
                parse_summary_page(&v),
                Err(WriteError::Protocol(_))
            ));
        }
    }

    #[test]
    fn bad_summaries_are_protocol_errors_without_values() {
        for photo in [
            json!({"id": 1, "rating": "Secret", "flag": "none"}),
            json!({"id": 1, "rating": 9, "flag": "none"}),
            json!({"id": 1, "rating": 1}),
            json!({"id": 1, "rating": 1, "flag": "none", "title": 7}),
            json!({"id": 1, "rating": 1, "flag": "none", "keywords": "Secret"}),
            json!({"id": null, "rating": 1, "flag": "none"}),
        ] {
            let err = parse_summary_page(&json!({"total": 1, "photos": [photo]})).unwrap_err();
            assert!(matches!(err, WriteError::Protocol(_)));
            assert!(!format!("{err} {err:?}").contains("Secret"));
        }
        assert!(parse_summary_page(&json!({"photos": []})).is_err());
        assert!(parse_summary_page(&json!([])).is_err());
    }

    #[test]
    fn inspect_reads_meta_or_top_level() {
        let v = json!({"id": 1, "meta": {"caption": "C", "copyright": "", "creator": null}});
        assert_eq!(parse_inspect(&v).unwrap(), (Some("C".into()), None, None));
        let v = json!({"id": 1, "creator": "Me"});
        assert_eq!(parse_inspect(&v).unwrap(), (None, None, Some("Me".into())));
    }

    #[test]
    fn bad_inspect_is_a_protocol_error_without_values() {
        for v in [
            json!("Secret"),
            json!({"meta": "Secret"}),
            json!({"meta": {"caption": ["Secret"]}}),
        ] {
            let err = parse_inspect(&v).unwrap_err();
            assert!(matches!(err, WriteError::Protocol(_)));
            assert!(!format!("{err} {err:?}").contains("Secret"));
        }
    }
}
