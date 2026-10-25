//! Shared desktop/remover credential format; removal preserves the clock watermark.

use secrecy::ExposeSecret as _;
use serde::{Deserialize, Serialize};

use crate::{EntitlementRecord, LicenseError, SecretString};

#[cfg(any(target_os = "linux", test))]
mod background_read;
#[cfg(target_os = "linux")]
mod secret_service_read;

const MAX_RECORD_BYTES: usize = 4_096;

/// Native service name. Development builds cannot overwrite product credentials.
pub const LOCAL_LICENSE_SERVICE: &str = if cfg!(debug_assertions) {
    "MixRelay.licensing.dev"
} else {
    "MixRelay.licensing"
};

/// The entitlement and its optional paid key are persisted together in one native entry.
#[derive(Clone)]
pub struct StoredLicense {
    /// Original clock history and the currently verified paid seat.
    pub record: EntitlementRecord,
    /// The secret paid key; it never crosses the desktop's UI IPC boundary.
    pub key: Option<SecretString>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedLicense {
    record: EntitlementRecord,
    key: Option<SecretString>,
}

#[derive(Serialize)]
struct LicenseView<'a> {
    record: &'a EntitlementRecord,
    key: Option<&'a str>,
}

impl StoredLicense {
    /// Decode the bounded native entry, requiring the paid key and seat to agree.
    ///
    /// # Errors
    /// Returns `StorageUnavailable` for malformed, oversized or inconsistent data.
    pub fn decode(value: &SecretString) -> Result<Self, LicenseError> {
        if value.expose_secret().len() > MAX_RECORD_BYTES {
            return Err(LicenseError::StorageUnavailable);
        }
        let saved: SavedLicense = serde_json::from_str(value.expose_secret())
            .map_err(|_| LicenseError::StorageUnavailable)?;
        if saved.record.activation().is_some() != saved.key.is_some()
            || saved.key.as_ref().is_some_and(|key| {
                let key = key.expose_secret();
                key.trim().is_empty() || key.len() > 255 || key.chars().any(char::is_control)
            })
        {
            return Err(LicenseError::StorageUnavailable);
        }
        Ok(Self {
            record: saved.record,
            key: saved.key,
        })
    }

    /// Encode the entitlement and key for one atomic native credential write.
    ///
    /// # Errors
    /// Returns `StorageUnavailable` if serialization fails or exceeds the entry budget.
    pub fn encode(&self) -> Result<SecretString, LicenseError> {
        let value = serde_json::to_string(&LicenseView {
            record: &self.record,
            key: self.key.as_ref().map(secrecy::ExposeSecret::expose_secret),
        })
        .map_err(|_| LicenseError::StorageUnavailable)?;
        if value.len() > MAX_RECORD_BYTES {
            return Err(LicenseError::StorageUnavailable);
        }
        Ok(SecretString::from(value))
    }
}

/// Read paid access for the local engine without exposing the key or renewing its lease.
/// Missing, malformed, inaccessible or expired credentials retain free access.
#[must_use]
pub fn has_local_paid_access() -> bool {
    let Ok(Some(value)) = read_background_credential() else {
        return false;
    };
    paid_access_at(&value, std::time::SystemTime::now())
}

#[cfg(target_os = "linux")]
fn read_background_credential() -> Result<Option<SecretString>, LicenseError> {
    secret_service_read::read()
}

#[cfg(not(target_os = "linux"))]
fn read_background_credential() -> Result<Option<SecretString>, LicenseError> {
    let entry = keyring::Entry::new(LOCAL_LICENSE_SERVICE, "desktop")
        .map_err(|_| LicenseError::StorageUnavailable)?;
    match entry.get_password() {
        Ok(value) => Ok(Some(SecretString::from(value))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(LicenseError::StorageUnavailable),
    }
}

fn paid_access_at(value: &SecretString, now: std::time::SystemTime) -> bool {
    let Ok(saved) = StoredLicense::decode(value) else {
        return false;
    };
    let Ok(now) = now.duration_since(std::time::UNIX_EPOCH) else {
        return false;
    };
    saved.record.access_at(now.as_secs()).status == crate::AccessStatus::Licensed
}

/// Forget this user's local paid key, seat and offline lease during true uninstall.
///
/// Clock history remains; uninstall does not renew paid access.
/// An absent entry is left absent. This makes no remote request and does not revoke
/// the purchase. Call as the installed interactive user, never as an administrator
/// acting on a different user's credential store. Updates must not invoke it.
///
/// # Errors
/// Returns `StorageUnavailable` if the native store cannot be read or updated, or
/// its existing entry cannot safely preserve the original clock history.
pub fn clear_local_license() -> Result<(), LicenseError> {
    let entry = keyring::Entry::new(LOCAL_LICENSE_SERVICE, "desktop")
        .map_err(|_| LicenseError::StorageUnavailable)?;
    clear_entry(&entry)
}

fn clear_entry(entry: &keyring::Entry) -> Result<(), LicenseError> {
    let value = match entry.get_password() {
        Ok(value) => SecretString::from(value),
        Err(keyring::Error::NoEntry) => return Ok(()),
        Err(_) => return Err(LicenseError::StorageUnavailable),
    };
    let mut saved = StoredLicense::decode(&value)?;
    saved.record.clear_paid();
    saved.key = None;
    entry
        .set_password(saved.encode()?.expose_secret())
        .map_err(|_| LicenseError::StorageUnavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Activation;

    fn entry() -> keyring::Entry {
        keyring::Entry::new_with_credential(Box::<keyring::mock::MockCredential>::default())
    }

    #[test]
    fn uninstall_forgets_paid_authentication_without_restarting_the_trial() {
        let entry = entry();
        let mut record = EntitlementRecord::new(1_000);
        let trial_expiry = record.observe(1_000).expires_at_unix;
        let activation: Activation =
            serde_json::from_str(r#"{"licenseId":7,"instanceId":"seat-1","purchasedMajor":0}"#)
                .expect("fixture activation");
        record
            .record_verified(activation, 2_000)
            .expect("paid seat");
        let saved = StoredLicense {
            record,
            key: Some(SecretString::from("fixture-paid-key")),
        };
        entry
            .set_password(saved.encode().expect("saved").expose_secret())
            .expect("mock save");

        clear_entry(&entry).expect("uninstall cleanup");
        clear_entry(&entry).expect("idempotent cleanup");

        let value = SecretString::from(entry.get_password().expect("trial history"));
        let mut saved = StoredLicense::decode(&value).expect("same format after cleanup");
        assert!(saved.key.is_none() && saved.record.activation().is_none());
        assert_eq!(saved.record.observe(2_001).expires_at_unix, trial_expiry);
        assert!(!value.expose_secret().contains("fixture-paid-key"));
        assert!(!value.expose_secret().contains("seat-1"));
    }

    #[test]
    fn absent_credentials_are_not_replaced_with_a_new_trial() {
        let entry = entry();
        clear_entry(&entry).expect("absent credential");
        assert!(matches!(entry.get_password(), Err(keyring::Error::NoEntry)));
    }

    #[test]
    fn malformed_credentials_are_not_overwritten_with_a_new_trial() {
        let entry = entry();
        entry.set_password("invalid-record").expect("mock entry");
        assert_eq!(clear_entry(&entry), Err(LicenseError::StorageUnavailable));
        assert_eq!(entry.get_password().expect("unchanged"), "invalid-record");
    }
}
