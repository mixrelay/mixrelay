# Audio graph UI incremental-update baseline

Date: 2026-09-02

## Scenario

Disconnect one middle edge from a graph containing 1,024 nodes and 1,023
edges. Count adapter-object identities invalidated between the two controlled
React Flow snapshots. The domain commands preserve references for every
unchanged node and edge.

## Before

The canvas mapped the complete domain model into new React Flow objects on every
model change:

- node adapter objects invalidated: 1,024 / 1,024
- remaining edge adapter objects invalidated: 1,022 / 1,022

## After

The keyed reconciler reuses adapters whose domain objects did not change, and
the custom node type is memoized:

- node adapter objects invalidated: 0 / 1,024
- remaining edge adapter objects invalidated: 0 / 1,022
- removed edge adapter objects: 1
- node collection state updates: 0 (the identical array reference is returned)

The same invariant applies to node creation, node removal, connection, and
reconnection: only added, removed, or domain-changed items receive new adapter
objects. `reactFlowAdapter.test.ts` fixes the 1,024-node disconnect scenario as
a regression test. This is an allocation and render-invalidation measurement,
not a frame-time claim; frame latency will be profiled in the packaged app once
representative graph sizes and node contents are available.
