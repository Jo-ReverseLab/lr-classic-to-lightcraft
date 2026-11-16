//! Acceptance tests for the neutral development-settings model (P6-1).
//!
//! The reader fills `Photo::develop`, the writer later consumes it. These
//! tests pin the shape a reader must be able to produce and the defaults a
//! photo without settings has:
//!
//! * `Develop` is empty by default: no controls, no partial merges, no
//!   curves, no crop, a white balance without mode or hint, no unmapped
//!   names, not deferred, no source hash.
//! * `Photo::new` sets `develop = None`; `with_develop` sets the settings.
//! * Equality includes the settings, so a re-sync can tell them apart.
//! * The merge-only values carry booleans and text; the white-balance hint
//!   distinguishes "no field", "Kelvin", "relative" and "undecidable".
//!
//! The types were added in P6-1; these tests pin their target shape.

use model::{
    CropGeometry, CurvePoint, Curves, Develop, MergeValue, Photo, Rating, WhiteBalance,
    WhiteBalanceHint,
};

#[test]
fn default_develop_is_empty() {
    let d = Develop::default();
    assert!(d.controls.is_empty());
    assert!(d.partial.is_empty());
    assert!(d.curves.master.is_empty());
    assert!(d.curves.red.is_empty());
    assert!(d.curves.green.is_empty());
    assert!(d.curves.blue.is_empty());
    assert!(d.crop.is_none());
    assert!(d.white_balance.mode.is_none());
    assert_eq!(d.white_balance.hint, WhiteBalanceHint::None);
    assert!(d.unmapped.is_empty());
    assert!(!d.deferred);
    assert!(d.source_hash.is_none());
}

#[test]
fn photo_new_has_no_develop_settings() {
    let photo = Photo::new("/photos/example.jpg", 7, Rating::default());
    assert!(photo.develop.is_none());
}

#[test]
fn with_develop_sets_the_settings() {
    let mut develop = Develop::default();
    develop.controls.insert("light.exposure".to_owned(), 1.25);
    develop.unmapped.push("MysterySlider".to_owned());

    let photo =
        Photo::new("/photos/example.jpg", 7, Rating::default()).with_develop(develop.clone());
    assert_eq!(photo.develop, Some(develop));
}

#[test]
fn photo_equality_includes_develop() {
    let mut with = Develop::default();
    with.controls.insert("light.contrast".to_owned(), 5.0);

    let plain = Photo::new("/photos/example.jpg", 7, Rating::default());
    let with_develop = plain.clone().with_develop(with);
    assert_ne!(plain, with_develop);
}

#[test]
fn controls_are_ordered_by_control_id() {
    let mut d = Develop::default();
    d.controls.insert("light.whites".to_owned(), 1.0);
    d.controls.insert("light.blacks".to_owned(), 2.0);
    d.controls.insert("color.vibrance".to_owned(), 3.0);

    let keys: Vec<&str> = d.controls.keys().map(String::as_str).collect();
    assert_eq!(keys, ["color.vibrance", "light.blacks", "light.whites"]);
}

#[test]
fn merge_values_carry_bool_and_text() {
    let mut d = Develop::default();
    d.partial
        .insert("treatment".to_owned(), MergeValue::Text("bw".to_owned()));
    d.partial
        .insert("hdr.enabled".to_owned(), MergeValue::Bool(true));
    d.partial
        .insert("optics.lens_profile".to_owned(), MergeValue::Bool(false));

    assert_eq!(
        d.partial.get("treatment"),
        Some(&MergeValue::Text("bw".to_owned()))
    );
    assert_eq!(d.partial.get("hdr.enabled"), Some(&MergeValue::Bool(true)));
    assert_eq!(
        d.partial.get("optics.lens_profile"),
        Some(&MergeValue::Bool(false))
    );
}

#[test]
fn white_balance_holds_an_optional_mode_and_a_hint() {
    let wb = WhiteBalance {
        mode: None,
        hint: WhiteBalanceHint::None,
        temperature: None,
        tint: None,
    };
    assert!(wb.mode.is_none());
    assert_eq!(wb.hint, WhiteBalanceHint::None);

    let custom = WhiteBalance {
        mode: Some("custom".to_owned()),
        hint: WhiteBalanceHint::Undecidable,
        temperature: Some(4800.0),
        tint: Some(-5.0),
    };
    assert_eq!(custom.mode.as_deref(), Some("custom"));
    assert_eq!(custom.hint, WhiteBalanceHint::Undecidable);
    assert_eq!(
        (custom.temperature, custom.tint),
        (Some(4800.0), Some(-5.0))
    );
}

#[test]
fn white_balance_hint_variants_are_distinct() {
    let hints = [
        WhiteBalanceHint::None,
        WhiteBalanceHint::Kelvin,
        WhiteBalanceHint::Relative,
        WhiteBalanceHint::Undecidable,
    ];
    for (i, a) in hints.iter().enumerate() {
        for (j, b) in hints.iter().enumerate() {
            assert_eq!(i == j, a == b, "{a:?} vs {b:?}");
        }
    }
}

#[test]
fn curves_start_empty() {
    let curves = Curves {
        master: Vec::new(),
        red: Vec::new(),
        green: Vec::new(),
        blue: Vec::new(),
    };
    assert!(curves.master.is_empty());
    assert!(curves.red.is_empty());
    assert!(curves.green.is_empty());
    assert!(curves.blue.is_empty());
}

#[test]
fn curves_keep_the_points_given() {
    let point = CurvePoint { x: 0.25, y: 0.5 };
    let curves = Curves {
        master: vec![point.clone()],
        red: vec![CurvePoint { x: 0.0, y: 0.0 }],
        green: Vec::new(),
        blue: vec![CurvePoint { x: 1.0, y: 1.0 }],
    };
    assert_eq!(curves.master, vec![point]);
    assert_eq!(curves.red.len(), 1);
    assert!(curves.green.is_empty());
    assert_eq!(curves.blue[0].y, 1.0);
}

#[test]
fn crop_geometry_keeps_all_coordinates() {
    let crop = CropGeometry {
        x0: 0.1,
        y0: 0.2,
        x1: 0.9,
        y1: 0.8,
        angle: 45.0,
    };
    assert_eq!(crop.x0, 0.1);
    assert_eq!(crop.y0, 0.2);
    assert_eq!(crop.x1, 0.9);
    assert_eq!(crop.y1, 0.8);
    assert_eq!(crop.angle, 45.0);
}

#[test]
fn source_hash_is_optional_and_kept() {
    let mut d = Develop::default();
    assert!(d.source_hash.is_none());
    d.source_hash = Some([7_u8; 32]);
    assert_eq!(d.source_hash, Some([7_u8; 32]));
}
