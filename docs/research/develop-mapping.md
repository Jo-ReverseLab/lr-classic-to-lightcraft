# Development settings: mapping from Lightroom Classic to LightCraft

> **Status:** Research for the develop-settings phase. Nothing here is implemented.
> Development settings are not converted by lr2lc yet.

This document answers four questions:

1. which development settings exist on both sides and how they correspond;
2. which settings have no counterpart and how they would appear in a report;
3. how the settings are stored in a Lightroom Classic catalog, as far as public
   sources show;
4. what the model extension and the test plan could look like.

## How to read this document

Every statement is either **sourced** (the source is named inline, with
`file:line` for code and a URL or repository path for documents) or marked
**(assumption)**. An assumption is not a fact and must be checked before it is
built on. A statement that carries neither a citation nor a URL from §9 is an
assumption; where such a statement is not marked explicitly, treat it as one.

Source roots used below:

- **LightCraft 0.14.0 source tree** — commit
  `bb08a697a41b2561a54f1b740245119503b24758`, public at
  https://github.com/storytold/lightcraft (§9 gives the URL for every
  citation and the retrieval date). Line numbers refer to that tree. Path
  shorthand used in citations:
  - `crs.rs` = `crates/engine/src/crs.rs`
  - `crs_masks.rs` = `crates/engine/src/crs_masks.rs`
  - `controls.rs` = `crates/develop/src/controls.rs`
  - `settings.rs` = `crates/develop/src/settings.rs`
  - `lightroom_catalog.rs` = `crates/engine/src/lightroom_catalog.rs`
  - `preset_import.rs` = `crates/engine/src/preset_import.rs`
  - `develop.rs` = `crates/engine/src/cmd/develop.rs`
  - docs are under `docs/`.
- **lr2lc repository**, branch `main`, commit `e611212` — line numbers for the
  files that exist there today. The repository is not published yet, so these
  citations have no public URL; later commits moved some of these lines, so
  re-check a line number against the current tree before relying on it.
- **lrcat-extractor 0.6.0** — commit
  `a3a59b37a1081a03f69b06648210c65a53aeb482`, public at
  https://github.com/hfiguiere/lrcat-extractor (§9). Only read, no code copied
  (MPL-2.0). Its format notes are `doc/lrcat_format.md`.

No real catalog was opened for this document. No values, names, paths or
keywords from any catalog appear here. The only examples are synthetic field
names and invented numbers.

---

## 1. What LightCraft offers today

### 1.1 Data model

`DevelopSettings` (`settings.rs:104-144`) is the stored per-photo structure.
Its fields are grouped by panel: `process`, `profile`, `treatment`, `wb`,
`light`, `hdr`, `curve`, `color`, `mixer`, `point_colors`, `bw_mix`, `grading`,
`effects`, `vignette`, `grain`, `detail`, `optics`, `geometry`, `crop`,
`orientation`, `masks`, `spots`, `red_eye`, `lens_blur`, `enhance`,
`calibration`, `disabled_sections`.

- `masks` and `spots` are part of the model (`settings.rs:135-136`), as are
  `red_eye` (`:137`), `lens_blur` (`:138`) and `enhance` (`:139`).
- `hdr` is skipped when default, so settings without HDR keep their JSON
  (`settings.rs:116-119`).
- Point Color is a vector of up to `MAX_POINT_COLORS` samples
  (`settings.rs:123-124`).
- Settings are stored per photo as `Photo.develop` in the library
  (`crates/catalog/src/model.rs:248-254`, cited in
  `docs/research/lightcraft-develop.md:20`).

### 1.2 Slider-addressable controls

- `CONTROLS` (`controls.rs:144`) declares 122 numeric controls
  (`controls.rs:171-292`). Each has an id, label, panel, min, max, default,
  step and decimals (`controls.rs:79-91`).
- Indexed controls (`pointColor.<i>.*`, `redEye.*`) are declared separately
  (`controls.rs:296-407`) and addressed as `<family>.<i>.<field>`
  (`controls.rs:410-461`).
- Writes are clamped to the declared range (`controls.rs:157-163`).
- The full list, with ranges, is the source for the mapping table in section 4.

Controls exist in the model that the current Lightroom import never sets:

| Control | Panel | Why the importer leaves it |
| --- | --- | --- |
| `profile.amount` | Profile | `CameraProfile` is not applied; only a preset's `Look/Amount` selects an imported profile |
| `hdr.maxEv`, `hdr.sdrBrightness`, `hdr.sdrContrast`, `hdr.sdrHighlights`, `hdr.sdrShadows`, `hdr.sdrWhites`, `hdr.sdrClarity` | Light | only `HDREditMode` (on/off) is read (`crs.rs:217-220`) |
| `pointColor.<i>.*` | Point Color | non-empty `PointColors` is reported, not mapped (`crs.rs:498-508`; test `crs.rs:876-884`) |
| `redEye.pupilSize`, `redEye.darken` | Red Eye | red eye is listed as not mapped (`docs/xmp-interop.md:191-192`) |
| `enhance.denoise` | Detail | Adobe AI denoise is not read |
| `optics.profileDistortion`, `optics.profileVignetting` | Optics | lens profiles are the file's own; the strength fields are ignored at their default and reported when changed (`crs.rs:500-507`, test `crs.rs:896-898`) |
| `lens_blur` (model field, not a slider) | — | lens blur is listed as not mapped (`docs/xmp-interop.md:191-192`) |

### 1.3 Commands

| Command | What it does | Source |
| --- | --- | --- |
| `develop.controls {section?}` | lists every slider with range, default and current value | `crates/engine/src/cmd/query.rs:190` |
| `develop.get {id?}` | returns the full settings JSON for one photo | `crates/engine/src/cmd/query.rs:177` |
| `develop.set {control, value \| values, ids?}` | sets numeric controls; unknown id is an error; WB is normalised against the photo's source info | `develop.rs:167-233` |
| `develop.adjust {control, delta}` | relative change | `develop.rs:234-255` |
| `develop.merge {settings, label?}` | merges a **partial `DevelopSettings` JSON** into the active photo; accepts any field of the model, including `curve`, `crop`, `masks`, `spots` | `develop.rs:256-263` |
| `develop.reset`, `develop.resetSection`, `develop.resetControl` | reset all / one panel / one control | `develop.rs:264`, `:332`, `:346` |
| `develop.updateProcess`, `develop.sectionEnabled`, `develop.auto`, `develop.wb`, `develop.curve`, `develop.treatment`, `develop.profile` | panel-specific operations | `develop.rs:303`, `:357`, `:387`, `:407`, and the lines cited in `docs/research/lightcraft-develop.md:27` |
| `preset.apply/create/import/export` | preset handling | `develop.rs:1010-1070`, cited in `docs/research/lightcraft-develop.md:27` |

