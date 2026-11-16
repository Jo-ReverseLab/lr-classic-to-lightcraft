//! Reading `Adobe_imageDevelopSettings.text` without executing it.
//!
//! Lightroom Classic stores one development-settings text per image: a Lua
//! table literal of the form `s = { Key = value, … }`. This module parses the
//! small subset that Lightroom writes — named fields, numbers, quoted
//! strings, booleans and nested tables (including positional array entries) —
//! and maps the known keys to LightCraft's control ids and partial settings.
//!
//! It never executes anything and never calls a function: an expression the
//! subset does not cover is a parse error. Every error keeps the photo; the
//! caller records it and continues.

use model::{
    BrushStroke, CropGeometry, CurvePoint, Curves, Develop, Mask, MaskComponent, MaskOp, MaskShape,
    MergeValue, Point, Spot, SpotMode, WhiteBalanceHint,
};
use sha2::{Digest, Sha256};

/// The largest settings text the reader accepts. A larger text is treated as
/// unreadable: it cannot come from a real edit and would only waste memory.
pub const DEVELOP_MAX_TEXT_BYTES: usize = 16 * 1024 * 1024;

/// The deepest table nesting the parser accepts.
pub const DEVELOP_MAX_DEPTH: usize = 64;

/// Why a settings text could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DevelopError {
    /// The text is larger than [`DEVELOP_MAX_TEXT_BYTES`].
    #[error("development settings text is too large (limit {DEVELOP_MAX_TEXT_BYTES} bytes)")]
    TooLarge,
    /// The text is not the supported Lua subset.
    #[error("invalid development settings text at byte {offset}: {message}")]
    Syntax {
        /// Byte offset of the problem.
        offset: usize,
        /// What the parser expected.
        message: &'static str,
    },
    /// A tone curve is not a list of x/y pairs of numbers.
    #[error("invalid tone curve: {message}")]
    Curve {
        /// What is wrong with the list.
        message: &'static str,
    },
}

/// Parses one settings text into the neutral model, without image geometry.
///
/// Empty or whitespace-only text is an empty [`Develop`]. The returned value
/// always carries the SHA-256 of `text` as its source hash.
///
/// Without the image's stored orientation and pixel size — the catalog reader
/// supplies them — a crop is expressed as if the image were upright and 1:1.
/// The crate-internal geometry-aware entry point produces the real crop.
pub fn parse_develop(text: &str) -> Result<Develop, DevelopError> {
    parse_develop_with_geometry(text, StoredGeometry::default())
}

/// Parses one settings text, using the image's stored orientation and pixel
/// size to express its crop in LightCraft's oriented frame.
pub(crate) fn parse_develop_with_geometry(
    text: &str,
    geometry: StoredGeometry,
) -> Result<Develop, DevelopError> {
    if text.len() > DEVELOP_MAX_TEXT_BYTES {
        return Err(DevelopError::TooLarge);
    }
    let mut parser = Parser::new(text);
    let mut table = parser.parse_document()?;
    let mut deferred = false;
    prune_sentinel(&mut table, &mut deferred);
    let mut develop = Develop {
        deferred,
        source_hash: Some(source_hash(text.as_bytes())),
        ..Develop::default()
    };
    map_table(&table, &mut develop, geometry)?;
    Ok(develop)
}

/// The stored orientation and pixel size of one image: the image as the file
/// holds it, before Lightroom's orientation and any crop.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct StoredGeometry {
    orientation: StoredOrientation,
    width: Option<f64>,
    height: Option<f64>,
}

impl StoredGeometry {
    /// Reads the three optional catalog columns. A missing or unknown
    /// orientation counts as upright, a missing or unusable size as 1:1.
    pub(crate) fn new(orientation: Option<&str>, width: Option<f64>, height: Option<f64>) -> Self {
        StoredGeometry {
            orientation: orientation
                .map(StoredOrientation::from_code)
                .unwrap_or_default(),
            width,
            height,
        }
    }

    /// Width divided by height of the stored image, or 1.0 when unknown.
    fn aspect(self) -> f64 {
        match (self.width, self.height) {
            (Some(width), Some(height))
                if width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0 =>
            {
                width / height
            }
            _ => 1.0,
        }
    }

    /// Width divided by height of the displayed image. A stored quarter turn
    /// swaps the axes, so the displayed aspect is the reciprocal.
    fn displayed_aspect(self) -> f64 {
        let aspect = self.aspect();
        if self.orientation.swaps_axes() {
            1.0 / aspect
        } else {
            aspect
        }
    }
}

/// Lightroom's `Adobe_images.orientation`, the eight EXIF orientations.
///
/// Lightroom stores where the stored image's top-left and top-right corners
/// land in the displayed photo, naming the displayed corners A, B, C and D
/// clockwise from the top left: `AB` upright, `BC` a quarter turn clockwise,
/// `DA` a quarter turn counter-clockwise, `BA` mirrored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum StoredOrientation {
    #[default]
    Normal,
    FlipH,
    Rotate180,
    FlipV,
    Transpose,
    Rotate90,
    Transverse,
    Rotate270,
}

impl StoredOrientation {
    fn from_code(code: &str) -> Self {
        match code {
            "BA" => StoredOrientation::FlipH,
            "CD" => StoredOrientation::Rotate180,
            "DC" => StoredOrientation::FlipV,
            "AD" => StoredOrientation::Transpose,
            "BC" => StoredOrientation::Rotate90,
            "CB" => StoredOrientation::Transverse,
            "DA" => StoredOrientation::Rotate270,
            _ => StoredOrientation::Normal,
        }
    }

    /// Split into a horizontal flip and clockwise quarter turns applied after
    /// it.
    fn parts(self) -> (bool, u8) {
        match self {
            StoredOrientation::Normal => (false, 0),
            StoredOrientation::Rotate90 => (false, 1),
            StoredOrientation::Rotate180 => (false, 2),
            StoredOrientation::Rotate270 => (false, 3),
            StoredOrientation::FlipH => (true, 0),
            StoredOrientation::Transverse => (true, 1),
            StoredOrientation::FlipV => (true, 2),
            StoredOrientation::Transpose => (true, 3),
        }
    }

    fn mirrored(self) -> bool {
        self.parts().0
    }

    fn swaps_axes(self) -> bool {
        matches!(
            self,
            StoredOrientation::Transpose
                | StoredOrientation::Rotate90
                | StoredOrientation::Transverse
                | StoredOrientation::Rotate270
        )
    }

    /// Moves a point of the stored image into the displayed one, both
    /// normalised to their own width and height.
    fn map(self, x: f64, y: f64, width: f64, height: f64) -> (f64, f64) {
        let (flip, turns) = self.parts();
        let mut x = if flip { width - x } else { x };
        let mut y = y;
        let mut width = width;
        let mut height = height;
        for _ in 0..turns {
            // A quarter turn clockwise: (x, y) -> (height - y, x).
            let rotated_x = height - y;
            y = x;
            x = rotated_x;
            std::mem::swap(&mut width, &mut height);
        }
        (x, y)
    }
}

fn source_hash(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

// ---------------------------------------------------------------------------
// Parsed Lua values
// ---------------------------------------------------------------------------

/// A value of the supported subset.
#[derive(Debug, Clone, PartialEq)]
enum LuaValue {
    Number(f64),
    Bool(bool),
    Text(String),
    Table(Table),
}

/// A Lua table: named fields and positional array entries, in source order.
#[derive(Debug, Clone, Default, PartialEq)]
struct Table {
    fields: Vec<(Option<String>, LuaValue)>,
}

/// Removes every `-999999` number, also inside nested tables, and notes when
/// at least one was found. Lightroom writes this sentinel for values it has
/// not computed yet; it must never reach the model.
fn prune_sentinel(table: &mut Table, deferred: &mut bool) {
    let before = table.fields.len();
    table.fields.retain(|(_, value)| !is_sentinel(value));
    if table.fields.len() != before {
        *deferred = true;
    }
    for (_, value) in &mut table.fields {
        if let LuaValue::Table(nested) = value {
            prune_sentinel(nested, deferred);
        }
    }
}

fn is_sentinel(value: &LuaValue) -> bool {
    matches!(value, LuaValue::Number(n) if *n == -999_999.0)
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(text: &'a str) -> Self {
        Parser {
            text,
            bytes: text.as_bytes(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.pos += 1;
        Some(byte)
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.pos += 1;
        }
    }

    fn error(&self, message: &'static str) -> DevelopError {
        DevelopError::Syntax {
            offset: self.pos,
            message,
        }
    }

    fn expect(&mut self, byte: u8, message: &'static str) -> Result<(), DevelopError> {
        if self.peek() == Some(byte) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.error(message))
        }
    }

    /// `s = { … }`, optionally followed by a semicolon. Empty input is an
    /// empty table.
    fn parse_document(&mut self) -> Result<Table, DevelopError> {
        self.skip_ws();
        if self.pos == self.bytes.len() {
            return Ok(Table::default());
        }
        if !self.peek().is_some_and(is_ident_start) {
            return Err(self.error("expected a settings variable"));
        }
        self.pos += 1;
        while self.peek().is_some_and(is_ident_continue) {
            self.pos += 1;
        }
        self.skip_ws();
        self.expect(b'=', "expected '=' after the settings variable")?;
        self.skip_ws();
        let table = self.parse_table(1)?;
        if table.fields.iter().any(|(name, _)| name.is_none()) {
            return Err(self.error("the settings table must use named fields"));
        }
        self.skip_ws();
        if self.peek() == Some(b';') {
            self.pos += 1;
            self.skip_ws();
        }
        if self.pos != self.bytes.len() {
            return Err(self.error("unexpected text after the settings table"));
        }
        Ok(table)
    }

    fn parse_table(&mut self, depth: usize) -> Result<Table, DevelopError> {
        if depth > DEVELOP_MAX_DEPTH {
            return Err(self.error("tables are nested too deeply"));
        }
        self.expect(b'{', "expected '{'")?;
        let mut table = Table::default();
        loop {
            self.skip_ws();
            match self.peek() {
                None => return Err(self.error("unterminated table")),
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(table);
                }
                Some(_) => {}
            }
            let name = self.try_key()?;
            self.skip_ws();
            let value = if name.is_some() {
                self.expect(b'=', "expected '=' after a field name")?;
                self.skip_ws();
                self.parse_value(depth + 1)?
            } else {
                self.parse_value(depth + 1)?
            };
            table.fields.push((name, value));
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(table);
                }
                None => return Err(self.error("unterminated table")),
                Some(_) => return Err(self.error("expected ',' or '}'")),
            }
        }
    }

    /// A field name (`name` or `["name"]`), or `None` if the entry is a
    /// positional array value.
    fn try_key(&mut self) -> Result<Option<String>, DevelopError> {
        match self.peek() {
            Some(b'[') => {
                self.pos += 1;
                self.skip_ws();
                let key = self.parse_string()?;
                self.skip_ws();
                self.expect(b']', "expected ']' after a bracketed field name")?;
                Ok(Some(key))
            }
            Some(byte) if is_ident_start(byte) => {
                let start = self.pos;
                self.pos += 1;
                while self.peek().is_some_and(is_ident_continue) {
                    self.pos += 1;
                }
                let end = self.pos;
                self.skip_ws();
                if self.peek() == Some(b'=') {
                    Ok(Some(self.text[start..end].to_owned()))
                } else {
                    // Not a field name: a bare identifier used as a value.
                    self.pos = end;
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }

    fn parse_value(&mut self, depth: usize) -> Result<LuaValue, DevelopError> {
        match self.peek() {
            Some(b'{') => Ok(LuaValue::Table(self.parse_table(depth)?)),
            Some(b'"' | b'\'') => Ok(LuaValue::Text(self.parse_string()?)),
            Some(byte) if byte.is_ascii_digit() || matches!(byte, b'-' | b'+' | b'.') => {
                Ok(LuaValue::Number(self.parse_number()?))
            }
            Some(byte) if is_ident_start(byte) => {
                let start = self.pos;
                self.pos += 1;
                while self.peek().is_some_and(is_ident_continue) {
                    self.pos += 1;
                }
                match &self.text[start..self.pos] {
                    "true" => Ok(LuaValue::Bool(true)),
                    "false" => Ok(LuaValue::Bool(false)),
                    _ => Err(self.error("function calls and variables are not allowed")),
                }
            }
            _ => Err(self.error("expected a value")),
        }
    }

    fn parse_number(&mut self) -> Result<f64, DevelopError> {
        let start = self.pos;
        if matches!(self.peek(), Some(b'+' | b'-')) {
            self.pos += 1;
        }
        let mut digits = 0;
        while self.peek().is_some_and(|b| b.is_ascii_digit()) {
            self.pos += 1;
            digits += 1;
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                self.pos += 1;
                digits += 1;
            }
        }
        if digits == 0 {
            return Err(self.error("invalid number"));
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            let mut exponent_digits = 0;
            while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                self.pos += 1;
                exponent_digits += 1;
            }
            if exponent_digits == 0 {
                return Err(self.error("invalid exponent"));
            }
        }
        let number: f64 = self.text[start..self.pos]
            .parse()
            .map_err(|_| self.error("invalid number"))?;
        if !number.is_finite() {
            return Err(self.error("number is not finite"));
        }
        Ok(number)
    }

    fn parse_string(&mut self) -> Result<String, DevelopError> {
        let quote = self
            .bump()
            .filter(|b| matches!(b, b'"' | b'\''))
            .ok_or_else(|| self.error("expected a quoted string"))?;
        let mut out = String::new();
        loop {
            let byte = self
                .bump()
                .ok_or_else(|| self.error("unterminated string"))?;
            if byte == quote {
                return Ok(out);
            }
            if byte == b'\\' {
                let escaped = self
                    .bump()
                    .ok_or_else(|| self.error("unterminated escape"))?;
                match escaped {
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'\\' => out.push('\\'),
                    b'"' => out.push('"'),
                    b'\'' => out.push('\''),
                    other if other.is_ascii() => {
                        // Unknown escapes keep both bytes, as Lua would.
                        out.push('\\');
                        out.push(other as char);
                    }
                    _ => return Err(self.error("invalid escape in string")),
                }
                continue;
            }
            // Copy one complete UTF-8 character. The input is a `&str`, so
            // taking the first character of the remaining slice is safe.
            let rest = &self.text[self.pos - 1..];
            let character = rest
                .chars()
                .next()
                .ok_or_else(|| self.error("unterminated string"))?;
            out.push(character);
            self.pos += character.len_utf8() - 1;
        }
    }
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

// ---------------------------------------------------------------------------
// Mapping to the neutral model
// ---------------------------------------------------------------------------

/// The eight colour bands: LightCraft spelling and the Lightroom key suffix.
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

/// Sliders that map 1:1: Lightroom key and LightCraft control id.
///
/// Source: the mapping table of `docs/research/develop-mapping.md`,
/// section 4.12, for the slider subset of phase 6a.
const CONTROL_IDS: [(&str, &str); 81] = [
    ("Exposure2012", "light.exposure"),
    ("Contrast2012", "light.contrast"),
    ("Highlights2012", "light.highlights"),
    ("Shadows2012", "light.shadows"),
    ("Whites2012", "light.whites"),
    ("Blacks2012", "light.blacks"),
    ("HDRMaxValue", "hdr.maxEv"),
    ("SDRBrightness", "hdr.sdrBrightness"),
    ("SDRContrast", "hdr.sdrContrast"),
    ("SDRHighlights", "hdr.sdrHighlights"),
    ("SDRShadows", "hdr.sdrShadows"),
    ("SDRWhites", "hdr.sdrWhites"),
    ("SDRClarity", "hdr.sdrClarity"),
    ("ParametricHighlights", "curve.highlights"),
    ("ParametricLights", "curve.lights"),
    ("ParametricDarks", "curve.darks"),
    ("ParametricShadows", "curve.shadows"),
    ("ParametricShadowSplit", "curve.splitShadows"),
    ("ParametricMidtoneSplit", "curve.splitMid"),
    ("ParametricHighlightSplit", "curve.splitHighlights"),
    ("CurveRefineSaturation", "curve.refineSaturation"),
    ("SplitToningShadowHue", "grading.shadows.hue"),
    ("SplitToningShadowSaturation", "grading.shadows.sat"),
    ("ColorGradeShadowLum", "grading.shadows.lum"),
    ("ColorGradeMidtoneHue", "grading.midtones.hue"),
    ("ColorGradeMidtoneSat", "grading.midtones.sat"),
    ("ColorGradeMidtoneLum", "grading.midtones.lum"),
    ("SplitToningHighlightHue", "grading.highlights.hue"),
    ("SplitToningHighlightSaturation", "grading.highlights.sat"),
    ("ColorGradeHighlightLum", "grading.highlights.lum"),
    ("ColorGradeGlobalHue", "grading.global.hue"),
    ("ColorGradeGlobalSat", "grading.global.sat"),
    ("ColorGradeGlobalLum", "grading.global.lum"),
    ("ColorGradeBlending", "grading.blending"),
    ("SplitToningBalance", "grading.balance"),
    ("Vibrance", "color.vibrance"),
    ("Saturation", "color.saturation"),
    ("Texture", "effects.texture"),
    ("Clarity2012", "effects.clarity"),
    ("Dehaze", "effects.dehaze"),
    ("PostCropVignetteAmount", "vignette.amount"),
    ("PostCropVignetteMidpoint", "vignette.midpoint"),
    ("PostCropVignetteRoundness", "vignette.roundness"),
    ("PostCropVignetteFeather", "vignette.feather"),
    ("PostCropVignetteHighlightContrast", "vignette.highlights"),
    ("GrainAmount", "grain.amount"),
    ("GrainSize", "grain.size"),
    ("GrainFrequency", "grain.roughness"),
    ("Sharpness", "detail.sharpenAmount"),
    ("SharpenRadius", "detail.sharpenRadius"),
    ("SharpenDetail", "detail.sharpenDetail"),
    ("SharpenEdgeMasking", "detail.sharpenMasking"),
    ("LuminanceSmoothing", "detail.nrLuminance"),
    ("LuminanceNoiseReductionDetail", "detail.nrDetail"),
    ("LuminanceNoiseReductionContrast", "detail.nrContrast"),
    ("ColorNoiseReduction", "detail.nrColor"),
    ("ColorNoiseReductionDetail", "detail.nrColorDetail"),
    ("ColorNoiseReductionSmoothness", "detail.nrColorSmoothness"),
    ("LensManualDistortionAmount", "optics.distortion"),
    ("VignetteAmount", "optics.vignetting"),
    ("VignetteMidpoint", "optics.vignettingMidpoint"),
    ("DefringePurpleAmount", "optics.defringePurple"),
    ("DefringePurpleHueLo", "optics.defringePurpleHueLo"),
    ("DefringePurpleHueHi", "optics.defringePurpleHueHi"),
    ("DefringeGreenAmount", "optics.defringeGreen"),
    ("DefringeGreenHueLo", "optics.defringeGreenHueLo"),
    ("DefringeGreenHueHi", "optics.defringeGreenHueHi"),
    ("PerspectiveVertical", "geometry.vertical"),
    ("PerspectiveHorizontal", "geometry.horizontal"),
    ("PerspectiveRotate", "geometry.rotate"),
    ("PerspectiveScale", "geometry.scale"),
    ("PerspectiveAspect", "geometry.aspect"),
    ("PerspectiveX", "geometry.offsetX"),
    ("PerspectiveY", "geometry.offsetY"),
    ("ShadowTint", "calibration.shadowsTint"),
    ("RedHue", "calibration.redHue"),
    ("RedSaturation", "calibration.redSat"),
    ("GreenHue", "calibration.greenHue"),
    ("GreenSaturation", "calibration.greenSat"),
    ("BlueHue", "calibration.blueHue"),
    ("BlueSaturation", "calibration.blueSat"),
];

/// The control id of a colour-mixer or black-and-white key, if it is one.
fn band_control(key: &str) -> Option<String> {
    let (template, suffix) = if let Some(rest) = key.strip_prefix("HueAdjustment") {
        ("mixer.{band}.hue", rest)
    } else if let Some(rest) = key.strip_prefix("SaturationAdjustment") {
        ("mixer.{band}.sat", rest)
    } else if let Some(rest) = key.strip_prefix("LuminanceAdjustment") {
        ("mixer.{band}.lum", rest)
    } else {
        let rest = key.strip_prefix("GrayMixer")?;
        ("bw.{band}", rest)
    };
    BANDS
        .iter()
        .find(|(_, crs)| *crs == suffix)
        .map(|(band, _)| template.replace("{band}", band))
}

fn control_id(key: &str) -> Option<String> {
    CONTROL_IDS
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, id)| (*id).to_owned())
        .or_else(|| band_control(key))
}

