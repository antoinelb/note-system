#let meta(id: none, type: none, created: none, tags: (), origin: none) = {
  let parts = ()
  if id != none { parts.push([#id]) }
  if type != none { parts.push([#type]) }
  if origin != none { parts.push([from #origin]) }
  if tags.len() > 0 { parts.push(tags.map(t => "#" + t).join(" ")) }
  if parts.len() > 0 {
    block(
      width: 100%,
      stroke: (bottom: 0.5pt + gray),
      inset: (bottom: 4pt),
      below: 1.2em,
      text(size: 8pt, fill: gray, parts.join([ · ])),
    )
  }
}

#let l(id) = text(fill: purple.darken(20%), [#id])

#let note(doc) = {
  set page(width: 14cm, height: auto, margin: 1.5cm)
  set text(size: 11pt)
  doc
}
