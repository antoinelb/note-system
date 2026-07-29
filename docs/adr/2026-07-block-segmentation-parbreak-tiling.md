# Blocks are Parbreak-separated runs that tile the note; fragments compile under a synthesized preamble

## Context

The hybrid editor needs the note cut into blocks ("top-level markup nodes,
never line-based", roadmap phase 8), each inactive block rendered as its own
SVG fragment.
Probing typst-syntax 0.15.1: the parse tree's top-level children tile the
text exactly (accumulating `SyntaxNode::len()` gives byte offsets); the
preamble's `#import`/`#show`/`#meta` lines are separated by `Space`, not
`Parbreak`; multi-line constructs (headings, paragraphs, list runs, raw
fences) never contain a top-level `Parbreak`.

## Decision

- **Segmentation**: a block is a maximal run of top-level non-`Parbreak`
  children. Blocks tile `[0, len)` — every byte belongs to exactly one
  block, separators trail the block they follow, leading blank lines belong
  to block 0, and an empty or blank-only note is one block. Tiling makes
  split/merge ordinary resegmentation: typing a blank line splits a block,
  deleting one merges it, no dedicated operations.
- The preamble needs no special casing: import/show/meta form block 0 by the
  split rule alone and render as the design's meta line. A block containing
  a top-level `ModuleImport` is marked **standalone** and compiles as-is.
- **Fragments**: every other block compiles as
  `#import "/templates/template.typ": *` + `#show: note` + a `#set page`
  vertical-margin override + the block's source, under the real note's path
  (`VaultWorld` already takes in-memory text). The synthesized preamble
  deliberately omits `#meta` — the template's `meta()` emits the visible
  meta line where called, and only block 0 should show it. The margin
  override sits after the show rule so it wins over the template's
  `margin: 1.5cm`, keeping stacked fragments from doubling page margins.
- **Cache**: `FragmentCache`, keyed by a hash of (note path, fragment
  source), caching errors as well as SVGs, bounded by mark-and-sweep on
  every resegmentation. This grows out of and replaces `SvgCache` — the
  succession `adr/2026-07-svg-cache-per-path.md` predicted; whole-note
  rendering leaves the app with it.

## Known ceilings (accepted)

- Cross-block state does not cross fragments: a `#let` referenced from
  another block is an inline compile error (honest); heading numbering,
  counters and footnotes restart per fragment (silently different from a
  whole-note compile).
- The fragment preamble duplicates the template's horizontal margin.
- With a buffer open, external edits to the same file are last-writer-wins
  (the watcher stays unwired, as before).

## Rejected

- **Line-based or regex segmentation** — typst constructs span lines and
  only compile as complete expressions; the tree is the authority
  (`plan.md` § Editor, and the phase-2 never-regex rule).
- **Whole-note compile + carving the SVG into per-block regions** — needs
  source→layout position mapping (typst-ide territory) and breaks the
  "active block is plain source" model for no gain at note scale.
- **Gap-based ranges (blocks exclude separators)** — every edit at a block
  edge would need ownership rules for the gap; tiling makes the question
  unaskable.
