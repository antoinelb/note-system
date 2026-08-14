# A template touch clears the render caches

## Context

The watcher classified only category-directory `.typ` files as changes, so
an edit to `templates/template.typ` produced no event at all — yet every
compile reads the template from disk.
Both caches served stale pixels until restart: bodies because nothing
invalidated them, fragments because the template is the one compile input
their content-addressed key never carries.
The 2026-07 SVG-cache ADR promised template-event clearing "in phase 5"
and it was never implemented.

## Decision

Taken with the user (2026-08-13):

- **The watcher gains `VaultChange::Template`**: any `.typ` under
  `templates/` written, created or removed. The index ignores it (it reads
  no templates); the change kind exists for the caches.
- **The UI's watcher loop clears both render caches on it** — bodies
  through their stale shelf (old pixels hold the slot while the recompile
  is out), fragments outright. A `Rescan` clears the fragments too: lost
  events could have been a template edit, the watcher's own trust-nothing
  doctrine.
- **The fragment cache gains the epoch guard bodies already carry.** This
  amends adr/2026-08-async-caches-pending-stale.md's "fragments need no
  staleness guard": content-addressing holds across everything *in* the
  key, and the template is not in it — a mid-flight compile landing after
  the clear would poison its key forever. The clear bumps the epoch and
  late outcomes drop whole.

## Rejected

- **Hashing the template into the fragment key** — a disk read per probe
  for a file that changes a few times a year; the same reasoning that
  rejected content hashes for bodies.
- **Treating a template edit as a full `Rescan`** — rebuilds an index the
  template cannot affect.
- **Watching non-`.typ` files under `templates/`** — nothing in the vault
  references any; widen the filter when something does.