Two facts matter for a writer:

- `develop.set` accepts `ids` and can update several selected photos in one
  undo step (`develop.rs:211-224`).
- `develop.merge` has **no** `ids` parameter (`develop.rs:256-263`); it edits
  the active photo. A per-photo `library.select` is therefore needed before
  every merge. The lr2lc plan script already supports exactly this pattern:
  `Step::Update` emits `library.select` then the command
  (`crates/lc-writer/src/plan.rs:401-416`).

### 1.4 LightCraft's built-in catalog import

`library.importLightroom {path, updateExisting?}` imports a whole catalog,
including develop settings (`docs/lightroom-catalog-import.md:59-74`). The
pertinent behaviour:

- Develop settings are read from the catalog, parsed as a Lua table literal,
  and passed through the **same XMP/preset mapper** that handles `crs:` fields
  (`lightroom_catalog.rs:690-712`; parser `preset_import.rs:351-371`; key
  renaming `preset_import.rs:392-435`).
- Lightroom's `-999999` "deferred adjustment" sentinel is removed recursively
  and counted, then reported (`lightroom_catalog.rs:767-780`, `:708-710`).
- A missing `HasCrop` is inferred from the presence of crop fields
  (`lightroom_catalog.rs:700-705`).
- Crop is re-projected from the stored (unoriented) image into LightCraft's
  oriented, rotated frame (`lightroom_catalog.rs:737-765`).
- Existing edited photos keep their edits unless `updateExisting=true`
  (`lightroom_catalog.rs:944`, `:993-995`).
- The import result has an `unmapped` object keyed by source image id; each
  value is a list of field names the mapper could not carry
  (`lightroom_catalog.rs:880`, `:987`, `:1096`).
- Camera profiles are not applied; the name is reported and the profile
  carries no rendering data (`docs/xmp-interop.md:178`).
- Rendering is approximate: values carry over, the look is not a pixel match
  (`docs/lightroom-catalog-import.md:100-107`, `docs/xmp-interop.md:127-132`).

---

## 2. Where the settings live in a Lightroom catalog

### 2.1 Tables and columns

| Table | Columns used | Purpose | Source |
| --- | --- | --- | --- |
| `Adobe_imageDevelopSettings` | `image`, `text` | one row per image; `image` refers to `Adobe_images.id_local`, `text` is the develop-settings Lua table | `lightroom_catalog.rs:481-484` |
| `Adobe_images` | `id_local`, `id_global`, `rootFile`, `masterImage`, `orientation`, `fileWidth`, `fileHeight`, `fileFormat` | photo identity, virtual copies, crop orientation and aspect | `lightroom_catalog.rs:974-981`; column names in `docs/research/lrcat-schema-lrc15.md:18-33` |
| `Adobe_imageProperties` | `image`, `propertiesString` | Lightroom's extra properties, including default crop and crop aspect | `doc/lrcat_format.md:131-148` |

The other columns of `Adobe_imageDevelopSettings` were not read and are not
documented here (assumption: the table also has `id_local` and `id_global`, as
other catalog tables do; not verified).

**Public documentation status.** No public Adobe schema documentation for the
catalog was found (assumption: Adobe does not publish one). The lrcat-extractor
format notes document folders, files, images, keywords, collections and faces,
but not develop settings (`doc/lrcat_format.md` has no develop section). The table and column names above therefore come from
LightCraft's public reader code, which was written against real catalogs. That
is a usable public source, but it is an implementation, not a specification.
Anything beyond the columns it reads is an **(assumption)**.

### 2.2 Encoding of `text`

The value is a Lua table literal, not JSON. LightCraft's reader:

- parses a subset of Lua without executing it: tables (array and key/value),
  numbers, strings and booleans (`preset_import.rs:100-371`);
- turns every top-level key `K` into the XMP property `crs:K`
  (`preset_import.rs:396-397`), so the catalog keys are the same field names
  that appear as `crs:` properties in XMP;
- keeps nested tables for local corrections in a separate structured value map
  (`preset_import.rs:412-413`);
- flattens a numeric array `x1, y1, x2, y2, …` into `"x, y"` point strings, the
  shape the curve mapper expects (`preset_import.rs:419-426`);
- de-localises strings of the form `"$$$/Key/Path=Default text"`
  (`preset_import.rs:373-379`).

Consequences for lr2lc:

- the catalog needs a small, non-executing Lua reader; a JSON parser is not
  enough;
- field names are not LightCraft control ids; they are Lightroom/XMP names and
  must be mapped;
- a top-level table value other than the known correction containers becomes
  the single string `"(table)"` and is reported as unmapped
  (`preset_import.rs:427-430`).

### 2.3 Special values

| Value | Meaning | Handling | Source |
| --- | --- | --- | --- |
| `-999999` | Lightroom's "deferred adjustment" sentinel (Adobe Auto Tone not evaluated yet) | removed recursively; reported once per photo as `Deferred Lightroom adjustments (-999999)`; never clamped into a slider | `lightroom_catalog.rs:767-780`, `:708-710`; `docs/lightroom-catalog-import.md:118-120` |
| no `HasCrop` | catalog stores crop edges/angle without the XMP flag | any crop field means the photo is cropped | `lightroom_catalog.rs:700-705` |
| `CropAngle` | degrees on the stored image, opposite sign to LightCraft's angle when mirrored | sign taken from the orientation | `lightroom_catalog.rs:745-765` |
| absent `WhiteBalance` | mode not stored | mapper falls back to `custom` if a temperature is present | `crs.rs:284-306` |
| absent `ProcessVersion` | older settings | LightCraft keeps its own latest process; the field is not mapped | `docs/lightroom-catalog-import.md:103-105` |

### 2.4 Per-photo and per-copy scope

Develop settings are keyed by `Adobe_images.id_local`, so a virtual copy has
its own develop row independently of its master. LightCraft's importer reads
the settings by that id (`lightroom_catalog.rs:481-484`, `:973-984`), and its
tests cover different settings on master and copy
(`lightroom_catalog.rs:1246-1251`).

---

## 3. Direction of the mapping

The mapping has two layers:

1. **Catalog field → neutral value.** Lightroom's `crs:`-named field is read,
   converted where the units or encoding differ, and stored in the neutral
   model.
2. **Neutral value → LightCraft.** A control id and a number, or a partial
   settings JSON for structured parts.

The tables below follow LightCraft's mapper (`crs.rs:202-464`,
`crs_masks.rs:254-296`, `:306-431`) because it is the only public,
maintained description of what each field means. Where LightCraft calls a
mapping "1:1" (`docs/xmp-interop.md:143-174`), lr2lc should use the identity.
No unit conversion is claimed where none is shown.

**Range caveat.** LightCraft clamps every value to its control range
(`controls.rs:157-163`; `docs/xmp-interop.md:176`). No public per-slider range
table for Lightroom was found, so the working **(assumption)** is that the
ranges coincide for the fields LightCraft calls 1:1; clamping is the safety net
either way.

