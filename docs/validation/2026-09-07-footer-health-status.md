# Footer health consistency — 2026-09-07

The integrating owner observed an actual in-app-browser footer with a red engine marker but
accessible text `Ready`, while virtual nodes correctly showed the session's unsynchronized-clock
warning. The footer used two unrelated decisions: any session issue selected red, while the
hidden text described only driver installation state.

## Change

- `features/audio-session/presentation.ts` now supplies shared session label/severity to the
  virtual-node presentation and footer. Running with no issue is positive; stopped/processing
  failure is critical; aggregate app/device/clock/storage warnings and idle are warning.
  Missing processing state or missing session is unknown, not evidence of readiness.
- Non-ready driver states still contribute their actual installation/permission/availability
  information. They do not make a ready driver equivalent to a running audio owner.
- `StudioScreen` renders one Core `StatusIndicator`: its color, accessible name and tooltip
  receive the same presentation. The old hidden driver-only `Ready` text and unused CSS are
  removed. No new permanently visible explanatory copy was added.
- The old App regression that expected `Not installed` after a failed status request now
  asserts unknown audio health instead; a transport error does not prove installation state.

## Evidence

- `screens/StudioEngineStatus.test.tsx`: **45 component cases**, 15 states across English,
  Korean and pseudo locale. Uses the real Studio footer and status polling hook with a
  `StorybookMockControlPort`; unrelated workspace/settings components are mocked. Clock warning
  cases also open the real tooltip and check it matches the accessible name and marker color.
- Focused footer/Studio/node status: **98 passed across three files**.
- Final complete UI run at 22:37:41 KST: **316 passed across 39 files**.
- TypeScript, scoped Biome, Core boundary and whitespace checks passed.
- The integrating owner confirmed actual in-app-browser hot reload now exposes
  `Session: clocks not synchronized`, consistent with the virtual-node warning. This is browser
  presentation evidence, not a claim that the underlying clocks are synchronized.

These tests make no live HTTP/audio-session changes. The local control instances are in-memory
fixtures; HTTP adapter regressions inject mock fetch functions. No native device, block size,
sample rate, graph, permission or system setting was changed by this task. Native OS audio and
vendor-editor qualification remain separate.
