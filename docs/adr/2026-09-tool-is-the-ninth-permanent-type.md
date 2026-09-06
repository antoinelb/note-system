# `tool` is the ninth permanent type

## Context

The permanent set has been eight types since `adr/2026-09-a-course-is-a-project.md` removed `course`: person, organisation, source, concept, claim, idea, personal, project.
Nothing in it names *the thing you work with*.
A program, a method, a physical instrument — a solver, the Zettelkasten method itself, a soldering iron — is written down for what it does and how it is used, not for what it asserts or who wrote it.
Filed as a `source` it lies: a source is something read, and a tool's note is not a reading note.
Filed as a `concept` it lies the other way: a concept is knowledge, and the note being written is a procedure.
That mismatch is what the `course` removal warned against — a type earns its row only when it carries something no existing type carries — and here it does: the note's whole content is *how to use this*, which none of the eight is about.

## Decision

**`tool` is the ninth permanent type**, deliberately generic: anything that helps do work — a program, a method, a physical instrument.
`NoteType::Tool` joins `from_name`/`as_name` and `is_permanent`; it is the last row of `create::TYPES`, so it comes last in the Ctrl+N picker and last in the filter overlay, where the eight kept their order.
It gets `--type-tool` in both themes and `.card.bar-tool` on the table, so a tool card wears its own bar like every other permanent type — the label beside the bar still names the type, so the hue is never the only encoding (AIR INP-3).
The hue is the green the eight left unused: dark `#609f80`, light `#4da378` — `hsl(150, 25%, 50%)` and `hsl(150, 36%, 47%)`, exactly the mute and the light-side inversion `adr/2026-08-light-table-colours-derived.md` set for `--type-person` (`#a0765f` / `#a36b4d`).
The eight sit at 21°, 40°, 66°, 193°, 228°, 247°, 269° and 310°, and the one wide gap on the wheel is the 127° between `source` (olive) and `claim` (cyan).
150° splits it: 84° from `source`, 43° from `claim`, where the tightest existing pairs are 19° apart.

**`templates/tool.typ` is the minimal template**: the meta block, the title, `== Purpose`, `== Notes`.
Purpose is what the type is for — what work this helps do — and notes is where the use accumulates.
It is the first permanent template with sections; the other eight are a meta block and a title, because their shape is prose from the first line, while a tool note is answered in two places.

## Alternatives rejected

- **Fold tools into `source`** — a source is something consumed for what it says, and the index treats it that way: a lecture links its project, a book is read once and cited. A tool is used repeatedly and its note is a procedure, not a citation; the two would share a hue, a template and a filter row while being read for opposite reasons.
- **Fold tools into `concept`** — a concept is knowledge that stands on its own and is linked *to*; a tool note is knowledge about operating something, and its value is in the steps. Merging them makes the concept filter useless for finding either.
- **A `tag` instead of a type** — tags are open and uncounted; the type is what the card's bar, the filter overlay and the Ctrl+N picker key on. A tool the user creates from Ctrl+N with a template is a type, and putting it in tags means no template and no bar.
- **Something narrower, `software` or `method`** — two types for one idea, and the boundary between them (is a checklist a method or software?) is a decision the user would make at every creation. Generic is the point: the note's own prose says which kind it is.
