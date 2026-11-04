# Development settings: mapping from Lightroom Classic to LightCraft

> **Status:** Phase-6 groundwork, updated in P6-0 (2026-11-04). The scope and
> seven of the eight open decisions are settled (section 8); the white-balance
> probe is still open (section 8.2). Nothing is implemented yet and development
> settings are not converted by lr2lc. The lr2lc citations were re-checked
> against commit `04ee6f5` (section 0).

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
built on. A statement that carries neither a citation nor a URL from §10 is an
assumption; where such a statement is not marked explicitly, treat it as one.

Source roots used below:

- **LightCraft 0.14.0 source tree** — commit
  `bb08a697a41b2561a54f1b740245119503b24758`, public at
  https://github.com/storytold/lightcraft (§10 gives the URL for every
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
- **lr2lc repository**, branch `main`, commit `04ee6f5` (the state this
  document was re-checked against in P6-0). The repository is not published
  yet, so these citations have no public URL. Section 0 lists the lr2lc
  citations checked in P6-0, with the corrected line range where a citation
  had drifted.
- **lrcat-extractor 0.6.0** — commit
  `a3a59b37a1081a03f69b06648210c65a53aeb482`, public at
  https://github.com/hfiguiere/lrcat-extractor (§10). Only read, no code copied
  (MPL-2.0). Its format notes are `doc/lrcat_format.md`.

No real catalog was opened for this document. No values, names, paths or
keywords from any catalog appear here. The only examples are synthetic field
names and invented numbers.

---

## 0. Citation check against lr2lc `04ee6f5` (P6-0)

The lr2lc citations in this document were corrected against `04ee6f5`; the
table below lists the checked sites with their current ranges. Every cited
lr2lc file exists at that commit. Changes since T12 are recorded here rather
than assumed away.

| Citation in this document | State at `04ee6f5` | Where it is now |
| --- | --- | --- |
| `crates/lc-writer/src/plan.rs:401-416` (the `library.select` + command pattern) | **stale** | The plan emits `library.select` and the command at `crates/lc-writer/src/plan.rs:530-546`. `Step::Update` itself is declared at `crates/lc-writer/src/plan.rs:157-164`; the `Step` enum starts at `:130` (first variants `CreateKeyword`, `Import`). |
| `crates/lc-writer/src/plan.rs:530-546` (cited at §1.3, §6.3) | valid | `Step::Update` injects `ids` into the command parameters at `crates/lc-writer/src/plan.rs:536-544`. |
| `crates/lc-writer/src/plan.rs:155-160` (cited at §1.3) | valid | The `Step::Update` doc comment names only `photo.*` commands at `crates/lc-writer/src/plan.rs:155-160`. |
| `crates/model/src/skip.rs:13-...`, `:1-6` | valid | `SkipCode` starts at `crates/model/src/skip.rs:13`; it now holds 58 codes, with `SkipCode::ALL` at `crates/model/src/skip.rs:102` (`:129` is a single variant, `CollectionUnknown`). New develop codes append to both lists. |
| `crates/lc-writer/src/state.rs` (prose, §6.4) | exists | The per-photo record is `PhotoEntry` (`crates/lc-writer/src/state.rs:87-105`); it already has two optional fields (`path` at `:98`, `copy_of` at `:104`), serde-defaulted at `:97` and `:103`. `KeywordEntry.lightroom_id` (`:116`, serde attribute; declaration `:117`) is a third such field. A develop hash can be added without a format break. The file version is `1` (`:23`) and is compared exactly (`:258`); no serde `deny_unknown_fields`, so older readers ignore a new field. |
| `crates/lc-writer/src/state.rs:258` (cited at §6.4) | valid | The loader compares `version` exactly at `crates/lc-writer/src/state.rs:258`. |
| `crates/lc-test-support/src/fake_lightcraft.rs` (§7.3 item 9) | exists | The file is present; it was not re-read in P6-0. |
| `tests/fixtures/README.md:1-4`, `:6-9` | valid | Generated-only rule at `tests/fixtures/README.md:3-4`; generator and layout at `:6-9`. |
| `tests/fixtures/generator` (§7) | valid | Workspace crate `lrcat-fixtures` (`tests/fixtures/generator/Cargo.toml:2`). |
| `docs/research/lightcraft-develop.md:20`, `:22-29`, `:27`, `:32` | valid | Storage, command list, `develop.merge` line and importer flow are at those lines. |
| `docs/research/lrcat-schema-lrc15.md:18-33` | valid | The `Adobe_images` column table starts at `:18`. |
| `crates/model/src/lib.rs` (`Photo`, §6.2) | exists, no develop field | `Photo` is at `crates/model/src/lib.rs:103-131` and has no `develop` field. No `Develop` type exists anywhere under `crates/` (grep, P6-0). |
| `crates/lr2lc/src/output.rs:11-40` (cited at §8, decision 6) | valid | `ReportJson` with one area per kind and `skipped_by_code` is at `crates/lr2lc/src/output.rs:11-35`; `AreaJson` is declared at `:39`. |
| `docs/research/reconciliation.md:1-10` (cited at §6.4) | valid | The status note at `docs/research/reconciliation.md:3` marks the default-only overwrite rule as planned, not implemented. |
| `crates/catalog/src/model.rs:248-254` (§1.1) | valid, but **LightCraft** path | Not an lr2lc path: it is `Photo.develop` in the LightCraft tree at the cited commit. The document says so at the point of use. |

