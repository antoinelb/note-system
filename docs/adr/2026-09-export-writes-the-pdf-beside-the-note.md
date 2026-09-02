# "export pdf" writes the open note's PDF beside it, off the UI thread

## Context

Every note compiles standalone with the vanilla CLI, and `make check-vault` proves it — but a student handing in an assignment or printing lecture notes had to leave the app and run `typst compile` by hand.
The app already holds a compiler, a `Paper` render theme that is exactly what the CLI produces without inputs (`adr/2026-07-note-rendering-theme-input.md`), and a compute tier for anything that takes longer than a keystroke (`adr/2026-08-compute-tier-worker-seam.md`).

## Decision

**A palette row, "export pdf", chordless.** A PDF is asked for a few times a term, not typed into; a chord would be one more to remember for a rare and deliberate act, like "edit template".
It is listed whenever the note is on screen — the logs' open note, or the table's open sheet — and hidden over the bare table and a closed editor (`Context.note_open`).

**The note is flushed first.** The autosave is debounced; an export of the file on disk while the last sentence still sits in the buffer would hand in a PDF that says less than the screen.
A flush the disk refuses is the notice, and nothing is exported.

**`compute::Job::Export` runs on the compute tier's compile lane**: `render::ExportJob` reads the note, compiles it through the same `compile_document` the SVG path uses — `RenderTheme::Paper(DEFAULT_SIZE)`, so the page is the CLI's page — encodes it with `typst-pdf` and writes `<stem>.pdf` beside the `.typ` through the atomic persist seam (`persist::write_atomic_bytes`).
`Outcome::Export` lands as a notice either way: `exported <path>` as an Info receipt, or the stage that refused (the read, the compile, the encoding, the write) as a Warning with "no pdf was written".

**The PDF is not vault data.** `*.pdf` was already ignored by git, the watcher and the scan only see `.typ`, and the index never learns of it; overwriting a stale export is the point.

## Alternatives rejected

- **A chord** — Ctrl+E and Ctrl+Shift+E are unclaimed, and that is the reason not to spend one on a rare act; the palette is the summon-and-name path for exactly these.
- **Exporting on the UI thread** — a page of equations compiles in hundreds of milliseconds; the 100 ms acknowledgement budget (AIR LAT) is why the compute tier exists.
- **Choosing a destination** — a file dialog is a second surface; the note's own directory is where the CLI would put it, and where the note's `#link` can find it.
- **Exporting the on-screen theme** — dark ink on a transparent page is a screen, not a document; the paper palette is what the CLI produces and what a reader expects.
