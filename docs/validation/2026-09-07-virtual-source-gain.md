# Virtual source gain — 2026-09-07

## Implemented path

Virtual inputs previously had no `gainControl` in the UI model, and the shared renderer's
`lower_virtual_endpoint` explicitly passed `0.0` to its source kernel. Adding a slider alone would
therefore have left the signal unchanged.

The complete graph-side path is now:

```text
Virtual input Options → updateRoutingNodeGain → config.gainDb → graph apply/history/catalog
→ VirtualEndpointConfig → validated source kernel gain → source meter → routes → sink meter/PCM
```

`VirtualEndpointConfig` is a graph-only wrapper around the unchanged native `VirtualEndpointSpec`.
Its `gainDb` is flattened into the existing JSON config, defaults to unity for old documents, and
is omitted when zero. The graph compiler accepts finite values from -60 through +12 dB. Native
endpoint format/name/channel metadata contains no gain field. macOS/common endpoint extraction,
Linux reconciliation, and Windows reconciliation take the wrapped `spec`, not the graph wrapper.
Thus gain-only graph changes do not change the desired native endpoint identity or format.

The UI still uses one logical runtime endpoint for its input and output cards. Serialization now
takes gain from the input card, preventing the later output card from overwriting a live input gain
with its older copy. This is tested with either card ordering, JSON roundtrip, and subsequent
endpoint name/channel edits. The input card exposes gain alongside the existing endpoint options;
the output card does not gain an unrelated input-gain control.

The existing live `onChange` and 400 ms history path are reused. No release-only update handler or
separate hidden gain node was added. Undo restores the pre-gesture source value; redo serializes the
final source value back to the runtime graph.

## Verification

Run from the repository root:

```sh
cargo test -p mixrelay-audio-graph -p mixrelay-audio-engine --lib
cargo test -p mixrelay-engine-service --lib virtual_source_gain
pnpm --filter @mixrelay/control-ui exec vitest run \
  src/features/routing-workspace/virtualSourceGain.test.tsx \
  src/features/routing-workspace/model.test.ts \
  src/features/routing-workspace/graphHistoryGroup.test.ts
pnpm --filter @mixrelay/control-ui typecheck
pnpm --filter @mixrelay/control-ui check:boundaries
```

Results on the macOS development host:

- Shared Rust graph/engine: **38 tests passed** (17 graph, 21 engine).
- Controller durability regression: **1 test passed**. Actual `graphs.json` bytes are written in an
  exclusively owned temporary directory; a fresh controller reads them and restores `gainDb=-12`.
  This is controller reconstruction in one test process, not an installed service/process restart.
- Focused UI: **10 tests passed**, including 3 new virtual-source-gain tests. TypeScript, the Core
  boundary check, and Biome on the two changed owned UI files passed.
- `cargo clippy -p mixrelay-audio-graph -p mixrelay-audio-engine -p mixrelay-engine-service
  --all-targets -- -D warnings` and `cargo fmt --all --check` passed at the final source freeze.
- The actual shared `BlockProcessor` scales 2, 8, 16, 32, and 64 channels by 0.5 at -6.0206003 dB.
  Both source and sink meter banks report 0.5 for full-scale test input; the signal is not attenuated
  a second time at the virtual output.
- An independent application-capture source routed directly into the virtual output stays at unity
  while the virtual input gain is -12 dB. Input gain does not act as output gain.
- For each tested width, 100 calls to the actual block processor perform **zero instrumented heap
  allocation/free operations**. This is a bounded real-time regression assertion, not a claim of
  whole-process memory-leak freedom or a before/after performance improvement.
- The real graph controller applied -6 then -12 dB and rendered PCM at the expected amplitudes.
  A recording endpoint adapter observed **2 preparation calls, 0 desired native-spec changes**.
  This proves the controller contract does not send gain as a native format change; it is not an
  observation of live Core Audio/PipeWire/WASAPI handle reopening.

## Key sources

- `crates/audio-graph/src/model.rs`, `compiler.rs`, `tests.rs`: flattened graph-only configuration,
  legacy/default compatibility, invalid-gain rejection, graph/native-spec separation.
- `crates/audio-engine/src/render.rs`: source-kernel lowering and PCM/meter/isolation regressions.
- `apps/engine-service/src/virtual_source_gain_tests.rs`: controller, actual disk persistence,
  fresh-controller restore, desired-native-spec recording, and PCM validation.
- `apps/control-ui/src/features/routing-workspace/model.ts`, `virtualSourceGain.test.tsx`: source
  Options, input/output card serialization, name/channel edits, and history roundtrip.

## Matrix handoff / remaining qualification

`source_gain`: virtual input is now implemented at the shared UI, graph, renderer, and durable
controller boundaries rather than missing. App and physical-input paths were not replaced.

Keep real installed-device slider-to-audio E2E and no-reopen observations open separately for
macOS, Windows, and Linux. Native GUI dragging, physical audio capture, a full service restart,
and long-running drift/latency behavior were not exercised by this bounded task. A common-renderer
test passing on macOS is not proof that each OS adapter has been qualified.
