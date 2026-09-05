# An `input` event is a delta against what the field showed, never the query itself

> Superseded by `adr/2026-09-the-sink-is-the-one-keyboard-socket.md`: the query fields are gone, and with them the `input` event and the `Shown` memory. The ambiguity this reader could not resolve — a report one character short being a Backspace or a stale field — is why.

## Context

`adr/2026-09-overlay-keys-relay-before-focus-lands.md` made the nine
query inputs controlled and had the relay push a key typed at the pane
onto the open overlay's query signal. That closed the grammar hole and
left a second race open. Dioxus desktop ships DOM patches over its edit
websocket and an element's `set_focus` over `evaluate_script`; nothing
orders "the relayed letters reached the input" before "the input took
the keyboard". A key typed at the input in that window is reported by an
`input` event whose value is the field as the webview still shows it plus
the key, and the handlers took that value whole: `creator_query.set(event.value())`.

`make upgrade` on 2026-09-02 caught it in the one scenario that sends its
chord and letters unpaced, `overlay-keys-before-focus.test.sh`, three
runs in six alone on a loaded machine, three different ways. A lone `t`
replaced the seven relayed letters and the type list ranked
`organisation` first. The title stage inherited the type name because
the step change cleared the signal while the field still showed
`concept`, and the first title letter was reported on top of it:
`concepttyped-before-focus.typ`. A third run left the field empty and
wrote nothing, which the relay's Enter ceiling would explain but the run
did not prove. The first two are one defect: the field's report was
trusted as the whole query.

## Decision

**`shown::Shown` remembers what the field may be showing** — the last
value it reported, then every value the app wrote since — and every
query input's `oninput` goes through one `typed` callback that reads the
report as a delta against the longest remembered value it extends,
appended to the query. A report one character short of a remembered
value is a Backspace and pops the query. A report matching nothing
remembered is a deliberate edit elsewhere in the field, and the field is
right. The report proves the patches up to the value it extends landed;
later writes stay remembered, and the answer joins them.

The writes the field cannot see coming are the ones recorded: each key
the relay pushes, and the creator's three step changes (Enter, a row's
click, and Escape back to the type list). Every opener resets the memory
to the fresh input's value, `""` for nine of them and the ex line's
prefill. The memory is bounded at 32 writes.

The e2e scenario stays unpaced: it is the regression test, and it is
what a person typing right after Ctrl+N on a loaded machine does.

## Alternatives rejected

- **Pacing the scenario** — the suite's earlier workaround, and this
  scenario exists to be the one that is not paced. Three failures in six
  alone is a defect, not a pacing hole.
- **Typing through `onkeydown` in the inputs, the DOM as a mirror** —
  one source of truth, but every patch moves the caret to the end and
  kills mid-line editing in the queries, and dead keys and the IME arrive
  through `input`, not `keydown`.
- **Focusing the input only after the relayed patch landed** — the edit
  socket's applied-ack is not exposed to the app, and the keys typed
  before the grab would still queue against a stale field.
- **Keeping focus on the sink and drawing the inputs read-only** — no
  race at all, and the nine accept paths leave the overlays; rejected
  once already in the relay ADR.

## Known ceiling

A double letter straddling the moment a patch lands is read as the
patch landing, one letter short; a Backspace on a field the relay has
not reached yet pops a letter the person could not see. Both need a key
inside the one window, on a loaded machine, in a one-line filter.
