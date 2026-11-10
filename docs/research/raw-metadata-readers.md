# Reading RAW metadata for the white-balance probe

Status: decision document, 2026-11-10. Nothing here is implemented; no
dependency was added.

Scope: which component, if any, lr2lc should use to read technical RAW metadata
(colour matrix, `AsShotNeutral`, as-shot white balance) from the originals, so
that a read-only white-balance probe can carry Lightroom's `Temperature` and
`Tint` into LightCraft. The decision covers DNG/TIFF and Canon CR3, because the
catalogs this project targets contain CR3 files.

Evidence conventions: `lightcraft:PATH:LINE` is the LightCraft source at
`bb08a69` (0.14.0); `PATH:LINE` is this repository at `600765f`;
`crate-NAME-VERSION/PATH:LINE` is that crate's published tarball. crates.io
metadata (version, date, licence, downloads) was read from
`https://crates.io/api/v1/crates/NAME` on 2026-11-10. Unestablished statements
are marked **(assumption)**.

## 1. What LightCraft needs — the yardstick

### 1.1 Two white-balance scales

LightCraft interprets the same two numbers `wb.temp` and `wb.tint` in two
different ways, decided per photo:

- **Absolute (Kelvin) scale** — `temp` is the scene illuminant in Kelvin, as
  Lightroom writes it for raws. The engine marks such values with
  `wb.scale: "kelvin"` (`lightcraft:crates/engine/src/crs.rs:290-296`;
  `lightcraft:crates/develop/src/settings.rs:279-286`).
- **Relative scale** — the image is assumed balanced at 6500 K / tint 0 ("as
  shot"), and `temp`/`tint` are a mired shift around that point
  (`lightcraft:crates/develop/src/settings.rs:294-297`;
  `lightcraft:crates/engine/src/crs.rs:22-31`).

Which scale applies is decided from the photo's **format and camera**, never
from the file's contents (`lightcraft:crates/catalog/src/model.rs:331-344`,
mirrored by `Target::for_file`, `lightcraft:crates/engine/src/crs.rs:40-57`).
A file is on the absolute scale when it carries colour matrices of its own
(DNG), or its camera has **measured matrices** in LightCraft's table
(`lightcraft:crates/raw/src/spectral_table.rs`, 52 entries), or its camera's
WB **presets** give a Kelvin curve
(`lightcraft:crates/raw/src/kelvin_presets.rs:44-55`). Everything else —
rendered files and raws whose camera is in neither table — is relative
(`lightcraft:crates/catalog/src/model.rs:342-344`).

For the CR3 bodies in question the measured table contains `EOS R`, `EOS R5`,
`EOS R5m2`, `EOS R6`, `EOS R6m2`, `EOS R10` and `EOS RP`
(`lightcraft:crates/raw/src/spectral_table.rs:107-148`); `EOS R3`, `EOS R7`,
`EOS R8`, `EOS R50`, `EOS R100`, `EOS R5 C` and `EOS R6 Mark III` are absent
(grep over the same file, 2026-11-10). CR3 is in the list of formats whose look
is a camera fit rather than a raw of its own matrices
(`lightcraft:crates/catalog/src/model.rs:414-416`).

### 1.2 What the probe is for

`crs.rs` converts an absolute Lightroom Kelvin value to the relative scale with
a mired shift `r = (1e6/as_shot − 1e6/t)/0.8`, `tint = tint − as_shot_tint`,
`value = rel_to_kelvin(r)` (`lightcraft:crates/engine/src/crs.rs:268-276`,
`:176-178`). It needs the **as-shot temperature of the file**. Without that
reference the value is not written and the photo stays As Shot, because reading
an absolute Kelvin on the relative scale is a large colour shift — the source
comment quantifies it as ΔE 22 against Lightroom's own export
(`lightcraft:crates/engine/src/crs.rs:297-305`).

So a probe must produce exactly one thing: **the as-shot temperature and tint of
this file, on the scale Lightroom reports**. Matrices, neutral and multipliers
are only ingredients.

### 1.3 Where as-shot data comes from today

