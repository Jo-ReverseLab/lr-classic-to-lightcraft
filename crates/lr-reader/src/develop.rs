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

use model::{CropGeometry, CurvePoint, Curves, Develop, MergeValue, WhiteBalanceHint};
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

/// Parses one settings text into the neutral model.
///
/// Empty or whitespace-only text is an empty [`Develop`]. The returned value
/// always carries the SHA-256 of `text` as its source hash.
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
    let mut has_absolute_temperature = false;
    let mut has_incremental_temperature = false;
    let mut master_2012 = None;
    let mut red_2012 = None;
    let mut green_2012 = None;
    let mut blue_2012 = None;
    let mut old_master = None;
    let mut has_2012_curve = false;
    let mut crop = RawCrop::default();

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
            "Temperature" | "Tint" => {
                has_absolute_temperature = true;
                // Without probing the original file the scale is unknown, so
                // the value cannot be carried.
                push_unmapped(&mut develop.unmapped, name);
            }
            "IncrementalTemperature" | "IncrementalTint" => {
                has_incremental_temperature = true;
                push_unmapped(&mut develop.unmapped, name);
            }
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

    develop.white_balance.hint = if has_incremental_temperature {
        WhiteBalanceHint::Relative
    } else if has_absolute_temperature {
        WhiteBalanceHint::Undecidable
    } else {
        WhiteBalanceHint::None
    };
    develop.curves = curves_from_parts(
        master_2012,
        red_2012,
        green_2012,
        blue_2012,
        has_2012_curve,
        old_master,
    );
    develop.crop = crop_geometry(&crop, geometry);
    Ok(())
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
    fn white_balance_hints() {
        let absolute = parse("s = { Temperature = 5500, Tint = 10 }");
        assert_eq!(absolute.white_balance.hint, WhiteBalanceHint::Undecidable);
        assert_eq!(absolute.unmapped, ["Temperature", "Tint"]);

        let relative = parse("s = { IncrementalTemperature = -20, IncrementalTint = 5 }");
        assert_eq!(relative.white_balance.hint, WhiteBalanceHint::Relative);
        assert_eq!(
            relative.unmapped,
            ["IncrementalTemperature", "IncrementalTint"]
        );

        let none = parse("s = { Exposure2012 = 1 }");
        assert_eq!(none.white_balance.hint, WhiteBalanceHint::None);
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
}
