//! Persist one entitlement and its key together in the OS credential store.

use std::sync::{Arc, Mutex};

use mixrelay_licensing::{
    AccessSnapshot, Activation, EntitlementRecord, LOCAL_LICENSE_SERVICE, LicenseError,
    SecretString, StoredLicense as Vault,
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
        let entry = keyring::Entry::new(LOCAL_LICENSE_SERVICE, "desktop")
            .map_err(|_| LicenseError::StorageUnavailable)?;
        match entry.get_password() {
            Ok(value) => Ok(Some(SecretString::from(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(LicenseError::StorageUnavailable),
        }
    }

    fn write(&self, value: &SecretString) -> Result<(), LicenseError> {
        keyring::Entry::new(LOCAL_LICENSE_SERVICE, "desktop")
            .and_then(|entry| entry.set_password(value.expose_secret()))
            .map_err(|_| LicenseError::StorageUnavailable)
    }
}

/// One writer across native calls and remote mutations; keys never cross IPC back to the UI.
pub(crate) struct LicensingState {
    vault: Mutex<Option<Vault>>,
    store: Arc<dyn CredentialStore>,
    pub(super) mutations: tokio::sync::Mutex<()>,
    journal: std::sync::OnceLock<std::path::PathBuf>,
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
        Self {
            vault: Mutex::new(None),
            store,
            mutations: tokio::sync::Mutex::new(()),
            journal: std::sync::OnceLock::new(),
        }
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
            *held = Some(match self.store.read()? {
                Some(value) => Vault::decode(&value)?,
                None => Vault {
                    record: EntitlementRecord::new(now),
                    key: None,
                },
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

    pub(crate) fn allows_mutation(&self, now: u64) -> bool {
        self.vault.lock().is_ok_and(|held| {
            held.as_ref()
                .is_some_and(|vault| super::allows_audio(&vault.record.access_at(now)))
        })
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
