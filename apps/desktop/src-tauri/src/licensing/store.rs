//! Persist one entitlement and its key together in the OS credential store.

use std::sync::{Arc, Mutex};

use mixrelay_licensing::{
    AccessSnapshot, Activation, EntitlementRecord, LicenseError, SecretString,
    StoredLicense as Vault,
};
use secrecy::ExposeSecret as _;

use super::recovery::{self, RemoteChange};

pub(super) trait CredentialStore: Send + Sync {
    fn read(&self) -> Result<Option<SecretString>, LicenseError>;
    fn write(&self, value: &SecretString) -> Result<(), LicenseError>;
}

struct NativeCredentials;

impl CredentialStore for NativeCredentials {
    fn read(&self) -> Result<Option<SecretString>, LicenseError> {
        mixrelay_licensing::read_local_license()
    }

    fn write(&self, value: &SecretString) -> Result<(), LicenseError> {
        mixrelay_licensing::write_local_license(value)
    }
}

/// The key and seat recovered from a stored record that no longer decodes. The seat is
/// restored only after the provider validates it again; it never grants access by itself.
#[derive(Clone)]
pub(super) struct SalvagedSeat {
    pub(super) key: SecretString,
    pub(super) license_id: String,
    pub(super) instance_id: String,
}

impl SalvagedSeat {
    /// Reads only the key and seat identity, so a format change or a damaged unrelated
    /// field cannot cost a paid user their activation.
    fn from_record(value: &SecretString) -> Option<Self> {
        let record: serde_json::Value = serde_json::from_str(value.expose_secret()).ok()?;
        let key = record.get("key")?.as_str()?.trim();
        let activation = record.get("record")?.get("activation")?;
        let license_id = activation.get("licenseId")?.as_str()?;
        let instance_id = activation.get("instanceId")?.as_str()?.trim();
        (!key.is_empty() && !license_id.is_empty() && !instance_id.is_empty()).then(|| Self {
            key: SecretString::from(key.to_owned()),
            license_id: license_id.to_owned(),
            instance_id: instance_id.to_owned(),
        })
    }
}

/// One writer across native calls and remote mutations; keys never cross IPC back to the UI.
pub(crate) struct LicensingState {
    vault: Mutex<Option<Vault>>,
    salvaged: Mutex<Option<SalvagedSeat>>,
    store: Arc<dyn CredentialStore>,
    pub(super) mutations: tokio::sync::Mutex<()>,
    journal: std::sync::OnceLock<std::path::PathBuf>,
    clock: Arc<dyn Fn() -> Result<u64, LicenseError> + Send + Sync>,
    pub(super) first_connection: std::sync::atomic::AtomicBool,
}

impl std::fmt::Debug for LicensingState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LicensingState")
            .finish_non_exhaustive()
    }
}

impl Default for LicensingState {
    fn default() -> Self {
        Self::with_store(Arc::new(NativeCredentials))
    }
}

impl LicensingState {
    fn with_store(store: Arc<dyn CredentialStore>) -> Self {
        Self::with_clock(store, Arc::new(super::now))
    }