For the new field-name list (section 4.12) the LightCraft commands and control
table were re-read at the same commit: `develop.set` (`develop.rs:167-233`),
`develop.merge` (`develop.rs:256-263`), the merge implementation
(`crates/develop/src/lib.rs:45-57`), the control table and its accessors
(`controls.rs:144-166`), the 122 entries (`controls.rs:171-292`) and the
indexed controls (`controls.rs:296-407`, addressed at `:410-461`). Other LightCraft citations
(`crs.rs`, `crs_masks.rs`, `lightroom_catalog.rs`, `preset_import.rs`, docs)
were spot-checked at the lines that name a field for section 4.12; the
remainder are unchanged from T12 and were not re-read in P6-0.

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
  every merge. That the command accepts an arbitrary partial is confirmed
  (P6-0) by `DevelopSettings::merged` (`crates/develop/src/lib.rs:45-57`),
  which deep-merges the object and re-deserialises the model. The lr2lc plan
  script already supports exactly this pattern: `Step::Update` emits
  `library.select` then the command
  (`crates/lc-writer/src/plan.rs:530-546`). Its command comment names only
  `photo.*` commands (`plan.rs:155-160`), so develop needs its own step (or a
  widened comment) — see section 6.3.
- Unknown keys inside `develop.merge` are **ignored**, not rejected: the
  model is `#[serde(default)]` and does not deny unknown fields
  (`settings.rs:1-3`). A typo in a merge field name is
  therefore silent; the writer must validate names itself (section 6.3).

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

### 4.12 Field-name list for the reader and writer

Source table for every row: `Adobe_imageDevelopSettings`, column `text`
(section 2.1). The "Lightroom key" column is a top-level key inside that Lua
table (section 2.2), written without the `crs:` prefix. Nothing else in the
catalog holds these values.

LightCraft has **two spellings** for the same value:

- `develop.set` takes the slider id from `controls.rs` (122 entries,
  `controls.rs:171-292`), e.g. `detail.sharpenAmount`;
- `develop.merge` takes the JSON path of `DevelopSettings`, which is mostly
  the Rust field name: `detail.sharpen_amount` for the same value
  (`settings.rs:615-626`). The panels that differ are Detail, Optics,
  Geometry, Calibration, HDR, crop, indexed controls and the
  non-slider fields.

`develop.set` accepts numbers only (`develop.rs:177-190`); booleans, strings,
curves, crop geometry (the crop rectangle; `crop.angle` is itself a control,
`controls.rs:292`), masks and spots go through `develop.merge` (section 4.12
marks those "merge only"). A "JSON path" below is **nested objects**, not a
flattened dotted key: `{"detail": {"sharpen_amount": 40}}`, not
`{"detail.sharpen_amount": 40}`. `develop.merge` deep-merges the object
(`crates/develop/src/lib.rs:46-48`) and ignores unknown keys silently
(`settings.rs:1-3`), so a flattened key and a typo behave the same: the
command succeeds and nothing changes. The writer must therefore check every
name against this list before sending.

Band sets used below: eight colour bands in Lightroom order, `Red, Orange,
Yellow, Green, Aqua, Blue, Purple, Magenta` (`crs.rs:60`;
`settings.rs:392`). A row written `<band>` expands once per band.

