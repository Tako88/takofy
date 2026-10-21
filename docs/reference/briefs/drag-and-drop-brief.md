# Drag and drop — what to draw

A brief for the designer. No code, no implementation detail. It says what the
app already looks like, what the pointer is about to be able to do, and which
pictures are missing.

---

## 1. The app, in one paragraph

Takofy is a desktop music player for Windows and Linux, built to the reference in
`docs/reference`. Near-black, two fonts, one green. The window is a shell of six
regions: a topbar with a search field, a narrow sidebar column on the left, a
large main pane, a Now Playing pane on the right that carries the queue, and a
play bar across the bottom. Every colour, size, weight, spacing step and duration
comes from the token layer — the reference's own `tokens.css` is the source, and
nothing in the app is allowed to invent a value.

## 2. What is being added

Dragging rows with the mouse. Five lists get it:

| List | Where | Row height | A row holds |
| --- | --- | --- | --- |
| Playlists | sidebar | 52pt | cover tile, name, one line of detail |
| Playlist / album / search tracks | main pane | 44pt | number, art, title, artist, album, added, duration, like, more |
| Queue | Now Playing pane | 48pt | art, title, artist, duration |
| Search results | main pane | 44pt (same table) | as above |

And one gesture crosses between them: a track dragged out of the main pane and
dropped into the queue at a chosen position.

Rows can also be moved several at a time — shift-click and ctrl-click select a
block, and dragging any selected row carries the whole block.

## 3. The vocabulary that already exists

Anything new has to sit in this, not beside it.

**Surfaces** (the elevation ladder is deliberately short — no large block may read
as grey):

```
--color-window          #000000   behind everything
--color-surface         #070708   a panel
--color-surface-raised  #121215   popups, menus
--color-surface-hover   #1c1c20   under the pointer
--color-surface-sunken  #000000   wells reading deeper than the window
--color-panel           #0b0b0d   the Now Playing pane
--color-border          bone at 11%
--color-border-strong   bone at 26%
```

**Text**: `--color-text` #f2f0ea, `--color-text-muted` #b0aca4,
`--color-text-subtle` #83807a.

**The one rule that will fight you.** The token file says it plainly: *"Green is
a functional signal here — it means 'sound is coming out' and nothing else. It is
not the brand colour, so it never paints a surface."* `--color-accent` #1ed760 is
already spent on three things and only three: the sounding row's 2pt bar at its
left margin, the play glyph, and the focused field's border. A drop indicator and
a selected row are both new claims on attention, and neither is sound. So either
they use something other than green, or the rule gets an exception written into
the token file. That decision is yours to make, and it should be made
deliberately rather than by picking the colour that looks best on the day.

**Radii**: 0, 1, 2, 3, 4, and pill. **Rule widths**: a hairline is 1pt, a marker
is 2pt — those are the only two weights that exist. **Spacing** is a 4pt scale
(4, 8, 12, 16, 24, 32, 48, 64). **Motion**: 110ms fast, 180ms base, 300ms slow,
ease-out `cubic-bezier(0.3, 0, 0, 1)`. The existing row hover is a 110ms fade of
a bone fill at 6%.

**Three states a row already has**, and the pictures must not be confusable with
them:

- **Hovered** — a bone fill at 6%, fading in over 110ms. Also reveals the row's
  play glyph and like.
- **Focused** (the keyboard is on it) — a focus ring, drawn by the app's one
  focus-ring helper.
- **Playing** — a 2pt accent bar at the row's left edge; the title stays bone.

## 4. The pointer's journey, so the pictures have an order

1. Press on a row and hold.
2. Move a few pixels — nothing has happened yet, and it must not: a sloppy
   double-click is still a double-click.
3. Past the threshold, the row is picked up.
4. Dragging over other rows — something shows where it would land.
5. Dragging over a place it cannot go — something shows that too.
6. Near the top or bottom of a long list, the list starts to scroll while the
   pointer holds still.
7. Release on a valid place — the row moves.
8. Release on a refused place, or press Escape — nothing moves, and the app says
   so or says nothing, your call.
9. Dropping a track onto a playlist in the sidebar *adds* it there; dropping a
   sidebar playlist onto another position in the sidebar *reorders* the sidebar.
   Same pointer, same place, two meanings. They must not be mistakable.

## 5. The pictures to draw

Each is one frame. A recommendation is given so nothing starts from a blank page
— disagree with it freely, that is the point of the exercise.

**D1 — A selected row.**
One row, no pointer near it. Then three rows selected consecutively, with the
pointer sitting on the middle one, so hover and selection can be seen at once.
Then the same, with one of the selected rows being the one that is playing — the
accent bar and the selection fill have to coexist.
*Recommendation*: selection is `--color-surface-hover` as a solid fill (the token
is already documented as "the fill under the pointer, and the selected row"), the
accent bar of the playing row sitting inside it unharmed. Problem: hover is a 6%
bone and selection would be an 11% surface — at a glance those may be too close,
so this is exactly the pair to look at side by side and rule on.

