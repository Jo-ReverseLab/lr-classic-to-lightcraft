# t21-audit: lossy casts, panics and indexing

Status: done — every finding is either fixed test-first on this branch or
justified as safe. No panic remains on catalog, LightCraft or command-line
data.
Branch: `agent/t21-audit`
Base: `agent/t23-ci-proposal` (`b5f440d`, "Correct CI cost arithmetic and audit job status"; contains the T20, T22 and T23 work). The fixes were written on top of `agent/t20-performance` (`3edac05`) and rebased onto T23 before the final run.

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

`serde_json::Value` indexing (`value["x"]`, `value[0]`) never panics: a
missing key or an out-of-range index yields `Value::Null`. Rows below are
listed because they are indexing sites, with the surrounding guard noted.

## Findings

### Panics (`unwrap`, `expect`, `panic!`, `unreachable!`)

| File:line | Finding | Data source | Decision | Test |
| --- | --- | --- | --- | --- |
| `crates/lr2lc/src/output.rs:263` | `.expect("report serializes")` on `serde_json::to_string_pretty` | report built from the model (strings, integer counts, booleans) | safe: serializing this struct cannot fail; no non-string map keys, no non-finite floats | — |
| `crates/lr-reader/src/collections.rs:162` | `unreachable!("own skips are part of the status")` | catalog collections | safe: `resolve` records a collection's own skip in `status` before the member check, so the `(own Err, member Ok)` case cannot arise | — |
| `crates/lr-reader/src/collections.rs:209` | `unreachable!("quick target without quick")` | catalog collections | safe: `Target::Quick` is only inserted for the Quick Collection row, which always sets `data.quick`; both are built together | — |
| `crates/lr-reader/src/stacks.rs:196` | `unreachable!("rows without a photo id have a reason")` | catalog stacks | safe: `reason` is set whenever `image` is `None` (first match arm), so `(None, None)` cannot arise | — |
| `crates/lr-reader/src/smart.rs:162,237,286` | `self.expect(byte)` | catalog smart-collection text | not a panic: `expect` is a parser method returning `Result<_, SmartParseError>` | — |
| `crates/lc-writer/src/albums.rs:139` (`collect`) | recursion over `children` without a depth limit | LightCraft `albums.list` answer | **fixed**: depth limit `MAX_ALBUM_DEPTH` (64) at `albums.rs:127`; deeper answers are a protocol error instead of a stack overflow | `albums.rs::tests::deeply_nested_answer_is_rejected`, `albums.rs::tests::nesting_up_to_the_limit_is_accepted` |
| `crates/lc-writer/src/apply.rs:1749` (`collect_keywords`) | recursion over `children` without a depth limit | LightCraft `keyword.list` answer | **fixed**: depth limit `MAX_KEYWORD_DEPTH` (64) at `apply.rs:1747` | `apply.rs::tests::deeply_nested_keyword_answer_is_rejected`, `apply.rs::tests::keyword_nesting_up_to_the_limit_is_accepted` |

No `assert!`, `debug_assert!`, `todo!` or `unimplemented!` remains in
production code (checked file by file up to the test module).

### Indexing, slicing and arithmetic