| LightCraft `develop.set` id | `develop.merge` JSON path | Lightroom key in `text` | Evidence |
| --- | --- | --- | --- |
| `profile.amount` | `profile.amount` | — (no Lightroom counterpart) | `controls.rs:171`; `settings.rs:191-207`; no mapping in `crs.rs:202-460` |
| `wb.temp`, `wb.tint` | `wb.temp`, `wb.tint` (+ `wb.scale`) | `Temperature`, `Tint`; `IncrementalTemperature`, `IncrementalTint`; `AsShotTemperature`, `AsShotTint` | `controls.rs:172-173`; `crs.rs:250-306`; `settings.rs:300-309` |
| — (merge only) | `wb.mode` (`asShot`, `auto`, `daylight`, …) | `WhiteBalance` | `crs.rs:250-260`; `settings.rs:210-267` |
| — (merge only) | `wb.scale: "kelvin"` | written by LightCraft, not read from Lightroom | `crs.rs:294-296`; `settings.rs:278-286` |
| `light.exposure` | `light.exposure` | `Exposure2012`; old `Exposure` | `controls.rs:174`; `crs.rs:210`, `:235` |
| `light.contrast` | `light.contrast` | `Contrast2012`; old `Contrast` | `controls.rs:175`; `crs.rs:211`, `:236` |
| `light.highlights` | `light.highlights` | `Highlights2012`; old `HighlightRecovery` | `controls.rs:176`; `crs.rs:212`, `:237` |
| `light.shadows` | `light.shadows` | `Shadows2012`; old `FillLight` | `controls.rs:177`; `crs.rs:213`, `:238` |
| `light.whites` | `light.whites` | `Whites2012`; old `Brightness` | `controls.rs:178`; `crs.rs:214`, `:239` |
| `light.blacks` | `light.blacks` | `Blacks2012`; old `Shadows` | `controls.rs:179`; `crs.rs:215`, `:240` |
| — (merge only) | `hdr.enabled` (bool) | `HDREditMode` | `crs.rs:217-220`; `settings.rs:1126-1127` |
| `hdr.maxEv` | `hdr.max_ev` | `HDRMaxValue` — confirmed on a catalog copy, 2026-11-04, 11 photos contain it; meaning inferred from the name | `controls.rs:180`; `settings.rs:1126-1137`; no `crs.rs` mapping |
| `hdr.sdrBrightness` | `hdr.sdr_brightness` | `SDRBrightness` — confirmed on a catalog copy, 2026-11-04, 11 photos contain it | `controls.rs:181`; `settings.rs:1131`; no `crs.rs` mapping |
| `hdr.sdrContrast` | `hdr.sdr_contrast` | `SDRContrast` — confirmed on a catalog copy, 2026-11-04, 11 photos contain it | `controls.rs:182`; `settings.rs:1132`; no `crs.rs` mapping |
| `hdr.sdrHighlights` | `hdr.sdr_highlights` | `SDRHighlights` — confirmed on a catalog copy, 2026-11-04, 11 photos contain it | `controls.rs:183`; `settings.rs:1133`; no `crs.rs` mapping |
| `hdr.sdrShadows` | `hdr.sdr_shadows` | `SDRShadows` — confirmed on a catalog copy, 2026-11-04, 11 photos contain it | `controls.rs:184`; `settings.rs:1134`; no `crs.rs` mapping |
| `hdr.sdrWhites` | `hdr.sdr_whites` | `SDRWhites` — confirmed on a catalog copy, 2026-11-04, 11 photos contain it | `controls.rs:185`; `settings.rs:1135`; no `crs.rs` mapping |
| `hdr.sdrClarity` | `hdr.sdr_clarity` | `SDRClarity` — confirmed on a catalog copy, 2026-11-04, 11 photos contain it | `controls.rs:186`; `settings.rs:1136`; no `crs.rs` mapping |
| `curve.highlights` | `curve.highlights` | `ParametricHighlights` | `controls.rs:187`; `crs.rs:324` |
| `curve.lights` | `curve.lights` | `ParametricLights` | `controls.rs:188`; `crs.rs:323` |
| `curve.darks` | `curve.darks` | `ParametricDarks` | `controls.rs:189`; `crs.rs:322` |
| `curve.shadows` | `curve.shadows` | `ParametricShadows` | `controls.rs:190`; `crs.rs:321` |
| `curve.splitShadows` | `curve.split_shadows` | `ParametricShadowSplit` | `controls.rs:191`; `crs.rs:325` |
| `curve.splitMid` | `curve.split_mid` | `ParametricMidtoneSplit` | `controls.rs:192`; `crs.rs:326` |
| `curve.splitHighlights` | `curve.split_highlights` | `ParametricHighlightSplit` | `controls.rs:193`; `crs.rs:327` |
| `curve.refineSaturation` | `curve.refine_saturation` | `CurveRefineSaturation` | `controls.rs:194`; `crs.rs:328` |
| — (merge only) | `curve.master`, `curve.red`, `curve.green`, `curve.blue` | `ToneCurvePV2012`, `ToneCurvePV2012Red`/`Green`/`Blue`; old `ToneCurve` | `crs.rs:329-350`; `settings.rs:332-374` |
| `color.vibrance` | `color.vibrance` | `Vibrance` | `controls.rs:195`; `crs.rs:246` |
| `color.saturation` | `color.saturation` | `Saturation` | `controls.rs:196`; `crs.rs:247` |
| `mixer.<band>.hue` | `mixer.<band>.hue` | `HueAdjustment<Band>` | `controls.rs:197-220`; `crs.rs:313-316` |
| `mixer.<band>.sat` | `mixer.<band>.sat` | `SaturationAdjustment<Band>` | same |
| `mixer.<band>.lum` | `mixer.<band>.lum` | `LuminanceAdjustment<Band>` | same |
| `bw.<band>` | `bw_mix.<band>` | `GrayMixer<Band>` | `controls.rs:221-228`; `crs.rs:317` |
| `grading.shadows.hue` | `grading.shadows.hue` | `SplitToningShadowHue` | `controls.rs:229`; `crs.rs:353` |
| `grading.shadows.sat` | `grading.shadows.sat` | `SplitToningShadowSaturation` | `controls.rs:230`; `crs.rs:354` |
| `grading.shadows.lum` | `grading.shadows.lum` | `ColorGradeShadowLum` | `controls.rs:231`; `crs.rs:355` |
| `grading.midtones.hue` | `grading.midtones.hue` | `ColorGradeMidtoneHue` | `controls.rs:232`; `crs.rs:359` |
| `grading.midtones.sat` | `grading.midtones.sat` | `ColorGradeMidtoneSat` | `controls.rs:233`; `crs.rs:360` |
| `grading.midtones.lum` | `grading.midtones.lum` | `ColorGradeMidtoneLum` | `controls.rs:234`; `crs.rs:361` |
| `grading.highlights.hue` | `grading.highlights.hue` | `SplitToningHighlightHue` | `controls.rs:235`; `crs.rs:356` |
| `grading.highlights.sat` | `grading.highlights.sat` | `SplitToningHighlightSaturation` | `controls.rs:236`; `crs.rs:357` |
| `grading.highlights.lum` | `grading.highlights.lum` | `ColorGradeHighlightLum` | `controls.rs:237`; `crs.rs:358` |
| `grading.global.hue` | `grading.global.hue` | `ColorGradeGlobalHue` | `controls.rs:238`; `crs.rs:362` |
| `grading.global.sat` | `grading.global.sat` | `ColorGradeGlobalSat` | `controls.rs:239`; `crs.rs:363` |
| `grading.global.lum` | `grading.global.lum` | `ColorGradeGlobalLum` | `controls.rs:240`; `crs.rs:364` |
| `grading.blending` | `grading.blending` | `ColorGradeBlending` | `controls.rs:241`; `crs.rs:365` |
| `grading.balance` | `grading.balance` | `SplitToningBalance` | `controls.rs:242`; `crs.rs:366` |
| `effects.texture` | `effects.texture` | `Texture` | `controls.rs:243`; `crs.rs:242` |
| `effects.clarity` | `effects.clarity` | `Clarity2012`; old `Clarity` | `controls.rs:244`; `crs.rs:243-244` |
| `effects.dehaze` | `effects.dehaze` | `Dehaze` | `controls.rs:245`; `crs.rs:245` |
| `vignette.amount` | `vignette.amount` | `PostCropVignetteAmount` | `controls.rs:246`; `crs.rs:381` |
| `vignette.midpoint` | `vignette.midpoint` | `PostCropVignetteMidpoint` | `controls.rs:247`; `crs.rs:382` |
| `vignette.roundness` | `vignette.roundness` | `PostCropVignetteRoundness` | `controls.rs:248`; `crs.rs:383` |
| `vignette.feather` | `vignette.feather` | `PostCropVignetteFeather` | `controls.rs:249`; `crs.rs:384` |
| `vignette.highlights` | `vignette.highlights` | `PostCropVignetteHighlightContrast` | `controls.rs:250`; `crs.rs:385` |
| — (merge only) | `vignette.style` (`highlightPriority`, `colorPriority`, `paintOverlay`) | `PostCropVignetteStyle` | `crs.rs:386-393`; `settings.rs:582-596` |
| `grain.amount` | `grain.amount` | `GrainAmount` | `controls.rs:251`; `crs.rs:394` |
| `grain.size` | `grain.size` | `GrainSize` | `controls.rs:252`; `crs.rs:395` |
| `grain.roughness` | `grain.roughness` | `GrainFrequency` | `controls.rs:253`; `crs.rs:396` |
| — (merge only) | `grain.seed` | — (no Lightroom counterpart) | `settings.rs:599-606` |
| `detail.sharpenAmount` | `detail.sharpen_amount` | `Sharpness` | `controls.rs:255`; `crs.rs:369`; `settings.rs:615-626` |
| `detail.sharpenRadius` | `detail.sharpen_radius` | `SharpenRadius` | `controls.rs:256`; `crs.rs:370` |
| `detail.sharpenDetail` | `detail.sharpen_detail` | `SharpenDetail` | `controls.rs:257`; `crs.rs:371` |
| `detail.sharpenMasking` | `detail.sharpen_masking` | `SharpenEdgeMasking` | `controls.rs:258`; `crs.rs:372` |
| `detail.nrLuminance` | `detail.nr_luminance` | `LuminanceSmoothing` | `controls.rs:259`; `crs.rs:373` |
| `detail.nrDetail` | `detail.nr_detail` | `LuminanceNoiseReductionDetail` | `controls.rs:260`; `crs.rs:374` |
| `detail.nrContrast` | `detail.nr_contrast` | `LuminanceNoiseReductionContrast` | `controls.rs:261`; `crs.rs:375` |
| `detail.nrColor` | `detail.nr_color` | `ColorNoiseReduction` | `controls.rs:262`; `crs.rs:376` |
| `detail.nrColorDetail` | `detail.nr_color_detail` | `ColorNoiseReductionDetail` | `controls.rs:263`; `crs.rs:377` |
| `detail.nrColorSmoothness` | `detail.nr_color_smoothness` | `ColorNoiseReductionSmoothness` | `controls.rs:264`; `crs.rs:378` |
| — (merge only) | `optics.lens_profile` (bool) | `LensProfileEnable` | `crs.rs:398-404`; `settings.rs:649-695` |
| — (merge only) | `optics.remove_ca` (bool) | `AutoLateralCA` | `crs.rs:405-407` |
| `optics.distortion` | `optics.distortion` | `LensManualDistortionAmount` | `controls.rs:265`; `crs.rs:408` |
| `optics.vignetting` | `optics.vignetting` | `VignetteAmount` | `controls.rs:266`; `crs.rs:409` |
| `optics.vignettingMidpoint` | `optics.vignetting_midpoint` | `VignetteMidpoint` | `controls.rs:267`; `crs.rs:410` |
| `optics.profileDistortion` | `optics.profile_distortion` | `LensProfileDistortionScale` is **reported, not mapped** | `controls.rs:268`; `crs.rs:506-507` |
| `optics.profileVignetting` | `optics.profile_vignetting` | `LensProfileVignettingScale` is **reported, not mapped** | `controls.rs:269`; `crs.rs:506-507` |
| `optics.defringePurple` | `optics.defringe_purple_amount` | `DefringePurpleAmount` | `controls.rs:270`; `crs.rs:411` |
| `optics.defringePurpleHueLo` | `optics.defringe_purple_hue_lo` | `DefringePurpleHueLo` | `controls.rs:271`; `crs.rs:412` |
| `optics.defringePurpleHueHi` | `optics.defringe_purple_hue_hi` | `DefringePurpleHueHi` | `controls.rs:272`; `crs.rs:413` |
| `optics.defringeGreen` | `optics.defringe_green_amount` | `DefringeGreenAmount` | `controls.rs:273`; `crs.rs:414` |
| `optics.defringeGreenHueLo` | `optics.defringe_green_hue_lo` | `DefringeGreenHueLo` | `controls.rs:274`; `crs.rs:415` |
| `optics.defringeGreenHueHi` | `optics.defringe_green_hue_hi` | `DefringeGreenHueHi` | `controls.rs:275`; `crs.rs:416` |
| `optics.caRed` | `optics.ca_red` | **(assumption)** no Lightroom key found; no such name in the available copy, 2026-11-04 | `controls.rs:276`; `settings.rs:673-676`; no `crs.rs` mapping |
| `optics.caBlue` | `optics.ca_blue` | **(assumption)** no Lightroom key found; no such name in the available copy, 2026-11-04 | `controls.rs:277`; `settings.rs:673-676`; no `crs.rs` mapping |
| `geometry.vertical` | `geometry.vertical` | `PerspectiveVertical` | `controls.rs:278`; `crs.rs:428` |
| `geometry.horizontal` | `geometry.horizontal` | `PerspectiveHorizontal` | `controls.rs:279`; `crs.rs:429` |
| `geometry.rotate` | `geometry.rotate` | `PerspectiveRotate` | `controls.rs:280`; `crs.rs:430` |
| `geometry.aspect` | `geometry.aspect` | `PerspectiveAspect` | `controls.rs:281`; `crs.rs:431` |
| `geometry.scale` | `geometry.scale` | `PerspectiveScale` | `controls.rs:282`; `crs.rs:432` |
| `geometry.offsetX` | `geometry.offset_x` | `PerspectiveX` | `controls.rs:283`; `crs.rs:433` |
| `geometry.offsetY` | `geometry.offset_y` | `PerspectiveY` | `controls.rs:284`; `crs.rs:434` |
| — (merge only) | `geometry.upright` (`off`, `auto`, `level`, `vertical`, `full`, `guided`) | `PerspectiveUpright` | `crs.rs:435-445`; `settings.rs:763-812` |
| — (merge only) | `geometry.guides`, `geometry.upright_transform` | — (saved matrices are reported, not imported) | `crs.rs:528-536`; `settings.rs:763-812` |
| `calibration.shadowsTint` | `calibration.shadows_tint` | `ShadowTint` | `controls.rs:285`; `crs.rs:419` |
| `calibration.redHue`, `calibration.redSat` | `calibration.red_hue`, `calibration.red_sat` | `RedHue`, `RedSaturation` | `controls.rs:286-287`; `crs.rs:420-421` |
| `calibration.greenHue`, `calibration.greenSat` | `calibration.green_hue`, `calibration.green_sat` | `GreenHue`, `GreenSaturation` | `controls.rs:288-289`; `crs.rs:422-423` |
| `calibration.blueHue`, `calibration.blueSat` | `calibration.blue_hue`, `calibration.blue_sat` | `BlueHue`, `BlueSaturation` | `controls.rs:290-291`; `crs.rs:424-425` |
| `crop.angle` | `crop.geometry.angle` | `CropAngle` | `controls.rs:292`; `crs.rs:447-458`; `settings.rs:815-822` |
| — (merge only) | `crop.geometry.rect` (`x0`, `y0`, `x1`, `y1`) | `CropLeft`, `CropTop`, `CropRight`, `CropBottom`; `HasCrop` | `crs.rs:447-458`; `lightroom_catalog.rs:700-705` |
| — (merge only) | `crop.aspect`, `crop.flip_h`, `crop.flip_v` | **(assumption)** derived from `fileWidth`/`fileHeight` and `orientation`; no direct key verified; no crop or flip key in the available copy, 2026-11-04 | `settings.rs:815-822`; `lightroom_catalog.rs:737-765` |
| — (merge only) | `treatment` (`color`, `bw`) | `ConvertToGrayscale` | `crs.rs:310-312`; `settings.rs:182-188` |
| `pointColor.<i>.hueShift` etc. | `point_colors[i].hue_shift`, `sat_shift`, `lum_shift`, `variance`, `range`, `hue_range`, `sat_range`, `lum_range` | `PointColors` exists (container key confirmed on a catalog copy, 2026-11-04, 11 photos, empty there), but LightCraft's importer does not map it (reported only); **(assumption)** on the catalog key shape | `controls.rs:296-414`; `crs.rs:498-508`; `settings.rs:426-469` |
| `redEye.<i>.pupilSize`, `.darken` | `red_eye[i].pupil_size`, `.darken` | `RedEyeInfo` container key confirmed on a catalog copy, 2026-11-04 (11 photos, empty there); inner field names **(assumption)** not verified | `controls.rs:296-414`; `settings.rs:1075-1087`; `xmp-interop.md:191-192` |
| — (merge only) | `masks[]` | `MaskGroupBasedCorrections`, `GradientBasedCorrections`, `CircularGradientBasedCorrections`, `PaintBasedCorrections` | `crs_masks.rs:23-24`, `:252-296`; `settings.rs:1015-1037` |
| — (merge only) | `spots[]` | `RetouchAreas`, `RetouchInfo` | `crs_masks.rs:298-431`; `settings.rs:1053-1073` |
| — (merge only) | `lens_blur` (`enabled`, `amount`, `focal_distance`, `focal_range`) | `LensBlur` container key confirmed on a catalog copy, 2026-11-04 (11 photos, empty there); inner field names **(assumption)** not verified | `settings.rs:1096-1103`; `xmp-interop.md:191-192` |
| `enhance.denoise` | `enhance.denoise` (+ `enhance.denoise_on`) | **(assumption)** Adobe AI-denoise key not verified; not present in the available copy, 2026-11-04 | `controls.rs:254`; `settings.rs:1105-1117` |
| — (merge only) | `enhance.raw_details`, `enhance.super_resolution` | **(assumption)** no Lightroom key verified; not present in the available copy, 2026-11-04 | `settings.rs:1105-1117` |
| — (merge only) | `disabled_sections` | — (LightCraft UI state, no Lightroom counterpart) | `settings.rs:143` |