/// The LightCraft control a legacy Lightroom field maps to, with the
/// conversion LightCraft's importer applies. `None` for the legacy point
/// curve, which is not a slider.
fn old_control(key: &str, value: f64) -> Option<f64> {
    let converted = match key {
        "Exposure" => value,
        "Contrast" => value - 25.0,
        "HighlightRecovery" => -value,
        "FillLight" => value,
        "Shadows" => -(value - 5.0) * 4.0,
        "Brightness" => (value - 50.0) * 0.6,
        "Clarity" => value,
        _ => return None,
    };
    Some(converted.clamp(-100.0, 100.0))
}

fn old_control_id(key: &str) -> Option<&'static str> {
    match key {
        "Exposure" => Some("light.exposure"),
        "Contrast" => Some("light.contrast"),
        "HighlightRecovery" => Some("light.highlights"),
        "FillLight" => Some("light.shadows"),
        "Shadows" => Some("light.blacks"),
        "Brightness" => Some("light.whites"),
        "Clarity" => Some("effects.clarity"),
        _ => None,
    }
}

fn as_bool(value: &LuaValue) -> Option<bool> {
    match value {
        LuaValue::Bool(b) => Some(*b),
        LuaValue::Number(n) if *n == 0.0 => Some(false),
        LuaValue::Number(n) if *n == 1.0 => Some(true),
        _ => None,
    }
}

fn white_balance_mode(stored: &str) -> String {
    let normalised = stored.to_ascii_lowercase().replace(' ', "");
    match normalised.as_str() {
        "asshot" => "asShot",
        "auto" => "auto",
        "daylight" => "daylight",
        "cloudy" => "cloudy",
        "shade" => "shade",
        "tungsten" => "tungsten",
        "fluorescent" => "fluorescent",
        "flash" => "flash",
        _ => "custom",
    }
    .to_owned()
}

/// Upright (`PerspectiveUpright`) and the `Upright*` fields Lightroom stores
/// with it. Off (0 or absent): none of them has an effect and nothing is
/// reported. Auto, level, vertical and full become LightCraft's
/// `geometry.upright` mode, which LightCraft analyses itself, as its own
/// importer does; Lightroom's computed result (`UprightTransform_*`, focal
/// length, centre) is reported as not carried. Guided needs the guide lines
/// in LightCraft's frame and stays reported with all its fields.
/// Bookkeeping fields (version, preview flag, counts, digests) are never
/// reported.
fn read_upright(upright: Option<&LuaValue>, fields: &[&str], develop: &mut Develop) {
    let mode = match upright {
        None => return,
        Some(LuaValue::Number(n)) if *n == 0.0 => return,
        Some(LuaValue::Number(n)) if *n == 1.0 => Some("auto"),
        Some(LuaValue::Number(n)) if *n == 2.0 => Some("level"),
        Some(LuaValue::Number(n)) if *n == 3.0 => Some("vertical"),
        Some(LuaValue::Number(n)) if *n == 4.0 => Some("full"),
        Some(_) => None,
    };
    match mode {
        Some(mode) => {
            develop.partial.insert(
                "geometry.upright".to_owned(),
                MergeValue::Text(mode.to_owned()),
            );
        }
        None => push_unmapped(&mut develop.unmapped, "PerspectiveUpright"),
    }
    for name in fields {
        let bookkeeping = matches!(
            *name,
            "UprightVersion"
                | "UprightPreview"
                | "UprightTransformCount"
                | "UprightFourSegmentsCount"
        ) || name.ends_with("Digest");
        if !bookkeeping {
            push_unmapped(&mut develop.unmapped, name);
        }
    }
}

fn push_unmapped(unmapped: &mut Vec<String>, name: &str) {
    if !unmapped.iter().any(|known| known == name) {
        unmapped.push(name.to_owned());
    }
}

fn as_number(value: &LuaValue) -> Option<f64> {
    match value {
        LuaValue::Number(number) => Some(*number),
        _ => None,
    }
}

/// Reads one numeric crop key. A present value of the wrong type makes the
/// whole stored crop unreadable and is reported under its own name, so the
/// geometry is never guessed from defaults.
fn crop_number(
    crop: &mut RawCrop,
    unmapped: &mut Vec<String>,
    name: &str,
    value: &LuaValue,
) -> Option<f64> {
    let number = as_number(value);
    if number.is_none() {
        crop.unreadable = true;
        push_unmapped(unmapped, name);
    }
    number
}

fn curve_error(message: &'static str) -> DevelopError {
    DevelopError::Curve { message }
}

/// One stored tone-curve list as unit-range points.
///
/// `None` means the list holds fewer than two points and therefore no curve.
/// `Some([])` is the straight identity line, which also means "no curve". An
/// odd number of values, a non-number or a named field makes the whole
/// settings row unreadable, like a broken settings text.
fn curve_points(value: &LuaValue) -> Result<Option<Vec<CurvePoint>>, DevelopError> {
    let LuaValue::Table(table) = value else {
        return Err(curve_error("a tone curve must be a list of numbers"));
    };
    let mut numbers = Vec::new();
    for (name, value) in &table.fields {
        if name.is_some() {
            return Err(curve_error("a tone curve must be a list of numbers"));
        }
        match value {
            LuaValue::Number(number) => numbers.push(*number),
            _ => return Err(curve_error("a tone curve point is not a number")),
        }
    }
    if numbers.len() % 2 != 0 {
        return Err(curve_error("a tone curve needs x/y pairs"));
    }
    let mut points: Vec<CurvePoint> = numbers
        .as_chunks::<2>()
        .0
        .iter()
        .map(|[x, y]| CurvePoint {
            x: (x / 255.0).clamp(0.0, 1.0),
            y: (y / 255.0).clamp(0.0, 1.0),
        })
        .collect();
    if points.len() < 2 {
        return Ok(None);
    }
    if points.len() == 2 && points.iter().all(|point| point.x == point.y) {
        points.clear();
    }
    Ok(Some(points))
}

/// The crop fields of one settings text, before the orientation transform.
#[derive(Debug, Clone, Copy, Default)]
struct RawCrop {
    /// At least one of the five crop keys is present.
    present: bool,
    /// One crop key is present but its value has the wrong type (or
    /// `HasCrop` is not a flag). The caller reports the name, and no crop is
    /// built from the remaining defaults.
    unreadable: bool,
    has_crop: Option<bool>,
    left: Option<f64>,
    top: Option<f64>,
    right: Option<f64>,
    bottom: Option<f64>,
    angle: Option<f64>,
}

/// The stored crop in LightCraft's oriented, rotated frame.
///
/// Lightroom stores the edges on the unoriented image and a straighten angle
/// that turns the other way; LightCraft wants the edges of the oriented image
/// after that rotation, normalised to the oriented size. This mirrors the
/// transform documented in `docs/research/develop-mapping.md` §4.9.
fn crop_geometry(raw: &RawCrop, geometry: StoredGeometry) -> Option<CropGeometry> {
    if raw.unreadable {
        // A crop key that is present but not convertible is reported by the
        // reader. Guessing the remaining edges from defaults would turn it
        // into a wrong rectangle, so there is no crop at all.
        return None;
    }
    if raw.has_crop == Some(false) {
        // An explicitly disabled crop is the whole image.
        return Some(CropGeometry {
            x0: 0.0,
            y0: 0.0,
            x1: 1.0,
            y1: 1.0,
            angle: 0.0,
        });
    }
    if !raw.present && raw.has_crop.is_none() {
        return None;
    }
    let left = raw.left.unwrap_or(0.0).clamp(0.0, 1.0);
    let top = raw.top.unwrap_or(0.0).clamp(0.0, 1.0);
    let right = raw.right.unwrap_or(1.0).clamp(0.0, 1.0);
    let bottom = raw.bottom.unwrap_or(1.0).clamp(0.0, 1.0);
    if right <= left || bottom <= top {
        return None;
    }
    let stored_angle = raw.angle.unwrap_or(0.0).clamp(-45.0, 45.0);
    if geometry.orientation == StoredOrientation::Normal && stored_angle == 0.0 {
        return Some(CropGeometry {
            x0: left,
            y0: top,
            x1: right,
            y1: bottom,
            angle: 0.0,
        });
    }
    let stored_aspect = geometry.aspect();
    let (oriented_width, oriented_height) = if geometry.orientation.swaps_axes() {
        (1.0, stored_aspect)
    } else {
        (stored_aspect, 1.0)
    };
    // A mirror turns the straighten angle the other way as well.
    let angle = if geometry.orientation.mirrored() {
        stored_angle
    } else {
        -stored_angle
    };
    let (sin, cos) = angle.to_radians().sin_cos();
    let (center_x, center_y) = (oriented_width / 2.0, oriented_height / 2.0);
    let corner = |x: f64, y: f64| {
        let (u, v) = geometry
            .orientation
            .map(x * stored_aspect, y, stored_aspect, 1.0);
        let (du, dv) = (u - center_x, v - center_y);
        (
            center_x + du * cos - dv * sin,
            center_y + du * sin + dv * cos,
        )
    };
    let (ax, ay) = corner(left, top);
    let (bx, by) = corner(right, bottom);
    let (x0, x1) = if ax < bx { (ax, bx) } else { (bx, ax) };
    let (y0, y1) = if ay < by { (ay, by) } else { (by, ay) };
    let crop = CropGeometry {
        x0: x0 / oriented_width,
        y0: y0 / oriented_height,
        x1: x1 / oriented_width,
        y1: y1 / oriented_height,
        angle,
    };
    (crop.x1 > crop.x0 && crop.y1 > crop.y0).then_some(crop)
}

