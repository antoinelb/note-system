# Coverage tool: cargo-llvm-cov on nightly

## Context

Phase 0 requires `make test` to produce a coverage report, and the roadmap holds every phase (2+) to 100% coverage.
The makefile already invoked `cargo +nightly llvm-cov` before this ADR; the decision formalizes it.

## Decision

`cargo-llvm-cov`, run on nightly, with `lib.rs`, `mod.rs` and `main.rs` excluded via `--ignore-filename-regex` (they contain only wiring, no logic to cover).

## Alternatives rejected

- **cargo-tarpaulin** — ptrace-based, historically less accurate (missed lines, no reliable branch data), Linux-only quirks. llvm-cov uses the compiler's own source-based instrumentation, so what it reports matches what actually ran.
- **Stable toolchain** — nightly is needed for coverage of doctests and some instrumentation features; the project already accepts nightly for this single command, the build itself stays on stable.
