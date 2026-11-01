use mixrelay_audio_io::{VirtualEndpointCapabilities, VirtualEndpointSettings};
use mixrelay_test_support::reply_fixture::refresh_reply_fixture;

use super::{DriverSnapshot, DriverState};

/// A stale fixture is rewritten and this fails; review the diff and commit it
/// (`pnpm fixtures:replies` reruns every fixture owner).
#[test]
fn driver_snapshot_reply_fixture_matches_the_serialized_contract() {
    let snapshot = DriverSnapshot {
        protocol_version: 1,
        revision: 5,
        state: DriverState::Ready,
        installed_version: Some("1.2.10".to_owned()),
        owner_process_id: Some(412),
        settings: VirtualEndpointSettings::configurable(&VirtualEndpointCapabilities {
            max_render_channels: 2,
            max_capture_channels: 2,
            supported_sample_rates: vec![44_100, 48_000],
            supports_duplex: true,
            supports_dynamic_creation: true,
        }),
        persistence_error: Some("disk full".to_owned()),
    };
    assert!(
        !refresh_reply_fixture("driver-snapshot", &snapshot).unwrap(),
        "the driver-snapshot reply fixture was stale and has been rewritten; review and commit it"
    );
}
