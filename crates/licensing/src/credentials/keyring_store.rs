//! Native keyring persistence for platforms outside the Linux Secret Service adapter.
use secrecy::ExposeSecret as _;

use super::LOCAL_LICENSE_SERVICE;
use crate::{LicenseError, SecretString};

fn entry() -> Result<keyring::Entry, LicenseError> {
    keyring::Entry::new(LOCAL_LICENSE_SERVICE, "desktop")
        .map_err(|_| LicenseError::StorageUnavailable)
}

pub(super) fn read() -> Result<Option<SecretString>, LicenseError> {
    match entry()?.get_password() {
        Ok(value) => Ok(Some(SecretString::from(value))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(LicenseError::StorageUnavailable),
    }
}

pub(super) fn write(value: &SecretString) -> Result<(), LicenseError> {
    entry()?
        .set_password(value.expose_secret())
        .map_err(|_| LicenseError::StorageUnavailable)
}
