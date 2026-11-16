# lr2lc

lr2lc converts Adobe Lightroom Classic catalogs (`.lrcat`) into LightCraft libraries. It is a Rust command-line tool. It is not affiliated with Adobe.

## Status

Version 0.1.0 is released (tag `v0.1.0`; repository still private); 0.2.0 fixes from the first live tests are on `main` (see `ROADMAP.md`). What works: folders, photos (as references to the existing files; videos are skipped), keywords with hierarchy, synonyms and attributes, star ratings, pick/reject flags, colour labels (via a label set), titles, captions, copyright and creator, collections as albums and collection sets as album folders, smart collections (exact star-rating rules only), stacks, the Quick Collection, stable identity via Lightroom's global ids, a safe re-sync, and development settings (sliders, tone curves, crop, white-balance mode, masks and spots). Temperature/tint, camera profiles and a few unconfirmed develop fields are reported, not converted. LightCraft's own Lightroom import also maps development settings, so lr2lc's edge is the safe re-sync, the dry run with a full report, path mapping, stacks, smart albums and stable identity. Only Lightroom Classic 15 catalogs (`Adobe_DBVersion` 1500000) are read.

What comes next is in [ROADMAP.md](ROADMAP.md) and in the GitHub issues (milestones `0.2.0`, `0.3.0`, `Later`). User-visible changes go into [CHANGELOG.md](CHANGELOG.md).

## Principles

These rules decide every design question. Code that breaks one of them is not merged.

1. **Read Lightroom only.** Open the catalog read-only (SQLite `mode=ro&immutable=1`). A checksum before and after each run proves it is unchanged.
2. **Never touch original photos.** No moving, renaming, rewriting, no XMP sidecars. LightCraft gets references to the files.
3. **Dry run by default.** Without `--apply` the tool only reports what it would do.
4. **Nothing is lost silently.** Everything that cannot be converted goes into the report (`report.json` plus a readable summary) with a reason and a skip code. The numbers add up: read = converted + skipped.
5. **Idempotent.** A second run creates no duplicate photos or keywords. Every photo carries its Lightroom id as provenance.
6. **Runs everywhere.** macOS, Linux and Windows. Platform-neutral paths (`std::path`), nothing hard-coded. A path mapping (old prefix → new prefix) handles moved catalogs and photo folders.
7. **Three separate parts.** Reader (Lightroom) → neutral model → writer (LightCraft). Each side is tested on its own and can be replaced.
8. **Use LightCraft's official interfaces** rather than writing to its library directly: the control port of the running app (`lightcraft --control <port>`).
9. **User data stays private.** Real catalogs and anything extracted from them never enter the repository: no file names, paths, keywords, names or values in code, tests, docs, issues or commit messages. Tests use generated catalogs.
10. **Small and readable.** Rust stable, `cargo fmt`, `cargo clippy -- -D warnings`, no `unsafe`, few dependencies.
11. **Public from the first commit.** English everywhere (code, comments, docs, commit messages, issues). "Lightroom" only as a compatibility statement, never in the project name.

## Layout

Cargo workspace, edition 2024, MSRV 1.88.0, licence MIT OR Apache-2.0.

| Crate | Role |
| --- | --- |
| `crates/lr-reader` | Reads a catalog into the model: `catalog.rs` (open, version and schema checks), `folders`, `photos`, `originals` (file existence), `keywords`, `collections`, `smart` (rule parser), `stacks`, `xmp` (titles), `develop` (Lua settings parser and mapping) |
| `crates/model` | Neutral data model, skip codes (`skip.rs`), develop model, label sets, mapping file format |
| `crates/lc-writer` | Writes to LightCraft: `control.rs` (port client), `plan.rs` (commands per photo), `apply.rs` (execution, re-sync), `live.rs` (reads the library state), `state.rs` (mapping file `<library>.lr2lc.json`), `albums`, `smart` |
| `crates/lr2lc` | The binary: `args.rs`, `dry_run.rs`, `writer.rs`, `output.rs` (summary), `report_file.rs` (`report.json`), `labels.rs` |
| `crates/lc-test-support` | Fake LightCraft control port for tests (not shipped) |
| `tests/fixtures/generator` | Generates test catalogs (`lrcat-fixtures`, not shipped); see `tests/fixtures/README.md` |

## Commands

```sh
cargo build --release -p lr2lc          # binary: target/release/lr2lc
cargo test --workspace                  # all tests (about 1,600)
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
scripts/check-links.sh                  # relative links in Markdown
```

CI (`.github/workflows/ci.yml`) runs the same on Ubuntu, macOS, Windows and MSRV, with `--locked`. Golden files: `crates/lr2lc/tests/golden/`, `tests/fixtures/golden/`.

## Workflow

- Branch from `main` as `js/<topic>`, one topic per branch, small commits, imperative English commit messages ("Add folder reader").
- Open a pull request; CI must be green before merging. Reference the issue (`Fixes #n`).
- Tests first: every reader and writer change gets a test against generated fixtures or the fake port.
- Add an entry under `## [Unreleased]` in `CHANGELOG.md` for every user-visible change.
- Update `README.md` (user documentation) and `docs/report-format.md` when options, output or the report change.
- Errors via `thiserror` in libraries, `anyhow` in the binary.
- New dependencies need the maintainer's approval. Approved so far: serde, serde_json, clap, thiserror, anyhow, rusqlite, flate2, chrono, sha2, walkdir, tempfile (tests only).
- No code is copied from lrcat-extractor (MPL-2.0). Reading it for understanding is fine.

## LightCraft

- Source: <https://github.com/storytold/lightcraft> (MIT OR Apache-2.0). The control protocol is in its `docs/control-protocol.md`; `docs/research/` here records what lr2lc relies on, with file:line references to the version read.
- Live-tested against LightCraft 0.14.0 and 0.21.1 on macOS. Unit and integration tests run against the fake port only.
- Live tests run outside this repository with copies of real catalogs and a throwaway library. Only numbers and field names from such runs may appear here.

## Documentation

| File | Content |
| --- | --- |
| `README.md` | User documentation: options, output, re-sync, known limits, exit codes |
| `ROADMAP.md` | Milestones and what is planned |
| `CHANGELOG.md` | Changes per release |
| `docs/report-format.md` | Contract of `report.json` |
| `docs/release-checklist.md` | Steps for a release |
| `docs/research/` | LightCraft and Lightroom findings (schema, control port, develop mapping, smart collections, label names, assumptions) |
| `docs/ci-proposal.md` | Background of the CI setup |
