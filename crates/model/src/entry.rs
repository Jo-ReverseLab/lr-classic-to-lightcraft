//! Log entries: one line of the run log per thing that was done, adjusted,
//! skipped, in conflict or failed.
//!
//! An entry names its subject by id ([`Subject`]); names, paths and values
//! of the catalog are kept apart (`item`, `source`, `result`) so a report can
//! drop them when it is anonymized. `message`, `reason` and the code never
//! contain a name, a path or a value.

/// What happened to the subject of an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    /// Done as in Lightroom.
    Transferred,
    /// Done, but changed on purpose to fit LightCraft.
    Adjusted,
    /// Deliberately not done: not supported yet, or not selected.
    Skipped,
    /// Changed on both sides since the last import; LightCraft's state kept.
    Conflict,
    /// Should have been done but failed.
    Error,
}

impl Status {
    pub const ALL: [Status; 5] = [
        Status::Transferred,
        Status::Adjusted,
        Status::Skipped,
        Status::Conflict,
        Status::Error,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Status::Transferred => "transferred",
            Status::Adjusted => "adjusted",
            Status::Skipped => "skipped",
            Status::Conflict => "conflict",
            Status::Error => "error",
        }
    }
}

/// The part of the catalog an entry belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Area {
    Folders,
    Photos,
    Keywords,
    Ratings,
    Labels,
    Flags,
    /// Title, caption, copyright and creator.
    Metadata,
    Collections,
    Stacks,
    VirtualCopies,
    Develop,
}

impl Area {
    pub fn as_str(self) -> &'static str {
        match self {
            Area::Folders => "folders",
            Area::Photos => "photos",
            Area::Keywords => "keywords",
            Area::Ratings => "ratings",
            Area::Labels => "labels",
            Area::Flags => "flags",
            Area::Metadata => "metadata",
            Area::Collections => "collections",
            Area::Stacks => "stacks",
            Area::VirtualCopies => "virtual-copies",
            Area::Develop => "develop",
        }
    }
}

/// What an entry is about, by id; names are looked up when the report is
/// written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    /// Nothing in particular (a whole area, the run).
    None,
    /// A photo by Lightroom id.
    Photo(i64),
    /// A photo by global id (the writer knows photos by it).
    PhotoGlobal(String),
    /// A keyword by its path text `a|b|c`.
    Keyword(String),
    /// A collection or collection set by Lightroom id.
    Collection(i64),
    /// A folder stack by Lightroom id.
    Stack(i64),
    /// A folder by its path.
    Folder(String),
}

/// The run's steps an entry can come from, in run order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stage {
    /// Reading the catalog.
    Read,
    /// Creating keywords.
    Keywords,
    /// Importing photos with their fields and development settings.
    Photos,
    /// Albums, stacks, the Quick Collection and smart albums.
    Organize,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Read => "read",
            Stage::Keywords => "keywords",
            Stage::Photos => "photos",
            Stage::Organize => "organize",
        }
    }
}

/// One line of the run log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub status: Status,
    pub area: Area,
    pub stage: Stage,
    pub subject: Subject,
    /// What happened, short; never a name, path or value.
    pub message: String,
    /// The code of a skip or failure, as the report gives it.
    pub code: Option<&'static str>,
    /// Why, in more detail; never a name, path or value.
    pub reason: Option<String>,
    /// The value in Lightroom (private: dropped when anonymized).
    pub source: Option<String>,
    /// The value in LightCraft (private: dropped when anonymized).
    pub result: Option<String>,
    /// A name the writer already knows (a file name, keyword or album
    /// name); private, dropped when anonymized.
    pub item: Option<String>,
}

impl Entry {
    pub fn new(
        status: Status,
        area: Area,
        stage: Stage,
        subject: Subject,
        message: impl Into<String>,
    ) -> Entry {
        Entry {
            status,
            area,
            stage,
            subject,
            message: message.into(),
            code: None,
            reason: None,
            source: None,
            result: None,
            item: None,
        }
    }

    pub fn code(mut self, code: &'static str) -> Entry {
        self.code = Some(code);
        self
    }

    pub fn reason(mut self, reason: impl Into<String>) -> Entry {
        self.reason = Some(reason.into());
        self
    }

    pub fn source(mut self, value: impl Into<String>) -> Entry {
        self.source = Some(value.into());
        self
    }

    pub fn result(mut self, value: impl Into<String>) -> Entry {
        self.result = Some(value.into());
        self
    }

    pub fn item(mut self, name: impl Into<String>) -> Entry {
        self.item = Some(name.into());
        self
    }
}
