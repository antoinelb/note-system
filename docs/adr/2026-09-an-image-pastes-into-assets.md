# An image on the clipboard pastes as a file under `vault/assets/` and an `#image` call

## Context

Every file in the vault was a `.typ`, and a note could show an image only by naming a file that lived somewhere else.
A student pastes screenshots — a slide, a figure, a whiteboard — and Typst's `#image("path")` shows one, but the path had to be made by hand outside the app.
The clipboard seam (`adr/2026-08-clipboard-reads-are-native.md`) read text only; arboard can read an image as RGBA when its `image-data` feature is on.

## Decision

**A paste with no text on the clipboard pastes its image.**
Normal-mode `p`/`P` and insert-mode Ctrl+V share one path, `pasted`: the text read first, and when it comes back empty, the image read.
The PNG lands at `assets/<note-stem>-<yyyymmdd-hhmmss>.png` through the atomic persist seam, and what enters the note is `#image("/assets/<file>")` — vanilla Typst, the leading `/` resolving at the vault root as every `#import` already does.
Text on the clipboard still wins: a paste that has words to paste never looks at pixels.

**The worker encodes the PNG.** `clipboard::Reader::read_image` is the text read's twin on the same bounded worker: arboard's RGBA is encoded with the `png` crate there, so the UI thread never touches a pixel buffer, and a clipboard with no image is `Ok(None)`, not a failure.

**`assets/` is seeded with the category directories**, the first place in the vault that is not `.typ`.
The watcher and the scan see only `.typ`, so a pasted image never reaches the index; the file is named by the note that pasted it and the moment, so two pastes never collide and a listing reads as a timeline.
A read that refuses, a note with no name to file under, or a write the disk refuses each say so on the status line and paste nothing.

## Alternatives rejected

- **A palette row only** — a paste is a paste; `p` and Ctrl+V are what the hand already does with the clipboard, and text and image are two shapes of the one register (`adr/2026-08-one-register-the-clipboard.md`).
- **Embedding the image in the note** (base64 in a `#image(bytes(...))`) — the file stays plain and small, and the image opens in any viewer.
- **A per-note directory** (`assets/<stem>/`) — one directory, one listing; the stem is in the filename.
- **Decoding on the UI thread** — a screenshot is megabytes of RGBA; the worker exists so the frame never waits on the clipboard.
