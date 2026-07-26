# Coverage at 100%: diagnose per instantiation, fill the cheapest side

## Context

`CLAUDE.md` requires that `make test` reach 100% coverage once a feature is done.
At the end of phase 3, `index.rs` capped at 89.81% of *regions* even though all logic lines seemed exercised.
The missing regions were error edges of `?`: each `?` compiles to a branch, so `connection.execute_batch(SCHEMA)?` counts two regions — success and failure.

Once these edges were covered, the counter stayed stuck at 95.67% even though the merged view (`segments`) showed **no** uncovered position.
This contradiction was first taken for an insurmountable artifact of the tool; that is wrong, and it was exactly the trap documented elsewhere (see *Prior art*).

## Decision

**`make test` fails below 100% of regions, lines, and functions.**

```
cargo +nightly llvm-cov --ignore-filename-regex '(lib\.rs|/mod\.rs|/main\.rs)$' \
    --fail-under-regions 100 --fail-under-lines 100 --fail-under-functions 100
```

### The operational rule

`llvm-cov` folds a group of instantiations by taking the **maximum covered-region count of a single copy**, never the union of the copies.
A file with inline unit tests is compiled twice — once bare (linked into the integration binary), once with `#[cfg(test)]` (unit-test binary) — therefore:

> **100% requires that a single compilation cover the whole file.**

Any union-style view — `segments`, lcov, HTML — will display 100% while the summary reports a shortfall.
**This contradiction is the signature of folding, not proof of a phantom.**

### The diagnostic procedure

Region coverage below 100% on a file with inline tests gets broken down **before** writing anything:

1. `cargo +nightly llvm-cov --json`;
2. group the `functions` entries by span fingerprint (first region + region count);
3. per group, compute the covered-region count of **each copy**; the offending group is the one where `max(covered) < total`;
4. list the regions missed by each copy, then fill the cheapest **measured** side.

Writing a test without this breakdown amounts to guessing which side the hole is on.
Here, the breakdown gave in one pass: `open` 20/22, `rebuild` 17/19, `scan_vault` 45/47, `insert_note` 58/61, `discard` 9/11, `query_rows` 17/18 — and for each of them the cheapest side was the unit-test binary, at 1 to 3 regions short, versus 2 to 27 on the integration side.

### The generic-function trap

`query_rows` is generic over the type of the parameters.
A test that called `query_paths(conn, sql, [])` therefore created an **additional instantiation** (6/18) instead of covering the one used by the public methods, which pass `[&str; 1]`.
The type of the arguments is part of the copy's identity: a coverage test targeting a generic function must pass **the same types** as the production code, otherwise it covers a copy of its own.

### What served to cover the error edges

Three means, from cheapest to most expensive:

1. **Real states of the filesystem and of SQLite** — a path under a nonexistent directory makes `Connection::open` fail; a *file* named `permanent` makes the internal `read_dir` fail; a blob planted by a raw connection breaks `row.get::<String>` (TEXT affinity converts numbers, never blobs); a `DROP TABLE` only makes `prepare` fail on the **second** call, SQLite keeping the schema cached per connection and failing at `step` the first time.
2. **SQLite's authorizer** — `Connection::authorizer` consults a callback at the preparation of each statement; `Authorization::Deny` turns the chosen statement into an error. "Make the INSERT into `tags` fail but not the one into `notes`" fits in four lines, without a trait or a fake object. Requires rusqlite's `hooks` feature, declared in `[dependencies]` for lack of per-profile features.
3. **A `faults` module compiled under `cfg(test)`** — outside of tests, each of its functions is the identity. It covers the four edges that no real state reaches on Linux: second connection opening in `open`, failure of the `PRAGMA foreign_keys`, failure of the schema creation, a `read_dir` iterator yielding an `Err` midway. Each injection point returns a **value** that the real code then uses (`execute_batch(faults::schema_sql())`), never an additional `?`: the control flow tested is the control flow shipped.

The injection tests live in `src/index.rs`: forcing a failure requires the private connection, and the unit-test module is the privilege boundary that allows it without widening the public API.

### Two production changes that came out of this hunt

They justify themselves on their own, independently of the counter:

- `discard()` replaces `if db_path.exists() { remove_file(..)? }` with a `match` treating `NotFound` as a success — the pre-check left a TOCTOU window where another process deleted the file and its absence was reported as a failure.
- `query_rows()` merges the near-identical bodies of `query_paths` and `dangling_links`, which each prepared, mapped, and collected on their own.

## Alternatives rejected

- **Lowering the threshold to lines only** (first draft of this ADR, erroneous): justified by a supposed insurmountable artifact, whereas the folding was simply misunderstood — the per-file sum had been taken for a sum of copies instead of a maximum per group. A lowered threshold would have frozen the error into the tooling.
- **An arbitrary threshold (`--fail-under-regions 95`)**: a magic number unjustifiable six months later, which rots as soon as a file moves.
- **`coverage(off)` on the recalcitrant functions**: 100% by non-measurement.
- **Injecting the filesystem and rusqlite behind traits**: changes the signatures of `Index::open` and `scan_vault` permanently and requires fake objects faithful to SQLite's real failure semantics — which are not obvious, as shown by the `DROP TABLE` that fails at `step` and not at `prepare`.
- **`--fail-under-file-lines 100`**: fails at 100.00% while it passes at 99.9%, visibly an edge comparison in the tool. `--fail-under-lines 100` on the total is just as strict: at the 100% threshold, a single uncovered line anywhere brings the total down.

## If 100% ever becomes truly impossible later

That will be the case if a production line is reachable neither by a real state, nor by the authorizer, nor by a `cfg(test)` fault, **and** the per-instantiation breakdown confirms that no copy can cover it.
In that case: **do not lower the threshold**; add `#[cfg_attr(coverage_nightly, coverage(off))]` on the function in question with a comment naming the exact reason.
The exemptions remain visible and reviewable one by one instead of hiding in a percentage.

Mandatory precondition: having done the breakdown of steps 1 to 4 above.
Without it, "impossible" only means "not yet diagnosed" — which is precisely the mistake made during the initial drafting of this ADR.

## Prior art

The same phenomenon had already been encountered and documented in `generateur_horaire`:
`docs/conception/adr/2026-07-couverture-par-instanciation-le-plus-petit-ecart.md` (2026-07-19) and
`docs/conception/adr/2026-07-couverture-100-et-frontiere-io.md` (2026-07-17).
