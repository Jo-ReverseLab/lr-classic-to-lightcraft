//! The check for photos LightCraft already has, as a front end drives it:
//! `prepare` → `check_library` (or `check_library_with` through a host's
//! sink) → decide → `Prepared::set_check` → `finish`; and `finish`
//! checking by itself when no front end did.

use lc_test_support::fake_lightcraft::{Config, FakeLightCraft, READ_COMMANDS, SeedPhoto};
use lr2lc_core::labels::LabelMode;
use lr2lc_core::{
    ExistingChoice, ExistingDecisions, ExistingReason, HostSink, Job, NoProgress, Phase, Progress,
    Request, check_library, check_library_with, finish, finish_with, prepare,
};
use lrcat_fixtures::write_keyword_edge_catalog_with_files;
use model::PathMappings;
use serde_json::{Value, json};
use std::path::PathBuf;

struct Setup {
    _photos: tempfile::TempDir,
    _work: tempfile::TempDir,
    job: Job,
}

fn setup() -> Setup {
    let photos = tempfile::tempdir().unwrap();
    let catalog = write_keyword_edge_catalog_with_files(photos.path()).unwrap();
    let work = tempfile::tempdir().unwrap();
    let library: PathBuf = work.path().join("Test.lightcraft");
    let job = Job {
        catalog,
        mappings: PathMappings::default(),
        labels: LabelMode::Auto,
        library: Some(library),
        apply: true,
        port: 0,
        allow_xmp: false,
        overwrite_develop: false,
        areas: lr2lc_core::Areas::ALL,
        anonymize: false,
        existing: ExistingDecisions::default(),
        removed: Default::default(),
    };
    Setup {
        _photos: photos,
        _work: work,
        job,
    }
}

/// A fake whose library has photo 7, edited and with three stars, holding
/// the same bytes as the first photo of the catalog. Returns the fake and
/// that photo's global id.
fn fake_with_a_copy_of_the_first_photo(job: &Job) -> (FakeLightCraft, String) {
    let prepared = prepare(job, &mut NoProgress).unwrap();
    let first = prepared
        .input
        .photos
        .iter()
        .filter(|p| p.global_id.is_some() && p.master.is_none())
        .min_by_key(|p| p.global_id.clone())
        .unwrap();
    let mut config = Config::new(job.library.as_deref().unwrap());
    config.seed_photos = vec![SeedPhoto {
        rating: 3,
        edited: true,
        ..SeedPhoto::plain(7)
    }];
    let seed = "/Users/example/Pictures/seed-7.jpg".to_string();
    config.file_contents.insert(seed, "same bytes".into());
    config.file_contents.insert(
        first.path.to_string_lossy().into_owned(),
        "same bytes".into(),
    );
    let gid = first.global_id.clone().unwrap();
    (FakeLightCraft::start(config), gid)
}

fn writer(done: &lr2lc_core::Finished) -> Value {
    let report: Value = serde_json::from_str(&done.report_json()).unwrap();
    report["writer"].clone()
}

struct Phases(Vec<Phase>);

impl Progress for Phases {
    fn phase(&mut self, phase: Phase, _total: usize) {
        self.0.push(phase);
    }
}

#[test]
fn check_then_finish_with_the_chosen_decisions() {
    let mut s = setup();
    let (fake, gid) = fake_with_a_copy_of_the_first_photo(&s.job);
    s.job.port = fake.port();
    let mut prepared = prepare(&s.job, &mut NoProgress).unwrap();
    let mut phases = Phases(Vec::new());
    let check = check_library(&s.job, &prepared, &mut phases).unwrap();
    assert!(check.removed.is_empty());
    let found = check.existing.clone();
    assert_eq!(phases.0, [Phase::CheckingLibrary]);
    assert!(fake.write_commands().is_empty(), "{:?}", fake.commands());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].global_id, gid);
    assert_eq!(found[0].reason, ExistingReason::Content);
    assert_eq!(found[0].lightcraft_id, json!(7));
    assert_eq!((found[0].live.rating, found[0].live.edited), (3, true));

    // The person chose to keep this photo as it is.
    s.job.existing.per_photo.insert(gid, ExistingChoice::Skip);
    prepared.set_check(check);
    let previews_before = preview_count(&fake);
    let done = finish(&s.job, prepared, &mut NoProgress).unwrap();
    done.result().unwrap();
    assert_eq!(preview_count(&fake), previews_before, "not checked again");
    let w = writer(&done);
    assert_eq!(w["photos_existing"], 1);
    assert_eq!(w["existing"]["found"], 1);
    assert_eq!(w["existing"]["skip"], 1);
    assert_eq!(w["existing"]["found_while_importing"], 0);
    assert_eq!(fake.photo_fields(7).unwrap().rating, 3);
    assert!(
        fake.write_commands()
            .iter()
            .filter(|(c, _)| c.starts_with("photo.") || c.starts_with("develop."))
            .all(|(_, p)| p["ids"] != json!([7])),
        "nothing written to the skipped photo"
    );
    assert!(done.writer_summary.contains(
        "existing: already in LightCraft 1 (overwrite 0, develop 0, develop-if-empty 0, \
         develop kept 0, skip 1; found while importing 0)"
    ));
}

