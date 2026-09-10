# macOS clock, SRC and session acceptance (VMs off)

This records local macOS checks only. Windows and Linux VM work stayed stopped;
no additional agents were used. No host default device or physical microphone
was selected. User graph topology was preserved.

## Installed transport and clock

See `2026-09-08-macos-initial-attachment.md` for the acknowledged-but-never-applied
HAL request and its completion watchdog. The installed CoreAudio duplex fixture
changed from 0/281600 matching samples to 281600/281600. After the final engine
restart it passed again (280576/280576, all nonzero, zero invalid callbacks).
During actual audio, the API reported `running`, clock authority `device`, and
`issue:null`; it returned to idle after the fixture stopped. The five cumulative
underruns observed around stream startup remain recorded; this is not a zero-xrun
endurance qualification.

## Real native rate boundaries

`bash scripts/check-macos-device-io.sh --live` passed the deterministic native
sanitizer cases, four device removal/reappearance cycles, and both Rust native
tests. Private fixture aggregates were destroyed. Native input/output stayed at
48000 Hz through graph 44100/96000/48000, retaining the same owners. Each measured
phase advanced 95744 native frames; input and output FIFO overflow counts were
both zero. This hardware adapter's 96 kHz graph test does not widen the installed
HAL's supported 44.1/48 kHz session range.

`zsh scripts/check-macos-application-clock.sh /var/tmp/mixrelay-application-clock-20260908-root --run`
passed with a real owned process tone, CoreAudio process tap, desktop SRC pump,
and isolated v2 source registry. The installed graph was not replaced.

| Native rate | Graph rate | Valid frames | Tone Hz | Peak | RMS | Synchronized |
| --- | --- | --- | --- | --- | --- | --- |
| 48000 | 44100 | 131328 | 440.234 | 0.050000 | 0.035355 | 1 |
| 48000 | 48000 | 143104 | 440.072 | 0.050000 | 0.035355 | 1 |
| 48000 | 44100 | 131584 | 440.048 | 0.050000 | 0.035355 | 1 |

Epochs advanced 3 -> 4 -> 5; native progress advanced in every phase. The owned
tone and pump exited after the test. This is component integration, not a claim
that all user applications or DAWs were tested.

The common release clock-bridge suite passed 11 tests again. Identical 180-second
simulations with +/-500 ppm offsets kept FIFO occupancy bounded (935..1113 around
target 1024); disabling correction accumulated -4322/+4318 frames. All 1/2/8/16/
32/64-channel hot-path fixtures reported zero heap operations. Physical-device
long-duration drift, latency and full-application leak qualification remain
separate; these figures are not scheduler or CPU speedup claims.

## Bidirectional settings and persistence

`node scripts/lab/macos-session-format-probe.mjs /tmp/mixrelay-macos-attach.2b8hUC/own-format-probe --run`
passed app-to-own-device 44.1/48 kHz changes, processing buffers 64/128/256/512,
and external own-device 44.1/48 kHz changes followed by the app. Every accepted
format was read back from native state and the saved preferences. Graph revisions
advanced 2 -> 9 while nodes/edges were preserved. Original 48000/256 was restored;
the subsequent engine restart restored it again without a session warning.
The buffer setting remains MixRelay's processing maximum, not a promise to set
other applications' client buffer sizes.

## Capability projection and remaining qualification

The macOS profile now advertises the native hardware and v2 application bridges'
SRC/drift support. These two booleans had remained hard-coded false after those
implementations were connected. They do **not** certify runtime synchronization:
per-binding clock observations still determine warnings, and the regression test
requires an unclocked session to retain `ClockNotSynchronized` even with both
support flags true. No UI warning was suppressed and no Windows/Linux support
flags were changed.

Engine capability/controller tests and all-target Clippy passed. The parity
matrix remains open for actual DAW/physical-device/endurance and other-OS
qualification; this document does not declare the entire product complete.
