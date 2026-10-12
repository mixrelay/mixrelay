//! Numeric release versions shared by optional Driver install decisions.
use anyhow::{Result, ensure};

pub(crate) fn numeric_version(text: &str) -> Result<[u32; 4]> {
    let parts = text.split('.').collect::<Vec<_>>();
    ensure!(
        (2..=4).contains(&parts.len()),
        "release version must have two to four numeric components"
    );
    let mut result = [0; 4];
    for (index, part) in parts.iter().enumerate() {
        ensure!(
            !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()),
            "invalid release version"
        );
        result[index] = part.parse()?;
    }
    Ok(result)
}

/// Missing pieces are repaired unless an existing bundle is newer than the offered release.
/// # Errors
/// Rejects malformed release versions instead of guessing whether a downgrade is safe.
pub fn optional_driver_install_needed(
    offered: &str,
    installed: &[String],
    complete: bool,
) -> Result<bool> {
    let offered = numeric_version(offered)?;
    let versions = installed
        .iter()
        .map(|version| numeric_version(version))
        .collect::<Result<Vec<_>>>()?;
    if versions.iter().any(|version| *version > offered) {
        return Ok(false);
    }
    Ok(!complete || versions.is_empty() || versions.iter().any(|version| *version < offered))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absent_old_current_new_and_partial_installations() {
        assert!(optional_driver_install_needed("1.10.0", &[], false).unwrap());
        assert!(optional_driver_install_needed("1.10.0", &["1.9.9".into()], true).unwrap());
        assert!(!optional_driver_install_needed("1.10.0", &["1.10".into()], true).unwrap());
        assert!(!optional_driver_install_needed("1.10.0", &["2.0".into()], true).unwrap());
        assert!(optional_driver_install_needed("1.10.0", &["1.10.0".into()], false).unwrap());
        assert!(!optional_driver_install_needed("1.10.0", &["2.0".into()], false).unwrap());
        assert!(
            !optional_driver_install_needed("1.10.0", &["1.9".into(), "2.0".into()], false)
                .unwrap()
        );
    }
    #[test]
    fn malformed_versions_fail_closed() {
        for version in [
            "",
            "1",
            "1.2.3.4.5",
            "1.-2",
            "1.2-beta",
            "1.9999999999999999",
            "1.",
        ] {
            assert!(optional_driver_install_needed("1.10", &[version.into()], true).is_err());
        }
    }
}
