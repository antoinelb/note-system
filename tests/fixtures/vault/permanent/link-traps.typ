#import "/templates/template.typ": *
#show: note
#meta(
  id: "link-traps",
  type: "concept",
  created: "2026-07-23",
)

= Link traps

The only real link in this note: #l("zettelkasten").

// #l("in-comment") — inside a comment, does not count
#let trap = "#l(\"in-string\")"
In raw code: `#l("in-raw")`.
