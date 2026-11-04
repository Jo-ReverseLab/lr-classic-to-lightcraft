//! Builders for synthetic `Adobe_imageDevelopSettings` rows.
//!
//! The settings text is the Lua table literal Lightroom Classic stores in the
//! `text` column. Every field name and every value written here is invented;
//! nothing comes from a real catalog. The builders only produce data; they do
//! not parse or execute it.

use super::{create_original_files, prepare_root_dirs, write_small};
use rusqlite::{Connection, Result, params};
use std::path::{Path, PathBuf};

/// The optional catalog table that holds one development-settings text per
/// image: `id_local`, `image`, `text` (plus `id_global`, which is not read).
pub const TABLE: &str = "Adobe_imageDevelopSettings";

/// Photo ids and fixed values of the develop fixtures.
///
/// Every name and value is invented. A fixture that is not listed here uses no
/// develop rows at all.
pub mod edge {
    /// Photo carrying every mapped 1:1 slider (`write_all_sliders_catalog_with_files`).
    pub const ALL_SLIDERS_PHOTO: i64 = 4;
    /// Photo with old-process fields only (`write_old_process_catalog_with_files`).
    pub const OLD_PROCESS_PHOTO: i64 = 4;
    /// Photo with old-process fields plus one `*2012` key: the old block must
    /// be ignored.
    pub const OLD_SUPPRESSED_PHOTO: i64 = 5;
    /// Input values of the old-process fixture.
    pub const OLD_EXPOSURE: f64 = 1.25;
    pub const OLD_CONTRAST: f64 = 30.0;
    pub const OLD_HIGHLIGHT_RECOVERY: f64 = 40.0;
    pub const OLD_FILL_LIGHT: f64 = 15.0;
    pub const OLD_SHADOWS: f64 = 10.0;
    pub const OLD_BRIGHTNESS: f64 = 60.0;
    pub const OLD_CLARITY: f64 = 20.0;
    /// The `*2012` value of [`OLD_SUPPRESSED_PHOTO`]; it alone maps.
    pub const OLD_SUPPRESSED_EXPOSURE2012: f64 = 1.0;

    /// Merge-only fixture: all switches on, with numeric and boolean spellings.
    pub const MERGE_ON_PHOTO: i64 = 4;
    /// Merge-only fixture: all switches off, with numeric and boolean spellings.
    pub const MERGE_OFF_PHOTO: i64 = 5;
    /// Merge-only fixture: `LensProfileEnable = 0`, which must not put a value.
    pub const MERGE_DISABLED_PHOTO: i64 = 6;

    /// Photo whose settings text contains the `-999999` sentinel at top level.
    pub const DEFERRED_PHOTO: i64 = 8;
    /// Photo whose only sentinel sits inside a nested table.
    pub const NESTED_SENTINEL_PHOTO: i64 = 9;
    /// The value that must never reach the model.
    pub const SENTINEL: f64 = -999999.0;
    /// Valid value next to the sentinel; it must survive.
    pub const DEFERRED_CONTRAST: f64 = 12.5;

    /// Photo with unknown and still-assumed keys.
    pub const UNMAPPED_PHOTO: i64 = 4;
    /// The keys of the unmapped fixture, in the order the text writes them.
    /// None of them has a counterpart in P6-1.
    pub const UNMAPPED_NAMES: [&str; 10] = [
        "RedEyeInfo",
        "LensBlur",
        "PointColors",
        "ca_red",
        "ca_blue",
        "enhance.denoise",
        "CropFlipHorizontal",
        "MysterySlider",
        "MysteryText",
        "SDRBlend",
    ];
    /// Invented numeric value attached to [`MysterySlider`](Self::UNMAPPED_NAMES).
    pub const UNMAPPED_NUMBER: f64 = 777.25;
    /// Invented text value attached to `MysteryText`.
    pub const UNMAPPED_TEXT: &str = "ZyxSecretValue";

    /// Broken text (unbalanced table), string where a number belongs and
    /// oversized text. The photos stay in the catalog.
    pub const INVALID_UNBALANCED_PHOTO: i64 = 5;
    pub const INVALID_MISSING_VALUE_PHOTO: i64 = 6;
    pub const OVERSIZED_PHOTO: i64 = 7;
    /// A valid row after the broken ones; it proves the run continues.
    pub const VALID_AFTER_INVALID_PHOTO: i64 = 8;

    /// Photo whose row has an empty `text`.
    pub const EMPTY_TEXT_PHOTO: i64 = 7;

    /// Virtual copy fixture: master 3 and copy 20 have different settings.
    pub const VIRTUAL_MASTER_PHOTO: i64 = 3;
    pub const VIRTUAL_COPY_PHOTO: i64 = 20;
    pub const VIRTUAL_MASTER_EXPOSURE: f64 = 1.0;
    pub const VIRTUAL_MASTER_CONTRAST: f64 = 2.0;
    pub const VIRTUAL_COPY_EXPOSURE: f64 = 3.0;
    pub const VIRTUAL_COPY_VIBRANCE: f64 = 4.0;