Open field-name gaps, checked read-only on a catalog copy in P6-6 (never on
the original catalog; result in the subsection below): the `hdr.*` group,
`red_eye`, `lens_blur`, `enhance.*`, `point_colors`, `optics.ca_red`/`ca_blue`
and the crop flip fields. P6-6 confirmed the `hdr.*` names and the container
keys of `red_eye`, `lens_blur` and `point_colors`; the rest stay unconfirmed.
Until a name is confirmed, the reader treats it as an assumption and reports
it rather than dropping it (section 5.1).

#### P6-6 confirmation (2026-11-04)

Method: a catalog copy provided for this task, 2026-11-04, opened read-only
(`mode=ro`, `immutable=1`); checksums of the source are unchanged before and
after. Catalog schema version (`Adobe_DBVersion`) 1500000, Camera Raw version
18.0.0. Only table, column and field names plus row counts were extracted from
`Adobe_imageDevelopSettings`; no value was read out or recorded. The settings
text is Lightroom output: no generator in this repository writes develop
settings.

Counts: `Adobe_imageDevelopSettings` has 13 rows for 13 images (one row per
image, no orphan rows); 11 rows carry a non-empty `text`, 2 rows are empty.
The table has 32 columns; `text` is declared without a type. The feature
columns `hasPointColor`, `hasLensBlur`, `hasAIMasks`, `hasMasks`,
`isHdrEditMode` and `hasBigData` are 0 in all 13 rows;
`hasDevelopAdjustmentsEx` is 1 for 5 rows, 0 for 2 and −1 for 6;
`hasDevelopAdjustments`, `hasRetouch` and `hasSettings1`/`hasSettings2` are
NULL.

