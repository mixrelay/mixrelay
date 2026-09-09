# Meter freshness and request ownership — 2026-09-07

## Agreed behavior repaired

Input, processor and output meters must represent current telemetry, not retain a
positive level after the engine stops answering. The previous workspace poller
retried rejected requests without clearing the meter store and had no independent
expiry while a native promise was hung. The store also survived control-port changes.

The shared UI now has one explicit meter-owner lifecycle:

- Successful polls continue every 50 ms, including changing values on an unchanged
  graph revision. Revision is not an audio-frame sequence counter.
- Rejection immediately clears levels and schedules a retry after 500 ms.
- Accepted levels expire after 250 ms without a new accepted frame. A response
  whose own request took at least 250 ms cannot revive the display. Request age uses
  a monotonic clock, so moving wall time backward does not revive an old frame.
- Hiding the view clears levels, pauses polling and invalidates already-started
  responses even if the view becomes visible before they settle.
- Each control-port owner has a distinct store. Cleanup clears levels, notifies
  subscribers, removes the visibility listener and cancels owned timers. Late
  replies cannot update a replacement engine's store.
- An in-flight request lease survives React StrictMode effect cleanup/replay with
  its original timestamp. A hung native request is not replaced by accumulating
  more unresolved requests. Once it settles, normal polling can resume.

## Changed-boundary evidence

`useRoutingMeters.test.ts`: **17 passed**, independently rerun by the implementing
owner and reviewer. Covers cadence, rejection/retry, expiry while hung, stale
responses, expiry refresh, all invalidation subscriber notifications, visibility,
owner replacement, unmount timers, wall-clock reversal and StrictMode replay.

The StrictMode test first used a nested wrapper that did not replay root effects
under this React version; it was corrected to the test library's root
`reactStrictMode: true` setting. The corrected regression failed before the owner
lease fix and passed after it. Under the identical fixture (first request pending,
100 ms elapsed), native calls were **4 before / 1 after**. This is a bounded
duplicate-request comparison, not a whole-application CPU/latency benchmark.

Final integrated UI: **188 passed / 31 files**, TypeScript/Vite production build and
Core boundaries passed; the existing large-bundle warning remains. Targeted Biome
checks passed. No native/audio engine/driver/device
settings changed. This shared UI implementation applies to macOS, Windows and
Linux control ports; it does not constitute actual native UI qualification on all
three OSes.

## Remaining limits

Clearing telemetry does not determine whether silence is healthy, capture is idle,
or the device/processor has failed. Actual per-node health and native processing
failure reporting remain in the failure-telemetry gate. This patch cannot cancel a
native IPC promise that never settles, and does not claim native worker recovery.
It also cannot detect a backend repeatedly returning an old snapshot as if it were
new: that requires native frame/activity metadata. Real PCM/channel correctness and
all-OS device stall/recovery tests remain open.
