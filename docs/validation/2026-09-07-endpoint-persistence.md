# Durable virtual-device settings — 2026-09-07

## Completed boundary

Configurable native adapters now explicitly enable `with_endpoint_preferences()`
before starting the audio worker. Per-user `virtual-endpoint.json` stores a
versioned, backend-tagged endpoint template and its attached input/output sides.
It shares the existing audio-session preference directory and atomic replacement
implementation (including `TONENARI_CONFIG_DIR` for isolated services/tests).

- Restart restores the accepted name, identities, asymmetric channel counts and
  deleted-side state through the normal graph validation/native resource transaction.
- Device preferences are **not** graph persistence: restoration never invents
  cables, reopens plug-ins or restores other graph nodes. Session-format preference
  negotiation remains separate; attachment restoration uses the current engine rate.
- Deleting both sides preserves their recreation template but restores an empty
  attachment graph after restart. Partial-side deletion retains the other side's
  saved channel count for subsequent recreation.
- Bad JSON, excessive file size, wrong backend/version, invalid capabilities or
  native staging rejection leave the saved file untouched and keep the service
  available. The document is not marked active until its normal worker activation.
- Writes happen on the serialized control thread only. Same-directory staging,
  file sync and replacement avoid truncating the previous accepted file. Staged
  files have cleanup ownership; Unix-created files use mode 0600.
- A save failure after graph publication is a separate persistence warning, not
  a graph rollback. A later changed device configuration retries saving. Unchanged
  successfully saved device snapshots are not rewritten for unrelated edits.
- Persistence warnings coexist with clock/device issues in status and the UI.
  English, Korean and pseudo-locales are covered. Windows' fixed LAB adapter skips
  configurable-device persistence; this does not enable a variable Windows driver.

## Verification

| Check | Result |
| --- | --- |
| macOS host | 25 engine library tests and engine Clippy passed; native engine and desktop checks passed |
| Windows ARM64 VM | build-v17: build + 25 library / 14 binary tests + Clippy passed; 2 interactive audio tests ignored |
| Ubuntu ARM64 VM | endpoint-persistence-v1: native build, 25 library tests, binary test compilation and Clippy passed |
| UI | 74 tests, TypeScript, Biome, UI-boundary check and production build passed |
| Registry/guest/Windows helper tests | 17 passed, including indirect PipeWire node ownership and probe lifecycle cleanup |
| Ubuntu native restart-v4 | Four actual processes; 64-render/16-capture settings, half-side removal, full removal, template re-add and clean SIGTERM/PipeWire cleanup passed |

The six new Rust persistence tests exercise process-lifetime reconstruction,
asymmetric/partial/empty graphs, native rejection, corrupted/oversized/wrong-version
files, failed replacement with staged-file cleanup, retry, unchanged snapshots and
abandoned atomic staging. These filesystem tests also ran natively on Windows.
UI popup tests verify expanded state/content in JSDOM, not native visual placement.

## Failed attempts retained

- Native restart-v1 could not open PipeWire: Ubuntu was logged out and had no
  `/run/user/1000` audio session. No endpoint restore was exercised by that run.
- restart-v2/v3 created the endpoint but the new probe's node ownership check
  incorrectly expected a PID directly on each node. Registry evidence showed
  `client.id` on nodes and the PID on the referenced client. The probe now resolves
  that relationship, with a regression test; restart-v4 passed.
- The same correction is applied to the earlier matrix harness's shutdown check.
  Its historical direct-node-PID-only cleanup evidence must not be treated as
  complete ownership proof; the full earlier matrix was not rerun in this slice.

Live testing used a transient systemd PAM user session, with an exclusive test
configuration directory. No graphical login, lingering setting, permanent unit,
driver installation, signing change, host audio restart or user configuration
replacement was performed. Failed and successful evidence is retained under
`/Volumes/Linear 1/test/tonenari/evidence/endpoint-persistence-20260907`.
The final read-only cleanup check confirmed all three transient session units
were unloaded/inactive, user 1000 was neither logged in nor lingering, and
`/run/user/1000/pipewire-0` no longer existed (cleanup job exit code 0).

## Remaining gates

Subsequent installed macOS HAL restore and stereo PCM verification is recorded in
[native endpoint follow-up](2026-09-07-native-endpoint-followup.md); it supersedes
the macOS live-test limitation below for that bounded sequence only.

macOS HAL restart/name-change acceptance was not exercised live. Windows product
endpoint descriptors/variable channels and native restoration remain unfinished.
Debian, Kali, Arch and Windows x64 were not requalified. This restart test sends no
PCM, so it does not resolve earlier underruns, drift or audio-quality failures and
does not establish a general memory-leak guarantee. The existing >500 kB UI chunk
warning remains. No runtime performance improvement is claimed by this change.
