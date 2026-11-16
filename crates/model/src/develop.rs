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
    /// Local corrections, in LightCraft's order and shape, or empty when the
    /// photo has no usable mask.
    pub masks: Vec<Mask>,
    /// Spot removal and healing, in catalog order, or empty when the photo
    /// has no usable spot.
    pub spots: Vec<Spot>,
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
    /// Which scale [`Self::temperature`] and [`Self::tint`] are on.
    pub hint: WhiteBalanceHint,
    /// The stored temperature of a custom white balance: Kelvin for
    /// [`WhiteBalanceHint::Undecidable`], Lightroom's relative
    /// `IncrementalTemperature` (-100..100) for [`WhiteBalanceHint::Relative`].
    /// `None` when the hint is `None`.
    pub temperature: Option<f64>,
    /// The stored tint of a custom white balance (`Tint` or
    /// `IncrementalTint`, by the hint). `None` when the hint is `None`.
    pub tint: Option<f64>,
}

/// What the catalog says about the stored temperature and tint.
///
/// Only a custom white balance (mode `custom`, or no mode stored) carries
/// its numbers. A named mode (`asShot`, `auto`, `daylight`, …) is carried by
/// the mode alone, as in LightCraft's own Lightroom import: its stored
/// numbers are Lightroom's rendering of that mode, and the hint is `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WhiteBalanceHint {
    /// No custom temperature or tint to carry.
    #[default]
    None,
    /// The value is an absolute Kelvin value and needs no conversion.
    Kelvin,
    /// The value is on the relative scale (`Incremental*` fields of a
    /// rendered file such as a JPEG).
    Relative,
    /// The value is an absolute Kelvin value (`Temperature`, `Tint` of a raw
    /// or DNG). It is carried only when LightCraft edits the photo on a
    /// Kelvin scale, which only LightCraft can tell after the import.
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

/// One local correction of a photo, in LightCraft's vocabulary.
#[derive(Debug, Clone, PartialEq)]
pub struct Mask {
    /// LightCraft's mask id: the position among the photo's converted masks,
    /// starting at 1.
    pub id: u32,
    /// The correction's name, or `Mask N` with the mask's id.
    pub name: String,
    /// Whether the correction is active. An inactive correction is never
    /// converted, so a stored mask is always visible.
    pub visible: bool,
    /// The correction-level inversion.
    pub invert: bool,
    /// The shape components, in catalog order.
    pub components: Vec<MaskComponent>,
    /// Local adjustment values by LightCraft's `adjust` field name. `amount`
    /// is always present.
    pub adjust: BTreeMap<String, f64>,
}

/// How a mask component combines with the ones before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskOp {
    /// Add the component's area.
    Add,
    /// Subtract the component's area.
    Subtract,
    /// Keep the overlap with the components before it.
    Intersect,
}

/// One shape component of a local correction.
#[derive(Debug, Clone, PartialEq)]
pub struct MaskComponent {
    /// How the shape combines with the mask.
    pub op: MaskOp,
    /// The component-level inversion.
    pub invert: bool,
    /// The shape itself.
    pub shape: MaskShape,
}

/// A shape of a mask component.
#[derive(Debug, Clone, PartialEq)]
pub enum MaskShape {
    /// Linear gradient: full effect at `start`, fading to none at `end`.
    Linear {
        /// Full-strength point, normalized to the photo.
        start: Point,
        /// Zero-strength point, normalized to the photo.
        end: Point,
    },
    /// Radial gradient.
    Radial {
        /// Ellipse center, normalized to the photo.
        center: Point,
        /// Horizontal radius in long-edge units.
        rx: f64,
        /// Vertical radius in long-edge units.
        ry: f64,
        /// Rotation in degrees.
        angle: f64,
        /// Feather on LightCraft's 0..100 scale.
        feather: f64,
        /// Whether the effect lies outside the ellipse.
        invert: bool,
    },
    /// Brush strokes.
    Brush {
        /// The strokes, in catalog order.
        strokes: Vec<BrushStroke>,
    },
    /// Luminance range with a feathered falloff on LightCraft's 0..1 scale.
    LuminanceRange {
        /// Lower bound.
        lo: f64,
        /// Upper bound.
        hi: f64,
        /// Feather below `lo`.
        lo_feather: f64,
        /// Feather above `hi`.
        hi_feather: f64,
    },
    /// Depth range.
    DepthRange {
        /// Lower bound.
        lo: f64,
        /// Upper bound.
        hi: f64,
        /// Feather at either bound.
        feather: f64,
    },
    /// An AI subject selection.
    Subject,
    /// An AI sky selection.
    Sky,
}