/// The curves of one settings text: the 2012 master curve, its three channel
/// curves only when all three exist, and the legacy curve only when no 2012
/// curve key is present at all.
fn curves_from_parts(
    master: Option<Vec<CurvePoint>>,
    red: Option<Vec<CurvePoint>>,
    green: Option<Vec<CurvePoint>>,
    blue: Option<Vec<CurvePoint>>,
    has_2012_curve: bool,
    old_master: Option<Vec<CurvePoint>>,
) -> Curves {
    let mut curves = Curves::default();
    if let Some(master) = master {
        curves.master = master;
        if let (Some(red), Some(green), Some(blue)) = (red, green, blue) {
            curves.red = red;
            curves.green = green;
            curves.blue = blue;
        }
    } else if !has_2012_curve && let Some(master) = old_master {
        curves.master = master;
    }
    curves
}

fn map_table(
    table: &Table,
    develop: &mut Develop,
    geometry: StoredGeometry,
) -> Result<(), DevelopError> {
    // LightCraft ignores the whole old-process block as soon as the settings
    // hold any `*2012` key; presets of newer versions carry the old fields at
    // their defaults.
    let newer_process = table
        .fields
        .iter()
        .any(|(name, _)| name.as_deref().is_some_and(|n| n.ends_with("2012")));
    // (temperature, tint) as stored: `Temperature`/`Tint` and the
    // `Incremental*` pair.
    let mut absolute: (Option<f64>, Option<f64>) = (None, None);
    let mut incremental: (Option<f64>, Option<f64>) = (None, None);
    let mut master_2012 = None;
    let mut red_2012 = None;
    let mut green_2012 = None;
    let mut blue_2012 = None;
    let mut old_master = None;
    let mut has_2012_curve = false;
    let mut crop = RawCrop::default();
    let mut upright: Option<&LuaValue> = None;
    let mut upright_fields: Vec<&str> = Vec::new();

    for (name, value) in &table.fields {
        // The parser guarantees named top-level fields.
        let Some(name) = name.as_deref() else {
            continue;
        };
        if name == "ProcessVersion" {
            // Bookkeeping only; LightCraft always uses its own process.
            continue;
        }
        if let Some(control) = control_id(name) {
            match value {
                LuaValue::Number(number) => {
                    develop.controls.insert(control, *number);
                }
                _ => push_unmapped(&mut develop.unmapped, name),
            }
            continue;
        }
        if let Some(control) = old_control_id(name) {
            if newer_process {
                // Suppressed by the 2012 block; not reported, the newer
                // fields carry the same settings.
                continue;
            }
            match value {
                LuaValue::Number(number) => {
                    if let Some(converted) = old_control(name, *number) {
                        develop.controls.insert(control.to_owned(), converted);
                    }
                }
                _ => push_unmapped(&mut develop.unmapped, name),
            }
            continue;
        }
        match name {
            "HDREditMode" => match as_bool(value) {
                Some(on) => {
                    develop
                        .partial
                        .insert("hdr.enabled".to_owned(), MergeValue::Bool(on));
                }
                None => push_unmapped(&mut develop.unmapped, name),
            },
            "ConvertToGrayscale" => match as_bool(value) {
                Some(on) => {
                    let treatment = if on { "bw" } else { "color" };
                    develop.partial.insert(
                        "treatment".to_owned(),
                        MergeValue::Text(treatment.to_owned()),
                    );
                }
                None => push_unmapped(&mut develop.unmapped, name),
            },
            "LensProfileEnable" => match as_bool(value) {
                // Only an enabled profile carries over: a disabled one leaves
                // LightCraft's embedded corrections as they are.
                Some(true) => {
                    develop
                        .partial
                        .insert("optics.lens_profile".to_owned(), MergeValue::Bool(true));
                }
                Some(false) => {}
                None => push_unmapped(&mut develop.unmapped, name),
            },
            "AutoLateralCA" => match as_bool(value) {
                Some(on) => {
                    develop
                        .partial
                        .insert("optics.remove_ca".to_owned(), MergeValue::Bool(on));
                }
                None => push_unmapped(&mut develop.unmapped, name),
            },
            "WhiteBalance" => match value {
                LuaValue::Text(stored) => {
                    develop.white_balance.mode = Some(white_balance_mode(stored));
                }
                _ => push_unmapped(&mut develop.unmapped, name),
            },
            // Kept aside: whether they are carried depends on the mode (read
            // after the loop) and, for Kelvin values, on LightCraft's scale
            // for the photo. The report counts them in its own white-balance
            // fields, not as unmapped names.
            "Temperature" | "Tint" | "IncrementalTemperature" | "IncrementalTint" => match value {
                LuaValue::Number(number) => match name {
                    "Temperature" => absolute.0 = Some(*number),
                    "Tint" => absolute.1 = Some(*number),
                    "IncrementalTemperature" => incremental.0 = Some(*number),
                    _ => incremental.1 = Some(*number),
                },
                _ => push_unmapped(&mut develop.unmapped, name),
            },
            "ToneCurvePV2012" => {
                has_2012_curve = true;
                master_2012 = curve_points(value)?;
            }
            "ToneCurvePV2012Red" => red_2012 = curve_points(value)?,
            "ToneCurvePV2012Green" => green_2012 = curve_points(value)?,
            "ToneCurvePV2012Blue" => blue_2012 = curve_points(value)?,
            "ToneCurve" => old_master = curve_points(value)?,
            "CropLeft" => {
                crop.present = true;
                let number = crop_number(&mut crop, &mut develop.unmapped, name, value);
                crop.left = number;
            }
            "CropTop" => {
                crop.present = true;
                let number = crop_number(&mut crop, &mut develop.unmapped, name, value);
                crop.top = number;
            }
            "CropRight" => {
                crop.present = true;
                let number = crop_number(&mut crop, &mut develop.unmapped, name, value);
                crop.right = number;
            }
            "CropBottom" => {
                crop.present = true;
                let number = crop_number(&mut crop, &mut develop.unmapped, name, value);
                crop.bottom = number;
            }
            "CropAngle" => {
                crop.present = true;
                let number = crop_number(&mut crop, &mut develop.unmapped, name, value);
                crop.angle = number;
            }
            "HasCrop" => match as_bool(value) {
                Some(on) => crop.has_crop = Some(on),
                None => {
                    crop.unreadable = true;
                    push_unmapped(&mut develop.unmapped, name);
                }
            },
            // Mask containers are read after the loop, in their fixed order,
            // so their order in the settings text does not matter.
            "MaskGroupBasedCorrections"
            | "GradientBasedCorrections"
            | "CircularGradientBasedCorrections"
            | "PaintBasedCorrections" => {}
            // Spot containers are read after the loop as well, so their
            // position in the settings text does not matter.
            "RetouchAreas" | "RetouchInfo" => {}
            // Decided after the loop: the Upright fields only matter when
            // Upright is on.
            "PerspectiveUpright" => upright = Some(value),
            _ if name.starts_with("Upright") => upright_fields.push(name),
            // Reported by name only; it never becomes a develop command.
            "CameraProfile" => match value {
                LuaValue::Text(profile) if !profile.is_empty() => {
                    develop.camera_profile = Some(profile.clone());
                }
                LuaValue::Text(_) => {}
                _ => push_unmapped(&mut develop.unmapped, name),
            },
            _ => push_unmapped(&mut develop.unmapped, name),
        }
    }

    // Only a custom white balance carries its numbers; a named mode is
    // carried by the mode (see `WhiteBalanceHint`). Without a stored mode
    // the numbers count as custom, as in LightCraft's own mapper.
    let custom = develop
        .white_balance
        .mode
        .as_deref()
        .is_none_or(|mode| mode == "custom");
    let (hint, (temperature, tint)) = if !custom {
        (WhiteBalanceHint::None, (None, None))
    } else if incremental != (None, None) {
        (WhiteBalanceHint::Relative, incremental)
    } else if absolute != (None, None) {
        (WhiteBalanceHint::Undecidable, absolute)
    } else {
        (WhiteBalanceHint::None, (None, None))
    };
    develop.white_balance.hint = hint;
    develop.white_balance.temperature = temperature;
    develop.white_balance.tint = tint;
    read_upright(upright, &upright_fields, develop);
    develop.curves = curves_from_parts(
        master_2012,
        red_2012,
        green_2012,
        blue_2012,
        has_2012_curve,
        old_master,
    );
    develop.crop = crop_geometry(&crop, geometry);
    let aspect = geometry.displayed_aspect();
    read_masks(table, aspect, develop);
    read_spots(table, aspect, develop);
    Ok(())
}

/// The four catalog containers that hold corrections, in the order their
/// masks are numbered.
const MASK_CONTAINERS: [&str; 4] = [
    "MaskGroupBasedCorrections",
    "GradientBasedCorrections",
    "CircularGradientBasedCorrections",
    "PaintBasedCorrections",
];

/// The `Local*` correction sliders with a LightCraft `adjust` counterpart:
/// `(catalog field, adjust field, scale)`. The 2012 spellings come first, so
/// they win over the older ones.
const LOCAL_SLIDERS: [(&str, &str, f64); 22] = [
    ("LocalExposure2012", "exposure", 4.0),
    ("LocalContrast2012", "contrast", 100.0),
    ("LocalHighlights2012", "highlights", 100.0),
    ("LocalShadows2012", "shadows", 100.0),
    ("LocalWhites2012", "whites", 100.0),
    ("LocalBlacks2012", "blacks", 100.0),
    ("LocalClarity2012", "clarity", 100.0),
    ("LocalTexture", "texture", 100.0),
    ("LocalDehaze", "dehaze", 100.0),
    ("LocalTemperature", "temp", 100.0),
    ("LocalTint", "tint", 100.0),
    ("LocalSaturation", "saturation", 100.0),
    ("LocalHue", "hue", 100.0),
    ("LocalSharpness", "sharpness", 100.0),
    ("LocalLuminanceNoise", "noise", 100.0),
    ("LocalMoire", "moire", 100.0),
    ("LocalDefringe", "defringe", 100.0),
    ("LocalToningSaturation", "color_sat", 100.0),
    ("LocalToningHue", "color_hue", 1.0),
    ("LocalExposure", "exposure", 4.0),
    ("LocalContrast", "contrast", 100.0),
    ("LocalClarity", "clarity", 100.0),
];

/// The named field of a table.
fn table_field<'a>(table: &'a Table, name: &str) -> Option<&'a LuaValue> {
    table
        .fields
        .iter()
        .find_map(|(key, value)| (key.as_deref() == Some(name)).then_some(value))
}

fn field_number(table: &Table, name: &str) -> Option<f64> {
    table_field(table, name).and_then(as_number)
}

/// A code field such as `MaskSubType`: the catalog stores it as a number,
/// an XMP packet as text. Both come back as text; a number only when it is
/// a whole number.
fn field_code(table: &Table, name: &str) -> Option<String> {
    match table_field(table, name) {
        Some(LuaValue::Text(text)) if !text.is_empty() => Some(text.clone()),
        Some(LuaValue::Number(n)) if n.fract() == 0.0 && n.abs() < 1e9 => {
            Some(format!("{}", *n as i64))
        }
        _ => None,
    }
}

fn field_text<'a>(table: &'a Table, name: &str) -> Option<&'a str> {
    match table_field(table, name) {
        Some(LuaValue::Text(text)) if !text.is_empty() => Some(text),
        _ => None,
    }
}

/// A numeric field with a fallback. A missing field keeps `default` silently;
/// a present value of the wrong type is reported under the stable `report`
/// name and `default` is used only as a fallback, never silently.
fn number_or_default(
    table: &Table,
    name: &str,
    default: f64,
    report: &str,
    skipped: &mut Vec<String>,
) -> f64 {
    match table_field(table, name) {
        None => default,
        Some(LuaValue::Number(number)) => *number,
        Some(_) => {
            skipped.push(report.to_owned());
            default
        }
    }
}

/// A flag field with a fallback, reported like [`number_or_default`].
fn flag_or_default(
    table: &Table,
    name: &str,
    default: bool,
    report: &str,
    skipped: &mut Vec<String>,
) -> bool {
    match table_field(table, name) {
        None => default,
        Some(value) => match as_bool(value) {
            Some(flag) => flag,
            None => {
                skipped.push(report.to_owned());
                default
            }
        },
    }
}

/// A component's `Version`, or `None` when the field is missing or unreadable.
/// An unreadable value is reported and treated like a missing one.
fn component_version(component: &Table, skipped: &mut Vec<String>) -> Option<f64> {
    match table_field(component, "Version") {
        None => None,
        Some(LuaValue::Number(version)) => Some(*version),
        Some(_) => {
            skipped.push("version".to_owned());
            None
        }
    }
}

// Provenance: the mask and spot mapping below leans on LightCraft's
// `crates/engine/src/crs_masks.rs` (MIT OR Apache-2.0, license-compatible).
// No code was copied from lrcat-extractor (MPL-2.0).

/// Reads the four mask containers into `develop`, in the fixed container
/// order. Unsupported components, local sliders and a container that is not a
/// list are reported by a stable `Mask: <kind>` name, never dropped silently.
/// `aspect` is the displayed photo's width divided by its height.
fn read_masks(table: &Table, aspect: f64, develop: &mut Develop) {
    let mut skipped: Vec<String> = Vec::new();
    for container in MASK_CONTAINERS {
        let Some(value) = table_field(table, container) else {
            continue;
        };
        let LuaValue::Table(list) = value else {
            skipped.push(format!("container {container}"));
            continue;
        };
        for (_, value) in &list.fields {
            let LuaValue::Table(correction) = value else {
                skipped.push("unknown mask".to_owned());
                continue;
            };
            if let Some(mask) =
                read_correction(correction, container, aspect, develop, &mut skipped)
            {
                develop.masks.push(mask);
            }
        }
    }
    skipped.sort_unstable();
    skipped.dedup();
    for kind in skipped {
        push_unmapped(&mut develop.unmapped, &format!("Mask: {kind}"));
    }
}

