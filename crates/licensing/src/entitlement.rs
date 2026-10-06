use serde::{Deserialize, Serialize};

use crate::{Activation, LicenseError};

const SECONDS_PER_DAY: u64 = 86_400;
const TRIAL_SECONDS: u64 = 14 * SECONDS_PER_DAY;
const PAID_LEASE_SECONDS: u64 = 30 * SECONDS_PER_DAY;
// Keeps persisted timestamps within the conventional four-digit UTC year range.
const MAX_UNIX_TIMESTAMP: u64 = 253_402_300_799;

/// Access decision after observing the local Unix clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessStatus {
    /// The original fourteen-day trial has not ended.
    Trial,
    /// A successful paid check is within its thirty-day offline lease.
    Licensed,
    /// Neither the trial nor a paid lease remains active.
    Expired,
    /// The record or clock is invalid, so access cannot be inferred safely.
    ClockInvalid,
}

/// Current access, its exact Unix expiry, and whole or partial days rounded up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessSnapshot {
    /// The access state at the observed clock value.
    pub status: AccessStatus,
    /// The selected trial or paid expiry; absent when the clock is invalid.
    pub expires_at_unix: Option<u64>,
    /// Days remaining, rounded up; zero once expired or clock-invalid.
    pub remaining_days: u64,
}

/// Persisted offline entitlement policy. The caller owns durable storage and clock sourcing.
///
/// Deserialization rejects inconsistent fields. The first trial timestamp and the latest
/// observed clock are preserved across restarts; remote failures do not renew either lease.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "StoredEntitlement")]
pub struct EntitlementRecord {
    trial_started_at_unix: u64,
    latest_observed_at_unix: u64,
    paid_verified_at_unix: Option<u64>,
    activation: Option<Activation>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredEntitlement {
    trial_started_at_unix: u64,
    latest_observed_at_unix: u64,
    paid_verified_at_unix: Option<u64>,
    activation: Option<Activation>,
}

impl TryFrom<StoredEntitlement> for EntitlementRecord {
    type Error = LicenseError;

    fn try_from(stored: StoredEntitlement) -> Result<Self, Self::Error> {
        let record = Self {
            trial_started_at_unix: stored.trial_started_at_unix,
            latest_observed_at_unix: stored.latest_observed_at_unix,
            paid_verified_at_unix: stored.paid_verified_at_unix,
            activation: stored.activation,
        };
        record.validate()?;
        Ok(record)
    }
}

impl EntitlementRecord {
    /// Begin the sole trial at `now`, a Unix timestamp in seconds.
    ///
    /// An out-of-range timestamp produces a record that only reports `ClockInvalid`;
    /// callers should obtain a valid clock before persisting it.
    #[must_use]
    pub fn new(now: u64) -> Self {
        Self {
            trial_started_at_unix: now,
            latest_observed_at_unix: now,
            paid_verified_at_unix: None,
            activation: None,
        }
    }

    /// Observe `now` and advance the saved clock only when it has not moved backwards.
    ///
    /// The caller must persist this record after a successful observation. A rollback,
    /// out-of-range clock, or invalid record returns `ClockInvalid` without mutation.
    #[must_use]
    pub fn observe(&mut self, now: u64) -> AccessSnapshot {
        let snapshot = self.access_at(now);
        if snapshot.status != AccessStatus::ClockInvalid {
            self.latest_observed_at_unix = now;
        }
        snapshot
    }

    /// Evaluate access at `now` without advancing the persisted clock watermark.
    ///
    /// Use this for read-only admission checks. The Unix timestamp must be at least the
    /// latest saved observation; rollback, out-of-range time, or invalid state reports
    /// `ClockInvalid` without changing the record.
    #[must_use]
    pub fn access_at(&self, now: u64) -> AccessSnapshot {
        if self.validate().is_err() || !valid_time(now) || now < self.latest_observed_at_unix {
            return AccessSnapshot {
                status: AccessStatus::ClockInvalid,
                expires_at_unix: None,
                remaining_days: 0,
            };
        }
        self.snapshot_at(now)
    }

