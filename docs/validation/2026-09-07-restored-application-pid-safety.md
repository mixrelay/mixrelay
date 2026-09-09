# Restored application PID safety — 2026-09-07

A persisted bare process ID has no birth identity. After restart, that number can
belong to an unrelated process, so catalog recall must not silently capture it.

`ApplicationCaptureRequest.requiresReselection` is optional and defaults to false
for old wire requests and newly selected applications. Catalog loading marks all
PID-based capture requests true, including inactive graphs. It preserves the
original selector/PID, labels, gain, nodes, cables, graph/catalog/layout revisions,
and original file bytes. Stable executable-path and bundle-ID requests are not
changed. The marker survives later saves and graph edits; selecting an application
again creates a fresh request without it.

This marker does not reject the graph contract. Restored graphs remain selectable,
renamable and editable. Linux/Windows adapters must treat the marker as a silent
waiting state, report application unavailability, and skip PID lookup/open and
automatic reconnection until explicit reselection. Their adapter implementations
and platform validation are owned by the corresponding platform slices; the
common tests below do not prove native process capture behavior.

## Common verification

On the macOS development host, the focused Rust checks pass:

- One audio-io test: missing-field compatibility, true-flag JSON round trip,
  unchanged PID selector, and valid graph contract.
- Four catalog tests: active/inactive restored PID restrictions with exact
  preservation of all other serialized fields and original file bytes; selection,
  rename and resave; idempotence and fresh application requests; actual portable
  controller load, DSP preparation, gain edit and disk save of the restored graph.
- Clippy for audio-io, audio-graph, audio-engine and engine-service library/tests
  with warnings denied; tracked diff whitespace check.

No user graph, native audio stream, service, process capture, VM setting, or
installed application was changed by these checks. The controller test uses the
portable resource backend. UI graph conversion retains unknown runtime config
fields, so no TypeScript schema expansion is required for the boolean marker.
No performance improvement is claimed.
