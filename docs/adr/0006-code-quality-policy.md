# ADR 0006: Enforce one local code-quality gate

## Status

Accepted

## Decision

`pnpm check` is the required local gate. It must complete without warnings and
runs UI formatting/linting, TypeScript type checking, unit tests, production and
Storybook builds, Rust formatting, unused dependency detection, Clippy, and the
100% Rust library coverage gate.

Rust uses workspace-wide deny-level `unused`, `rust_2018_idioms`, Clippy
`all`, `complexity`, `pedantic`, and `perf` policies. `clippy.toml` limits
cognitive complexity to 15, arguments to 5, function length to 80 lines, and
type complexity to 200. `rustfmt.toml` fixes the edition, line width, newline,
and import-order policy.

React and TypeScript use Biome for formatting and linting. Recommended rules,
unused imports and variables, explicit `any`, non-null assertions, cognitive
complexity above 15, and functions above 80 lines are rejected. TypeScript's
strict compiler and 100% Vitest statement/branch/function/line coverage remain
independent gates.

## Stable values and localization

- Protocol values, node type IDs, port IDs, event names, error codes, and route
  paths are declared once as typed constants or enums in their owning Core or
  domain module.
- Machine values use stable lowercase identifiers and are never translated.
- Human-facing strings use localization keys. A translated label must never be
  parsed back into domain behavior.
- Repeated prose in fixtures may remain literal when it is test input rather
  than a program identifier.

## Structure

- Composition is the default. Behavior is supplied through small traits,
  providers, hooks, and components rather than base classes.
- Mantine remains below Core; Feature consumes Core and Screen composes Feature.
- A function that exceeds a threshold is split by responsibility. Suppressing a
  rule requires a local explanation showing why extraction would make the code
  less clear or break a platform boundary.
- Generated artifacts are excluded from source checks rather than reformatted.

## Consequences

- New warnings fail locally before a future CI workflow exists.
- Formatter output is deterministic across editors and machines.
- Complexity limits are review aids, not a substitute for clear naming,
  cohesive modules, contract tests, or performance measurement.
