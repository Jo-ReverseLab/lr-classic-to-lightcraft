# lr2lc

lr2lc converts Adobe Lightroom Classic catalogs (`.lrcat`) into LightCraft libraries. It is a Rust command-line tool. It is not affiliated with Adobe.

First release (MVP): folders, photos (as references to the existing files), keywords including hierarchy, and star ratings (0–5). Development settings follow in a later phase and are the project's main differentiator.

## Principles

These rules decide every design question. Code that breaks one of them is not merged.

1. **Read Lightroom only.** Open the catalog read-only (SQLite `mode=ro`). A checksum before and after each run proves it is unchanged.
2. **Never touch original photos.** No moving, renaming, rewriting, no XMP sidecars. LightCraft gets references to the files.
3. **Dry run by default.** Without `--apply` the tool only reports what it would do.
4. **Nothing is lost silently.** Everything that cannot be converted goes into the report (`report.json` plus a readable summary) with a reason. The numbers add up: read = converted + skipped.
5. **Idempotent.** A second run creates no duplicate photos or keywords. Every photo carries its Lightroom id as provenance.
6. **Runs everywhere.** macOS, Linux and Windows. Platform-neutral paths (`std::path`), nothing hard-coded. A path mapping (old prefix → new prefix) handles moved catalogs and photo folders.
7. **Three separate parts.** Reader (Lightroom) → neutral model → writer (LightCraft). Each side is tested on its own and can be replaced.
8. **Use LightCraft's official interfaces** (engine or CLI) rather than writing to its library directly, if the research phase confirms this is possible.
9. **User data stays private.** Real catalogs and anything extracted from them never enter the repository. Tests use generated test catalogs; a generator produces large catalogs for performance tests.
10. **Small and readable.** Rust stable, `cargo fmt`, `cargo clippy -- -D warnings`, no `unsafe`, few dependencies.
11. **Ready to publish from the first commit.** English everywhere (code, comments, docs, commit messages). No personal paths, names or data in code or history. "Lightroom" only as a compatibility statement, never in the project name.

## Planned layout

Cargo workspace:

- `crates/lr-reader` – reads Lightroom Classic catalogs into the model
- `crates/model` – neutral data model (folders, photos, keywords, ratings, later develop settings)
- `crates/lc-writer` – writes the model into a LightCraft library
- `crates/lr2lc` – the command-line tool
- `tests/fixtures/` – generated test catalogs only

## Conventions

- Tests first. Every reader and writer function has unit tests against generated fixtures.
- Errors via `thiserror` in libraries, `anyhow` in the binary.
- Commit messages in English, imperative mood ("Add folder reader").
- License: MIT OR Apache-2.0 (to match LightCraft).
- No code is copied from lrcat-extractor (MPL-2.0). Reading it for understanding is fine.