---

## 4. Mapping table

Status legend:

- **1:1** — same number, no conversion.
- **converted** — a defined conversion (stated in the row).
- **partial** — some values or sub-fields are lost; the loss is reported.
- **reported** — not carried over; the field name appears in the report.

### 4.1 Light panel

| Lightroom field | LightCraft control | Conversion / status | Source |
| --- | --- | --- | --- |
| `Exposure2012` | `light.exposure` | 1:1 | `crs.rs:211`; `xmp-interop.md:143` |
| `Contrast2012` | `light.contrast` | 1:1 | `crs.rs:212` |
| `Highlights2012` | `light.highlights` | 1:1 | `crs.rs:213` |
| `Shadows2012` | `light.shadows` | 1:1 | `crs.rs:214` |
| `Whites2012` | `light.whites` | 1:1 | `crs.rs:215` |
| `Blacks2012` | `light.blacks` | 1:1 | `crs.rs:216` |
| `HDREditMode` | `hdr.enabled` | boolean → on/off only | `crs.rs:217-220` |
| `Exposure` (old process) | `light.exposure` | 1:1, only when no `*2012` key exists | `crs.rs:235` |
| `Contrast` (old) | `light.contrast` | `v - 25` | `crs.rs:236` |
| `HighlightRecovery` (old) | `light.highlights` | `-v` | `crs.rs:237` |
| `FillLight` (old) | `light.shadows` | 1:1 | `crs.rs:238` |
| `Shadows` (old) | `light.blacks` | `-(v - 5) * 4` | `crs.rs:239` |
| `Brightness` (old) | `light.whites` | `(v - 50) * 0.6` | `crs.rs:240` |

The old-process conversions are LightCraft's approximation, applied only when
the packet has no `*2012` key (`crs.rs:222-234`). They are not an Adobe-published
formula.

### 4.2 White balance

| Lightroom field | LightCraft field/control | Conversion / status | Source |
| --- | --- | --- | --- |
| `WhiteBalance` | `wb.mode` | names map: `As Shot`, `Auto`, `Daylight`, `Cloudy`, `Shade`, `Tungsten`, `Fluorescent`, `Flash`; anything else → `custom` | `crs.rs:250-260`, `:284-285` |
| `Temperature`, `Tint` | `wb.temp`, `wb.tint`, `wb.scale: kelvin` | Kelvin and tint as written, only on targets LightCraft develops on a Kelvin scale (DNG, raws with colour matrices, presets); otherwise see the next row | `crs.rs:261`, `:278-297` |
| `Temperature`, `Tint` on a raw LightCraft develops relative to as-shot | `wb.temp`, `wb.tint` | needs an as-shot reference; the catalog has none, so the photo stays As Shot and `Temperature, Tint (custom white balance without AsShotTemperature: kept As Shot)` is reported | `crs.rs:268-283`, `:298-306`; `docs/lightroom-catalog-import.md:108-117` |
| `IncrementalTemperature`, `IncrementalTint` | `wb.temp`, `wb.tint` | rendered files: −100..100 on the relative scale, mired shift around 6500 K | `crs.rs:176-178`, `:262-264` |

White balance is the one part of the mapping that depends on **probing the
original file**: whether a raw has a Kelvin scale is decided from the file's
colour matrices or its camera's measured matrices, not from the catalog
(`crs.rs:40-57`; `docs/lightroom-catalog-import.md:112-117`). A converter that
never opens originals cannot make that decision. See decision 2.

### 4.3 Presence, treatment and colour mixer

| Lightroom field | LightCraft control | Conversion / status | Source |
| --- | --- | --- | --- |
| `Texture` | `effects.texture` | 1:1 | `crs.rs:242` |
| `Clarity2012` / `Clarity` (old) | `effects.clarity` | 1:1; old field only without `*2012` | `crs.rs:243-244` |
| `Dehaze` | `effects.dehaze` | 1:1 | `crs.rs:245` |
| `Vibrance` | `color.vibrance` | 1:1 | `crs.rs:246` |
| `Saturation` | `color.saturation` | 1:1 | `crs.rs:247` |
| `ConvertToGrayscale` | `treatment` (`color` / `bw`) | boolean | `crs.rs:310-312` |
| `HueAdjustment<Band>` | `mixer.<band>.hue` | 1:1, bands Red, Orange, Yellow, Green, Aqua, Blue, Purple, Magenta | `crs.rs:60`, `:313-316` |
| `SaturationAdjustment<Band>` | `mixer.<band>.sat` | 1:1 | same |
| `LuminanceAdjustment<Band>` | `mixer.<band>.lum` | 1:1 | same |
| `GrayMixer<Band>` | `bw_mix.<band>`, control `bw.<band>` | 1:1 | `crs.rs:317`; `controls.rs:221-228` |

### 4.4 Tone curve

| Lightroom field | LightCraft control | Conversion / status | Source |
| --- | --- | --- | --- |
| `ParametricShadows` | `curve.shadows` | 1:1 | `crs.rs:321` |
| `ParametricDarks` | `curve.darks` | 1:1 | `crs.rs:322` |
| `ParametricLights` | `curve.lights` | 1:1 | `crs.rs:323` |
| `ParametricHighlights` | `curve.highlights` | 1:1 | `crs.rs:324` |
| `ParametricShadowSplit` | `curve.split_shadows` | 1:1 | `crs.rs:325` |
| `ParametricMidtoneSplit` | `curve.split_mid` | 1:1 | `crs.rs:326` |
| `ParametricHighlightSplit` | `curve.split_highlights` | 1:1 | `crs.rs:327` |
| `CurveRefineSaturation` | `curve.refine_saturation` | 1:1 | `crs.rs:328` |
| `ToneCurve` (old) | `curve.master` | point list `x, y` in 0..255 → 0..1; only when no `ToneCurvePV2012` | `crs.rs:181-197`, `:329-334` |
| `ToneCurvePV2012` | `curve.master` | same normalisation; a straight 0→255 line means "no curve" | `crs.rs:339-350` |
| `ToneCurvePV2012Red/Green/Blue` | `curve.red/green/blue` | **all three** channel curves plus the master must be present, otherwise none is read (Lightroom behaves the same way) | `crs.rs:335-349`; `xmp-interop.md:157` |

### 4.5 Colour grading

