# macOS native test fixtures

Create or select a dedicated ToneNari Virtual IO with at least two input and two
output channels, then explicitly provide its current CoreAudio UID. The fixture
does not select a default device, guess another device, or recreate a deleted
Virtual IO. Missing selection or unavailable channels fail the test, without skips.

```sh
TONENARI_TEST_ENDPOINT_UID='io.tonenari.audio.endpoint.<actual-device-uuid>' cargo test -p tonenari-macos-test-support --lib -- --test-threads=1
TONENARI_TEST_ENDPOINT_UID='io.tonenari.audio.endpoint.<actual-device-uuid>' cargo test -p tonenari-engine-service --bin tonenari-engine-service -- --test-threads=1
```

Replace the placeholder with the exact UID of the explicitly chosen test device.
`DeviceFixture` owns only its process-private aggregate and captures the selected
member UID once. Disconnect/reconnect removes and recreates that aggregate with
the same member; it does not delete the member or alter system default devices.
Other native tests may require their own explicit fixture variables, including
the primary/secondary device selections for native group lifetime tests.
