# t21-audit: lossy casts, panics and indexing

Status: in progress — the table lists every finding; rows marked *fix* are
being implemented test-first on this branch, all others are justified safe.
Branch: `agent/t21-audit`
Base: `agent/t20-performance` (`3edac05`, "Record the T20 performance measurements")

## Goal

T05 fixed two `.expect` calls in the reader's keyword resolver. This audit
covers the whole workspace for:

- `unwrap`, `expect`, `panic!`, `unreachable!`, `todo!`, `unimplemented!`;
- array/slice indexing and range slices (`a[i]`, `&a[i..j]`);
- integer `as` casts and arithmetic that can overflow on a big or hostile
  value;
- `String::from_utf8_lossy`, `to_string_lossy` and similar transformations
  that silently change data.

Scope: production code only (`crates/*/src`). Test code, including the
test-only `lc-test-support` crate, is out of scope as the task says. Two
external data sources matter: the Lightroom catalog (via rusqlite) and the
running LightCraft app's JSON answers; command-line arguments are the third.

## Findings

### Panics (`unwrap`, `expect`, `panic!`, `unreachable!`)

| File:line | Finding | Data source | Decision | Test |
| --- | --- | --- | --- | --- |
| `lr2lc/src/output.rs:263` | `.expect("report serializes")` on `serde_json::to_string_pretty` | report built from the model (strings, integer counts, booleans) | safe: serializing this struct cannot fail; no non-string map keys, no non-finite floats | — |
| `lr-reader/src/collections.rs:162` | `unreachable!("own skips are part of the status")` | catalog collections | safe: `resolve` records a collection's own skip in `status` before the member check, so the `(own Err, member Ok)` case cannot arise | — |
| `lr-reader/src/collections.rs:209` | `unreachable!("quick target without quick")` | catalog collections | safe: `Target::Quick` is only inserted for the Quick Collection row, which always sets `data.quick`; both are built together | — |
| `lr-reader/src/stacks.rs:196` | `unreachable!("rows without a photo id have a reason")` | catalog stacks | safe: `reason` is set whenever `image` is `None` (first match arm), so `(None, None)` cannot arise | — |
| `lr-reader/src/smart.rs:162,237,286` | `self.expect(byte)` | catalog smart-collection text | not a panic: `expect` is a parser method returning `Result<_, SmartParseError>` | — |
| `lc-writer/src/albums.rs` `collect` | recursion over `children` without a depth limit | LightCraft `albums.list` answer | **fix**: depth limit `MAX_ALBUM_DEPTH`, deeper answers are a protocol error instead of a stack overflow | `albums.rs` tests `deeply_nested_answer_is_rejected`, `nesting_up_to_the_limit_is_accepted` |
| `lc-writer/src/apply.rs` `collect_keywords` | recursion over `children` without a depth limit | LightCraft `keyword.list` answer | **fix**: depth limit `MAX_KEYWORD_DEPTH` | `apply.rs` tests `deeply_nested_keyword_answer_is_rejected`, `keyword_nesting_up_to_the_limit_is_accepted` |

### Indexing, slicing and arithmetic

| File:line | Finding | Data source | Decision | Test |
| --- | --- | --- | --- | --- |
| `lr-reader/src/collections.rs:193` | `data.collections[i].kind` | catalog | safe: `i` comes from the index map built over `data.collections` | — |
| `lr-reader/src/collections.rs:207` | `data.collections[i].members.push(p)` | catalog | safe: same index map as above | — |
| `lr-reader/src/collections.rs:563,575,579` | `&path[cycle_from..]`, `path[below]`, `path[i]`, `path[i+1]` | catalog parent chain | safe: `path` is a non-empty walk up the chain; `cycle_from`/`below` are derived from its length and the `last` status is only set after at least one step | — |
| `lr-reader/src/keywords.rs:326` | `chain[chain.len() - 1]` | catalog keyword rows | safe: `chain` is seeded with the current row, so its length is at least 1 | — |
| `lr-reader/src/keywords.rs:290` | `String::from_utf8_lossy` for blob names | catalog keyword/synonym names | **fix**: invalid UTF-8 is a skip (`NameInvalid`) with a reason, never replaced by U+FFFD | see lossy strings below |
| `model/src/lib.rs:271` | `&self.path[self.path.len() - 1]` | model keyword (built by reader) | safe: `Keyword::new` rejects an empty path; `parent` only slices when `len() > 1` | — |
| `model/src/lib.rs:290` | `self.path[..self.path.len() - 1]` | model keyword | safe: guarded by `len() > 1` | — |
| `lc-writer/src/apply.rs:891` | `held.len() + page.ids.len() >= page.total` | LightCraft `catalog.query` answer | safe: both are lengths of in-memory vectors, bounded by memory; the sum cannot wrap in practice | — |
| `lc-writer/src/apply.rs:1264` | `params["paths"][0]` | locally built import params | safe: `serde_json` indexing by name/index returns `Null` for missing, it never panics; the value is built one line above with exactly one path | — |
| `lc-writer/src/apply.rs:1696` | `&items[0]` | LightCraft answer | safe: the guarded pattern is `Some(items) if items.len() == 1` | — |
| `lc-writer/src/live.rs` summary loop | `held.len()` / `offset` arithmetic | LightCraft `catalog.query` answer | safe: offsets and lengths are in-memory counts; the loop stops when `ids` is empty or a page is short | — |
| `lc-writer/src/live.rs:240` | `source[name]` | LightCraft `photo.inspect` answer | safe: `serde_json` indexing returns `Null` for missing keys instead of panicking | — |
| `lr-reader/src/catalog.rs:257-279` | `raw[0]`, `raw[1]`, `&raw[2..]` | catalog `AgLibraryRootFolder.absolutePath` | safe: guarded by `raw.len() >= 2`; only ASCII bytes are pushed as `char` | — |
| `lr-reader/src/xmp.rs` `find_start_tag`, `next_element`, `decode_entities` | slices `&text[a..b]` | catalog XMP blob | safe: offsets come from `find`/`str::char_indices` and are always char boundaries; the blob is capped by `MAX_XMP_BYTES` | — |
| `lr2lc/src/dry_run.rs:440-442` | `[0; Rating::MAX as usize + 1]` and `counts[usize::from(rating)]` | model rating | safe: the array length is the constant maximum + 1; the index is a `Rating` (0..=5) | — |

