# Engine-only checkpoint — 2026-09-14

This checkpoint contains the engine service, shared audio/graph/control crates,
native macOS transport, diagnostic scripts and their existing measurement reports.
Desktop, UI, Stream Deck, their recorder preferences and desktop-only lockfile
additions remain uncommitted. The UI coverage gate is outside this checkpoint's
explicitly narrowed scope; its threshold has not been modified.

Immediately before this checkpoint, the eight affected Rust library suites were
rerun serially on the staged source: **657 passed**, plus **131 passed** in the
selected macOS engine binary boundary/pacing/device/activity/bridge/recovery suites.
Two timing benchmarks require separate serial invocation and were not rerun;
54 other engine binary tests were outside the selected boundary. Dedicated native
fixture tests and other operating systems are not claimed as passed.

Affected all-target strict Clippy (`-D warnings`), workspace Rustfmt, normal engine
build, affected-package documentation and staged whitespace checks passed. All
engine/native/crate source changes are staged; no unstaged engine implementation
is required. Desktop-only `tempfile` dependencies and `Cargo.lock` edits are excluded.
The preceding whole-workspace compilation is recorded in the local preflight report,
but is not a substitute for this engine-only scope.

The same-condition DSP comparison is retained in
`2026-09-14-node-render-scaling.md`: 32-channel/32-frame median block time fell
71.2% for the chain and 75.4% for fanout; these are in-memory DSP measurements,
not native multichannel or whole-app latency claims. Startup counters and real
30-second capture evidence are in `2026-09-14-input-startup-admission.md`.
Raw recordings and telemetry remain local under ignored `output/`.

This establishes a first source checkpoint, not full release qualification.
The next slice is OS-neutral control-service ownership and native-adapter
boundaries; it must not modify UI, substitute a polling clock, or claim Windows/
Linux device qualification from macOS-only execution.