| Lightroom field | LightCraft control | Status | Source |
| --- | --- | --- | --- |
| `SplitToningShadowHue` | `grading.shadows.hue` | 1:1 | `crs.rs:353` |
| `SplitToningShadowSaturation` | `grading.shadows.sat` | 1:1 | `crs.rs:354` |
| `ColorGradeShadowLum` | `grading.shadows.lum` | 1:1 | `crs.rs:355` |
| `SplitToningHighlightHue` | `grading.highlights.hue` | 1:1 | `crs.rs:356` |
| `SplitToningHighlightSaturation` | `grading.highlights.sat` | 1:1 | `crs.rs:357` |
| `ColorGradeHighlightLum` | `grading.highlights.lum` | 1:1 | `crs.rs:358` |
| `ColorGradeMidtoneHue/Sat/Lum` | `grading.midtones.*` | 1:1 | `crs.rs:359-361` |
| `ColorGradeGlobalHue/Sat/Lum` | `grading.global.*` | 1:1 | `crs.rs:362-364` |
| `ColorGradeBlending` | `grading.blending` | 1:1 | `crs.rs:365` |
| `SplitToningBalance` | `grading.balance` | 1:1 | `crs.rs:366` |

### 4.6 Detail

| Lightroom field | LightCraft control | Status | Source |
| --- | --- | --- | --- |
| `Sharpness` | `detail.sharpenAmount` | 1:1, clamped to 0..150 | `crs.rs:369`; `controls.rs:255` |
| `SharpenRadius` | `detail.sharpenRadius` | 1:1, clamped to 0.5..3 | `crs.rs:370`; `controls.rs:256` |
| `SharpenDetail` | `detail.sharpenDetail` | 1:1 | `crs.rs:371` |
| `SharpenEdgeMasking` | `detail.sharpenMasking` | 1:1 | `crs.rs:372` |
| `LuminanceSmoothing` | `detail.nrLuminance` | 1:1 | `crs.rs:373` |
| `LuminanceNoiseReductionDetail` | `detail.nrDetail` | 1:1 | `crs.rs:374` |
| `LuminanceNoiseReductionContrast` | `detail.nrContrast` | 1:1 | `crs.rs:375` |
| `ColorNoiseReduction` | `detail.nrColor` | 1:1 | `crs.rs:376` |
| `ColorNoiseReductionDetail` | `detail.nrColorDetail` | 1:1 | `crs.rs:377` |
| `ColorNoiseReductionSmoothness` | `detail.nrColorSmoothness` | 1:1 | `crs.rs:378` |

Raws get LightCraft's own capture-sharpening and colour-noise defaults at
import time, not from the mapper (`settings.rs:629-631`, cited in
`docs/research/lightcraft-develop.md:32`). A catalog that stores Lightroom's
defaults explicitly will therefore set the same controls explicitly; whether
the two defaults are equal is not claimed (assumption: they are close, not
identical).

### 4.7 Effects: post-crop vignette, grain, HDR

| Lightroom field | LightCraft control | Conversion / status | Source |
| --- | --- | --- | --- |
| `PostCropVignetteAmount` | `vignette.amount` | 1:1 | `crs.rs:381` |
| `PostCropVignetteMidpoint` | `vignette.midpoint` | 1:1 | `crs.rs:382` |
| `PostCropVignetteRoundness` | `vignette.roundness` | 1:1 | `crs.rs:383` |
| `PostCropVignetteFeather` | `vignette.feather` | 1:1 | `crs.rs:384` |
| `PostCropVignetteHighlightContrast` | `vignette.highlights` | 1:1 | `crs.rs:385` |
| `PostCropVignetteStyle` | `vignette.style` | number → `highlightPriority` (1), `colorPriority` (2), `paintOverlay` (3) | `crs.rs:386-393` |
| `GrainAmount` | `grain.amount` | 1:1 | `crs.rs:394` |
| `GrainSize` | `grain.size` | 1:1 | `crs.rs:395` |
| `GrainFrequency` | `grain.roughness` | 1:1 (name differs) | `crs.rs:396` |

### 4.8 Optics

| Lightroom field | LightCraft control | Conversion / status | Source |
| --- | --- | --- | --- |
| `LensProfileEnable` | `optics.lens_profile` | only `1`/`True` carries over; `0` (the default in most presets) leaves LightCraft's embedded corrections as they are | `crs.rs:398-404`; `xmp-interop.md:168` |
| `AutoLateralCA` | `optics.remove_ca` | boolean | `crs.rs:405-407` |
| `LensManualDistortionAmount` | `optics.distortion` | 1:1 | `crs.rs:408` |
| `VignetteAmount` | `optics.vignetting` | 1:1 | `crs.rs:409` |
| `VignetteMidpoint` | `optics.vignettingMidpoint` | 1:1 | `crs.rs:410` |
| `DefringePurpleAmount` | `optics.defringePurple` | 1:1 | `crs.rs:411` |
| `DefringePurpleHueLo` | `optics.defringePurpleHueLo` | 1:1 | `crs.rs:412` |
| `DefringePurpleHueHi` | `optics.defringePurpleHueHi` | 1:1 | `crs.rs:413` |
| `DefringeGreenAmount` | `optics.defringeGreen` | 1:1 | `crs.rs:414` |
| `DefringeGreenHueLo` | `optics.defringeGreenHueLo` | 1:1 | `crs.rs:415` |
| `DefringeGreenHueHi` | `optics.defringeGreenHueHi` | 1:1 | `crs.rs:416` |
| `LensProfileName`, `LensProfileFilename`, `LensProfileIsEmbedded`, `LensProfileSetup`, `LensProfileDigest` | — | ignored: LightCraft uses the file's own corrections | `crs.rs:100-104`; `xmp-interop.md:168` |
| `LensProfileDistortionScale`, `LensProfileVignettingScale` | — | ignored at 100; reported when changed | `crs.rs:506-507`, `:516`; test `crs.rs:896-898` |

### 4.9 Geometry and crop

| Lightroom field | LightCraft control/field | Conversion / status | Source |
| --- | --- | --- | --- |
| `PerspectiveVertical` | `geometry.vertical` | 1:1 | `crs.rs:428` |
| `PerspectiveHorizontal` | `geometry.horizontal` | 1:1 | `crs.rs:429` |
| `PerspectiveRotate` | `geometry.rotate` | 1:1 | `crs.rs:430` |
| `PerspectiveScale` | `geometry.scale` | 1:1 | `crs.rs:431` |
| `PerspectiveAspect` | `geometry.aspect` | 1:1 | `crs.rs:432` |
| `PerspectiveX` | `geometry.offsetX` | 1:1 | `crs.rs:433` |
| `PerspectiveY` | `geometry.offsetY` | 1:1 | `crs.rs:434` |
| `PerspectiveUpright` | `geometry.upright` | number → off (0), auto (1), level (2), vertical (3), full (4), guided (5) | `crs.rs:435-445` |
| `UprightTransform_*`, `UprightTransformCount`, `UprightCenterNorm*`, `UprightVersion` | — | saved matrices are not imported; reported (the mode is recalculated) | `xmp-interop.md:173`; `crs.rs:528-536` |
| `CropLeft/Top/Right/Bottom` | `crop.geometry.rect` (`x0,y0,x1,y1`) | normalised edges of the stored image → rectangle in the oriented, rotated frame; aspect comes from `fileWidth`/`fileHeight` | `crs.rs:447-458`; `lightroom_catalog.rs:737-765` |
| `CropAngle` | `crop.geometry.angle`, control `crop.angle` | degrees, sign flipped when the orientation mirrors | `crs.rs:456`; `lightroom_catalog.rs:754-757` |
| missing `HasCrop` | — | inferred from any crop field | `lightroom_catalog.rs:700-705` |

