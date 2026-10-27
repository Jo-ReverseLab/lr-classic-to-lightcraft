//! Acceptance tests for the writer plan (task P5-4b), module `lc_writer::plan`.
//!
//! API under test (see also `tests/state.rs`):
//! - `WriteInput { photos, keywords, photo_keywords: BTreeMap<i64, Vec<Keyword>> }`
//!   (key = `Photo::lightroom_id`; missing ancestors are created implicitly);
//! - `plan(&WriteInput, &State, &BTreeSet<String>) -> Plan` with
//!   `Plan { steps, skipped }`, `Step::{CreateKeyword{path,params},
//!   Import{global_id,path}, Update{global_id,command,params}}`,
//!   `model::Skipped { reason: String }` (no own type);
//! - `Plan::to_script(&self, &State) -> String`: one JSON line per step,
//!   `{"method":"engine.execute","params":{"command":..,"params":..}}`; an
//!   update carries `ids` (`[<lightcraft_id>]` if known, else `["lr:<gid>"]`).
//!
//! Rules checked: new photo = import + non-default fields only; known photo =
//! only changed fields, at most one `photo.setMeta` (add/remove keywords and
//! text together); removed values are sent as "" / "none"; keywords are
//! created parents first and before all photo steps; photos without global id
//! are skipped; imports are exactly `{paths:[p], mode:"add"}`; the order is
//! deterministic (by global id).
//!
//! STATUS: waiting for P5-4b. The whole file is disabled by the crate-level
//! `cfg(any())` below. The developer may ONLY remove the `#![cfg(any())]`
//! line (and add the dependency on `model` and `serde` to `Cargo.toml`). Any
//! other change to this file needs the test author.

use lc_writer::plan::{Plan, Step, WriteInput, plan};
use lc_writer::state::{State, snapshot};
use model::{ColorLabel, Flag, Keyword, Photo, Rating};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

fn kw(parts: &[&str]) -> Keyword {
    Keyword::new(parts.iter().map(|s| s.to_string()).collect()).unwrap()
}

fn path_of(k: &Keyword) -> String {
    k.path().join("|")
}

fn photo(gid: &str, lr_id: i64, rating: u8) -> Photo {
    Photo::new(
        format!("/Users/example/Pictures/{gid}.jpg"),
        lr_id,
        Rating::new(rating).unwrap(),
    )
    .with_global_id(gid)
}

fn full_photo() -> Photo {
    photo("g-full", 1, 3)
        .with_flag(Flag::Pick)
        .with_label(ColorLabel::Purple)
        .with_title("Title")
        .with_caption("Caption")
        .with_copyright("(c) example")
        .with_creator("example")
}

fn input(photos: Vec<Photo>, per_photo: Vec<(i64, Vec<Keyword>)>) -> WriteInput {
    let mut keywords: Vec<Keyword> = Vec::new();
    for (_, ks) in &per_photo {
        for k in ks {
            if let Some(p) = k.parent()
                && !keywords.iter().any(|x| x.path() == p.path())
            {
                keywords.push(p);
            }
            if !keywords.iter().any(|x| x.path() == k.path()) {
                keywords.push(k.clone());
            }
        }
    }
    WriteInput {
        photos,
        keywords,
        photo_keywords: per_photo.into_iter().collect::<BTreeMap<_, _>>(),
    }
}

fn none() -> BTreeSet<String> {
    BTreeSet::new()
}

/// Records every photo of the input and every keyword, as an executor would.
fn recorded(inp: &WriteInput) -> State {
    let mut state = State::default();
    for (i, p) in inp.photos.iter().enumerate() {
        if let Some(gid) = &p.global_id {
            let ks = inp
                .photo_keywords
                .get(&p.lightroom_id)
                .cloned()
                .unwrap_or_default();
            state.record(gid, p.lightroom_id, json!(100 + i as i64), snapshot(p, &ks));
        }
    }
    for k in &inp.keywords {
        state.record_keyword(k);
    }
    state
}

