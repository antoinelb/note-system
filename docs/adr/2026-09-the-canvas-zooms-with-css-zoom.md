# The canvas zooms with CSS `zoom`, not a scale transform

## Context

The table's canvas was scaled by `transform: scale(s) translate(pan)` (`adr/2026-08-body-zoom-scale-and-metrics.md`, kept by `adr/2026-09-the-table-zooms-continuously.md`).
Zoomed far in, the card text went blurry: under a GPU-composited WebKitGTK session a transformed layer is rasterised at scale 1 and the compositor stretches that bitmap, and WebKitGTK's coordinated backing stores do not repaint at the transform's scale.
The blur does not reproduce under the e2e harness, which runs Xvfb with compositing disabled and a software renderer — both a `transform: scale(3)` and a `zoom: 3` box render crisp text there, which is why the harness cannot guard this.

## Decision

- **The canvas is scaled by `zoom: s` and only translated by the transform**: `zoom: {s}; transform: translate(pan)`.
  With `zoom` the layer is laid out at the scale, so every glyph is rasterised at the size it shows and nothing is stretched.
- **The geometry is unchanged.** A `translate` on a zoomed element is zoomed with it, so screen = s·(canvas + pan) still holds, the pan stays in canvas units and `point()` still divides client coordinates by the scale once.
  A probe against the installed WebKitGTK confirmed it: `zoom: 3; transform: translate(10px, 10px)` puts a 176-wide card at exactly the rect `transform: scale(3) translate(10px, 10px)` does.
- **A card's own offset coordinates now arrive in screen pixels**, where a transformed card reported them in its untransformed local space.
  The same probe showed `offsetX` on a zoomed element measured in zoomed pixels from the padding edge.
  The one reader is the card's wheel handler, which now takes the pane point as `s·(corner + pan) + offset` instead of `s·(corner + offset + pan)`; the void's presses land on the pane itself, which is not zoomed, so `pane_origin` and `canvas_point` are untouched.
- `transform-origin: 0 0` leaves the canvas rule: a translate has no origin to orbit.

## Rejected

- **Keeping the transform and asking for a sharper raster** (`will-change`, `translateZ(0)`, `backface-visibility`).
  Those promote the layer to the compositor, which is where the stretching happens; WebKitGTK has no property that repaints a layer at its transform's scale.
- **Scaling every card's `left`, `top`, `width` and font size by hand.** What `zoom` does, reimplemented across every rule that names a length.
- **Leaving it.** The zoom exists to read titles in a corner of the map; blurry titles defeat it.

## Not verified here

The fix rests on the probe's geometry and on the cause named above; the blur itself was only observed on the user's GPU session and cannot be reproduced under Xvfb.
If the text still blurs after this change, the cause is elsewhere and the transform can return without touching any coordinate math but the wheel's.
