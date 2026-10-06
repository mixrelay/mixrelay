use mixrelay_licensing::{LicenseClient, LicenseError, ProductIdentity};

pub(super) fn configuration() -> Result<Option<(ProductIdentity, String)>, LicenseError> {
    parse_configuration([
        option_env!("MIXRELAY_LEMON_STORE_ID"),
        option_env!("MIXRELAY_LEMON_PRODUCT_ID"),
        option_env!("MIXRELAY_LEMON_VARIANT_ID"),
        option_env!("MIXRELAY_LEMON_CHECKOUT_URL"),
    ])
}

fn parse_configuration(
    values: [Option<&str>; 4],
) -> Result<Option<(ProductIdentity, String)>, LicenseError> {
    if values.iter().all(Option::is_none) {
        return Ok(None);
    }
    let [Some(store), Some(product), Some(variant), Some(checkout)] = values else {
        return Err(LicenseError::InvalidConfiguration);
    };
    let number = |value: &str| {
        value
            .parse()
            .map_err(|_| LicenseError::InvalidConfiguration)
    };
    let product = ProductIdentity::new(number(store)?, number(product)?, number(variant)?)?;
    let url = reqwest::Url::parse(checkout).map_err(|_| LicenseError::InvalidConfiguration)?;
    if url.scheme() != "https"
        || !url
            .host_str()
            .is_some_and(|host| host.ends_with(".lemonsqueezy.com"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || !url.path().starts_with("/buy/")
        || url.path().trim_start_matches("/buy/").is_empty()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(LicenseError::InvalidConfiguration);
    }
    Ok(Some((product, url.into())))
}

pub(super) fn client() -> Result<LicenseClient, LicenseError> {
    let (product, _) = configuration()?.ok_or(LicenseError::InvalidConfiguration)?;
    LicenseClient::new(product)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_is_absent_or_complete_not_a_placeholder() {
        assert_eq!(parse_configuration([None; 4]), Ok(None));
        let ids = [
            Some("1"),
            Some("2"),
            Some("3"),
            Some("https://mixrelay.lemonsqueezy.com/buy/product"),
        ];
        assert!(parse_configuration(ids).expect("configured").is_some());
        for index in 0..4 {
            let mut incomplete = ids;
            incomplete[index] = None;
            assert_eq!(
                parse_configuration(incomplete),
                Err(LicenseError::InvalidConfiguration)
            );
        }
        for number in ["0", "-1", "invalid"] {
            let mut invalid = ids;
            invalid[0] = Some(number);
            assert_eq!(
                parse_configuration(invalid),
                Err(LicenseError::InvalidConfiguration)
            );
        }
    }

    #[test]
    fn checkout_cannot_inject_credentials_or_redirect_to_untrusted_hosts() {
        for checkout in [
            "http://mixrelay.lemonsqueezy.com/buy/a",
            "https://lemonsqueezy.com.attacker.invalid/buy/a",
            "https://user:secret@mixrelay.lemonsqueezy.com/buy/a",
            "https://mixrelay.lemonsqueezy.com:9999/buy/a",
            "https://mixrelay.lemonsqueezy.com/buy/",
            "https://mixrelay.lemonsqueezy.com/buy/a?email=private",
            "https://mixrelay.lemonsqueezy.com/buy/a#secret",
            "https://mixrelay.lemonsqueezy.com/redirect/a",
        ] {
            assert_eq!(
                parse_configuration([Some("1"), Some("2"), Some("3"), Some(checkout)]),
                Err(LicenseError::InvalidConfiguration)
            );
        }
    }
}
