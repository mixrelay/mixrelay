# macOS native test fixtures

Run Cargo checks from the repository root through the supported wrapper. It
prepares the HOST Cargo static libraries required by the native fixtures:

```sh
node scripts/lab/native-check.mjs cargo test -p mixrelay-service --bin mixrelay-service native_callback_and_hal_contracts -- --exact
```

`cargo test -p mixrelay-macos-test-support` only builds the fixtures and tests the
bounded process runner; the engine test above executes the fixtures.

The crate builds the native C contract tests (callback clock, HAL bridge,
application clock and PCM rings) with ASan and UBSan, and
`assert_native_contracts` runs each one in an owned, bounded process. None of
them selects, creates or changes an audio device. `device_io_timing_layout`
reports the callback timing layout the engine's C header declares, which the
engine compares with its Rust definition.

Engine device recovery, buffer, sample-rate and private-group lifetime tests run
by default against the transport's simulated `CoreAudio` HAL: the production
aggregate and device-preparation code runs unchanged and only the `CoreAudio`
calls are simulated, so they need no installed device or environment selection.
