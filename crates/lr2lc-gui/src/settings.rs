//! What the window remembers between starts: catalog and library, the path
//! mappings per catalog, what to transfer and the advanced options. Kept as
//! JSON in the user's configuration folder; a missing or broken file means
//! defaults.

use crate::theme::ThemeMode;
use lr2lc_core::labels::{LabelChoice, LabelMode};
use lr2lc_core::{Areas, Job};
use model::{PathMapping, PathMappings};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The default control port of LightCraft.
pub const DEFAULT_PORT: u16 = 7980;

/// One path mapping as typed: the path in Lightroom, the path here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MappingRow {
    pub old: String,
    pub new: String,
}

impl MappingRow {
    pub fn is_empty(&self) -> bool {
        self.old.trim().is_empty() && self.new.trim().is_empty()
    }
}

/// Which colour label names to read.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Labels {
    #[default]
    Auto,
    English,
    German,
    File(String),
}

/// What to transfer, as the checkboxes show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Selection {
    pub keywords: bool,
    pub ratings: bool,
    pub labels: bool,
    pub flags: bool,
    pub texts: bool,
    pub collections: bool,
    pub stacks: bool,
    pub virtual_copies: bool,
    pub develop: bool,
}

impl Default for Selection {
    fn default() -> Self {
        Selection::EVERYTHING
    }
}

impl Selection {
    pub const EVERYTHING: Selection = Selection {
        keywords: true,
        ratings: true,
        labels: true,
        flags: true,
        texts: true,
        collections: true,
        stacks: true,
        virtual_copies: true,
        develop: true,
    };

    pub const NOTHING: Selection = Selection {
        keywords: false,
        ratings: false,
        labels: false,
        flags: false,
        texts: false,
        collections: false,
        stacks: false,
        virtual_copies: false,
        develop: false,
    };

    pub fn areas(&self) -> Areas {
        Areas {
            keywords: self.keywords,
            ratings: self.ratings,
            labels: self.labels,
            flags: self.flags,
            texts: self.texts,
            collections: self.collections,
            stacks: self.stacks,
            virtual_copies: self.virtual_copies,
            develop: self.develop,
        }
    }
}

/// Everything the window remembers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub catalog: String,
    pub library: String,
    /// Path mappings per catalog path.
    pub mappings: BTreeMap<String, Vec<MappingRow>>,
    pub selection: Selection,
    pub overwrite_develop: bool,
    pub allow_xmp: bool,
    pub labels: Labels,
    pub port: u16,
    pub theme: ThemeMode,
    /// The target is whatever library LightCraft has open.
    pub follow_app: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            catalog: String::new(),
            library: String::new(),
            mappings: BTreeMap::new(),
            selection: Selection::default(),
            overwrite_develop: false,
            allow_xmp: false,
            labels: Labels::Auto,
            port: DEFAULT_PORT,
            theme: ThemeMode::System,
            follow_app: true,
        }
    }
}

/// Why the settings cannot make a job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invalid {
    NoCatalog,
    NoLibrary,
    /// A mapping row (1-based) with only one side filled in.
    Mapping(usize),
    /// The label file cannot be read or is not a label set.
    Labels(String),
}

impl Invalid {
    pub fn text(&self) -> String {
        match self {
            Invalid::NoCatalog => "Choose a Lightroom catalog.".to_owned(),
            Invalid::NoLibrary => "Choose the LightCraft library.".to_owned(),
            Invalid::Mapping(n) => format!(
                "Path mapping {n} needs both the path in Lightroom and the path on this computer."
            ),
            Invalid::Labels(why) => format!("Color label names: {why}"),
        }
    }
}

