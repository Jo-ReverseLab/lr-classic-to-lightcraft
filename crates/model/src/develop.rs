//! Neutral development settings of one photo.
//!
//! The values are already expressed in LightCraft's vocabulary: slider
//! values use LightCraft's control ids and the structured parts use the
//! field names of LightCraft's partial settings object. The model itself
//! knows nothing about either catalog format.
//!
//! Slider values live in a map instead of typed fields because LightCraft
//! owns the list of control ids and may extend it. The map keeps the model
//! small and lets the writer pass the values on unchanged.

use std::collections::BTreeMap;

/// Development settings of one photo.
///
/// A photo whose catalog row cannot be read has no settings at all
/// (`None` on the photo), which is distinct from an empty `Develop`
/// (a row that holds no usable value).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Develop {
    /// Slider-addressable values: LightCraft control id to value, ordered by
    /// id. Only values the catalog actually stores.
    pub controls: BTreeMap<String, f64>,
    /// Values that only `develop.merge` accepts, keyed by the JSON path of
    /// LightCraft's partial settings (for example `treatment`, `hdr.enabled`
    /// or `optics.remove_ca`).
    pub partial: BTreeMap<String, MergeValue>,
    /// Tone curves, in LightCraft's unit-range coordinates. An empty list
    /// means the photo has no such curve.
    pub curves: Curves,
    /// Crop rectangle and angle in LightCraft's oriented, rotated frame,
    /// or `None` when the photo has no crop.
    pub crop: Option<CropGeometry>,
    /// White-balance mode and what its scale would need.
    pub white_balance: WhiteBalance,
    /// The camera profile name the catalog stores, byte for byte, or `None`
    /// when the row has no (or an empty) name. It is reported by name only
    /// and never written; a profile alone is no develop command.
    pub camera_profile: Option<String>,
    /// Names of catalog fields that could not be carried over, once each,
    /// in the order the settings text stores them. Only names, never values.
    pub unmapped: Vec<String>,
    /// True if the settings text contained the `-999999` sentinel, which
    /// Lightroom writes for values it has not computed yet.
    pub deferred: bool,
    /// SHA-256 of the raw settings text, for an idempotent re-sync.
    pub source_hash: Option<[u8; 32]>,
}

/// A value that LightCraft only accepts through `develop.merge`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeValue {
    /// A boolean switch.
    Bool(bool),
    /// A text value, kept byte for byte.
    Text(String),
}

/// White-balance mode and what the catalog can tell about its scale.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WhiteBalance {
    /// LightCraft's `wb.mode` spelling (`asShot`, `auto`, `custom`, …), if the
    /// catalog stores a mode. The stored Lightroom spelling is not kept.
    pub mode: Option<String>,
    /// What a later phase would need to express the stored temperature.
    pub hint: WhiteBalanceHint,
}

/// What the catalog says about the stored temperature and tint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WhiteBalanceHint {
    /// The row holds no temperature or tint.
    #[default]
    None,
    /// The value is an absolute Kelvin value and needs no conversion.
    Kelvin,
    /// The value is on the relative scale (`Incremental*` fields).
    Relative,
    /// The value is an absolute Kelvin value but the catalog holds no
    /// as-shot reference, so it cannot be converted without probing the
    /// original file.
    Undecidable,
}

/// The four tone curves of a photo.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Curves {
    /// Master curve.
    pub master: Vec<CurvePoint>,
    /// Red channel curve.
    pub red: Vec<CurvePoint>,
    /// Green channel curve.
    pub green: Vec<CurvePoint>,
    /// Blue channel curve.
    pub blue: Vec<CurvePoint>,
}

/// One point of a tone curve.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CurvePoint {
    /// Input value.
    pub x: f64,
    /// Output value.
    pub y: f64,
}

/// The crop rectangle in LightCraft's normalised coordinates, plus the angle.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CropGeometry {
    /// Left edge.
    pub x0: f64,
    /// Top edge.
    pub y0: f64,
    /// Right edge.
    pub x1: f64,
    /// Bottom edge.
    pub y1: f64,
    /// Rotation angle in degrees.
    pub angle: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_is_optional_and_keeps_its_bytes() {
        assert_eq!(Develop::default().camera_profile, None);
        let develop = Develop {
            camera_profile: Some("Profil Ärger 📷".to_owned()),
            ..Develop::default()
        };
        assert_eq!(develop.camera_profile.as_deref(), Some("Profil Ärger 📷"));
        assert_ne!(develop, Develop::default());
    }
}