/// Reads spot removal into `develop`: the current `RetouchAreas` container,
/// and the older `RetouchInfo` lines only when that yields no spot. Anything
/// that cannot be carried over is reported by a stable `Spot: <kind>` name.
fn read_spots(table: &Table, aspect: f64, develop: &mut Develop) {
    let mut spots: Vec<Spot> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    match table_field(table, "RetouchAreas") {
        Some(LuaValue::Table(list)) => {
            for (_, value) in &list.fields {
                let LuaValue::Table(area) = value else {
                    skipped.push("unknown spot".to_owned());
                    continue;
                };
                match read_spot(area, aspect, &mut skipped) {
                    Ok(spot) => spots.push(spot),
                    Err(kind) => skipped.push(kind),
                }
            }
        }
        Some(_) => skipped.push("container RetouchAreas".to_owned()),
        None => {}
    }
    if spots.is_empty()
        && let Some(value) = table_field(table, "RetouchInfo")
    {
        match value {
            LuaValue::Table(lines) => {
                for (_, value) in &lines.fields {
                    let LuaValue::Text(line) = value else {
                        skipped.push("unknown spot".to_owned());
                        continue;
                    };
                    match legacy_spot(line, aspect, &mut skipped) {
                        Some(spot) => spots.push(spot),
                        None => skipped.push("legacy line".to_owned()),
                    }
                }
            }
            _ => skipped.push("container RetouchInfo".to_owned()),
        }
    }
    develop.spots = spots;
    skipped.sort_unstable();
    skipped.dedup();
    for kind in skipped {
        push_unmapped(&mut develop.unmapped, &format!("Spot: {kind}"));
    }
}

/// One `RetouchAreas` entry. `Err(kind)` names what cannot be carried over.
/// `aspect` is the displayed photo's width divided by its height.
fn read_spot(area: &Table, aspect: f64, skipped: &mut Vec<String>) -> Result<Spot, String> {
    let mode = match table_field(area, "SpotType") {
        None => SpotMode::Heal,
        Some(LuaValue::Text(text)) if text == "clone" => SpotMode::Clone,
        Some(LuaValue::Text(_)) => SpotMode::Heal,
        // A present value of the wrong type is reported; `heal` is only the
        // fallback.
        Some(_) => {
            skipped.push("spot type".to_owned());
            SpotMode::Heal
        }
    };
    let mask = match table_field(area, "Masks") {
        Some(LuaValue::Table(masks)) => match masks.fields.first() {
            Some((_, LuaValue::Table(mask))) => mask,
            _ => return Err("no mask".to_owned()),
        },
        _ => return Err("no mask".to_owned()),
    };
    let width_to_long = aspect.min(1.0);
    let (points, size) = match field_text(mask, "What").unwrap_or("") {
        "Mask/Ellipse" => {
            let (Some(x), Some(y), Some(size_x)) = (
                field_number(mask, "X"),
                field_number(mask, "Y"),
                field_number(mask, "SizeX"),
            ) else {
                return Err("ellipse spot".to_owned());
            };
            (vec![Point { x, y }], size_x * width_to_long)
        }
        "Mask/CircularGradient" => {
            let (Some(top), Some(left), Some(bottom), Some(right)) = (
                field_number(mask, "Top"),
                field_number(mask, "Left"),
                field_number(mask, "Bottom"),
                field_number(mask, "Right"),
            ) else {
                return Err("circular spot".to_owned());
            };
            (
                vec![Point {
                    x: (left + right) / 2.0,
                    y: (top + bottom) / 2.0,
                }],
                (right - left).abs() / 2.0 * width_to_long,
            )
        }
        "Mask/Paint" => {
            // A spot is one path: its segments are joined, as in
            // LightCraft's importer.
            let points: Vec<Point> = dab_segments(mask, skipped).into_iter().flatten().collect();
            if points.is_empty() {
                return Err("empty brush".to_owned());
            }
            (
                points,
                number_or_default(mask, "Radius", 0.02, "radius", skipped),
            )
        }
        "" => return Err("unknown spot".to_owned()),
        other => return Err(other.trim_start_matches("Mask/").to_owned()),
    };
    let size = size.clamp(0.001, 0.25);
    let feather =
        (number_or_default(area, "Feather", 0.0, "feather", skipped) * 100.0).clamp(0.0, 100.0);
    let opacity =
        (number_or_default(area, "Opacity", 1.0, "opacity", skipped) * 100.0).clamp(0.0, 100.0);
    let source_state = match table_field(area, "SourceState") {
        None => None,
        Some(LuaValue::Text(text)) => Some(text.as_str()),
        Some(_) => {
            skipped.push("source state".to_owned());
            None
        }
    };
    Ok(Spot {
        mode,
        points: points.clone(),
        size,
        feather,
        opacity,
        source_offset: spot_source_offset(
            area,
            "SourceX",
            "OffsetY",
            source_state,
            points.first().copied(),
            skipped,
        ),
    })
}

/// One `RetouchInfo` line as a spot, or `None` when a required value is
/// missing or unreadable.
fn legacy_spot(line: &str, aspect: f64, skipped: &mut Vec<String>) -> Option<Spot> {
    let mut values: std::collections::BTreeMap<&str, &str> = std::collections::BTreeMap::new();
    for part in line.split(',') {
        if let Some((key, value)) = part.split_once('=') {
            values.insert(key.trim(), value.trim());
        }
    }
    let x = legacy_number(&values, "centerX", skipped)?;
    let y = legacy_number(&values, "centerY", skipped)?;
    let radius = legacy_number(&values, "radius", skipped)?;
    let mode = match values.get("spotType") {
        Some(&"clone") => SpotMode::Clone,
        _ => SpotMode::Heal,
    };
    let source_offset = match (
        values.get("sourceState").copied(),
        legacy_number(&values, "sourceX", skipped),
        legacy_number(&values, "sourceY", skipped),
    ) {
        (None | Some("sourceSetExplicitly"), Some(source_x), Some(source_y))
            if source_x != x || source_y != y =>
        {
            Some(Point {
                x: source_x - x,
                y: source_y - y,
            })
        }
        _ => None,
    };
    let opacity =
        (legacy_number(&values, "opacity", skipped).unwrap_or(1.0) * 100.0).clamp(0.0, 100.0);
    Some(Spot {
        mode,
        points: vec![Point { x, y }],
        size: (radius * aspect.min(1.0)).clamp(0.001, 0.25),
        feather: 0.0,
        opacity,
        source_offset,
    })
}

/// One numeric value of a legacy comma-separated line. A missing key returns
/// `None` silently; a present value that is not a finite number is reported
/// under the key name and also returns `None`.
fn legacy_number(
    values: &std::collections::BTreeMap<&str, &str>,
    key: &str,
    skipped: &mut Vec<String>,
) -> Option<f64> {
    match values.get(key) {
        None => None,
        Some(value) => match value
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())
        {
            Some(number) => Some(number),
            None => {
                skipped.push(key.to_owned());
                None
            }
        },
    }
}

/// The offset from a spot's first target point to an explicit source, or
/// `None` when the source is automatic, missing or at the same position.
fn spot_source_offset(
    area: &Table,
    source_x_field: &str,
    source_y_field: &str,
    source_state: Option<&str>,
    target: Option<Point>,
    skipped: &mut Vec<String>,
) -> Option<Point> {
    let explicit = source_state.is_none_or(|state| state == "sourceSetExplicitly");
    let source_x = match table_field(area, source_x_field) {
        None => None,
        Some(LuaValue::Number(number)) => Some(*number),
        Some(_) => {
            skipped.push("source x".to_owned());
            None
        }
    };
    let source_y = match table_field(area, source_y_field) {
        None => None,
        Some(LuaValue::Number(number)) => Some(*number),
        Some(_) => {
            skipped.push("source y".to_owned());
            None
        }
    };
    let (Some(source_x), Some(source_y), Some(target)) = (source_x, source_y, target) else {
        return None;
    };
    if !explicit {
        return None;
    }
    let (dx, dy) = (source_x - target.x, source_y - target.y);
    (dx.is_finite() && dy.is_finite() && (dx != 0.0 || dy != 0.0)).then_some(Point { x: dx, y: dy })
}

/// One correction, or `None` when it is inactive or holds no supported
/// component. `develop` supplies the running mask count for default names.
fn read_correction(
    correction: &Table,
    container: &str,
    aspect: f64,
    develop: &Develop,
    skipped: &mut Vec<String>,
) -> Option<Mask> {
    if !flag_or_default(correction, "CorrectionActive", true, "active flag", skipped) {
        return None;
    }
    // Unsupported local sliders are reported even when every component of the
    // correction is unsupported and no mask is built from it.
    report_unmapped_locals(correction, skipped);
    let components = match table_field(correction, "CorrectionMasks") {
        Some(LuaValue::Table(components)) if !components.fields.is_empty() => components,
        _ => {
            skipped.push("no components".to_owned());
            return None;
        }
    };
    let mut parts: Vec<MaskComponent> = Vec::new();
    for (_, value) in &components.fields {
        let LuaValue::Table(component) = value else {
            skipped.push("unknown mask".to_owned());
            continue;
        };
        match read_component(component, container, aspect, skipped) {
            Ok(part) => parts.push(part),
            Err(kind) => skipped.push(kind),
        }
    }
    if parts.is_empty() {
        return None;
    }
    let id = u32::try_from(develop.masks.len() + 1).unwrap_or(u32::MAX);
    let name = match table_field(correction, "CorrectionName") {
        Some(LuaValue::Text(text)) if !text.is_empty() => text.clone(),
        Some(LuaValue::Text(_)) => format!("Mask {id}"),
        Some(_) => {
            skipped.push("name".to_owned());
            format!("Mask {id}")
        }
        None => format!("Mask {id}"),
    };
    Some(Mask {
        id,
        name,
        visible: true,
        invert: flag_or_default(correction, "MaskInverted", false, "inverted", skipped),
        components: merge_brush_parts(parts),
        adjust: local_adjust(correction, skipped),
    })
}

/// Runs of brush components with the same op become one component, as in
/// LightCraft's own importer.
fn merge_brush_parts(parts: Vec<MaskComponent>) -> Vec<MaskComponent> {
    let mut merged: Vec<MaskComponent> = Vec::new();
    for part in parts {
        if let MaskShape::Brush { strokes } = &part.shape
            && let Some(previous) = merged
                .iter_mut()
                .find(|other| other.op == part.op && matches!(other.shape, MaskShape::Brush { .. }))
            && let MaskShape::Brush {
                strokes: previous_strokes,
            } = &mut previous.shape
        {
            previous_strokes.extend(strokes.iter().cloned());
            continue;
        }
        merged.push(part);
    }
    merged
}

/// The correction's `adjust` object: the mapped local sliders plus `amount`,
/// which is `CorrectionAmount × 100` clamped to 0..200. A present slider of
/// the wrong type is reported under its Lightroom name and left out.
fn local_adjust(
    correction: &Table,
    skipped: &mut Vec<String>,
) -> std::collections::BTreeMap<String, f64> {
    let mut adjust = std::collections::BTreeMap::new();
    for (field, ours, scale) in LOCAL_SLIDERS {
        if adjust.contains_key(ours) {
            continue;
        }
        let Some(value) = field_number(correction, field).filter(|v| v.is_finite()) else {
            if table_field(correction, field).is_some() {
                skipped.push((*field).to_owned());
            }
            continue;
        };
        let limit = match ours {
            "exposure" => 4.0,
            "color_hue" => 360.0,
            _ => 100.0,
        };
        let lower = if ours == "color_hue" { 0.0 } else { -limit };
        adjust.insert(ours.to_owned(), (value * scale).clamp(lower, limit));
    }
    let amount = number_or_default(
        correction,
        "CorrectionAmount",
        1.0,
        "correction amount",
        skipped,
    );
    adjust.insert("amount".to_owned(), (amount * 100.0).clamp(0.0, 200.0));
    adjust
}

/// Reports `Local*` sliders without a counterpart when they are set. A
/// `LocalCurveRefineSaturation` of 100 is neutral, not a value. A present
/// slider of the wrong type is always reported, never dropped silently.
fn report_unmapped_locals(correction: &Table, skipped: &mut Vec<String>) {
    for (name, value) in &correction.fields {
        let Some(name) = name.as_deref().filter(|n| n.starts_with("Local")) else {
            continue;
        };
        if LOCAL_SLIDERS.iter().any(|(field, ..)| *field == name) {
            continue;
        }
        let neutral = if name == "LocalCurveRefineSaturation" {
            100.0
        } else {
            0.0
        };
        match as_number(value) {
            Some(v) if v == neutral => {}
            Some(_) | None => skipped.push(name.to_owned()),
        }
    }
}

