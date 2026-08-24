# The caret lives on `Editor`, in note-global bytes

## Context

v2 phase 0 replaces the active block's textarea with an app-drawn caret.
The textarea's caret was DOM state, read through an async JS `selectionStart` probe in UTF-16 units and written back through remounts — the `CaretProbe`/`CaretWriter` seams, plus the frozen-caret machinery every overlay carried (`Palette.caret`, `Creator.caret`, `Picker.anchor`, `pending_caret`, `epoch`).
The buffer-owned-cursor path was sketched in `adr/2026-07-buffer-is-path-plus-string.md` and deferred by `adr/2026-07-hybrid-active-block-textarea.md`; phase 0 is where it lands.

## Decision

- `Editor` owns the caret: `Caret { anchor, head }`, both **note-global byte offsets** on grapheme-cluster boundaries, meaningful only while a block is active. Selection is `anchor != head`; `head` is where the caret blinks.
- All text math lives pure in `src/caret.rs` (cluster steps, line edges, goal-column verticals, word steps, and the `layout` render model the widget draws). `unicode-segmentation` provides cluster boundaries — French text is the fixture, not the edge case.
- Every mutation routes through the existing whole-block `Editor::edit` — one staleness policy. `insert_at_caret` and `delete_at_caret` compute the block's new value; `edit` answers whether the splice landed so the caret only moves over text that actually changed.
- Vertical moves on a block's edge lines slide to the neighbouring block through the activate path; the goal column (in clusters) survives short lines and is forgotten by any other move.
- UTF-16 leaves the `Editor` API. `blocks::byte_offset_of_utf16` survives with one caller: converting the mouse hit probe's answer.
- The mouse needs geometry the app can never own (the source pane is proportional Cormorant Garamond), so one new seam exists: `HitProbe`, wrapping `caretPositionFromPoint`, answering (hit span's `data-start`, UTF-16 units within its text node). This reads *pointer geometry*, not caret state — the `Viewport` seam's category, not `CaretProbe` resurrected.
- Everything frozen-caret retires: `CaretProbe`, `CaretWriter`, `epoch`, `pending_caret`, and the caret fields of every overlay. App-owned state cannot go stale behind an overlay, so overlays just close and the focus effect hands focus back.

## Consequences

Accepted divergences from the textarea era, all noted here on purpose: arrows and Home/End work on logical lines, not visual wraps; kerning breaks at caret and selection span boundaries; Ctrl+arrows no longer double as month paging while editing (they are the word step, consumed).
The palette's caret commands lost their `frozen.caret` guard — they are listed only over an active block (`palette::available`), which is the guard; an unreachable second guard cannot be covered and was dropped.

## Rejected

- **Block-relative offsets** — phases 2–5 need note-scoped motions (`gg`, `dG`, search landing outside the active block); `activate` already speaks note-global coordinates, and resegmentation never edits text, so a note-global offset survives every flush.
- **UTF-16 units** — they existed only because the JS probe answered in them; they die with it.
- **Keeping the caret in the widget layer** — headless tests and the v2 keymap both need it below the widget; "components stay thin" (`adr/2026-07-ui-covered-at-100.md`).
