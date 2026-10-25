//! Select an existing, unique unlocked credential without an unlock capability.
use crate::{LicenseError, SecretString};

pub(super) fn with_unique_unlocked<T, R>(
    unlocked: &[T],
    locked_count: usize,
    apply: impl FnOnce(&T) -> Result<R, LicenseError>,
) -> Result<Option<R>, LicenseError> {
    match (unlocked, locked_count) {
        ([], 0) => Ok(None),
        ([item], 0) => apply(item).map(Some),
        _ => Err(LicenseError::StorageUnavailable),
    }
}

pub(super) fn decode_secret(bytes: Vec<u8>) -> Result<SecretString, LicenseError> {
    String::from_utf8(bytes)
        .map(SecretString::from)
        .map_err(|_| LicenseError::StorageUnavailable)
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
            let result: Result<Option<SecretString>, LicenseError> =
                with_unique_unlocked(&unlocked, locked_count, |_| {
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
        let value = with_unique_unlocked(&[7], 0, |item| {
            assert_eq!(*item, 7);
            decode_secret(b"existing-entitlement".to_vec())
        })
        .unwrap()
        .unwrap();
        assert_eq!(value.expose_secret(), "existing-entitlement");
        assert!(matches!(
            with_unique_unlocked::<_, SecretString>(&[7], 0, |_| Err(
                LicenseError::StorageUnavailable
            )),
            Err(LicenseError::StorageUnavailable)
        ));
        assert!(matches!(
            with_unique_unlocked(&[7], 0, |_| decode_secret(vec![0xff])),
            Err(LicenseError::StorageUnavailable)
        ));
    }
}
