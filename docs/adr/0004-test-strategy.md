# ADR 0004: Unit-first testing with complete Rust and UI coverage gates

## Status

Accepted

## Decision

Deterministic behavior is tested with unit tests colocated with the owning
crate. Rust tests under top-level or crate-level `tests/` directories are not
the default because they hide private boundaries and tend to grow into slow,
broad integration suites.

`pnpm coverage:rust` runs `cargo llvm-cov` for all workspace libraries and
fails if any function, line, or region is uncovered. The gate also requires
the absolute uncovered count for each category to be zero, avoiding percentage
rounding.

The control UI runs Vitest with V8 coverage and fails below 100% statements,
branches, functions, or lines. Stories, test setup, and the one-line browser
entry wiring are excluded; Core, Feature, Screen, adapters, and localization
initialization remain covered.

Integration tests are limited to boundaries that require real external state,
including installed HAL plug-ins, Core Audio devices, process-capture
permissions, OBS, hot-plug events, and independent hardware clocks.

## Consequences

- New deterministic branches must be covered before the local check passes.
- Platform adapters must isolate state machines from operating-system calls so
  most behavior remains unit-testable.
- Coverage is evidence of execution, not correctness; assertions must still
  verify contracts and failure semantics.
- Generated code and truly unreachable defensive code require an explicit
  source-level exclusion with a written justification rather than lowering the
  gate.
