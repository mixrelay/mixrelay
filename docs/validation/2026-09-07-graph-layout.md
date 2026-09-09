# Graph-specific presentation persistence — 2026-09-07

## Backend contract

Presentation is saved beside each document in `graphs.json`, not inside the DSP
`GraphDocument`. It uses UI node identities: two cards representing a duplex endpoint can
have different positions even though they refer to the same runtime node.

```json
{
  "graphId": "graph-1",
  "layoutRevision": 1,
  "layout": {
    "positions": {
      "duplex:input": { "x": 120, "y": 100 },
      "duplex:output": { "x": 520, "y": 300 }
    },
    "viewport": { "x": -80, "y": 55, "zoom": 0.75 }
  }
}
```

- `GET /api/v1/graphs/{id}/layout` returns this shape for an existing active or inactive graph.
- `PUT` on the same path accepts `{ expectedLayoutRevision, layout }`.
- `GET /api/v1/graphs` and catalog mutation responses include the active graph's `layout`
  and `layoutRevision`. Older saved catalogs default to empty positions, no explicit
  viewport and layout revision zero; there is no forced reinitialization.
- Each graph has its own layout revision. Layout writes do **not** advance DSP revisions
  or the catalog's create/select/rename revision. They remain valid while another graph's
  audio publication is pending.
- Layout writes use the existing engine mutation lock and atomic disk replacement. They
  never call native resource preparation, processor snapshots or DSP submission.
- Stage/commit failures reject the layout write and preserve previous disk bytes and
  in-memory layout. There is no audio side effect requiring a rollback. The same expected
  layout revision can be retried after storage recovery.
- A successful layout write saves the complete current catalog and clears the backend's
  prior catalog storage warning. Frontend warning lifecycle is a separate integration boundary.

Errors identify `parameters.resource = "graph_layout"`. A stale layout revision returns
`graph_revision_stale` with the graph ID and both layout revisions. Missing IDs or invalid
values return `graph_invalid`; storage failures return `control_unavailable` scoped to
`graph_layout`, not an assertion that the audio engine disconnected.

## Bounds

- At most 4,096 positions per graph.
- UI identities: nonblank, at most 256 UTF-8 bytes, no control characters.
- Position/viewport coordinates: finite, within ±10,000,000.
- Zoom: finite, within 0.05–8.0.
- Existing whole-catalog 16 MiB limit still applies.
- Unknown shape fields and invalid JSON coordinate types are rejected.

No runtime-node membership restriction is imposed on UI identities, because duplex and
presentation-specific cards intentionally use identities distinct from DSP nodes. Options
disclosure state is not persisted by this slice.

## Focused native fixture evidence

Six new engine tests cover:

1. Inactive graph layout saves, empty initial creation, selection, rename and restart recall.
2. A backend that panics if native preparation is called: layout saves succeed with **zero**
   native preparations and unchanged graph document, active/submitted audio revisions and
   catalog revision.
3. Stale layout rejection with unchanged saved bytes, while normal graph edits and renames
   do not invalidate the independent layout revision.
4. Disk staging and final rename failures preserving old layout/file, then same-revision retry.
5. Unknown graph IDs, nonfinite/out-of-range coordinates/zoom, invalid IDs and excessive count.
6. Pre-layout catalog compatibility and preserving corrupted layout files without replacement.

One new server test checks GET/PUT routing, duplex UI identity retention, independent
revision conflicts and malformed coordinate rejection with the correct error resource.
One protocol test checks older catalog responses and presentation serialization round trips.

Observed local combined run: **4 protocol + 8 server + 48 engine tests passed**, including
the concurrently implemented virtual-source-gain restart regression. The six layout tests
use exclusively owned temporary directories, not the installed service or real user graphs.
Common engine/control-server/control-protocol `cargo clippy --lib --tests -- -D warnings`
also passed on the final source.

No speedup claim is made: the zero-native-call assertion proves architectural separation,
not a before/after timing benchmark. Browser interaction, desktop reload/restart and Windows/
Linux native reruns are owned by the parent integration task and require their own evidence.

Source boundaries:

- `apps/engine-service/src/graph_layout.rs`
- `apps/engine-service/src/graph_layout_tests.rs`
- `apps/engine-service/src/graph_catalog.rs`
- `crates/control-protocol/src/lib.rs`
- `crates/control-server/src/lib.rs`