    /// White-balance fixture: `(photo, stored WhiteBalance, expected mode)`.
    /// The expected modes use LightCraft's `wb.mode` spelling.
    pub const WB_MODES: [(i64, &str, &str); 8] = [
        (1, "As Shot", "asShot"),
        (2, "Auto", "auto"),
        (3, "Daylight", "daylight"),
        (4, "Cloudy", "cloudy"),
        (5, "Shade", "shade"),
        (7, "Tungsten", "tungsten"),
        (8, "Fluorescent", "fluorescent"),
        (9, "Flash", "flash"),
    ];
    /// `Custom` is not one of the named modes and maps to `custom`.
    pub const WB_CUSTOM_PHOTO: i64 = 10;
    /// An invented mode name maps to `custom`.
    pub const WB_UNKNOWN_PHOTO: i64 = 11;
    /// A row without any white-balance field.
    pub const WB_NONE_PHOTO: i64 = 12;
    /// Photo 4 also stores a Kelvin/tint pair: reported, never mapped.
    pub const WB_ABSOLUTE_PHOTO: i64 = 4;
    pub const WB_TEMPERATURE: f64 = 5500.0;
    pub const WB_TINT: f64 = 10.0;
    /// Photo 5 also stores the incremental pair: relative scale hint.
    pub const WB_RELATIVE_PHOTO: i64 = 5;
    pub const WB_INCREMENTAL_TEMPERATURE: f64 = -20.0;
    pub const WB_INCREMENTAL_TINT: f64 = 5.0;
    pub const WB_UNMAPPED_ABSOLUTE: [&str; 2] = ["Temperature", "Tint"];
    pub const WB_UNMAPPED_RELATIVE: [&str; 2] = ["IncrementalTemperature", "IncrementalTint"];

    /// Balance fixture: one row for every photo of the small fixture.
    pub const BALANCE_PHOTOS: i64 = 20;
    /// Rows that are not readable: unbalanced text and a missing value.
    pub const BALANCE_INVALID: [i64; 2] = [5, 6];
    /// A row with an empty `text`: `Some(Develop::default())`.
    pub const BALANCE_EMPTY_TEXT: i64 = 7;
    /// A row with the sentinel: still mapped, marked deferred.
    pub const BALANCE_DEFERRED: i64 = 8;
}

/// Tone-curve fixtures (P6-3). All point lists are invented and use
/// Lightroom's stored 0..255 scale, as pairs `x, y`.
pub mod curve {
    /// Master curve only (`ToneCurvePV2012`).
    pub const MASTER_PHOTO: i64 = 4;
    /// Master curve plus one channel curve (`ToneCurvePV2012Red`).
    pub const ONE_CHANNEL_PHOTO: i64 = 5;
    /// Master curve plus all three channel curves.
    pub const ALL_CHANNELS_PHOTO: i64 = 6;
    /// A straight 0→255 identity line: no curve.
    pub const IDENTITY_PHOTO: i64 = 7;
    /// Old `ToneCurve` without a 2012 key.
    pub const OLD_PHOTO: i64 = 8;
    /// Old `ToneCurve` plus `ToneCurvePV2012`: the 2012 curve wins.
    pub const OLD_AND_NEW_PHOTO: i64 = 9;
    /// A list with an odd number of values: the row is invalid.
    pub const ODD_LIST_PHOTO: i64 = 10;
    /// A list with a string where a number belongs: the row is invalid.
    pub const BROKEN_POINT_PHOTO: i64 = 11;
    /// All three channel curves but no master: no channel curve is read.
    pub const CHANNELS_WITHOUT_MASTER_PHOTO: i64 = 12;

    /// Master points in stored coordinates, `x1, y1, x2, y2, …`.
    pub const MASTER_255: [i64; 8] = [0, 0, 64, 96, 192, 240, 255, 255];
    /// Red channel points.
    pub const RED_255: [i64; 6] = [0, 0, 128, 120, 255, 255];
    /// Green channel points.
    pub const GREEN_255: [i64; 4] = [0, 0, 255, 240];
    /// Blue channel points.
    pub const BLUE_255: [i64; 4] = [0, 30, 255, 255];
    /// Old `ToneCurve` points (different from [`NEW_255`]).
    pub const OLD_255: [i64; 6] = [0, 0, 128, 140, 255, 255];
    /// `ToneCurvePV2012` points of the old-and-new fixture.
    pub const NEW_255: [i64; 6] = [0, 10, 128, 150, 255, 240];
    /// The identity line `(0,0)–(255,255)`.
    pub const IDENTITY_255: [i64; 4] = [0, 0, 255, 255];
}

/// Crop fixtures (P6-3). The rectangle is invented; the orientation codes are
/// Lightroom's two-letter codes (`AB` upright, `BA` mirrored horizontally).
pub mod crop {
    /// Upright photo with a stored crop and angle 0. Its `orientation` stays
    /// NULL, which must be read as upright.
    pub const PLAIN_PHOTO: i64 = 4;
    /// Upright photo with a positive stored angle.
    pub const ANGLE_POSITIVE_PHOTO: i64 = 5;
    /// Upright photo with a negative stored angle.
    pub const ANGLE_NEGATIVE_PHOTO: i64 = 6;
    /// Only `CropAngle`, no edges: a crop with the unit rectangle.
    pub const ANGLE_ONLY_PHOTO: i64 = 7;
    /// No crop field at all.
    pub const NONE_PHOTO: i64 = 8;
    /// Mirrored horizontally (`BA`) with a stored angle.
    pub const FLIP_H_ANGLE_PHOTO: i64 = 9;
    /// Quarter turn clockwise (`BC`) with angle 0.
    pub const ROTATE90_PHOTO: i64 = 10;
    /// Mirrored horizontally (`BA`) with angle 0.
    pub const FLIP_H_PHOTO: i64 = 11;
    /// Crop plus the unverified aspect/flip keys, which are reported.
    pub const ASSUMPTIONS_PHOTO: i64 = 12;

    /// Stored edges of [`PLAIN_PHOTO`].
    pub const LEFT: f64 = 0.1;
    pub const TOP: f64 = 0.2;
    pub const RIGHT: f64 = 0.9;
    pub const BOTTOM: f64 = 0.8;
    /// Stored edges of the orientation fixtures. Chosen so that the reference
    /// rule (`lightroom_catalog.rs:737-765`) gives clean expected values.
    pub const ORIENT_LEFT: f64 = 0.1;
    pub const ORIENT_TOP: f64 = 0.2;
    pub const ORIENT_RIGHT: f64 = 0.6;
    pub const ORIENT_BOTTOM: f64 = 0.9;
    /// `fileWidth` / `fileHeight` of the oriented fixtures.
    pub const STORED_WIDTH: i64 = 1500;
    pub const STORED_HEIGHT: i64 = 1000;
    /// Stored `CropAngle` values.
    pub const POSITIVE_ANGLE: f64 = 5.0;
    pub const NEGATIVE_ANGLE: f64 = -3.5;
    pub const ANGLE_ONLY: f64 = -1.5;
    pub const FLIP_ANGLE: f64 = 2.0;
    /// Keys that stay assumptions and are reported, never mapped.
    pub const UNMAPPED_ASSUMPTIONS: [&str; 3] = [
        "CropConstrainAspectRatio",
        "CropFlipHorizontal",
        "CropFlipVertical",
    ];
}

