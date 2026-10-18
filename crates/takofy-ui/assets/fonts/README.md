# Bundled fonts

Three voices, all under the SIL Open Font License, with each licence's full text
beside the faces it covers:

- **Archivo** carries the interface.
- **JetBrains Mono** carries every number and field label.
- **Noto Sans CJK** catches everything the other two cannot draw. It is only ever
  a fallback.

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
position. Every file here carries a single `wght` axis, so the app registers one
face per weight by sharing the same blob at different coordinates.

Archivo's `wdth` axis is pinned at 100 and dropped because the design never varies
width, which saves 396 KB. Its default instance is wght 600, so an embed that sets
no coordinates draws Semibold.

## The CJK fallback, and what it costs

| file | size | glyphs |
|---|---|---|
| NotoSansJP-Variable.ttf | 9.15 MB | 16,732 |
| NotoSansKR-Variable.ttf | 9.93 MB | 23,174 |
| NotoSansSC-Variable.ttf | 16.95 MB | 30,890 |
| NotoSansTC-Variable.ttf | 11.39 MB | 20,745 |

47.4 MB in total, which is the real cost of not drawing boxes. egui has no system
font fallback — it draws only the faces it is handed — so an unhandled script is
tofu, and an artist's name is exactly where a rare character turns up.

The regions overlap far more than the sizes suggest. Measured against each face's
cmap:

| character | JP | KR | SC | TC |
|---|---|---|---|---|
| `漢` | yes | yes | yes | yes |
| `あ` kana | yes | yes | yes | yes |
| `가` hangul | — | yes | — | — |
| `汉` simplified | — | — | yes | yes |

Only Korean carries hangul, and only the Chinese faces carry simplified forms. So
the four are not four independent alphabets; they are four repertories with a
large shared middle, which is why trimming one is tempting and why keeping all
four is the cautious choice.

**A missing glyph cannot be detected by measuring it.** Every one of these four
gives `.notdef` an advance of one full em — 1.000 units at 14 pt — which is the
same width as a real ideograph. Tofu is therefore invisible to layout, and the
only defence is a fallback chain that covers the language. Anything that needs to
know whether a glyph really exists has to read the cmap, not the rendered width.

## Provenance

- **Archivo** — the official variable font from google/fonts
  (`ofl/archivo/Archivo[wdth,wght].ttf`), with the width axis pinned:

      fonttools varLib.instancer --update-name-table 'Archivo[wdth,wght].ttf' wdth=100

- **JetBrains Mono** — `ofl/jetbrainsmono/JetBrainsMono[wght].ttf` from
  google/fonts.

- **Noto Sans CJK** — `ofl/notosansjp`, `notosanskr`, `notosanssc` and
  `notosanstc` from google/fonts. All four carry the same licence file, which is
  Adobe's: these are Source Han Sans derivatives and the reserved name is
  `Source`. Nothing here renames them, so that reservation does not apply.

## The other two faces

Archivo has 653 glyphs — latin, latin-ext, punctuation and arrows. JetBrains Mono
has 1363 and adds Cyrillic and Greek. Neither has CJK, which is what the Noto
faces are for; they are registered last in the family so they are reached only
when the two above them come up empty.
