# T17 — Path mapping test matrix

Task: pin the behaviour of `--map OLD=NEW` with a table-driven test file and
fix real defects. The pure mapping function must stay testable without a file
system; nothing touches the originals.

Branch: `glm/t17-path-mapping` (base `origin/main`, 7f36890).

## What was added / changed

- `crates/model/src/mapping.rs`
  - new `MappedPath` enum (`Unchanged`, `Mapped`, `Escaped`) and
    `PathMappings::classify` returning it;
  - `PathMappings::apply` / `map` now built on `classify` (escape → refused,
    `map` falls back to the original path);
  - `escapes_prefix` + `component_depth` helpers — a `..` that would pop to the
    parent of `NEW` or above is an escape;
  - `PathMappings::collisions(&[&Path]) -> Vec<usize>` — later indices whose
    effective path duplicates an earlier one.
- `crates/model/src/skip.rs` — two new codes: `PathMappingEscaped`,
  `PathMappingCollision` (with `as_str` keys `path_mapping_escaped`,
  `path_mapping_collision`; `ALL` grew from 55 to 57).
- `crates/model/src/lib.rs` — re-export `MappedPath`.
- `crates/lr2lc/src/dry_run.rs` — folders and photos are mapped via `classify`;
  escaped items are reported and dropped (`path_mapping_escaped`); after
  mapping, collisions are reported and the later photo is dropped
  (`path_mapping_collision`). Balance is preserved: `read = converted +
  skipped`.
- `crates/model/tests/path_mapping_matrix.rs` — new table-driven test file
  (pure, no file system).
- `README.md` — the `--map` section now documents the tie rule, the `..` escape
  refusal, the collision report, and "no Unicode normalization".

## Documented rule (the exact behaviour)

- Prefixes match at component boundaries: `/a/b` matches `/a/b` and `/a/b/c`,
  not `/a/bc`.
- `/` and `\` are equivalent separators. Case matters, except a leading drive
  letter (`C:`). A trailing separator on either side makes no difference.
- Several rules: the longest matching `OLD` prefix wins; ties (same length,
  e.g. `/a` and `/a/`) break to the first rule given.
- The rest uses the separator style of `NEW` (`\` if `NEW` contains `\` or
  starts with a drive letter, else `/`). Nothing else is normalized.
- Names are not Unicode-normalized: composed and decomposed accents stay
  different paths.
- Empty `NEW` is allowed and yields a relative rest.
- `..` must not climb out of `NEW`: a `..` that pops to `NEW`'s parent or above
  is refused (`path_mapping_escaped`); a `..` that cancels a component added
  within the rest is allowed.
- Two photos landing on the same path after mapping: the later one is reported
  (`path_mapping_collision`) and not converted.

## Defects fixed

1. **`..` climbed out of the new prefix silently.** `/data/../secret/x.jpg`
   mapped with `/data=/mnt` produced `/mnt/../secret/x.jpg` (`/secret/x.jpg`),
   a path outside `/mnt`. Now refused and reported as `path_mapping_escaped`.
   The boundary is `NEW`'s depth: a `..` that would pop to `NEW`'s parent or
   above escapes; a `..` that only cancels an earlier component in the rest
   stays.
2. **Two photos could silently merge onto the same path.** Two different
   catalog paths that map to the same target (e.g. `/a/b/p.jpg` and
   `/a/c/p.jpg` with `/a/b=/x` and `/a/c=/x`) both landed on `/x/p.jpg`. Now the
   later one is reported as `path_mapping_collision` and dropped.

## Per-case matrix (single-rule `apply`, pure)

| case | from | to | input | expected |
| --- | --- | --- | --- | --- |
| prefix-boundary-no-match | /a/b | /x | /a/bc/d.jpg | None |
| prefix-boundary-match | /a/b | /x | /a/b/d.jpg | /x/d.jpg |
| trailing-sep-on-old | /data/ | /mnt | /data/a.jpg | /mnt/a.jpg |
| trailing-sep-on-new | /data | /mnt/ | /data/a.jpg | /mnt/a.jpg |
| trailing-sep-on-path | /data | /mnt | /data/ | /mnt |
| drive-letter-match | C:\data | D:\photos | C:\data\x.jpg | D:\photos\x.jpg |
| drive-letter-case-insensitive | C:\data | D:\photos | c:\data\x.jpg | D:\photos\x.jpg |
| folder-name-case-sensitive | C:\Data | D:\photos | C:\data\x.jpg | None |
| unc-path | \\server\share | /mnt | \\server\share\x.jpg | /mnt/x.jpg |
| mixed-separators | /data | /mnt | /data\sub/x.jpg | /mnt/sub/x.jpg |
| composed-acute-preserved | /data | /mnt | /data/café.jpg | /mnt/café.jpg |
| decomposed-acute-preserved | /data | /mnt | /data/cafe\u0301.jpg | /mnt/cafe\u0301.jpg |
| empty-new-yields-relative | /data | (empty) | /data/a/b.jpg | a/b.jpg |
| relative-prefix-matches-relative | data | /mnt | data/a.jpg | /mnt/a.jpg |
| relative-prefix-no-match-absolute | data | /mnt | /data/a.jpg | None |
| dotdot-escape-refused | /data | /mnt | /data/../secret/x.jpg | None |
| dotdot-within-prefix-allowed | /data | /mnt | /data/sub/../x.jpg | /mnt/sub/../x.jpg |

Dedicated tests (not single-rule): `longest_old_prefix_wins`,
`tie_breaks_to_first_rule`, `classify_distinguishes_unchanged_mapped_and_escaped`,
`dotdot_boundary_at_prefix_root`, `empty_new_prefix_escapes_on_leading_dotdot`,
`collisions_reports_later_duplicate`, `collisions_sees_unmapped_duplicates`,
and `composed_and_decomposed_stay_different`.

## Verification

- `cargo fmt --check` — clean.
- `cargo test` / `cargo clippy` — **not run**: no local C compiler in this
  workspace; CI is the proof. The matrix cases and the two defect fixes were
  derived by reading `mapping.rs` and tracing each path through `matches`,
  `escapes_prefix` and `join`.

## Open notes

- The collision check compares mapped paths by string equality (no
  normalization), so `/mnt/a/x.jpg` and `/mnt/b/../a/x.jpg` are treated as two
  different paths and do not collide. This is consistent with "nothing else is
  normalized"; a real collision needs the two effective paths to be identical
  text.
- Escape reporting covers folders and photos. Folder paths with `..` are far
  less likely than photo paths, but the same safe rule applies.
