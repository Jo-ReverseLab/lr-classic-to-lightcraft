//! The mapping file: what lr2lc has already written into a LightCraft
//! library.
//!
//! The file lives next to the library folder as `<folder name>.lr2lc.json`.
//! It maps each photo's Lightroom global id to its LightCraft id and keeps
//! the field values that were last imported, so a later run only sends what
//! changed in Lightroom since then. Keywords are kept under their path text
//! (`a|b|c`) together with the attributes they were created with.
//! Collections are kept under their Lightroom id with the LightCraft id of
//! the album or album folder, the name and parent they were created with,
//! and the members lr2lc added. Smart collections are kept the same way as
//! smart albums, with the translated rules that were sent and no members.

use model::{CollectionKind, ColorLabel, Flag, Keyword, Photo};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The only file format version this code reads and writes.
pub const VERSION: u64 = 1;

/// Suffix appended to the library folder name.
const SUFFIX: &str = ".lr2lc.json";

/// Errors while reading or writing the mapping file.
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    /// The file could not be read or written.
    #[error("cannot access the mapping file {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The file is not valid JSON or does not have the expected shape.
    #[error("the mapping file {} is damaged: {source}", path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    /// The file has no `version` or one this program does not know.
    #[error(
        "the mapping file {} has an unsupported version ({found}); expected {VERSION}",
        path.display()
    )]
    Version { path: PathBuf, found: String },
}

/// The last imported value of each field of a photo.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhotoFields {
    /// Stars, 0 to 5.
    pub rating: u8,
    /// `none`, `pick` or `reject`.
    pub flag: String,
    /// `red`, `yellow`, `green`, `blue`, `purple`, or `None` without label.
    pub label: Option<String>,
    /// Title; never an empty string.
    pub title: Option<String>,
    /// Caption; never an empty string.
    pub caption: Option<String>,
    /// Copyright notice; never an empty string.
    pub copyright: Option<String>,
    /// Creator; never an empty string.
    pub creator: Option<String>,
    /// Keyword path texts (`a|b|c`), sorted and without duplicates.
    pub keywords: Vec<String>,
}

impl PhotoFields {
    /// The fields of a photo as LightCraft creates it on import: no stars,
    /// no flag, no label, no texts, no keywords.
    pub fn imported_default() -> PhotoFields {
        PhotoFields {
            flag: flag_text(Flag::None).to_string(),
            ..PhotoFields::default()
        }
    }
}

/// One photo written to the library.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhotoEntry {
    /// The photo's id in the Lightroom catalog (provenance).
    pub lightroom_id: i64,
    /// The photo's id in LightCraft, exactly as the app returned it.
    pub lightcraft_id: Value,
    /// The field values last written.
    pub fields: PhotoFields,
    /// The absolute path of the original as last imported. It is the base
    /// of the move comparison and is never replaced afterwards, so a moved
    /// file is reported again on every later run. Missing in older files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The master photo's global id if this entry is a virtual copy, or
    /// `None` for a master. A copy is created once with `photo.virtualCopy`
    /// from the master's LightCraft id and afterwards behaves like a normal
    /// photo: only its fields are updated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copy_of: Option<String>,
    /// Lowercase hex SHA-256 of the development-settings text last written,
    /// if any. Missing in files of older versions; `None` means the next run
    /// writes the settings once. It is recorded only after every develop
    /// command of the photo succeeded, never after a failed one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub develop_hash: Option<String>,
    /// lr2lc began writing development settings and no hash is recorded
    /// yet (a command failed or the run stopped). The next run writes them
    /// again even though LightCraft shows the photo as edited.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub develop_started: bool,
}

