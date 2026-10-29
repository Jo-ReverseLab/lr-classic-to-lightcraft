# T15 — The reader decides the smart-collection verdict

**Branch:** `glm/t15-smart-verdict`
**Base:** `origin/main` (7f36890)
**Status:** done (unverified — no local compiler, CI is the proof)
**Attempt:** 1 of 3

## Problem

`crates/lr2lc/src/dry_run.rs` rewrote report entries after the fact by matching the
prefix `"collection <id>:"` through `Report::convert_skipped` and
`Report::replace_skipped`. That contradicts the rule in `crates/model/src/skip.rs`:
a skip code is set **where the reason arises**, never derived from the reason text.

The verdict (is a smart collection translatable?) was computed in two places that
could diverge: the CLI's `convert_smart` and the writer's `translate_rule`.

## Change

**Single source of truth in the model.** `SmartRule::translatable()` and
`SmartRules::verdict()` in `crates/model/src/smart.rs` now decide whether a smart
collection can be converted and, if not, name the offending criterion with the
skip code `SmartUntranslatable` (or `SmartNoCriteria` when there are no rules,
`SmartUntranslatable` for an unknown combine mode).

```rust
impl SmartRule {
    pub fn translatable(&self) -> bool {
        self.criterion == "rating"
            && self.value2.is_none()
            && self.units.is_none()
            && matches!(self.operation.as_str(), "==" | ">=")
            && matches!(
                self.value,
                Some(SmartValue::Number(n)) if n.fract() == 0.0 && (0.0..=MAX_STARS).contains(&n)
            )
    }
}

impl SmartRules {
    pub fn verdict(&self) -> Result<(), (SkipCode, String)> { /* ... */ }
}
```

**Reader books the verdict.** `crates/lr-reader/src/collections.rs` no longer
relies on the CLI. Smart collections are booked in a deferred pass **after** the
row loop, once every collection is known, so the parent check ("parent must be a
converted collection set") can see the final state. Convertible smart collections
are collected in the new `CollectionData::convertible_smart` field, exposed so the
CLI does not recompute the verdict.

`smart_verdict(s, collections)` returns `Ok` or `Err(Skipped)`:

- `SmartRules::verdict()` failure → `SmartUntranslatable` / `SmartNoCriteria`;
- parent missing or not a converted folder → `ParentNotSet`.

**Writer delegates.** `translate_rule` (`crates/lc-writer/src/smart.rs`) now starts
with `if !rule.translatable() { return None; }`, reusing the model predicate, so
the reader and writer cannot disagree.

**CLI no longer edits the report by text.** `crates/lr2lc/src/dry_run.rs`:
`convert_smart` is deleted; the converted set is taken straight from
`collection_data.convertible_smart`. The "which smart ids were not converted" set
is derived by subtracting `convertible_smart` from `smart_ids`.

**Removed.** `Report::convert_skipped` and `Report::replace_skipped`
(`crates/model/src/lib.rs`) and their tests — nothing else used them.

## Files

- `crates/model/src/smart.rs` — `translatable()`, `verdict()`, tests.
- `crates/lc-writer/src/smart.rs` — `translate_rule` delegates to `translatable()`.
- `crates/lr-reader/src/collections.rs` — `convertible_smart` field, deferred
  smart booking, `smart_verdict()`.
- `crates/lr2lc/src/dry_run.rs` — `convert_smart` deleted; reads
  `convertible_smart`; test renamed `reader_produces_the_smart_verdict`.
- `crates/model/src/lib.rs` — `convert_skipped` / `replace_skipped` removed.
- `crates/lr-reader/tests/smart_rules.rs`, `crates/lr-reader/tests/smart_bool.rs`
  — doc + test wording updated to the new reason text.

## Acceptance

- **No reason-text matching remains.** Grep proof:
  `grep -rn "convert_skipped\|replace_skipped" crates/` → no matches (exit 1).
  `grep -rn "fn convert_smart" crates/` → no matches (exit 1).
- **Verdict comes from the reader.** `reader_produces_the_smart_verdict` (fixture
  based) asserts `convertible_smart == [101]`, the report is balanced, 101 is not
  skipped, and 102's reason is `collection 102: smart collection is not converted
  (criterion "pick" has no exact LightCraft equivalent)`.
- **Existing smart-collection tests pass (by design).** `convertible_smart` holds
  the same ids `[101, 201, 202, 203]` the old `convert_smart` produced, so the CLI
  balance test in `apply_smart_cli.rs` (`collections.smart == 4`, balance formula)
  and the translate-fixture tests are unchanged in their numbers.
- **Old output was wrong in one place.** The reason text used the placeholder
  phrase "first criterion"; the new text names the actual criterion
  (`criterion "pick" has no exact LightCraft equivalent`). This is the only
  visible difference and it is the intended fix. Tests that asserted the old
  phrase now assert the new one and that the old phrase is gone.
- **read = converted + skipped still holds.** The deferred booking uses
  `report.converted()` / `report.push_skipped()` exactly as the album/folder pass
  does, and every test asserts `report.is_balanced()`.

## Layering

Reader → model → writer is preserved: `SmartRules::verdict()` lives in the model;
the reader calls it; the writer calls the model's `translatable()`. The reader
does not depend on the writer.

## Not verified locally

`cargo fmt --check` passes. `cargo check` / clippy / tests need a C compiler
(`cc`), which is not installed here, so they are left to CI.