| File:line | Finding | Data source | Decision | Test |
| --- | --- | --- | --- | --- |
| `crates/lr-reader/src/collections.rs:193` | `data.collections[i].kind` | catalog | safe: `i` comes from the index map built over `data.collections` | — |
| `crates/lr-reader/src/collections.rs:207` | `data.collections[i].members.push(p)` | catalog | safe: same index map as above | — |
| `crates/lr-reader/src/collections.rs:563,575,579` | `&path[cycle_from..]`, `path[below]`, `path[i]`, `path[i+1]` | catalog parent chain | safe: `path` is a non-empty walk up the chain; `cycle_from` and `below` are derived from its length, and `last` is only set after at least one step | — |
| `crates/lr-reader/src/keywords.rs:344` | `chain[chain.len() - 1]` | catalog keyword rows | safe: `chain` is seeded with the current row, so its length is at least 1 | — |
| `crates/lr-reader/src/folders.rs:118` | `b[0]`, `b[1]` for root detection | catalog folder paths | safe: guarded by `b.len() == 1` / `b.len() == 3` | — |
| `crates/lr-reader/src/smart.rs:206,296,314,324,356` | `&self.text[start..self.pos]`, `as_bytes()[start]`, `as_bytes()[self.pos - 1]` | catalog smart-rule text | safe: `pos`/`start` are byte offsets kept on char boundaries; `start` points at an already-read `Some(byte)`, so `start < len`; `[pos - 1]` is only read when the current byte is `+`/`-`, which can only follow a consumed sign or exponent | — |
| `crates/lr-reader/src/xmp.rs:76,78,98-106,122-134` | slices `&s[a..b]`, `&rest[..i]`, `&tail[1..]` | catalog XMP blob | safe: offsets come from `find`, `str::char_indices` or a known ASCII delimiter and are always char boundaries; the blob is capped by `MAX_XMP_BYTES` | — |
| `crates/lr-reader/src/catalog.rs:259-265` | `raw[0]`, `raw[1]`, `&raw[2..]` | catalog `AgLibraryRootFolder.absolutePath` | safe: guarded by `raw.len() >= 2`; only ASCII bytes are pushed as `char` | — |
| `crates/model/src/lib.rs:271` | `&self.path[self.path.len() - 1]` | model keyword (built by the reader) | safe: `Keyword::new` rejects an empty path | — |
| `crates/model/src/lib.rs:290` | `self.path[..self.path.len() - 1]` | model keyword | safe: guarded by `len() > 1` | — |
| `crates/model/src/mapping.rs:119` | `parts[prefix.len()..]` | path mapping, command line | safe: early return when `parts.len() < prefix.len()` | — |
| `crates/model/src/mapping.rs:263,277` | `b[0]`, `b[1]`, `&s[..trimmed.len() + 1]` | path mapping, command line | safe: length checked before indexing; `trimmed.len() < s.len()` and `trimmed.len() + 1 <= s.len()` | — |
| `crates/lc-writer/src/albums.rs:119` | slice pattern `[one]` | LightCraft answer | safe: the pattern only matches a slice of length 1 | — |
| `crates/lc-writer/src/apply.rs:802` | slice pattern `[id]` | LightCraft answer | safe: the pattern only matches a slice of length 1 | — |
| `crates/lc-writer/src/apply.rs:893` | `held.len() + page.ids.len() >= page.total` | LightCraft `catalog.query` answer | safe: both are lengths of in-memory vectors, bounded by memory; the sum cannot wrap in practice | — |
| `crates/lc-writer/src/apply.rs:1282` | `params["paths"][0]` | locally built import params | safe: `serde_json` indexing returns `Null` for missing entries and never panics; the value is built one line above with exactly one path | — |
| `crates/lc-writer/src/apply.rs:1708` | `&items[0]` | LightCraft answer | safe: the guarded pattern is `Some(items) if items.len() == 1` | — |
| `crates/lc-writer/src/live.rs:166` | `held.len() + page.ids.len() >= page.total` | LightCraft `catalog.query` answer | safe: as above; offsets and lengths are in-memory counts, and the loop stops on an empty or short page | — |
| `crates/lc-writer/src/live.rs:242` | `source[name]` | LightCraft `photo.inspect` answer | safe: `serde_json` indexing returns `Null` for a missing key instead of panicking | — |
| `crates/lc-writer/src/plan.rs:333,338` | `&self.skipped[..n]` and `&self.skipped[n..]` | plan state | safe: `n` is clamped with `.min(self.skipped.len())` | — |
| `crates/lc-writer/src/plan.rs:894,897` | `by_gid[gid]` | model global ids | safe: `gid` is taken from `by_gid.keys()` | — |
| `crates/lr2lc/src/dry_run.rs:275,280` | `mapped_photos[i]`, `master_indices[j]` | model photos / mapping | safe: `i` comes from `enumerate` over `mapped_photos`; `j` comes from `collisions`, which returns indices into `master_paths`, built one-to-one from `master_indices` | — |
| `crates/lr2lc/src/dry_run.rs:440-442` | `[0; Rating::MAX as usize + 1]` and `counts[usize::from(rating)]` | model rating | safe: the array length is the constant maximum + 1; the index is a `Rating` (0..=5) | — |

No variable division, modulo or shift remains in production code (only
constant multiplications and no shifts; checked with a scan for `/`, `%`,
`<<` and `>>`). The counters (`Report::read/converted`, the apply summary)
add one per element held in memory, so they cannot overflow before the
process runs out of memory.

### Integer casts

