# Colour-label names per language

Research for the `--label-set auto` detection (task T04). `auto` recognises
the standard colour-label names that Adobe Lightroom Classic stores by
default for a locale. Matching is **exact**, byte for byte: no case folding,
no trimming, no Unicode normalisation. A decomposed form does not match.

Only names that can be **cited** from a public Adobe source go into the code
(official Adobe documentation or Adobe-published language resources). A name
without a citation does not go into the code; this file records what was
attempted and what is still open.

## Names in use (English, German)

These sets are already in the code (`crates/model/src/label.rs`):

| Colour  | English | German      |
|---------|---------|-------------|
| red     | Red     | Rot         |
| yellow  | Yellow  | Gelb        |
| green   | Green   | Grün (NFC)  |
| blue    | Blue    | Blau        |
| purple  | Purple  | Lila        |

The English and German names predate T04 and were not added from a freshly
retrieved URL; they are the long-standing default sets. T04 raises the bar
for any *new* language: a citable Adobe source is required before a name
enters the code.

## French and Spanish (T04) — pending a citable source

The goal of T04 is to extend `auto` with the standard French and Spanish
names. The candidate names are the widely reported Lightroom Classic
defaults:

| Colour  | French (candidate) | Spanish (candidate) |
|---------|--------------------|---------------------|
| red     | Rouge              | Rojo                |
| yellow  | Jaune              | Amarillo            |
| green   | Vert               | Verde               |
| blue    | Bleu               | Azul                |
| purple  | Violet             | Púrpura             |

**These candidates are NOT in the code.** They are recorded here as
candidates only, because no public Adobe source could be retrieved from
this environment to cite them. Per the task rule ("a name without a
citation does not go into the code; do not guess spellings or accents"),
they are left out until a source is on file. The Spanish purple name in
particular (`Púrpura` versus a possible alternative) is exactly the kind
of accent/spelling detail that must not be guessed.

### Sources attempted (retrieval date 2026-10-29)

The following Adobe-published pages were attempted and could not be
retrieved from this environment. The seat has no general web access; the
documentation servers (Context7, gitmcp, MDN, DeepWiki) do not carry
Lightroom Classic UI strings, and Adobe Helpx blocks automated fetches.

| URL | Result |
|-----|--------|
| `https://helpx.adobe.com/lightroom-classic/how-to/organize-photos-with-color-labels.html` | HTTP 403 Forbidden (WebFetch); not retrievable (gitmcp) |
| `https://helpx.adobe.com/fr/lightroom-classic/userguide.html` | HTTP 403 Forbidden (WebFetch); not retrievable (gitmcp) |
| `https://helpx.adobe.com/es/lightroom-classic/userguide.html` | HTTP 403 Forbidden (WebFetch); not retrievable (gitmcp) |

A code search of `adobe/lightroom-classic-sdk` on GitHub for the localised
strings returned no matches.

### What would settle it

A citable Adobe source for each name. The canonical Adobe-published
language resource is the `TranslatedStrings_<LOCALE>.txt` file shipped
inside the Lightroom Classic application bundle (an Adobe-published
language resource, not a web URL). If the owner can paste the five label
lines for `fr` and `es` from a Lightroom Classic install (names and field
names only — no personal data), or point to an Adobe URL that an
authenticated fetch can read, the cited names drop into
`LabelSet::standard_sets()` and the `auto` chain in one step, and the
collision test (`no_standard_name_maps_to_two_colors`) and the per-language
tests below cover them automatically.

## Collision guard

`LabelSet::standard_sets()` returns every standard set that `auto`
recognises (English and German today). The test
`no_standard_name_maps_to_two_colors` in `crates/model/src/label.rs` walks
every name across all standard sets and fails if any name maps to two
different colours. Adding a cited French or Spanish set to
`standard_sets()` extends the guard automatically; no name may mean two
colours across the whole table.
