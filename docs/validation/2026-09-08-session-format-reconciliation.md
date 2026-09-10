# Session format reconciliation boundary

This slice implements common control-plane transactions and the macOS monitor hook. It is not evidence that every native Windows/Linux adapter or attached capture transport supports every rate.

## Contract

- `RuntimeStatusBackend::external_format` is opt-in and reads the own endpoint's actual native pair; the legacy `external_sample_rate` fallback preserves the DSP block size. The controller skips unknown, unchanged, pending-publication, or concurrently mutating graphs.
- An external change snapshots surviving hosted processor state, prepares a new DSP plan and staged native resources, then adopts the observed format at an acknowledged processing boundary. It never calls the native sample-rate setter. A second native rate change during preparation/suspension rejects that obsolete adoption.
- User changes retain the native setter/confirmation/rollback contract. Explicit `with_graph_format_control` permits engine-format-only control without a dummy native setter or invented native-rate observation. A platform must separately reject unsupported fixed-rate virtual devices.
- `GraphResourceBackend::prepare_format_change` returns an owned `PreparedGraphFormat`: all fallible native work and mailbox reservation occur before native negotiation. Drop cancels staging; infallible commit runs while DSP is paused immediately before the matching graph revision publishes. The default rejects attached hardware/application transports on format changes.
- Capabilities for a proposed format have a separate projection from the currently activated status format. Wrappers must delegate this projection.
- Both user and externally accepted formats become the saved last-accepted format, preventing restart from undoing an OS/DAW change. External preference/catalog storage failure warns but does not veto safe DSP adoption; the last valid disk bytes are preserved. In that failure case restart cannot guarantee the unsaved choice.
- macOS uses an owned, cancelable/joined control monitor with weak controller references. No graph compilation, file I/O, native property query, preset snapshot, or resource destruction is added to the audio callback.

## Focused verification

The new deterministic tests cover external change coalescing, concurrent/pending graph deferral, native rejection and staged-resource cancellation, preparation failure before negotiation, unsupported/unknown rates, stale hosted-state refusal, active-format publication timing, actual-rate race during suspension, saved-format reload, warning-only external persistence failure, and engine-only control preserving an independently observed native rate.

`cargo test -p mixrelay-engine-service --lib` passed 83 tests (11 new rate/transaction regressions); `cargo clippy -p mixrelay-engine-service --lib --tests -- -D warnings` passed. Native transport rejection precedes hosted state export/reopen, and the monitor uses a one-second retry interval after failure rather than repeatedly reopening an unsupported graph every 250 ms.

Native platform acceptance remains separate: actual HAL setter/follow behavior, macOS application/hardware SRC handoff, PipeWire negotiated stream changes, Windows shared-mode versus virtual-driver format policy, and real DAW interaction must be verified by the platform owners. DSP maximum block size is not a claim of native hardware buffer negotiation.

## Full native format extension

`with_native_format_control` and `with_observed_format_following` accept owned closures, permitting adapter-local/weak state without global callbacks. These are optional contracts for an adapter that actually owns the relevant native I/O configuration; rate-only and graph-only adapters remain compatible. Full native writes require a readable pre-write pair and verify the observed pair after the setter acknowledges. A partial setter or preference-commit failure restores the **observed native pair**, not the previous DSP format: native 512 frames versus DSP 128 frames is an explicit regression. Failed rollback leaves processing suspended. External buffer-only changes never call the setter, and a second native buffer change during suspension rejects the old prepared pair.

`observedDeviceBufferFrames` is an optional additive field, published with the observed rate through one atomic snapshot. Absence means unknown, not equal to DSP block size. A known own-endpoint mismatch uses the existing `block_size_mismatch` issue. It is not a generic PipeWire callback quantum field. Unsupported external sizes are not quantized or written back: the old safe processing plan and an explicit mismatch remain until a supported configuration is accepted; native workers retain responsibility for safe capacity slicing.

The full-pair extension adds nine regressions covering rollback, setter-ack verification, persistence failure, buffer-only/stale-revision, late native change, unknown observation, and old-wire compatibility. Final combined `cargo test -p mixrelay-engine-service --lib`: **92 passed**, 1.30 seconds. `cargo clippy -p mixrelay-engine-service --lib --tests -- -D warnings`: **passed**. The preceding common slice's 83-test checkpoint is retained above for provenance.

### macOS live counterevidence and corrected deployment

The deployment probe disproved the assumption that the engine client's `BufferFrameSize` readback controls a global HAL callback quantum: the engine read 256 while a simultaneous fresh Core Audio probe of the same own device UID read 512. MixRelay's own-HAL engine transport uses direct shared memory, not an `AudioDeviceIOProc` owned by this Core Audio client. Setter/readback agreement therefore did not prove that any actual consuming application's callback buffer changed.

macOS startup has consequently returned to genuine nominal-sample-rate-only setter/following. `observedDeviceBufferFrames` remains absent there; it must not report this unused client view as active native I/O evidence. The generic full-pair contract and deterministic tests remain, but **macOS global buffer synchronization is not implemented or qualified**, and its processing block limit remains distinct from another application's native callback quantum.