- The catalog does not carry it: Lightroom's Lua develop table stores
  `Temperature`/`Tint` absolutely and no as-shot pair; the assumed
  `AsShotTemperature`/`AsShotTint` names were never observed
  (`docs/research/develop-mapping.md:331`, `:537`). The reader maps both to
  "undecidable" for every raw (`crates/lr-reader/src/develop.rs:1009-1015`,
  `:1080-1086`).
- LightCraft computes it from the file: `WbScaleOf::new` takes the colour data
  and WB multipliers (`lightcraft:crates/engine/src/files.rs:581-612`),
  `as_shot()` rounds the result (`:641-655`), via
  `spectral::as_shot_white_through` (multipliers → xy → CCT:
  `lightcraft:crates/raw/src/spectral.rs:106-120`,
  `lightcraft:crates/raw/src/color.rs:108-154`).
- On import LightCraft probes each original and stores the result
  (`lightcraft:crates/engine/src/import.rs:1471`, `:1595`); its own importer
  documentation says the camera "is read from the file, not from the Lightroom
  catalog" (`lightcraft:docs/lightroom-catalog-import.md:115-117`).
- The photo header reports `(6500, 0)` for every relative raw and the stored
  pair otherwise (`lightcraft:crates/engine/src/media.rs:1229-1247`).

### 1.4 Consequence: the probe is needed for a subset only

- **Covered camera (absolute scale):** Lightroom's Kelvin is written as it
  stands, with `wb.scale: "kelvin"`; no RAW metadata is needed from lr2lc
  (`lightcraft:crates/engine/src/cmd/develop.rs:197-203`,
  `lightcraft:crates/pipeline/src/local.rs:148-161`).
- **Uncovered camera (relative scale):** the as-shot reference is required. A
  CR3 holds no colour matrix — only vendor WB multipliers
  (`lightcraft:crates/catalog/src/model.rs:414-416`) — so turning multipliers
  into a temperature needs a camera colour model that LightCraft itself does
  not have for those bodies. A probe must bring its own. This is the crux: the
  unsolved part is colour science, not the file format.

### 1.5 The app can answer the scale question directly

`photo.inspect` serialises the photo record including its develop settings
(`lightcraft:crates/engine/src/cmd/query.rs:161-166`,
`lightcraft:crates/catalog/src/model.rs:248`), and `wb.scale` appears only when
it is Kelvin (`lightcraft:crates/develop/src/settings.rs:305-308`); import sets
that scale on absolute raws via `camera_defaults`
(`lightcraft:crates/catalog/src/model.rs:441-451`). After `library.import`, a
`photo.inspect` therefore tells lr2lc whether a probe is needed at all — with no
camera table in lr2lc and without opening the file. Not implemented; see Open
questions.

## 2. What the files actually contain

### 2.1 DNG / TIFF

- `AsShotNeutral` (50728 / 0xC628), `AsShotWhiteXY` (50729 / 0xC629),
  `ColorMatrix1/2` (50721/50722) and `ForwardMatrix1/2` are read in LightCraft's
  DNG path (`lightcraft:crates/tiff/src/tags.rs:121-128`,
  `lightcraft:crates/raw/src/dng.rs:108-126`).
- A neutral plus a matrix gives the as-shot xy and then the CCT
  (`lightcraft:crates/raw/src/color.rs:108-154`), so for DNG the requirement is
  satisfiable from the file alone.

### 2.2 CR3

CR3 is ISO/IEC 14496-12 (BMFF): a `ftyp` box with brand `crx `, a Canon `uuid`
box with the `CMT1` (TIFF/EXIF), `CMT2` (Exif IFD), `CMT3` (maker note) and
`CMT4` (GPS) sub-boxes, plus `moov`/track boxes with the picture data. The
reference reader is `lightcraft:crates/meta/src/cr3.rs` for the container and
`lightcraft:crates/raw/src/vendor/cr3.rs` for the maker note.

Inside `CMT3`, tag `0x4001` (`ColorData`) holds the as-shot WB multipliers; the
word offset depends on a version number derived from the camera. Three
independent implementations agree on the table:

- rawler: `ColorData = 0x4001` (`crate-rawler-0.7.2/src/decoders/cr3.rs:778`),
  parsed with a per-version offset table shared with its CR2 path
  (`crate-rawler-0.7.2/src/decoders/cr2/colordata.rs:34-95`).