### 4.10 Calibration

| Lightroom field | LightCraft control | Status | Source |
| --- | --- | --- | --- |
| `ShadowTint` | `calibration.shadowsTint` | 1:1 | `crs.rs:419` |
| `RedHue`, `RedSaturation` | `calibration.redHue`, `calibration.redSat` | 1:1 | `crs.rs:420-421` |
| `GreenHue`, `GreenSaturation` | `calibration.greenHue`, `calibration.greenSat` | 1:1 | `crs.rs:422-423` |
| `BlueHue`, `BlueSaturation` | `calibration.blueHue`, `calibration.blueSat` | 1:1 | `crs.rs:424-425` |

### 4.11 Local corrections (masks) and spot removal

| Lightroom field | LightCraft field | Conversion / status | Source |
| --- | --- | --- | --- |
| `MaskGroupBasedCorrections`, `GradientBasedCorrections`, `CircularGradientBasedCorrections`, `PaintBasedCorrections` | `masks[]` | one correction → one mask; four containers supported | `crs_masks.rs:23-24`, `:252-296` |
| `CorrectionName`, `CorrectionAmount`, `CorrectionActive` | mask name, amount, visibility | amount ×100, clamped 0..200; inactive corrections skipped | `crs_masks.rs:95-100`, `:260-291` |
| `LocalExposure2012` | `masks[].adjust.exposure` | fraction → ×4 EV | `crs_masks.rs` (table in `xmp-interop.md:222`) |
| other `Local*` sliders | matching `adjust.*` | stored −1..1 → ×100; hue in degrees | `xmp-interop.md:223-224` |
| `Mask/Gradient` | linear mask | endpoints from `FullX/Y`, `ZeroX/Y` | `xmp-interop.md:226` |
| `Mask/CircularGradient` | radial mask | box fractions → long-edge radii; feather and `Flipped` rules differ per container | `xmp-interop.md:227`; `crs_masks.rs:190-195` |
| `Mask/Paint` | brush mask | dabs, radius, flow, `MaskValue` ≤ 0 erases | `crs_masks.rs:196-220` |
| `Mask/Image` `MaskSubType` 1 / 2 | subject / sky mask | other AI subtypes are reported as `Mask: AI mask (n)` | `crs_masks.rs:221-225` |
| `Mask/RangeMask` type 2 / 3 | luminance / depth range | lightness converted; colour ranges are reported | `crs_masks.rs:226-245` |
| `MaskBlendMode`, `MaskInverted` | add / subtract / intersect, invert | numbers 0/1/2 | `xmp-interop.md:231` |
| `RetouchAreas`, `RetouchInfo` | `spots[]` | heal/clone, circle or brush, source offset, opacity, feather | `crs_masks.rs:298-431` |
| other mask kinds | — | reported as `Mask: <kind>` | `crs_masks.rs:240-248` |
| other `Local*` sliders (e.g. `LocalGrain`) | — | reported as `Mask: LocalGrain` when set | `crs_masks.rs:102-115` |

The correction geometry is converted relative to the photo's aspect ratio
(width / height) for radial and spot masks (`crs_masks.rs:253-254`); presets
assume 3:2 (`xmp-interop.md:227`). lr2lc must pass the real photo aspect, which
it knows from the catalog's `fileWidth`/`fileHeight` (section 2.1).

---

## 5. What has no counterpart, and how it would be reported

### 5.1 Settings that are not carried over

| Lightroom setting | Why it cannot be carried over | LightCraft behaviour today | Report behaviour | Source |
| --- | --- | --- | --- | --- |
| `ProcessVersion` | names Lightroom's rendering algorithm | LightCraft keeps its own latest process | **silent**: listed as bookkeeping, not in `unmapped` | `crs.rs:63-107`; `docs/lightroom-catalog-import.md:103-105` |
| `CameraProfile` | profile name carries no rendering data; LightCraft has its own profiles | profile untouched | reported as `CameraProfile` | `xmp-interop.md:178`; test `lightroom_catalog.rs:1255` |
| `Look` (a named profile/preset) | LightCraft's catalog import does not resolve a Look to a profile | profile untouched | reported as `Look` | `crs.rs:498-516`; test `crs.rs:893-894` |
| non-empty `PointColors` | mapper has no Point Color conversion | model and `pointColor.*` controls exist, but stay at defaults | reported as `PointColors` | `crs.rs:498-508`; test `crs.rs:876-884` |
| red-eye corrections | not read by the mapper | `red_eye` model and `redEye.*` controls exist | reported (field name) — **(assumption:** that catalogs use a `RedEyeInfo` field and it lands in `unmapped`; the field name was not verified) | `xmp-interop.md:191-192` |
| lens blur | not read by the mapper | `lens_blur` model field exists | reported (field names not verified) — **(assumption)** | `xmp-interop.md:191-192` |
| Adobe AI denoise | not read by the mapper | `enhance.denoise` control exists | reported (field names not verified) — **(assumption)** | `xmp-interop.md:191-192`; `controls.rs:254` |
| HDR SDR/headroom sliders | mapper reads only `HDREditMode` | `hdr.*` controls exist, stay at defaults | reported when the catalog contains those fields — **(assumption:** field names not verified) | `crs.rs:217-220`; `settings.rs:116-119` |
| saved Upright matrices | coordinate convention not imported | geometry mode is recalculated | reported | `crs.rs:528-536` |
| AI masks other than subject/sky | no counterpart | mask is skipped, other components of the same correction survive | `Mask: AI mask (n)` | `crs_masks.rs:221-225` |
| colour-range masks | LightCraft has no colour range shape | mask component skipped | `Mask: colour range` | `crs_masks.rs:240-245` |
| local sliders without counterpart | no matching `adjust.*` | correction otherwise carried over | `Mask: <field>` | `crs_masks.rs:102-115` |
| lens profile name/file/digests | LightCraft uses file-embedded corrections | ignored | **silent** (bookkeeping) | `crs.rs:96-104` |
| history and snapshots | source data, not native LightCraft history | archived in the recovery archive, not imported | not part of `unmapped` | `docs/lightroom-catalog-import.md:76-83`, `:105-107` |
| `-999999` deferred values | not a value | removed before mapping | `Deferred Lightroom adjustments (-999999)` | `lightroom_catalog.rs:708-710` |

