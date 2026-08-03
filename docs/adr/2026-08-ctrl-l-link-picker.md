# Ctrl+L opens a link picker that owns its own query field

## Context

Phase 9's exit criterion is "you never type a full `#l("...")` by hand".
The active block is an uncontrolled `<textarea>`
(`adr/2026-07-hybrid-active-block-textarea.md`): the webview owns its caret,
its value is only seeded at mount, and every plain keystroke stays native.
Inserting a link therefore has two problems — where the caret is, and how a
buffer edit becomes visible in a widget nobody controls.

## Decision

- **Ctrl+L**, handled on the **logs pane's keydown**, not the app root.
  The chord needs the editor, the caret probe and the index; the root has
  none of them, and it only carries the app-global chords (Ctrl+T, Ctrl+Q).
  The textarea already lets ctrl chords bubble, so the chord arrives from
  inside the block being edited.
  Guards: there must be an **active block** (with nothing to type into there
  is no caret to anchor to) and **no picker already open**. Failing either is
  a silent no-op — a notice would be noise.
- **The anchor is probed once, at open**, through the existing `CaretProbe`.
  It cannot go stale: the picker takes focus, so the textarea's text can no
  longer change while the picker is up. Same for the completion list, which
  is snapshotted from `Index::completions()` at open.
- **The picker owns its own `<input>`.** Filtering through the textarea would
  mean diffing its value against the buffer on every key to tell "query" from
  "prose", and it breaks the moment the caret moves mid-filter.
  The field asks for focus in `onmounted`, exactly as the textarea does.
- Inside the picker: **arrows** move the highlight and stop at both ends,
  **Enter** accepts the highlighted row (nothing at all when the query
  matches nothing), **Escape** closes without writing, a **click** on a row
  accepts it. Every plain key is `stop_propagation`'d so Enter never reaches
  the empty-day create handler; ctrl chords still bubble, so Ctrl+T and
  Ctrl+Q work over an open picker.
  The list is capped at `links::MAX_MATCHES` (8) — past a handful, reading
  costs more than typing one more letter.
- **Accepting** calls `Editor::insert(anchor, "#l(\"id\")")`, which routes
  through the same `edit` the widget uses — one staleness policy, and the
  buffer/widget separation the v2 vim layer needs is untouched.
- The textarea's key carries a **remount epoch**, bumped only by an accepted
  completion. A new key means a fresh element with a fresh `initial_value`,
  which is how a buffer edit reaches an uncontrolled widget. Keystrokes never
  touch the epoch, so ordinary typing still never remounts.
- The caret then belongs **past the link**, not wherever the webview puts it,
  so a new `CaretWriter` root context (a JS `setSelectionRange`, the
  `CaretProbe` pattern in the other direction) is called from the remounted
  textarea's `onmounted`. The pending offset lives in a plain cell, not a
  signal: only the mount handler ever reads it.
- The picker's moving state (query, highlight) lives in its **own signals**,
  separate from the `Option<Picker>`. Handlers that exist only while the
  picker is open then need no `Option` branch — and the project's 100%
  coverage rule has no unreachable arm to explain away.

## Rejected

- **A trigger character (`@`, `[[`)** — no chord to learn, but the trigger
  can appear in real prose and would need an escape story; and it means
  reading the textarea's text to know when it fired.
- **A controlled textarea (`value:` instead of `initial_value:`)** — would
  make the spliced text appear with no epoch and no remount, but it re-renders
  the widget on every keystroke and puts the caret at the mercy of the diff.
  That is precisely what the phase-8 ADR avoided.
- **Handling the chord at the app root** — symmetric with Ctrl+T/Ctrl+Q, but
  the root would have to reach down for the editor and the index, and the
  quit chord already shows what that costs (`QuitFlush`).
- **Leaving the caret where the remount puts it** — one less injected
  primitive, but the caret lands at the start of the block and the next
  keystroke lands in the wrong place, which is the opposite of "cheap to type
  mid-sentence".