- LibRaw 0.21.3: a `CR3_ColorData(offset)` macro
  (`crate-rsraw-sys-0.1.1/LibRaw/src/metadata/canon.cpp:759-766`) called with
  `0x0047` for versions 16–19, `0x0055` for 32/33 (EOS R5, R6) and `0x0069` for
  34/48 (EOS R3, R7, R10) (`:1239-1252`).
- libopenraw: a fixed word index per `ColourData` version
  (`crate-libopenraw-0.4.0-alpha.13/src/canon/colour.rs:165-200`), derived from
  ExifTool's tag tables (`:24`).

The same region carries Canon's **own WB preset table with a colour temperature
per preset**; LibRaw reads rows of "(tint, as shot R, as shot B, CCT)"
(`crate-rsraw-sys-0.1.1/LibRaw/src/metadata/canon.cpp:670-688`, `:761-763`) — a
per-file, vendor-provided temperature, no camera colour model needed. Whether
it equals the number Lightroom shows as as-shot is **(assumption)**, and the
preset index (Canon `wbi`) must be resolved too.

What a CR3 does **not** contain is a camera colour matrix: every candidate that
offers one takes it from a bundled database (section 3).

## 3. Candidates

All crates below were inspected in their published source, not from
documentation alone. "CR3" means: the crate has code that parses a CR3
container and reaches the image or its maker note — verified by file and line.

### 3.1 rawler 0.8.0 (and 0.7.2)

- crates.io: 0.8.0 (2026-08-30, 219 159 downloads, **LGPL-2.1**, MSRV **1.89**);
  0.7.2 (2026-02-26, MSRV **1.88**, `Cargo.toml:14`, `:35`). The project is on
  1.88.0, so only 0.7.2 resolves without raising it.
- CR3: `src/decoders/cr3.rs`, `ColorData = 0x4001`
  (`crate-rawler-0.7.2/src/decoders/cr3.rs:778`), as-shot multipliers decoded
  and normalised (`:522-526`, `:688-694`).
- WB exposure: `RawImage.wb_coeffs` and `RawImage.camera`
  (`crate-rawler-0.7.2/src/rawimage.rs:198`, `:216`). The **metadata-only** API
  returns `RawMetadata` **without any WB field**
  (`crate-rawler-0.7.2/src/decoders/mod.rs:241-247`; 0.8.0 likewise,
  `crate-rawler-0.8.0/src/decoders/mod.rs:237-244`, the value staying in the
  private `Cr3Metadata`, `crate-rawler-0.8.0/src/decoders/cr3.rs:567`), so WB
  needs `raw_image()` — a full pixel decode.
- Matrices come from bundled tables (`data/cameras/canon/r5.toml:8-10`), not
  from the file. An unknown camera is rejected before any metadata is read
  (`src/decoders/mod.rs:848`).
- Footprint: 24 direct dependencies incl. `image`, `jxl-oxide`, `rayon`,
  `clap`, `uuid` (`Cargo.toml:70-150`), 57 `unsafe` occurrences, 24 MB.
- Assessment: solves CR3 and supplies matrices, but is LGPL-2.1, needs 1.89 or a
  pin, exposes WB only through a full decode, and is far larger than "small and
  readable" allows.

### 3.2 rawloader 0.37.2 — not a CR3 reader

- crates.io: 0.37.2, 2026-08-15, 334 604 downloads, LGPL-2.1, no MSRV.
- No CR3 module in the decoder list
  (`crate-rawloader-0.37.2/src/decoders/mod.rs:60-80`) and no `cr3` occurrence
  in `src/` (case-insensitive grep). rawler is its successor (same decoder
  layout, same licence, newer and faster); rawloader is not a candidate.
- Assessment: rejected — no CR3.

### 3.3 LibRaw through libraw-rs / libraw-rs-sys / rsraw / rsraw-sys

