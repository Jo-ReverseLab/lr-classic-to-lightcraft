# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
The first release is 0.1.0.

## [Unreleased]

### Added

- `--skip AREAS` leaves parts of the catalog out: keywords, ratings,
  labels, flags, texts, collections, stacks, virtual-copies, develop. A part
  left out sends nothing and is not compared; its fields keep their last
  imported value, so a later full run writes them. `report.json` lists them
  under `not_selected`, the summary in a `not selected:` line. Library API:
  `Job.areas` / `WriteInput.areas` (`Areas`).
- Lightroom's current brush masks (`Mask/Aggregate`) are carried over as
  brush masks with their strokes and erasers, and the AI background mask
  maps to LightCraft's background mask. On a real catalog, photos with a
  converted mask rose from 25 to 115 of 157 (#11).
- Custom white balance is carried over (#9). A relative pair (JPEG, TIFF and
  other rendered files) is converted to LightCraft's relative scale and
  written. A Kelvin pair (raw, DNG) is written only when LightCraft reports
  through `photo.inspect` that it edits the photo in Kelvin; otherwise it is
  counted as not carried. New report fields `develop.white_balance_custom`,
  `white_balance_to_check` and `white_balance_written`; the summary line
  shows all four counts. No original file is opened.
- `keywords.trimmed` in the report and the summary: keywords whose name
  starts or ends with white space. LightCraft trims keyword names, so such a
  keyword arrives without the spaces; this is now reported instead of
  happening silently (#4).
- `develop.unmapped_photos` in the report: every development field that is
  not carried over, with the number of photos it affects. The existing
  `develop.unmapped` list stays capped at 20 names (#7).
- Upright modes auto, level, vertical and full are carried over as
  LightCraft's `geometry.upright`; LightCraft computes the correction itself,
  as its own importer does. Guided Upright stays reported (#13).

### Changed

- The conversion now lives in a library crate, `lr2lc-core` (`Job`,
  `prepare`, `finish`, typed `RunError`), shared by the command line and a
  coming GUI. Output, report and exit codes are unchanged.
- The writer sends LightCraft commands through a `CommandSink` trait. The
  control-port client is one implementation; `HostSink` lets a host that runs
  LightCraft's commands in its own process drive a whole run
  (`lr2lc_core::finish_with`) without the port.
- Progress and cancel for front ends: `Progress` reports the phases
  (checksum, reading, checking originals, reading the library, writing per
  step) and can stop a run. While writing, a stop happens only between
  photos, keywords and album steps, never inside one photo; finished photos
  are recorded and the next run continues (`WriteError::Cancelled`,
  `ErrorKind::Cancelled`). The command line is unchanged.
- Production crates deny `unwrap`, `expect`, `panic!`, `unreachable!`,
  `todo!` and `unimplemented!` and forbid `unsafe` (tests may still
  unwrap), as LightCraft requires; the four `unreachable!` invariants became
  safe code.
- A named white-balance mode (As Shot, Auto, Daylight, …) no longer counts
  as `white_balance_not_carried`: the mode carries it, as in LightCraft's own
  Lightroom import. `Temperature`, `Tint` and the `Incremental*` names no
  longer appear in `develop.unmapped`; white balance has its own counters
  (#9).
- `--label-set auto` now does what Lightroom shows: it detects the active
  label set (English or German, whichever names label more photos) and maps
  only that set's names to colours. A name of the other set, such as `Red`
  in a German catalog, stays without colour and is reported as custom, as in
  Lightroom. New report field `photos.labels.detected_set`; the summary line
  names the detected set (#5).
- README: new section "Missing originals" explains why such photos are
  skipped, what stays behind and how to bring them over (connect the disk or
  `--map`, then run again). Offline records were requested from LightCraft
  (storytold/lightcraft#4392). Development settings and known limits now
  describe white balance, brush and background masks and Upright as they
  are written (#23).

### Fixed

- AI mask and range-mask codes are read when the catalog stores them as
  numbers, as Lightroom Classic does; before, every subject or sky mask from
  a catalog was reported as `Mask: AI mask (?)` (#11).
- Brush points with per-stroke state (`f`, `h`, `r` entries) are no longer
  reported as `Spot: dabs` / `Mask: dabs`, and an `M` entry starts a new,
  unconnected stroke (#12).
- Photos whose files have identical bytes at different paths (the same file
  copied into two folders) no longer collapse into one LightCraft photo.
  Each gets its own record, linked to its own file, and is counted in the new
  `writer.photos_same_content` (#17).
- A virtual copy no longer keeps keywords, ratings, labels, texts,
  development settings, albums or the stack of its master in LightCraft; it
  is reset to import defaults before its own values are written (#3).
- A request that crosses LightCraft's idle-close notice on the control port
  is sent once more on a new connection instead of being reported as
  rejected (#8).
- `Upright*` fields are no longer reported as not carried for every photo:
  with Upright off they have no effect, and version, preview, count and
  digest fields are bookkeeping (#13).

## [0.1.0] - 2026-11-15

First public release.

### Added

- Reader for Lightroom Classic catalogs, opened read-only with a version
  check, a check of the required tables and columns, and a refusal if
  unsaved `-wal` or `-journal` changes are pending. A checksum before and
  after each run proves the catalog is unchanged.
- Reading of folders and photos, with a check that the original files exist.
  Videos are skipped with a reason; a virtual copy is converted with its
  master and created once in LightCraft.
- Path mapping (`--map OLD=NEW`) for moved catalogs and photo folders, with
  the same behaviour on every platform.
- Keywords with their hierarchy, synonyms and attributes (export flags,
  person type) and their assignments to photos.
- Star ratings (0 to 5), pick/reject flags, colour labels with
  `--label-set` (`en`, `de`, `auto` or a JSON file), titles, captions,
  copyright and creator.
- Collections as albums and collection sets as album folders, with their
  members and nesting; smart collections (rule parser, strict translation of
  star-rating rules); folder stacks; the Quick Collection.
- Stable identity of photos and keywords through Lightroom's global ids, so
  repeated runs create no duplicates.
- Reading of development settings from the catalog (phase 6a sliders): the
  Light, HDR/SDR, tone-curve, colour-grading, colour, mixer, effects,
  vignette, grain, detail, optics, geometry and calibration controls, the
  old-process fields with LightCraft's approximation, the merge-only
  switches, the white-balance mode, the 2012 tone curves, the crop geometry,
  the masks (linear, radial, brush, luminance range, depth range, subject and
  sky, with blend operations and local sliders), the spots (heal and clone
  from `RetouchAreas` or the legacy `RetouchInfo`, with points, size, feather,
  opacity and source offset) and the camera profile name
  (kept byte for byte). The stored Lua table is
  parsed without executing it; unknown
  fields, unreadable entries and Lightroom's `-999999` sentinel are recorded
  for the report. The crop is transformed into LightCraft's oriented frame
  through the stored orientation and image size; a broken curve list follows
  the unreadable-row path.
- Writing of development settings (phase 6a): slider values as
  `develop.set`, the structured parts, the white-balance mode, the tone
  curves, the crop geometry, the masks and the spots as `develop.merge` after
  a selection. Temperature, tint, the look and the other unmapped fields are
  reported by name and never written; the camera profile is read and reported
  but not written, and there is no profile or LUT converter. A mask or spot
  that LightCraft cannot express is reported under a stable `Mask: ...` or
  `Spot: ...` name, never written partially.
- `develop_hash` in the mapping file keeps the develop writes idempotent. A
  known photo whose settings changed in Lightroom is only rewritten with the
  new `--overwrite-develop`.
- A top-level `develop` area in `report.json` and one summary line count the
  photos with settings, the planned and written commands, the unmapped field
  names (capped at 20), the changes left alone, the overwrites, the converted
  photos with a camera profile and the converted photos with masks and with
  spots. Profiles
  are grouped by name (sorted, capped at 20) with the full distinct count.
  The counter `white_balance_not_carried` counts photos whose temperature or
  tint pair was not carried over; only the number is reported.
- `--probe-white-balance` is reserved for reading an original file to derive
  a white-balance scale. It is not implemented yet and a run given it stops
  with exit code 2 before the catalog is opened. Without it lr2lc never opens
  an original photo; temperature and tint are only reported.
- Dry run by default. The summary and `--report FILE` (`report.json`) list
  everything that is not converted, each with a reason and a fixed code
  (such as `missing`, `video` or `smart_untranslatable`); read equals
  converted plus skipped for every area.
- Protection of the catalog and of other files: `--report` and `--script`
  refuse the catalog and its companion files (detected by file identity, also
  through symbolic and hard links on Linux and macOS) and files that are not
  earlier lr2lc output.
- Writer that talks to the control port of the running LightCraft app
  (`--apply`, `--library`, `--port`, `--allow-xmp`). It checks the app before
  writing anything (persistent library, matching library, no library
  problem, XMP auto-write off), keeps a mapping file next to the library for
  idempotent re-runs, and writes `report.json` also when a run is aborted.
- `library.select` before every photo command, as LightCraft requires.
- Albums and album folders with their members; stacks (written expanded);
  smart albums only when the rule is proven to be equivalent (star-rating
  rules); the Quick Collection, which is only added to.
- Recovery after a lost mapping file: existing albums and smart albums with
  the same name, parent and rules are taken over; ambiguous matches are
  skipped and reported.
- `--script FILE` writes the planned commands as JSON lines.
- A read-only snapshot of the current LightCraft library state, as groundwork
  for comparing it with Lightroom.
- Three-way reconciliation on a second run. For every field of a photo the
  mapping file knows and LightCraft still has, lr2lc compares the last
  imported value, the Lightroom value and the live LightCraft value. A field
  changed differently on both sides is not written and is reported with the
  photo id and field name only. Keywords are reconciled element by element
  and never removed: ones that left the photo in Lightroom are kept and
  counted. `report.json` gains `writer.conflicts`, `conflicts_count` and
  `keywords_removed` for applied runs. The live state is read only when a
  known photo changed in Lightroom; a dry run never connects.
- CI for Linux, macOS and Windows (format, clippy, tests).
- Reporting of items removed or renamed in Lightroom on a later run.
  `report.json` gains `writer.photos_removed`, `removed_photos`,
  `photos_moved`, `moved_photos`, `keywords_renamed` and
  `renamed_keywords`, naming only Lightroom ids. Nothing is deleted or
  renamed in LightCraft: a removed photo leaves only the albums lr2lc
  created itself, a moved photo is not imported again, and a renamed
  keyword is neither created nor added to a photo. The mapping file keeps
  each original's path as the base of the move comparison.
- Research notes on the Lightroom catalog schema and on LightCraft in
  `docs/research`.

### Changed

- Renamed the `writer.keywords_skipped` report field to
  `writer.keywords_not_created` (and `keywords_skipped_by_code` to
  `keywords_not_created_by_code`). The old name was misleading: it counts
  keywords the writer did not create (parent failed, rejected, or attributes
  changed after creation), a different set from the reader's
  `keywords.skipped`, which counts keywords skipped while reading the catalog.
  The readable summary lines follow the new wording. No release has shipped, so
  no schema migration is needed.
- `--label-set auto` still recognises only the English and German standard
  colour-label names. French and Spanish names are **not** added yet: they
  are pending a citable Adobe source for each name (see
  `docs/research/label-names.md`). Added a collision guard
  (`no_standard_name_maps_to_two_colors`) so a future French or Spanish set
  cannot silently clash with an existing name, and a `LabelSet::entries` /
  `LabelSet::standard_sets` accessor for the check.
- With `--label-set auto` (the default), the English and German standard
  colour-label names are recognised together: a name that equals one of
  them exactly maps to its colour, whatever the other labels are. Other
  non-empty labels are custom labels of their own photo. `report.json` gains
  `photos.labels.matched_sets` (the standard sets that matched at least one
  name, in the order `en`, `de`; empty for a fixed set); `label_set` is now
  `auto` under auto, never `none`. The old hint recommending `--label-set`
  is removed in every case.
- Photos, collections and stacks each satisfy a strict balance in the
  summary and report, in dry runs and real runs alike.
- Documentation brought up to date: README options, limits and the status of
  the research notes.

### Fixed

- `ui.inspect` is now sent as a method of the control port instead of an
  engine command. Before, every `--apply` against the real app stopped in
  the pre-flight check.
