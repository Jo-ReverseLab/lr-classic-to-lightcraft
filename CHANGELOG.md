# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
The project has not made a release yet.

## [Unreleased]

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
  sky, with blend operations and local sliders) and the camera profile name
  (kept byte for byte). The stored Lua table is
  parsed without executing it; unknown
  fields, unreadable entries and Lightroom's `-999999` sentinel are recorded
  for the report. The crop is transformed into LightCraft's oriented frame
  through the stored orientation and image size; a broken curve list follows
  the unreadable-row path.
- Writing of development settings (phase 6a): slider values as
  `develop.set`, the structured parts, the white-balance mode, the tone
  curves, the crop geometry and the masks as `develop.merge` after a
  selection. Temperature, tint, the look and the other unmapped fields are
  reported by name and never written; the camera profile is read and reported
  but not written, and there is no profile or LUT converter. A mask that
  LightCraft cannot express is reported under a stable `Mask: ...` name, never
  written partially. Spots (healing and cloning) follow in a later change.
- `develop_hash` in the mapping file keeps the develop writes idempotent. A
  known photo whose settings changed in Lightroom is only rewritten with the
  new `--overwrite-develop`.
- A top-level `develop` area in `report.json` and one summary line count the
  photos with settings, the planned and written commands, the unmapped field
  names (capped at 20), the changes left alone, the overwrites, the converted
  photos with a camera profile and the converted photos with masks. Profiles
  are grouped by name (sorted, capped at 20) with the full distinct count.
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