**D2 — A row being dragged.**
The row it came from, while the drag is in progress: still there, dimmed? gone
entirely? And the thing under the pointer: a copy of the whole row at
`--color-surface-raised`? A smaller pill with just the title? A grey silhouette?
*Recommendation*: the source stays in place at reduced opacity (or a hollow
outline) so you can see what you are moving out of; the ghost is the row itself,
raised, with a 1pt border and the menu shadow, absolutely positioned under the
pointer and offset so its own row does not hide the place it will land.

**D3 — Dragging several.**
Same as D2 but with a block of, say, seven rows selected. Does the ghost show
seven rows (impossible for forty), one row, or one row plus a count?
*Recommendation*: one row plus a small count chip ("7") on its trailing edge,
because the block can be longer than the screen and a stack of ghosts lies about
its size.

**D4 — The insertion indicator.**
The line, rule, or gap that says where the row will land. Between two rows, at
the very top of the list, and at the very bottom. Say how wide it is — the row's
full width, the content's width, or inset by the row's own padding — and whether
it is a 1pt hairline, a 2pt marker, or a gap that opens.
*Recommendation*: a 2pt line spanning the list's inner width, in whatever colour
D1 settles on for selection (they are the same kind of claim: "this is where the
thing goes"), with the row on either side not moving. A gap that opens makes
every row below jump while you drag, which is nauseating at speed.

**D5 — A refused target.**
Dragging a track over a playlist that cannot be edited, or over a region that
takes no drops at all. Does the ghost change, does the line vanish, does a small
"not here" mark appear?
*Recommendation*: the ghost loses its border and drops to the app's disabled
text colour, no line appears, and nothing else on screen changes. Silence plus a
dead ghost is honest; a red cross is loud for something this common.

**D6 — The cross-region drop.**
A track from the main pane, dragged across the gap into the Now Playing pane.
The pane is 320pt or 260pt wide depending on the window. What happens to the
pane's own row list while the ghost is over it — the insertion line between two
queue rows, or something that says "add to the queue here"? And what does the
pane's "Next in queue" caption do — change, highlight, stay put?
*This one matters most*: it is the only gesture that crosses a region boundary,
and the queue is the only list whose rows are a different height from the rows
being dragged.

**D7 — Dropping onto the sidebar.**
The same pointer in the same column, once to *add a track to a playlist* (the
target is a playlist row) and once to *reorder the playlist list* (the target is
a position between two playlist rows). They must be visibly different in the
second before release, or a mis-drop reorders your library when you meant to add
a song.
*Recommendation*: over a playlist row, the row takes the hover fill it already
has and the insertion line is absent; between rows, the line appears and no row
is filled.

**D8 — Autoscroll.**
Dragging to the top or bottom edge of a 1,400-row list. Does anything visible
say the list is scrolling on your behalf? (The list scrolls itself; the question
is whether the user is told.)

**D9 — Narrow window.**
At 1120px and below the Now Playing pane — and therefore the queue — is not
drawn at all. So at that width there is nowhere to drop a track to queue it.
Three answers: leave it (queuing is a wide-window ability), add a drop target
that is not the queue, or draw something else. Say which, or say that this is
out of scope.

**D10 — Escape and cancel.**
Mid-drag, Escape is pressed. What does the return to rest look like — does
anything animate, or is it instant?
*Recommendation*: instant. A 180ms flight home reads as the app doing something
you did not ask for.

## 6. Constraints

- Dark only. There is no light theme and no plan for one.
- No touch. This is a desktop with a mouse; there are no long-press or swipe
  variants.
- No new colours, spacing steps, radii or durations without a token. If a value
  is needed twice, it is a token, and it belongs in the token file with the
  reasoning written next to it, in the style of the file already there.
- Green stays a signal for sound unless you rule otherwise and write the
  exception into the token file (see §3).
- The drop indicator cannot move the rows around it, and no state may animate
  for longer than 180ms during a drag — the gesture is continuous, and anything
  slower reads as lag.
- The pictures have to work at six rows and at forty, in a 260pt pane and in a
  1,200pt pane.

## 7. What to hand back

1. A page in the reference — `docs/reference/drag.html` — drawn with the app's
   own classes and tokens, showing D1 through D8 as a state board, each frame
   labelled with the state it is.
2. For each new visual state, either the existing token it reuses or the name of
   the new token it implies, in the file's own style, with the reason in one
   line. That is what gets built; without it the values become my taste.
3. A ruling on green: kept for sound only, or extended to the drop indicator and
   the selection.
4. A sentence on D9 (narrow window) and D10 (cancel), since those can be answered
   in words instead of pixels.
