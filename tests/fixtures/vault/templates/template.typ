// Prose typography from design/wireframes-v0.md § Typography at 1px =
// 0.75pt, bumped one step for reading comfort in the hybrid editor
// (adr/2026-07-reading-scale-bumped.md): body 18px → 13.5pt, title
// 32px → 24pt, meta line 9px → 6.75pt.
// The app compiles with a `theme` input and gets a transparent page and the
// matching palette column; vanilla typst (make check-vault, exports) gets
// the paper look (adr/2026-07-note-rendering-theme-input.md). Colour
// literals live here because templates cannot consume the app's CSS
// variables (adr/2026-07-theme-attribute-on-app-root.md); the dark column
// mirrors assets/theme.css.
// The in-app columns keep bare margins because the app renders per-block
// fragments that stack in a pane with its own padding — the source
// textarea and the rendered text share a left edge; paper keeps real page
// margins.
#let palette = (
  paper: (
    page: white,
    margin: 1.5cm,
    ink: rgb("#45415a"),
    muted: rgb("#8b87a0"),
    hairline: rgb("#d0cdda"),
    link: rgb("#6b5fa8"),
    done: rgb("#4a8a6a"),
  ),
  light: (
    page: none,
    margin: 6pt,
    ink: rgb("#45415a"),
    muted: rgb("#8b87a0"),
    hairline: rgb("#d0cdda"),
    link: rgb("#6b5fa8"),
    done: rgb("#4a8a6a"),
  ),
  dark: (
    page: none,
    margin: 6pt,
    ink: rgb("#c9c4dd"),
    muted: rgb("#6f6a8c"),
    hairline: rgb("#332c52"),
    link: rgb("#8f84c9"),
    done: rgb("#6fb08c"),
  ),
).at(sys.inputs.at("theme", default: "paper"))

#let meta(id: none, type: none, created: none, tags: (), origin: none) = {
  let parts = ()
  if id != none { parts.push([#id]) }
  if type != none { parts.push([#type]) }
  if origin != none { parts.push([from #origin]) }
  if tags.len() > 0 { parts.push(tags.map(t => "#" + t).join(" ")) }
  if parts.len() > 0 {
    block(
      width: 100%,
      stroke: (bottom: 0.5pt + palette.hairline),
      inset: (bottom: 4pt),
      below: 1.2em,
      text(
        font: "DejaVu Sans Mono",
        size: 6.75pt,
        fill: palette.muted,
        parts.join([ · ]),
      ),
    )
  }
}

#let l(id) = text(fill: palette.link, [#id])

// The task circle: open = stroked outline, done = filled with a check
// (adr/2026-07-checklist-rendering.md).
#let check(done) = box(
  width: 0.85em,
  height: 0.85em,
  baseline: 0.15em,
  radius: 50%,
  stroke: if done { none } else { 0.75pt + palette.muted },
  fill: if done { palette.done } else { none },
  align(center + horizon, if done {
    // no font named: Parisienne has no ✓, the fallback chain does
    text(fill: white, size: 0.6em, "✓")
  }),
)

#let note(doc) = {
  set page(
    width: 14cm,
    height: auto,
    margin: palette.margin,
    fill: palette.page,
  )
  set text(font: "Parisienne", size: 13.5pt, fill: palette.ink)
  set par(leading: 0.75em)
  show heading.where(level: 1): set text(size: 24pt, weight: 600)
  // `- [ ]` renders as an open task circle, `- [x]` as a done one with the
  // text struck; in markup the brackets are plain text, so the match reads
  // the item's leading children (adr/2026-07-checklist-rendering.md)
  show list.item: it => {
    let c = if it.body.func() == [].func() { it.body.children } else {
      (it.body,)
    }
    let bracket(i, ch) = c.at(i, default: none).at("text", default: none) == ch
    if bracket(0, "[") and bracket(2, "]") and (c.at(1) == [ ] or c.at(1) == [x]) {
      let done = c.at(1) == [x]
      let rest = c.slice(3).join()
      // plain content, not a rebuilt list.item: the bullet marker goes
      // with it, so the circle takes the marker's place
      block[#box(check(done)) #if done {
          text(fill: palette.muted, strike(rest))
        } else { rest }]
    } else { it }
  }
  doc
}