/// The script lines as (command, params) pairs.
fn lines(plan: &Plan, state: &State) -> Vec<(String, Value)> {
    plan.to_script(state)
        .lines()
        .map(|l| {
            let v: Value = serde_json::from_str(l).expect("each line is valid JSON");
            assert_eq!(v["method"], json!("engine.execute"));
            (
                v["params"]["command"].as_str().unwrap().to_string(),
                v["params"]["params"].clone(),
            )
        })
        .collect()
}

fn updates(plan: &Plan) -> Vec<(&str, &Value)> {
    plan.steps
        .iter()
        .filter_map(|s| match s {
            Step::Update {
                command, params, ..
            } => Some((command.as_str(), params)),
            _ => None,
        })
        .collect()
}

/// Plans a change of one known photo: records `before`, plans for `after`.
fn change(before: Photo, after: Photo, ks_before: &[Keyword], ks_after: &[Keyword]) -> Plan {
    let b = input(vec![before], vec![(1, ks_before.to_vec())]);
    let state = recorded(&b);
    let a = input(vec![after], vec![(1, ks_after.to_vec())]);
    plan(&a, &state, &none())
}

#[test]
fn empty_state_three_photos_gives_the_expected_steps() {
    let k_parent = kw(&["Reisen"]);
    let k_child = kw(&["Reisen", "Italien"]);
    let inp = input(
        vec![
            photo("g-plain", 2, 0),
            full_photo(),
            Photo::new("/Users/example/Pictures/nogid.jpg", 3, Rating::default()),
        ],
        vec![(1, vec![k_child.clone()])],
    );
    let state = State::default();
    let p = plan(&inp, &state, &none());

    assert_eq!(p.skipped.len(), 1);
    assert!(!p.skipped[0].reason.is_empty());
    assert!(p.skipped[0].reason.contains('3'), "names the lightroom id");

    // Keywords first, parent before child, then photos ordered by global id.
    let kinds: Vec<&str> = p
        .steps
        .iter()
        .map(|s| match s {
            Step::CreateKeyword { .. } => "kw",
            Step::Import { .. } => "import",
            Step::Update { command, .. } => command.as_str(),
        })
        .collect();
    assert_eq!(
        kinds,
        vec![
            "kw",
            "kw",
            "import",
            "photo.rate",
            "photo.flag",
            "photo.label",
            "photo.setMeta",
            "import"
        ]
    );
    match (&p.steps[0], &p.steps[1]) {
        (
            Step::CreateKeyword {
                path: a,
                params: pa,
            },
            Step::CreateKeyword {
                path: b,
                params: pb,
            },
        ) => {
            assert_eq!(a, &path_of(&k_parent));
            assert_eq!(b, &path_of(&k_child));
            assert_eq!(pa["name"], json!("Reisen"));
            assert_eq!(pa["parent"], Value::Null);
            assert_eq!(pb["name"], json!("Italien"));
            assert_eq!(pb["parent"], json!("Reisen"));
        }
        other => panic!("unexpected {other:?}"),
    }
    match &p.steps[2] {
        Step::Import { global_id, path } => {
            assert_eq!(global_id, "g-full");
            assert_eq!(path, &full_photo().path);
        }
        other => panic!("unexpected {other:?}"),
    }
    match &p.steps[7] {
        Step::Import { global_id, .. } => assert_eq!(global_id, "g-plain"),
        other => panic!("unexpected {other:?}"),
    }

    let u = updates(&p);
    assert_eq!(u[0], ("photo.rate", &json!({"rating": 3})));
    assert_eq!(u[1], ("photo.flag", &json!({"flag": "pick"})));
    assert_eq!(u[2], ("photo.label", &json!({"label": "purple"})));
    assert_eq!(
        u[3],
        (
            "photo.setMeta",
            &json!({
                "title": "Title",
                "caption": "Caption",
                "copyright": "(c) example",
                "creator": "example",
                "addKeywords": ["Reisen|Italien"]
            })
        )
    );
}