/// One mask component. `Err(kind)` names an unsupported shape.
fn read_component(
    component: &Table,
    container: &str,
    aspect: f64,
    skipped: &mut Vec<String>,
) -> Result<MaskComponent, String> {
    let op = match table_field(component, "MaskBlendMode") {
        None => MaskOp::Add,
        Some(LuaValue::Number(mode)) if *mode == 0.0 => MaskOp::Add,
        Some(LuaValue::Number(mode)) if *mode == 1.0 => MaskOp::Subtract,
        Some(LuaValue::Number(mode)) if *mode == 2.0 => MaskOp::Intersect,
        // An unknown mode must not silently become `add`.
        Some(LuaValue::Number(mode)) => return Err(format!("blend mode {mode}")),
        // A present value of the wrong type is reported and `add` is only the
        // fallback.
        Some(_) => {
            skipped.push("blend mode".to_owned());
            MaskOp::Add
        }
    };
    let invert = flag_or_default(component, "MaskInverted", false, "inverted", skipped);
    let what = field_text(component, "What").unwrap_or("");
    let shape = match what {
        "Mask/Gradient" => {
            let mut g = |key: &str, default: f64| {
                number_or_default(component, key, default, "gradient position", skipped)
            };
            MaskShape::Linear {
                start: Point {
                    x: g("FullX", 0.5),
                    y: g("FullY", 0.0),
                },
                end: Point {
                    x: g("ZeroX", 0.5),
                    y: g("ZeroY", 1.0),
                },
            }
        }
        "Mask/CircularGradient" => {
            let mut g = |key: &str, default: f64| {
                number_or_default(component, key, default, "radial bounds", skipped)
            };
            let (left, right, top, bottom) = (
                g("Left", 0.25),
                g("Right", 0.75),
                g("Top", 0.25),
                g("Bottom", 0.75),
            );
            let (sx, sy) = if aspect >= 1.0 {
                (1.0, 1.0 / aspect)
            } else {
                (aspect, 1.0)
            };
            MaskShape::Radial {
                center: Point {
                    x: (left + right) / 2.0,
                    y: (top + bottom) / 2.0,
                },
                rx: ((right - left).abs() / 2.0 * sx).max(1e-3),
                ry: ((bottom - top).abs() / 2.0 * sy).max(1e-3),
                angle: number_or_default(component, "Angle", 0.0, "angle", skipped),
                feather: radial_feather(component, container, skipped),
                invert: !radial_inside(component, container, skipped),
            }
        }
        "Mask/Paint" => MaskShape::Brush {
            strokes: paint_strokes(component, skipped)?,
        },
        // Lightroom's current brush: a group of paint strokes (erase strokes
        // included) with the group's own blend mode and invert flag.
        "Mask/Aggregate" => MaskShape::Brush {
            strokes: aggregate_strokes(component, skipped)?,
        },
        "Mask/Image" => match field_code(component, "MaskSubType").as_deref() {
            Some("1") => MaskShape::Subject,
            Some("2") => MaskShape::Sky,
            // Subtype 0 is an object selection unless its category is the
            // background (22).
            Some("0") if field_code(component, "MaskSubCategoryID").as_deref() == Some("22") => {
                MaskShape::Background
            }
            other => {
                return Err(format!("AI mask ({})", other.unwrap_or("?")));
            }
        },
        "Mask/RangeMask" => {
            let range = match table_field(component, "CorrectionRangeMask") {
                Some(LuaValue::Table(range)) => range,
                _ => component,
            };
            match field_code(range, "Type").as_deref() {
                Some("2") => {
                    let values = range_values(range, "LumRange", "0 0 1 1", skipped);
                    let [a, b, c, d] = values[..] else {
                        return Err("luminance range".to_owned());
                    };
                    let open_top = c >= 1.0;
                    let (a, b, c, d) = (
                        lightness_to_range(a),
                        lightness_to_range(b),
                        lightness_to_range(c),
                        lightness_to_range(d),
                    );
                    let (hi, hi_feather) = if open_top {
                        (1.0, 0.0)
                    } else {
                        (c, (d - c).max(0.0))
                    };
                    MaskShape::LuminanceRange {
                        lo: b,
                        hi,
                        lo_feather: (b - a).max(0.0),
                        hi_feather,
                    }
                }
                Some("3") => {
                    let values = range_values(range, "DepthRange", "0 0 1 1", skipped);
                    let [a, b, c, _] = values[..] else {
                        return Err("depth range".to_owned());
                    };
                    MaskShape::DepthRange {
                        lo: b,
                        hi: c,
                        feather: (b - a).max(0.0),
                    }
                }
                _ => return Err("colour range".to_owned()),
            }
        }
        "" => return Err("unknown mask".to_owned()),
        other => return Err(other.trim_start_matches("Mask/").to_owned()),
    };
    Ok(MaskComponent { op, invert, shape })
}

/// Four whitespace-separated numbers of a range field. A missing field keeps
/// the default silently; a present field of the wrong type or with a token
/// that is not a number is reported as `range values`.
fn range_values(range: &Table, name: &str, default: &str, skipped: &mut Vec<String>) -> Vec<f64> {
    match table_field(range, name) {
        Some(LuaValue::Text(text)) => {
            let mut values = Vec::new();
            let mut invalid = false;
            for token in text.split_whitespace() {
                match token.parse::<f64>() {
                    Ok(value) => values.push(value),
                    Err(_) => invalid = true,
                }
            }
            if invalid {
                skipped.push("range values".to_owned());
            }
            values
        }
        Some(_) => {
            skipped.push("range values".to_owned());
            default
                .split_whitespace()
                .filter_map(|v| v.parse::<f64>().ok())
                .collect()
        }
        None => default
            .split_whitespace()
            .filter_map(|v| v.parse::<f64>().ok())
            .collect(),
    }
}

/// One entry of a `Dabs` list.
#[derive(Debug, PartialEq)]
enum Dab {
    /// `M x y`: a point that starts a new, unconnected segment.
    Move(Point),
    /// `d x y`: a point connected to the one before.
    Line(Point),
    /// Per-stroke state between the points (`f` flow, `h` hardness, `r`
    /// radius); it carries no position.
    State,
}

/// Parses one `Dabs` entry; `None` for anything unknown.
fn parse_dab(dab: &str) -> Option<Dab> {
    let mut parts = dab.split_whitespace();
    let word = parts.next()?;
    if matches!(word, "f" | "h" | "r") {
        return Some(Dab::State);
    }
    let mut numbers = parts.map(|v| v.parse::<f64>().ok());
    let (Some(Some(x)), Some(Some(y))) = (numbers.next(), numbers.next()) else {
        return None;
    };
    match word {
        "M" => Some(Dab::Move(Point { x, y })),
        "d" => Some(Dab::Line(Point { x, y })),
        _ => None,
    }
}

/// The points of a `Dabs` list as connected segments: every `M` starts a new
/// segment. A list of the wrong type or an unknown entry is reported as
/// `dabs`; the readable points are kept.
fn dab_segments(table: &Table, skipped: &mut Vec<String>) -> Vec<Vec<Point>> {
    let dabs = match table_field(table, "Dabs") {
        Some(LuaValue::Table(dabs)) => dabs,
        Some(_) => {
            skipped.push("dabs".to_owned());
            return Vec::new();
        }
        None => return Vec::new(),
    };
    let mut segments: Vec<Vec<Point>> = Vec::new();
    let mut invalid = false;
    for (_, value) in &dabs.fields {
        let dab = match value {
            LuaValue::Text(text) => parse_dab(text),
            _ => None,
        };
        match dab {
            Some(Dab::Move(point)) => segments.push(vec![point]),
            Some(Dab::Line(point)) => match segments.last_mut() {
                Some(segment) => segment.push(point),
                None => segments.push(vec![point]),
            },
            Some(Dab::State) => {}
            None => invalid = true,
        }
    }
    if invalid {
        skipped.push("dabs".to_owned());
    }
    segments
}

/// One `Mask/Paint` component as brush strokes, one per segment of its
/// `Dabs` list, all with the component's size, feather, flow and density.
fn paint_strokes(component: &Table, skipped: &mut Vec<String>) -> Result<Vec<BrushStroke>, String> {
    let segments = dab_segments(component, skipped);
    if segments.is_empty() {
        return Err("empty brush".to_owned());
    }
    let mask_value = match table_field(component, "MaskValue") {
        None => None,
        Some(LuaValue::Number(value)) => Some(*value),
        Some(_) => {
            skipped.push("brush density".to_owned());
            None
        }
    };
    let size =
        number_or_default(component, "Radius", 0.03, "brush radius", skipped).clamp(0.001, 1.0);
    let feather = (number_or_default(component, "CenterWeight", 0.5, "brush feather", skipped)
        * 100.0)
        .clamp(0.0, 100.0);
    let flow = (number_or_default(component, "Flow", 1.0, "brush flow", skipped) * 100.0)
        .clamp(0.0, 100.0);
    let density = (mask_value.unwrap_or(1.0).abs() * 100.0).clamp(0.0, 100.0);
    let erase = mask_value.is_some_and(|value| value <= 0.0);
    Ok(segments
        .into_iter()
        .map(|points| BrushStroke {
            points,
            size,
            feather,
            flow,
            density,
            erase,
        })
        .collect())
}

/// The strokes of a `Mask/Aggregate` component. Every child must be a paint
/// stroke that adds to the group (`MaskBlendMode` 0 or absent, not
/// inverted); erase strokes are paint strokes with `MaskValue` 0. LightCraft
/// combines mask components flat, so another child cannot be expressed and
/// the whole group is reported as `Aggregate (<child>)`.
fn aggregate_strokes(
    component: &Table,
    skipped: &mut Vec<String>,
) -> Result<Vec<BrushStroke>, String> {
    let Some(LuaValue::Table(children)) = table_field(component, "Masks") else {
        return Err("Aggregate (no strokes)".to_owned());
    };
    let mut strokes = Vec::new();
    for (_, child) in &children.fields {
        let LuaValue::Table(child) = child else {
            return Err("Aggregate (unknown child)".to_owned());
        };
        let what = field_text(child, "What").unwrap_or("?");
        if what != "Mask/Paint" {
            return Err(format!("Aggregate ({what})"));
        }
        let adds = matches!(
            table_field(child, "MaskBlendMode"),
            None | Some(LuaValue::Number(0.0))
        );
        let inverted = matches!(
            table_field(child, "MaskInverted"),
            Some(LuaValue::Bool(true))
        );
        if !adds || inverted {
            return Err("Aggregate (combined strokes)".to_owned());
        }
        // An empty child stroke draws nothing; the others still count.
        if let Ok(child_strokes) = paint_strokes(child, skipped) {
            strokes.extend(child_strokes);
        }
    }
    if strokes.is_empty() {
        return Err("empty brush".to_owned());
    }
    Ok(strokes)
}

/// Whether a radial component puts its effect inside the ellipse. `Flipped`
/// means different things per container, as in LightCraft's importer.
fn radial_inside(component: &Table, container: &str, skipped: &mut Vec<String>) -> bool {
    if container == "MaskGroupBasedCorrections" {
        if component_version(component, skipped).is_some_and(|v| v >= 2.0) {
            flag_or_default(component, "Flipped", true, "flipped", skipped)
        } else {
            true
        }
    } else {
        flag_or_default(component, "Flipped", false, "flipped", skipped)
    }
}

/// A radial component's feather on LightCraft's 0..100 scale.
fn radial_feather(component: &Table, container: &str, skipped: &mut Vec<String>) -> f64 {
    let feather = number_or_default(component, "Feather", 50.0, "feather", skipped);
    let versionless_group = container == "MaskGroupBasedCorrections"
        && !component_version(component, skipped).is_some_and(|v| v >= 2.0);
    (if versionless_group {
        feather / 2.0
    } else {
        feather
    })
    .clamp(0.0, 100.0)
}