"Reported" means: the field name (without the `crs:` prefix) appears in the
per-photo `unmapped` list of the import result (`lightroom_catalog.rs:509-519`,
`:986-988`). Fields that only switch a panel or name a profile are filtered out
of the report (`crs.rs:498-508`): `Enable*`, `ToneCurveName`, `AutoTone`,
`AutoGrayscaleMix`, empty `PointColors`, and lens-profile scales at 100.

### 5.2 Settings for which LightCraft has a model but the import sets nothing

This is the reverse gap: LightCraft could hold the value, but nothing in the
Lightroom mapping produces it. These are candidates for later phases, not MVP:

- camera profile choice (`profile`), profile amount (`profile.amount`);
- Point Color (`point_colors`, `pointColor.*`);
- red eye (`red_eye`, `redEye.*`);
- lens blur (`lens_blur`);
- AI denoise (`enhance.denoise`);
- HDR SDR controls (`hdr.sdr*`, `hdr.maxEv`);
- manual lens-profile strength (`optics.profileDistortion`,
  `optics.profileVignetting`).

Reporting these as "no counterpart" would be wrong; the correct wording is
"not yet mapped".

### 5.3 How lr2lc's own `report.json` should show it

lr2lc already has stable skip codes (`crates/model/src/skip.rs:13-...`). The
develop phase needs at least:

- `DevelopUnmapped` — one entry per photo and field (or an aggregated count),
  with the field name as the free-text reason;
- `DevelopInvalid` — the settings text could not be parsed, or a value had the
  wrong type;
- `DevelopDeferred` — the `-999999` sentinel was present;
- `DevelopNotApplied` — changed settings on re-sync were not written because
  the photo already has edits (see decision 5).

The names follow the existing convention (fixed snake_case, no catalog values,
`skip.rs:1-6`). The counts must satisfy "read = converted + skipped"
(`CLAUDE.md`, principle 4): every photo with a develop row is either
converted, partially converted (with its unmapped list), or skipped.

---

## 6. Proposal for the model extension

### 6.1 Design constraints

1. The model is neutral (`CLAUDE.md`, principle 7). It should not depend on
   LightCraft crates, but it may carry LightCraft control ids because the
   writer's only official interface is control ids and partial settings JSON
   (`docs/research/lightcraft-develop.md:22-29`).
2. The tool must stay small (principle 10). Re-declaring LightCraft's 122
   controls as Rust fields duplicates a table that LightCraft already owns and
   may change.
3. A second run must not overwrite edits (principle 5 and the "only adds"
   promise).
4. Nothing is lost silently (principle 4): unmapped fields belong in the model
   so the report can be built without re-reading the catalog.

### 6.2 Proposed types (sketch)

```rust
/// Development settings of one photo, already expressed in LightCraft's
/// control ids and partial-settings JSON.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Develop {
    /// Slider-addressable values: LightCraft control id -> value.
    /// Only controls whose value differs from the LightCraft default.
    pub controls: BTreeMap<String, f64>,
    /// Structured parts no slider addresses.
    pub curves: Curves,
    pub crop: Option<CropGeometry>,
    /// Masks and spots, phase 6b (see decision 3).
    pub masks: Vec<Mask>,
    pub spots: Vec<Spot>,
    /// White balance metadata that cannot be expressed as a control.
    pub white_balance: WhiteBalanceHint,
    /// Fields present in the catalog that could not be carried over,
    /// deduplicated, in catalog order.
    pub unmapped: Vec<String>,
    /// Hash of the source `text` value, for idempotent re-sync.
    pub source_hash: Option<[u8; 32]>,
}

pub struct Curves {
    pub master: Vec<CurvePoint>,
    pub red: Vec<CurvePoint>,
    pub green: Vec<CurvePoint>,
    pub blue: Vec<CurvePoint>,
}

pub struct CropGeometry { pub x0: f64, pub y0: f64, pub x1: f64, pub y1: f64, pub angle: f64 }

pub enum WhiteBalanceHint {
    None,
    Kelvin,          // value is Kelvin; writer must send `wb.scale: "kelvin"`
    Relative,        // value is on the relative scale
    Undecidable,     // custom Kelvin on a raw without as-shot reference; reported
}
```

`Photo` gets `pub develop: Option<Develop>`; `None` means "the catalog had no
usable settings" and is distinct from `Some(Develop::default())` (a photo with
a settings row that is all defaults).

Why a map of control ids instead of typed sections: it matches
`develop.controls` / `develop.set` exactly, keeps the model one screen long,
and lets the report list unsupported ids without a second table. The cost is
that the compiler cannot check that `light.exposure` exists; a small
`const`-checked list of ids used by the reader (or a unit test against a
checked-in copy of LightCraft's ids) closes that gap. This is decision 6.

### 6.3 Writer mapping

Two possible command shapes:

1. **Sliders only:** one `develop.set {values: {...}, ids: [photo]}` per photo
   (or per batch of photos with identical values). Cheap, one undo step, but
   cannot carry curves, masks, spots or crop rectangles.
2. **Everything else:** `library.select` the photo, then
   `develop.merge {settings: partial}`. The partial is the JSON object with
   LightCraft's field names (`light`, `wb`, `curve`, `crop`, `masks`,
   `spots`, …); `develop.merge` merges it into the current settings
   (`develop.rs:256-263`). This is the same shape LightCraft's own importer
   produces (`crs.rs:202-464`).

Recommendation: use **(2) for every photo that has any structured part**, and
**(1) for pure-slider photos**. Both fit the existing plan machinery: a new
`Step::Develop` would emit `library.select` + `develop.merge`, and a batched
`develop.set` with `ids` for the rest.

White balance: when `WhiteBalanceHint::Kelvin`, the partial must carry
`wb.scale: "kelvin"`, exactly as the mapper does (`crs.rs:294-296`), because a
partial whose `wb` sets `temp`/`tint` is read on the relative scale otherwise
(`docs/control-protocol.md:115-122`). `WhiteBalanceHint::Undecidable` must not
be written; it is reported instead.

### 6.4 Safe re-sync

The existing writer plan compares input with the live library and only adds
(`docs/research/lightcraft-library.md`, `reconciliation.md`). Develop settings
need one more rule:

- **New photo:** write its develop settings in the same run.
- **Known photo, settings hash unchanged:** no command.
- **Known photo, settings changed, no explicit opt-in:** no command; report
  `DevelopNotApplied` with a count. This preserves the "never silently
  overwrite" promise.
- **Known photo, settings changed, opt-in (`--update-develop` or similar):**
  write, and say so in the report.

The mapping file should store the source settings hash next to the existing
identity records, so the third case is distinguishable from the second
without reading the library's develop state (assumption: the existing mapping
format has room for optional per-photo data; check
`crates/lc-writer/src/state.rs` before implementing).