impl Settings {
    /// The file the settings live in.
    pub fn file() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("lr2lc").join("gui.json"))
    }

    /// Reads the settings from `path`; defaults when missing or broken.
    pub fn load(path: &Path) -> Settings {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Writes the settings to `path`, creating its folder.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, text)
    }

    /// The mapping rows of the current catalog.
    pub fn rows(&self) -> &[MappingRow] {
        self.mappings.get(&self.catalog).map_or(&[], Vec::as_slice)
    }

    /// The mapping rows of the current catalog, to edit.
    pub fn rows_mut(&mut self) -> &mut Vec<MappingRow> {
        self.mappings.entry(self.catalog.clone()).or_default()
    }

    /// The path mapping rules; empty rows are ignored.
    pub fn path_mappings(&self) -> Result<PathMappings, Invalid> {
        let mut rules = Vec::new();
        for (index, row) in self.rows().iter().enumerate() {
            if row.is_empty() {
                continue;
            }
            if row.old.trim().is_empty() || row.new.trim().is_empty() {
                return Err(Invalid::Mapping(index + 1));
            }
            let rule = PathMapping::new(row.old.trim(), row.new.trim())
                .map_err(|_| Invalid::Mapping(index + 1))?;
            rules.push(rule);
        }
        Ok(PathMappings::new(rules))
    }

    /// The job for a dry run (`apply` false) or a transfer.
    pub fn job(&self, apply: bool) -> Result<Job, Invalid> {
        if self.catalog.trim().is_empty() {
            return Err(Invalid::NoCatalog);
        }
        let library = Some(self.library.trim())
            .filter(|l| !l.is_empty())
            .map(PathBuf::from);
        if apply && library.is_none() {
            return Err(Invalid::NoLibrary);
        }
        let choice = match &self.labels {
            Labels::Auto => LabelChoice::Auto,
            Labels::English => LabelChoice::En,
            Labels::German => LabelChoice::De,
            Labels::File(path) => LabelChoice::File(PathBuf::from(path)),
        };
        let labels = LabelMode::load(&choice).map_err(Invalid::Labels)?;
        Ok(Job {
            catalog: PathBuf::from(self.catalog.trim()),
            mappings: self.path_mappings()?,
            labels,
            library,
            apply,
            port: self.port,
            allow_xmp: self.allow_xmp,
            overwrite_develop: self.overwrite_develop && self.selection.develop,
            areas: self.selection.areas(),
            // The report in the run history keeps the names; the person can
            // save an anonymized copy.
            anonymize: false,
            // Without the question in the window: the safe default.
            existing: Default::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_survive_a_broken_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sub").join("gui.json");
        let mut settings = Settings {
            catalog: "/c.lrcat".into(),
            labels: Labels::German,
            theme: ThemeMode::Dark,
            ..Settings::default()
        };
        settings.rows_mut().push(MappingRow {
            old: "D:\\Photos".into(),
            new: "/Volumes/Photos".into(),
        });
        settings.save(&file).unwrap();
        assert_eq!(Settings::load(&file), settings);
        std::fs::write(&file, "{ not json").unwrap();
        assert_eq!(Settings::load(&file), Settings::default());
    }

    #[test]
    fn mappings_are_kept_per_catalog() {
        let mut s = Settings {
            catalog: "/a.lrcat".into(),
            ..Settings::default()
        };
        s.rows_mut().push(MappingRow {
            old: "/x".into(),
            new: "/y".into(),
        });
        s.catalog = "/b.lrcat".into();
        assert!(s.rows().is_empty());
        s.catalog = "/a.lrcat".into();
        assert_eq!(s.rows().len(), 1);
    }

    #[test]
    fn a_job_needs_a_catalog_and_for_a_transfer_a_library() {
        let mut s = Settings::default();
        assert_eq!(s.job(false).unwrap_err(), Invalid::NoCatalog);
        s.catalog = "/c.lrcat".into();
        assert!(!s.job(false).unwrap().apply);
        assert_eq!(s.job(true).unwrap_err(), Invalid::NoLibrary);
        s.library = "/Pictures/Lib".into();
        assert!(s.job(true).unwrap().apply);
    }

    #[test]
    fn half_rows_are_refused_and_empty_rows_ignored() {
        let mut s = Settings {
            catalog: "/c.lrcat".into(),
            ..Settings::default()
        };
        s.rows_mut().push(MappingRow::default());
        s.rows_mut().push(MappingRow {
            old: "C:\\Fotos".into(),
            new: "/Users/example/Fotos".into(),
        });
        assert_eq!(s.job(false).unwrap().mappings.rules().len(), 1);
        s.rows_mut().push(MappingRow {
            old: "/x".into(),
            new: " ".into(),
        });
        assert_eq!(s.job(false).unwrap_err(), Invalid::Mapping(3));
    }

    #[test]
    fn develop_off_never_overwrites_and_selection_maps_to_areas() {
        let mut s = Settings {
            catalog: "/c.lrcat".into(),
            overwrite_develop: true,
            ..Settings::default()
        };
        s.selection.develop = false;
        s.selection.keywords = false;
        let job = s.job(false).unwrap();
        assert!(!job.overwrite_develop);
        assert_eq!(job.areas.not_selected(), ["keywords", "develop"]);
    }
}
