# Icon geometry

The 28 icons the shell draws are the reference's own sprite, flattened once,
offline, into `crates/takofy-ui/src/icon/data.rs`. There is no SVG parser in the
binary: the runtime turns a committed polyline into an `egui::Shape`.

**The generator is committed.** It is the `takofy-ui` example
`crates/takofy-ui/examples/icons_flatten.rs`. The conversion rules live here, so
the table can be regenerated or audited with the example rather than by hand.

## Source

`docs/reference/main.js`, the `SPRITE` array: 34 `<symbol>` elements, of which
28 are in scope. The 6 left out (`mic chevron-down heart-filled download check
verified`) have no consumer; `verified` would also need a literal `#fff`
stroke.

## Conversion

- **Subpaths.** Every `M` starts a new subpath and every `z` closes the current
  one. A `d` may hold several subpaths (`i-pause` is two `M…z` runs); they are
  kept apart, because joining them would tessellate a bow-tie.
- **Commands.** `M m L l H h V v A a Z z`, including implicit repeats and the
  implicit lineto after `M`. A number has at most one decimal point, so
  `-2.6.7` is two numbers (`i-library` relies on this).
- **Arcs** (`A`/`a`) use the SVG endpoint-to-centre parameterisation and are
  flattened at `π/8` radians per segment, large-arc and sweep flags honoured.
- **`rect rx`** becomes a sampled rounded rectangle; **circles and ellipses**
  become 64-sided polygons. `egui::CornerRadius` is a `u8`, which would round a
  `rx` of 1.5, so rects are not handed to egui as rects.
- **Fill versus stroke is routed per element.** `fill="currentColor"` becomes a
  convex fill piece, `stroke="currentColor"` becomes a polyline, and an element
  may be one or the other (`i-screen` and `i-queue` mix both inside one symbol).
  A per-element `stroke-width` overrides the size's own (`i-search` carries
  `stroke-width="2"`).
- **Rounding.** Coordinates are rounded to three decimals. `stroke` widths are
  kept in the sprite's 24-unit space; the drawn width is
  `width × side / 24`, so it scales with the icon size.

## The fill pieces

A fill is one `Shape::convex_polygon` per piece, so the tessellator feathers its
edge. Three fills are concave and are decomposed by hand into convex pieces
that overlap by at least **2 grid units** and stay inside the shape's union; the
overlap stops the feathered edges of abutting pieces from seaming:

| Glyph | Pieces |
| --- | --- |
| `i-logo` | the top bar, and the stem extended 2 units up into it |
| `i-home` | the roof block, and two legs whose tops reach 2 units into it; the gap between them is the door |
| `i-volume`, `i-volume-low`, `i-volume-mute` | the speaker cone, and a box extended 2 units into the cone |

## The drift hash

Each glyph records `source`, the FNV-1a 64 of the exact `<symbol id="…">…</symbol>`
text it was flattened from (`offset basis 0xcbf29ce484222325`, prime
`0x100000001b3`). A test reads `docs/reference/main.js` and recomputes it, so a
change to the reference fails the test and says to regenerate. The hash covers
the **source**, not the output: it does not catch a hand-edited coordinate, only
drift in the reference. The convexity and bounds invariants do run on the output.

## What the runtime adds

The reference strokes its icons with `stroke-linecap: round; stroke-linejoin:
round`; epaint squares its caps and mitres its joins. `icon.rs` therefore adds a
radius-`width/2` disc at each free end of an open line (`cap_discs`) and replaces
sharp interior vertices with a short arc (`round_corners`). Both are pure and
applied at draw time, not baked into the data, so the same polyline can be
stroked at the size's own width.

## Regenerating

Adding a glyph bootstraps: the example is in this same package, so
`cargo run --example` cannot build until the crate compiles, and the crate
needs the new `Icon` variant and an arm for it in `data.rs`. Add the variant
and a stub arm first, then run the example to fill the arm in, then format:

```sh
cargo run -p takofy-ui --example icons_flatten
cargo fmt --all
```

It rewrites `crates/takofy-ui/src/icon/data.rs`, and the formatter keeps the
table in the repository's style.
