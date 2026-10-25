//! Select an existing, unique unlocked credential without an unlock capability.
use crate::{LicenseError, SecretString};

pub(super) fn unique_unlocked<T>(
    unlocked: &[T],
    locked_count: usize,
    read: impl FnOnce(&T) -> Result<Vec<u8>, LicenseError>,
) -> Result<Option<SecretString>, LicenseError> {
    match (unlocked, locked_count) {
        ([], 0) => Ok(None),
        ([item], 0) => {
            let bytes = read(item)?;
            let value = String::from_utf8(bytes).map_err(|_| LicenseError::StorageUnavailable)?;
            Ok(Some(SecretString::from(value)))
        }
        _ => Err(LicenseError::StorageUnavailable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret as _;

    #[test]
    fn locked_absent_and_ambiguous_entries_never_request_a_secret() {
        for (unlocked, locked_count, absent) in [
            (vec![], 0, true),
            (vec![], 1, false),
            (vec![1], 1, false),
            (vec![1, 2], 0, false),
        ] {
            let result = unique_unlocked(&unlocked, locked_count, |_| {
                panic!("secret read must not start")
            });
            if absent {
                assert!(matches!(result, Ok(None)));
            } else {
                assert!(matches!(result, Err(LicenseError::StorageUnavailable)));
            }
        }
    }

    #[test]
    fn one_unlocked_entry_preserves_the_stored_value_and_read_errors() {
        let value = unique_unlocked(&[7], 0, |item| {
            assert_eq!(*item, 7);
            Ok(b"existing-entitlement".to_vec())
        })
        .unwrap()
        .unwrap();
        assert_eq!(value.expose_secret(), "existing-entitlement");
        assert!(matches!(
            unique_unlocked(&[7], 0, |_| Err(LicenseError::StorageUnavailable)),
            Err(LicenseError::StorageUnavailable)
        ));
        assert!(matches!(
            unique_unlocked(&[7], 0, |_| Ok(vec![0xff])),
            Err(LicenseError::StorageUnavailable)
        ));
    }
}
