# Colour-label names per language

Research for the `--label-set auto` detection. `auto` recognises the
standard colour-label names that Adobe Lightroom Classic stores by default
for a locale. Matching is **exact**, byte for byte: no case folding, no
trimming, no Unicode normalisation. A decomposed form does not match.

Names enter this file only from public documentation or from observing
Lightroom Classic's behaviour (the text it writes into a catalog). Nothing is
read from inside the Lightroom application bundle.

## How Lightroom stores a label

Lightroom stores a label as text in `Adobe_images.colorLabels`: the name the
**active Color Label Set** gives that colour, not a colour code. The active
set's names live in Lightroom's preferences, not in the catalog. The default
set ("Lightroom Default") is translated into the interface language at the
moment it is chosen (or at start-up when no set is stored) and then kept, so
a catalog labelled in German keeps `Rot` … `Lila` even after the interface
is switched to English. Lightroom shows a colour only when the stored text
equals a name of the set that is active now.

The same text goes into XMP as `xmp:Label`. Next to it Lightroom also writes
`photoshop:LabelColor` with a language-independent value (`red` for `Rot`).
The XMP packet cached inside the catalog does not always carry the label, so
lr2lc reads `colorLabels`.

## Observed default names (Lightroom Classic 15.0, macOS, 2026-11-19)

Method: a throwaway copy of a test catalog whose photos all point to a
missing folder (so no image file can be touched), Lightroom started per
language with `-AppleLanguages`, the default Color Label Set chosen in the
Metadata menu, keys 6–9 and the fifth colour entry of the Set Color Label
menu on five photos, then `colorLabels` read from the catalog. All values
are in NFC.

| Colour  | English | German | French | Spanish  | Italian |
|---------|---------|--------|--------|----------|---------|
| red     | Red     | Rot    | Rouge  | Roja     | Rossa   |
| yellow  | Yellow  | Gelb   | Jaune  | Amarilla | Gialla  |
| green   | Green   | Grün   | Vert   | Verde    | Verde   |
| blue    | Blue    | Blau   | Bleu   | Azul     | Blu     |
| purple  | Purple  | Lila   | Violet | Púrpura  | Viola   |

Spanish and Italian use the feminine forms (`Roja`, `Amarilla`, `Rossa`,
`Gialla`). The candidates this file listed before (`Rojo`, `Amarillo`) were
wrong, which is why names are observed rather than guessed.

Not observed: Japanese and every other language Lightroom ships. Pitfall
when observing: the Color Label Set menu lists "Bridge Default" first; the
default set is the entry named after Lightroom. "Review Status" gives status
names, not colours.

## In the code

`crates/model/src/label.rs` has the English and German sets
(`LabelSet::en()`, `LabelSet::de()`), and `auto` recognises those two.
French, Spanish and Italian are observed but not in the code yet. Adding a
set to `LabelSet::standard_sets()` and the `auto` chain in
`crates/lr2lc-core/src/labels.rs` is the follow-up. `Verde` is green in both
Spanish and Italian, so the two sets share a name without a clash.

## Collision guard

`LabelSet::standard_sets()` returns every standard set that `auto`
recognises. The test `no_standard_name_maps_to_two_colors` in
`crates/model/src/label.rs` walks every name across all standard sets and
fails if any name maps to two different colours, so a new set cannot
silently clash with an existing name.