    /// Start or renew the paid offline lease only after successful remote verification.
    ///
    /// `activation` must identify a valid remote seat and `now` is the verified Unix time.
    /// Returns the licensed snapshot. On invalid state, seat, or clock, returns an error
    /// without modifying the record.
    ///
    /// # Errors
    /// Returns `InvalidState` for inconsistent stored data, `InvalidResponse` for an invalid
    /// activation, or `ClockInvalid` for rollback or out-of-range time.
    pub fn record_verified(
        &mut self,
        activation: Activation,
        now: u64,
    ) -> Result<AccessSnapshot, LicenseError> {
        self.validate()?;
        if !valid_time(now)
            || now < self.latest_observed_at_unix
            || now
                .checked_add(PAID_LEASE_SECONDS)
                .is_none_or(|end| !valid_time(end))
        {
            return Err(LicenseError::ClockInvalid);
        }
        if !valid_activation(&activation) {
            return Err(LicenseError::InvalidResponse);
        }
        self.latest_observed_at_unix = now;
        self.paid_verified_at_unix = Some(now);
        self.activation = Some(activation);
        Ok(self.snapshot_at(now))
    }

    /// Clear the paid seat and lease after confirmed deactivation or rejection.
    ///
    /// This preserves the original trial start and latest observed clock.
    pub fn clear_paid(&mut self) {
        self.paid_verified_at_unix = None;
        self.activation = None;
    }

    /// Return the paid seat, if one is saved in a valid record.
    #[must_use]
    pub fn activation(&self) -> Option<&Activation> {
        self.validate().ok()?;
        self.activation.as_ref()
    }

    fn validate(&self) -> Result<(), LicenseError> {
        if !valid_time(self.trial_started_at_unix)
            || !valid_time(self.latest_observed_at_unix)
            || self.trial_started_at_unix > self.latest_observed_at_unix
            || self
                .trial_started_at_unix
                .checked_add(TRIAL_SECONDS)
                .is_none_or(|end| !valid_time(end))
        {
            return Err(LicenseError::InvalidState);
        }
        match (self.paid_verified_at_unix, self.activation.as_ref()) {
            (None, None) => Ok(()),
            (Some(verified), Some(activation))
                if verified >= self.trial_started_at_unix
                    && verified <= self.latest_observed_at_unix
                    && verified
                        .checked_add(PAID_LEASE_SECONDS)
                        .is_some_and(valid_time)
                    && valid_activation(activation) =>
            {
                Ok(())
            }
            (None | Some(_), Some(_)) | (Some(_), None) => Err(LicenseError::InvalidState),
        }
    }

