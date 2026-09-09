# ADR 0003: Keep Mantine behind the Core UI boundary

## Status

Accepted

## Decision

ToneNari uses Mantine and a shared Mantine theme as the low-level React UI
foundation. Imports from `@mantine/*` are allowed only below
`apps/control-ui/src/core`.

React Flow is the low-level interaction engine for audio graph canvases. Imports
from `@xyflow/react` are also allowed only below Core. Features receive the
ToneNari graph model and semantic `AudioGraphCanvas` contract; they do not
depend on React Flow node, edge, handle, or change-event types.

Screens and features consume semantic Core components such as `Panel`,
`StatusBadge`, and `ActionButton`. They must not compose product UI directly
from Mantine components.

The theme uses a zero default radius for structural and interactive components.
Badge components explicitly opt into a pill radius. The same provider and theme
are mounted in the application, tests, and Storybook.

`pnpm --filter @tonenari/control-ui check:boundaries` enforces the import
boundary during local and CI checks.

## Consequences

- Product features remain independent from Mantine and React Flow APIs and
  styling defaults.
- Cross-cutting accessibility, localization, and visual changes have one Core
  implementation point.
- A future component-library migration is limited primarily to Core.
- New Mantine primitives require a deliberate Core abstraction before product
  features can use them.
