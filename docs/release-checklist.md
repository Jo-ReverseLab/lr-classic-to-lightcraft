# Release checklist for the first public release (0.1.0)

> Historical: describes the state before 0.1.0. For later releases the gate in
> `.github/workflows/release.yml` is the same (tag = workspace version, the
> version appears in `CHANGELOG.md`, no `tasks/`).

A checklist, not a script. Nothing in it has been executed; it describes
`main` at `600765f`. Commands are read-only unless a step says otherwise.
`path:line` refers to this repository at that commit; counts come from the
quoted read-only commands. Steps marked **owner** need the repository owner's
account or a decision only the owner can take; section 5 collects them.

## 1. Starting point

### 1.1 Workspace and version

| Item | Value | Evidence |
| --- | --- | --- |
| Workspace version | `0.1.0` | `Cargo.toml:6` |
| Edition, MSRV, licence | `2024`, `1.88.0`, `MIT OR Apache-2.0` | `Cargo.toml:7-9` |
| Members | six, including two that are never shipped | `Cargo.toml:3` |
| Lock file, tracked files, tags | `Cargo.lock` tracked, 218 files, no tag | `git ls-files Cargo.lock`, `git ls-files \| wc -l`, `git tag \| wc -l` |
| Binary | `lr2lc`, prints `lr2lc <version>` | `crates/lr2lc/Cargo.toml:8-10`, `crates/lr2lc/src/main.rs:23-25` |

### 1.2 The release gate

`.github/workflows/release.yml` runs on a pushed tag matching `v*`
(`.github/workflows/release.yml:2-6`) and enforces three conditions:

1. `tasks/` does not exist (`.github/workflows/release.yml:18-19`).
2. The tag without `v` equals the workspace version, read from the first
   `^version = "` line of `Cargo.toml` (`.github/workflows/release.yml:22-27`).
3. `CHANGELOG.md` contains that version as a literal substring
   (`.github/workflows/release.yml:28-31`).

The workflow has one job, `release-sanity`
(`.github/workflows/release.yml:12`), and no step that builds a binary,
uploads an artefact or creates a GitHub Release; the file ends at line 31.

### 1.3 Current gate state

| Condition | State | Evidence |
| --- | --- | --- |
| `tasks/` gone | fails: 20 files, 212 KB, 2,755 lines | `find tasks -type f`, `du -sh`, `test -e tasks` |
| Changelog mentions `0.1.0` | fails: the string does not occur | `grep -c '0\.1\.0' CHANGELOG.md` returns 0 |
| Changelog state | opens with `## [Unreleased]`; line 6 says no release has been made | `CHANGELOG.md:6`, `CHANGELOG.md:8` |

### 1.4 Documentation, licence, metadata

- README: 833 lines; status "Early development; the version is 0.x"
  (`README.md:10-14`); Known limits (`README.md:783`); licence section
  (`README.md:826-833`); `LICENSE-APACHE` 10,763 bytes, `LICENSE-MIT` 1,074.
- Ignored: `/target/`, `/.worktrees/`, the `.lrcat` family, `.DS_Store`,
  `/.claude/` and sandbox placeholders (`.gitignore:1-2`, `.gitignore:4-11`).
- The root manifest has no `description`, `repository`, `homepage`, `readme`,
  `authors`, `keywords` or `categories` key (`Cargo.toml:1-9`); the only
  `description` belongs to the fixture generator
  (`tests/fixtures/generator/Cargo.toml:8`).
- `publish = false` is set only on the two crates that are never shipped
  (`crates/lc-test-support/Cargo.toml:7`,
  `tests/fixtures/generator/Cargo.toml:7`); the shipped crates have no
  `publish` key, for example `crates/lr2lc/Cargo.toml:1-6`. The tool is a
  binary, so this matters only if someone ever publishes the crates.

### 1.5 Automation

`ci.yml` runs on pushes to `main`, on pull requests and on manual runs (`ci.yml:3-8`): format
(`ci.yml:46-47`), relative links (`ci.yml:48-50`), personal data in added lines
(`ci.yml:51-62`), clippy (`ci.yml:63-64`), tests (`ci.yml:65-66`), and an MSRV
check (`ci.yml:68-85`), across Ubuntu, macOS and Windows; pull requests run the test job on Ubuntu only and skip the MSRV check (`ci.yml:22-29`, `ci.yml:70`). The
personal-data scan runs only for pull requests (`ci.yml:52`), so a commit
pushed straight to `main` is never scanned. `audit.yml` runs weekly and is
report-only unless an advisory is `high` or `critical` (`audit.yml:3-8`);
`performance.yml` runs weekly.

### 1.6 Privacy and identifier scan (numbers only)

