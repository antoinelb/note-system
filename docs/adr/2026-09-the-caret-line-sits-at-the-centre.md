# The caret's line sits at the pane's centre

## Context

`adr/2026-08-scroll-anchor-is-consumed-once.md` gave `j` and `k` the user's own `gjzz`/`gkzz`: every vertical move pinned the caret to the middle of the pane.
Every *other* way the caret moves — typing past the fold, `o`, `G`, a `/` search landing, an undo, a Ctrl+O landing, a click — fell through to `ScrollLogicalPosition::Nearest`, which scrolls the least it can get away with.
So the line being written drifted: centred while walking with `j`, pinned to the bottom edge while typing down the page, wherever it happened to be after a jump.

The user asked for the whole of it: *"the current line should always be vertically centred even if that makes extra spacing appear at the bottom (but this doesn't apply to the top where it's ok when the cursor is higher than the centre when no more lines are before)."*

**And none of the centring worked at all.** Verified on the shipped binary before anything was changed: every caret move — `j`, `k`, `zz`, `zt`, `zb` alike — put the caret's line at the *top* of the pane. `dioxus-desktop`'s `scroll_to` serialises `ScrollToOptions` as `{behavior, vertical, horizontal}` and hands that object straight to `Element.scrollIntoView`, which reads `block` and `inline`. The alignment is dropped on the floor and `block` takes its default, `start`. `make test` could never see it: `dioxus_ssr` renders markup, and the headless `MountedData` backing has no `scrollIntoView` to disagree with.

## Decision

**Every caret move centres the line the caret landed on. A re-render nobody asked for still scrolls nothing.**

The mechanism is one comparison. `settle_caret` takes `at`, the note-global offset this mount draws the caret at (`Pane::Source`'s block start plus the block-relative head — a block's content starts where the block does, so the sum is exactly `editor.caret().head`), and compares it against `settled_at`, the offset the last mount already scrolled to:

- a **different** offset is the user having moved the caret, and a consumed anchor falls back to `Center` instead of `Nearest`;
- the **same** offset is a re-render — the async fragment compile landing that remounts this very span — and falls back to `Nearest`, which moves nothing.

That keeps the rule the anchor ADR was written for (AIR LAY-2 / Core rule 5: the interface moves only when the user moved it) while inverting the default, and it catches every path at once: one seam at the mount, rather than an `Act` arm per way the caret can move.
Block-relative would not do — the same column in two different blocks is the same number, and `j` between two blocks would read as "did not move".

`Act::WalkVisual` loses its explicit `Center` arm: `j` and `k` are moves like any other now. The one behaviour that changes with it is a `j` on the note's last line, which lands nowhere and therefore scrolls nothing, where before it re-centred a caret that had not moved.

**`zz`, `zt` and `zb` are unchanged, and they still act once.** They arm an explicit anchor with a nonce, the mount consumes it, and the next caret move re-centres — which is exactly what "consumed by the mount that uses it" has always meant. `zt` is now the way to *stop* centring for one screenful; there is no mode to leave, and none was asked for. They also work for the first time.

**The alignment is spoken to the DOM directly**, through a third injected script beside `HIT_PROBE` and `LINE_WALK` (`launch::CARET_SCROLL`, `ui::CaretScroll`, wired in `main.rs` — the `HitProbe` pattern, `adr/2026-08-caret-on-editor-note-bytes.md`): the script finds the caret the way `LINE_WALK` finds it and calls `scrollIntoView({behavior: 'instant', block, inline: 'nearest'})` with the word the app asked for. `scroll_to_with_options` is gone from the app; there is no version of it that can carry an alignment, and the one round trip it cost is the one round trip the script costs. What a headless test can see is the word asked for, which is what the two tests assert.

**The note gains a tail so its last lines can reach the centre.** A `div.scroll-tail` is the last child of each scroll container — `.centre` on the logs, `.sheet` on the table — and carries `--scroll-tail`, half the pane's height, inline. `.centre` observes itself through its own `onresize`, the idiom `adr/2026-08-viewport-culling-onresize.md` already established for the table pane, falling back to the same deterministic default; the sheet reads the frame `table::sheet_frame` already computes for the index card (`adr/2026-09-the-sheet-is-an-index-card.md`), so the card's tail is always half the card. Half of a scroll container's own height is not a length CSS can spell, which is why the measurement comes from Rust.
`theme.css` gives the tail that height only inside a pane that holds a note (`:has(.note-blocks)`), so an empty day still scrolls nothing at all.
It is the container's last child, *after* the sheet's backlinks footer, so nothing real is pushed half a card out of reach.

