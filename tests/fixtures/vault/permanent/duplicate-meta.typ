#import "/templates/template.typ": *
#show: note
#meta(
  id: "duplicate-meta",
  type: "concept",
  created: "2026-07-23",
)
#meta(
  id: "duplicate-meta-bis",
  type: "idea",
  created: "2026-07-23",
)

= Two meta calls

Edge case: the parser must pick a policy (first wins?) without crashing.
