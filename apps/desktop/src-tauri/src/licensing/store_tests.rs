use std::sync::atomic::{AtomicBool, Ordering};

use super::*;

#[derive(Default)]
struct MemoryCredentials {
    value: Mutex<Option<SecretString>>,
    fail: AtomicBool,
}

impl CredentialStore for MemoryCredentials {
    fn read(&self) -> Result<Option<SecretString>, LicenseError> {
        if self.fail.load(Ordering::Relaxed) {
            return Err(LicenseError::StorageUnavailable);
        }
        Ok(self.value.lock().expect("mock lock").clone())
    }

    fn write(&self, value: &SecretString) -> Result<(), LicenseError> {
        if self.fail.load(Ordering::Relaxed) {
            return Err(LicenseError::StorageUnavailable);
        }
        *self.value.lock().expect("mock lock") = Some(value.clone());
        Ok(())
    }
}

fn activation() -> Activation {
    serde_json::from_str(r#"{"licenseId":"70000000-0000-4000-8000-000000000007","instanceId":"a0000000-0000-4000-8000-000000000001","purchasedMajor":0}"#)
        .expect("seat")
}

#[test]
fn restart_and_deactivation_never_restart_the_trial() {
    let store = Arc::new(MemoryCredentials::default());
    let first = LicensingState::with_store(store.clone());
    let trial = first.snapshot(1_000).expect("trial");
    let restarted = LicensingState::with_store(store.clone());
    assert_eq!(
        restarted
            .snapshot(2_000)
            .expect("saved trial")
            .expires_at_unix,
        trial.expires_at_unix
    );
    restarted
        .verified(SecretString::from("secret"), activation(), 2_000)
        .expect("verified");
    let cleared = restarted.clear_paid(3_000).expect("deactivated");
    assert_eq!(cleared.expires_at_unix, trial.expires_at_unix);
    assert!(restarted.saved_activation().expect("saved").is_none());
}

#[test]
fn an_unreadable_record_is_replaced_and_an_inaccessible_store_stays_an_error() {
    let store = Arc::new(MemoryCredentials::default());
    *store.value.lock().expect("mock lock") = Some(SecretString::from("invalid"));
    let state = LicensingState::with_store(store.clone());
    let access = state
        .snapshot(1_000)
        .expect("a corrupt record must not block licensing");
    assert_eq!(access.status, mixrelay_licensing::AccessStatus::Free);
    let saved = store
        .value
        .lock()
        .expect("mock lock")
        .clone()
        .expect("rewritten");
    assert!(
        Vault::decode(&saved).is_ok(),
        "the replacement is a readable record"
    );
    assert!(state.saved_activation().expect("readable").is_none());
    // A store that cannot be read is not overwritten: its record may still be valid.
    store.fail.store(true, Ordering::Relaxed);
    assert!(matches!(
        LicensingState::with_store(store).snapshot(1_000),
        Err(LicenseError::StorageUnavailable)
    ));
}

#[test]
fn failed_save_retains_the_remote_seat_for_recovery_without_reactivation() {
    let store = Arc::new(MemoryCredentials::default());
    let state = LicensingState::with_store(store.clone());
    state.snapshot(1_000).expect("initial");
    store.fail.store(true, Ordering::Relaxed);
    assert!(matches!(
        state.verified(SecretString::from("secret"), activation(), 2_000),
        Err(LicenseError::StorageUnavailable)
    ));
    assert_eq!(
        state
            .saved_activation()
            .expect("retained")
            .expect("seat")
            .1
            .instance_id(),
        "a0000000-0000-4000-8000-000000000001"
    );
    store.fail.store(false, Ordering::Relaxed);
    state.snapshot(2_001).expect("save retry");
    let reopened = LicensingState::with_store(store);
    reopened.snapshot(2_002).expect("restored");
    assert_eq!(
        reopened
            .saved_activation()
            .expect("saved")
            .expect("seat")
            .1
            .instance_id(),
        "a0000000-0000-4000-8000-000000000001"
    );
}

#[test]
fn impossible_key_activation_pair_is_rejected_without_resetting_saved_data() {
    let record = EntitlementRecord::new(1_000);
    let value = serde_json::to_string(&serde_json::json!({
        "record": record,
        "key": "secret",
    }))
    .expect("fixture");
    assert!(matches!(
        Vault::decode(&SecretString::from(value)),
        Err(LicenseError::StorageUnavailable)
    ));
}

#[test]
fn credential_write_failure_recovers_the_committed_seat_after_restart() {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("receipt.json");
    let store = Arc::new(MemoryCredentials::default());
    let state = LicensingState::with_store(store.clone());
    state.set_journal_path(path.clone());
    let trial = state.snapshot(1_000).expect("trial");
    store.fail.store(true, Ordering::Relaxed);
    assert!(
        state
            .verified(SecretString::from("secret"), activation(), 2_000)
            .is_err()
    );
    drop(state);
    store.fail.store(false, Ordering::Relaxed);
    let reopened = LicensingState::with_store(store);
    reopened.set_journal_path(path.clone());
    assert_eq!(
        reopened
            .snapshot(2_001)
            .expect("trial retained")
            .expires_at_unix,
        trial.expires_at_unix
    );
    assert_eq!(
        reopened
            .recoverable_activation(&SecretString::from("secret"))
            .expect("receipt")
            .expect("seat")
            .instance_id(),
        "a0000000-0000-4000-8000-000000000001"
    );
    assert!(matches!(
        reopened.recoverable_activation(&SecretString::from("mistyped")),
        Err(LicenseError::InvalidInput)
    ));
    assert!(path.exists());
    reopened
        .verified(SecretString::from("secret"), activation(), 2_002)
        .expect("validated seat saved");
    assert!(!path.exists());
}

#[test]
fn confirmed_clear_cannot_restore_an_old_paid_lease_after_restart() {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("receipt.json");
    let store = Arc::new(MemoryCredentials::default());
    let state = LicensingState::with_store(store.clone());
    state.set_journal_path(path.clone());
    state.snapshot(1_000).expect("trial");
    state
        .verified(SecretString::from("secret"), activation(), 2_000)
        .expect("paid");
    store.fail.store(true, Ordering::Relaxed);
    let expired_trial = 1_000 + 14 * 86_400;
    assert!(state.clear_paid(expired_trial).is_err());
    drop(state);
    store.fail.store(false, Ordering::Relaxed);
    let reopened = LicensingState::with_store(store);
    reopened.set_journal_path(path.clone());
    assert_eq!(
        reopened
            .snapshot(expired_trial + 1)
            .expect("clear restored")
            .status,
        mixrelay_licensing::AccessStatus::Free
    );
    assert!(reopened.saved_activation().expect("cleared").is_none());
    assert!(!path.exists());
}

#[test]
fn journal_io_failure_retains_activation_and_reconciles_confirmed_clear_in_memory() {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("blocked").join("receipt.json");
    let state = LicensingState::with_store(Arc::new(MemoryCredentials::default()));
    state.set_journal_path(path);
    state.snapshot(1_000).expect("trial");
    std::fs::File::create(directory.path().join("blocked")).expect("block parent");
    assert!(state.preflight_remote_change().is_err());
    assert!(
        state
            .verified(SecretString::from("secret"), activation(), 2_000)
            .is_err()
    );
    assert_eq!(
        state
            .saved_activation()
            .expect("retained")
            .expect("seat")
            .1
            .instance_id(),
        "a0000000-0000-4000-8000-000000000001"
    );
    let expired_trial = 1_000 + 14 * 86_400;
    assert!(state.clear_paid(expired_trial).is_err());
    assert!(state.saved_activation().expect("cleared").is_none());
}

#[test]
fn rejected_recovery_can_clear_the_receipt_and_permit_a_fresh_activation() {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("receipt.json");
    let state = LicensingState::with_store(Arc::new(MemoryCredentials::default()));
    state.set_journal_path(path.clone());
    state.snapshot(1_000).expect("trial");
    recovery::write(
        &path,
        &RemoteChange::Activated {
            activation: activation(),
            key_fingerprint: recovery::key_fingerprint("secret"),
        },
    )
    .expect("pending remote seat");
    assert!(
        state
            .recoverable_activation(&SecretString::from("secret"))
            .expect("recoverable")
            .is_some()
    );
    state
        .clear_paid(2_000)
        .expect("confirmed exact-key rejection");
    assert!(
        state
            .recoverable_activation(&SecretString::from("new-secret"))
            .expect("fresh activation available")
            .is_none()
    );
    assert!(!path.exists());
}