**No tail at the top, by the user's own instruction.** Within the first half-screen the pane stays at `scrollTop: 0` and the caret sits above the centre, because `scrollIntoView` cannot scroll past the start of the content. That falls out of doing nothing, which is why nothing is done.

## Verified

`make static` clean, `make test` at 100% (1165 lib tests), and seven e2e scenarios — `visual-line-j-k`, `daily-note`, `ex-line-substitutes`, `search-text`, `sink-survives-a-wake`, `keys-before-the-sink-focuses`, `create-note` — pass.

The vault cannot witness a scroll position, so there is no new e2e scenario: the harness asserts on `.typ` files, never on pixels (`adr/2026-08-headless-x11-e2e.md`). Verified instead by driving the shipped release binary in the harness's own headless X server (1400x900, i3, `import` for the screenshots), over a sixty-line day note on the logs and the same sixty lines in a permanent note opened as a card's sheet. The pane spans y 36–900, so its centre is y 468; the index card spans y 198–738, so its centre is y 468 too. Read off the shots:

- **the logs, `gg`** — the caret sits on `#import` at y 121 with the pane at `scrollTop: 0` and the scrollbar thumb at its top. The first line is above the centre and nothing is padded above it: what the user asked for.
- **the logs, `30gg`** — the caret sits on `line 19 of sixty` at **y 468**, sixteen lines showing above it and sixteen below.
- **the logs, `G`** — the caret sits on the note's trailing empty line at **y 468**, `line 60 of sixty` immediately above it and the tail's empty room below, down to the pane's foot.
- **the logs, `k` from there** — the caret on `line 60 of sixty` at **y 468**: the note stepped up by exactly one row, nothing else moved.
- **the sheet, `30gg`** — the caret on `line 19 of sixty` at **y 468**, the card's own centre.
- **the sheet, `G`** — the caret at **y 468**, the backlinks footer `← 2` still immediately under the note at y 517, and the tail's room below it inside the card.
- **an empty day** — "no note for july 24 — press enter to start one", no scrollbar, nothing to scroll.

The same run against the pre-change binary put the caret at the top of the pane in every one of those shots, which is how the dropped alignment was found.

## Alternatives rejected

- **Arming `Center` at every `Act` that moves the caret.** Ten-odd arms plus the typing path, the undo path, the Ctrl+O landing and the click path, each of which would have to remember; and a new way to move the caret would silently not centre. The mount already knows where the caret is — the comparison belongs there.
- **Computing the scroll in Rust** — the caret's client rect, the container's rect and its scroll offset, then a `scroll()` — four round trips over the same channel per keystroke, and three of them racing the fourth. One script that does it inside the DOM is the idiom this repo already reaches for when geometry is the webview's (`adr/2026-08-visual-line-j-k-through-a-geometry-seam.md`).
- **A `50vh` tail in CSS alone.** The logs' pane is the window minus the chrome, so `50vh` is close enough there; the index card is at most 540px tall, and half a 900px window is most of a card's height again in dead space. One number cannot serve two panes of different heights.
- **`container-type: size` on the scroll containers and a `50cqh` tail.** No Rust at all, and WebKitGTK 2.52 supports it — but `container-type: size` implies `contain: layout`, which makes the container the containing block for `position: fixed` descendants, and the IME sink is fixed precisely so that focusing it never scrolls the pane (`adr/2026-09-the-sink-outlives-the-active-block.md`). Buying a smaller diff by moving a load-bearing invariant onto new ground is not a trade this file makes.
- **Bottom padding on the scroll container itself.** Whether a scroll container's bottom padding is part of its scrollable overflow has been a browser difference for years; a spacer element is a box, and boxes scroll.
- **Making the centring a setting**, or a mode the user leaves. Configuration is off the v2 list (`adr/2026-08-v2-caret-first-order.md`), and the ask was unconditional.
- **A scrolloff-style margin** — rejected once already by `adr/2026-08-scroll-anchor-is-consumed-once.md`, and this ADR only widens the case that rejection was made for.

## Ceiling

`settled_at` remembers one number and nothing about which note it belongs to. Landing on a different note at the same byte offset — Ctrl+O onto another note, the caret at the same place in both — reads as "did not move" and asks for `Nearest`. The one shape that reaches in practice is a fresh note opening at offset 0, where the pane is already at the top and `Nearest` scrolls nothing anyway; the next keystroke centres. Storing the note's path beside the offset would close it, and costs a clone per mount for a case that has no visible symptom.