    fn snapshot_at(&self, now: u64) -> AccessSnapshot {
        let trial_end = self.trial_started_at_unix + TRIAL_SECONDS;
        let paid_end = self
            .paid_verified_at_unix
            .map(|verified| verified + PAID_LEASE_SECONDS);
        let (status, expiry) = if let Some(end) = paid_end.filter(|&end| now < end) {
            (AccessStatus::Licensed, end)
        } else if now < trial_end {
            (AccessStatus::Trial, trial_end)
        } else {
            (
                AccessStatus::Expired,
                paid_end.map_or(trial_end, |end| end.max(trial_end)),
            )
        };
        let seconds_remaining = expiry.saturating_sub(now);
        AccessSnapshot {
            status,
            expires_at_unix: Some(expiry),
            remaining_days: seconds_remaining.div_ceil(SECONDS_PER_DAY),
        }
    }
}

const fn valid_time(unix: u64) -> bool {
    unix <= MAX_UNIX_TIMESTAMP
}

fn valid_activation(activation: &Activation) -> bool {
    let instance_id = activation.instance_id();
    activation.license_id() != 0
        && !instance_id.trim().is_empty()
        && instance_id.len() <= 255
        && !instance_id.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: u64 = 1_700_000_000;

    fn activation() -> Activation {
        serde_json::from_str(r#"{"licenseId":7,"instanceId":"seat-1"}"#)
            .expect("valid seat fixture")
    }

    #[test]
    fn trial_boundary_and_rounding_are_exact() {
        let mut record = EntitlementRecord::new(START);
        assert_eq!(record.observe(START).status, AccessStatus::Trial);
        assert_eq!(record.observe(START).remaining_days, 14);
        let before_end = record.observe(START + TRIAL_SECONDS - 1);
        assert_eq!(before_end.expires_at_unix, Some(START + TRIAL_SECONDS));
        assert_eq!(before_end.remaining_days, 1);
        let at_end = record.observe(START + TRIAL_SECONDS);
        assert_eq!(at_end.status, AccessStatus::Expired);
        assert_eq!(at_end.remaining_days, 0);
    }

    #[test]
    fn verified_lease_expires_and_only_new_verification_renews_it() {
        let mut record = EntitlementRecord::new(START);
        let verified_at = START + TRIAL_SECONDS;
        let paid = record
            .record_verified(activation(), verified_at)
            .expect("verified");
        assert_eq!(paid.status, AccessStatus::Licensed);
        assert_eq!(paid.remaining_days, 30);
        assert_eq!(paid.expires_at_unix, Some(verified_at + PAID_LEASE_SECONDS));
        assert_eq!(
            record.observe(verified_at + PAID_LEASE_SECONDS - 1).status,
            AccessStatus::Licensed
        );
        assert_eq!(
            record.observe(verified_at + PAID_LEASE_SECONDS).status,
            AccessStatus::Expired
        );
        assert_eq!(
            record
                .record_verified(activation(), verified_at + PAID_LEASE_SECONDS)
                .expect("fresh verification")
                .status,
            AccessStatus::Licensed
        );
    }

    #[test]
    fn restart_and_observation_do_not_reset_trial_or_paid_lease() {
        let mut original = EntitlementRecord::new(START);
        original
            .record_verified(activation(), START + 1)
            .expect("verified");
        let encoded = serde_json::to_string(&original).expect("serialize");
        let mut loaded: EntitlementRecord = serde_json::from_str(&encoded).expect("reload");
        assert_eq!(loaded, original);
        assert_eq!(
            loaded.observe(START + 100).expires_at_unix,
            Some(START + 1 + PAID_LEASE_SECONDS)
        );
        assert_eq!(
            loaded.observe(START + 200).expires_at_unix,
            Some(START + 1 + PAID_LEASE_SECONDS)
        );
    }

    #[test]
    fn rollback_cannot_renew_or_grant_access() {
        let mut record = EntitlementRecord::new(START);
        assert_eq!(record.observe(START + 100).status, AccessStatus::Trial);
        let before = record.clone();
        assert_eq!(
            record.observe(START + 99).status,
            AccessStatus::ClockInvalid
        );
        assert_eq!(record, before);
        assert_eq!(
            record.record_verified(activation(), START + 99),
            Err(LicenseError::ClockInvalid)
        );
        assert_eq!(record, before);
        assert_eq!(record.observe(START + 100).status, AccessStatus::Trial);
    }

    #[test]
    fn read_only_access_does_not_move_watermark() {
        let mut record = EntitlementRecord::new(START);
        let original = record.clone();
        assert_eq!(record.access_at(START + 100).status, AccessStatus::Trial);
        assert_eq!(
            record.access_at(START - 1).status,
            AccessStatus::ClockInvalid
        );
        assert_eq!(record, original);
        assert_eq!(record.observe(START + 50).status, AccessStatus::Trial);
    }

    #[test]
    fn read_only_access_checks_persisted_watermark_after_restart() {
        let mut record = EntitlementRecord::new(START);
        assert_eq!(record.observe(START + 100).status, AccessStatus::Trial);
        let encoded = serde_json::to_string(&record).expect("serialize");
        let mut loaded: EntitlementRecord = serde_json::from_str(&encoded).expect("reload");
        let saved = loaded.clone();
        assert_eq!(
            loaded.access_at(START + 99).status,
            AccessStatus::ClockInvalid
        );
        assert_eq!(loaded.access_at(START + 101).status, AccessStatus::Trial);
        assert_eq!(loaded, saved);
        assert_eq!(loaded.observe(START + 101).status, AccessStatus::Trial);
        assert_eq!(
            loaded.access_at(START + 100).status,
            AccessStatus::ClockInvalid
        );
    }

    #[test]
    fn clearing_paid_retains_the_original_trial() {
        let mut record = EntitlementRecord::new(START);
        record
            .record_verified(activation(), START + 1)
            .expect("verified");
        record.clear_paid();
        assert!(record.activation().is_none());
        assert_eq!(record.observe(START + 2).status, AccessStatus::Trial);
        assert_eq!(
            record.observe(START + TRIAL_SECONDS).status,
            AccessStatus::Expired
        );
    }

    #[test]
    fn deactivation_restores_remaining_trial_without_extra_days() {
        let mut record = EntitlementRecord::new(START);
        record
            .record_verified(activation(), START)
            .expect("verified");
        record.clear_paid();
        let trial = record.observe(START + 10 * SECONDS_PER_DAY);
        assert_eq!(trial.status, AccessStatus::Trial);
        assert_eq!(trial.remaining_days, 4);
        assert_eq!(trial.expires_at_unix, Some(START + TRIAL_SECONDS));
    }

    #[test]
    fn snapshot_json_uses_the_desktop_contract() {
        let snapshot = EntitlementRecord::new(START).observe(START);
        let json = serde_json::to_value(snapshot).expect("snapshot JSON");
        assert_eq!(json["status"], "trial");
        assert_eq!(json["expiresAtUnix"], START + TRIAL_SECONDS);
        assert_eq!(json["remainingDays"], 14);
    }

    #[test]
    fn invalid_persisted_fields_are_rejected() {
        let valid = serde_json::to_value(EntitlementRecord::new(START)).expect("serialize");
        let mut examples = Vec::new();
        let mut future_start = valid.clone();
        future_start["trialStartedAtUnix"] = (START + 1).into();
        examples.push(future_start);
        let mut activation_without_time = valid.clone();
        activation_without_time["activation"] = serde_json::to_value(activation()).expect("seat");
        examples.push(activation_without_time);
        let mut time_without_activation = valid.clone();
        time_without_activation["paidVerifiedAtUnix"] = START.into();
        examples.push(time_without_activation);
        let mut paid_in_future = valid.clone();
        paid_in_future["paidVerifiedAtUnix"] = (START + 1).into();
        paid_in_future["activation"] = serde_json::to_value(activation()).expect("seat");
        examples.push(paid_in_future);
        let mut invalid_seat = valid.clone();
        invalid_seat["paidVerifiedAtUnix"] = START.into();
        invalid_seat["activation"] = serde_json::json!({"licenseId": 0, "instanceId": "seat-1"});
        examples.push(invalid_seat);
        let mut out_of_range = valid;
        out_of_range["latestObservedAtUnix"] = u64::MAX.into();
        examples.push(out_of_range);
        for value in examples {
            assert!(serde_json::from_value::<EntitlementRecord>(value).is_err());
        }
    }

    #[test]
    fn invalid_clock_and_seat_do_not_mutate_record() {
        let mut record = EntitlementRecord::new(START);
        let original = record.clone();
        assert_eq!(record.observe(u64::MAX).status, AccessStatus::ClockInvalid);
        assert_eq!(
            record.record_verified(activation(), u64::MAX),
            Err(LicenseError::ClockInvalid)
        );
        let invalid_seat: Activation =
            serde_json::from_str(r#"{"licenseId":0,"instanceId":"seat-1"}"#).expect("shape valid");
        assert_eq!(
            record.record_verified(invalid_seat, START),
            Err(LicenseError::InvalidResponse)
        );
        assert_eq!(record, original);
    }
}