/// A Lua positional array of numbers: `{ 0, 0, 255, 255 }`.
pub fn lua_list(values: &[i64]) -> String {
    let items: Vec<String> = values.iter().map(|v| v.to_string()).collect();
    format!("{{ {} }}", items.join(", "))
}

/// One value of the generated Lua table.
#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    /// A Lua number written like `1.25`.
    Number(f64),
    /// A whole number written without a decimal point.
    Integer(i64),
    /// `true` or `false`.
    Boolean(bool),
    /// A quoted Lua string.
    Text(String),
    /// A ready-made Lua expression, e.g. an array `{ 0, 0, 255, 255 }`.
    Raw(String),
}

/// A Lua table literal builder for one image's settings text.
///
/// Fields keep their insertion order, so a fixture and its expected unmapped
/// list line up. Values are written deterministically.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Settings {
    fields: Vec<(String, Field)>,
}

impl Settings {
    /// Adds a field with an explicit value.
    pub fn field(mut self, key: &str, field: Field) -> Self {
        self.fields.push((key.to_owned(), field));
        self
    }

    /// Adds a number.
    pub fn number(self, key: &str, value: f64) -> Self {
        self.field(key, Field::Number(value))
    }

    /// Adds a whole number.
    pub fn integer(self, key: &str, value: i64) -> Self {
        self.field(key, Field::Integer(value))
    }

    /// Adds a boolean.
    pub fn boolean(self, key: &str, value: bool) -> Self {
        self.field(key, Field::Boolean(value))
    }

    /// Adds a quoted string.
    pub fn text(self, key: &str, value: &str) -> Self {
        self.field(key, Field::Text(value.to_owned()))
    }

    /// Adds a ready-made Lua expression.
    pub fn raw(self, key: &str, value: &str) -> Self {
        self.field(key, Field::Raw(value.to_owned()))
    }

    /// The complete `text` value: `s = { … }` with one field per line.
    pub fn build(&self) -> String {
        let mut out = String::from("s = {\n");
        for (key, field) in &self.fields {
            out.push('\t');
            out.push_str(&key_text(key));
            out.push_str(" = ");
            out.push_str(&value_text(field));
            out.push_str(",\n");
        }
        out.push('}');
        out
    }
}

/// Creates an empty settings builder.
pub fn settings() -> Settings {
    Settings::default()
}

fn number_text(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 {
        format!("{value:.1}")
    } else {
        value.to_string()
    }
}

