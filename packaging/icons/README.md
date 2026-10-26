# The application icon

`takofy.svg` is the source. `hicolor/<size>x<size>/apps/takofy.png` are rendered
from it, so a desktop can pick the size it wants instead of scaling.

The mark is the app's own logo: the `i-logo` glyph, whose two polygons are in
`crates/takofy-ui/src/icon/data.rs` as `LOGO_FILLS`, drawn in the sprite's
24-unit box. The icon scales that box to 72% of a 512 tile and centres it, then
colours it from the theme: the tile is the raised surface (`#121215`) and the
mark is the bone text colour (`#f2f0ea`). If the logo ever moves, this file
moves with it, and the sprite's drift hash is what says so.

## Regenerating

`rsvg-convert` (librsvg) is the only tool needed:

```
cd packaging/icons
rsvg-convert -w 512 -h 512 takofy.svg -o /tmp/check.png   # look at it first
for size in 16 24 32 48 64 128 256 512; do
    rsvg-convert -w "$size" -h "$size" takofy.svg -o "hicolor/${size}x${size}/apps/takofy.png"
done
```

Install the tree into `/usr/share/icons/hicolor/` as it is. The sizes that carry
a PNG are the ones listed above; 16 and 32 are the ones a window list actually
uses, so look at those before believing the 512.