Over the 218 tracked files: 38 Markdown files; 100 path fragments matching
`/(home|Users)/<name>`, all `example`; 81 fragments matching
`[A-Za-z]:\\+[A-Za-z0-9._-]+` (drive letter, colon, backslashes, name), of which
11 are `\n`-style escapes in string literals and the other 70 are example
placeholders; 0 e-mail addresses; 0 credential patterns (`ghp_`,
`github_pat_`, `AKIA`, `xox…`); 1 operational note about the permission scope
of the repository token, with no value and no secret (in the former
`PLAN.md`, removed since).
Internal identifiers: 454 occurrences, 56 distinct, in 66 of 218 files
(40 under `crates/`, 15 under `tasks/`, 7 under `docs/`, 4 under `tests/`).
Current practice keeps them; they do not block the release.

## 2. Preconditions

1. `git rev-parse main` — Expected: `600765f…` (40 characters).
2. `gh run list --branch main --limit 5` — Expected: the newest run for
   `main` is `completed` / `success`.
3. `git status --porcelain` — Expected: no output.
4. `git tag` and `gh release list` — Expected: no output from either.

## 3. Changes before the tag

5. **owner** Decide the package metadata (question 1). The root `Cargo.toml` is
   a virtual manifest with no `[package]` section (`Cargo.toml:1-9`): the values
   go under `[workspace.package]` first, and each chosen `[package]` inherits
   them with `description.workspace = true` and `repository.workspace = true`;
   a member line without the workspace key fails to build. Do this for
   `crates/lr2lc/Cargo.toml`, or for all four shipped crates (`lr-reader`,
   `model`, `lc-writer`, `lr2lc`). Check: `cargo metadata --no-deps --offline
   --format-version 1 | grep -c '"repository": "'` — Expected: `1` for the
   first variant, `4` for the second, `0` if the owner declines.
6. Insert the `0.1.0` section from section 6 below `## [Unreleased]`
   (`CHANGELOG.md:8`) and replace line 6, which would contradict it. Check:
   `grep -n '^## \[0\.1\.0\]' CHANGELOG.md` — Expected: one line;
   `grep -c 'has not made a release yet' CHANGELOG.md` — Expected: `0`.
7. **owner** Approve removing `tasks/`. Check afterwards:
   `test ! -e tasks && echo gone` — Expected: `gone`;
   `git ls-files tasks | wc -l` — Expected: `0`.
8. Full local verification, in this order, from the repository root:
   - `cargo fmt --all --check` — Expected: exit 0.
   - `sh scripts/check-links.sh` — Expected: exit 0 (it fails only when a
     relative Markdown link no longer resolves, `scripts/check-links.sh:4-15`).
   - `cargo clippy --workspace --all-targets --locked -- -D warnings` —
     Expected: exit 0.
   - `cargo test --workspace --locked` — Expected: exit 0, every suite passes,
     0 failed.
   - `cargo +1.88.0 check --workspace --all-targets --locked` — Expected:
     exit 0, if that toolchain is installed; otherwise rely on the MSRV job
     (`ci.yml:68-85`).
9. Scan the release diff by hand, because CI skips it on a direct push
   (`ci.yml:52`). Use the pattern and the two example exemptions from
   `ci.yml:58-59` against the added lines of `git diff --unified=0 main -- .`.
   Expected: no output.
10. Commit in English, imperative mood, one concern per commit (manifest and
    changelog may share one; the `tasks/` removal gets its own). Check:
    `git log --oneline -3` — Expected: two new commits on top of `600765f`.
11. Rehearse the gate locally; this transcribes `release.yml:22-31`:
    `v=0.1.0; w="$(grep -m1 '^version = "' Cargo.toml | sed 's/^version = "//; s/"$//')"; test "$v" = "$w" && test ! -e tasks && grep -qF "$v" CHANGELOG.md && echo gate-ok`
    — Expected: `gate-ok`.

## 4. Publishing

12. **owner** `git push origin main` — Expected: fast-forward, a new CI run.
13. `gh run list --branch main --limit 1` — Expected: `completed` /
    `success`.
14. **owner** `git tag -a v0.1.0 -m "lr2lc 0.1.0"`, then `git cat-file -t
    v0.1.0` — Expected: `tag`.
15. **owner** `git push origin v0.1.0` — Expected: the Release workflow
    starts (`release.yml:2-6`).
16. `gh run list --workflow=release.yml --limit 1` — Expected: `success`.
    A failure names the broken condition: `tasks` still present, version
    mismatch, or no changelog mention (`release.yml:18-31`).
17. `gh release list` — Expected: still empty; the workflow creates no release
    and uploads no artefact (`release.yml:11-31`).
