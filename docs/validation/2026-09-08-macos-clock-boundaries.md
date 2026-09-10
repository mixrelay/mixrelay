# macOS clock boundary migration — first slice

Root-only work; VMs remain stopped. No installed graph, host default device,
physical rate, user Aggregate, permission setting or installed HAL was changed.
This is not completion of the private-Aggregate / three-OS migration.

## Deterministic checks

`bash scripts/check-macos-clock-boundaries.sh` passed:

- Bounded native timestamp snapshot: validity, negative sample positions,
  discontinuities, host-time reversal, missing/NaN timestamps, and 100000
  concurrent publications. Atomic fields are lock-free on this host; no reader
  spin or non-atomic seqlock payload. ASan/UBSan enabled.
- Actual hardware callback selects input/output buffer timestamp, not callback
  arrival timestamp. Non-aggregate identity proof rejects unknown transport,
  missing property and unverified Aggregate. Existing 384 layout/epoch cases,
  64 SPSC cases, default-selector and teardown tests retained.
- Actual Process Tap callback exports timestamp/continuity metadata; 1000
  teardown/recovery cases and format invalidation pass with ASan/UBSan.
- Seven hardware-worker tests; four desktop clock-pump tests, including restart
  on a timestamp discontinuity without any format change or ring overflow.

`cargo clippy -p mixrelay-engine-service -p mixrelay-desktop --all-targets -- -D warnings`
passed after correcting two new lint findings (test literal notation and field
naming). The feature matrix generator, check and six self-test assertions pass;
the release gates remain open. `git diff --check` is clean for tracked changes.

## Same-condition improvement

The production `DeviceBridge` is constructed once with the previous blanket
independent-clock policy and once with the new same-native-device decision.
Both use 2 channels, native/graph 48000 Hz, identical PCM, 512 queued input
frames, and a 256-frame output request.

| Measurement | Previous policy | Same-device policy |
| --- | ---: | ---: |
| SRC algorithmic delay | 128 frames | 0 frames |
| SRC algorithmic delay at 48 kHz | 2.667 ms | 0 ms |

Bypass PCM is bit-exact. This is converter delay, **not** end-to-end latency or
a CPU speedup measurement. Queue priming/native buffer latency is unchanged.
Other devices, unknown IDs, rate mismatch, disconnected masters and absent
masters retain independent treatment. Physical-device latency remains unmeasured.

## Native component integration

`bash scripts/check-macos-device-io.sh --live` passed, including four actual
removal/reappearance cycles and two Rust native tests. Temporary owned
Aggregates were destroyed. Native input/output remain at 48000 Hz while graph
44100/96000/48000 phases each advanced 95744 native frames. Input and output FIFO
overflows were zero in each phase; owners were retained. These fixture Aggregates
are deliberately not treated as verified same-clock devices by the new policy.

`zsh scripts/check-macos-application-clock.sh /var/tmp/mixrelay-clock-boundary-live-20260908 --run`
passed through a real owned 440 Hz application, actual Process Tap, desktop pump
and isolated source registry. No installed graph replacement or physical output.

| Native Hz | Graph Hz | Valid PCM frames | Measured tone Hz | Synchronized |
| ---: | ---: | ---: | ---: | ---: |
| 48000 | 44100 | 131200 | 439.992 | 1 |
| 48000 | 48000 | 143104 | 440.072 | 1 |
| 48000 | 44100 | 131584 | 440.048 | 1 |

Native timestamp reached sample 454401 / host tick 32990819429570, valid, with
zero timestamp discontinuities in this short run. Graph epochs advanced 4/5/6.
The owned tone and pump were reaped. This is not long-duration drift or complete
application/driver memory-leak qualification.

## Remaining acceptance

- Physically independent devices with OS-owned Aggregate drift compensation.
- Real same-hardware duplex bypass and hardware sample-rate negotiation/rollback.
- Device/DAW external settings arbitration; no setting ping-pong.
- Complete production Aggregate grouping and Windows/Linux ownership migration.
- Long-duration physical clock, latency and memory qualification.

The implementation direction and deliberate boundaries are in ADR 0012.
