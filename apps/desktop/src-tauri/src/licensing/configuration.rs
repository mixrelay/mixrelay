//! Public Polar configuration embedded by the checked desktop build.
use mixrelay_licensing::{LicenseClient, LicenseError, ProductIdentity};

pub(super) fn configuration() -> Result<Option<(ProductIdentity, String)>, LicenseError> {
    parse_configuration(env!("MIXRELAY_POLAR_CONFIGURATION"), cfg!(debug_assertions))
}

fn parse_configuration(
    marker: &str,
    development: bool,
) -> Result<Option<(ProductIdentity, String)>, LicenseError> {
    if marker.is_empty() {
        return Ok(None);
    }
    let invalid = LicenseError::InvalidConfiguration;
    let values: Vec<_> = marker
        .strip_prefix("MIXRELAY_POLAR_V1|")
        .and_then(|value| value.strip_suffix("|END_POLAR"))
        .ok_or(invalid)?
        .split('|')
        .collect();
    let [organization, benefit, checkout, environment] = values.as_slice() else {
        return Err(invalid);
    };
    let product = ProductIdentity::new(organization, benefit)?;
    let sandbox = match *environment {
        "production" => false,
        "sandbox" if development => true,
        _ => return Err(invalid),
    };
    let prefix = if sandbox {
        "https://sandbox-api.polar.sh/v1/checkout-links/"
    } else {
        "https://buy.polar.sh/"
    };
    let token = checkout.strip_prefix(prefix).ok_or(invalid)?;
    let token = if sandbox {
        token.strip_suffix("/redirect").ok_or(invalid)?
    } else {
        token
    };
    let token = token.strip_prefix("polar_cl_").ok_or(invalid)?;
    if token.is_empty()
        || !token
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || value == b'_' || value == b'-')
    {
        return Err(invalid);
    }
    Ok(Some((product, (*checkout).to_owned())))
}

pub(super) fn client() -> Result<LicenseClient, LicenseError> {
    let (product, checkout) = configuration()?.ok_or(LicenseError::InvalidConfiguration)?;
    if checkout.starts_with("https://sandbox-api.polar.sh/") {
        LicenseClient::sandbox(product)
    } else {
        LicenseClient::new(product)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ORG: &str = "10000000-0000-4000-8000-000000000001";
    const BENEFIT: &str = "20000000-0000-4000-8000-000000000002";
    fn marker(checkout: &str, environment: &str) -> String {
        format!("MIXRELAY_POLAR_V1|{ORG}|{BENEFIT}|{checkout}|{environment}|END_POLAR")
    }
    #[test]
    fn configuration_is_absent_or_complete_with_canonical_public_ids() {
        assert_eq!(parse_configuration("", true), Ok(None));
        let valid = marker("https://buy.polar.sh/polar_cl_fixture", "production");
        assert!(parse_configuration(&valid, false).unwrap().is_some());
        for invalid in [
            valid.replace(ORG, ""),
            valid.replace(BENEFIT, "1"),
            valid.replace("production", "test"),
            valid.replace("|END_POLAR", ""),
        ] {
            assert_eq!(
                parse_configuration(&invalid, true),
                Err(LicenseError::InvalidConfiguration)
            );
        }
    }
    #[test]
    fn checkout_rejects_credentials_ports_queries_fragments_and_host_spoofing() {
        for url in [
            "http://buy.polar.sh/polar_cl_a",
            "https://buy.polar.sh.evil.invalid/polar_cl_a",
            "https://user:secret@buy.polar.sh/polar_cl_a",
            "https://buy.polar.sh:443/polar_cl_a",
            "https://buy.polar.sh/polar_cl_",
            "https://buy.polar.sh/polar_cl_a?email=private",
            "https://buy.polar.sh/polar_cl_a#secret",
            "https://polar.sh/checkout/a",
            "https://buy.polar.sh/polar_cl_a/../b",
            "https://buy.polar.sh/polar_cl_%61",
        ] {
            assert_eq!(
                parse_configuration(&marker(url, "production"), true),
                Err(LicenseError::InvalidConfiguration)
            );
        }
    }
    #[test]
    fn sandbox_is_explicit_and_never_accepted_by_a_release() {
        let value = marker(
            "https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_fixture/redirect",
            "sandbox",
        );
        assert!(parse_configuration(&value, true).unwrap().is_some());
        assert_eq!(
            parse_configuration(&value, false),
            Err(LicenseError::InvalidConfiguration)
        );
        assert!(
            parse_configuration(&marker("https://buy.polar.sh/polar_cl_a", "sandbox"), true)
                .is_err()
        );
    }
}