Confirmed top-level field names (number of the 11 non-empty rows):

- `HDREditMode` (11), `HDRMaxValue` (11);
- `SDRBlend`, `SDRBrightness`, `SDRContrast`, `SDRHighlights`, `SDRShadows`,
  `SDRWhites`, `SDRClarity` (11 each);
- `RedEyeInfo` (11), `LensBlur` (11), `PointColors` (11);
- `AutoLateralCA` (11) — automatic CA removal, already documented, distinct
  from the manual `optics.ca_red`/`ca_blue` sliders;
- already-documented names seen as expected: `WhiteBalance` (11),
  `Temperature`, `Tint`, `IncrementalTemperature`, `IncrementalTint` (3 each),
  `ProcessVersion` (11). `AsShotTemperature` and `AsShotTint` (listed in the
  table above, not marked as assumptions) did not occur in this copy.

Structure: the HDR keys are siblings at the top level, not fields of an `HDR`
table; `HDRMaxValue` and the seven `SDR*` keys are separate top-level keys.
`SDRBlend` has no counterpart in LightCraft's `Hdr` model (section 1.2 does
not list it), so it currently would be reported as unmapped. `HDRMaxValue`,
`SDRBrightness` and the other `SDR*` values are numeric.

Limits: `RedEyeInfo`, `LensBlur` and `PointColors` are empty table literals in
every row of this copy (matching the zero feature flags), so the names of
their inner fields could not be observed. No field name containing `Crop`,
`Flip` or `Chromatic` occurs in this copy; the only CA-related name is
`AutoLateralCA` (automatic). Likewise none contains `Denoise`, `Enhance`,
`RawDetails` or `SuperResolution`.

