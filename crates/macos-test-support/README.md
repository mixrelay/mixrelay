# macOS native test fixtures

Run Cargo checks from the repository root through the supported wrapper. It
prepares the HOST Cargo static libraries required by the native fixtures:

```sh
node scripts/lab/native-check.mjs cargo test -p mixrelay-macos-test-support
```

Create or select a dedicated MixRelay Virtual IO with at least two input and two
output channels, then explicitly provide its current CoreAudio UID. The fixture
does not select a default device, guess another device, or recreate a deleted
Virtual IO. Missing selection or unavailable channels fail the test, without skips.

```sh
MIXRELAY_TEST_ENDPOINT_UID='io.mixrelay.audio.endpoint.<actual-device-uuid>' node scripts/lab/native-check.mjs cargo test -p mixrelay-macos-test-support --lib -- --test-threads=1
MIXRELAY_TEST_ENDPOINT_UID='io.mixrelay.audio.endpoint.<actual-device-uuid>' node scripts/lab/native-check.mjs cargo test -p mixrelay-engine-service --bin mixrelay-engine-service -- --test-threads=1
```

Replace the placeholder with the exact UID of the explicitly chosen test device.
`DeviceFixture` owns only its process-private aggregate and captures the selected
member UID once. Disconnect/reconnect removes and recreates that aggregate with
the same member; it does not delete the member or alter system default devices.
Other native tests may require their own explicit fixture variables, including
the primary/secondary device selections for native group lifetime tests.