/// One keyword created in the library, with the attributes it was created
/// with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeywordEntry {
    /// Lightroom's global id of the keyword, if known (provenance).
    pub global_id: Option<String>,
    /// The keyword's id in the Lightroom catalog, if known. A renamed
    /// keyword is reported by this id, never by a name. Missing in older
    /// files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lightroom_id: Option<i64>,
    /// Synonyms in source order.
    pub synonyms: Vec<String>,
    /// Written into exported photos.
    pub include_on_export: bool,
    /// Parents exported with it.
    pub export_containing: bool,
    /// Synonyms exported with it.
    pub export_synonyms: bool,
    /// Names a person.
    pub person: bool,
}

impl KeywordEntry {
    /// The entry for a keyword.
    pub fn of(keyword: &Keyword) -> KeywordEntry {
        KeywordEntry {
            global_id: keyword.global_id.clone(),
            lightroom_id: keyword.lightroom_id,
            synonyms: keyword.synonyms.clone(),
            include_on_export: keyword.include_on_export,
            export_containing: keyword.export_containing,
            export_synonyms: keyword.export_synonyms,
            person: keyword.person,
        }
    }

    /// True if the keyword's synonyms and attributes equal the recorded
    /// ones. The global id is not compared.
    pub fn same_attributes(&self, keyword: &Keyword) -> bool {
        self.synonyms == keyword.synonyms
            && self.include_on_export == keyword.include_on_export
            && self.export_containing == keyword.export_containing
            && self.export_synonyms == keyword.export_synonyms
            && self.person == keyword.person
    }
}

/// What a collection became in LightCraft.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    /// An album (Lightroom: collection).
    Album,
    /// An album folder (Lightroom: collection set).
    Folder,
    /// A smart album (Lightroom: smart collection).
    Smart,
}

impl From<CollectionKind> for EntryKind {
    fn from(kind: CollectionKind) -> EntryKind {
        match kind {
            CollectionKind::Album => EntryKind::Album,
            CollectionKind::Folder => EntryKind::Folder,
        }
    }
}

/// One collection created in the library as an album or album folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollectionEntry {
    /// The album's id in LightCraft, exactly as the app returned it.
    pub lightcraft_id: Value,
    /// Album or album folder.
    pub kind: EntryKind,
    /// The name sent on creation (the app may have trimmed it).
    pub name: String,
    /// The Lightroom id of the parent at creation; `None` on top level.
    pub parent: Option<i64>,
    /// Global ids of the photos lr2lc added and did not remove, sorted and
    /// without duplicates. Always empty for a smart album.
    pub members: Vec<String>,
    /// The `rules` parameter sent with `album.createSmart`; only for a
    /// smart album. Missing in older files.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<Value>,
}

/// One stack grouped in the library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StackEntry {
    /// Global ids of the photos lr2lc grouped, the top first. Members that
    /// left the stack in Lightroom stay here (and in the LightCraft stack).
    pub members: Vec<String>,
}

/// The Quick Collection as written by lr2lc.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuickEntry {
    /// Global ids of the photos lr2lc added or found in it, sorted. Members
    /// removed in Lightroom stay here (and in the LightCraft album).
    pub members: BTreeSet<String>,
}

/// The content of the mapping file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    photos: BTreeMap<String, PhotoEntry>,
    keywords: BTreeMap<String, KeywordEntry>,
    collections: BTreeMap<i64, CollectionEntry>,
    stacks: BTreeMap<i64, StackEntry>,
    quick: QuickEntry,
}

/// The file layout; `version` is checked before this is parsed.
/// `collections`, `stacks` and `quick` came later and may be missing in older files.
#[derive(Serialize, Deserialize)]
struct StateFile {
    version: u64,
    photos: BTreeMap<String, PhotoEntry>,
    keywords: BTreeMap<String, KeywordEntry>,
    #[serde(default)]
    collections: BTreeMap<i64, CollectionEntry>,
    #[serde(default)]
    stacks: BTreeMap<i64, StackEntry>,
    #[serde(default)]
    quick: QuickEntry,
}