| File:line | Finding | Data source | Decision | Test |
| --- | --- | --- | --- | --- |
| `crates/lc-writer/src/apply.rs:386` | `as_u64()? as usize` (was) for `total` | LightCraft `catalog.query` answer | **fixed**: `usize::try_from`; a value that does not fit is a protocol error | `apply.rs::tests::query_page_total_is_never_truncated` (asserts the 32-bit failure path there) |
| `crates/lc-writer/src/live.rs:187` | `as_u64()? as usize` (was) for `total` | LightCraft `catalog.query` answer | **fixed**: `usize::try_from` | `live.rs::tests::summary_page_total_is_never_truncated` |
| `crates/lr-reader/src/keywords.rs:268` | `*f as i64` for a real id | catalog `AgLibraryKeyword` / `Adobe_variablesTable` | **fixed**: range guard `f.fract() == 0.0 && f.abs() < 9.0e15`, the same as `collections::as_id`; out-of-range reals are no id | `keywords.rs::tests::out_of_range_real_is_not_an_id` |
| `crates/lr-reader/src/collections.rs:623` | `f as i64` | catalog collection ids | safe: guarded by `f.fract() == 0.0 && f.abs() < 9.0e15` | — |
| `crates/lr-reader/src/photos.rs:393` | `*v as i64` | catalog rating | safe: guarded by `(0.0..=5.0).contains(v)` | — |
| `crates/lc-writer/src/live.rs:199` | `as u8` for a rating | LightCraft answer | safe: filtered by `*r <= 5` first | — |
| `crates/lc-writer/src/smart.rs:81` | `n as u64` for a star count | model smart rule | safe: guarded by `n.fract() == 0.0 && (0.0..=5.0).contains(&n)` | — |
| `crates/lr-reader/src/catalog.rs:262,273` | `u8 as char` | catalog path bytes | safe: only ASCII bytes reach these lines (`is_ascii_alphabetic`, `is_ascii_alphanumeric` and a fixed set) | — |
| `crates/lr-reader/src/smart.rs:321` | `c as u32` | catalog smart-rule text | safe: `char` to `u32` is lossless | — |
| `crates/lr-reader/src/xmp.rs:32` | `out.len() as u64` | catalog XMP blob | safe: widening `usize` to `u64` | — |
| `crates/lr2lc/src/report_file.rs:233` | `bytes.len() as u64` | report file | safe: widening `usize` to `u64` | — |
| `crates/lr2lc/src/dry_run.rs:440` | `Rating::MAX as usize` | constant | safe: `u8` constant widened to `usize` | — |

### Lossy text conversions

| File:line | Finding | Data source | Decision | Test |
| --- | --- | --- | --- | --- |
| `crates/lr-reader/src/catalog.rs:153` | `String::from_utf8_lossy` for `Adobe_DBVersion` | catalog | safe: the result is only compared to the exact supported version; invalid bytes can never match, and only the error text could show a replacement character | — |
| `crates/lr-reader/src/keywords.rs:299` | `String::from_utf8_lossy` (was) for keyword and synonym names | catalog | **fixed**: strict `String::from_utf8`; invalid UTF-8 skips the keyword/synonym with `NameInvalid` and the reason "not valid UTF-8", never a U+FFFD name. NULL and empty keep `NameEmpty` | `keywords.rs::tests::invalid_utf8_keyword_name_is_skipped`, `keywords.rs::tests::invalid_utf8_synonym_name_is_skipped`, `keywords.rs::tests::valid_utf8_blob_name_is_kept_byte_for_byte` |
| `crates/lc-writer/src/plan.rs:560` | `to_string_lossy` (was) for the import path | photo path from the model (originally catalog) | **fixed**: `import_params` returns `Option` and uses `to_str()`; a non-UTF-8 path is never lossily sent. The plan already skips such photos with `PathNotUtf8`; `apply` repeats the skip defensively | `plan.rs::tests::import_params_rejects_a_non_utf8_path` |
| `crates/lr2lc/src/args.rs:161` | `to_string_lossy` (was) for `--port` | command line | **fixed**: `to_str()` with an explicit "not valid UTF-8" error | `args.rs::tests::non_utf8_port_is_an_error` (`#[cfg(unix)]`) |
| `crates/lr2lc/src/report_file.rs:186` | `to_string_lossy().to_lowercase()` | report/script paths | safe: Windows-only case-insensitive comparison; a replacement character can only make two different paths compare equal, which refuses the write (conservative), never overwrites the wrong file | — |

Strict `str::from_utf8` sites (`collections.rs:614,636`, `xmp.rs:35`,
`report_file.rs:211`) return `None`/an error on invalid bytes; they do not
change data and are not findings.

## Out of scope

- `crates/lc-test-support/src/*` — test-only crate, used by integration tests.
- Every `unwrap`/`expect`/`panic!` inside `#[cfg(test)]` modules.
- The generator under `tests/fixtures/`.

## Verification

All commands run from the task worktree with a cold target directory. The
branch was rebased onto `agent/t23-ci-proposal` and all three commands were
re-run green on the rebased tree:

- `cargo fmt --all` — clean.
- `cargo clippy --workspace --all-targets -- -D warnings` — clean.
- `cargo test --workspace` — all tests pass, 0 failed, nothing ignored.

Output is unchanged: the golden report and the acceptance tests were not
modified and stay green. The only observable changes are for inputs that
were previously mangled or could abort the run: an invalid-UTF-8 keyword or
synonym name is now skipped with `NameInvalid` instead of being written with
U+FFFD, and a non-UTF-8 path or port is refused with a clear error.

Residual risk: the 32-bit truncation paths in the two `usize::try_from`
fixes cannot be exercised on this 64-bit host; the tests assert them only
when `usize::BITS < 64`. Separately, pagination trusts the local LightCraft
app to honour `offset`: an app that answers every page in full while
reporting an unreachable `total` would keep the reader paging until memory
runs out. That is a denial of service by the paired application, not a
data-integrity problem, and the protocol has no other stop condition.
