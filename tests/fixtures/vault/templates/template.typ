// Prose typography per design/wireframes-v0.md § Typography, at 1px = 0.75pt:
// body 15px → 11.25pt, sheet title 26px → 19.5pt, meta line 9px → 6.75pt.
// The app compiles with a `theme` input and gets a transparent page and the
// matching palette column; vanilla typst (make check-vault, exports) gets
// the paper look (adr/2026-07-note-rendering-theme-input.md). Colour
// literals live here because templates cannot consume the app's CSS
// variables (adr/2026-07-theme-attribute-on-app-root.md); the dark column
// mirrors assets/theme.css.
// The in-app columns keep tight vertical margins because the app renders
// per-block fragments that stack — half the whole-page paragraph rhythm on
// each side; paper keeps real page margins.
#let palette = (
  paper: (
    page: white,
    margin: 1.5cm,
    ink: rgb("#45415a"),
    muted: rgb("#8b87a0"),
    hairline: rgb("#d0cdda"),
    link: rgb("#6b5fa8"),
  ),
  light: (
    page: none,
    margin: (x: 1.5cm, y: 6pt),
    ink: rgb("#45415a"),
    muted: rgb("#8b87a0"),
    hairline: rgb("#d0cdda"),
    link: rgb("#6b5fa8"),
  ),
  dark: (
    page: none,
    margin: (x: 1.5cm, y: 6pt),
    ink: rgb("#c9c4dd"),
    muted: rgb("#6f6a8c"),
    hairline: rgb("#332c52"),
    link: rgb("#8f84c9"),
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

#let note(doc) = {
  set page(
    width: 14cm,
    height: auto,
    margin: palette.margin,
    fill: palette.page,
  )
  set text(font: "Parisienne", size: 11.25pt, fill: palette.ink)
  set par(leading: 0.75em)
  show heading.where(level: 1): set text(size: 19.5pt, weight: 600)
  doc
}
