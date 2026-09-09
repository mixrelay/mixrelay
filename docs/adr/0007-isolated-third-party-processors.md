# ADR 0007: Isolated third-party processor workers

Status: Accepted

## Context

ToneNari's render callback cannot load, unload, lock, or destroy third-party plug-ins. A plug-in
may also crash, hang during shutdown, allocate in its DSP callback, or expose hundreds of rapidly
changing parameters. Keeping those objects in the graph service would make one vendor binary part
of the engine's real-time and lifetime guarantees.

## Decision

- `audio-engine` defines only `HostedAudioProcessor` and `HostedProcessorFactory`. It has no VST,
  Audio Unit, COM, Core Audio, PipeWire, or WASAPI dependency.
- Each active external-processor node owns one child worker. The child alone creates, processes,
  stops, and destroys the plug-in instance.
- The engine callback exchanges one fixed-size interleaved block through a private memory mapping
  and process-shared atomics. It never waits for the child. A missed block takes the graph's
  deterministic channel-preserving bypass path.
- Interactive parameter changes use a single-value coalescing mailbox. The worker applies the most
  recent normalized value before its next block; the UI records the full gesture as one debounced
  undo entry.
- Native-editor open and close requests use a second coalescing mailbox. The desktop proxies the
  request, while the existing DSP worker creates the outer native window and embeds the editor of
  the same plug-in instance. This avoids unsynchronized mirror instances and invalid cross-process
  `NSView` pointers.
- Discovery scans each format and probes each candidate in disposable processes. A bad scanner
  target cannot terminate the graph service or poison the shared catalog snapshot.
- The probe distinguishes a vendor-supplied native editor from a host-generated parameter view.
  In particular, Apple's in-box `AUGenericViewFactory` is represented by ToneNari's existing
  parameter controls instead of being mislabeled and opened as a floating plug-in editor.
- The macOS system Mach service runs the engine and processor workers as the logged-in user. Only
  the HAL endpoint attachment is accepted from `_coreaudiod`; this preserves per-user AU, VST3,
  and CLAP registration, settings, and licensing without executing third-party code as the
  audio-server account.
- `processor-host` exposes one format-neutral descriptor, normalized parameter contract, worker
  transport, and lifetime policy. Platform adapters are VST3 and CLAP on all targets, AUv2/AUv3
  on Apple targets, and LV2 on Linux. VST3 remains available on macOS in addition to Audio Unit.
- The scanner selects a usable effect bus layout deterministically, preferring stereo and then a
  symmetric layout. Instruments and MIDI-only processors are not exposed in the audio-effect menu.
- The desktop owns native folder selection and persists explicit scan roots. Rescanning replaces
  one shared catalog snapshot, so the resolver and worker factory cannot disagree about which
  plug-ins are available.
- A monitor thread reaps a crashed worker immediately. Normal removal signals shutdown, closes the
  parent pipe, waits at most two seconds, kills a non-responsive child, unmaps both views, and only
  then removes transport files. Weak control registrations cannot extend processor lifetime.
- Editor events run on the worker's main thread. DSP runs on a dedicated worker thread and uses a
  non-blocking plug-in lock; window move/resize event pumping never holds that lock, and a rare
  collision takes the existing deterministic bypass path instead of blocking audio.
- Editor parents use `NSView` on macOS, `HWND` on Windows, and X11 window IDs on Linux. XWayland is
  supported by forcing the worker to X11 when `DISPLAY` exists. Pure Wayland sessions report
  `display_unavailable` because the current common editor API has no Wayland parent-handle contract.
- `processor-host` is the provider adapter. Platform conditionals stop at the format scanner and
  loader; neither the render graph nor `audio-engine` imports plug-in SDK types.

The asynchronous boundary adds one audio block of latency to third-party nodes. Automatic plug-in
latency compensation remains a separate graph-timing concern.

## Consequences

Plug-in faults degrade one node to bypass rather than corrupting the graph controller. Graph swaps
still occur at block boundaries, while resource creation and reclamation remain outside the audio
thread. The worker transport is bounded by the compiled channel and block limits, so repeated
processing does not grow memory.

The editor broker exposes `unavailable`, `closed`, `opening`, `open`, and `failed` states plus a
stable failure category. Closing a graph node or stopping the engine closes the editor before the
plug-in is deactivated, then joins the DSP thread and reclaims the window, mappings, and temporary
files. The desktop never retains a plug-in SDK object or native pointer.