#[test]
fn photo_with_default_values_is_only_imported() {
    let inp = input(vec![photo("g1", 1, 0)], vec![]);
    let p = plan(&inp, &State::default(), &none());
    assert_eq!(p.steps.len(), 1);
    assert!(matches!(p.steps[0], Step::Import { .. }));
    assert!(p.skipped.is_empty());
}

#[test]
fn reject_flag_and_each_label_are_spelled_as_the_app_expects() {
    let cases = [
        (ColorLabel::Red, "red"),
        (ColorLabel::Yellow, "yellow"),
        (ColorLabel::Green, "green"),
        (ColorLabel::Blue, "blue"),
        (ColorLabel::Purple, "purple"),
    ];
    for (label, text) in cases {
        let inp = input(vec![photo("g1", 1, 0).with_label(label)], vec![]);
        let p = plan(&inp, &State::default(), &none());
        assert_eq!(
            updates(&p),
            vec![("photo.label", &json!({ "label": text }))]
        );
    }
    let inp = input(vec![photo("g1", 1, 0).with_flag(Flag::Reject)], vec![]);
    let p = plan(&inp, &State::default(), &none());
    assert_eq!(
        updates(&p),
        vec![("photo.flag", &json!({"flag": "reject"}))]
    );
}

#[test]
fn second_plan_after_recording_everything_is_empty() {
    let ks = vec![kw(&["Reisen", "Italien"]), kw(&["Zebra"])];
    let inp = input(
        vec![
            full_photo(),
            photo("g-plain", 2, 0),
            Photo::new("/Users/example/Pictures/nogid.jpg", 3, Rating::default()),
        ],
        vec![(1, ks)],
    );
    let first = plan(&inp, &State::default(), &none());
    assert!(!first.steps.is_empty());
    let state = recorded(&inp);
    let second = plan(&inp, &state, &none());
    assert_eq!(second.steps.len(), 0, "{:?}", second.steps);
    assert_eq!(second.to_script(&state).lines().count(), 0);
}

#[test]
fn second_plan_is_empty_after_the_state_went_through_a_file() {
    let inp = input(vec![full_photo()], vec![(1, vec![kw(&["A", "B"])])]);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("s.lr2lc.json");
    recorded(&inp).save(&file).unwrap();
    let state = State::load(&file).unwrap();
    assert_eq!(plan(&inp, &state, &none()).steps.len(), 0);
}

#[test]
fn known_photo_never_gets_an_import() {
    let inp = input(vec![full_photo()], vec![]);
    let state = recorded(&inp);
    let changed = input(vec![full_photo().with_title("Other")], vec![]);
    let p = plan(&changed, &state, &none());
    assert!(!p.steps.iter().any(|s| matches!(s, Step::Import { .. })));
}

#[test]
fn changed_rating_gives_exactly_one_rate_command() {
    let p = change(
        full_photo(),
        {
            let mut x = full_photo();
            x.rating = Rating::new(5).unwrap();
            x
        },
        &[],
        &[],
    );
    assert_eq!(p.steps.len(), 1);
    assert_eq!(updates(&p), vec![("photo.rate", &json!({"rating": 5}))]);
}

#[test]
fn rating_reset_to_zero_is_sent() {
    let mut x = full_photo();
    x.rating = Rating::default();
    let p = change(full_photo(), x, &[], &[]);
    assert_eq!(updates(&p), vec![("photo.rate", &json!({"rating": 0}))]);
}

#[test]
fn changed_flag_gives_one_flag_command() {
    let p = change(full_photo(), full_photo().with_flag(Flag::Reject), &[], &[]);
    assert_eq!(p.steps.len(), 1);
    assert_eq!(
        updates(&p),
        vec![("photo.flag", &json!({"flag": "reject"}))]
    );
}