#[test]
fn finish_checks_by_itself_and_applies_the_job_default() {
    let mut s = setup();
    let (fake, _) = fake_with_a_copy_of_the_first_photo(&s.job);
    s.job.port = fake.port();
    s.job.existing = ExistingDecisions::all(ExistingChoice::Overwrite);
    let prepared = prepare(&s.job, &mut NoProgress).unwrap();
    let done = finish(&s.job, prepared, &mut NoProgress).unwrap();
    done.result().unwrap();
    assert_eq!(preview_count(&fake), 1);
    let w = writer(&done);
    assert_eq!(w["existing"]["found"], 1);
    assert_eq!(w["existing"]["overwrite"], 1);
    assert_eq!(w["existing"]["found_while_importing"], 0);
    let report: Value = serde_json::from_str(&done.report_json()).unwrap();
    let adopted = report["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["code"] == "existing_adopted")
        .count();
    assert_eq!(adopted, 1);
}

#[test]
fn the_check_through_a_host_sink_matches_the_port() {
    let mut s = setup();
    let (port_fake, gid) = fake_with_a_copy_of_the_first_photo(&s.job);
    s.job.port = port_fake.port();
    let prepared = prepare(&s.job, &mut NoProgress).unwrap();
    let by_port = check_library(&s.job, &prepared, &mut NoProgress)
        .unwrap()
        .existing;

    let local = setup();
    let (fake, _) = fake_with_a_copy_of_the_first_photo(&local.job);
    let mut sink = HostSink::new(|request, params| match request {
        Request::Command(command) => fake.dispatch(command, params, true),
        Request::Method(method) => fake.dispatch(method, params, false),
    });
    let mut prepared = prepare(&local.job, &mut NoProgress).unwrap();
    let check = check_library_with(&local.job, &prepared, &mut sink, &mut NoProgress).unwrap();
    let found = &check.existing;
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].global_id, gid);
    assert_eq!(found[0].live, by_port[0].live);
    assert!(fake.write_commands().is_empty());

    prepared.set_check(check);
    let done = finish_with(&local.job, prepared, &mut sink, &mut NoProgress).unwrap();
    done.result().unwrap();
    assert_eq!(writer(&done)["existing"]["develop_if_empty"], 1);
}

#[test]
fn a_dry_run_checks_nothing() {
    let mut s = setup();
    s.job.apply = false;
    let prepared = prepare(&s.job, &mut NoProgress).unwrap();
    // No LightCraft is running on port 0: a check would fail.
    assert!(
        check_library(&s.job, &prepared, &mut NoProgress)
            .unwrap()
            .existing
            .is_empty()
    );
    let done = finish(&s.job, prepared, &mut NoProgress).unwrap();
    assert!(
        done.writer_summary
            .contains("existing: checked when transferring")
    );
}

#[test]
fn a_cancel_during_the_check_writes_nothing() {
    let mut s = setup();
    let (fake, _) = fake_with_a_copy_of_the_first_photo(&s.job);
    s.job.port = fake.port();
    let prepared = prepare(&s.job, &mut NoProgress).unwrap();

    struct Stop;
    impl Progress for Stop {
        fn cancelled(&self) -> bool {
            true
        }
    }
    let Err(error) = check_library(&s.job, &prepared, &mut Stop) else {
        panic!("expected a cancel");
    };
    assert_eq!(error.kind, lr2lc_core::ErrorKind::Cancelled);
    assert!(fake.write_commands().is_empty());
}

fn preview_count(fake: &FakeLightCraft) -> usize {
    assert!(READ_COMMANDS.contains(&"library.importPreview"));
    fake.commands()
        .iter()
        .filter(|(c, _)| c == "library.importPreview")
        .count()
}