- LibRaw is C++; the Rust crates are thin wrappers that **compile the bundled
  LibRaw with `cc`**: `libraw-rs-sys` 0.0.4+libraw-0.20.1 (2021-02-02, LibRaw
  **0.20.1**, `crate-libraw-rs-sys/libraw/libraw/libraw_version.h:23-25`) and
  `rsraw-sys` 0.1.1 (2026-03-21, LibRaw **0.21.3**,
  `crate-rsraw-sys-0.1.1/LibRaw/libraw/libraw_version.h:23-25`).
- CR3: both bundle `src/metadata/cr3_parser.cpp` and decode Canon `ColorData`
  (`crate-rsraw-sys-0.1.1/LibRaw/src/metadata/canon.cpp:759-766`, `:1239-1252`);
  multipliers land in `cam_mul`, matrices in `cam_xyz`
  (`crate-libraw-rs-sys/src/bindings.rs:10028-10043`).
- WB exposure from Rust: the **high-level** crates do not expose colour data.
  `libraw-rs` 0.0.4 offers `Processor`, `RawImage` and `Sizes` only, `rsraw`
  0.1.1 has no `cam_mul`; reaching the values needs the `*-sys` crates and
  `unsafe` FFI against a C struct — `unsafe` is not allowed in this project.
- Licence: LibRaw is **LGPL-2.1 or CDDL-1.0** (both files ship in the tree,
  `crate-libraw-rs-sys/libraw/LICENSE.LGPL`, `LICENSE.CDDL`); the wrappers are
  MIT/Apache-2.0. Static linking into an MIT/Apache binary raises the usual
  LGPL question.
- Platform: needs a C++ toolchain and `libclang` (bindgen) on all three
  platforms, contrary to "runs everywhere" without a toolchain. Assessment: the
  most complete decoder, but the Rust surface lacks the needed value, the
  licence is copyleft and the build is heavy.

### 3.4 libopenraw 0.4.0-alpha.13

- crates.io: latest stable 0.1.2 (2022-12-29, MPL-2.0); newest
  `0.4.0-alpha.13` (2026-09-18), **LGPL-3.0-or-later**, MSRV 1.85. The repository
  is on GitLab, outside the sources this review may use; everything below comes
  from the published crate source.
- CR3: `src/canon/cr3.rs`, `src/canon/colour.rs`, `src/canon/matrices.rs`,
  `src/mp4/parse/craw.rs` are all present.
- WB: as-shot multipliers per `ColourData` version
  (`src/canon/colour.rs:165-200`); the as-shot **white point in xy** is set on
  the DNG path only (`src/dng.rs:445`, `src/rawimage.rs:332-338`), not for CR3.
- Footprint: 160 `unsafe` occurrences, `libc` among the dependencies, alpha
  version, no metadata-only entry point (`src/lib.rs` exports `extensions()`
  only). Assessment: the closest to the requirement (it already computes an
  as-shot white point), but alpha, LGPL-3.0 and only after a full decode.

### 3.5 flashraw 0.1.1

- crates.io: 0.1.1, 2026-10-25, **532 downloads**, MIT OR Apache-2.0, MSRV 1.85;
  three crates plus a data crate.
- CR3: `flashraw-formats-0.1.1/src/canon/cr3/mod.rs`, as-shot multipliers
  `mn.as_shot_wb()` → `WhiteBalance::from_multipliers` (`:99-100`), matrices
  from the bundled database (`:108-109`). Maker note: tag `0x4001` →
  `ColorData::parse` (`crate-flashraw-formats-0.1.1/src/canon/makernote.rs:311`),
  as-shot multipliers `:349-360`.
- WB exposure: `WhiteBalance { coeffs: [f32; 4] }`, green-normalised
  (`crate-flashraw-core-0.1.1/src/types.rs:470-484`). Matrices:
  `flashraw-camera-db` 0.1.1 (2026-10-25, 1.19 MB `cameras.json`, 814 matrix
  entries incl. `EOS R3`, `EOS R5`, `EOS R7`, `EOS R50`).
- Caveat: no CCT is computed, so lr2lc would still implement the
  multipliers → xy → CCT inversion. For bodies that write no DNG the database
  keeps matrices "as stored" (`flashraw-camera-db-0.1.1/README.md`), i.e. from an
  external source, not measured.