/// Perceptual lightness 0..1, as range masks store it, mapped to
/// LightCraft's log-luminance 0..1 range scale.
fn lightness_to_range(lightness: f64) -> f64 {
    let y = lightness.clamp(1e-4, 1.0).powf(2.2);
    ((y.log2() + 8.0) / 12.0).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Develop {
        parse_develop(text).expect("valid settings text")
    }

    #[test]
    fn empty_and_blank_text_is_an_empty_develop() {
        for text in ["", "   \n\t"] {
            let develop = parse(text);
            let expected = Develop {
                source_hash: develop.source_hash,
                ..Develop::default()
            };
            assert_eq!(develop, expected);
            assert!(develop.source_hash.is_some());
        }
    }

    #[test]
    fn a_simple_slider_maps() {
        let develop = parse("s = { Exposure2012 = 1.25 }");
        assert_eq!(develop.controls.get("light.exposure"), Some(&1.25));
        assert!(develop.unmapped.is_empty());
        assert!(!develop.deferred);
    }

    #[test]
    fn numbers_support_signs_decimals_and_exponents() {
        let develop = parse("s = { Exposure2012 = +1.5, Contrast2012 = -2, Dehaze = 1e2 }");
        assert_eq!(develop.controls.get("light.exposure"), Some(&1.5));
        assert_eq!(develop.controls.get("light.contrast"), Some(&-2.0));
        assert_eq!(develop.controls.get("effects.dehaze"), Some(&100.0));
    }

    #[test]
    fn strings_keep_their_bytes_and_escapes() {
        let develop = parse(r#"s = { WhiteBalance = "As Shot", ["Mystery Key"] = "a\"b\\c" }"#);
        assert_eq!(develop.white_balance.mode.as_deref(), Some("asShot"));
        assert_eq!(develop.unmapped, vec!["Mystery Key".to_owned()]);
    }

    #[test]
    fn booleans_and_numbers_are_merge_values() {
        let develop = parse(
            "s = { ConvertToGrayscale = true, HDREditMode = 0, \
                   LensProfileEnable = false, AutoLateralCA = false }",
        );
        assert_eq!(
            develop.partial.get("treatment"),
            Some(&MergeValue::Text("bw".to_owned()))
        );
        assert_eq!(
            develop.partial.get("hdr.enabled"),
            Some(&MergeValue::Bool(false))
        );
        assert_eq!(
            develop.partial.get("optics.remove_ca"),
            Some(&MergeValue::Bool(false))
        );
        assert!(!develop.partial.contains_key("optics.lens_profile"));
    }

    #[test]
    fn the_eight_bands_expand() {
        let develop = parse("s = { HueAdjustmentRed = 1, GrayMixerMagenta = 2 }");
        assert_eq!(develop.controls.get("mixer.red.hue"), Some(&1.0));
        assert_eq!(develop.controls.get("bw.magenta"), Some(&2.0));
    }

    #[test]
    fn old_process_formulas_and_suppression() {
        let develop = parse(
            "s = { Exposure = 1, Contrast = 30, HighlightRecovery = 40, \
                   FillLight = 15, Shadows = 10, Brightness = 60, Clarity = 20 }",
        );
        assert_eq!(develop.controls.get("light.contrast"), Some(&5.0));
        assert_eq!(develop.controls.get("light.highlights"), Some(&-40.0));
        assert_eq!(develop.controls.get("light.blacks"), Some(&-20.0));
        assert_eq!(develop.controls.get("light.whites"), Some(&6.0));

        let suppressed = parse("s = { Exposure2012 = 1, Exposure = 2, Contrast = 30 }");
        assert_eq!(suppressed.controls.len(), 1);
        assert_eq!(suppressed.controls.get("light.exposure"), Some(&1.0));
    }

    #[test]
    fn upright_off_reports_nothing_and_a_mode_maps() {
        let off = parse(
            "s = { PerspectiveUpright = 0, UprightVersion = 6, UprightCenterMode = 0, \
                   UprightFocalLength35mm = 35, UprightTransform_0 = \"1 0 0 0 1 0 0 0 1\" }",
        );
        assert!(off.unmapped.is_empty(), "{:?}", off.unmapped);
        assert!(off.partial.is_empty());

        let absent = parse("s = { UprightVersion = 6, UprightPreview = false }");
        assert!(absent.unmapped.is_empty(), "{:?}", absent.unmapped);

        let auto = parse(
            "s = { PerspectiveUpright = 1, UprightVersion = 6, UprightTransformCount = 6, \
                   UprightTransform_0 = \"1 0 0 0 1 0 0 0 1\", UprightFocalLength35mm = 35, \
                   UprightDependentDigest = \"AB\" }",
        );
        assert_eq!(
            auto.partial.get("geometry.upright"),
            Some(&MergeValue::Text("auto".to_owned()))
        );
        assert_eq!(
            auto.unmapped,
            ["UprightTransform_0", "UprightFocalLength35mm"]
        );

        for (stored, mode) in [(2, "level"), (3, "vertical"), (4, "full")] {
            let d = parse(&format!("s = {{ PerspectiveUpright = {stored} }}"));
            assert_eq!(
                d.partial.get("geometry.upright"),
                Some(&MergeValue::Text(mode.to_owned()))
            );
        }
    }

    #[test]
    fn guided_upright_stays_reported() {
        let guided = parse(
            "s = { PerspectiveUpright = 5, UprightFourSegmentsCount = 1, \
                   UprightFourSegments_0 = \"0.1,0.1,0.1,0.9\" }",
        );
        assert!(guided.partial.is_empty());
        assert_eq!(
            guided.unmapped,
            ["PerspectiveUpright", "UprightFourSegments_0"]
        );
    }

    #[test]
    fn white_balance_hints() {
        let absolute = parse("s = { Temperature = 5500, Tint = 10 }");
        assert_eq!(absolute.white_balance.hint, WhiteBalanceHint::Undecidable);
        assert_eq!(absolute.white_balance.temperature, Some(5500.0));
        assert_eq!(absolute.white_balance.tint, Some(10.0));
        assert!(absolute.unmapped.is_empty(), "{:?}", absolute.unmapped);

        let relative = parse(
            "s = { WhiteBalance = \"Custom\", IncrementalTemperature = -20, \
                   IncrementalTint = 5 }",
        );
        assert_eq!(relative.white_balance.hint, WhiteBalanceHint::Relative);
        assert_eq!(relative.white_balance.temperature, Some(-20.0));
        assert_eq!(relative.white_balance.tint, Some(5.0));
        assert!(relative.unmapped.is_empty(), "{:?}", relative.unmapped);

        let none = parse("s = { Exposure2012 = 1 }");
        assert_eq!(none.white_balance.hint, WhiteBalanceHint::None);
    }

    #[test]
    fn a_named_white_balance_mode_carries_no_numbers() {
        for text in [
            "s = { WhiteBalance = \"As Shot\", Temperature = 5200, Tint = 3 }",
            "s = { WhiteBalance = \"As Shot\", IncrementalTemperature = 0, IncrementalTint = 0 }",
            "s = { WhiteBalance = \"Daylight\", Temperature = 5500, Tint = 10 }",
        ] {
            let develop = parse(text);
            assert_eq!(develop.white_balance.hint, WhiteBalanceHint::None, "{text}");
            assert_eq!(develop.white_balance.temperature, None, "{text}");
            assert_eq!(develop.white_balance.tint, None, "{text}");
            assert!(
                develop.unmapped.is_empty(),
                "{text}: {:?}",
                develop.unmapped
            );
        }
    }

    #[test]
    fn a_non_numeric_white_balance_value_is_unmapped() {
        let develop = parse("s = { Temperature = \"warm\", Tint = 4 }");
        assert_eq!(develop.white_balance.hint, WhiteBalanceHint::Undecidable);
        assert_eq!(develop.white_balance.temperature, None);
        assert_eq!(develop.unmapped, ["Temperature"]);
    }

    #[test]
    fn white_balance_mode_normalisation() {
        for (stored, expected) in [
            ("As Shot", "asShot"),
            ("Auto", "auto"),
            ("Custom", "custom"),
            ("Zyx-Odd", "custom"),
        ] {
            let text = format!("s = {{ WhiteBalance = \"{stored}\" }}");
            assert_eq!(
                parse(&text).white_balance.mode.as_deref(),
                Some(expected),
                "{stored}"
            );
        }
    }

    #[test]
    fn the_sentinel_is_removed_and_marks_deferred() {
        let develop = parse("s = { Exposure2012 = -999999, Contrast2012 = 12.5 }");
        assert!(develop.deferred);
        assert_eq!(develop.controls.len(), 1);
        assert_eq!(develop.controls.get("light.contrast"), Some(&12.5));
        assert!(develop.unmapped.is_empty());
    }

    #[test]
    fn the_sentinel_is_removed_inside_nested_tables() {
        let develop = parse("s = { PointColors = { -999999, 1.0 } }");
        assert!(develop.deferred);
        assert!(develop.controls.is_empty());
        assert_eq!(develop.unmapped, ["PointColors"]);
    }

    #[test]
    fn unknown_keys_are_reported_once_in_order() {
        let develop = parse("s = { Mystery = 1, Other = 2, Mystery = 3 }");
        assert_eq!(develop.unmapped, ["Mystery", "Other"]);
        for name in &develop.unmapped {
            assert!(!name.contains('1') && !name.contains('2'));
        }
    }

    #[test]
    fn process_version_is_silent() {
        let develop = parse(r#"s = { ProcessVersion = "15.4", Exposure2012 = 1 }"#);
        assert!(develop.unmapped.is_empty());
    }

    #[test]
    fn broken_text_is_an_error_not_a_panic() {
        for text in [
            "s = { Exposure2012 = 1.0,",
            "s = { Exposure2012 = }",
            "s = { Exposure2012 = os.execute('x') }",
            "s = { [\"open\"] = 1",
            "s = { Exposure2012 = 1 } trailing",
            "s = { Exposure2012 = 1e }",
        ] {
            assert!(matches!(
                parse_develop(text),
                Err(DevelopError::Syntax { .. })
            ));
        }
    }

    #[test]
    fn a_too_large_text_is_rejected_before_parsing() {
        let text = format!(
            "s = {{ ZyxPad = \"{}\" }}",
            "x".repeat(DEVELOP_MAX_TEXT_BYTES)
        );
        assert!(text.len() > DEVELOP_MAX_TEXT_BYTES);
        assert_eq!(parse_develop(&text), Err(DevelopError::TooLarge));
    }

    #[test]
    fn nesting_is_limited() {
        let mut text = String::from("s = { Key = ");
        for _ in 0..DEVELOP_MAX_DEPTH {
            text.push('{');
        }
        for _ in 0..DEVELOP_MAX_DEPTH {
            text.push('}');
        }
        text.push('}');
        assert!(matches!(
            parse_develop(&text),
            Err(DevelopError::Syntax { .. })
        ));
    }

    #[test]
    fn source_hash_follows_the_text() {
        let a = parse("s = { Exposure2012 = 1.0 }");
        let b = parse("s = { Exposure2012 = 1.0 }");
        let c = parse("s = { Exposure2012 = 2.0 }");
        assert_eq!(a.source_hash, b.source_hash);
        assert_ne!(a.source_hash, c.source_hash);
    }

    #[test]
    fn stored_orientation_codes_map_to_the_eight_orientations() {
        for (code, orientation) in [
            ("AB", StoredOrientation::Normal),
            ("BA", StoredOrientation::FlipH),
            ("CD", StoredOrientation::Rotate180),
            ("DC", StoredOrientation::FlipV),
            ("AD", StoredOrientation::Transpose),
            ("BC", StoredOrientation::Rotate90),
            ("CB", StoredOrientation::Transverse),
            ("DA", StoredOrientation::Rotate270),
            ("Zyx", StoredOrientation::Normal),
        ] {
            assert_eq!(StoredOrientation::from_code(code), orientation, "{code}");
        }
    }

    #[test]
    fn a_quarter_turn_swaps_the_corners() {
        let rotate = StoredOrientation::Rotate90;
        // The stored top-left lands on the displayed top-right.
        assert_eq!(rotate.map(0.0, 0.0, 1.5, 1.0), (1.0, 0.0));
        assert!(rotate.swaps_axes());
        assert!(!rotate.mirrored());
        assert!(StoredOrientation::FlipV.mirrored());
        assert!(!StoredOrientation::FlipV.swaps_axes());
    }

    #[test]
    fn an_empty_curve_list_is_no_curve() {
        let develop = parse("s = { ToneCurvePV2012 = {} }");
        assert!(develop.curves.master.is_empty());
        assert!(develop.unmapped.is_empty());
    }

    #[test]
    fn a_broken_curve_is_an_error_not_an_unmapped_name() {
        for text in [
            "s = { ToneCurvePV2012 = { 0, 0, 128 } }",
            "s = { ToneCurvePV2012 = { 0, 0, \"Zyx\", 255 } }",
            "s = { ToneCurvePV2012 = { x = 1, y = 1 } }",
            "s = { ToneCurvePV2012Red = 5 }",
        ] {
            assert!(
                matches!(parse_develop(text), Err(DevelopError::Curve { .. })),
                "{text}"
            );
        }
    }

    #[test]
    fn a_curve_alone_is_develop_content_without_a_control() {
        let develop = parse("s = { ToneCurvePV2012 = { 0, 0, 255, 255 } }");
        assert!(develop.curves.master.is_empty(), "identity, not a curve");
        let curved = parse("s = { ToneCurvePV2012 = { 0, 0, 128, 200, 255, 255 } }");
        assert_eq!(curved.curves.master.len(), 3);
        assert!(curved.controls.is_empty());
        assert!(curved.unmapped.is_empty());
    }

    #[test]
    fn crop_fields_become_a_crop_and_never_an_unmapped_name() {
        let develop = parse(
            "s = { CropLeft = 0.1, CropTop = 0.2, CropRight = 0.9, \
                   CropBottom = 0.8, CropAngle = 0 }",
        );
        let crop = develop.crop.expect("crop");
        assert_eq!((crop.x0, crop.y0, crop.x1, crop.y1), (0.1, 0.2, 0.9, 0.8));
        assert_eq!(crop.angle, 0.0);
        assert!(develop.unmapped.is_empty());
        assert!(!develop.controls.contains_key("crop.angle"));

        let turned = parse(
            "s = { CropLeft = 0.1, CropTop = 0.2, CropRight = 0.9, \
                             CropBottom = 0.8, CropAngle = 2.0 }",
        );
        assert_eq!(
            turned.crop.expect("crop").angle,
            -2.0,
            "upright photos turn the other way"
        );
        assert!(turned.unmapped.is_empty());
    }

    #[test]
    fn a_disabled_crop_is_the_unit_rectangle() {
        let develop = parse("s = { HasCrop = false, CropLeft = 0.4 }");
        let crop = develop.crop.expect("unit crop");
        assert_eq!((crop.x0, crop.y0, crop.x1, crop.y1), (0.0, 0.0, 1.0, 1.0));
        assert_eq!(crop.angle, 0.0);
        assert!(develop.unmapped.is_empty());
    }

    #[test]
    fn an_inverted_crop_rectangle_means_no_crop() {
        // Numeric but inverted edges are the documented reference case for
        // "no crop": nothing was unreadable, so nothing is reported.
        let develop = parse("s = { CropLeft = 0.9, CropRight = 0.1 }");
        assert_eq!(develop.crop, None);
        assert!(develop.unmapped.is_empty());
    }

    #[test]
    fn a_crop_value_with_the_wrong_type_is_reported_and_means_no_crop() {
        let develop = parse(
            "s = { CropLeft = \"Zyx\", CropTop = 0.2, CropRight = 0.9, \
                   CropBottom = 0.8 }",
        );
        assert_eq!(
            develop.crop, None,
            "the remaining fields must not become a rectangle of defaults"
        );
        assert_eq!(develop.unmapped, ["CropLeft"]);
    }

    #[test]
    fn a_crop_flag_with_the_wrong_type_is_reported_and_means_no_crop() {
        let develop = parse(
            "s = { HasCrop = \"Zyx\", CropLeft = 0.1, CropTop = 0.2, \
                   CropRight = 0.9, CropBottom = 0.8 }",
        );
        assert_eq!(develop.crop, None);
        assert_eq!(develop.unmapped, ["HasCrop"]);
    }

    #[test]
    fn each_unreadable_crop_field_is_reported_once() {
        let develop = parse("s = { CropLeft = \"Zyx\", CropAngle = true, CropRight = 0.9 }");
        assert_eq!(develop.crop, None);
        let mut names = develop.unmapped.clone();
        names.sort();
        assert_eq!(names, ["CropAngle", "CropLeft"]);
    }

    #[test]
    fn the_crop_angle_is_clamped_like_lightcraft() {
        let develop = parse("s = { CropAngle = 90 }");
        assert_eq!(develop.crop.expect("crop").angle, -45.0);
    }

    #[test]
    fn the_crop_transform_uses_the_stored_geometry() {
        let geometry = StoredGeometry::new(Some("BC"), Some(1500.0), Some(1000.0));
        let develop = parse_develop_with_geometry(
            "s = { CropLeft = 0.1, CropTop = 0.2, CropRight = 0.6, CropBottom = 0.9 }",
            geometry,
        )
        .expect("valid");
        let crop = develop.crop.expect("crop");
        for (actual, expected) in [
            (crop.x0, 0.1),
            (crop.y0, 0.1),
            (crop.x1, 0.8),
            (crop.y1, 0.6),
            (crop.angle, 0.0),
        ] {
            assert!((actual - expected).abs() < 1e-9, "{actual} vs {expected}");
        }
    }

    #[test]
    fn a_missing_stored_size_falls_back_to_square() {
        let geometry = StoredGeometry::new(None, None, None);
        assert_eq!(geometry.aspect(), 1.0);
        assert_eq!(geometry.orientation, StoredOrientation::Normal);
        let geometry = StoredGeometry::new(Some("AB"), Some(0.0), Some(1000.0));
        assert_eq!(geometry.aspect(), 1.0);
    }

    #[test]
    fn a_camera_profile_keeps_its_bytes() {
        for profile in ["Studio Neutral", "Profil Ärger 📷", "  padded  "] {
            let develop = parse(&format!("s = {{ CameraProfile = \"{profile}\" }}"));
            assert_eq!(
                develop.camera_profile.as_deref(),
                Some(profile),
                "profile must survive byte for byte"
            );
            assert!(develop.unmapped.is_empty(), "{profile}");
        }
    }

    #[test]
    fn a_missing_or_empty_camera_profile_is_none() {
        for text in ["s = {}", "s = { CameraProfile = \"\" }"] {
            let develop = parse(text);
            assert_eq!(develop.camera_profile, None, "{text}");
            assert!(develop.unmapped.is_empty(), "{text}");
        }
    }

    #[test]
    fn a_wrongly_typed_camera_profile_is_unmapped() {
        for text in [
            "s = { CameraProfile = 42 }",
            "s = { CameraProfile = true }",
            "s = { CameraProfile = { X = 1 } }",
        ] {
            let develop = parse(text);
            assert_eq!(develop.camera_profile, None, "{text}");
            assert_eq!(develop.unmapped, ["CameraProfile"], "{text}");
        }
    }

    #[test]
    fn a_look_is_still_unmapped_and_not_a_camera_profile() {
        let develop = parse("s = { CameraProfile = \"A\", Look = \"B\" }");
        assert_eq!(develop.camera_profile.as_deref(), Some("A"));
        assert_eq!(develop.unmapped, ["Look"]);
    }

    #[test]
    fn the_four_mask_containers_map_in_a_fixed_order() {
        let develop = parse(
            r#"s = {
                PaintBasedCorrections = {
                    { CorrectionMasks = { { What = "Mask/Paint", Dabs = { "d 0.1 0.2" } } } },
                },
                GradientBasedCorrections = {
                    { CorrectionName = "Linear", CorrectionMasks = { { What = "Mask/Gradient" } } },
                },
                MaskGroupBasedCorrections = {
                    { CorrectionName = "Luminance", CorrectionMasks = {
                        { What = "Mask/RangeMask",
                          CorrectionRangeMask = { Type = "2", LumRange = "0 0 1 1" } } } },
                    { CorrectionName = "Inactive", CorrectionActive = false,
                      CorrectionMasks = { { What = "Mask/Gradient" } } },
                },
                CircularGradientBasedCorrections = {
                    { CorrectionName = "Radial", CorrectionMasks = {
                        { What = "Mask/CircularGradient" } } },
                },
            }"#,
        );
        let names: Vec<&str> = develop.masks.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Luminance", "Linear", "Radial", "Mask 4"]);
        assert_eq!(
            develop.masks.iter().map(|m| m.id).collect::<Vec<_>>(),
            [1, 2, 3, 4]
        );
        assert!(develop.unmapped.is_empty(), "{:?}", develop.unmapped);
    }

    #[test]
    fn dab_state_entries_are_skipped_silently() {
        let develop = parse(
            r#"s = { MaskGroupBasedCorrections = { { CorrectionMasks = {
                { What = "Mask/Paint", Dabs = { "r 0.05", "d 0.1 0.2", "f 0.8",
                  "h 0.3", "d 0.3 0.4" } } } } } }"#,
        );
        assert!(develop.unmapped.is_empty(), "{:?}", develop.unmapped);
        let MaskShape::Brush { strokes } = &develop.masks[0].components[0].shape else {
            panic!("{:?}", develop.masks[0].components[0].shape);
        };
        assert_eq!(
            strokes[0].points,
            [Point { x: 0.1, y: 0.2 }, Point { x: 0.3, y: 0.4 }]
        );

        let spots = parse(
            r#"s = { RetouchAreas = { { Masks = { { What = "Mask/Paint",
                Dabs = { "r 0.02", "d 0.5 0.5", "d 0.6 0.5" } } },
                SpotType = "heal", SourceState = "sourceAutoComputed" } } }"#,
        );
        assert!(
            !spots.unmapped.iter().any(|n| n == "Spot: dabs"),
            "{:?}",
            spots.unmapped
        );
    }

    #[test]
    fn every_move_dab_starts_its_own_stroke() {
        let develop = parse(
            r#"s = { MaskGroupBasedCorrections = { { CorrectionMasks = {
                { What = "Mask/Paint", Radius = 0.02, Dabs = {
                  "M 0.1 0.1", "M 0.5 0.5", "d 0.6 0.5", "r 0.03", "M 0.9 0.9" } } } } } }"#,
        );
        assert!(develop.unmapped.is_empty(), "{:?}", develop.unmapped);
        let MaskShape::Brush { strokes } = &develop.masks[0].components[0].shape else {
            panic!();
        };
        let lens: Vec<usize> = strokes.iter().map(|s| s.points.len()).collect();
        assert_eq!(lens, [1, 2, 1]);
        assert!(strokes.iter().all(|s| (s.size - 0.02).abs() < 1e-12));
    }

    #[test]
    fn an_unknown_dab_entry_is_reported_and_the_points_kept() {
        let develop = parse(
            r#"s = { MaskGroupBasedCorrections = { { CorrectionMasks = {
                { What = "Mask/Paint", Dabs = { "q 1", "d 0.1 0.2", "d 0.5" } } } } } }"#,
        );
        assert_eq!(develop.unmapped, ["Mask: dabs"]);
        let MaskShape::Brush { strokes } = &develop.masks[0].components[0].shape else {
            panic!();
        };
        assert_eq!(strokes[0].points, [Point { x: 0.1, y: 0.2 }]);
    }

    #[test]
    fn an_aggregate_becomes_one_brush_with_its_strokes_and_erasers() {
        let develop = parse(
            r#"s = { MaskGroupBasedCorrections = { { CorrectionMasks = {
                { What = "Mask/Aggregate", MaskBlendMode = 1, MaskInverted = true,
                  Masks = {
                    { What = "Mask/Paint", MaskBlendMode = 0, MaskValue = 1,
                      Radius = 0.04, CenterWeight = 0.2, Flow = 0.5,
                      Dabs = { "r 0.04", "d 0.1 0.1", "d 0.2 0.2" } },
                    { What = "Mask/Paint", MaskBlendMode = 0, MaskValue = 0,
                      Radius = 0.01, Dabs = { "d 0.15 0.15" } },
                  } } } } } }"#,
        );
        assert!(develop.unmapped.is_empty(), "{:?}", develop.unmapped);
        let component = &develop.masks[0].components[0];
        assert_eq!(component.op, MaskOp::Subtract);
        assert!(component.invert);
        let MaskShape::Brush { strokes } = &component.shape else {
            panic!("{:?}", component.shape);
        };
        assert_eq!(strokes.len(), 2);
        assert!(!strokes[0].erase);
        assert_eq!(strokes[0].points.len(), 2);
        assert!((strokes[0].feather - 20.0).abs() < 1e-9);
        assert!((strokes[0].flow - 50.0).abs() < 1e-9);
        assert!(strokes[1].erase, "MaskValue 0 is an eraser");
    }

    #[test]
    fn an_aggregate_with_another_child_kind_is_reported_whole() {
        let develop = parse(
            r#"s = { MaskGroupBasedCorrections = { { CorrectionMasks = {
                { What = "Mask/Aggregate", Masks = {
                    { What = "Mask/Paint", Dabs = { "d 0.1 0.1" } },
                    { What = "Mask/Gradient" },
                  } } } } } }"#,
        );
        assert!(develop.masks.is_empty());
        assert_eq!(develop.unmapped, ["Mask: Aggregate (Mask/Gradient)"]);

        let develop = parse(
            r#"s = { MaskGroupBasedCorrections = { { CorrectionMasks = {
                { What = "Mask/Aggregate", Masks = {
                    { What = "Mask/Paint", MaskBlendMode = 2, Dabs = { "d 0.1 0.1" } },
                  } } } } } }"#,
        );
        assert_eq!(develop.unmapped, ["Mask: Aggregate (combined strokes)"]);
    }

    #[test]
    fn catalog_codes_stored_as_numbers_map_like_text() {
        let develop = parse(
            r#"s = { MaskGroupBasedCorrections = {
                { CorrectionMasks = { { What = "Mask/Image", MaskSubType = 1 } } },
                { CorrectionMasks = { { What = "Mask/Image", MaskSubType = 2 } } },
                { CorrectionMasks = {
                    { What = "Mask/Image", MaskSubType = 0, MaskSubCategoryID = 22 } } },
                { CorrectionMasks = { { What = "Mask/RangeMask",
                    CorrectionRangeMask = { Type = 3, DepthRange = "0.1 0.2 0.8 0.9" } } } },
                { CorrectionMasks = { { What = "Mask/Image", MaskSubType = 3 } } },
            } }"#,
        );
        let shapes: Vec<&MaskShape> = develop
            .masks
            .iter()
            .map(|m| &m.components[0].shape)
            .collect();
        assert_eq!(shapes.len(), 4, "{:?}", develop.unmapped);
        assert_eq!(*shapes[0], MaskShape::Subject);
        assert_eq!(*shapes[1], MaskShape::Sky);
        assert_eq!(*shapes[2], MaskShape::Background);
        assert!(matches!(shapes[3], MaskShape::DepthRange { .. }));
        assert_eq!(develop.unmapped, ["Mask: AI mask (3)"]);
    }

    #[test]
    fn the_ai_background_maps_and_an_object_stays_reported() {
        let develop = parse(
            r#"s = { MaskGroupBasedCorrections = {
                { CorrectionMasks = {
                    { What = "Mask/Image", MaskSubType = "0", MaskSubCategoryID = "22" } } },
                { CorrectionMasks = {
                    { What = "Mask/Image", MaskSubType = "0" } } },
            } }"#,
        );
        assert_eq!(develop.masks.len(), 1);
        assert_eq!(develop.masks[0].components[0].shape, MaskShape::Background);
        assert_eq!(develop.unmapped, ["Mask: AI mask (0)"]);
    }

    #[test]
    fn every_supported_mask_shape_maps_with_the_stored_geometry() {
        let geometry = StoredGeometry::new(Some("AB"), Some(2000.0), Some(1000.0));
        let develop = parse_develop_with_geometry(
            r#"s = { MaskGroupBasedCorrections = { { CorrectionMasks = {
                { What = "Mask/CircularGradient", Left = 0.2, Right = 0.8,
                  Top = 0.1, Bottom = 0.6, Angle = 15, Feather = 40,
                  Flipped = true, Version = 2 },
                { What = "Mask/Image", MaskSubType = "1" },
                { What = "Mask/Image", MaskSubType = "2" },
                { What = "Mask/RangeMask",
                  CorrectionRangeMask = { Type = "3", DepthRange = "0.1 0.2 0.8 0.9" } },
                { What = "Mask/RangeMask",
                  CorrectionRangeMask = { Type = "2", LumRange = "0.0 0.2 0.8 1.0" } },
            } } } }"#,
            geometry,
        )
        .expect("valid");
        let components = &develop.masks[0].components;
        assert_eq!(components.len(), 5);
        let MaskShape::Radial {
            center,
            rx,
            ry,
            angle,
            feather,
            invert,
        } = components[0].shape
        else {
            panic!("{:?}", components[0].shape);
        };
        assert!((center.x - 0.5).abs() < 1e-9 && (center.y - 0.35).abs() < 1e-9);
        assert!((rx - 0.3).abs() < 1e-9, "{rx}");
        assert!((ry - 0.125).abs() < 1e-9, "{ry}");
        assert_eq!((angle, feather, invert), (15.0, 40.0, false));
        assert_eq!(components[1].shape, MaskShape::Subject);
        assert_eq!(components[2].shape, MaskShape::Sky);
        assert_eq!(
            components[3].shape,
            MaskShape::DepthRange {
                lo: 0.2,
                hi: 0.8,
                feather: 0.1
            }
        );
        let MaskShape::LuminanceRange {
            lo,
            hi,
            lo_feather,
            hi_feather,
        } = components[4].shape
        else {
            panic!("{:?}", components[4].shape);
        };
        assert!((lo - 0.24097984927065022).abs() < 1e-12, "{lo}");
        assert!((hi - 0.6076465159373169).abs() < 1e-12, "{hi}");
        assert!((lo_feather - 0.24097984927065022).abs() < 1e-12);
        assert!((hi_feather - 0.0590201507293497).abs() < 1e-12);
    }

    #[test]
    fn blend_modes_and_component_inversion_map() {
        let develop = parse(
            r#"s = { MaskGroupBasedCorrections = { { CorrectionMasks = {
                { What = "Mask/Gradient", MaskBlendMode = 0, MaskInverted = true },
                { What = "Mask/Gradient", MaskBlendMode = 1 },
                { What = "Mask/Gradient", MaskBlendMode = 2 },
                { What = "Mask/Gradient", MaskBlendMode = 7 },
            } } } }"#,
        );
        let components = &develop.masks[0].components;
        assert_eq!(components[0].op, MaskOp::Add);
        assert!(components[0].invert);
        assert_eq!(components[1].op, MaskOp::Subtract);
        assert_eq!(components[2].op, MaskOp::Intersect);
        assert_eq!(components.len(), 3, "an unknown mode builds no component");
        assert_eq!(develop.unmapped, ["Mask: blend mode 7"]);
    }

    #[test]
    fn wrongly_typed_mask_fields_are_reported_and_fall_back() {
        let develop = parse(
            r#"s = { MaskGroupBasedCorrections = { {
                CorrectionActive = "yes", MaskInverted = "yes", CorrectionAmount = "full",
                CorrectionMasks = {
                    { What = "Mask/Gradient", MaskBlendMode = "add", FullX = "half" },
                } } } }"#,
        );
        let mask = &develop.masks[0];
        assert!(!mask.invert);
        assert_eq!(mask.adjust.get("amount"), Some(&100.0));
        let component = &mask.components[0];
        assert_eq!(component.op, MaskOp::Add);
        let MaskShape::Linear { start, end } = component.shape else {
            panic!("{:?}", component.shape);
        };
        assert_eq!(start, Point { x: 0.5, y: 0.0 });
        assert_eq!(end, Point { x: 0.5, y: 1.0 });
        assert_eq!(
            develop.unmapped,
            [
                "Mask: active flag",
                "Mask: blend mode",
                "Mask: correction amount",
                "Mask: gradient position",
                "Mask: inverted",
            ]
        );
    }

    #[test]
    fn wrongly_typed_radial_fields_are_reported_and_fall_back() {
        let develop = parse(
            r#"s = { CircularGradientBasedCorrections = { { CorrectionMasks = {
                { What = "Mask/CircularGradient", Left = "left",
                  Feather = "soft", Flipped = "yes" } } } } }"#,
        );
        let MaskShape::Radial {
            center,
            feather,
            invert,
            ..
        } = develop.masks[0].components[0].shape
        else {
            panic!("{:?}", develop.masks[0].components[0].shape);
        };
        assert_eq!(center, Point { x: 0.5, y: 0.5 });
        assert_eq!(feather, 50.0);
        assert!(invert);
        assert_eq!(
            develop.unmapped,
            ["Mask: feather", "Mask: flipped", "Mask: radial bounds"]
        );
    }

    #[test]
    fn wrongly_typed_brush_fields_are_reported_and_fall_back() {
        let develop = parse(
            r#"s = { PaintBasedCorrections = { { CorrectionMasks = {
                { What = "Mask/Paint", Dabs = { "d 0.1 0.2", 5 },
                  Radius = "wide", CenterWeight = "soft", Flow = "full",
                  MaskValue = "erase" } } } } }"#,
        );
        let MaskShape::Brush { strokes } = &develop.masks[0].components[0].shape else {
            panic!("{:?}", develop.masks[0].components[0].shape);
        };
        let stroke = &strokes[0];
        assert_eq!(stroke.points.len(), 1);
        assert_eq!(stroke.size, 0.03);
        assert_eq!(stroke.feather, 50.0);
        assert_eq!(stroke.flow, 100.0);
        assert_eq!(stroke.density, 100.0);
        assert!(!stroke.erase);
        assert_eq!(
            develop.unmapped,
            [
                "Mask: brush density",
                "Mask: brush feather",
                "Mask: brush flow",
                "Mask: brush radius",
                "Mask: dabs",
            ]
        );
    }

    #[test]
    fn missing_mask_fields_keep_the_defaults_without_a_report() {
        let develop = parse(
            r#"s = { MaskGroupBasedCorrections = { { CorrectionMasks = {
                { What = "Mask/Gradient" } } } } }"#,
        );
        assert!(develop.unmapped.is_empty());
        let MaskShape::Linear { start, end } = develop.masks[0].components[0].shape else {
            panic!("{:?}", develop.masks[0].components[0].shape);
        };
        assert_eq!(start, Point { x: 0.5, y: 0.0 });
        assert_eq!(end, Point { x: 0.5, y: 1.0 });
    }

    #[test]
    fn wrongly_typed_spot_fields_are_reported_and_fall_back() {
        let develop = parse(
            r#"s = { RetouchAreas = { {
                SpotType = 1, Feather = "soft", Opacity = "half",
                SourceState = 2, SourceX = "left", OffsetY = "up",
                Masks = { { What = "Mask/Ellipse", X = 0.4, Y = 0.3, SizeX = 0.08 } } } } }"#,
        );
        let spot = &develop.spots[0];
        assert_eq!(spot.mode, SpotMode::Heal);
        assert_eq!((spot.feather, spot.opacity), (0.0, 100.0));
        assert_eq!(spot.source_offset, None);
        assert_eq!(
            develop.unmapped,
            [
                "Spot: feather",
                "Spot: opacity",
                "Spot: source state",
                "Spot: source x",
                "Spot: source y",
                "Spot: spot type",
            ]
        );
    }

    #[test]
    fn wrong_typed_brush_spot_fields_are_reported_and_fall_back() {
        let develop = parse(
            r#"s = { RetouchAreas = { { Masks = { {
                What = "Mask/Paint", Dabs = { "d 0.1 0.2", 7 },
                Radius = "wide" } } } } }"#,
        );
        let spot = &develop.spots[0];
        assert_eq!(spot.points.len(), 1);
        assert_eq!(spot.size, 0.02);
        assert_eq!(develop.unmapped, ["Spot: dabs", "Spot: radius"]);
    }

    #[test]
    fn missing_spot_fields_keep_the_defaults_without_a_report() {
        let develop = parse(
            r#"s = { RetouchAreas = { { Masks = { { What = "Mask/Ellipse",
                X = 0.4, Y = 0.3, SizeX = 0.08 } } } } }"#,
        );
        assert!(develop.unmapped.is_empty());
        let spot = &develop.spots[0];
        assert_eq!(spot.mode, SpotMode::Heal);
        assert_eq!((spot.feather, spot.opacity), (0.0, 100.0));
        assert_eq!(spot.source_offset, None);
    }

    #[test]
    fn a_wrongly_typed_legacy_opacity_is_reported() {
        let develop = parse(
            r#"s = { RetouchInfo = {
                "centerX = 0.5, centerY = 0.5, radius = 0.04, opacity = lots" } }"#,
        );
        assert_eq!(develop.spots[0].opacity, 100.0);
        assert_eq!(develop.unmapped, ["Spot: opacity"]);
    }

    #[test]
    fn radial_masks_use_the_displayed_aspect() {
        let text = r#"s = { MaskGroupBasedCorrections = { { CorrectionMasks = {
            { What = "Mask/CircularGradient", Left = 0.2, Right = 0.6,
              Top = 0.3, Bottom = 0.5 } } } } }"#;
        let upright = parse_develop_with_geometry(
            text,
            StoredGeometry::new(Some("AB"), Some(2000.0), Some(1000.0)),
        )
        .unwrap();
        let MaskShape::Radial { rx, ry, .. } = upright.masks[0].components[0].shape else {
            panic!("{:?}", upright.masks[0].components[0].shape);
        };
        assert!((rx - 0.2).abs() < 1e-12, "{rx}");
        assert!((ry - 0.05).abs() < 1e-12, "{ry}");
        for orientation in ["BC", "DA"] {
            let turned = parse_develop_with_geometry(
                text,
                StoredGeometry::new(Some(orientation), Some(2000.0), Some(1000.0)),
            )
            .unwrap();
            let MaskShape::Radial { rx, ry, .. } = turned.masks[0].components[0].shape else {
                panic!("{:?}", turned.masks[0].components[0].shape);
            };
            assert!((rx - 0.1).abs() < 1e-12, "{orientation}: {rx}");
            assert!((ry - 0.1).abs() < 1e-12, "{orientation}: {ry}");
        }
    }

    #[test]
    fn an_unsupported_correction_still_reports_its_unsupported_locals() {
        let develop = parse(
            r#"s = { GradientBasedCorrections = {
                { LocalGrain = 1.0, LocalExposure2012 = 0.5,
                  CorrectionMasks = { { What = "Mask/UnknownShape" } } } } }"#,
        );
        assert!(develop.masks.is_empty());
        assert_eq!(develop.unmapped, ["Mask: LocalGrain", "Mask: UnknownShape"]);
    }

    #[test]
    fn a_mask_container_that_is_not_a_list_is_reported() {
        let develop = parse(r#"s = { GradientBasedCorrections = "ZyxBroken" }"#);
        assert!(develop.masks.is_empty());
        assert_eq!(
            develop.unmapped,
            ["Mask: container GradientBasedCorrections"]
        );
    }

    #[test]
    fn retouch_areas_become_spots_with_source_offsets() {
        let geometry = StoredGeometry::new(Some("AB"), Some(1000.0), Some(2000.0));
        let develop = parse_develop_with_geometry(
            r#"s = { RetouchAreas = {
                { Masks = { { What = "Mask/Ellipse", X = 0.4, Y = 0.3, SizeX = 0.08 } },
                  Feather = 0.5, Opacity = 0.8, SourceState = "sourceSetExplicitly",
                  SourceX = 0.6, OffsetY = 0.5 },
                { SpotType = "clone",
                  Masks = { { What = "Mask/Paint", Dabs = { "d 0.1 0.2", "d 0.3 0.4" },
                              Radius = 0.05 } },
                  SourceState = "sourceAuto", SourceX = 0.4, OffsetY = 0.5 },
                { Masks = { { What = "Mask/CircularGradient", Top = 0.2, Left = 0.3,
                              Bottom = 0.6, Right = 0.7 } } },
                { Masks = { { What = "Mask/Gradient" } } },
                { Feather = 0.1 },
            } }"#,
            geometry,
        )
        .unwrap();
        assert_eq!(develop.spots.len(), 3);
        let heal = &develop.spots[0];
        assert_eq!(heal.mode, SpotMode::Heal);
        assert_eq!(heal.points, [Point { x: 0.4, y: 0.3 }]);
        assert!((heal.size - 0.04).abs() < 1e-12, "{}", heal.size);
        assert_eq!((heal.feather, heal.opacity), (50.0, 80.0));
        let offset = heal.source_offset.expect("explicit source");
        assert!((offset.x - 0.2).abs() < 1e-12, "{}", offset.x);
        assert!((offset.y - 0.2).abs() < 1e-12, "{}", offset.y);
        let clone = &develop.spots[1];
        assert_eq!(clone.mode, SpotMode::Clone);
        assert_eq!(clone.points.len(), 2);
        assert_eq!(clone.size, 0.05, "a brush radius is not aspect-scaled");
        assert_eq!(clone.source_offset, None, "an automatic source has none");
        assert_eq!(clone.opacity, 100.0);
        let circle = &develop.spots[2];
        assert!((circle.points[0].x - 0.5).abs() < 1e-12);
        assert!((circle.points[0].y - 0.4).abs() < 1e-12);
        assert!((circle.size - 0.1).abs() < 1e-12, "{}", circle.size);
        assert_eq!(develop.unmapped, ["Spot: Gradient", "Spot: no mask"]);
    }

    #[test]
    fn retouch_info_is_only_a_fallback_with_legacy_units() {
        let geometry = StoredGeometry::new(Some("AB"), Some(2000.0), Some(1000.0));
        let develop = parse_develop_with_geometry(
            r#"s = { RetouchInfo = {
                "centerX = 0.25, centerY = 0.75, radius = 0.04, spotType = heal, sourceState = sourceSetExplicitly, sourceX = 0.5, sourceY = 0.6, opacity = 0.5",
                "centerX = 0.6, centerY = 0.2, radius = 0.1, spotType = clone, sourceState = sourceAuto, sourceX = 0.9, sourceY = 0.9",
                "centerX = 0.5, radius = 0.02",
            } }"#,
            geometry,
        )
        .unwrap();
        assert_eq!(develop.spots.len(), 2);
        let heal = &develop.spots[0];
        assert_eq!(heal.mode, SpotMode::Heal);
        assert!((heal.size - 0.04).abs() < 1e-12);
        assert_eq!(heal.opacity, 50.0);
        assert_eq!(heal.feather, 0.0);
        let offset = heal.source_offset.expect("explicit source");
        assert!((offset.x - 0.25).abs() < 1e-12);
        assert!((offset.y + 0.15).abs() < 1e-12);
        let clone = &develop.spots[1];
        assert_eq!(clone.mode, SpotMode::Clone);
        assert!((clone.size - 0.1).abs() < 1e-12);
        assert_eq!(clone.source_offset, None);
        assert_eq!(develop.unmapped, ["Spot: legacy line"]);

        let both = parse_develop_with_geometry(
            r#"s = { RetouchAreas = { { Masks = { { What = "Mask/Ellipse",
                    X = 0.1, Y = 0.1, SizeX = 0.02 } } } },
                RetouchInfo = { "centerX = 0.9, centerY = 0.9, radius = 0.01" } }"#,
            geometry,
        )
        .unwrap();
        assert_eq!(both.spots.len(), 1, "RetouchAreas wins over RetouchInfo");
        assert!(both.unmapped.is_empty());
    }

    #[test]
    fn spot_size_feather_opacity_and_sources_are_clamped() {
        let geometry = StoredGeometry::new(Some("AB"), Some(2000.0), Some(1000.0));
        let develop = parse_develop_with_geometry(
            r#"s = { RetouchAreas = {
                { Masks = { { What = "Mask/Ellipse", X = 0.2, Y = 0.2, SizeX = 0.9 } },
                  SourceX = 0.2, OffsetY = 0.2 },
                { Masks = { { What = "Mask/Ellipse", X = 0.8, Y = 0.8, SizeX = 0.0005 } },
                  Feather = 1.5, Opacity = -0.5 },
            } }"#,
            geometry,
        )
        .unwrap();
        assert_eq!(develop.spots[0].size, 0.25);
        assert_eq!(
            develop.spots[0].source_offset, None,
            "same position is no offset"
        );
        assert_eq!(develop.spots[1].size, 0.001);
        assert_eq!(develop.spots[1].feather, 100.0);
        assert_eq!(develop.spots[1].opacity, 0.0);
        assert!(develop.unmapped.is_empty());
    }

    #[test]
    fn a_spot_container_that_is_not_a_list_is_reported() {
        let develop = parse(r#"s = { RetouchAreas = "ZyxBroken" }"#);
        assert!(develop.spots.is_empty());
        assert_eq!(develop.unmapped, ["Spot: container RetouchAreas"]);
    }

    #[test]
    fn unsupported_masks_and_local_sliders_are_named_never_dropped() {
        let develop = parse(
            r#"s = { MaskGroupBasedCorrections = {
                { CorrectionMasks = { { What = "Mask/UnknownShape" } } },
                { CorrectionName = "No components", CorrectionMasks = {} },
                { CorrectionName = "Empty brush",
                  CorrectionMasks = { { What = "Mask/Paint", Dabs = {} } } },
                { CorrectionName = "Grain", LocalGrain = 3737,
                  CorrectionMasks = { { What = "Mask/Image", MaskSubType = "1" } } },
                { CorrectionName = "AI", CorrectionMasks = {
                    { What = "Mask/Image", MaskSubType = "9" } } },
                { CorrectionName = "Colour", CorrectionMasks = {
                    { What = "Mask/RangeMask",
                      CorrectionRangeMask = { Type = "1", SampleRange = "0.1 0.2" } } } },
            } }"#,
        );
        assert_eq!(develop.masks.len(), 1);
        assert_eq!(develop.masks[0].name, "Grain");
        assert_eq!(
            develop.unmapped,
            [
                "Mask: AI mask (9)",
                "Mask: LocalGrain",
                "Mask: UnknownShape",
                "Mask: colour range",
                "Mask: empty brush",
                "Mask: no components",
            ]
        );
    }

    #[test]
    fn amount_is_scaled_clamped_and_local_sliders_use_lightcraft_units() {
        let develop = parse(
            r#"s = { GradientBasedCorrections = {
                { CorrectionAmount = 3.0, LocalExposure2012 = 0.5,
                  LocalContrast2012 = 0.25, LocalHue = 2.0,
                  CorrectionMasks = { { What = "Mask/Gradient" } } },
                { CorrectionAmount = -0.5,
                  CorrectionMasks = { { What = "Mask/Gradient" } } },
            } }"#,
        );
        let high = &develop.masks[0].adjust;
        assert_eq!(high.get("amount"), Some(&200.0));
        assert_eq!(high.get("exposure"), Some(&2.0));
        assert_eq!(high.get("contrast"), Some(&25.0));
        assert_eq!(high.get("hue"), Some(&100.0), "clamped to the slider range");
        assert_eq!(develop.masks[1].adjust.get("amount"), Some(&0.0));
        assert_eq!(
            develop.masks[0].id, 1,
            "the first mask of a photo starts at 1"
        );
    }

    #[test]
    fn a_correction_name_survives_byte_for_byte() {
        let name = "Profil Ärger 📷";
        let develop = parse(&format!(
            r#"s = {{ PaintBasedCorrections = {{ {{ CorrectionName = "{name}",
                CorrectionMasks = {{ {{ What = "Mask/Paint",
                    Dabs = {{ "d 0.5 0.5" }} }} }} }} }} }}"#
        ));
        assert_eq!(develop.masks[0].name, name);
    }

    #[test]
    fn a_correction_level_inversion_is_the_mask_inversion() {
        let develop = parse(
            r#"s = { GradientBasedCorrections = { { MaskInverted = true,
                CorrectionMasks = { { What = "Mask/Gradient" } } } } }"#,
        );
        assert!(develop.masks[0].invert);
        assert!(!develop.masks[0].components[0].invert);
    }
}