#[test]
fn cloud_only_originals_to_import_are_counted_and_logged() {
    let mut s = setup();
    s.job.apply = false;
    let mut prepared = prepare(&s.job, &mut NoProgress).unwrap();
    assert_eq!(prepared.cloud_only_to_import(), (0, 0));
    // The dataless flag can only be set by a file provider such as iCloud;
    // mark the first photo the way the reader would.
    let first = prepared.input.photos[0].lightroom_id;
    prepared.outcome.cloud_only.photos.push((first, 30_000_000));
    assert_eq!(prepared.cloud_only().count(), 1);
    assert_eq!(prepared.cloud_only_to_import(), (1, 30_000_000));
    assert!(
        prepared
            .catalog_summary(&s.job)
            .contains("originals only in the cloud (not downloaded): 1 (30.0 MB)\ndownload them")
    );
    let done = finish(&s.job, prepared, &mut NoProgress).unwrap();
    let report: Value = serde_json::from_str(&done.report_json()).unwrap();
    assert_eq!(report["photos"]["cloud_only"], 1);
    assert_eq!(report["photos"]["cloud_only_bytes"], 30_000_000);
    let warned: Vec<&Value> = report["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["code"] == "original_in_cloud")
        .collect();
    assert_eq!(warned.len(), 1);
    assert_eq!(warned[0]["photo"], first);
    assert!(
        warned[0]["recommendation"]
            .as_str()
            .unwrap()
            .contains("Download")
    );
}

#[test]
fn check_library_reports_photos_deleted_in_lightcraft() {
    let mut s = setup();
    let fake = FakeLightCraft::start(Config::new(s.job.library.as_deref().unwrap()));
    s.job.port = fake.port();
    let prepared = prepare(&s.job, &mut NoProgress).unwrap();
    finish(&s.job, prepared, &mut NoProgress)
        .unwrap()
        .result()
        .unwrap();
    let state = lc_writer::state::State::load(&lc_writer::state::state_path(
        s.job.library.as_deref().unwrap(),
    ))
    .unwrap();
    let (gid, entry) = state.photos().find(|(_, e)| e.copy_of.is_none()).unwrap();
    fake.trash(entry.lightcraft_id.as_i64().unwrap());

    let mut prepared = prepare(&s.job, &mut NoProgress).unwrap();
    let check = check_library(&s.job, &prepared, &mut NoProgress).unwrap();
    assert_eq!(check.removed.len(), 1);
    assert_eq!(check.removed[0].global_id, gid);
    assert!(check.removed[0].trashed);
    s.job.removed = lr2lc_core::RemovedDecisions::all(lr2lc_core::RemovedChoice::Leave);
    prepared.set_check(check);
    let done = finish(&s.job, prepared, &mut NoProgress).unwrap();
    done.result().unwrap();
    let w = writer(&done);
    assert_eq!(w["removed_in_lightcraft"]["left"], 1);
    assert_eq!(w["photos_skipped_by_code"]["removed_in_lightcraft"], 1);
    assert!(done.writer_summary.contains(
        "removed in LightCraft: 1 (imported again 0, restored from the trash 0, left 1)"
    ));
}

#[test]
fn the_report_keeps_lightcrafts_message_without_paths_when_anonymized() {
    let mut s = setup();
    let mut config = Config::new(s.job.library.as_deref().unwrap());
    config
        .failures
        .push(lc_test_support::fake_lightcraft::Failure {
            command: "photo.rate".into(),
            photo_id: None,
            message: "photo locked: \"/Users/example/Pictures/a.jpg\" is read-only".into(),
        });
    let fake = FakeLightCraft::start(config);
    s.job.port = fake.port();
    let prepared = prepare(&s.job, &mut NoProgress).unwrap();
    let done = finish(&s.job, prepared, &mut NoProgress).unwrap();
    let reason = |anonymize: bool| -> String {
        let report: Value = serde_json::from_str(&done.report_json_as(anonymize)).unwrap();
        report["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["code"] == "rejected")
            .expect("a rejected entry")["reason"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let named = reason(false);
    assert!(
        named.contains("LightCraft rejected `photo.rate`"),
        "{named}"
    );
    assert!(
        named.contains("LightCraft said: photo locked: \"/Users/example/Pictures/a.jpg\""),
        "{named}"
    );
    let anonymous = reason(true);
    assert!(
        anonymous.ends_with("LightCraft said: photo locked: \"<path>\" is read-only"),
        "{anonymous}"
    );
    // The writer's own skip list keeps fixed text.
    let report: Value = serde_json::from_str(&done.report_json_as(false)).unwrap();
    for item in report["writer"]["photos_incomplete"].as_array().unwrap() {
        assert!(!item["reason"].as_str().unwrap().contains("/Users"));
    }
}