- Footprint: 13 `unsafe` occurrences in `flashraw-core`; a young single-vendor
  project whose Windows behaviour is untested here. Assessment: the only
  permissively licensed pure-Rust CR3 reader with matrices, but pre-1.0 and
  without the value the requirement is about.

### 3.6 EXIF-only crates: kamadak-exif, nom-exif, little_exif

- **kamadak-exif 0.6.2** (2026-11-05, BSD-2-Clause, MSRV 1.60, 17 M downloads)
  dispatches TIFF/JPEG/PNG/WebP/HEIF (`src/reader.rs:133-142`), but its ISOBMFF
  branch knows only the HEIF brands `mif1`/`msf1` (`src/isobmff.rs:37`), so
  `ftyp crx ` is not read.
- **nom-exif 3.8.0** (2026-09-07, MIT per `LICENSE:1`; crates.io says
  "non-standard" because the manifest uses `license-file`, `Cargo.toml:43`;
  MSRV 1.83) parses the `CMT1`/`CMT2`/`CMT3` boxes but skips the maker note by
  design — "Skipping CMT3 (MakerNotes) - proprietary format"
  (`crate-nom-exif-3.8.0/src/cr3.rs:155`), so no `ColorData` and no WB.
- **little_exif 0.6.23** (2026-01-13, MIT OR Apache-2.0, MSRV 1.65) covers
  JPEG, JXL, HEIF/HEIC/HIF/AVIF, PNG, TIFF and WebP (`README.md:17-20`): no
  CR3, and it is an EXIF *writer* first.

All three are fine for EXIF and useless for as-shot WB.

### 3.7 tiff 0.11.3

- crates.io: 0.11.3, 2026-02-10, MIT, MSRV 1.85, 153 M downloads.
- A TIFF/BigTIFF decoder with a generic tag model: unknown tags come back as
  `Unknown(u16)` (`src/tags.rs:21`) and any tag can be fetched by number
  (`src/decoder/mod.rs:1612`), so it reads DNG `AsShotNeutral` and the matrices
  when the caller supplies the tag numbers.
- Not a RAW reader: no CR3, no maker notes, no camera database. Its value here
  is the **TIFF half of an own parser** (section 5), because `CMT1` and `CMT3`
  are TIFF structures embedded in the BMFF container.

### 3.8 Screened out without a full review

| Crate | Version / date | Licence | Why not |
| --- | --- | --- | --- |
| quickraw | 0.1.6, 2023-06-19 | LGPL-2.1 | no `cr3` in its sources; LGPL |
| rawbit | 0.1.18, 2026-06-03 | MIT | no `cr3` in its sources |
| raw-files | 0.9.0, 2026-11-10 | MIT OR Apache-2.0 | MSRV 1.95, 266 downloads, no `cr3` in its sources |
| rawkit | 0.1.0, 2024-11-03 | MIT OR Apache-2.0 | no `cr3` in its sources; 1 278 downloads |
| zenraw | 0.2.0, 2026-04-17 | AGPL-3.0-only | licence incompatible; MSRV 1.93 |
| gufo-exif | 0.5.5, 2026-10-26 | MPL-2.0 OR LGPL-2.1-or-later | MSRV 1.93; EXIF-level, no verified CR3 maker note |
| cr3 | 0.0.0, 2018-11-23 | GPL-3.0-only | empty placeholder |
| fpexif | 0.0.3, 2026-03-02 | MIT OR Apache-2.0 | EXIF only, no CR3 path found |

The "no `cr3` in its sources" statements are a case-insensitive grep over the
extracted tarballs (2026-11-10): evidence of absence in the source, not of a
documented limitation.

## 4. Comparison