#[test]
fn removed_flag_is_sent_as_none() {
    let p = change(full_photo(), full_photo().with_flag(Flag::None), &[], &[]);
    assert_eq!(p.steps.len(), 1);
    assert_eq!(updates(&p), vec![("photo.flag", &json!({"flag": "none"}))]);
}

#[test]
fn removed_label_is_sent_as_none() {
    let mut x = full_photo();
    x.label = None;
    let p = change(full_photo(), x, &[], &[]);
    assert_eq!(p.steps.len(), 1);
    assert_eq!(
        updates(&p),
        vec![("photo.label", &json!({"label": "none"}))]
    );
}

#[test]
fn changed_title_gives_one_setmeta_with_only_the_title() {
    let p = change(full_photo(), full_photo().with_title("New"), &[], &[]);
    assert_eq!(p.steps.len(), 1);
    assert_eq!(
        updates(&p),
        vec![("photo.setMeta", &json!({"title": "New"}))]
    );
}

#[test]
fn removed_text_is_set_to_the_empty_string() {
    let mut x = full_photo();
    x.caption = None;
    let p = change(full_photo(), x, &[], &[]);
    assert_eq!(p.steps.len(), 1);
    assert_eq!(
        updates(&p),
        vec![("photo.setMeta", &json!({"caption": ""}))]
    );
}

#[test]
fn each_text_field_is_compared_on_its_own() {
    let cases: Vec<(Photo, &str, &str)> = vec![
        (full_photo().with_caption("c2"), "caption", "c2"),
        (full_photo().with_copyright("c2"), "copyright", "c2"),
        (full_photo().with_creator("c2"), "creator", "c2"),
    ];
    for (after, key, value) in cases {
        let p = change(full_photo(), after, &[], &[]);
        assert_eq!(p.steps.len(), 1, "{key}");
        assert_eq!(updates(&p), vec![("photo.setMeta", &json!({ key: value }))]);
    }
}

#[test]
fn added_keyword_gives_one_setmeta_with_addkeywords() {
    let a = kw(&["A"]);
    let b = kw(&["B"]);
    let b_state = input(vec![full_photo()], vec![(1, vec![a.clone()])]);
    let state = recorded(&b_state);
    let after = input(vec![full_photo()], vec![(1, vec![a, b])]);
    let p = plan(&after, &state, &none());
    // `B` is new for the library as well, so it is created first.
    let metas: Vec<_> = updates(&p);
    assert_eq!(
        metas,
        vec![("photo.setMeta", &json!({"addKeywords": ["B"]}))]
    );
    assert_eq!(
        p.steps
            .iter()
            .filter(|s| matches!(s, Step::CreateKeyword { .. }))
            .count(),
        1
    );
}

#[test]
fn removed_keyword_gives_removekeywords() {
    let p = change(
        full_photo(),
        full_photo(),
        &[kw(&["A"]), kw(&["B"])],
        &[kw(&["A"])],
    );
    assert_eq!(p.steps.len(), 1);
    assert_eq!(
        updates(&p),
        vec![("photo.setMeta", &json!({"removeKeywords": ["B"]}))]
    );
}

#[test]
fn one_keyword_added_and_one_removed_make_a_single_setmeta() {
    let p = change(
        full_photo(),
        full_photo(),
        &[kw(&["A"]), kw(&["B"])],
        &[kw(&["A"]), kw(&["C"])],
    );
    let u = updates(&p);
    assert_eq!(u.len(), 1);
    assert_eq!(
        u[0],
        (
            "photo.setMeta",
            &json!({"addKeywords": ["C"], "removeKeywords": ["B"]})
        )
    );
}

#[test]
fn keyword_changes_and_text_changes_share_one_setmeta() {
    let p = change(
        full_photo(),
        full_photo().with_title("New"),
        &[kw(&["A"])],
        &[kw(&["B"])],
    );
    let u = updates(&p);
    assert_eq!(u.len(), 1);
    assert_eq!(
        u[0],
        (
            "photo.setMeta",
            &json!({"title": "New", "addKeywords": ["B"], "removeKeywords": ["A"]})
        )
    );
}