    pub(super) fn with_clock(
        store: Arc<dyn CredentialStore>,
        clock: Arc<dyn Fn() -> Result<u64, LicenseError> + Send + Sync>,
    ) -> Self {
        Self {
            vault: Mutex::new(None),
            salvaged: Mutex::new(None),
            store,
            mutations: tokio::sync::Mutex::new(()),
            journal: std::sync::OnceLock::new(),
            clock,
            first_connection: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub(super) fn now(&self) -> Result<u64, LicenseError> {
        (self.clock)()
    }

    pub(super) fn begin_automatic_check(&self) -> Result<bool, LicenseError> {
        let now = self.now()?;
        self.update(now, |vault| vault.record.begin_automatic_check(now))
    }

    pub(super) fn record_attempt(&self) -> Result<(), LicenseError> {
        let now = self.now()?;
        self.update(now, |vault| vault.record.record_attempt(now))
    }

    pub(super) fn begin_activation(&self, key: &SecretString) -> Result<(), LicenseError> {
        let path = self.journal.get().ok_or(LicenseError::StorageUnavailable)?;
        recovery::write(
            path,
            &RemoteChange::ActivationPending {
                key_fingerprint: recovery::key_fingerprint(key.expose_secret()),
            },
        )
    }

    pub(super) fn discard_pending_activation(&self) -> Result<(), LicenseError> {
        if let Some(path) = self.journal.get() {
            recovery::remove(path)?;
        }
        Ok(())
    }

    pub(crate) fn set_journal_path(&self, path: std::path::PathBuf) {
        let _ = self.journal.set(path);
    }

    pub(super) fn recoverable_activation(
        &self,
        key: &SecretString,
    ) -> Result<Option<Activation>, LicenseError> {
        let Some(path) = self.journal.get() else {
            return Ok(None);
        };
        Ok(match recovery::read(path)? {
            Some(RemoteChange::Activated {
                activation,
                key_fingerprint,
            }) => {
                if key_fingerprint != recovery::key_fingerprint(key.expose_secret()) {
                    return Err(LicenseError::InvalidInput);
                }
                Some(activation)
            }
            Some(RemoteChange::ActivationPending { .. }) => {
                return Err(LicenseError::ActivationUncertain);
            }
            Some(RemoteChange::Cleared) | None => None,
        })
    }

    pub(super) fn preflight_remote_change(&self) -> Result<(), LicenseError> {
        self.journal
            .get()
            .map_or(Ok(()), |path| recovery::preflight(path))
    }

    fn reconcile_in_memory<T>(
        &self,
        change: impl FnOnce(&mut Vault) -> Result<T, LicenseError>,
    ) -> Result<T, LicenseError> {
        let mut held = self
            .vault
            .lock()
            .map_err(|_| LicenseError::StorageUnavailable)?;
        let vault = held.as_mut().ok_or(LicenseError::StorageUnavailable)?;
        let result = change(vault)?;
        // A failed receipt write must not prevent an independent credential write.
        // The caller still reports the receipt error; neither failure is hidden.
        self.store.write(&vault.encode()?)?;
        Ok(result)
    }

    fn update<T>(
        &self,
        now: u64,
        change: impl FnOnce(&mut Vault) -> Result<T, LicenseError>,
    ) -> Result<T, LicenseError> {
        let mut held = self
            .vault
            .lock()
            .map_err(|_| LicenseError::StorageUnavailable)?;
        if held.is_none() {
            let fresh = || Vault {
                record: EntitlementRecord::new(now),
                key: None,
            };
            *held = Some(match self.store.read()? {
                // An unreadable record must not block licensing for good. Free access has no
                // time limit, so a fresh record grants nothing; a seat it held is restored by
                // activating again online. A store that cannot be read stays an error above.
                Some(value) => Vault::decode(&value).unwrap_or_else(|_| {
                    let salvaged = SalvagedSeat::from_record(&value);
                    tracing::warn!(
                        seat_recovered = salvaged.is_some(),
                        "replacing an unreadable stored license; a recovered seat is validated online"
                    );
                    *self
                        .salvaged
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = salvaged;
                    fresh()
                }),
                None => fresh(),
            });
        }
        let vault = held.as_mut().ok_or(LicenseError::StorageUnavailable)?;
        let pending = self
            .journal
            .get()
            .map(|path| recovery::read(path))
            .transpose()?
            .flatten();
        if matches!(pending, Some(RemoteChange::Cleared)) {
            vault.record.clear_paid();
            vault.key = None;
        }
        let result = change(vault)?;
        // While a recovered seat awaits validation, the unreadable record still holds its
        // key, so it is not overwritten: a restart recovers the seat again.
        if self.salvaged_seat().is_some() {
            return Ok(result);
        }
        // Keep a remotely committed seat in memory if the save fails. Retrying
        // persistence or validating that seat must not consume another activation.
        self.store.write(&vault.encode()?)?;
        if let Some(path) = self.journal.get()
            && (matches!(pending, Some(RemoteChange::Cleared))
                || matches!(pending, Some(RemoteChange::Activated { ref activation, .. }) if vault.record.activation() == Some(activation)))
        {
            recovery::remove(path)?;
        }
        Ok(result)
    }

    pub(crate) fn snapshot(&self, now: u64) -> Result<AccessSnapshot, LicenseError> {
        self.update(now, |vault| Ok(vault.record.observe(now)))
    }

    /// A seat recovered from an unreadable record, waiting for online validation.
    pub(super) fn salvaged_seat(&self) -> Option<SalvagedSeat> {
        self.salvaged
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Ends recovery once the provider has answered conclusively.
    pub(super) fn forget_salvaged_seat(&self) {
        *self
            .salvaged
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    pub(super) fn saved_activation(
        &self,
    ) -> Result<Option<(SecretString, Activation)>, LicenseError> {
        let held = self
            .vault
            .lock()
            .map_err(|_| LicenseError::StorageUnavailable)?;
        let Some(vault) = held.as_ref() else {
            return Err(LicenseError::StorageUnavailable);
        };
        Ok(vault.record.activation().and_then(|activation| {
            vault
                .key
                .as_ref()
                .map(|key| (key.clone(), activation.clone()))
        }))
    }

    pub(super) fn verified(
        &self,
        key: SecretString,
        activation: Activation,
        now: u64,
    ) -> Result<AccessSnapshot, LicenseError> {
        if let Some(path) = self.journal.get()
            && let Err(error) = recovery::write(
                path,
                &RemoteChange::Activated {
                    activation: activation.clone(),
                    key_fingerprint: recovery::key_fingerprint(key.expose_secret()),
                },
            )
        {
            self.reconcile_in_memory(|vault| {
                vault.record.record_verified(activation, now)?;
                vault.key = Some(key);
                Ok(())
            })?;
            return Err(error);
        }
        self.update(now, |vault| {
            let snapshot = vault.record.record_verified(activation, now)?;
            vault.key = Some(key);
            Ok(snapshot)
        })
    }

    pub(super) fn clear_paid(&self, now: u64) -> Result<AccessSnapshot, LicenseError> {
        if let Some(path) = self.journal.get()
            && let Err(error) = recovery::write(path, &RemoteChange::Cleared)
        {
            self.reconcile_in_memory(|vault| {
                vault.record.clear_paid();
                vault.key = None;
                Ok(())
            })?;
            return Err(error);
        }
        self.update(now, |vault| {
            vault.record.clear_paid();
            vault.key = None;
            Ok(vault.record.observe(now))
        })
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