Still assumptions after P6-6: the inner field names of `RedEyeInfo`,
`LensBlur` and `PointColors`; the manual CA sliders (`optics.ca_red`,
`ca_blue`); the crop aspect and flip fields; the Adobe AI-denoise and
enhance fields. Confirming them needs a catalog copy in which those edits are
actually applied (red eye, lens blur, point colour, AI denoise, cropped and
flipped photos); the copy used here leaves every one of them at its default.

The assumption notes in section 5.1 name the same fields; this subsection
states which of those names are confirmed and which remain open.

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
`Step::DevelopMerge` would emit `library.select` + `develop.merge`, and a
batched `develop.set` with `ids` for the rest. `Step::Update` is not suitable
for merge: it injects `ids` into the parameters
(`crates/lc-writer/src/plan.rs:536-544`), while `develop.merge` takes no `ids`
and would silently ignore them. For `develop.set` the injection is correct,
so `Step::Update` could carry it, but its comment and name are limited to
`photo.*` (`plan.rs:155-160`); a dedicated step is clearer and keeps the
merge case from reusing a command shape it does not have.

Because `develop.merge` ignores unknown keys (`settings.rs:1-3`), the writer
must validate every field name against section 4.12 before sending; otherwise
a typo produces a successful-looking run with no effect.

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
- **Known photo, settings changed, opt-in (`--overwrite-develop`):**
  write, and say so in the report.

The mapping file should store the source settings hash next to the existing
identity records, so the third case is distinguishable from the second
without reading the library's develop state. P6-0 verified that the format
has room for it: `PhotoEntry` (`crates/lc-writer/src/state.rs:87-105`)
already carries optional, serde-defaulted fields (`path` at `:98`, `copy_of`
at `:104`, serde attributes at `:97`/`:103`; `KeywordEntry.lightroom_id` at
`:117`, serde attribute at `:116`), and
the loader does not reject unknown fields (`crates/lc-writer/src/state.rs:258`
checks the version, `VERSION = 1` at `:23`). No format bump is needed for an
optional field.

Two limits are worth stating:

- The hash covers the catalog's `text` only. If someone edits the photo in
  LightCraft after lr2lc wrote it, lr2lc cannot see that from the mapping
  file. The live state *can* be read: `photo.inspect` returns the whole
  `Photo`, including `develop` (`crates/engine/src/cmd/query.rs:161-175`), and
  `catalog.query` reports an `edited` boolean (`query.rs:24`). Reading it for
  every photo costs one call per photo; a cheaper compromise is to read
  `photo.inspect` only for photos whose hash changed, before `--overwrite-develop`
  writes. **(assumption:** `photo.inspect` serialises `develop` with the same
  field names as `develop.merge`; the struct has no `rename_all`, so this
  follows from `settings.rs:1-3`, but it was not run.)
- "Only adds" also means the first run must not overwrite a photo the library
  already had with its own edits. The same protection is **planned, not
  implemented**, for the photo fields: only values still at their default may
  be overwritten, any other difference is a reported conflict
  (`docs/research/reconciliation.md:1-10`; the status note at `:3` says
  "Planned, not implemented yet"). Develop settings should follow that planned
  rule in P6-2 rather than invent a different one.

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

