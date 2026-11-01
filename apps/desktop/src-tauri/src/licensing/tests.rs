use mixrelay_licensing::{AccessSnapshot, AccessStatus};
use std::collections::BTreeMap;

use mixrelay_test_support::{
    every_variant,
    reply_fixture::{refresh_reply_fixture, variants},
};

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
    let statuses = BTreeMap::from([(
        "status",
        variants(&every_variant!(AccessStatus {
            Free,
            Licensed,
            ClockInvalid,
        })),
    )]);
    let status_stale = refresh_reply_fixture("licensing-status", &status).unwrap();
    let statuses_stale = refresh_reply_fixture("licensing-status.variants", &statuses).unwrap();
    assert!(
        !status_stale && !statuses_stale,
        "licensing-status reply fixtures were stale and have been rewritten; review and commit them"
    );
}
