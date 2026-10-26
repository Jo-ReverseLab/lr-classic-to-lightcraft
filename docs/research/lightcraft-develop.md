# LightCraft develop settings: overview for phase 6

References are relative to the LightCraft source root (0.14.0). Statements marked *(assumption)* are not verified in code.

## Controls (sliders)

- 122 numeric controls are declared in one table, `CONTROLS`, in `crates/develop/src/controls.rs:144` (entries `controls.rs:171-292`, counted by `grep`). Each has `id`, label, section, `min`, `max`, `default`, step, decimals (`controls.rs:79-91`). Sections: Light, Tone Curve, Color, Color Mixer, B&W Mixer, Color Grading, Effects, Vignette, Grain, Detail, Optics, Geometry, Profile, Calibration, Point Color, Red Eye (`controls.rs:12-29`).
- Examples with ranges: `wb.temp` 2000..50000 (default 6500), `wb.tint` -150..150, `light.exposure` -5..5, `light.contrast` -100..100, `profile.amount` 0..200, `crop.angle` -45..45 (`controls.rs:171-175`, `:292`).
- Point Color samples are *indexed* controls, `pointColor.<i>.hueShift`, up to 8 (`controls.rs:5-6`; `crates/develop/src/settings.rs:419`).
- Values are clamped to the range on write; non-finite values fall back to the default (`controls.rs:93-95`, `:152-157`).
- Not slider-addressable: tone curves (point lists), masks, spots, red eye, crop rectangle, lens blur, enhance, Hdr (fields in `crates/develop/src/settings.rs:104-144`).

## Data model