## 8. Decisions (settled 2026-11-04)

Seven of the eight questions T12 left open are settled. The phase-6 scope and
these seven decisions were **released by the owner on 2026-11-04**; the project
only carried that release out. The white-balance probe (row 2, section 8.2) is
still open. "User" in the Owner column marks such an owner decision; "Lead"
would mark a decision the team may take under the project rules — no row here
is of that kind. The "decision" column is what P6-1…P6-7 implement.

| # | Question | Facts today (evidence) | Decision | Owner |
| --- | --- | --- | --- | --- |
| 1 | Which route takes the develop settings? | `develop.set` and `develop.merge` exist and cover sliders and a partial settings object (`develop.rs:167-263`; `crates/develop/src/lib.rs:45-57`). `library.importLightroom` always imports a whole catalog (`lightroom_catalog.rs:481`, `:1096`), so it cannot be a develop-only call. | **Own mapper.** lr2lc reads `Adobe_imageDevelopSettings.text` itself and writes through `develop.set` / `develop.merge`. `library.importLightroom` is not used for develop settings. | User (released 2026-11-04) |
| 2 | White balance on raws. | Kelvin handling needs source info from the original file (`crs.rs:250-306`, `:40-57`); lr2lc must not touch originals (principle 2). | **Probe originals read-only only when original paths are released; otherwise do not map `Temperature`/`Tint` and report them as not carried.** The mode (`WhiteBalance`) is always mapped. Enabling the probe is the one residual question (see 8.2). | Open (section 8.2): until answered, only the WB mode is mapped; `Temperature`/`Tint` are reported as not carried |
| 3 | Scope of phase 6. | Masks and spots are the bulk (`crs_masks.rs:23-24`, `:252-431`); sliders, curves and crop are bounded and testable from generated fixtures. | **6a = sliders, tone curves and crop. 6b = masks and spots, and 6b must not delay the import** (P6-7 runs last). | User (released 2026-11-04) |
| 4 | Model shape. | 122 controls would duplicate a table LightCraft owns (`controls.rs:144-166`, `:171-292`); tone curves, the crop rectangle, masks and spots have no slider (`settings.rs:332-374`, `:815-822`, `:1015-1073`), though `crop.angle` is a control (`controls.rs:292`). | **Hybrid:** a generic control-id map for sliders (including `crop.angle`), typed fields for tone curves, the crop rectangle, masks and spots. | User (released 2026-11-04) |
| 5 | Re-sync policy for changed edits. | The plan only adds; the mapping file records the base values (`state.rs:87-105`); `develop.merge` can overwrite. | **Report changed settings by default; overwrite only with explicit `--overwrite-develop`.** | User (released 2026-11-04) |
| 6 | Report size. | `report.json` has one area per kind plus `skipped_by_code` (`crates/lr2lc/src/output.rs:11-40`); per-field entries can explode. | **Capped detail per field and per photo in `report.json`, plus a counter; the readable summary aggregates.** | User (released 2026-11-04) |
| 7 | Fields that are assumed. | These catalog key names are unverified (section 4.12): the whole `hdr.*` SDR group (`hdr.max_ev`, `hdr.sdr_brightness`, `hdr.sdr_contrast`, `hdr.sdr_highlights`, `hdr.sdr_shadows`, `hdr.sdr_whites`, `hdr.sdr_clarity`), red eye, lens blur, AI denoise (`enhance.*`), Point Color (`point_colors`), the manual CA sliders (`optics.ca_red`, `optics.ca_blue`) and the crop flip fields. Some of these are also not mapped by LightCraft's importer (`xmp-interop.md:191-192`). | **Confirm the names read-only on a catalog copy in P6-6 (including Point Color and CA) before implementing them; until then they stay assumptions and are reported, never silently dropped.** | User (released 2026-11-04); the names are confirmed read-only on a catalog copy in P6-6 |
| 8 | Camera profile handling. | The profile name is metadata only and carries no rendering data (`crs.rs:522-526`); LightCraft does not apply Adobe profiles. | **Do not rebuild profiles.** Report the name and aggregate by profile name; a profile/LUT converter is a separate research project. | User (released 2026-11-04) |

### 8.1 Recorded as released by the owner on 2026-11-04

1. Own mapper: read the catalog, write through `develop.set` / `develop.merge`;
   no `library.importLightroom` for develop settings.
2. Scope: 6a = sliders, tone curves, crop; 6b = masks and spots, without
   delaying the import.
3. Hybrid model: control-id map plus typed curve, crop, masks and spots.
4. Re-sync: report only; overwrite only with `--overwrite-develop`.
5. Report: capped detail per field in `report.json` plus counters; the
   readable summary aggregates.
6. Unconfirmed field names: confirm them read-only on a catalog copy in P6-6
   before implementing them; until then they stay assumptions and are reported.
7. Camera profiles: report only; no converter in this project.

The one question left for the owner is in 8.2.

### 8.2 Open user question

**May lr2lc read the original photo files (read-only) to determine the
white-balance scale?** Yes → Kelvin/tint are mapped for raws with the correct
scale. No → only the WB mode is mapped and `Temperature`/`Tint` are reported
as not carried. Nothing is written to the originals either way. Until the
answer, P6-4 implements the "No" default and keeps the probe behind a flag.

---

## 9. Implementation plan P6-1…P6-7

The order follows the approved breakdown. P6-6 is independent of the code
work and can run in parallel; P6-7 comes last because it is 6b.

