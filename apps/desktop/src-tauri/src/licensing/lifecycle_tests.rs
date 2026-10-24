//! Hermetic Driver lifecycle checks with memory credentials, HTTP and clock.
use super::{LicensingState, automatic, operations, store::CredentialStore};
use mixrelay_licensing::{
    AccessStatus, Activation, LicenseClient, LicenseError, LicenseHttp, LicenseHttpResponse,
    ProductIdentity, SecretString,
};
use secrecy::ExposeSecret as _;
use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

const START: u64 = 1_700_000_000;
const DAY: u64 = 86_400;

#[derive(Default)]
struct MemoryCredentials(Mutex<Option<SecretString>>);
impl CredentialStore for MemoryCredentials {
    fn read(&self) -> Result<Option<SecretString>, LicenseError> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn write(&self, value: &SecretString) -> Result<(), LicenseError> {
        *self.0.lock().unwrap() = Some(value.clone());
        Ok(())
    }
}

type Reply = Result<(u16, serde_json::Value), LicenseError>;
struct Http {
    replies: Mutex<VecDeque<Reply>>,
    requests: Mutex<Vec<String>>,
    release: Option<Arc<tokio::sync::Semaphore>>,
    entered: tokio::sync::Notify,
}
impl LicenseHttp for Http {
    fn post<'a>(
        &'a self,
        operation: &'a str,
        _: Vec<(&'a str, &'a str)>,
    ) -> Pin<Box<dyn Future<Output = Result<LicenseHttpResponse, LicenseError>> + Send + 'a>> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(operation.to_owned());
            self.entered.notify_one();
            if let Some(release) = &self.release {
                release.acquire().await.unwrap().forget();
            }
            let (status, body) = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected request")?;
            Ok(LicenseHttpResponse {
                status,
                body: serde_json::to_vec(&body).unwrap(),
            })
        })
    }
}
fn http(
    replies: Vec<Reply>,
    release: Option<Arc<tokio::sync::Semaphore>>,
) -> (LicenseClient, Arc<Http>) {
    let http = Arc::new(Http {
        replies: Mutex::new(replies.into()),
        requests: Mutex::default(),
        release,
        entered: tokio::sync::Notify::new(),
    });
    (
        LicenseClient::with_http(ProductIdentity::new(1, 2, 3).unwrap(), http.clone()),
        http,
    )
}
fn valid(flag: &str, instance: Option<&str>) -> (u16, serde_json::Value) {
    (
        200,
        serde_json::json!({flag: true, "license_key":{"id":7,"status":if instance.is_some() {"active"} else {"inactive"}}, "instance": instance.map(|id| serde_json::json!({"id":id})), "meta":{"store_id":1,"product_id":2,"variant_id":3}}),
    )
}
fn state(store: &Arc<MemoryCredentials>, clock: &Arc<AtomicU64>) -> Arc<LicensingState> {
    let clock = clock.clone();
    Arc::new(LicensingState::with_clock(
        store.clone(),
        Arc::new(move || Ok(clock.load(Ordering::Relaxed))),
    ))
}
fn licensed(state: &LicensingState) -> u64 {
    state.snapshot(START).unwrap();
    let activation: Activation =
        serde_json::from_str(r#"{"licenseId":7,"instanceId":"seat-1","purchasedMajor":0}"#)
            .unwrap();
    state
        .verified(SecretString::from("test-key"), activation, START)
        .unwrap()
        .expires_at_unix
        .unwrap()
}

#[tokio::test]
async fn startup_and_first_connection_coalesce_and_restart_obeys_attempt_cooldown() {
    let store = Arc::new(MemoryCredentials::default());
    let clock = Arc::new(AtomicU64::new(START));
    let initial = state(&store, &clock);
    let expiry = licensed(&initial);
    let (client, http) = http(
        vec![
            Err(LicenseError::Unavailable),
            Ok(valid("valid", Some("seat-1"))),
        ],
        None,
    );
    let (startup, connection) = tokio::join!(
        automatic::check(&initial, &client),
        automatic::check(&initial, &client)
    );
    startup.unwrap();
    connection.unwrap();
    assert_eq!(http.requests.lock().unwrap().len(), 1);
    let restarted = state(&store, &clock);
    clock.store(START + DAY - 1, Ordering::Relaxed);
    automatic::check(&restarted, &client).await.unwrap();
    assert_eq!(http.requests.lock().unwrap().len(), 1);
    assert_eq!(
        restarted.snapshot(START + DAY - 1).unwrap().expires_at_unix,
        Some(expiry)
    );
    clock.store(START + DAY, Ordering::Relaxed);
    automatic::check(&restarted, &client).await.unwrap();
    assert_eq!(http.requests.lock().unwrap().len(), 2);
    assert_eq!(
        restarted.snapshot(START + DAY).unwrap().expires_at_unix,
        Some(START + 31 * DAY)
    );
}

#[tokio::test]
async fn pending_online_check_keeps_local_access_and_only_success_renews_the_lease() {
    let store = Arc::new(MemoryCredentials::default());
    let clock = Arc::new(AtomicU64::new(START));
    let initial = state(&store, &clock);
    let expiry = licensed(&initial);
    let release = Arc::new(tokio::sync::Semaphore::new(0));
    let (client, http) = http(
        vec![Ok(valid("valid", Some("seat-1")))],
        Some(release.clone()),
    );
    clock.store(START + DAY, Ordering::Relaxed);
    let checking = {
        let state = initial.clone();
        tokio::spawn(async move { automatic::check(&state, &client).await })
    };
    http.entered.notified().await;
    let pending = initial.snapshot(START + DAY).unwrap();
    assert_eq!(pending.status, AccessStatus::Licensed);
    assert_eq!(pending.expires_at_unix, Some(expiry));
    release.add_permits(1);
    checking.await.unwrap().unwrap();
    assert_eq!(
        initial.snapshot(START + DAY).unwrap().expires_at_unix,
        Some(START + 31 * DAY)
    );
}

#[tokio::test]
async fn transient_failures_never_clear_or_extend_the_lease_but_revocation_cleans_up() {
    for reply in [
        Err(LicenseError::Unavailable),
        Err(LicenseError::Rejected),
        Err(LicenseError::InvalidResponse),
        Err(LicenseError::WrongProduct),
        Ok((429, serde_json::Value::Null)),
        Ok((500, serde_json::Value::Null)),
        Ok((503, serde_json::Value::Null)),
    ] {
        let store = Arc::new(MemoryCredentials::default());
        let clock = Arc::new(AtomicU64::new(START));
        let initial = state(&store, &clock);
        let expiry = licensed(&initial);
        let (client, http) = http(vec![reply], None);
        automatic::check(&initial, &client).await.unwrap();
        assert_eq!(
            initial.snapshot(START + DAY).unwrap().expires_at_unix,
            Some(expiry)
        );
        assert!(initial.saved_activation().unwrap().is_some());
        let restarted = state(&store, &clock);
        assert_eq!(
            restarted.snapshot(expiry).unwrap().status,
            AccessStatus::Free
        );
        assert!(restarted.saved_activation().unwrap().is_some());
        assert_eq!(http.requests.lock().unwrap().len(), 1);
    }
    for error in [
        "license_key is disabled.",
        "license_key not found.",
        "instance not found.",
    ] {
        let store = Arc::new(MemoryCredentials::default());
        let clock = Arc::new(AtomicU64::new(START));
        let initial = state(&store, &clock);
        licensed(&initial);
        let (client, _) = http(
            vec![Ok((400, serde_json::json!({"valid":false,"error":error})))],
            None,
        );
        automatic::check(&initial, &client).await.unwrap();
        assert_eq!(initial.snapshot(START).unwrap().status, AccessStatus::Free);
        assert!(
            state(&store, &clock)
                .snapshot(START)
                .unwrap()
                .expires_at_unix
                .is_none()
        );
        assert!(initial.saved_activation().unwrap().is_none());
    }
}

#[tokio::test]
async fn manual_validation_bypasses_automatic_cooldown() {
    let store = Arc::new(MemoryCredentials::default());
    let clock = Arc::new(AtomicU64::new(START));
    let initial = state(&store, &clock);
    licensed(&initial);
    let (client, http) = http(
        vec![
            Err(LicenseError::Unavailable),
            Ok(valid("valid", Some("seat-1"))),
        ],
        None,
    );
    automatic::check(&initial, &client).await.unwrap();
    clock.store(START + 1, Ordering::Relaxed);
    initial.record_attempt().unwrap();
    assert_eq!(
        operations::validate_saved(&initial, &client)
            .await
            .unwrap()
            .expires_at_unix,
        Some(START + 1 + 30 * DAY)
    );
    automatic::check(&initial, &client).await.unwrap();
    assert_eq!(http.requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn uncertain_activation_survives_restart_and_cannot_create_another_seat() {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(MemoryCredentials::default());
    let clock = Arc::new(AtomicU64::new(START));
    let initial = state(&store, &clock);
    let journal = directory.path().join("intent.json");
    initial.set_journal_path(journal.clone());
    initial.snapshot(START).unwrap();
    let key = SecretString::from("test-key");
    let (client, http) = http(
        vec![Ok(valid("valid", None)), Err(LicenseError::Unavailable)],
        None,
    );
    assert_eq!(
        operations::activate_new(&initial, &client, &key, "Mac").await,
        Err(LicenseError::ActivationUncertain)
    );
    assert_eq!(
        operations::activate_new(&initial, &client, &key, "Mac").await,
        Err(LicenseError::ActivationUncertain)
    );
    let restarted = state(&store, &clock);
    restarted.set_journal_path(journal);
    restarted.snapshot(START).unwrap();
    assert_eq!(
        operations::activate_new(&restarted, &client, &key, "Mac").await,
        Err(LicenseError::ActivationUncertain)
    );
    assert_eq!(
        http.requests.lock().unwrap().as_slice(),
        ["validate", "activate"]
    );
    assert!(restarted.saved_activation().unwrap().is_none());
}

#[tokio::test]
async fn device_release_cleans_up_an_already_absent_activation_but_not_a_transient_failure() {
    for (reply, removed) in [
        (
            Ok((
                404,
                serde_json::json!({"valid":false,"error":"instance not found."}),
            )),
            true,
        ),
        (Ok((503, serde_json::Value::Null)), false),
    ] {
        let store = Arc::new(MemoryCredentials::default());
        let clock = Arc::new(AtomicU64::new(START));
        let initial = state(&store, &clock);
        licensed(&initial);
        let (client, http) = http(vec![reply], None);
        let result = operations::deactivate_saved(&initial, &client).await;
        assert_eq!(result.is_ok(), removed);
        assert_eq!(initial.saved_activation().unwrap().is_none(), removed);
        let restored = state(&store, &clock);
        assert_eq!(
            restored.snapshot(START).unwrap().status == AccessStatus::Free,
            removed
        );
        assert_eq!(http.requests.lock().unwrap().as_slice(), ["validate"]);
    }
}

/// A stored record from another build: the activation lacks a field this build requires.
fn unreadable_paid_record(store: &MemoryCredentials) {
    *store.0.lock().unwrap() = Some(SecretString::from(
        serde_json::json!({
            "record": {"initializedAtUnix": START, "latestObservedAtUnix": START, "paidVerifiedAtUnix": START,
                       "activation": {"licenseId": 7, "instanceId": "seat-1"}},
            "key": "test-key",
        })
        .to_string(),
    ));
}

#[tokio::test]
async fn an_unreadable_paid_record_is_restored_by_online_validation_even_across_a_restart() {
    let store = Arc::new(MemoryCredentials::default());
    let clock = Arc::new(AtomicU64::new(START));
    unreadable_paid_record(&store);
    let original = store.0.lock().unwrap().clone();
    let first = state(&store, &clock);
    // The app keeps working as free while the seat waits for validation.
    assert_eq!(first.snapshot(START).unwrap().status, AccessStatus::Free);
    // A transient failure keeps the recovered seat and the unreadable record that holds it.
    let (offline, _) = http(vec![Err(LicenseError::Unavailable)], None);
    automatic::check(&first, &offline).await.unwrap();
    assert!(first.salvaged_seat().is_some());
    assert_eq!(
        store
            .0
            .lock()
            .unwrap()
            .as_ref()
            .map(|value| value.expose_secret().to_owned()),
        original
            .as_ref()
            .map(|value| value.expose_secret().to_owned()),
        "the record holding the key is not overwritten before the seat is restored"
    );
    // After a restart the seat is recovered again and restored without a new activation.
    let restarted = state(&store, &clock);
    restarted.snapshot(START).unwrap();
    let (online, requests) = http(vec![Ok(valid("valid", Some("seat-1")))], None);
    operations::validate_saved(&restarted, &online)
        .await
        .unwrap();
    assert_eq!(*requests.requests.lock().unwrap(), ["validate"]);
    assert_eq!(
        restarted.snapshot(START).unwrap().status,
        AccessStatus::Licensed
    );
    assert!(restarted.salvaged_seat().is_none());
    let reopened = state(&store, &clock);
    assert_eq!(
        reopened.snapshot(START).unwrap().status,
        AccessStatus::Licensed
    );
    assert!(
        reopened.saved_activation().unwrap().is_some(),
        "saved in the current format"
    );
}

#[tokio::test]
async fn a_revoked_recovered_seat_ends_recovery_and_saves_a_free_record() {
    let store = Arc::new(MemoryCredentials::default());
    let clock = Arc::new(AtomicU64::new(START));
    unreadable_paid_record(&store);
    let state_now = state(&store, &clock);
    state_now.snapshot(START).unwrap();
    let (client, _) = http(
        vec![Ok((
            400,
            serde_json::json!({"valid":false,"error":"license_key is disabled."}),
        ))],
        None,
    );
    assert!(
        operations::validate_saved(&state_now, &client)
            .await
            .is_err()
    );
    assert!(state_now.salvaged_seat().is_none());
    state_now.snapshot(START).unwrap();
    let reopened = state(&store, &clock);
    assert_eq!(reopened.snapshot(START).unwrap().status, AccessStatus::Free);
    assert!(
        reopened.salvaged_seat().is_none(),
        "the free record replaced the unreadable one"
    );
}
