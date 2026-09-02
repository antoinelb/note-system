// Prose typography from design/wireframes-v0.md § Typography at 1px =
// 0.75pt, bumped one step for reading comfort in the hybrid editor
// (adr/2026-07-reading-scale-bumped.md): body 18px → 13.5pt, title
// 32px → 24pt, meta line 9px → 6.75pt. The body size is now a compile
// input rather than a frozen number, so the editor's textarea and the
// rendered page always agree on one size instead of just agreeing at
// today's default (adr/2026-08-one-font-size-for-source-and-render.md);
// the title and meta line keep their ratio to it (24/13.5, 6.75/13.5)
// rather than becoming inputs of their own.
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

// The app's one font-size dial, in CSS pixels converted to points at the
// same 1px = 0.75pt this file already uses; vanilla typst (make
// check-vault, exports) gets the pre-input body size as the default
// (adr/2026-08-one-font-size-for-source-and-render.md).
#let base-size = sys.inputs.at("size", default: 13.5)

// `due` is the day a note's work is owed; the app's open-loops list reads
// it, and the meta line says it so the page agrees with the list
// (adr/2026-09-course-type-and-due-loops.md).
#let meta(
  id: none,
  type: none,
  created: none,
  tags: (),
  origin: none,
  due: none,
) = {
  let parts = ()
  if id != none { parts.push([#id]) }
  if type != none { parts.push([#type]) }
  if origin != none { parts.push([from #origin]) }
  if due != none { parts.push([due #due]) }
  if tags.len() > 0 { parts.push(tags.map(t => "#" + t).join(" ")) }
  if parts.len() > 0 {
    block(
      width: 100%,
      stroke: (bottom: 0.5pt + palette.hairline),
      inset: (bottom: 4pt),
      below: 1.2em,
      text(
        font: "DejaVu Sans Mono",
        size: base-size * (6.75 / 13.5) * 1pt,
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
    // no font named: Cormorant Garamond has no ✓, the fallback chain does
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
  set text(font: "Cormorant Garamond", size: base-size * 1pt, fill: palette.ink)
  set par(leading: 0.75em)
  show heading.where(level: 1): set text(
    size: base-size * (24 / 13.5) * 1pt,
    weight: 600,
  )
  // A quote in a note is always its own full-width block; the inline form
  // has no place in prose here, so every #quote is promoted before the
  // show rule below ever sees it.
  set quote(block: true)
  // `> ` at a paragraph's start is the stored quote syntax: the editor no
  // longer expands it into `#quote(...)` on Enter, so vanilla Typst reads
  // the literal `>` a note keeps on disk. The check reads the paragraph's
  // own child sequence, the same way `show list.item` below reads item
  // children, and mirrors what the deleted `editor::quote_completion`
  // enforced: an empty/whitespace-only body stays an ordinary paragraph,
  // and a trailing ` _Attribution_` splits off only when it is plain text,
  // non-empty, free of its own `_`, and leaves a non-empty body behind —
  // otherwise the attribution stays part of the body. Once the marker is
  // gone from the first child, the rewrite's own output never starts with
  // `"> "` again, so this cannot re-fire on what it just produced.
  show par: it => {
    let c = if it.body.func() == [].func() { it.body.children } else {
      (it.body,)
    }
    let first = c.at(0, default: none)
    let first-text = if first != none {
      first.at("text", default: none)
    } else { none }
    // Typst only folds "> " into one text child when plain text follows
    // it; the moment inline markup (emphasis, a link, ...) sits right
    // after the marker, the tokenizer splits it into a bare ">" child
    // and a separate space child instead, so a plain starts-with("> ")
    // check misses it. Recognise that split shape too — first child is
    // exactly ">", second is a lone space — and slice past both.
    let (after-marker, rest) = if (
      first-text != none and first-text.starts-with("> ")
    ) {
      (first-text.slice(2), c.slice(1))
    } else if first-text == ">" and c.at(1, default: none) == [ ] {
      ("", c.slice(2))
    } else {
      (none, ())
    }
    if after-marker == none {
      it
    } else {
      let is-blank(parts) = parts.all(
        part => part.func() == text and part.text.trim() == "",
      )
      let last = if rest.len() > 0 { rest.at(rest.len() - 1) } else { none }
      // An attribution candidate requires a literal space right before
      // its opening `_`, matching `trailing_attribution`'s exact `" _"`
      // substring search. That space survives parsing as its own child
      // between the body and the closing emphasis, so `rest` needs the
      // space plus the emphasis at minimum — a bare "> _Attr_" paragraph,
      // where the marker's own space already stands in for it, has no
      // such child and is a body, never an attribution.
      let has-space-before-emph = (
        rest.len() >= 2 and rest.at(rest.len() - 2) == [ ]
      )
      let candidate = if (
        has-space-before-emph and last != none and last.func() == emph
      ) {
        let inner = last.at("body", default: none)
        let tail = if inner != none {
          inner.at("text", default: none)
        } else { none }
        if tail != none and tail.trim() != "" and not tail.contains("_") {
          tail.trim()
        } else { none }
      } else { none }
      let stripped-first = text(after-marker)
      // The attribution is only accepted when what is left of the body
      // (everything but the trailing emphasis) is not itself blank —
      // matching `trailing_attribution`'s `!quote.is_empty()` guard.
      // Otherwise the whole `rest`, attribution emphasis included, falls
      // back to being the quote body.
      let accepted = if candidate != none {
        let reduced = (stripped-first,) + rest.slice(0, rest.len() - 1)
        if is-blank(reduced) { none } else { (candidate, reduced) }
      } else { none }
      if accepted != none {
        let (attribution, quote-parts) = accepted
        quote(
          block: true,
          attribution: [#attribution],
        )[#quote-parts.join()]
      } else {
        let quote-parts = (stripped-first,) + rest
        if is-blank(quote-parts) {
          it
        } else {
          quote(block: true)[#quote-parts.join()]
        }
      }
    }
  }
  // Quotes use a non-colour rule as well as the muted palette role, so the
  // boundary survives every theme and greyscale reproduction. Spacing was
  // 0pt under per-line fragments (the pane supplied the gaps); regions
  // compile consecutive quotes together, where flush blocks collide (text
  // edges are cap-height to baseline, so a descender hangs into the next
  // quote's caps) and typst's 1.2em default reads too airy — 0.9em is the
  // per-line look's own gap, the old fragment pages' 6pt + 6pt margins at
  // the default body size (adr/2026-08-quote-spacing-returns-to-default.md).
  show quote.where(block: true): it => block(
    width: 100%,
    above: 0.9em,
    below: 0.9em,
    inset: (left: 8pt),
    stroke: (left: 0.5pt + palette.muted),
  )[
    #it.body#if it.attribution != none {
      h(0.35em)
      emph(it.attribution)
    }
  ]
  // `- [ ]` renders as an open task circle, `- [x]` as a done one with the
  // text struck; in markup the brackets are plain text, so the match reads
  // the item's leading children (adr/2026-07-checklist-rendering.md). A
  // nested `- [ ]` under a task item is not wrapped in a child list by
  // typst: it arrives as further list.item children trailing the label
  // in the same body sequence, so they are split off and indented one
  // step rather than joined into the struck label
  // (adr/2026-08-nested-checklist-indentation.md).
  show list.item: it => {
    let c = if it.body.func() == [].func() { it.body.children } else {
      (it.body,)
    }
    let bracket(i, ch) = c.at(i, default: none).at("text", default: none) == ch
    if bracket(0, "[") and bracket(2, "]") and (c.at(1) == [ ] or c.at(1) == [x]) {
      let done = c.at(1) == [x]
      let tail = c.slice(3)
      let split = tail.position(child => child.func() == list.item)
      let (label, nested) = if split == none {
        (tail, ())
      } else {
        (tail.slice(0, split), tail.slice(split))
      }
      let rest = label.join()
      // plain content, not a rebuilt list.item: the bullet marker goes
      // with it, so the circle takes the marker's place. Nested items
      // pass back through this same show rule, so deeper levels indent
      // cumulatively without any recursion of our own; they are never
      // struck or muted by a done parent.
      block[#box(check(done)) #if done {
          text(fill: palette.muted, strike(rest))
        } else { rest }
        #if nested.len() > 0 {
          block(inset: (left: 1em), nested.join())
        }]
    } else { it }
  }
  doc
}
