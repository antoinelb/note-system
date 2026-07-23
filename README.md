# Note system

A personal knowledge system where every note is a plain [Typst](https://typst.app) file and the main view is an infinite table of index cards.

Notes live on a spatial canvas with persistent positions — reach for a card, write on it, put it back.
The files compile standalone with the vanilla typst CLI; the app only maintains a derived index (links, tags, positions) that can always be rebuilt from them.
AI can suggest links and tags, but never writes in your notes: accepting a suggestion means writing it yourself.

Built with Rust and Dioxus for Linux.
Single user, no accounts, no plugins — it's my system, extended by editing the source.

**Status: early development, nothing usable yet.** Design in [`docs/plan.md`](docs/plan.md), decisions in [`docs/adr/`](docs/adr/).
