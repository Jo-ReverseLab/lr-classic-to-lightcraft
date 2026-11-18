# What works and what does not

[Back to the README](../README.md)

## Works

Checked by tests and in live runs against LightCraft 0.21.1 on macOS, with
catalogs from Lightroom Classic 15:

- Photos as references to the existing files, virtual copies with their
  master, ratings, pick/reject flags, colour labels, titles, captions,
  copyright and creator.
- Keywords with hierarchy, synonyms and attributes, and their assignments.
- Collections and collection sets as albums and album folders, folder
  stacks, the Quick Collection, smart collections with star-rating rules.
- Development settings: sliders, tone curves, crop, Upright modes (Auto,
  Level, Vertical, Full), white balance, masks (linear, radial, brush,
  luminance range, depth range, subject, sky, background) and spots (heal,
  clone).
- A second run adds only what is new or changed, never duplicates, and
  reports values changed on both sides as conflicts instead of writing them.
- Photos the library already has (imported by hand) are found before
  writing and handled by choice, never duplicated; photos deleted in
  LightCraft since an earlier run are found and imported again or left.
  The window asks for both before it writes.
- Dry run with summary, `report.json` (optionally anonymized) and a
  run log; in the window also Stop and Cancel.

## Known limits

Everything below is left out and reported by name or code in the summary
and in `report.json`; nothing is dropped silently. The issue numbers link
to the planned work.

Setup

- Only catalogs of version `1500000` (Lightroom Classic 15) are read; any
  other version stops with exit code 1.
- LightCraft must run on the same computer with its control port enabled;
  only `127.0.0.1` is contacted.
- Transfers into a running LightCraft are tested on macOS only. On Linux
  and Windows only the automated tests run.
- LightCraft reads an XMP sidecar next to an original when it imports the
  file (rating, label, keywords, development settings). lr2lc records a new
  photo with LightCraft's import defaults and writes only the Lightroom
  values that differ from them, so a value from a sidecar that Lightroom
  does not have (for example stars in the sidecar, none in the catalog) can
  stay in LightCraft. Turn off “Automatically write changes into XMP” in
  Lightroom, or remove stale sidecars, before the first transfer.

Photos

- Photos whose original file is missing are skipped with everything that
  belongs to them; LightCraft offers no way yet to add them as offline
  records over the control port (see [Missing originals](usage.md#missing-originals),
  [#23](https://github.com/Jo-ReverseLab/lr2lc/issues/23)).
- Originals that are only in iCloud (or another cloud drive) are
  downloaded by LightCraft one by one while importing, often many seconds
  per photo. lr2lc only points them out and downloads nothing itself (see
  [Originals only in the cloud](usage.md#originals-only-in-the-cloud)); download
  them first.
- The choices for photos LightCraft already has and for photos deleted in
  LightCraft (`--existing`, `--removed`, the window's question) hold for
  one run; they are not remembered.
- Videos are skipped
  ([#15](https://github.com/Jo-ReverseLab/lr2lc/issues/15)).
- A virtual copy whose master is not converted (for example a video) is
  skipped with its memberships.

Collections and re-sync
([#16](https://github.com/Jo-ReverseLab/lr2lc/issues/16))

- Smart collections become smart albums only when every rule translates
  exactly, which currently means star-rating rules; all others are only
  reported (see [Smart collections](usage.md#smart-collections)).
- Stacks are always written expanded; the collapsed state is only counted.
- Renaming, moving and deleting collections, photos that left a stack, photos
  that left the Quick Collection, photos that disappeared from Lightroom,
  photos whose file moved and keywords that left a photo or were renamed in
  Lightroom are only reported, never applied. A moved photo is not imported
  again, and a renamed keyword is neither created nor added. Photos that left
  a collection are removed from the album, but only what lr2lc wrote itself.

Development settings (see [Development settings](usage.md#development-settings))

- A photo rotated or mirrored in Lightroom arrives in its camera
  orientation; the rotation or mirror is not carried over
  ([#10](https://github.com/Jo-ReverseLab/lr2lc/issues/10)).
- Not carried over, only reported:
  - camera profile and creative look
    ([#14](https://github.com/Jo-ReverseLab/lr2lc/issues/14));
  - point color, lens blur, AI denoise, Raw Details, Super Resolution and
    red eye ([#10](https://github.com/Jo-ReverseLab/lr2lc/issues/10));
  - colour range masks, AI object and people-part masks, masks combining
    several parts
    ([#11](https://github.com/Jo-ReverseLab/lr2lc/issues/11));
  - painted spots, generative Remove and distraction removal
    ([#12](https://github.com/Jo-ReverseLab/lr2lc/issues/12));
  - Guided Upright
    ([#13](https://github.com/Jo-ReverseLab/lr2lc/issues/13));
  - a Kelvin white balance on a photo LightCraft does not edit in Kelvin,
    and every other field without an exact LightCraft equivalent.
- `--probe-white-balance` (reading the original file) is not implemented
  and is refused with exit code 2 before the catalog is opened.
- A known photo whose development settings changed in Lightroom is left
  alone unless `--overwrite-develop` is given.
- Cancel cannot set back development settings it wrote to photos that were
  already in the library; the log lists them (`develop_not_reset`).

Window

- The per-photo list of photos LightCraft already has shows LightCraft's
  values (stars, edited or not), not Lightroom's.
- **Retry this photo** and **Overwrite with Lightroom state** are disabled
  until lr2lc can run a single photo
  ([#36](https://github.com/Jo-ReverseLab/lr2lc/issues/36)).
- No pause; the spinner ignores the system's reduced-motion setting; some
  labels and info marks are not reachable by keyboard or screen reader, and
  a run cannot be deleted from the history
  ([#45](https://github.com/Jo-ReverseLab/lr2lc/issues/45)).

Report and resources

- Skip reasons and unmapped names can quote short constants from the
  catalog (smart-rule criteria, mask kinds), also with `--anonymize`
  ([#43](https://github.com/Jo-ReverseLab/lr2lc/issues/43)).
- A dry run of about 25,000 photos needs about 300 MB of memory
  ([#44](https://github.com/Jo-ReverseLab/lr2lc/issues/44)).