| Candidate | Latest / date | Licence | CR3 (evidence) | As-shot WB values | Matrices for CR3 | Pure Rust | Linux/macOS/Windows | MSRV 1.88 | Deps / size | unsafe |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| rawler | 0.8.0 / 2026-08-30 (0.7.2: 1.88) | LGPL-2.1 | yes (`cr3.rs:778`) | only via full decode (`rawimage.rs:216`) | bundled TOML (`r5.toml:8`) | yes | yes | 0.8.0 needs 1.89 | 24 deps, 24 MB | 57 |
| rawloader | 0.37.2 / 2026-08-15 | LGPL-2.1 | **no** (no CR3 module) | – | – | yes | yes | – | 5 deps, 7 MB | 3 |
| libraw-rs(+sys) | 0.0.4 / 2021-02-02 | MIT wrapper, LibRaw LGPL/CDDL | yes (bundled 0.20.1, `cr3_parser.cpp`) | `cam_mul` in sys bindings (`bindings.rs:10038`) | `cam_xyz` (`:10043`) | **no** (C++) | needs C++ toolchain | yes | heavy | FFI in caller |
| rsraw(+sys) | 0.1.1 / 2026-03-21 | MIT wrapper, LibRaw LGPL/CDDL | yes (bundled 0.21.3) | sys only | sys only | **no** (C++) | needs C++ toolchain | yes | heavy | FFI in caller |
| libopenraw | 0.4.0-alpha.13 / 2026-09-18 | LGPL-3.0-or-later | yes (`src/canon/cr3.rs`) | multipliers only, `as_shot_white_xy` DNG-only (`dng.rs:445`) | bundled `matrices.rs` | yes | yes (uses `libc`) | yes | medium | 160 |
| flashraw | 0.1.1 / 2026-10-25 | MIT OR Apache-2.0 | yes (`cr3/mod.rs:99`) | multipliers, no CCT (`types.rs:470`) | bundled DB (R3/R5/R7/R50) | yes | unverified on Windows | yes (1.85) | 4 crates, ~1.2 MB data | 13 |
| kamadak-exif | 0.6.2 / 2026-11-05 | BSD-2-Clause | **no** (`isobmff.rs:37`) | no | no | yes | yes | yes (1.60) | small | 2 |
| nom-exif | 3.8.0 / 2026-09-07 | MIT | container only, maker note skipped (`cr3.rs:155`) | no | no | yes | yes | yes (1.83) | medium | 1 |
| little_exif | 0.6.23 / 2026-01-13 | MIT OR Apache-2.0 | **no** (README:17-20) | no | no | yes | yes | yes (1.65) | small | – |
| tiff | 0.11.3 / 2026-02-10 | MIT | no (TIFF only) | DNG only, by tag (`tags.rs:21`) | DNG only | yes | yes | yes (1.85) | small | 6 |
| own parser | – | project licence | yes, by construction | yes, from `ColorData` | needs own DB | yes | yes | yes | new code | 0 allowed |

No candidate returns the as-shot **temperature** for a CR3 out of the box; those
returning multipliers need the vendor CCT table (section 2.2) or a matrix plus
the CCT inversion of `lightcraft:crates/raw/src/color.rs:108-154`.

## 5. Own parser

### 5.1 DNG / TIFF half (low risk)

A TIFF IFD reader for `AsShotNeutral`, `AsShotWhiteXY`, `ColorMatrix1/2` and
`ForwardMatrix1/2` is well specified and small (tag numbers as in
`lightcraft:crates/tiff/src/tags.rs:121-128`). Written once, it serves DNG and
the CR3 maker note alike.

### 5.2 CR3 / ISOBMFF half (moderate risk)

Four small pieces: (1) box walk `ftyp` → `uuid` → `CMT3` (the `moov` tree only
for pixel data, which is not needed); (2) the TIFF IFD reader over the `CMT3`
slice; (3) `ColorData` parsing, i.e. version → word offset for the multipliers
and the preset CCT table; (4) CCT computation, only if the vendor CCT is not
used.

The offset table is **not documented by Canon**. Every implementation has
reverse-engineered it, and the three above differ in structure (rawler shares a
CR2 table, LibRaw switches on a version id, libopenraw indexes by `ColourData`
variant). New bodies add versions, so it needs maintenance. A wrong offset for
a new body silently yields a wrong white balance, so any result must be checked
for plausibility (positive multipliers, temperature inside the slider range)
and fall back to "report" — never guess.

Cost: a few hundred lines plus a per-version fixture set that needs real CR3
files, which the test rule excludes — and a hand-built maker note could encode
the same wrong assumption as the parser.

## 6. Rejected path: external tools

