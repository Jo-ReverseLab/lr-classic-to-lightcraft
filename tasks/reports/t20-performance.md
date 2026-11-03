# t20-performance report

Status: ready for review
Branch: agent/t20-performance
Base: agent/t11-reconcile (09b9eeb)
Commits: 9077dbf (reader query), 0a45d5d (copies and HashSet)

## Environment

- Raspberry Pi 5, Linux, NVMe storage, release build
  (`cargo test --release -p lr2lc --test performance -- --ignored --nocapture`).
- Every measurement uses a generated catalog (`write_performance_catalog_with_files`,
  proportional shape, deterministic seed) in a fresh temporary directory. No real
  catalog is read.
- The generated schema has no indexes (see `tests/fixtures/README.md`). A catalog
  without indexes is valid input, so the reader itself had to stop being
  quadratic; no fixture index was added.

## Baseline

| Photos | Folders | Keywords | Collections | Catalog | Generate | Dry run (1st) | Dry run (2nd) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 10 000 | 500 | 1 000 | 20 | 2.4 MiB | 0.7 s | 2.8 s | 2.8 s |
| 100 000 | 5 000 | 10 000 | 200 | 24.5 MiB | 6.2 s | 306.8 s | 300.4 s |

Both runs were measured on this branch before any change. The 100 000 run
grows by a factor of about 108 for 10 times the photos, i.e. quadratic. The
500 000 dry run was not completed before the fix: extrapolating the quadratic
curve gives roughly 2.1 hours for one run, so it was deliberately not run.

## Cause

`crates/lr-reader/src/photos.rs`, `Catalog::photo_data` read the photo rows in one
statement but fetched four extra values with a **correlated scalar subquery per
photo**:

```sql
(SELECT c.caption   FROM AgLibraryIPTC c WHERE c.image = i.id_local ORDER BY c.id_local LIMIT 1)
(SELECT c.copyright FROM AgLibraryIPTC c WHERE c.image = i.id_local ORDER BY c.id_local LIMIT 1)
(SELECT a.xmp       FROM Adobe_AdditionalMetadata a WHERE a.image = i.id_local ORDER BY a.id_local LIMIT 1)
(SELECT n.value     FROM AgHarvestedIptcMetadata h LEFT JOIN ... WHERE h.image = i.id_local ORDER BY h.id_local LIMIT 1)
```

Without an index on the `image` column, each subquery scans its whole table for
every photo, so the read costs O(photos × rows of the ancillary table) instead of
O(photos + rows). On the generated performance catalog only `AgLibraryIPTC` (empty)
and `Adobe_AdditionalMetadata` (one row per photo) exist, so the baseline executes
`1 + 3 × photos` statements; a real catalog with the creator tables would
execute `1 + 4 × photos`.

## Fix

`photo_data` now reads the photo rows and joins each ancillary table **once**:

```sql
LEFT JOIN (SELECT image AS iptc_image, min(id_local) AS iptc_row
           FROM AgLibraryIPTC GROUP BY image) ic ON ic.iptc_image = i.id_local
LEFT JOIN AgLibraryIPTC c ON c.id_local = ic.iptc_row
```

and analogously for `Adobe_AdditionalMetadata` and the creator tables. Each
derived table is grouped by `image` and keeps the smallest `id_local`, which is
exactly the row the old `ORDER BY id_local LIMIT 1` chose. Caption and copyright
still come from the same row. The optional tables are only joined when their
columns exist, as before; a missing table still yields `NULL`. The statement
count per reader run drops from `1 + 3N` / `1 + 4N` to **1**.

Semantics that are unchanged and covered by tests:

- duplicates: smallest `id_local` wins, per image (including a NULL value or a
  non-text value in the smallest row winning over a later useful one);
- blob decoding of `xmp` for the title, and the `title_unreadable` list;
- `NULL` and empty text counting as absent for caption, copyright and creator;
- row order (`ORDER BY i.id_local`) and every report count;
- the balance `read = converted + skipped`.

New unit tests in `crates/lr-reader/src/photos.rs` pin the smallest-row rule for
caption, copyright, creator and xmp on the existing generated fixtures. The
existing `captions_report`, `rights_report`, `titles_report` and the golden
report tests are unchanged and pass.

## Result

| Photos | Dry run before | Dry run after | Speed-up |
| --- | --- | --- | --- |
| 10 000 | 2.8 s / 2.8 s | 0.2 s / 0.3 s | ≈ 14× / ≈ 9× |
| 100 000 | 306.8 s / 300.4 s | 2.1 s / 2.0 s | ≈ 145× / ≈ 150× |
| 500 000 | not run (≈ 2.1 h extrapolated) | 13.0 s | — |