impl State {
    /// Reads the mapping file. A missing file gives the empty state; a file
    /// that cannot be parsed or has an unknown version is an error. The file
    /// is never changed.
    pub fn load(path: &Path) -> Result<State, StateError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(State::default()),
            Err(source) => {
                return Err(StateError::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        let parse_error = |source| StateError::Parse {
            path: path.to_path_buf(),
            source,
        };
        let value: Value = serde_json::from_str(&text).map_err(parse_error)?;
        match value.get("version") {
            Some(v) if v.as_u64() == Some(VERSION) => {}
            other => {
                return Err(StateError::Version {
                    path: path.to_path_buf(),
                    found: other.map_or_else(|| "missing".to_string(), Value::to_string),
                });
            }
        }
        let file: StateFile = serde_json::from_value(value).map_err(parse_error)?;
        Ok(State {
            photos: file.photos,
            keywords: file.keywords,
            collections: file.collections,
            stacks: file.stacks,
            quick: file.quick,
        })
    }

    /// Writes the mapping file atomically: the content goes into a
    /// temporary file in the same folder, which then replaces the target.
    pub fn save(&self, path: &Path) -> Result<(), StateError> {
        let file = StateFile {
            version: VERSION,
            photos: self.photos.clone(),
            keywords: self.keywords.clone(),
            collections: self.collections.clone(),
            stacks: self.stacks.clone(),
            quick: self.quick.clone(),
        };
        let mut text = serde_json::to_string_pretty(&file).map_err(|source| StateError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
        text.push('\n');
        let tmp = temporary_path(path);
        let io_error = |source| StateError::Io {
            path: path.to_path_buf(),
            source,
        };
        let written =
            write_and_sync(&tmp, text.as_bytes()).and_then(|()| std::fs::rename(&tmp, path));
        if let Err(e) = written {
            let _ = std::fs::remove_file(&tmp);
            return Err(io_error(e));
        }
        Ok(())
    }

    /// Records a photo as written with the given field values, replacing an
    /// earlier entry for the same global id. Used for masters and for the
    /// field refresh of an already recorded copy (the existing `copy_of` is
    /// kept).
    pub fn record(
        &mut self,
        global_id: impl Into<String>,
        lightroom_id: i64,
        lightcraft_id: Value,
        fields: PhotoFields,
    ) {
        let global_id = global_id.into();
        let (copy_of, path, develop_hash, develop_started) =
            self.photos
                .get(&global_id)
                .map_or((None, None, None, false), |e| {
                    (
                        e.copy_of.clone(),
                        e.path.clone(),
                        e.develop_hash.clone(),
                        e.develop_started,
                    )
                });
        self.photos.insert(
            global_id,
            PhotoEntry {
                lightroom_id,
                lightcraft_id,
                fields,
                path,
                copy_of,
                develop_hash,
                develop_started,
            },
        );
    }

    /// Remembers the absolute path of a photo's original if none is
    /// recorded yet. An already recorded path is never replaced: it is the
    /// base of the move comparison, so a later move is reported again.
    /// Returns whether the entry changed.
    pub fn remember_path(&mut self, global_id: &str, path: &Path) -> bool {
        let Some(text) = path.to_str() else {
            return false;
        };
        match self.photos.get_mut(global_id) {
            Some(entry) if entry.path.is_none() => {
                entry.path = Some(text.to_string());
                true
            }
            _ => false,
        }
    }

    /// Records the lowercase hex SHA-256 of the development-settings text
    /// that was written, replacing an earlier value. Returns whether the
    /// entry changed. Only called after every develop command of the photo
    /// succeeded, so an incomplete photo is written again by the next run.
    pub fn record_develop_hash(&mut self, global_id: &str, hash: Option<String>) -> bool {
        match self.photos.get_mut(global_id) {
            Some(entry) if entry.develop_hash != hash || entry.develop_started => {
                entry.develop_started &= hash.is_none();
                entry.develop_hash = hash;
                true
            }
            _ => false,
        }
    }

    /// Notes that lr2lc begins writing a photo's development settings.
    /// Returns whether the entry changed.
    pub fn mark_develop_started(&mut self, global_id: &str) -> bool {
        match self.photos.get_mut(global_id) {
            Some(entry) if !entry.develop_started => {
                entry.develop_started = true;
                true
            }
            _ => false,
        }
    }

    /// Records a virtual copy right after it was created with
    /// `photo.virtualCopy`, with the global id of its master so a later run
    /// never creates the copy again.
    pub fn record_copy(
        &mut self,
        global_id: impl Into<String>,
        lightroom_id: i64,
        lightcraft_id: Value,
        master_global_id: impl Into<String>,
        fields: PhotoFields,
    ) {
        self.photos.insert(
            global_id.into(),
            PhotoEntry {
                lightroom_id,
                lightcraft_id,
                fields,
                path: None,
                copy_of: Some(master_global_id.into()),
                develop_hash: None,
                develop_started: false,
            },
        );
    }

    /// Records a keyword as present in the library, with its current
    /// attributes.
    pub fn record_keyword(&mut self, keyword: &Keyword) {
        self.keywords
            .insert(keyword_text(keyword), KeywordEntry::of(keyword));
    }

    /// Records a collection as created, replacing an earlier entry for the
    /// same Lightroom id. `members` is sorted and freed of duplicates.
    pub fn record_collection(&mut self, lightroom_id: i64, mut entry: CollectionEntry) {
        entry.members.sort();
        entry.members.dedup();
        self.collections.insert(lightroom_id, entry);
    }

    /// The entry of a collection, by Lightroom id.
    pub fn collection(&self, lightroom_id: i64) -> Option<&CollectionEntry> {
        self.collections.get(&lightroom_id)
    }

    /// All recorded collections, ordered by Lightroom id.
    pub fn collections(&self) -> impl Iterator<Item = (i64, &CollectionEntry)> {
        self.collections.iter().map(|(k, v)| (*k, v))
    }

    /// Records a stack as grouped with `members` (global ids, top first),
    /// replacing an earlier entry for the same Lightroom id.
    pub fn record_stack(&mut self, lightroom_id: i64, members: Vec<String>) {
        self.stacks.insert(lightroom_id, StackEntry { members });
    }

    /// The Quick Collection members lr2lc recorded.
    pub fn quick(&self) -> &QuickEntry {
        &self.quick
    }

    /// Records photos (global ids) as members of the Quick Collection.
    pub fn record_quick(&mut self, members: impl IntoIterator<Item = String>) {
        self.quick.members.extend(members);
    }

    /// Forgets a photo lr2lc wrote that is gone from LightCraft: its entry
    /// and its recorded album, stack and Quick Collection memberships, so
    /// the next plan imports it and adds it to them again as a new photo.
    /// Returns whether it was recorded.
    pub fn forget_photo(&mut self, global_id: &str) -> bool {
        if self.photos.remove(global_id).is_none() {
            return false;
        }
        for entry in self.collections.values_mut() {
            entry.members.retain(|g| g != global_id);
        }
        for entry in self.stacks.values_mut() {
            entry.members.retain(|g| g != global_id);
        }
        self.quick.members.remove(global_id);
        true
    }

    /// The entry of a stack, by Lightroom id.
    pub fn stack(&self, lightroom_id: i64) -> Option<&StackEntry> {
        self.stacks.get(&lightroom_id)
    }

    /// The entry of a photo, by Lightroom global id.
    pub fn photo(&self, global_id: &str) -> Option<&PhotoEntry> {
        self.photos.get(global_id)
    }

    /// The entry of a keyword, by path text (`a|b|c`).
    pub fn keyword(&self, path: &str) -> Option<&KeywordEntry> {
        self.keywords.get(path)
    }

    /// All recorded photos, ordered by global id.
    pub fn photos(&self) -> impl Iterator<Item = (&str, &PhotoEntry)> {
        self.photos.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// All recorded keywords, ordered by path text.
    pub fn keywords(&self) -> impl Iterator<Item = (&str, &KeywordEntry)> {
        self.keywords.iter().map(|(k, v)| (k.as_str(), v))
    }
}

/// Where the mapping file of a library lives: next to the library folder,
/// named `<folder name>.lr2lc.json`.
pub fn state_path(library: &Path) -> PathBuf {
    match (library.parent(), library.file_name()) {
        (Some(parent), Some(name)) => {
            let mut file = name.to_os_string();
            file.push(SUFFIX);
            parent.join(file)
        }
        // No folder name (e.g. a root): append to the path as given.
        _ => {
            let mut file = library.as_os_str().to_os_string();
            file.push(SUFFIX);
            PathBuf::from(file)
        }
    }
}

/// The lowercase hex spelling of a SHA-256 hash, as the mapping file stores
/// it.
pub fn hex_hash(hash: &[u8; 32]) -> String {
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

/// The field values of a photo with its keywords, as the plan compares
/// them and as they are recorded after writing.
pub fn snapshot(photo: &Photo, keywords: &[Keyword]) -> PhotoFields {
    let mut texts: Vec<String> = keywords.iter().map(keyword_text).collect();
    texts.sort();
    texts.dedup();
    PhotoFields {
        rating: photo.rating.get(),
        flag: flag_text(photo.flag).to_string(),
        label: photo.label.map(|l| label_text(l).to_string()),
        title: non_empty(&photo.title),
        caption: non_empty(&photo.caption),
        copyright: non_empty(&photo.copyright),
        creator: non_empty(&photo.creator),
        keywords: texts,
    }
}

/// The path text of a keyword as LightCraft expects it: `a|b|c`.
pub fn keyword_text(keyword: &Keyword) -> String {
    keyword.path().join("|")
}

/// LightCraft's spelling of a flag.
pub fn flag_text(flag: Flag) -> &'static str {
    match flag {
        Flag::None => "none",
        Flag::Pick => "pick",
        Flag::Reject => "reject",
    }
}

/// LightCraft's spelling of a colour label.
pub fn label_text(label: ColorLabel) -> &'static str {
    match label {
        ColorLabel::Red => "red",
        ColorLabel::Yellow => "yellow",
        ColorLabel::Green => "green",
        ColorLabel::Blue => "blue",
        ColorLabel::Purple => "purple",
    }
}

fn non_empty(text: &Option<String>) -> Option<String> {
    text.as_ref().filter(|t| !t.is_empty()).cloned()
}

/// A temporary file next to `path`, unique per process.
fn temporary_path(path: &Path) -> PathBuf {
    let mut name: OsString = path.file_name().map(OsString::from).unwrap_or_default();
    name.push(format!(".tmp-{}", std::process::id()));
    path.with_file_name(name)
}

fn write_and_sync(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::Rating;
    use serde_json::json;

    fn kw(parts: &[&str]) -> Keyword {
        Keyword::new(parts.iter().map(|s| s.to_string()).collect()).unwrap()
    }

    #[test]
    fn snapshot_of_a_plain_photo_is_the_import_default() {
        let p = Photo::new("a.jpg", 1, Rating::default());
        assert_eq!(snapshot(&p, &[]), PhotoFields::imported_default());
    }

    #[test]
    fn snapshot_treats_empty_text_as_missing() {
        let p = Photo::new("a.jpg", 1, Rating::default()).with_title("");
        assert_eq!(snapshot(&p, &[]).title, None);
    }

    #[test]
    fn snapshot_drops_duplicate_keywords() {
        let p = Photo::new("a.jpg", 1, Rating::default());
        let f = snapshot(&p, &[kw(&["A"]), kw(&["A"])]);
        assert_eq!(f.keywords, vec!["A".to_string()]);
    }

    #[test]
    fn keyword_text_joins_the_path() {
        assert_eq!(keyword_text(&kw(&["a", "b", "c"])), "a|b|c");
    }

    #[test]
    fn same_attributes_ignores_the_global_id() {
        let entry = KeywordEntry::of(&kw(&["A"]).with_global_id("x"));
        assert!(entry.same_attributes(&kw(&["A"])));
        let mut other = kw(&["A"]);
        other.person = true;
        assert!(!entry.same_attributes(&other));
    }

    #[test]
    fn version_as_text_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.lr2lc.json");
        std::fs::write(&path, r#"{"version":"1","photos":{},"keywords":{}}"#).unwrap();
        assert!(matches!(
            State::load(&path),
            Err(StateError::Version { .. })
        ));
    }

    #[test]
    fn wrong_shape_is_a_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.lr2lc.json");
        std::fs::write(&path, r#"{"version":1,"photos":[]}"#).unwrap();
        assert!(matches!(State::load(&path), Err(StateError::Parse { .. })));
    }

    #[test]
    fn save_into_a_missing_folder_fails_and_leaves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing").join("s.lr2lc.json");
        assert!(State::default().save(&path).is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    fn collection_entry(members: &[&str]) -> CollectionEntry {
        CollectionEntry {
            lightcraft_id: Value::from(7),
            kind: EntryKind::Album,
            name: "A".to_string(),
            parent: Some(1),
            members: members.iter().map(|s| s.to_string()).collect(),
            rules: None,
        }
    }

    #[test]
    fn collections_round_trip_with_text_keys_and_lowercase_kind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.lr2lc.json");
        let mut s = State::default();
        s.record_collection(12, collection_entry(&["b", "a", "b"]));
        s.save(&path).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            v["collections"]["12"],
            serde_json::json!({"lightcraft_id": 7, "kind": "album", "name": "A",
                "parent": 1, "members": ["a", "b"]})
        );
        assert_eq!(State::load(&path).unwrap(), s);
    }

    #[test]
    fn stacks_round_trip_in_order_and_missing_ones_load_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.json");
        let mut s = State::default();
        s.record_stack(7, vec!["b".into(), "a".into()]);
        s.save(&path).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["stacks"]["7"], json!({"members": ["b", "a"]}));
        assert_eq!(State::load(&path).unwrap(), s);
        std::fs::write(&path, r#"{"version":1,"photos":{},"keywords":{}}"#).unwrap();
        assert!(State::load(&path).unwrap().stack(7).is_none());
    }