fn quote(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn key_text(key: &str) -> String {
    let identifier = key
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if identifier {
        key.to_owned()
    } else {
        format!("[{}]", quote(key))
    }
}

fn value_text(field: &Field) -> String {
    match field {
        Field::Number(v) => number_text(*v),
        Field::Integer(v) => v.to_string(),
        Field::Boolean(v) => v.to_string(),
        Field::Text(v) => quote(v),
        Field::Raw(v) => v.clone(),
    }
}

/// The eight Lightroom colour bands and their catalog spelling, in order.
const BANDS: [(&str, &str); 8] = [
    ("red", "Red"),
    ("orange", "Orange"),
    ("yellow", "Yellow"),
    ("green", "Green"),
    ("aqua", "Aqua"),
    ("blue", "Blue"),
    ("purple", "Purple"),
    ("magenta", "Magenta"),
];

/// Every 1:1 slider of section 4.12: `(Lightroom key, control id, value)`.
///
/// The values are unique per entry (0.5, 0.75, 1.0, …), so a swapped slot is
/// caught. The 8 colour bands expand to `mixer.<band>.hue`, `.sat`, `.lum`
/// and `bw.<band>`. Merge-only values, crop, curves and masks are not part of
/// P6-1 and do not appear here.
pub fn all_controls() -> Vec<(String, String, f64)> {
    fn add(out: &mut Vec<(String, String, f64)>, key: &str, control: &str) {
        let value = 0.5 + out.len() as f64 * 0.25;
        out.push((key.to_owned(), control.to_owned(), value));
    }

    let mut out: Vec<(String, String, f64)> = Vec::new();

    // Light
    add(&mut out, "Exposure2012", "light.exposure");
    add(&mut out, "Contrast2012", "light.contrast");
    add(&mut out, "Highlights2012", "light.highlights");
    add(&mut out, "Shadows2012", "light.shadows");
    add(&mut out, "Whites2012", "light.whites");
    add(&mut out, "Blacks2012", "light.blacks");

    // HDR sliders (names confirmed in P6-6; HDREditMode is merge-only)
    add(&mut out, "HDRMaxValue", "hdr.maxEv");
    add(&mut out, "SDRBrightness", "hdr.sdrBrightness");
    add(&mut out, "SDRContrast", "hdr.sdrContrast");
    add(&mut out, "SDRHighlights", "hdr.sdrHighlights");
    add(&mut out, "SDRShadows", "hdr.sdrShadows");
    add(&mut out, "SDRWhites", "hdr.sdrWhites");
    add(&mut out, "SDRClarity", "hdr.sdrClarity");

    // Tone curve (parametric regions only)
    add(&mut out, "ParametricHighlights", "curve.highlights");
    add(&mut out, "ParametricLights", "curve.lights");
    add(&mut out, "ParametricDarks", "curve.darks");
    add(&mut out, "ParametricShadows", "curve.shadows");
    add(&mut out, "ParametricShadowSplit", "curve.splitShadows");
    add(&mut out, "ParametricMidtoneSplit", "curve.splitMid");
    add(
        &mut out,
        "ParametricHighlightSplit",
        "curve.splitHighlights",
    );
    add(&mut out, "CurveRefineSaturation", "curve.refineSaturation");

    // Colour grading
    add(&mut out, "SplitToningShadowHue", "grading.shadows.hue");
    add(
        &mut out,
        "SplitToningShadowSaturation",
        "grading.shadows.sat",
    );
    add(&mut out, "ColorGradeShadowLum", "grading.shadows.lum");
    add(&mut out, "ColorGradeMidtoneHue", "grading.midtones.hue");
    add(&mut out, "ColorGradeMidtoneSat", "grading.midtones.sat");
    add(&mut out, "ColorGradeMidtoneLum", "grading.midtones.lum");
    add(
        &mut out,
        "SplitToningHighlightHue",
        "grading.highlights.hue",
    );
    add(
        &mut out,
        "SplitToningHighlightSaturation",
        "grading.highlights.sat",
    );
    add(&mut out, "ColorGradeHighlightLum", "grading.highlights.lum");
    add(&mut out, "ColorGradeGlobalHue", "grading.global.hue");
    add(&mut out, "ColorGradeGlobalSat", "grading.global.sat");
    add(&mut out, "ColorGradeGlobalLum", "grading.global.lum");
    add(&mut out, "ColorGradeBlending", "grading.blending");
    add(&mut out, "SplitToningBalance", "grading.balance");

    // Colour
    add(&mut out, "Vibrance", "color.vibrance");
    add(&mut out, "Saturation", "color.saturation");

    // Effects
    add(&mut out, "Texture", "effects.texture");
    add(&mut out, "Clarity2012", "effects.clarity");
    add(&mut out, "Dehaze", "effects.dehaze");

    // Post-crop vignette
    add(&mut out, "PostCropVignetteAmount", "vignette.amount");
    add(&mut out, "PostCropVignetteMidpoint", "vignette.midpoint");
    add(&mut out, "PostCropVignetteRoundness", "vignette.roundness");
    add(&mut out, "PostCropVignetteFeather", "vignette.feather");
    add(
        &mut out,
        "PostCropVignetteHighlightContrast",
        "vignette.highlights",
    );

    // Grain
    add(&mut out, "GrainAmount", "grain.amount");
    add(&mut out, "GrainSize", "grain.size");
    add(&mut out, "GrainFrequency", "grain.roughness");

    // Detail
    add(&mut out, "Sharpness", "detail.sharpenAmount");
    add(&mut out, "SharpenRadius", "detail.sharpenRadius");
    add(&mut out, "SharpenDetail", "detail.sharpenDetail");
    add(&mut out, "SharpenEdgeMasking", "detail.sharpenMasking");
    add(&mut out, "LuminanceSmoothing", "detail.nrLuminance");
    add(&mut out, "LuminanceNoiseReductionDetail", "detail.nrDetail");
    add(
        &mut out,
        "LuminanceNoiseReductionContrast",
        "detail.nrContrast",
    );
    add(&mut out, "ColorNoiseReduction", "detail.nrColor");
    add(
        &mut out,
        "ColorNoiseReductionDetail",
        "detail.nrColorDetail",
    );
    add(
        &mut out,
        "ColorNoiseReductionSmoothness",
        "detail.nrColorSmoothness",
    );

    // Optics (manual corrections; profile switches are merge-only)
    add(&mut out, "LensManualDistortionAmount", "optics.distortion");
    add(&mut out, "VignetteAmount", "optics.vignetting");
    add(&mut out, "VignetteMidpoint", "optics.vignettingMidpoint");
    add(&mut out, "DefringePurpleAmount", "optics.defringePurple");
    add(
        &mut out,
        "DefringePurpleHueLo",
        "optics.defringePurpleHueLo",
    );
    add(
        &mut out,
        "DefringePurpleHueHi",
        "optics.defringePurpleHueHi",
    );
    add(&mut out, "DefringeGreenAmount", "optics.defringeGreen");
    add(&mut out, "DefringeGreenHueLo", "optics.defringeGreenHueLo");
    add(&mut out, "DefringeGreenHueHi", "optics.defringeGreenHueHi");

    // Geometry
    add(&mut out, "PerspectiveVertical", "geometry.vertical");
    add(&mut out, "PerspectiveHorizontal", "geometry.horizontal");
    add(&mut out, "PerspectiveRotate", "geometry.rotate");
    add(&mut out, "PerspectiveScale", "geometry.scale");
    add(&mut out, "PerspectiveAspect", "geometry.aspect");
    add(&mut out, "PerspectiveX", "geometry.offsetX");
    add(&mut out, "PerspectiveY", "geometry.offsetY");

    // Calibration
    add(&mut out, "ShadowTint", "calibration.shadowsTint");
    add(&mut out, "RedHue", "calibration.redHue");
    add(&mut out, "RedSaturation", "calibration.redSat");
    add(&mut out, "GreenHue", "calibration.greenHue");
    add(&mut out, "GreenSaturation", "calibration.greenSat");
    add(&mut out, "BlueHue", "calibration.blueHue");
    add(&mut out, "BlueSaturation", "calibration.blueSat");

    // Colour mixer and black-and-white mix, once per band
    for (band, crs) in BANDS {
        add(
            &mut out,
            &format!("HueAdjustment{crs}"),
            &format!("mixer.{band}.hue"),
        );
        add(
            &mut out,
            &format!("SaturationAdjustment{crs}"),
            &format!("mixer.{band}.sat"),
        );
        add(
            &mut out,
            &format!("LuminanceAdjustment{crs}"),
            &format!("mixer.{band}.lum"),
        );
        add(&mut out, &format!("GrayMixer{crs}"), &format!("bw.{band}"));
    }

    out
}

/// Old-process fields and their invented input values, in catalog order.
pub fn old_process_fields() -> Vec<(&'static str, f64)> {
    vec![
        ("Exposure", edge::OLD_EXPOSURE),
        ("Contrast", edge::OLD_CONTRAST),
        ("HighlightRecovery", edge::OLD_HIGHLIGHT_RECOVERY),
        ("FillLight", edge::OLD_FILL_LIGHT),
        ("Shadows", edge::OLD_SHADOWS),
        ("Brightness", edge::OLD_BRIGHTNESS),
        ("Clarity", edge::OLD_CLARITY),
    ]
}

/// Expected controls of the old-process fields, with the formulas of section
/// 4.1. The old point curve (`ToneCurve`) is read since P6-3; this fixture
/// stores it as the straight identity line, so it stays empty.
pub fn old_process_expected() -> Vec<(&'static str, f64)> {
    vec![
        ("light.exposure", edge::OLD_EXPOSURE),
        ("light.contrast", edge::OLD_CONTRAST - 25.0),
        ("light.highlights", -edge::OLD_HIGHLIGHT_RECOVERY),
        ("light.shadows", edge::OLD_FILL_LIGHT),
        ("light.blacks", -(edge::OLD_SHADOWS - 5.0) * 4.0),
        ("light.whites", (edge::OLD_BRIGHTNESS - 50.0) * 0.6),
        ("effects.clarity", edge::OLD_CLARITY),
    ]
}

/// Creates the empty settings table in the catalog at `path`.
pub fn create_table(path: &Path) -> Result<()> {
    let c = Connection::open(path)?;
    c.execute_batch(&format!(
        "CREATE TABLE {TABLE} (id_local INTEGER PRIMARY KEY, id_global, image INTEGER NOT NULL, text);"
    ))?;
    Ok(())
}

/// Drops the settings table from the catalog at `path`, if present.
pub fn drop_table(path: &Path) -> Result<()> {
    let c = Connection::open(path)?;
    c.execute_batch(&format!("DROP TABLE IF EXISTS {TABLE};"))?;
    Ok(())
}

/// Inserts `(image, text)` rows. `id_local` equals the image id.
pub fn add_rows(path: &Path, rows: &[(i64, String)]) -> Result<()> {
    let c = Connection::open(path)?;
    c.execute_batch("BEGIN")?;
    for (image, text) in rows {
        c.execute(
            &format!(
                "INSERT INTO {TABLE} (id_local, id_global, image, text) VALUES (?1, ?2, ?3, ?4)"
            ),
            params![image, format!("develop-{image:08}"), image, text],
        )?;
    }
    c.execute_batch("COMMIT")?;
    Ok(())
}

fn base_catalog(dir: &Path, file: &str) -> Result<PathBuf> {
    let (mac, win) = prepare_root_dirs(dir)?;
    let catalog = dir.join(file);
    write_small(&catalog, &mac, &win)?;
    create_original_files(&catalog)?;
    Ok(catalog)
}

/// Writes the small fixture as `dir/file` with the settings rows given, as
/// `(image, text)`. Returns the catalog path.
pub fn write_develop_catalog_with_files(
    dir: &Path,
    file: &str,
    rows: &[(i64, String)],
) -> Result<PathBuf> {
    let catalog = base_catalog(dir, file)?;
    create_table(&catalog)?;
    add_rows(&catalog, rows)?;
    Ok(catalog)
}

/// One photo with every mapped 1:1 slider (see [`all_controls`]).
pub fn write_all_sliders_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    let mut s = settings();
    for (key, _control, value) in all_controls() {
        s = s.number(&key, value);
    }
    write_develop_catalog_with_files(
        dir,
        "develop-all-sliders.lrcat",
        &[(edge::ALL_SLIDERS_PHOTO, s.build())],
    )
}