/// One brush stroke of a brush mask.
#[derive(Debug, Clone, PartialEq)]
pub struct BrushStroke {
    /// The dabs' points, normalized to the photo.
    pub points: Vec<Point>,
    /// Radius as a fraction of the photo's long edge.
    pub size: f64,
    /// Feather on LightCraft's 0..100 scale.
    pub feather: f64,
    /// Flow on LightCraft's 0..100 scale.
    pub flow: f64,
    /// Density on LightCraft's 0..100 scale.
    pub density: f64,
    /// Whether the stroke erases instead of paints.
    pub erase: bool,
}

/// How a spot rebuilds the area it covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpotMode {
    /// Content-aware removal: the spot is rebuilt from its surroundings.
    #[default]
    Remove,
    /// Healing with a source, blended with the target.
    Heal,
    /// Cloning from an explicit source.
    Clone,
    /// Content-aware fill without a single source.
    Fill,
}

/// One spot removal or healing area, in LightCraft's vocabulary.
#[derive(Debug, Clone, PartialEq)]
pub struct Spot {
    /// How the area is rebuilt.
    pub mode: SpotMode,
    /// The target: one point for a circle, a path for a brush.
    pub points: Vec<Point>,
    /// Radius as a fraction of the photo's long edge.
    pub size: f64,
    /// Feather on LightCraft's 0..100 scale.
    pub feather: f64,
    /// Opacity on LightCraft's 0..100 scale.
    pub opacity: f64,
    /// Offset from the first target point to the source, or `None` when the
    /// source is automatic.
    pub source_offset: Option<Point>,
}

/// A point normalized to a photo's width and height.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    /// Horizontal position.
    pub x: f64,
    /// Vertical position.
    pub y: f64,
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

    #[test]
    fn masks_default_empty_and_carry_typed_shapes() {
        assert!(Develop::default().masks.is_empty());
        let mask = Mask {
            id: 1,
            name: "Sky Mask".to_owned(),
            visible: true,
            invert: false,
            components: vec![MaskComponent {
                op: MaskOp::Add,
                invert: false,
                shape: MaskShape::Sky,
            }],
            adjust: BTreeMap::from([("amount".to_owned(), 100.0)]),
        };
        let develop = Develop {
            masks: vec![mask.clone()],
            ..Develop::default()
        };
        assert_eq!(develop.masks, vec![mask]);
        assert_ne!(develop.masks, Develop::default().masks);
    }

    #[test]
    fn spots_default_empty_and_carry_their_points() {
        assert!(Develop::default().spots.is_empty());
        let spot = Spot {
            mode: SpotMode::Heal,
            points: vec![Point { x: 0.4, y: 0.3 }],
            size: 0.04,
            feather: 50.0,
            opacity: 80.0,
            source_offset: Some(Point { x: 0.2, y: 0.2 }),
        };
        let develop = Develop {
            spots: vec![spot.clone()],
            ..Develop::default()
        };
        assert_eq!(develop.spots, vec![spot]);
        assert_ne!(develop.spots, Develop::default().spots);
    }

    #[test]
    fn every_spot_mode_is_distinct() {
        assert_ne!(SpotMode::Remove, SpotMode::Heal);
        assert_ne!(SpotMode::Heal, SpotMode::Clone);
        assert_ne!(SpotMode::Clone, SpotMode::Fill);
        assert_eq!(SpotMode::default(), SpotMode::Remove);
    }

    #[test]
    fn every_mask_op_and_shape_is_distinct() {
        assert_ne!(MaskOp::Add, MaskOp::Subtract);
        assert_ne!(MaskOp::Subtract, MaskOp::Intersect);
        assert_ne!(MaskShape::Subject, MaskShape::Sky);
        assert_ne!(
            MaskShape::DepthRange {
                lo: 0.2,
                hi: 0.8,
                feather: 0.1
            },
            MaskShape::LuminanceRange {
                lo: 0.2,
                hi: 0.8,
                lo_feather: 0.1,
                hi_feather: 0.1
            }
        );
    }
}
