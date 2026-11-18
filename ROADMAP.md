# Roadmap

Where lr2lc stands and what comes next. Details and progress are in the
[issues](https://github.com/Jo-ReverseLab/lr2lc/issues) and their
[milestones](https://github.com/Jo-ReverseLab/lr2lc/milestones). Finished
work is listed in [CHANGELOG.md](CHANGELOG.md).

## 0.1.0 – first public release

Released as `v0.1.0`. Everything it converts is described in the
[README](README.md#status). Remaining step: make the repository public (#2).

## 0.2.0 – fixes from the first live tests

The first runs against the real LightCraft app (0.14.0 and 0.21.1, macOS)
confirmed the counts, re-sync and develop sliders, and found these gaps.
Fixed on `main` (see [CHANGELOG.md](CHANGELOG.md)):

- Photos with identical file content collapse into one LightCraft photo (#17)
- A virtual copy inherits keywords of its master (#3)
- Keyword names with leading or trailing spaces are trimmed silently (#4)
- Colour label names outside the active label set now behave as in Lightroom (#5)
- Report every unmapped development field with a photo count (#7)
- Handle the control port's idle-close notice (#8)

Open:

- Smart rules on rating 0 also match unrated photos in LightCraft; needs a
  check of how Lightroom counts them (#6)

## 0.3.0 – development settings, part 2

- White balance temperature and tint, using the scale LightCraft reports
  for each photo (#9, done)
- Confirm and map the remaining development fields: red eye, lens blur,
  point color, manual chromatic aberration, crop flip, enhance (#10)
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