/// One photo with old-process fields only, one photo with old fields plus a
/// `*2012` key (the old block is ignored then).
pub fn write_old_process_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    let mut old = settings();
    for (key, value) in old_process_fields() {
        old = old.number(key, value);
    }
    let old = old.raw("ToneCurve", "{ 0, 0, 255, 255 }");

    let mut suppressed = settings().number("Exposure2012", edge::OLD_SUPPRESSED_EXPOSURE2012);
    for (key, value) in old_process_fields() {
        suppressed = suppressed.number(key, value);
    }

    write_develop_catalog_with_files(
        dir,
        "develop-old-process.lrcat",
        &[
            (edge::OLD_PROCESS_PHOTO, old.build()),
            (edge::OLD_SUPPRESSED_PHOTO, suppressed.build()),
        ],
    )
}

/// Merge-only switches in all three spellings (numeric, boolean, disabled).
pub fn write_merge_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    let on = settings()
        .boolean("ConvertToGrayscale", true)
        .integer("HDREditMode", 1)
        .integer("LensProfileEnable", 1)
        .boolean("AutoLateralCA", true);
    let off = settings()
        .boolean("ConvertToGrayscale", false)
        .integer("HDREditMode", 0)
        .boolean("LensProfileEnable", true)
        .boolean("AutoLateralCA", false);
    let disabled = settings()
        .integer("LensProfileEnable", 0)
        .integer("AutoLateralCA", 1);
    write_develop_catalog_with_files(
        dir,
        "develop-merge.lrcat",
        &[
            (edge::MERGE_ON_PHOTO, on.build()),
            (edge::MERGE_OFF_PHOTO, off.build()),
            (edge::MERGE_DISABLED_PHOTO, disabled.build()),
        ],
    )
}

