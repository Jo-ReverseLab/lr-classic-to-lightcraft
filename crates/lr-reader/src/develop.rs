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

use model::{Develop, MergeValue, WhiteBalanceHint};
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
}

/// Parses one settings text into the neutral model.
///
/// Empty or whitespace-only text is an empty [`Develop`]. The returned value
/// always carries the SHA-256 of `text` as its source hash.
pub fn parse_develop(text: &str) -> Result<Develop, DevelopError> {
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
    map_table(&table, &mut develop);
    Ok(develop)
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

fn map_table(table: &Table, develop: &mut Develop) {
    // LightCraft ignores the whole old-process block as soon as the settings
    // hold any `*2012` key; presets of newer versions carry the old fields at
    // their defaults.
    let newer_process = table
        .fields
        .iter()
        .any(|(name, _)| name.as_deref().is_some_and(|n| n.ends_with("2012")));
    let mut has_absolute_temperature = false;
    let mut has_incremental_temperature = false;

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
            let mut expected = Develop::default();
            expected.source_hash = develop.source_hash;
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
}