    #[test]
    fn missing_collections_load_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.lr2lc.json");
        std::fs::write(&path, r#"{"version":1,"photos":{},"keywords":{}}"#).unwrap();
        assert_eq!(State::load(&path).unwrap().collections().count(), 0);
    }

    #[test]
    fn unknown_collection_kind_is_a_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.lr2lc.json");
        let text = r#"{"version":1,"photos":{},"keywords":{},"collections":{"1":
            {"lightcraft_id":1,"kind":"zyx","name":"x","parent":null,"members":[]}}}"#;
        std::fs::write(&path, text).unwrap();
        assert!(matches!(State::load(&path), Err(StateError::Parse { .. })));
    }

    #[test]
    fn smart_entries_keep_their_rules_and_rules_are_optional() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.lr2lc.json");
        let mut s = State::default();
        let rules = json!({"ruleSet": {"match": "all", "rules": []}});
        s.record_collection(
            5,
            CollectionEntry {
                kind: EntryKind::Smart,
                rules: Some(rules.clone()),
                ..collection_entry(&[])
            },
        );
        s.save(&path).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["collections"]["5"]["kind"], "smart");
        assert_eq!(v["collections"]["5"]["rules"], rules);
        assert_eq!(State::load(&path).unwrap(), s);
        let text = r#"{"version":1,"photos":{},"keywords":{},"collections":{"1":
            {"lightcraft_id":1,"kind":"smart","name":"x","parent":null,"members":[]}}}"#;
        std::fs::write(&path, text).unwrap();
        let loaded = State::load(&path).unwrap();
        assert_eq!(loaded.collection(1).unwrap().rules, None);
    }

    #[test]
    fn record_replaces_an_earlier_entry() {
        let mut s = State::default();
        s.record("g", 1, Value::from(1), PhotoFields::imported_default());
        s.record("g", 1, Value::from(2), PhotoFields::imported_default());
        assert_eq!(s.photos().count(), 1);
        assert_eq!(s.photo("g").unwrap().lightcraft_id, Value::from(2));
    }

    #[test]
    fn the_recorded_path_round_trips_and_is_never_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.lr2lc.json");
        let old = dir.path().join("old.jpg");
        let new = dir.path().join("new.jpg");
        let mut s = State::default();
        s.record("g", 1, Value::from(1), PhotoFields::imported_default());
        assert!(s.remember_path("g", &old));
        assert!(!s.remember_path("g", &new));
        s.save(&path).unwrap();
        let loaded = State::load(&path).unwrap();
        assert_eq!(loaded.photo("g").unwrap().path.as_deref(), old.to_str());
        // Recording new field values keeps the base path for move detection.
        s.record("g", 1, Value::from(1), PhotoFields::imported_default());
        assert_eq!(s.photo("g").unwrap().path.as_deref(), old.to_str());
    }

    #[test]
    fn remember_path_ignores_unknown_photos() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = State::default();
        assert!(!s.remember_path("missing", &dir.path().join("a.jpg")));
    }

    #[test]
    fn the_develop_hash_round_trips_and_record_keeps_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.lr2lc.json");
        let mut s = State::default();
        s.record("g", 1, Value::from(1), PhotoFields::imported_default());
        assert!(s.record_develop_hash("g", Some(hex_hash(&[0xab; 32]))));
        assert!(!s.record_develop_hash("g", Some(hex_hash(&[0xab; 32]))));
        // Recording new field values keeps the hash, like the base path.
        s.record("g", 1, Value::from(1), PhotoFields::imported_default());
        assert_eq!(
            s.photo("g").unwrap().develop_hash.as_deref(),
            Some(hex_hash(&[0xab; 32]).as_str())
        );
        s.save(&path).unwrap();
        let loaded = State::load(&path).unwrap();
        assert_eq!(
            loaded.photo("g").unwrap().develop_hash.as_deref(),
            Some(hex_hash(&[0xab; 32]).as_str())
        );
    }

    #[test]
    fn an_entry_without_a_develop_hash_loads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.lr2lc.json");
        let text = r#"{"version":1,"photos":{"g":{"lightroom_id":1,"lightcraft_id":2,
            "fields":{"rating":0,"flag":"none","label":null,"title":null,
            "caption":null,"copyright":null,"creator":null,"keywords":[]}}},
            "keywords":{}}"#;
        std::fs::write(&path, text).unwrap();
        let loaded = State::load(&path).unwrap();
        assert_eq!(loaded.photo("g").unwrap().develop_hash, None);
    }

    #[test]
    fn hex_hash_is_lowercase_and_64_characters() {
        let text = hex_hash(&[0xab; 32]);
        assert_eq!(text.len(), 64);
        assert!(text.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(!text.chars().any(|c| c.is_ascii_uppercase()));
    }

    #[test]
    fn keyword_lightroom_id_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.lr2lc.json");
        let keyword = kw(&["A"]).with_global_id("kg-1").with_lightroom_id(13);
        assert_eq!(KeywordEntry::of(&keyword).lightroom_id, Some(13));
        let mut s = State::default();
        s.record_keyword(&keyword);
        s.save(&path).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["keywords"]["A"]["lightroom_id"], 13);
        assert_eq!(
            State::load(&path)
                .unwrap()
                .keyword("A")
                .unwrap()
                .lightroom_id,
            Some(13)
        );
    }
}