18. **owner** Make the repository public (order: question 2):
    `gh repo edit --visibility public --accept-visibility-change-consequences`,
    then `gh repo view --json visibility` — Expected: `PUBLIC`.
19. **owner** Fill in the repository page:
    `gh repo edit --description "<one line>" --add-topic lightroom
    --add-topic lightcraft`, then `gh repo view --json description` —
    Expected: the description just set.
20. `git ls-remote --tags origin` — Expected: one `v0.1.0` line.
21. After the release, keep `## [Unreleased]` at the top and empty and the
    `0.1.0` section below it (`CHANGELOG.md:8`). Do not bump the workspace
    version before the next release: the gate compares it with the tag
    (`release.yml:22-27`).

## 5. Steps only the owner can take

- Approve the release commit, the metadata decision and the removal of
  `tasks/` (steps 5, 7, 12).
- Push `main` and the tag (steps 12, 15); only the owner holds write access.
- Change visibility, description and topics (steps 18, 19).
- Delete and re-push a tag that fails the gate after it went public. Anyone
  who already fetched it will see the retraction.

## 6. Draft CHANGELOG section (not applied)

Insert below `## [Unreleased]` (`CHANGELOG.md:8`) and replace line 6, for
example with "The first release is 0.1.0." It only summarises the
`Unreleased` section (`CHANGELOG.md:10-147`; the wording follows
`CHANGELOG.md:39-40` and `CHANGELOG.md:49-51`) and adds no claim that is not
already there.

```markdown
## [0.1.0] - YYYY-MM-DD

First public release.

### Added

- Reader for Lightroom Classic catalogs, opened read-only and proven unchanged
  by a checksum before and after every run.
- Folders, photos, path mapping, keywords with hierarchy, synonyms and
  attributes, ratings, flags, colour labels, titles, captions, copyright and
  creator.
- Collections as albums and collection sets as album folders, smart
  collections as smart albums where every rule translates exactly, stacks,
  the Quick Collection.
- Development settings: sliders, tone curves, colour grading, masks, spots,
  crop geometry and the white-balance mode. Temperature, tint and the look are
  reported, never written; the camera profile is read and reported by name.
- Dry run by default, a readable summary, and `report.json` with a fixed
  reason code for everything not converted; read equals converted plus
  skipped.
- Writer through the control port of the running app, a mapping file for
  idempotent re-runs, three-way reconciliation on a second run, and reporting
  of removed, renamed and moved items.
- CI for Linux, macOS and Windows (format, clippy, tests).
- Research notes on the catalog schema and on LightCraft in `docs/research`.

### Changed

- Report fields for keywords the writer did not create use the
  `keywords_not_created` wording throughout.

### Fixed

- `ui.inspect` is sent as a method of the control port.
```

A bare header with no body also satisfies the gate (`release.yml:28-31`).

## 7. What this checklist cannot cover

- **No binaries.** The workflow builds and uploads nothing
  (`release.yml:11-31`); 0.1.0 ships source only, and a build job is a separate change.
- **No crates.io publication.** No shipped crate sets `publish = false` and no
  publishing step exists; publication would also need a licence decision.
- **macOS and Windows beyond CI.** The test matrix (`ci.yml:22-29`) is the only
  proof for those platforms; a real run against the app stays a manual check.
- **GitHub-side metadata.** Description, topics and visibility live on the
  repository, not in the tree (steps 18-19).

## Open questions

1. Should the shipped crates inherit `description` and `repository` from
   `[workspace.package]` before the tag, which pins the manifest (`Cargo.toml:1-9`)?
2. Public before or after the tag? Before: free Actions minutes and a public
   release from the first second. After: the owner watches the run in private.
3. Should `tasks/` be removed in its own commit (step 10) or with the changelog entry?
4. Should a GitHub Release with notes be created by hand after the tag? The
   workflow does not create one (`release.yml:11-31`).
5. Should the shipped crates get `publish = false` so an accidental
   `cargo publish` cannot fail halfway?
6. Does the operational note about the token's permission scope stay as
   written in the public repository? (Resolved: `PLAN.md` was replaced by
   `ROADMAP.md`.)

## Recommendation

Do steps 1-4 on `main`, then steps 5-11 on a short branch merged through a
pull request, so the personal-data scan (`ci.yml:51-62`) sees the release diff,
which a direct push does not. Tag only after that merge is green. Make the
repository public after the tag run succeeds: the release is then the first
thing a visitor sees, and a failed gate is fixed while still private. Answer
question 1 with yes; a one-line description and the repository URL cost
nothing and cannot be added to a tag afterwards. Keep 0.1.0 source-only;
binaries and crate publication belong to a later release.
