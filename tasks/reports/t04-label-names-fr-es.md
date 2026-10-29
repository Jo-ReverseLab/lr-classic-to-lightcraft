# t04-label-names-fr-es report

Status: partial (names pending a citable Adobe source — see Questions)
Branch: glm/t04-label-names-fr-es
Last commit: <filled on commit>
Base: 229a09a (glm/t01-auto-label-set, "Add report for t01-auto-label-set")
Attempt: 1 of 3

## Goal

Extend `--label-set auto` with the standard French and Spanish colour-label
names of Lightroom Classic, each name cited from a public Adobe source, with
a collision test over the whole table and per-language tests.

## What changed

- `crates/model/src/label.rs` — added `LabelSet::entries()` (the exact stored
  name→colour pairs, no normalisation) and `LabelSet::standard_sets()` (every
  standard set `auto` recognises, in the order it consults them: English and
  German today). Added the required collision test
  `no_standard_name_maps_to_two_colors`: it walks every name across all
  standard sets and fails if any name maps to two colours. The guard
  auto-extends to a future French or Spanish set added to `standard_sets()`.
- `docs/research/label-names.md` (new) — the research document. Records the
  English/German table already in use, the French/Spanish **candidate** names
  (marked unverified), the Adobe sources attempted with their retrieval
  result and date, the rule that uncited names do not enter the code, and
  what would settle each name.
- `README.md` — the `--label-set` section now states that French and Spanish
  are not in `auto` yet (pending a citable Adobe source) and points to the
  research document; mentions the collision guard.
- `CHANGELOG.md` — notes that `auto` still recognises only English and
  German, that French/Spanish are pending citation, and records the collision
  guard and the new accessors.

## What is NOT in this attempt

**No French or Spanish name was added to the code.** The task rule is
explicit: "Only names that you can cite from a public source … A name
without a citation does not go into the code. Do not guess spellings or
accents; matching is exact." No Adobe source could be retrieved from this
environment (see Verification), so the candidate names stay out. The `auto`
chain in `crates/lr2lc/src/labels.rs` is unchanged.

## Acceptance

- **Table** — in `docs/research/label-names.md` (English/German in use;
  French/Spanish candidates marked unverified).
- **Research document with sources** — `docs/research/label-names.md` lists
  the Adobe URLs attempted and the retrieval result, dated 2026-10-29.
- **Collision test** — `no_standard_name_maps_to_two_colors` in
  `crates/model/src/label.rs`, future-proof for fr/es.
- **Per-language tests** — the existing English/German tests in
  `labels.rs` and `label.rs` cover the sets in use; French/Spanish
  per-language tests are **not** added because there are no cited names to
  test yet. They will be added together with the cited names.
- **README and CHANGELOG** — updated.

## Verification

- `cargo fmt --check` — clean.
- `cargo check` / `cargo clippy` / `cargo test` — **could not run locally.**
  The workspace pulls build scripts that need a C toolchain (`cc`), which is
  not installed in this seat. CI on this branch is the proof; the seat token
  cannot read Actions runs or PRs from here (403).
- **Adobe source retrieval failed from this environment.** Attempted
  (`WebFetch` and the gitmcp URL fetcher, 2026-10-29):
  - `https://helpx.adobe.com/lightroom-classic/how-to/organize-photos-with-color-labels.html` → HTTP 403 / not retrievable;
  - `https://helpx.adobe.com/fr/lightroom-classic/userguide.html` → HTTP 403 / not retrievable;
  - `https://helpx.adobe.com/es/lightroom-classic/userguide.html` → HTTP 403 / not retrievable.
  - A code search of `adobe/lightroom-classic-sdk` on GitHub for the localised strings returned no matches.
  The documentation servers available here (Context7, gitmcp, MDN, DeepWiki)
  do not carry Lightroom Classic UI strings, and the seat has no general web
  search.
- Design verified by reasoning: `entries()` and `standard_sets()` are pure
  additions over the existing `LabelSet`; the collision test uses only
  `HashMap` (already imported) and the new accessors. No existing behaviour
  changes.

## Deviations from the task

- **French and Spanish names left out.** The task's goal is to extend `auto`
  with fr/es, but the citation rule and the absence of any retrievable Adobe
  source from this environment make adding them a guess. The task itself
  prescribes the escape hatch: "If you cannot find a source for a name, leave
  it out and list it under 'Questions' in your report." Done.
- **No per-language fr/es tests.** They depend on the cited names; added
  together with them in a later attempt.
- **`auto` chain unchanged.** `crates/lr2lc/src/labels.rs` still consults
  English then German directly. When cited fr/es sets are added to
  `standard_sets()`, the `auto` chain must be extended at the same time (the
  `standard_sets()` doc comment says so). Kept apart in this attempt to
  avoid touching working code with no new names to show for it.

## Questions

- **French names.** Candidate set: Rouge, Jaune, Vert, Bleu, Violet. Needs a
  citable Adobe source (URL, or the five label lines from a Lightroom
  Classic `fr` install — names only, no personal data).
- **Spanish names.** Candidate set: Rojo, Amarillo, Verde, Azul, Púrpura.
  The Spanish purple name (`Púrpura` vs a possible alternative) must be
  confirmed from a source; it is exactly the accent/spelling detail the task
  says not to guess. Needs the same kind of citation as French.
- If the owner supplies either source, attempt 2 adds the cited names to
  `LabelSet::standard_sets()` and the `auto` chain, plus the per-language
  tests.

## Risks / things the reviewer should look at first

- **No compile or test run.** CI is the first real proof. The change is small
  and purely additive, so the likely failure surface is a typo in the new
  methods or test.
- **`standard_sets()` is `pub` and currently used only by the collision
  test.** It is the intended extension point for cited fr/es sets; review
  whether a `pub` accessor is acceptable, or whether it should be
  `pub(crate)` until fr/es land.
- **The candidate fr/es names in the research doc are not verified.** They
  must not be copied into the code without a citation, even if they look
  right.
