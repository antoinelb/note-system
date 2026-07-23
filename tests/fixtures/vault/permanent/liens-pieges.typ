#import "/templates/template.typ": *
#show: note
#meta(
  id: "liens-pieges",
  type: "concept",
  created: "2026-07-23",
)

= Liens pièges

Seul lien réel de cette note : #l("zettelkasten").

// #l("dans-commentaire") — dans un commentaire, ne compte pas
#let piege = "#l(\"dans-chaine\")"
Dans du code verbatim : `#l("dans-raw")`.
