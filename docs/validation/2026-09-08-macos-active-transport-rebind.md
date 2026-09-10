# macOS running transport replacement: clock lifecycle regression

## Failure boundary

The first coordinated updated-HAL duplex smoke returned 281,600 compared samples,
zero matching / nonzero samples, and zero invalid callbacks. The engine reported
timer authority rather than qualified native pacing. This did not establish which
native boundary failed, so it was not recorded as a successful clock deployment.

Source inspection then found a reproducible lifecycle defect in
`MixRelayHAL_PerformDeviceConfigurationChange(kDeviceConfigurationChangeChannels)`:
it reinitialized `MRHALIOContext`, clearing its clock rate, running state and epoch,
then bound the new transport. Existing CoreAudio clients could still be counted in
`gDevice_IOIsRunning`. Their next `StartIO` need not enter the first-client branch,
so the new mapping's idle clock never accepted native demand credits.

## Fix

The driver preserves the previous context clock epoch across reinitialization and
publishes the actual current rate / running state with a new epoch after both
successful replacement and rollback to an existing mapping. A running replacement
also starts fresh sample-buffer epochs so old PCM cannot satisfy the new timeline.
Idle replacement remains explicitly idle. Successful nominal-rate changes update
the attached clock metadata too; they do not wait for a future StartIO to make the
new native rate observable.

This runs inside the existing control-plane configuration boundary, not the audio
callback. No new callback allocation, blocking lock or OS query was added. The
engine already resets its timeline and HAL pacer when its endpoint activation
returns a new configuration; preserving the driver's epoch independently also
avoids generation reuse at the producer boundary.

## Before / after verification

The test includes the real driver translation unit and invokes actual
`PerformDeviceConfigurationChange`; only the unrelated XPC send boundary is
stubbed. Temporary descriptors / shared mappings are fixture-owned and unlinked;
no installed device is changed.

- **Before fix:** `zsh scripts/check-macos-native.sh` exited 134 at
  `clock.running == 1U && clock.sample_rate == 48000U` after a running replacement.
- **After fix:** the same regression passed, including two running replacements,
  failed replacement restoring the current mapping, an idle replacement, 44.1 /
  48 kHz idle changes, strictly advancing epochs, fresh demand after replacement,
  and zero demand credit from idle writes.
- Final full native script: pass, ASan/UBSan, existing isolated-native 100%
  line/function gates and universal arm64 / x86_64 HAL build.
- `cargo clippy -p mixrelay-engine-service --all-targets -- -D warnings`: pass.

The deterministic comparison establishes the lifecycle defect and its correction,
not live latency, jitter or CPU improvement. A root-coordinated reinstall and
repeat of the actual duplex smoke are still required to determine whether this
was the complete cause of the first installed failure. No successful installed
PCM result is claimed by the fixture tests.