The 500 000 run was not attempted before the fix (the quadratic curve makes it
take hours). After the fix it takes one minute including generating the
catalog, so it was measured for real instead of extrapolated. Its shape is five
times the 100 000 catalog: five times the photos, twice the folders, five times
the keywords and collections (the generator keeps the proportions).

Timing scales linearly from 100 000 to 500 000 photos (five times the photos,
6.2 times the time; the keyword and collection counts grow as well).

Peak memory, sampled from `/proc/<pid>/status` (`VmHWM`) while `lr2lc` runs
(`/usr/bin/time` is not installed here):

| Photos | Peak RSS after |
| --- | --- |
| 100 000 | 280 MiB |
| 500 000 | 1.30 GiB |

The baseline peak was not sampled (sampling was set up for the after runs
only); its in-memory model is the same, so no large difference is expected.

SQL statements per reader run (excluding the constant schema check at open):

| Catalog | Before | After |
| --- | --- | --- |
| performance catalog (creator tables absent) | 1 + 3N | 1 |
| real catalog (creator tables present) | 1 + 4N | 1 |

## Other changes

- `crates/lr2lc/src/dry_run.rs`: the unreadable-title ids are collected into a
  `HashSet` once instead of a linear `Vec` scan for every photo (quadratic only
  when almost every title is unreadable; output-identical).
- `crates/lr2lc/src/writer.rs`, `main.rs`: `writer::input` now takes the converted
  elements by value and `main` moves them out of the dry-run outcome with
  `std::mem::take`. Previously every photo, keyword and keyword assignment was
  cloned a second time before planning; on 500 000 photos that is a large
  avoidable memory copy. Output-identical.

## Live writer: control requests per photo

A live `--apply` run sends, on the read side (`read_live`):

| Request | Count |
| --- | --- |
| `catalog.query` for all library photos | `ceil(P / 200)` |
| `photo.inspect`, one per mapped photo the library still has | `N` (about 1 per photo) |
| `albums.list` | 1 |
| `keyword.list` | 1 |
| `catalog.query` per mapped album | `sum ceil(members / 200)` |

A first apply run has no known photos, so `needs_live` is false and `read_live` is
skipped entirely; a reconciliation run pays the `photo.inspect` call once per
photo.

On the write side (`plan` → `apply`):

| Step | Count |
| --- | --- |
| new photo: `library.import` | 1 per photo |
| virtual copy: `photo.virtualCopy` | 1 per copy |
| changed known photo: `library.select` | 1 per photo |
| changed field: `photo.rate` / `photo.flag` / `photo.label` / `photo.setMeta` | 0–4 per changed photo |
| new keyword: `keyword.create` | 1 per missing keyword |
| album/folder: `album.create` | 1 per new collection |
| membership: `library.select` + `album.addPhotos`/`album.removePhotos` | 2 per changed album |
| stack: `stack.group` | 1 per changed stack |
| quick collection | at most 2 per run |
| smart album: `album.createSmart` | 1 per new smart album |

So a run costs about **1 request per new photo** (the import) and about
**2 + changed fields per reconciled photo**, plus one request per 200 photos for
pagination. Bundling opportunities (deliberately **not** implemented here, this
is a design decision for the LightCraft interface):

- `photo.inspect` one per photo → a batch inspect, or the missing three text
  fields in the `catalog.query` summary, would turn `N` into `ceil(N / B)`.
- `library.import` already accepts a `paths` array; importing in batches would
  turn `N` into `ceil(N / B)`. The result shape and the 60 s timeout still need
  checking against a running app.
- `library.select` before every photo's updates: the update commands already
  carry explicit `ids`; one selection per batch (or none, if the commands really
  address ids) would remove one request per changed photo.
- `keyword.create` one per keyword → a batch create (about `N / 10` keywords on
  a proportional catalog).
- `catalog.query` pages are already batched at 200; a larger page size would
  reduce the page count linearly.

## Deliberately not changed

- **Fixture indexes.** Adding an index to the generated schema would hide the
  reader's own query shape. The fixture README forbids it.
- **Live-writer batching.** The per-photo control requests above are a
  correctness/design matter of the control port; changing them is a separate
  task. They are listed here, not implemented.
- **Small superlinear spots that are not on the measured path:** the stack
  planner's `gids.iter().any` and `same_members` are O(members²) but stacks are
  small; `plan_photos` checks keyword removals with `contains` per keyword;
  `albums.rs` filters listed nodes with `claimed.contains` once per node. None
  of them is quadratic in the number of photos on the measured catalog.
- **`write_performance_catalog_with_files` generation time** (placeholder files)
  is outside the dry run and is printed separately by the test.