### 6.5 Alternatives considered

- **Call `library.importLightroom` and let it do everything.** Smallest code,
  best tested mapper, but it imports the whole catalog and bypasses lr2lc's
  reader, report, path mapping and dry-run. It also cannot apply lr2lc's own
  decisions (e.g. skip a video, refuse a path). Reasonable as a documented
  fallback, not as the main path (see decision 1).
- **Copy LightCraft's `crs.rs` mapping.** Excluded: different repository and
  license, and the mapping is already available through the official
  interface. lr2lc writes its own reader.
- **Store the raw Lua text in the model and translate at write time.** Avoids
  a conversion layer but leaks a Lightroom format into the neutral model.
  Rejected.

---

## 7. Test plan with generated fixtures

All fixtures are generated; no real catalog file or value is used
(`tests/fixtures/README.md:1-4`). The generator is the `lrcat-fixtures` crate
(`tests/fixtures/generator`), which writes Lightroom Classic 15 layouts
deterministically (`tests/fixtures/README.md:6-9`).

### 7.1 Generator additions

1. Add `CREATE TABLE Adobe_imageDevelopSettings (id_local INTEGER PRIMARY KEY,
   id_global, image INTEGER NOT NULL, text)` and one row per photo that has
   settings. The exact real column set beyond `image`/`text` is unknown
   (section 2.1); extra columns can be added later without changing the
   reader's contract.
2. Add a `develop` module with small builders for settings text, so tests do
   not hand-write Lua strings:
   - `settings().exposure(1.25).contrast(-10).build()`
   - `settings().old_process().build()`
   - `settings().curve(&[(0, 0), (128, 140), (255, 255)])`
   - `settings().mask_radial(...)`, `.spot_heal(...)`
   - `settings().raw_text("…")` for malformed input.
3. Add fixture variants:
   - `develop_all_sliders` — one photo with every mapped control set;
   - `develop_old_process` — a pre-2012 settings table;
   - `develop_bw` — `ConvertToGrayscale` plus `GrayMixer*` and HSL;
   - `develop_curves` — master only, master + one channel, and all four;
   - `develop_crop_orientation` — crop with each of the eight orientations;
   - `develop_virtual_copy` — different settings on master and copy;
   - `develop_unmapped` — unknown field, `PointColors`, `CameraProfile`,
     AI mask, colour range, `LocalGrain`;
   - `develop_deferred` — `-999999` in several places;
   - `develop_malformed` — unbalanced table, a string where a number belongs,
     a 20 MiB text, a 1,000-deep nested table;
   - `develop_absent` — no `Adobe_imageDevelopSettings` table at all;
   - `develop_empty` — table present, no rows for any photo.

### 7.2 Reader tests (`crates/lr-reader`)

1. Every 1:1 field from section 4 lands on the stated control id with the same
   value.
2. Old-process fields produce the documented formulas and are ignored when a
   `*2012` key is present.
3. White balance: mode names; Kelvin path sets `WhiteBalanceHint::Kelvin`;
   custom without as-shot sets `Undecidable` and produces a report entry; an
   `Incremental*` pair uses the relative scale.
4. Curves: `x, y` 0..255 → 0..1; identity line dropped; channel curves only
   when all three plus the master exist; an old `ToneCurve` is used only
   without `ToneCurvePV2012`.
5. Crop: normalised edges, angle sign for mirrored orientations, the same
   frame for a rotated crop as LightCraft's own correction (port the expected
   values from the documented rule, not from a real catalog).
6. Masks: each of the four containers; brush merging; `MaskValue <= 0` erases;
   subject/sky subtypes; luminance/depth ranges; colour range reported; the
   photo aspect is used for radial geometry.
7. Spots: heal and clone; circle and brush; source offset present only with
   `sourceSetExplicitly`.
8. Sentinel `-999999` removed everywhere and counted once.
9. Unknown fields reported once per photo, in a deterministic order, without
   values in the reason text.
10. Malformed or oversized text skips the photo's settings with
    `DevelopInvalid`, and the run continues (the rest of the catalog must
    still convert).
11. Absent table → zero develop settings, no error, no warning (an optional
    table).
12. Virtual copy gets its own settings; no inheritance from the master.
13. `read = converted + skipped` holds with and without develop settings.
14. The reader opens the catalog read-only and the checksum before/after is
    unchanged (existing invariant, re-asserted here because a new table is
    read).

### 7.3 Writer tests (`crates/lc-writer`)

1. Pure-slider photo → one `develop.set` with `values` and `ids`, no
   `library.select` needed.
2. Photo with a curve/crop → `library.select` then `develop.merge` with the
   partial JSON; field names and nesting match LightCraft's documented shape.
3. `WhiteBalanceHint::Kelvin` → `wb.scale: "kelvin"` present;
   `Undecidable` → no WB command and a report entry.
4. Dry run: no develop command leaves the process; the report announces the
   counts.
5. Second run with identical input → zero develop commands.
6. Changed input on a known photo → no command by default, `DevelopNotApplied`
   reported; with opt-in → one command and a note.
7. New photo in a run that also has known photos → develop command only for
   the new one.
8. Unmapped fields appear in `report.json` with stable codes and no values.
9. The fake control server (`crates/lc-test-support/src/fake_lightcraft.rs`)
   receives the expected commands and arguments and returns the fake photo
   id; a rejected command aborts the run without partial writes.

### 7.4 End-to-end tests (`crates/lr2lc/tests`)

1. `develop_all_sliders` catalog → fake LightCraft → expected script and
   `report.json`.
2. `develop_unmapped` catalog → counts: converted, partially converted,
   skipped; per-field unmapped entries.
3. `develop_malformed` catalog → the bad photo is skipped, all other photos
   are converted, exit status reflects "completed with skips" as the existing
   CLI defines it.
4. Byte-identical catalog checksum before and after.
5. A large generated catalog (e.g. 10,000 photos) with settings rows completes
   within the project's performance budget (assumption: the existing
   large-catalog budget applies unchanged; measure before promising a number).

### 7.5 Fidelity tests

Slider parity can be tested against the mapping table. **Rendering parity
cannot** be tested inside this repository: it would need a reference renderer
and reference exports, and those would come from a real catalog, which must
not enter the repository (principle 9). The honest test is structural:
"the same control ids and values arrive". Rendering differences are documented
as expected (`docs/lightroom-catalog-import.md:100-107`).

---

## 8. Open decisions for the owner

1. **Which route takes the develop settings?**
   (a) lr2lc reads the catalog and writes settings through `develop.set` /
   `develop.merge`; (b) lr2lc calls LightCraft's `library.importLightroom` for
   develop settings only; (c) lr2lc calls it for the whole import and becomes
   a thin wrapper. (a) keeps the neutral pipeline and the report, but needs a
   Lua parser and a maintained mapper; (b) is not possible as a partial call
   today — the command imports develop settings together with everything else;
   (c) abandons most of lr2lc's own value.
