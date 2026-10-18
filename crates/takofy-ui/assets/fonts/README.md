# Bundled fonts

Archivo carries the interface; JetBrains Mono carries every number and field
label. Both are under the SIL Open Font License and neither declares a Reserved
Font Name, so the full licence text for each ships beside the faces.

## Why not the reference's own files

`docs/reference/fonts/` holds woff2, and egui does not read woff2. epaint hands
the bytes to skrifa, which rejects the container outright:

    Error parsing "..." TTF/OTF font file: Invalid sfnt version 0x774F4632

`0x774F4632` is "wOF2". There is no fallback to another font; it panics. So the
app needs sfnt tables, which is what these are.

The reference's files are also Google's latin-only subset, 230 glyphs. `ō` is not
among them, so the reference's own demo text ("Hana Sō") already draws that glyph
from a fallback in the prototype.

## Variable, not one file per weight

egui instances a variable font rather than ignoring its axes:
`FontData::variation_axes` reports them and `FontTweak::coords` selects a
position. Both files here carry a single `wght` axis, so the app registers one
face per weight by sharing the same blob at different coordinates.

Two files of 262 KB and 187 KB cover every weight the design uses, against 1.37 MB
for static faces at the same weights. Archivo's `wdth` axis is pinned at 100 and
dropped because the design never varies width, which alone saves 396 KB.

## Provenance

- **Archivo** — the official variable font from google/fonts
  (`ofl/archivo/Archivo[wdth,wght].ttf`), with the width axis pinned:

      fonttools varLib.instancer --update-name-table 'Archivo[wdth,wght].ttf' wdth=100

  Its default instance is wght 600, so an embed that sets no coordinates draws
  Semibold.

- **JetBrains Mono** — `ofl/jetbrainsmono/JetBrainsMono[wght].ttf` from
  google/fonts.

## Coverage, and the hole in it

Archivo has 653 glyphs: latin, latin-ext, punctuation and arrows. JetBrains Mono
has 1363 and adds Cyrillic and Greek. Neither has CJK, and egui has no system font
fallback — it draws only the faces it is handed. A Japanese, Korean or Chinese
artist name is boxes until that is decided (#15).