/// Every named white-balance mode, one custom name, no white-balance fields at
/// all, and one Kelvin/tint and one incremental pair.
pub fn write_white_balance_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    let mut rows: Vec<(i64, String)> = Vec::new();
    for (photo, stored, _expected) in edge::WB_MODES {
        let mut s = settings().text("WhiteBalance", stored);
        if photo == edge::WB_ABSOLUTE_PHOTO {
            s = s
                .number("Temperature", edge::WB_TEMPERATURE)
                .number("Tint", edge::WB_TINT);
        }
        if photo == edge::WB_RELATIVE_PHOTO {
            s = s
                .number("IncrementalTemperature", edge::WB_INCREMENTAL_TEMPERATURE)
                .number("IncrementalTint", edge::WB_INCREMENTAL_TINT);
        }
        rows.push((photo, s.build()));
    }
    rows.push((
        edge::WB_CUSTOM_PHOTO,
        settings().text("WhiteBalance", "Custom").build(),
    ));
    rows.push((
        edge::WB_UNKNOWN_PHOTO,
        settings().text("WhiteBalance", "Zyx-Odd").build(),
    ));
    // A row without any white-balance field: the hint must be None.
    rows.push((
        edge::WB_NONE_PHOTO,
        settings().number("Vibrance", 0.5).build(),
    ));
    write_develop_catalog_with_files(dir, "develop-white-balance.lrcat", &rows)
}

/// Unknown and still-assumed keys, next to one known slider.
pub fn write_unmapped_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    let s = settings()
        .raw("RedEyeInfo", "{ 1 }")
        .raw("LensBlur", "{ 1 }")
        .raw("PointColors", "{ \"0.5, 0.2\" }")
        .number("ca_red", 12.5)
        .number("ca_blue", -12.5)
        .number("enhance.denoise", 40.0)
        .boolean("CropFlipHorizontal", true)
        .number("MysterySlider", edge::UNMAPPED_NUMBER)
        .text("MysteryText", edge::UNMAPPED_TEXT)
        .number("SDRBlend", 3.5)
        .number("Exposure2012", 1.0);
    write_develop_catalog_with_files(
        dir,
        "develop-unmapped.lrcat",
        &[(edge::UNMAPPED_PHOTO, s.build())],
    )
}

/// The sentinel at top level and inside a nested table.
pub fn write_deferred_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    let top = settings()
        .number("Exposure2012", edge::SENTINEL)
        .number("Contrast2012", edge::DEFERRED_CONTRAST)
        .number("Vibrance", edge::SENTINEL);
    let nested = settings().raw("PointColors", "{ -999999, 1.0 }");
    write_develop_catalog_with_files(
        dir,
        "develop-deferred.lrcat",
        &[
            (edge::DEFERRED_PHOTO, top.build()),
            (edge::NESTED_SENTINEL_PHOTO, nested.build()),
        ],
    )
}

/// Broken, oversized and valid settings rows in one catalog.
pub fn write_invalid_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    write_develop_catalog_with_files(
        dir,
        "develop-invalid.lrcat",
        &[
            (
                edge::ALL_SLIDERS_PHOTO,
                settings().number("Exposure2012", 1.0).build(),
            ),
            (
                edge::INVALID_UNBALANCED_PHOTO,
                "s = { Exposure2012 = 1.0,".to_owned(),
            ),
            (
                edge::INVALID_MISSING_VALUE_PHOTO,
                "s = { Exposure2012 = }".to_owned(),
            ),
            (edge::OVERSIZED_PHOTO, oversized_text()),
            (
                edge::VALID_AFTER_INVALID_PHOTO,
                settings().number("Contrast2012", 2.0).build(),
            ),
        ],
    )
}

/// A table with no rows.
pub fn write_empty_table_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    write_develop_catalog_with_files(dir, "develop-empty-table.lrcat", &[])
}

/// One row whose `text` is the empty string.
pub fn write_empty_text_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    write_develop_catalog_with_files(
        dir,
        "develop-empty-text.lrcat",
        &[(edge::EMPTY_TEXT_PHOTO, String::new())],
    )
}

/// A master and its virtual copy with independent settings.
pub fn write_virtual_copy_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    let master = settings()
        .number("Exposure2012", edge::VIRTUAL_MASTER_EXPOSURE)
        .number("Contrast2012", edge::VIRTUAL_MASTER_CONTRAST);
    let copy = settings()
        .number("Exposure2012", edge::VIRTUAL_COPY_EXPOSURE)
        .number("Vibrance", edge::VIRTUAL_COPY_VIBRANCE);
    write_develop_catalog_with_files(
        dir,
        "develop-virtual-copy.lrcat",
        &[
            (edge::VIRTUAL_MASTER_PHOTO, master.build()),
            (edge::VIRTUAL_COPY_PHOTO, copy.build()),
        ],
    )
}

/// One row for every photo of the small fixture: valid, invalid, empty and
/// deferred rows, so the balance can be checked.
pub fn write_balance_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    let mut rows = Vec::new();
    for photo in 1..=edge::BALANCE_PHOTOS {
        let text = match photo {
            edge::INVALID_UNBALANCED_PHOTO => "s = { Exposure2012 = 1.0,".to_owned(),
            edge::INVALID_MISSING_VALUE_PHOTO => "s = { Exposure2012 = }".to_owned(),
            edge::BALANCE_EMPTY_TEXT => String::new(),
            edge::BALANCE_DEFERRED => settings()
                .number("Exposure2012", edge::SENTINEL)
                .number("Contrast2012", edge::DEFERRED_CONTRAST)
                .build(),
            _ => settings().number("Exposure2012", 1.0).build(),
        };
        rows.push((photo, text));
    }
    write_develop_catalog_with_files(dir, "develop-balance.lrcat", &rows)
}

/// Sets `Adobe_images.orientation`, `fileWidth` and `fileHeight` for one
/// image. The small fixture leaves all three NULL.
pub fn set_image_geometry(
    path: &Path,
    image: i64,
    orientation: &str,
    width: i64,
    height: i64,
) -> Result<()> {
    let c = Connection::open(path)?;
    c.execute(
        "UPDATE Adobe_images SET orientation = ?1, fileWidth = ?2, fileHeight = ?3 \
         WHERE id_local = ?4",
        params![orientation, width, height, image],
    )?;
    Ok(())
}