2. **White balance on raws.** The correct Kelvin handling needs the original
   file to be probed (colour matrices, as-shot reference). Options: allow
   read-only probing of originals; map WB only for DNG/absolute-scale files and
   report the rest; or map everything on the relative scale and accept a
   visible colour shift. This is a user-visible quality decision.
3. **Scope of phase 6.** Sliders only, or also curves, crop, masks and spots?
   Masks and spots nearly double the reader and are the largest source of
   edge cases. Suggest: sliders + curves + crop in phase 6, masks/spots in 6b.
4. **Model shape.** Control-id map plus structured fields (recommended), or
   typed sections mirroring `DevelopSettings`. The typed version is safer to
   use but duplicates a 122-entry table owned by LightCraft.
5. **Re-sync policy for changed edits.** Never overwrite (report only), or an
   explicit opt-in flag? The project promise says "only adds and never
   silently overwrites"; an opt-in flag is the smallest extension.
6. **Report size.** One entry per unmapped field per photo can explode on
   large catalogs with an unsupported AI feature everywhere. Options: full
   detail in `report.json`, aggregated counts in the readable summary; or cap
   the detail per field and add a count.
7. **Fields that are assumed.** Red eye, lens blur, AI denoise and the HDR SDR
   sliders are listed as unmapped by LightCraft, but their catalog field names
   were not verified here. Someone with a catalog should confirm the names
   before lr2lc reports them; until then they stay assumptions.
8. **Camera profile handling.** LightCraft does not apply Adobe profiles.
   Should lr2lc report "profile not applied" once per photo, once per profile
   name, or only in the summary? The name is metadata, not a value, but it may
   still be user-visible.

---

## 9. Sources

### Citation rule

LightCraft was read at commit `bb08a697a41b2561a54f1b740245119503b24758`
(its `Cargo.toml` says version 0.14.0), retrieved 2026-11-04 from
https://github.com/storytold/lightcraft.

- A citation written `lightcraft:<path>:<line>` means
  https://github.com/storytold/lightcraft/blob/bb08a697a41b2561a54f1b740245119503b24758/<path>#L<line>.
- The path shorthands under "How to read this document" use the same rule:
  `crs.rs:211` is `crates/engine/src/crs.rs:211`, i.e.
  https://github.com/storytold/lightcraft/blob/bb08a697a41b2561a54f1b740245119503b24758/crates/engine/src/crs.rs#L211.
  A range (`:211-220`) is `#L211-L220`.
- A document shorthand without a directory (`xmp-interop.md:143`,
  `lightroom-catalog-import.md:59-74`) is a file under `docs/` of the same
  commit, e.g.
  https://github.com/storytold/lightcraft/blob/bb08a697a41b2561a54f1b740245119503b24758/docs/xmp-interop.md#L143.

lrcat-extractor was read at commit
`a3a59b37a1081a03f69b06648210c65a53aeb482` (version 0.6.0), retrieved
2026-11-04 from https://github.com/hfiguiere/lrcat-extractor. A citation
`doc/lrcat_format.md:131` means
https://github.com/hfiguiere/lrcat-extractor/blob/a3a59b37a1081a03f69b06648210c65a53aeb482/doc/lrcat_format.md#L131.

lr2lc is not published yet; its citations (`docs/…`, `crates/…`, `tests/…`,
`CLAUDE.md`) have no public URL and refer to commit `e611212` in the local
repository. The one bare lr2lc shorthand, `skip.rs` (§5), means
`crates/model/src/skip.rs`.

A statement without a citation or URL is an assumption.

### Public sources

| Source | Version / commit | URL | Retrieved |
| --- | --- | --- | --- |
| LightCraft source tree | 0.14.0, `bb08a697a41b2561a54f1b740245119503b24758` | https://github.com/storytold/lightcraft/tree/bb08a697a41b2561a54f1b740245119503b24758 | 2026-11-04 |
| LightCraft `docs/lightroom-catalog-import.md` | same commit | https://github.com/storytold/lightcraft/blob/bb08a697a41b2561a54f1b740245119503b24758/docs/lightroom-catalog-import.md | 2026-11-04 |
| LightCraft `docs/xmp-interop.md` | same commit | https://github.com/storytold/lightcraft/blob/bb08a697a41b2561a54f1b740245119503b24758/docs/xmp-interop.md | 2026-11-04 |
| LightCraft `docs/control-protocol.md` | same commit | https://github.com/storytold/lightcraft/blob/bb08a697a41b2561a54f1b740245119503b24758/docs/control-protocol.md | 2026-11-04 |
| LightCraft `docs/process-versions.md` | same commit | https://github.com/storytold/lightcraft/blob/bb08a697a41b2561a54f1b740245119503b24758/docs/process-versions.md | 2026-11-04 |
| lrcat-extractor | 0.6.0, `a3a59b37a1081a03f69b06648210c65a53aeb482` | https://github.com/hfiguiere/lrcat-extractor/tree/a3a59b37a1081a03f69b06648210c65a53aeb482 | 2026-11-04 |
| lrcat-extractor `doc/lrcat_format.md` | same commit | https://github.com/hfiguiere/lrcat-extractor/blob/a3a59b37a1081a03f69b06648210c65a53aeb482/doc/lrcat_format.md | 2026-11-04 |
| lr2lc repository | `e611212` | not published; local repository only | – |

What was read from these sources:

- LightCraft 0.14.0, `crates/engine/src/crs.rs`, `crs_masks.rs`,
  `lightroom_catalog.rs`, `preset_import.rs`, `cmd/develop.rs`, `cmd/query.rs`.
- LightCraft 0.14.0, `crates/develop/src/settings.rs`, `controls.rs`.
- LightCraft 0.14.0, `docs/lightroom-catalog-import.md`,
  `docs/xmp-interop.md`, `docs/control-protocol.md`,
  `docs/process-versions.md`.
- lr2lc `main` at `e611212`, `docs/research/lightcraft-develop.md`,
  `lightcraft-library.md`, `lightcraft-import.md`, `lrcat-schema-lrc15.md`,
  `docs/research/reconciliation.md`, `CLAUDE.md`,
  `crates/model/src/lib.rs`, `crates/model/src/skip.rs`,
  `crates/lc-writer/src/plan.rs`, `crates/lc-test-support/src/fake_lightcraft.rs`,
  `tests/fixtures/README.md`, `tests/fixtures/generator/`.
- lrcat-extractor 0.6.0, `doc/lrcat_format.md` (read only).

No other public source is cited in this document. In particular, no public
Adobe schema or slider-range documentation was found (§2.1, §3 range caveat);
those parts are marked as assumptions and are not upgraded by this section.
