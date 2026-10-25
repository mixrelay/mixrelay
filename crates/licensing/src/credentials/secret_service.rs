//! Noninteractive Linux storage shared by editor, engine admission and removal.
use std::collections::HashMap;

use dbus_secret_service::{EncryptionType, Error, Item, SecretService};
use secrecy::ExposeSecret as _;

use super::{
    LOCAL_LICENSE_SERVICE,
    background_read::{decode_secret, with_unique_unlocked},
};
use crate::{LicenseError, SecretString};

fn unavailable(_: Error) -> LicenseError {
    LicenseError::StorageUnavailable
}

fn attributes() -> HashMap<&'static str, &'static str> {
    HashMap::from([("service", LOCAL_LICENSE_SERVICE), ("username", "desktop")])
}

fn modern_attributes() -> HashMap<&'static str, &'static str> {
    let mut attributes = attributes();
    attributes.insert("target", "default");
    attributes
}

fn connect() -> Result<SecretService, LicenseError> {
    // Zero refuses Prompt itself. Ordinary D-Bus methods have separate 2s budgets.
    SecretService::connect_with_max_prompt_timeout(EncryptionType::Dh, 0).map_err(unavailable)
}

fn with_existing<R>(
    service: &SecretService,
    apply: impl FnOnce(&Item<'_>) -> Result<R, LicenseError>,
) -> Result<Option<R>, LicenseError> {
    let matches = service
        .search_items(modern_attributes())
        .map_err(unavailable)?;
    if !matches.unlocked.is_empty() || !matches.locked.is_empty() {
        return with_unique_unlocked(&matches.unlocked, matches.locked.len(), apply);
    }
    // Legacy keyring entries had no target and lived in the default collection.
    // ReadAlias and SearchItems neither create nor unlock a collection.
    let collection = match service.get_default_collection() {
        Ok(collection) => collection,
        Err(Error::NoResult) => return Ok(None),
        Err(error) => return Err(unavailable(error)),
    };
    let legacy = collection.search_items(attributes()).map_err(unavailable)?;
    with_unique_unlocked(&legacy, 0, |item| {
        if item.is_locked().map_err(unavailable)? {
            return Err(LicenseError::StorageUnavailable);
        }
        apply(item)
    })
}

pub(super) fn read() -> Result<Option<SecretString>, LicenseError> {
    let service = connect()?;
    with_existing(&service, |item| {
        // GetSecret fails on a lock race and does not unlock the item.
        decode_secret(item.get_secret().map_err(unavailable)?)
    })
}

pub(super) fn write(value: &SecretString) -> Result<(), LicenseError> {
    let service = connect()?;
    if with_existing(&service, |item| {
        item.set_secret(value.expose_secret().as_bytes(), "text/plain")
            .map_err(unavailable)
    })?
    .is_some()
    {
        return Ok(());
    }
    let collection = service.get_default_collection().map_err(unavailable)?;
    if collection.is_locked().map_err(unavailable)? {
        return Err(LicenseError::StorageUnavailable);
    }
    let mut attributes = modern_attributes();
    attributes.insert("application", "rust-keyring-client");
    collection
        .create_item(
            "MixRelay license",
            attributes,
            value.expose_secret().as_bytes(),
            true,
            "text/plain",
        )
        .map(|_| ())
        .map_err(unavailable)
}
