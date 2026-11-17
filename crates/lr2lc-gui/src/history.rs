//! The run history: each run's `report.json` (with names) plus a small index
//! in the app's data folder. The Log lists runs newest first.

use chrono::{DateTime, Local, TimeZone};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The kind of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Transfer,
    DryRun,
}

/// One run in the index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// Also the report's file name (without `.json`).
    pub id: String,
    pub kind: Kind,
    /// Unix seconds.
    pub started: i64,
    pub finished: i64,
    pub catalog: String,
    pub library: String,
    pub cancelled: bool,
    /// Why the run failed or stopped, if it did.
    pub error: Option<String>,
    /// Whether a report was written (none when reading failed).
    pub has_report: bool,
}

impl Record {
    /// `15 Nov 2026, 22:14 · Transfer`.
    pub fn title(&self) -> String {
        let kind = match self.kind {
            Kind::Transfer => "Transfer",
            Kind::DryRun => "Dry run",
        };
        format!(
            "{} · {kind}",
            local(self.started).format("%-d %b %Y, %H:%M")
        )
    }

    /// `m:ss`.
    pub fn duration(&self) -> String {
        let secs = (self.finished - self.started).max(0);
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

/// The local time of unix seconds `secs`.
pub fn local(secs: i64) -> DateTime<Local> {
    Local
        .timestamp_opt(secs, 0)
        .single()
        .unwrap_or_else(Local::now)
}

/// How many runs are kept; older ones are removed with their reports.
pub const KEEP: usize = 50;

/// The history folder.
#[derive(Debug, Clone)]
pub struct History {
    dir: PathBuf,
}

impl History {
    /// The app's history folder, `<data dir>/lr2lc/runs`.
    pub fn default_dir() -> Option<PathBuf> {
        dirs::data_dir().map(|d| d.join("lr2lc").join("runs"))
    }

    pub fn new(dir: PathBuf) -> History {
        History { dir }
    }

    fn index_file(&self) -> PathBuf {
        self.dir.join("index.json")
    }

    /// Every run, newest first; empty when there is no index.
    pub fn list(&self) -> Vec<Record> {
        let mut records: Vec<Record> = std::fs::read_to_string(self.index_file())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        records.sort_by(|a, b| b.started.cmp(&a.started).then(b.id.cmp(&a.id)));
        records
    }

    /// Adds `record` with its report, with names and anonymized.
    pub fn add(&self, record: &Record, report: &str, anonymized: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        if record.has_report {
            std::fs::write(self.report_file(&record.id), report)?;
            std::fs::write(
                self.report_file(&format!("{}-anonymized", record.id)),
                anonymized,
            )?;
        }
        let mut records = self.list();
        records.retain(|r| r.id != record.id);
        records.insert(0, record.clone());
        records.sort_by(|a, b| b.started.cmp(&a.started).then(b.id.cmp(&a.id)));
        // Reports hold names and paths: old runs do not pile up.
        for old in records.drain(KEEP.min(records.len())..) {
            let _ = std::fs::remove_file(self.report_file(&old.id));
            let _ = std::fs::remove_file(self.report_file(&format!("{}-anonymized", old.id)));
        }
        let text = serde_json::to_string_pretty(&records).map_err(std::io::Error::other)?;
        // Written beside and renamed, so a crash never leaves half an index.
        let tmp = self.dir.join("index.json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, self.index_file())
    }

    /// The report of run `id`.
    pub fn report(&self, id: &str) -> Option<String> {
        std::fs::read_to_string(self.report_file(id)).ok()
    }

    /// The anonymized report of run `id`.
    pub fn anonymized(&self, id: &str) -> Option<String> {
        std::fs::read_to_string(self.report_file(&format!("{id}-anonymized"))).ok()
    }

    fn report_file(&self, id: &str) -> PathBuf {
        // Ids are made by [`new_id`]: digits and a dash only.
        let safe: String = id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        self.dir.join(format!("{safe}.json"))
    }
}

/// A new run id from the start time: `20261115-221432-3`.
pub fn new_id(started: DateTime<Local>, existing: &[Record]) -> String {
    let base = started.format("%Y%m%d-%H%M%S").to_string();
    let mut id = base.clone();
    let mut n = 1;
    while existing.iter().any(|r| r.id == id) {
        n += 1;
        id = format!("{base}-{n}");
    }
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: &str, started: i64) -> Record {
        Record {
            id: id.into(),
            kind: Kind::DryRun,
            started,
            finished: started + 41,
            catalog: "/c.lrcat".into(),
            library: String::new(),
            cancelled: false,
            error: None,
            has_report: true,
        }
    }

    #[test]
    fn runs_are_kept_newest_first_with_their_reports() {
        let dir = tempfile::tempdir().unwrap();
        let history = History::new(dir.path().join("runs"));
        assert!(history.list().is_empty());
        history.add(&record("a", 100), "{\"x\":1}", "{}").unwrap();
        history.add(&record("b", 200), "{\"x\":2}", "{}").unwrap();
        let ids: Vec<String> = history.list().into_iter().map(|r| r.id).collect();
        assert_eq!(ids, ["b", "a"]);
        assert_eq!(history.report("a").as_deref(), Some("{\"x\":1}"));
        assert_eq!(history.anonymized("a").as_deref(), Some("{}"));
        assert_eq!(record("a", 100).duration(), "0:41");
        assert!(history.report("../../etc").is_none());
    }

    #[test]
    fn only_the_newest_runs_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let history = History::new(dir.path().join("runs"));
        for i in 0..(KEEP as i64 + 3) {
            history
                .add(&record(&format!("r{i}"), i), "{}", "{}")
                .unwrap();
        }
        let list = history.list();
        assert_eq!(list.len(), KEEP);
        assert!(
            history.report("r0").is_none(),
            "the oldest report is removed"
        );
        assert!(history.report(&format!("r{}", KEEP + 2)).is_some());
    }

    #[test]
    fn ids_are_unique() {
        let t = local(1_700_000_000);
        let first = new_id(t, &[]);
        let second = new_id(t, &[record(&first, 0)]);
        assert_ne!(first, second);
        assert!(second.ends_with("-2"));
    }
}
