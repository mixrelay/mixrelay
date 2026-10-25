//! Noninteractive Linux admission reads the existing Secret Service entry.
use std::collections::HashMap;

use dbus_secret_service::{EncryptionType, Error, Item, SecretService};

use super::{LOCAL_LICENSE_SERVICE, background_read::unique_unlocked};
use crate::{LicenseError, SecretString};

fn unavailable(_: Error) -> LicenseError {
    LicenseError::StorageUnavailable
}

fn attributes() -> HashMap<&'static str, &'static str> {
    HashMap::from([("service", LOCAL_LICENSE_SERVICE), ("username", "desktop")])
}

fn read_item(item: &Item<'_>) -> Result<Vec<u8>, LicenseError> {
    // GetSecret fails if the item locks after discovery; it never unlocks it.
    item.get_secret().map_err(unavailable)
}

pub(super) fn read() -> Result<Option<SecretString>, LicenseError> {
    // Zero prevents the dependency's Prompt call itself, not merely its wait.
    // Its ordinary D-Bus method calls retain their separate two-second budgets.
    let service = SecretService::connect_with_max_prompt_timeout(EncryptionType::Dh, 0)
        .map_err(unavailable)?;
    let mut modern = attributes();
    modern.insert("target", "default");
    let matches = service.search_items(modern).map_err(unavailable)?;
    if !matches.unlocked.is_empty() || !matches.locked.is_empty() {
        return unique_unlocked(&matches.unlocked, matches.locked.len(), read_item);
    }
    // keyring v1 entries had no target and lived in the default collection.
    // ReadAlias and SearchItems do not create or unlock a collection.
    let collection = match service.get_default_collection() {
        Ok(collection) => collection,
        Err(Error::NoResult) => return Ok(None),
        Err(error) => return Err(unavailable(error)),
    };
    let legacy = collection.search_items(attributes()).map_err(unavailable)?;
    match legacy.as_slice() {
        [] => Ok(None),
        [item] if !item.is_locked().map_err(unavailable)? => unique_unlocked(&legacy, 0, read_item),
        _ => Err(LicenseError::StorageUnavailable),
    }
}