- `DevelopSettings` (`crates/develop/src/settings.rs:104-144`, `serde(default)`): `version` (schema, `SCHEMA_VERSION = 1`, `settings.rs:9`), `process`, `profile`, `treatment`, `wb`, `light`, `hdr`, `curve`, `color`, `mixer`, `point_colors`, `bw_mix`, `grading`, `effects`, `vignette`, `grain`, `detail`, `optics`, `geometry`, `crop`, `orientation`, `masks`, `spots`, `red_eye`, `lens_blur`, `enhance`, `calibration`, `disabled_sections`.
- Examples: `Light` = exposure, contrast, highlights, shadows, whites, blacks (`settings.rs:321-328`); `WhiteBalance` = mode (as-shot/auto/…/custom), `temp`, `tint`, `scale` (`settings.rs:300-309`); `Detail` = sharpen and noise-reduction fields (`settings.rs:615-626`); `Crop` = geometry (normalised rect + angle), aspect, flips (`settings.rs:815-821`).
- `process` is the rendering process (V1, V2 latest); stored as a plain number, missing = V1 (`settings.rs:104-111`; `docs/process-versions.md:1-12`).
- Storage: `Photo.develop: Arc<DevelopSettings>` plus `edited` timestamp, `history`, `versions` (`crates/catalog/src/model.rs:248-254`). Changes are `Op::SetDevelop {id, settings, label, edited}` in the journal (`crates/catalog/src/lib.rs:97-102`), i.e. inside `catalog.snap`/`catalog.log` (see `lightcraft-library.md`). Settings may also be exchanged as XMP sidecar (`crs:` fields plus LightCraft's own, `docs/xmp-interop.md`).

## Setting via official commands

All through the engine registry (CLI `lightcraft-cli run`, MCP; see `lightcraft-import.md`):
- `develop.controls {section?}` lists every slider with range, default and current value (`crates/engine/src/cmd/query.rs:190`); `develop.get {id?}` returns the full settings JSON (`crates/engine/src/cmd/query.rs:177`).
- `develop.set {control, value | values:{id:number}, ids?}`: numeric controls only; unknown id is an error (`crates/engine/src/cmd/develop.rs:168-192`). White-balance writes are normalised against the photo's as-shot data (`crates/engine/src/cmd/develop.rs:197-200`), so a raw file must be probed (file present) for Kelvin values to be interpreted correctly *(inference)*.
- `develop.merge` (`crates/engine/src/cmd/develop.rs:256`), `develop.curve` (`:509`), `develop.treatment` (`:475`), `develop.profile` (`:482`), `develop.reset`/`resetControl`/`resetSection` (`:264`, `:346`, `:332`), `develop.auto` (`:387`), `preset.apply|create|import|export` (`crates/engine/src/cmd/develop.rs:1010-1070`, `preset_files.rs`).
- Structured parts (masks, spots) are set through a partial-settings JSON, the format the importer builds (`crates/engine/src/crs.rs:202`, `put(o, "masks", …)` at `crs.rs:472-495`); which public command accepts an arbitrary partial was not confirmed (`develop.merge` is the likely one, *(assumption)*).

## What `library.importLightroom` takes from Lightroom settings

Flow: the catalog's develop-settings text (a Lua table) is parsed (`crates/engine/src/lightroom_catalog.rs:697-699`), Lightroom's `-999999` "deferred" sentinels are stripped and reported (`:698`, `:708-710`), a missing `HasCrop` is inferred from crop fields (`:700-705`), then the **same XMP/preset mapper** converts `crs:` names into a partial `DevelopSettings` (`crs::to_partial_report`, `crs.rs:472`) which is applied over defaults (`lightroom_catalog.rs:994-1010`, `apply_partial`). Photo `edited` is set; existing edited photos keep their edits unless `updateExisting=true` (`:944`, `:956-957`; `docs/lightroom-catalog-import.md:71-72`). Catalog crop is corrected for orientation (`lightroom_catalog.rs:745`, `:717`). Raws get Lightroom's sharpen/NR defaults at import time, not via the mapper (`settings.rs:629-631`).

Mapped (name → field, mostly 1:1, no unit conversion) in `to_partial` (`crs.rs:202-465`). Path shorthand from here on: `crs.rs` = `crates/engine/src/crs.rs`, `crs_masks.rs` = `crates/engine/src/crs_masks.rs`, `lightroom_catalog.rs` = `crates/engine/src/lightroom_catalog.rs`, `controls.rs` = `crates/develop/src/controls.rs`, `settings.rs` = `crates/develop/src/settings.rs`:
- Light: `Exposure2012, Contrast2012, Highlights2012, Shadows2012, Whites2012, Blacks2012` (`crs.rs:210-215`); older process fields converted by fixed formulas, e.g. `Contrast-25`, `Brightness→whites (v-50)*0.6` (`crs.rs:230-236`) — only when no `*2012` key exists (`crs.rs:222`).
- Presence/colour: `Texture, Clarity2012, Dehaze, Vibrance, Saturation` (`crs.rs:240-246`); HDR edit mode (`:217`).
- White balance: named modes; Kelvin read as absolute or as a relative scale depending on the file type (`crs.rs:248-294`); custom Kelvin on raws developed relative to as-shot is converted by a mired shift when `AsShotTemperature` is present, otherwise the photo stays As Shot and is reported (`crs.rs:264-290`; `docs/lightroom-catalog-import.md:108-117`).
- B&W treatment, HSL mixer (hue/sat/lum per band), B&W mix (`crs.rs:298-307`).
- Tone curve: parametric regions + split points, refine saturation, point curves (master and per-channel only when all channels exist) (`crs.rs:310-345`, `crs.rs:841` test).
- Color grading (shadows/midtones/highlights/global/blending/balance) (`crs.rs:352-366`).
- Detail: sharpening, noise reduction (`crs.rs:368-378`); post-crop vignette, grain (`:380-396`); manual optics (distortion, vignetting, defringe) and the lens-profile switch; lens profiles themselves are LightCraft's (`crs.rs:398-418`); calibration (`:420-426`); geometry/Upright mode (`:428-444`); crop (`:447-465`).
- Masks: `MaskGroupBasedCorrections`, `GradientBasedCorrections`, `CircularGradientBasedCorrections`, `PaintBasedCorrections` become masks (brush, linear, radial, colour range, luminance range, subject, sky) (`crates/engine/src/crs_masks.rs:23-24`, `:196-250`); spots from `RetouchAreas`/`RetouchInfo` (`crs_masks.rs:301`).

Dropped or approximate (each is reported in `unmapped`, `crs.rs:466-520`; archived in `Interop/lightroom-import-N.lca` when saving succeeds, `docs/lightroom-catalog-import.md:76-83`):
- `ProcessVersion` is not mapped: imported photos render with LightCraft's latest process (`docs/lightroom-catalog-import.md:103-105`).
- Camera profiles, Adobe AI models (denoise, AI masks other than subject/sky: reported as `Mask: AI mask (n)`, `crs_masks.rs:221-225`), some masking/retouch fields, process-version algorithms: not reproduced (`docs/lightroom-catalog-import.md:100-105`). Camera profile name is kept as metadata only (`crs.rs:522-526`).
- Colour-range masks of unsupported kinds ("colour range" error), other `Mask/*` kinds (`crs_masks.rs:240-246`).
- Point Color: not mapped by `to_partial` in the lines read; non-empty `PointColors` is reported as unmapped (test `crs.rs:880-882`). The test shows it listed in `unmapped`.
- Look/preset bookkeeping fields (`Enable*`, digests) are ignored silently (`crs.rs:63-100`, `:498-512`).
- History and snapshots are archived, not imported as native history (`docs/lightroom-catalog-import.md:105-107`).
- Rendering is stated to be "approximate": slider values map but the render differs from Lightroom (`docs/lightroom-catalog-import.md:100-103`). Fidelity against Lightroom exports is tracked in LightCraft's own docs (`docs/target-app-parity.md`, not read in detail).

## Open questions

1. Which public command applies a full partial-settings JSON with masks (`develop.merge`?) — needed if lr2lc ever maps settings itself. Not confirmed.
2. Exact JSON field names (serde renames) of `DevelopSettings` in `catalog.snap`; only the Rust structure was read.
3. How accurate is the render vs. Lightroom for each slider (tone curve, HSL, grain)? Only LightCraft's claims were seen; not measured.
4. Is the catalog's develop-settings text complete for sliders (e.g. Point Color, Look, `Hdr`)? Not checked against real catalogs.
5. Does the importer skip masks for photos with unsupported AI components entirely or just those components? Code suggests per-component (`crs_masks.rs:216-225`), not tested.

## Recommendation

Develop settings are already handled by the built-in importer using one mapper for the sliders listed above, masks, spots and crop; what is lost is profiles, AI masks, process version and native history. Rebuilding that in lr2lc would duplicate a large, tested mapping (`crs.rs` ~500 lines of mapping plus `crs_masks.rs`). Recommendation: do **not** write a second slider mapper in lr2lc. If develop settings are in scope, call `library.importLightroom` (it also needs the files for correct white balance on raws), and let lr2lc add value around it: dry-run report, unmapped-field statistics (from `library.inspectLightroom` and the import report), and checks. If lr2lc keeps its own pipeline, `develop.set` for the numeric controls is sufficient for simple cases, but curves/masks would need the partial-settings route (Open question 1).