#[test]
fn keyword_hierarchy_is_sent_as_path_text() {
    let p = change(full_photo(), full_photo(), &[], &[kw(&["A", "B", "C"])]);
    assert_eq!(
        updates(&p),
        vec![("photo.setMeta", &json!({"addKeywords": ["A|B|C"]}))]
    );
}

#[test]
fn keyword_already_in_the_library_is_not_created() {
    let inp = input(
        vec![full_photo()],
        vec![(1, vec![kw(&["Reisen", "Italien"])])],
    );
    let existing: BTreeSet<String> = ["Reisen".to_string(), "Reisen|Italien".to_string()].into();
    let p = plan(&inp, &State::default(), &existing);
    assert!(
        !p.steps
            .iter()
            .any(|s| matches!(s, Step::CreateKeyword { .. }))
    );
    // The photo still gets the keyword.
    assert!(
        updates(&p)
            .iter()
            .any(|(c, v)| *c == "photo.setMeta" && v["addKeywords"] == json!(["Reisen|Italien"]))
    );
}

#[test]
fn only_the_missing_child_is_created_when_the_parent_exists() {
    let inp = input(vec![], vec![]);
    let inp = WriteInput {
        keywords: vec![kw(&["Reisen"]), kw(&["Reisen", "Italien"])],
        ..inp
    };
    let existing: BTreeSet<String> = ["Reisen".to_string()].into();
    let p = plan(&inp, &State::default(), &existing);
    assert_eq!(p.steps.len(), 1);
    match &p.steps[0] {
        Step::CreateKeyword { path, params } => {
            assert_eq!(path, "Reisen|Italien");
            assert_eq!(params["parent"], json!("Reisen"));
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn keyword_known_only_to_the_state_is_not_created() {
    let inp = WriteInput {
        keywords: vec![kw(&["A"])],
        ..input(vec![], vec![])
    };
    let mut state = State::default();
    state.record_keyword(&kw(&["A"]));
    assert!(plan(&inp, &state, &none()).steps.is_empty());
}

#[test]
fn keyword_used_only_by_a_photo_is_created() {
    let mut inp = input(vec![full_photo()], vec![(1, vec![kw(&["Solo"])])]);
    inp.keywords.clear();
    let p = plan(&inp, &State::default(), &none());
    assert!(matches!(&p.steps[0], Step::CreateKeyword { path, .. } if path == "Solo"));
}

#[test]
fn create_keyword_carries_synonyms_and_attributes() {
    let mut k = kw(&["Personen", "Anna"]);
    k.synonyms = vec!["Ännchen".into(), "A.".into()];
    k.include_on_export = false;
    k.export_containing = false;
    k.export_synonyms = false;
    k.person = true;
    let inp = WriteInput {
        keywords: vec![kw(&["Personen"]), k],
        ..input(vec![], vec![])
    };
    let p = plan(&inp, &State::default(), &none());
    match &p.steps[1] {
        Step::CreateKeyword { params, .. } => assert_eq!(
            params,
            &json!({
                "name": "Anna",
                "parent": "Personen",
                "synonyms": ["Ännchen", "A."],
                "includeOnExport": false,
                "exportContaining": false,
                "exportSynonyms": false,
                "person": true
            })
        ),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn deep_hierarchy_is_created_parent_first_even_if_listed_backwards() {
    let inp = WriteInput {
        keywords: vec![
            kw(&["A", "B", "C", "D"]),
            kw(&["A", "B"]),
            kw(&["A"]),
            kw(&["A", "B", "C"]),
        ],
        ..input(vec![], vec![])
    };
    let p = plan(&inp, &State::default(), &none());
    let order: Vec<&str> = p
        .steps
        .iter()
        .map(|s| match s {
            Step::CreateKeyword { path, .. } => path.as_str(),
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(order, vec!["A", "A|B", "A|B|C", "A|B|C|D"]);
}

#[test]
fn all_keywords_are_created_before_any_photo_step() {
    let inp = input(
        vec![photo("g1", 1, 2), photo("g2", 2, 0)],
        vec![(2, vec![kw(&["K"])])],
    );
    let p = plan(&inp, &State::default(), &none());
    let first_photo = p
        .steps
        .iter()
        .position(|s| !matches!(s, Step::CreateKeyword { .. }))
        .unwrap();
    assert!(
        p.steps[first_photo..]
            .iter()
            .all(|s| !matches!(s, Step::CreateKeyword { .. }))
    );
}

#[test]
fn changed_attributes_of_a_created_keyword_are_only_counted() {
    let mut state = State::default();
    state.record_keyword(&kw(&["Geheim-Name"]));
    let mut changed = kw(&["Geheim-Name"]);
    changed.synonyms = vec!["Alias".into()];
    let inp = WriteInput {
        keywords: vec![changed],
        ..input(vec![], vec![])
    };
    let p = plan(&inp, &state, &none());
    assert!(p.steps.is_empty());
    assert_eq!(p.skipped.len(), 1);
    let reason = &p.skipped[0].reason;
    assert!(!reason.is_empty());
    assert!(!reason.contains("Geheim-Name") && !reason.contains("Alias"));
}

#[test]
fn photo_without_global_id_is_skipped_with_its_lightroom_id_only() {
    let mut p0 = Photo::new(
        "/Users/example/Pictures/secret-name.jpg",
        4711,
        Rating::new(5).unwrap(),
    );
    p0.title = Some("Secret title".into());
    let inp = input(vec![p0], vec![]);
    let p = plan(&inp, &State::default(), &none());
    assert!(p.steps.is_empty());
    assert_eq!(p.skipped.len(), 1);
    let reason = &p.skipped[0].reason;
    assert!(reason.contains("4711"));
    assert!(!reason.contains("secret-name") && !reason.contains("Secret"));
}

#[test]
fn read_equals_planned_plus_skipped() {
    let inp = input(
        vec![
            photo("g1", 1, 1),
            Photo::new("/Users/example/Pictures/n1.jpg", 2, Rating::default()),
            photo("g3", 3, 0),
            Photo::new("/Users/example/Pictures/n2.jpg", 4, Rating::default()),
        ],
        vec![],
    );
    let p = plan(&inp, &State::default(), &none());
    let imports = p
        .steps
        .iter()
        .filter(|s| matches!(s, Step::Import { .. }))
        .count();
    assert_eq!(imports + p.skipped.len(), inp.photos.len());
}

#[test]
fn import_parameters_are_exactly_paths_and_mode_add() {
    let inp = input(vec![full_photo(), photo("g2", 2, 0)], vec![]);
    let state = State::default();
    let p = plan(&inp, &state, &none());
    let imports: Vec<_> = lines(&p, &state)
        .into_iter()
        .filter(|(c, _)| c == "library.import")
        .collect();
    assert_eq!(imports.len(), 2, "one import per photo");
    for (_, params) in imports {
        let obj = params.as_object().unwrap();
        assert_eq!(obj.len(), 2, "{params}");
        assert_eq!(params["mode"], json!("add"));
        assert_eq!(params["paths"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn no_forbidden_import_parameter_appears_anywhere() {
    const FORBIDDEN: [&str; 9] = [
        "copy",
        "move",
        "destination",
        "organize",
        "rename",
        "dng",
        "local",
        "preset",
        "metadataPreset",
    ];
    let inp = input(
        vec![full_photo(), photo("g2", 2, 1)],
        vec![(1, vec![kw(&["A", "B"])])],
    );
    let state = State::default();
    let p = plan(&inp, &state, &none());
    let script = p.to_script(&state);
    for (command, params) in lines(&p, &state) {
        if command == "library.import" {
            for key in FORBIDDEN.iter().chain(&["keywords"]) {
                assert!(params.get(*key).is_none(), "import has {key}");
            }
        }
        assert_ne!(params["mode"], json!("copy"));
        assert_ne!(params["mode"], json!("move"));
    }
    assert!(!script.contains("\"copy\"") && !script.contains("\"move\""));
}

#[test]
fn update_params_never_contain_ids() {
    let inp = input(vec![full_photo()], vec![(1, vec![kw(&["A"])])]);
    let p = plan(&inp, &State::default(), &none());
    for (_, params) in updates(&p) {
        assert!(params.get("ids").is_none());
    }
}

#[test]
fn script_has_one_valid_json_line_per_step() {
    let inp = input(vec![full_photo()], vec![(1, vec![kw(&["A", "B"])])]);
    let state = State::default();
    let p = plan(&inp, &state, &none());
    let script = p.to_script(&state);
    assert_eq!(script.lines().count(), p.steps.len());
    assert!(script.lines().all(|l| !l.trim().is_empty()));
    let l = lines(&p, &state);
    assert_eq!(l[0].0, "keyword.create");
    assert_eq!(l[2].0, "library.import");
    assert_eq!(
        l[2].1["paths"],
        json!([full_photo().path.to_str().unwrap()])
    );
}

#[test]
fn script_uses_a_placeholder_id_for_new_photos() {
    let inp = input(vec![full_photo()], vec![]);
    let state = State::default();
    let script = plan(&inp, &state, &none()).to_script(&state);
    let mut found = 0;
    for l in script.lines() {
        let v: Value = serde_json::from_str(l).unwrap();
        let params = &v["params"]["params"];
        if v["params"]["command"] != json!("library.import") {
            assert_eq!(params["ids"], json!(["lr:g-full"]));
            found += 1;
        }
    }
    assert_eq!(found, 4);
}

#[test]
fn script_uses_the_real_id_for_known_photos() {
    let inp = input(vec![full_photo()], vec![]);
    let state = recorded(&inp);
    let changed = input(vec![full_photo().with_title("New")], vec![]);
    let p = plan(&changed, &state, &none());
    let l = lines(&p, &state);
    assert_eq!(l.len(), 1);
    assert_eq!(l[0].0, "photo.setMeta");
    assert_eq!(l[0].1["ids"], json!([100]));
    assert_eq!(l[0].1["title"], json!("New"));
}

#[test]
fn script_keeps_the_type_of_a_text_lightcraft_id() {
    let p0 = full_photo();
    let mut state = State::default();
    state.record("g-full", 1, json!("abc-1"), snapshot(&p0, &[]));
    let p = plan(&input(vec![p0.with_title("X")], vec![]), &state, &none());
    assert_eq!(lines(&p, &state)[0].1["ids"], json!(["abc-1"]));
}

#[test]
fn order_is_deterministic_and_by_global_id() {
    let a = photo("a", 1, 1);
    let b = photo("b", 2, 2);
    let c = photo("c", 3, 3);
    let ks = vec![
        (1, vec![kw(&["K1"])]),
        (2, vec![kw(&["K2"])]),
        (3, vec![kw(&["K3"])]),
    ];
    let state = State::default();
    let s1 = plan(
        &input(vec![a.clone(), b.clone(), c.clone()], ks.clone()),
        &state,
        &none(),
    )
    .to_script(&state);
    let s2 = plan(&input(vec![c, a, b], ks), &state, &none()).to_script(&state);
    assert_eq!(s1, s2);
    let import_order: Vec<String> = s1
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .filter(|v| v["params"]["command"] == json!("library.import"))
        .map(|v| {
            v["params"]["params"]["paths"][0]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(
        import_order,
        vec![
            "/Users/example/Pictures/a.jpg",
            "/Users/example/Pictures/b.jpg",
            "/Users/example/Pictures/c.jpg"
        ]
    );
}

#[test]
fn umlaut_and_emoji_survive_byte_for_byte_in_plan_and_script() {
    let title = "Grüße aus Köln 🎉";
    let k = kw(&["Reisen", "Köln 🌅"]);
    let inp = input(
        vec![photo("g1", 1, 0).with_title(title)],
        vec![(1, vec![k])],
    );
    let state = State::default();
    let p = plan(&inp, &state, &none());
    let meta = updates(&p)
        .into_iter()
        .find(|(c, _)| *c == "photo.setMeta")
        .unwrap()
        .1
        .clone();
    assert_eq!(meta["title"].as_str().unwrap().as_bytes(), title.as_bytes());
    assert_eq!(
        meta["addKeywords"][0].as_str().unwrap().as_bytes(),
        "Reisen|Köln 🌅".as_bytes()
    );
    // The script itself parses back to the same bytes.
    let script = p.to_script(&state);
    assert!(script.contains("Köln") || script.contains("K\\u00f6ln"));
    let parsed: Vec<Value> = script
        .lines()
        .map(|x| serde_json::from_str(x).unwrap())
        .collect();
    assert!(
        parsed
            .iter()
            .any(|v| v["params"]["params"]["title"] == json!(title))
    );
    assert!(
        parsed
            .iter()
            .any(|v| v["params"]["params"]["name"] == json!("Köln 🌅"))
    );
}

#[test]
fn unchanged_text_with_umlaut_gives_no_command() {
    let a = photo("g1", 1, 0).with_title("Grüße 🎉");
    let ks = vec![kw(&["Köln 🌅"])];
    let inp = input(vec![a], vec![(1, ks)]);
    let state = recorded(&inp);
    assert!(plan(&inp, &state, &none()).steps.is_empty());
}

#[test]
fn planning_does_not_modify_the_state() {
    let inp = input(vec![full_photo()], vec![(1, vec![kw(&["A"])])]);
    let state = State::default();
    let before = state.clone();
    let _ = plan(&inp, &state, &none());
    assert_eq!(state, before);
}

#[test]
fn missing_ancestors_are_created_implicitly_with_default_attributes() {
    let child = kw(&["a", "b"]);
    let inp = WriteInput {
        photos: vec![photo("g1", 1, 0)],
        keywords: vec![],
        photo_keywords: BTreeMap::from([(1, vec![child])]),
    };
    let p = plan(&inp, &State::default(), &none());
    let created: Vec<(&str, &Value)> = p
        .steps
        .iter()
        .filter_map(|s| match s {
            Step::CreateKeyword { path, params } => Some((path.as_str(), params)),
            _ => None,
        })
        .collect();
    assert_eq!(created.len(), 2);
    assert_eq!(created[0].0, "a");
    assert_eq!(created[1].0, "a|b");
    assert_eq!(
        created[0].1,
        &json!({
            "name": "a",
            "parent": null,
            "synonyms": [],
            "includeOnExport": true,
            "exportContaining": true,
            "exportSynonyms": true,
            "person": false
        })
    );
    assert_eq!(created[1].1["parent"], json!("a"));
}

#[test]
fn ancestor_known_to_the_library_or_state_is_not_created_again() {
    let inp = WriteInput {
        photos: vec![],
        keywords: vec![],
        photo_keywords: BTreeMap::from([(1, vec![kw(&["a", "b", "c"])])]),
    };
    let existing: BTreeSet<String> = ["a".to_string()].into();
    let mut state = State::default();
    state.record_keyword(&kw(&["a", "b"]));
    let p = plan(&inp, &state, &existing);
    let paths: Vec<&str> = p
        .steps
        .iter()
        .filter_map(|s| match s {
            Step::CreateKeyword { path, .. } => Some(path.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(paths, vec!["a|b|c"]);
}
