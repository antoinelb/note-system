# `#link` is the link form for what is not a note, and it opens outside the app

## Context

`#l("id")` is the one link the app knows: the parser indexes it, the footer lists it, `gf`, Ctrl+Enter and the palette follow it, and an id nothing owns is a dangling loop.
A student's notes link to things that are not notes — a lecture's slide deck, a PDF, a course page — and every one of them written as `#l` manufactured a dangling loop the open-loops list could never resolve.
Typst has its own `#link(dest)[body]`, which the parser already ignored; the app just did nothing with it.

## Decision

**`#l` stays for notes; `#link` is for resources.** Nothing new is spelled: `#link("/assets/slides.pdf")[the slides]` and `#link("https://…")[the course page]` are vanilla Typst, compile unchanged, and never reach the links table, so they are never debt.

**Following one leaves the app.** `links::link_at` answers a `LinkTarget` — `Note(id)` for `#l`, `Resource(dest)` for `#link` — and the one follow path (`follow_at`, shared by `gf`, Ctrl+Enter and the palette) hands a resource to a `Launcher` injected at launch: `launch::open_with("xdg-open", target)`, which starts the opener and returns at once, reaping the child on its own thread.
A leading `/` resolves against the vault root, as Typst's own `#import "/templates/…"` does; anything else is passed as written.
A launcher that will not start is a warning on the status line under its own source, resolved by the next open that does; what the launcher does with the target afterwards is the desktop's business.

**The editor draws it as a link.** `markup::recognized_role` gives `link` the same `Role::Link` as `l`, and the content block's own brackets join the CSS allow-list, so a line with a `#link` no longer falls back to a compiled widget.

## Alternatives rejected

- **`#l` telling ids from resources by shape** (a `/`, a `://`, an extension) — a heuristic in the one place the index must be exact, and a second meaning for the word the whole link model is built on.
- **A new `#f("path")` helper** — a third link word to remember, for what Typst already spells.
- **Awaiting the launcher** — `xdg-open` hands off to a browser or a viewer and may not return for the life of that window; the UI thread never waits on a child.
- **Listing `#link` in the footer** — the footer is the note graph's edges in both directions; a resource is not a node in it.
