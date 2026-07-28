// Prose typography per design/wireframes-v0.md § Typography, at 1px = 0.75pt:
// body 15px → 11.25pt, sheet title 26px → 19.5pt, meta line 9px → 6.75pt.
// Colour literals are values from the palette's light column — typst pages
// are white, and templates cannot consume the app's CSS variables
// (adr/2026-07-theme-attribute-on-app-root.md).
#let meta(id: none, type: none, created: none, tags: (), origin: none) = {
  let parts = ()
  if id != none { parts.push([#id]) }
  if type != none { parts.push([#type]) }
  if origin != none { parts.push([from #origin]) }
  if tags.len() > 0 { parts.push(tags.map(t => "#" + t).join(" ")) }
  if parts.len() > 0 {
    block(
      width: 100%,
      stroke: (bottom: 0.5pt + rgb("#d0cdda")),
      inset: (bottom: 4pt),
      below: 1.2em,
      text(
        font: "DejaVu Sans Mono",
        size: 6.75pt,
        fill: rgb("#8b87a0"),
        parts.join([ · ]),
      ),
    )
  }
}

#let l(id) = text(fill: rgb("#6b5fa8"), [#id])

#let note(doc) = {
  set page(width: 14cm, height: auto, margin: 1.5cm)
  set text(font: "Libertinus Serif", size: 11.25pt, fill: rgb("#45415a"))
  set par(leading: 0.75em)
  show heading.where(level: 1): set text(size: 19.5pt, weight: 600)
  doc
}
