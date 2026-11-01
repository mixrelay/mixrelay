use mixrelay_licensing::{AccessSnapshot, AccessStatus};
use mixrelay_test_support::reply_fixture::refresh_reply_fixture;

use super::LicensingStatus;

/// A stale fixture is rewritten and this fails; review the diff and commit it
/// (`pnpm fixtures:replies` reruns every fixture owner).
#[test]
fn licensing_status_reply_fixture_matches_the_serialized_contract() {
    let status = LicensingStatus {
        access: AccessSnapshot {
            status: AccessStatus::Licensed,
            expires_at_unix: Some(1_700_000_000),
            remaining_days: 12,
        },
        configured: true,
        has_activation: true,
    };
    assert!(
        !refresh_reply_fixture("licensing-status", &status).unwrap(),
        "the licensing-status reply fixture was stale and has been rewritten; review and commit it"
    );
}