/// Curve fixtures: master only, master plus one channel, master plus all three
/// channels, an identity line, an old curve, an old curve beside a 2012 one,
/// channel curves without a master, and two broken lists.
pub fn write_curves_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    let list = lua_list;
    let rows = vec![
        (
            crate::develop::curve::MASTER_PHOTO,
            settings()
                .raw("ToneCurvePV2012", &list(&crate::develop::curve::MASTER_255))
                .build(),
        ),
        (
            crate::develop::curve::ONE_CHANNEL_PHOTO,
            settings()
                .raw("ToneCurvePV2012", &list(&crate::develop::curve::MASTER_255))
                .raw("ToneCurvePV2012Red", &list(&crate::develop::curve::RED_255))
                .build(),
        ),
        (
            crate::develop::curve::ALL_CHANNELS_PHOTO,
            settings()
                .raw("ToneCurvePV2012", &list(&crate::develop::curve::MASTER_255))
                .raw("ToneCurvePV2012Red", &list(&crate::develop::curve::RED_255))
                .raw(
                    "ToneCurvePV2012Green",
                    &list(&crate::develop::curve::GREEN_255),
                )
                .raw(
                    "ToneCurvePV2012Blue",
                    &list(&crate::develop::curve::BLUE_255),
                )
                .build(),
        ),
        (
            crate::develop::curve::IDENTITY_PHOTO,
            settings()
                .raw(
                    "ToneCurvePV2012",
                    &list(&crate::develop::curve::IDENTITY_255),
                )
                .build(),
        ),
        (
            crate::develop::curve::OLD_PHOTO,
            settings()
                .raw("ToneCurve", &list(&crate::develop::curve::OLD_255))
                .build(),
        ),
        (
            crate::develop::curve::OLD_AND_NEW_PHOTO,
            settings()
                .raw("ToneCurve", &list(&crate::develop::curve::OLD_255))
                .raw("ToneCurvePV2012", &list(&crate::develop::curve::NEW_255))
                .build(),
        ),
        (
            crate::develop::curve::ODD_LIST_PHOTO,
            settings().raw("ToneCurvePV2012", "{ 0, 0, 128 }").build(),
        ),
        (
            crate::develop::curve::BROKEN_POINT_PHOTO,
            settings()
                .raw("ToneCurvePV2012", "{ 0, 0, \"ZyxNotANumber\", 255 }")
                .build(),
        ),
        (
            crate::develop::curve::CHANNELS_WITHOUT_MASTER_PHOTO,
            settings()
                .raw("ToneCurvePV2012Red", &list(&crate::develop::curve::RED_255))
                .raw(
                    "ToneCurvePV2012Green",
                    &list(&crate::develop::curve::GREEN_255),
                )
                .raw(
                    "ToneCurvePV2012Blue",
                    &list(&crate::develop::curve::BLUE_255),
                )
                .build(),
        ),
    ];
    write_develop_catalog_with_files(dir, "develop-curves.lrcat", &rows)
}

/// Crop fixtures: upright and oriented crops, an angle-only crop, a photo
/// without crop fields, and one with the unverified aspect/flip keys.
pub fn write_crop_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    let edges = |left: f64, top: f64, right: f64, bottom: f64| {
        settings()
            .number("CropLeft", left)
            .number("CropTop", top)
            .number("CropRight", right)
            .number("CropBottom", bottom)
    };
    use crate::develop::crop as c;
    let rows = vec![
        (
            c::PLAIN_PHOTO,
            edges(c::LEFT, c::TOP, c::RIGHT, c::BOTTOM)
                .number("CropAngle", 0.0)
                .build(),
        ),
        (
            c::ANGLE_POSITIVE_PHOTO,
            edges(
                c::ORIENT_LEFT,
                c::ORIENT_TOP,
                c::ORIENT_RIGHT,
                c::ORIENT_BOTTOM,
            )
            .number("CropAngle", c::POSITIVE_ANGLE)
            .build(),
        ),
        (
            c::ANGLE_NEGATIVE_PHOTO,
            edges(
                c::ORIENT_LEFT,
                c::ORIENT_TOP,
                c::ORIENT_RIGHT,
                c::ORIENT_BOTTOM,
            )
            .number("CropAngle", c::NEGATIVE_ANGLE)
            .build(),
        ),
        (
            c::ANGLE_ONLY_PHOTO,
            settings().number("CropAngle", c::ANGLE_ONLY).build(),
        ),
        (
            c::NONE_PHOTO,
            settings().number("Exposure2012", 0.5).build(),
        ),
        (
            c::FLIP_H_ANGLE_PHOTO,
            edges(
                c::ORIENT_LEFT,
                c::ORIENT_TOP,
                c::ORIENT_RIGHT,
                c::ORIENT_BOTTOM,
            )
            .number("CropAngle", c::FLIP_ANGLE)
            .build(),
        ),
        (
            c::ROTATE90_PHOTO,
            edges(
                c::ORIENT_LEFT,
                c::ORIENT_TOP,
                c::ORIENT_RIGHT,
                c::ORIENT_BOTTOM,
            )
            .number("CropAngle", 0.0)
            .build(),
        ),
        (
            c::FLIP_H_PHOTO,
            edges(
                c::ORIENT_LEFT,
                c::ORIENT_TOP,
                c::ORIENT_RIGHT,
                c::ORIENT_BOTTOM,
            )
            .number("CropAngle", 0.0)
            .build(),
        ),
        (
            c::ASSUMPTIONS_PHOTO,
            edges(
                c::ORIENT_LEFT,
                c::ORIENT_TOP,
                c::ORIENT_RIGHT,
                c::ORIENT_BOTTOM,
            )
            .boolean("CropConstrainAspectRatio", true)
            .boolean("CropFlipHorizontal", true)
            .boolean("CropFlipVertical", false)
            .number("CropAngle", 0.0)
            .build(),
        ),
    ];
    let catalog = write_develop_catalog_with_files(dir, "develop-crop.lrcat", &rows)?;
    let upright =
        |photo| set_image_geometry(&catalog, photo, "AB", c::STORED_WIDTH, c::STORED_HEIGHT);
    for photo in [
        c::ANGLE_POSITIVE_PHOTO,
        c::ANGLE_NEGATIVE_PHOTO,
        c::ANGLE_ONLY_PHOTO,
        c::NONE_PHOTO,
        c::ASSUMPTIONS_PHOTO,
    ] {
        upright(photo)?;
    }
    set_image_geometry(
        &catalog,
        c::FLIP_H_ANGLE_PHOTO,
        "BA",
        c::STORED_WIDTH,
        c::STORED_HEIGHT,
    )?;
    set_image_geometry(
        &catalog,
        c::ROTATE90_PHOTO,
        "BC",
        c::STORED_WIDTH,
        c::STORED_HEIGHT,
    )?;
    set_image_geometry(
        &catalog,
        c::FLIP_H_PHOTO,
        "BA",
        c::STORED_WIDTH,
        c::STORED_HEIGHT,
    )?;
    Ok(catalog)
}