### Integer casts

| File:line | Finding | Data source | Decision | Test |
| --- | --- | --- | --- | --- |
| `lc-writer/src/apply.rs:384` | `as_u64()? as usize` for `total` | LightCraft `catalog.query` answer | **fix**: `usize::try_from`, a value that does not fit is a protocol error | `apply.rs` tests `total_too_large_for_usize_is_a_protocol_error` |
| `lc-writer/src/live.rs:185` | `as_u64()? as usize` for `total` | LightCraft `catalog.query` answer | **fix**: `usize::try_from` | `live.rs` test `total_too_large_for_usize_is_a_protocol_error` |
| `lr-reader/src/keywords.rs:256` | `*f as i64` for a real id | catalog `AgLibraryKeyword` / variables table | **fix**: same range guard as `collections.rs` (`f.abs() < 9.0e15`); out-of-range reals are no id | `keywords.rs` test `out_of_range_real_is_not_an_id` |
| `lr-reader/src/collections.rs:623` | `f as i64` | catalog collection ids | safe: guarded by `f.fract() == 0.0 && f.abs() < 9.0e15` | — |
| `lr-reader/src/photos.rs:393` | `*v as i64` | catalog rating | safe: guarded by `(0.0..=5.0).contains(v)`; integer 0..=5 | — |
| `lc-writer/src/live.rs:197` | `as u8` for a rating | LightCraft answer | safe: filtered by `*r <= 5` first | — |
| `lc-writer/src/smart.rs:81` | `n as u64` for a star count | model smart rule | safe: guarded by `n.fract() == 0.0 && (0.0..=5.0).contains(&n)` | — |
| `lr-reader/src/catalog.rs:262,273` | `u8 as char` | catalog path bytes | safe: only ASCII bytes reach these lines (`is_ascii_alphabetic`, `is_ascii_alphanumeric` and a fixed set) | — |
| `lr-reader/src/smart.rs:321` | `c as u32` | catalog smart-rule text | safe: `char` to `u32` is lossless | — |
| `lr-reader/src/xmp.rs:32` | `out.len() as u64` | catalog XMP blob | safe: widening `usize` to `u64` on all supported targets | — |
| `lr2lc/src/report_file.rs:233` | `bytes.len() as u64` | report file | safe: widening `usize` to `u64` | — |
| `lr2lc/src/dry_run.rs:440` | `Rating::MAX as usize` | constant | safe: `u8` constant widened to `usize` | — |

### Lossy text conversions

| File:line | Finding | Data source | Decision | Test |
| --- | --- | --- | --- | --- |
| `lr-reader/src/catalog.rs:153` | `String::from_utf8_lossy` for `Adobe_DBVersion` | catalog | safe: the result is only compared to the exact supported version; invalid bytes can never match and only affect the error text | — |
| `lr-reader/src/keywords.rs:290` | `String::from_utf8_lossy` for keyword and synonym names | catalog | **fix**: decode strictly; invalid UTF-8 skips the keyword/synonym with `NameInvalid` and a reason ("not valid UTF-8"), so a name is never altered | `keywords.rs` tests `invalid_utf8_keyword_name_is_skipped`, `invalid_utf8_synonym_name_is_skipped`, `valid_utf8_blob_name_is_kept_byte_for_byte` |
| `lc-writer/src/plan.rs:550` | `to_string_lossy` for the import path | photo path from the model (originally catalog) | **fix**: `import_params` returns `Option`; a non-UTF-8 path is never lossily sent, the step is skipped (`PathNotUtf8`, already emitted by `plan`) | `plan.rs` test `import_params_rejects_a_non_utf8_path` |
| `lr2lc/src/args.rs:162` | `to_string_lossy` for `--port` | command line | **fix**: `to_str()` with an explicit "not valid UTF-8" error | `args.rs` test `non_utf8_port_is_an_error` |
| `lr2lc/src/report_file.rs:186` | `to_string_lossy().to_lowercase()` | report/script paths | safe: Windows-only case-insensitive comparison; a replacement character can only make two different paths compare equal, which refuses the write (conservative), never overwrites the wrong file | — |

## Out of scope

- `crates/lc-test-support/src/*` — test-only crate, used by integration tests.
- Every `unwrap`/`expect`/`panic!` inside `#[cfg(test)]` modules.
- The generator under `tests/fixtures/`.

## Verification

Filled in after the fixes: `cargo fmt --all`, `cargo clippy --workspace
--all-targets -- -D warnings`, `cargo test --workspace`.