`exiftool`, `dcraw` and `dcraw_emu`/`libraw` would answer the question today
(`-ColorTempAsShot`, `-WB_RGGBLevelsAsShot`, `-ColorMatrix2`), but they need an
installation the project rules do not allow, are not present on all three
platforms by default, and their textual output is not a stable interface.
Rejected.

## 7. Options

### Option A — approve one crate for the probe only

Read the original with a single crate behind `--probe-white-balance`, behind a
trait in `crates/lr-reader` so the reader stays replaceable (principle 7).

- Only realistic candidate on licence grounds: **flashraw 0.1.1** (MIT OR
  Apache-2.0, pure Rust, CR3 with multipliers and matrices). It still needs the
  CCT inversion in lr2lc and is 0.1.x.
- LibRaw or libopenraw would need less own code, but the licence (LGPL/CDDL,
  LGPL-3.0) and the C++ toolchain conflict with the project rules.
- rawler would work too, but needs MSRV 1.89 and a full pixel decode.

### Option B — own parser, DNG/TIFF plus CR3/ISOBMFF

The reader of section 5, with no new dependency except possibly `tiff` (MIT)
for the TIFF half: smallest footprint, full control, no licence question. Cost:
the reverse-engineered `ColorData` table, a CR3 fixture strategy, maintenance
per new camera.

### Option C — defer the probe

Keep `--probe-white-balance` refused (exit 2, `README.md:48`) and
`Temperature`/`Tint` reported as not carried
(`crates/lr-reader/src/develop.rs:1009-1015`); do the dependency-free step first
— ask the running app for the target scale (section 1.5) — and only then decide
whether a probe is still needed.

## Open questions

1. **Which Canon bodies do the catalogs contain?** Seven EOS R bodies are
   covered by LightCraft's measured matrices, others are not; without the body
   names the size of the problem cannot be estimated. (Gear information, not
   catalog content.)
2. **Is Canon's per-preset CCT equal to the as-shot temperature Lightroom
   shows?** Unverified; it decides whether an own parser can skip the CCT
   inversion.
3. **Does lr2lc accept a camera table?** Duplicate LightCraft's coverage list,
   read the scale from the app (section 1.5), or read matrices from a bundled
   database — each has a coupling or maintenance cost.
4. **How much may the MSRV rise?** rawler 0.8.0 needs 1.89; the project is on
   1.88.0.
5. **Is copyleft acceptable for an MIT/Apache project?** LibRaw
   (LGPL-2.1/CDDL-1.0) and libopenraw (LGPL-3.0) are technically strongest but
   not permissively licensed.
6. **How are CR3 fixtures produced under the "generated fixtures only" rule?**
   The layout is reverse-engineered, so a hand-built fixture could encode the
   same wrong assumption as the parser.

## Recommendation

**Option C now, Option B if the probe is wanted soon.** Concretely: add no
dependency in this release. Do the dependency-free step first — read the target
scale from the app after import (section 1.5) and carry absolute Kelvin where
LightCraft keeps a Kelvin scale. That covers the seven measured EOS R bodies
without RAW parsing and without a licence question.

For the remaining bodies the file gives multipliers but no matrix, so a probe
must bring its own colour model and will produce a *different* as-shot
reference than Lightroom's — a systematic shift on every white-balance edit, not
a one-off error. The yardstick is the as-shot temperature as Lightroom defines
it.

If the probe is wanted anyway, Option B fits better than Option A: the data sits
in one maker-note tag with a known, small offset table, the TIFF half can use
`tiff` (MIT), and no permissively licensed crate in the comparison returns the
as-shot temperature for a CR3. Option A would add a young dependency (flashraw)
and still leave the CCT inversion to lr2lc.

Fallback in every case, and the part that must not change: an unreadable,
unknown or implausible file is **reported**, never guessed at, and the import
continues. `Temperature`/`Tint` stay in the unmapped list with a fixed reason
code, the counters keep read = converted + skipped, and the photo keeps its
white-balance mode (`crates/lc-writer/src/plan.rs:1211-1225`,
`docs/research/develop-mapping.md:334-338`). No probe result is ever written to
the original file, and the probe never runs implicitly
(`lightcraft:crates/engine/src/cmd/develop.rs:197-203` shows the only place a
WB value is normalised, and it runs on `develop.set` only).