/// The byte length [`oversized_text`] produces: one byte above the reader's
/// 16 MiB limit, plus the Lua wrapper.
pub const OVERSIZED_PADDING_BYTES: usize = 16 * 1024 * 1024 + 1;

/// A syntactically valid settings text whose byte length exceeds the reader's
/// `DEVELOP_MAX_TEXT_BYTES` (16 MiB).
pub fn oversized_text() -> String {
    let padding = "x".repeat(OVERSIZED_PADDING_BYTES);
    format!("s = {{ ZyxPad = \"{padding}\" }}")
}

/// Camera-profile fixture (P6-5). All names are invented.
///
/// One `CameraProfile` per row, plus a `Look` on one row. Every row carries no
/// slider and no other mapped field, so a profile alone must not plan a
/// develop command.
pub mod profile {
    /// Two photos share [`STUDIO`]; a third shares it through [`LOOK_PHOTO`].
    pub const STUDIO_A_PHOTO: i64 = 1;
    pub const STUDIO_B_PHOTO: i64 = 2;
    /// Profile name with umlauts and an emoji; must survive byte for byte.
    pub const BYTE_EXACT_PHOTO: i64 = 3;
    /// Same byte-exact name again: count 2.
    pub const BYTE_EXACT_SECOND_PHOTO: i64 = 10;
    /// Row without any `CameraProfile` field.
    pub const NO_PROFILE_PHOTO: i64 = 4;
    /// Row with `CameraProfile = ""`: no name, no report entry.
    pub const EMPTY_PROFILE_PHOTO: i64 = 5;
    /// Row with `CameraProfile = 42`: wrong type, unmapped, no profile.
    pub const WRONG_TYPE_PHOTO: i64 = 6;
    /// Row with `CameraProfile` and a `Look`; the profile counts, the Look is
    /// still unmapped.
    pub const LOOK_PHOTO: i64 = 7;
    /// Row whose only field is `CameraProfile`.
    pub const PROFILE_ONLY_PHOTO: i64 = 8;

    pub const STUDIO: &str = "Studio Neutral";
    pub const BYTE_EXACT: &str = "Profil Ärger 📷";
    pub const PROFILE_ONLY: &str = "Solo Profile";
    pub const LOOK: &str = "Zyx Look";

    /// Photos that carry a non-empty profile name: 1, 2, 3, 7, 8 and 10.
    pub const PHOTOS_WITH_PROFILE: usize = 6;
    /// Distinct names: [`STUDIO`], [`BYTE_EXACT`], [`PROFILE_ONLY`].
    pub const DISTINCT_PROFILES: usize = 3;
    /// Photo 6 (wrong type) and photo 7 (`Look`) are the only unmapped fields.
    pub const UNMAPPED_NAMES: [&str; 2] = ["CameraProfile", "Look"];
}

/// The camera-profile fixture: ten rows, profile-only except the `Look` and
/// the wrong-typed value.
pub fn write_camera_profile_catalog_with_files(dir: &Path) -> Result<PathBuf> {
    use profile as p;
    let rows = [
        (
            p::STUDIO_A_PHOTO,
            settings().text("CameraProfile", p::STUDIO).build(),
        ),
        (
            p::STUDIO_B_PHOTO,
            settings().text("CameraProfile", p::STUDIO).build(),
        ),
        (
            p::BYTE_EXACT_PHOTO,
            settings().text("CameraProfile", p::BYTE_EXACT).build(),
        ),
        (p::NO_PROFILE_PHOTO, settings().build()),
        (
            p::EMPTY_PROFILE_PHOTO,
            settings().text("CameraProfile", "").build(),
        ),
        (
            p::WRONG_TYPE_PHOTO,
            settings().integer("CameraProfile", 42).build(),
        ),
        (
            p::LOOK_PHOTO,
            settings()
                .text("CameraProfile", p::STUDIO)
                .text("Look", p::LOOK)
                .build(),
        ),
        (
            p::PROFILE_ONLY_PHOTO,
            settings().text("CameraProfile", p::PROFILE_ONLY).build(),
        ),
        (
            p::BYTE_EXACT_SECOND_PHOTO,
            settings().text("CameraProfile", p::BYTE_EXACT).build(),
        ),
    ];
    // Photos 2 and 3 are among the small fixture's missing originals; this
    // catalog needs every row converted, so create all originals. Lead
    // exception: only this fixture base changes, every documented count and
    // every assertion of the acceptance test stays unchanged.
    let catalog = write_develop_catalog_with_files(dir, "develop-camera-profile.lrcat", &rows)?;
    crate::create_all_original_files(&catalog)?;
    Ok(catalog)
}
