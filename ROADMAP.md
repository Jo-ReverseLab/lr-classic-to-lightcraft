# Roadmap

Where lr2lc stands and what comes next. Details and progress are in the
[issues](https://github.com/Jo-ReverseLab/lr2lc/issues) and their
[milestones](https://github.com/Jo-ReverseLab/lr2lc/milestones). Finished
work is listed in [CHANGELOG.md](CHANGELOG.md).

## 0.1.0 – first public release

Released as `v0.1.0`. Everything it converts is described in the
[README](README.md#what-comes-over).

## 0.2.0 – desktop window and fixes from the first live tests

Released as `v0.2.0`. The first runs against the real LightCraft app
(0.14.0 and 0.21.1, macOS) confirmed the counts, re-sync and develop
sliders, and found these gaps. Done (see [CHANGELOG.md](CHANGELOG.md)):

- Desktop window `lr2lc-gui` with dry run, transfer, Stop, Cancel (taking
  a run back) and the run log
- Photos LightCraft already has are found before writing and handled by
  choice (`--existing`; the window asks), never overwritten silently
- Photos deleted in LightCraft since an earlier run are found and imported
  again or left (`--removed`; the window asks)
- An import LightCraft rejects but finishes later is not duplicated;
  LightCraft's own error text is kept in the log
- Originals only in iCloud are found and pointed out before the transfer
- Photos with identical file content collapse into one LightCraft photo (#17)
- A virtual copy inherits keywords of its master (#3)
- Keyword names with leading or trailing spaces are trimmed silently (#4)
- Colour label names outside the active label set now behave as in Lightroom (#5)
- Smart rules on rating 0 checked against Lightroom's counts (#6)
- Report every unmapped development field with a photo count (#7)
- Handle the control port's idle-close notice (#8)

## 0.3.0 – development settings, part 2

- White balance temperature and tint, using the scale LightCraft reports
  for each photo (#9, done)
- Map the remaining development fields, shapes confirmed in Lightroom:
  lens blur, point color, manual chromatic aberration (defringe), enhance
  (denoise), mirroring (crop flip); red eye still to confirm (#10)
- AI masks and aggregate masks (#11; brush groups and background done,
  objects and people parts open)
- Brush spots and remove areas (#12; brush spots done, generative Remove
  open)
- Upright and guided perspective corrections (#13; Auto, Level, Vertical
  and Full done, Guided open)
- Photos whose original file is missing: import them as offline instead of
  skipping them (#23; waits for a LightCraft control command,
  storytold/lightcraft#4392)

## Later

- Camera profiles and looks (#14)
- Videos (#15)
- Smart collection rules beyond star ratings, the collapsed state of
  stacks, applying removals and renames on re-sync (#16)
- Lightroom Classic catalog versions other than 15
