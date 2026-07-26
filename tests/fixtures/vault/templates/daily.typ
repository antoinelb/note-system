#import "/templates/template.typ": *
#show: note
#meta(
  id: "{{id}}",
  type: "daily",
  created: "{{created}}",
  tags: (),
)

= {{id}}

#l("{{prev}}") | #l("{{next}}")

== Quotes

== Tasks

- [ ]

== Notes