| Step | Content | Depends on | First testable piece |
| --- | --- | --- | --- |
| P6-1 | Reader: read `Adobe_imageDevelopSettings.text`, parse the non-executing Lua subset, map known keys, count and report unknown keys; neutral `Develop` in `crates/model` | generator table (section 7.1), P6-6 for the assumed names only | A unit test that parses a generated `text` value and maps one slider; then the `develop_all_sliders` fixture. |
| P6-2 | Writer: `develop.set` for sliders, `library.select` + `develop.merge` for structured parts, only with `--apply`; re-sync report, `--overwrite-develop`; capped report area | P6-1 model; `state.rs` hash field; fake port | A dry-run plan test against `fake_lightcraft.rs`: expected `library.select` + `develop.set` / `develop.merge` steps, no command without `--apply`. |
| P6-3 | Typed 6a parts: tone curves and crop | P6-1 types; P6-2 merge step | Curve identity and channel-completeness tests; crop frame tests per orientation (section 7.2 items 4-5). |
| P6-4 | White balance: read-only probe behind a flag, only when original paths are released; otherwise report | P6-1; answer to 8.2 for switching the probe on | Mode mapping without a probe first, then the "not carried" report path. A generated placeholder only exercises the undecidable path; the probe itself is checked read-only on a released original. |
| P6-5 | Camera profile: read the name, report it, aggregate per profile name | P6-1 reader; P6-2 report area | A generated fixture with two profile names and a count assertion; no pixel or profile data involved. |
| P6-6 | Confirm the assumed field names read-only on a catalog copy (HDR SDR, red eye, lens blur, AI denoise, Point Color, CA) | none (parallel to P6-1) | A read-only query that lists table and column names and counts rows; never values, never the original catalog. |
| P6-7 | 6b: masks and spots through `develop.merge`, without delaying the import | P6-2 merge step; P6-3 model types | Mask and spot JSON against the fake port; a timing check that the 6a path does not wait for 6b. |

Dependency rule: P6-1 can start on the confirmed keys before P6-6 finishes;
only the assumed keys wait for it. P6-2 can be built against the P6-1 types
and the fake port without LightCraft running. P6-3 is the first structured
step and proves the merge route end to end; P6-7 reuses that route and must
stay optional at runtime so a catalog full of masks cannot slow the 6a
import.

---

## 10. Sources

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
`CLAUDE.md`) have no public URL and refer to commit `04ee6f5` in the local
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
| lr2lc repository | `04ee6f5` | not published; local repository only | – |

What was read from these sources:

- LightCraft 0.14.0, `crates/engine/src/crs.rs`, `crs_masks.rs`,
  `lightroom_catalog.rs`, `preset_import.rs`, `cmd/develop.rs`, `cmd/query.rs`.
- LightCraft 0.14.0, `crates/develop/src/settings.rs`, `controls.rs`.
- LightCraft 0.14.0, `docs/lightroom-catalog-import.md`,
  `docs/xmp-interop.md`, `docs/control-protocol.md`,
  `docs/process-versions.md`.
- lr2lc `main` at `04ee6f5`, `docs/research/lightcraft-develop.md`,
  `lightcraft-library.md`, `lightcraft-import.md`, `lrcat-schema-lrc15.md`,
  `docs/research/reconciliation.md`, `CLAUDE.md`,
  `crates/model/src/lib.rs`, `crates/model/src/skip.rs`,
  `crates/lc-writer/src/plan.rs`, `crates/lc-writer/src/state.rs`,
  `crates/lr2lc/src/output.rs`, `crates/lc-test-support/src/fake_lightcraft.rs`,
  `tests/fixtures/README.md`, `tests/fixtures/generator/`.
- lrcat-extractor 0.6.0, `doc/lrcat_format.md` (read only).

No other public source is cited in this document. In particular, no public
Adobe schema or slider-range documentation was found (§2.1, §3 range caveat);
those parts are marked as assumptions and are not upgraded by this section.

---

## Open questions

1. **Original paths for white balance (user).** May lr2lc open the original
   photo files read-only to decide the Kelvin scale? Until the answer, P6-4
   ships the "No" path and keeps the probe behind a flag.
2. **Assumed catalog keys (technical, P6-6).** Are the Lightroom names for the
   HDR SDR group, red eye, lens blur, AI denoise, Point Color and the manual
   CA sliders the ones listed in section 4.12? Only a read-only look at a
   catalog copy can answer this; no values are needed.
3. **`photo.inspect` shape (technical, P6-2).** Does the serialised `develop`
   object use exactly the merge paths of section 4.12? The struct has no
   `rename_all`, so it should, but this was not run.
4. **Detecting a LightCraft-side edit (technical, P6-2).** Is `edited` updated
   by every develop command, and is it exposed by `photo.inspect`? If yes, the
   overwrite path can warn instead of relying only on the mapping hash.
5. **Slider ranges (assumption).** Do Lightroom and LightCraft use the same
   range for every 1:1 slider? No public Lightroom range table was found;
   clamping is the safety net.
6. **Rendering parity (out of scope).** Structural parity is testable;
   pixel parity needs a reference renderer and real exports, which cannot
   enter the repository (principle 9).

## Recommendation

Keep the approved scope and order: build P6-1 (reader) and P6-2 (writer,
dry-run first) against generated fixtures and the fake port; add P6-3
(curves, crop) as the first structured end-to-end proof. Run P6-6 in parallel
so the assumed field names are settled before they are implemented, and do
not start P6-7 until 6a is stable and the import timing is measured. Ask the
owner only the white-balance path question; the other seven decisions in
section 8 were released by the owner on 2026-11-04. If the WB probe is refused, P6-4 still
ships the mode mapping and the report entry, so no photo is left without a
reason.
