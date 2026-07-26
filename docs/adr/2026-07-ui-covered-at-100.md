# The UI lives in `src/ui.rs` and is held to 100% coverage like everything else

## Context

`makefile` excludes `main.rs` from coverage, so putting Dioxus components there would exempt the UI from the 100% rule of `adr/2026-07-coverage-100-percent-lines.md`.
Phase 4 is where that exemption would first be used, and an exemption taken once is taken forever.

Dioxus offers no first-class component test harness: the official 0.7 testing guides sanction `dioxus_ssr::render_element` for markup assertions and Playwright for end-to-end, and document no way to fire an event from a Rust test.

## Decision

Components live in `src/ui.rs` and are covered like any other module. `main.rs` keeps only the launch call — a `LaunchBuilder` carrying the root context described below, still zero logic.

The harness (verified working on dioxus 0.7.9):

1. `VirtualDom::new(Component)`, then `rebuild_to_vec()` — filter the returned edits for `Mutation::NewEventListener { name: "click", id }` to get each listener's `ElementId` in document order.
2. `set_event_converter(Box::new(TestEvents))` before firing anything. `dioxus-desktop` installs a converter at launch; a test binary does not, and `with_event_converter` unwraps a `None`, so the first simulated event panics without this. `TestEvents` implements one of the trait's 21 methods and leaves the other 20 `unimplemented!()`.
3. `dom.runtime().handle_event("click", Event::new(Rc::new(PlatformEventData::new(..)), true), target)`, then `process_events()` and `render_immediate_to_vec()`.
4. Assert on `dioxus_ssr::render(&dom)`.

`SerializedMouseData` requires the `serialize` feature of `dioxus-html`, which `dioxus/desktop` already enables — no new dependency.

### The vault enters through the root context

`dioxus::launch` mounts a zero-prop component, so `App` cannot take the vault path as a prop; reading the environment inside `App` would make its two branches — vault resolved, nothing to resolve — untestable without `set_var`, which the tests in `vault.rs` already refused as process-global and unsafe in edition 2024.

The resolved path crosses the boundary as a root context instead:

- `main.rs`: `LaunchBuilder::new().with_context(ui::VaultRoot(vault::vault_path())).launch(ui::App)` — `vault_path()` stays covered in `vault.rs`, and `main.rs` stays logic-free.
- `App` reads `use_context::<VaultRoot>()`.
- Tests inject either branch with `VirtualDom::insert_any_root_context(Box::new(VaultRoot(..)))` — the same mechanism `dioxus-desktop` uses to hand components the window handle.

Rejected: a `try_consume_context` fallback to the environment inside `App` (keeps `main.rs` on the bare `dioxus::launch`, but ships a context read that only tests exercise from the injected side), and env-driven tests behind a mutex (reintroduces exactly the `set_var` unsafety the `vault.rs` tests refused).

The `TestEvents` impl block carries `#[cfg_attr(coverage_nightly, coverage(off))]`: its 20 stubs are scaffolding that exists to be *not* called, exactly like the `faults` modules in `index.rs` and `watch.rs`.

Phase 4's note list is scaffolding due for deletion (`adr/2026-07-two-screens-table-and-logs.md`), so holding *it* to 100% is not where the value is. The harness is: it is written once and reused by the editor, the logs screen and the v1 table. Building it against three throwaway components is the cheapest place to get it wrong.

Corollary that survives even if this harness rots: components stay thin. Anything that decides something — vault resolution, list building, rendering, caching — is a lib function tested without a `VirtualDom`. The UI tests then cover wiring, not logic.

## Alternatives rejected

- **UI in `main.rs`, coverage-excluded** — the shortest path, and the one that quietly makes "100% coverage" mean "100% of the parts we felt like testing". The number stops being informative the day it stops being total.
- **UI in `src/ui.rs` with an extended ignore regex** — same exemption, but now it erodes by default: every new UI file silently opts out unless someone remembers to think about it.
- **Playwright end-to-end** — tests the real app, but produces no Rust coverage and needs `dx serve` plus a browser in the loop. Worth revisiting for v1's canvas interactions; it cannot satisfy this gate.
