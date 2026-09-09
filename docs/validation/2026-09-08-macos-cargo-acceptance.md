# macOS format and Cargo acceptance

## Current evidence

The engine binary's macOS tests now run with Cargo, not the removed
`check-macos-device-io.sh` / `check-macos-device-memory.sh` wrappers. Their two
backup copies were also deleted; no backup directory remains.

```sh
TONENARI_GROUP_PRIMARY_UID=BuiltInSpeakerDevice \
TONENARI_GROUP_SECONDARY_UID=io.tonenari.audio.endpoint.default \
cargo test -p tonenari-engine-service --bin tonenari-engine-service -- --test-threads=1
```

Final rerun after fixing the Clippy function-length failure:
**54 passed, 0 failed, 0 ignored, 0 filtered out**, 30.34 s.
Log: `/tmp/tonenari-cargo-macos-final-20260908.log`.
Engine library: **93 passed**, common audio I/O library: **34 passed**, both
with zero failures, ignored or filtered tests. Normal engine binary build,
workspace formatting check, and Clippy for audio-io/engine-service/macOS test
support across all targets with warnings denied passed. The earlier successful
test run alone did not constitute a passed build/lint gate.

The six former ignored attributes were removed. Missing required native devices
or explicit group UIDs fail; they do not cause an early successful return.
Cargo's macOS-only dev-dependency builds the existing native fixtures in OUT_DIR.
Production builds do not depend on that fixture package. Rust owns graph,
transaction, reconnect and lifetime assertions. Actual native callback and HAL
tests run with ASan/UBSan. Three lifetime cases run their Rust assertions in an
isolated process under macOS `leaks`; both the child result and zero-leak report
are required. Restricted-process inspection warnings limit allocator evidence;
this is not a proof that all future executions are leak-free.

## Format boundary

- Shared product catalog: 44.1/48/88.2/96/176.4/192 kHz and
  32/64/128/256/512/1024/2048 samples. 352.8/384 kHz are out of scope.
- All 42 combinations accepted through the real graph controller and processor
  exchange in the application fixture, retaining the application node.
- All seven native client buffer sizes observed in actual callbacks, each before
  and after an owned aggregate disconnect/reconnect.
- Embedded HAL: six-rate enumeration, partial reads with untouched trailing
  storage, both directions at 1/2/7/64 channels, accepted nominal writes,
  configuration callback and native clock updates, invalid-rate rejection.
- Worker bounds check dimensions separately, including inactive directions of
  one-way endpoints. Scratch storage supports the 2048-sample upper bound.
- Old installed HAL discovery narrows the advertised catalog; building the new
  source does not update the installed driver.

## Performance / limits

This slice is correctness/refactoring, not a claimed latency improvement. Worker
scratch reservation increased from two 512×64 f32 arrays (256 KiB total) to two
2048×64 arrays (1024 KiB): +768 KiB, allocated before real-time processing.
That is capacity, not additional queued audio. The unchanged queue-budget test
remains part of the successful Cargo run. No new comparable physical round-trip
or CPU measurement was made, so no speedup is claimed.

## Still open

This is not macOS product completion: installed-driver/app format acceptance,
full persisted/native settings-following coverage at all rates, long-duration
physical clock/latency qualification and the remaining macOS test-script
migrations remain open. No installed service/driver was replaced, user graph
edited, default device changed, or VM started in this slice. Windows/Linux
implementation expansion remains deferred. The parity matrix still reports
31 open release gates; this test result does not close those gates wholesale.
