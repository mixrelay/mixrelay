use serde::Deserialize;

use crate::{Activation, LicenseError, ProductIdentity};

// Deliberately omit echoed keys, customer names/emails and provider error text.
#[derive(Deserialize)]
pub(super) struct Response {
    pub activated: Option<bool>,
    pub valid: Option<bool>,
    pub deactivated: Option<bool>,
    pub license_key: Option<License>,
    pub instance: Option<Instance>,
    pub meta: Option<Metadata>,
}

#[derive(Deserialize)]
pub(super) struct License {
    pub id: u64,
    pub status: String,
}

#[derive(Deserialize)]
pub(super) struct Instance {
    pub id: String,
}

#[derive(Deserialize)]
pub(super) struct Metadata {
    #[serde(rename = "store_id")]
    store: u64,
    #[serde(rename = "product_id")]
    product: u64,
    #[serde(rename = "variant_id")]
    variant: u64,
}

impl Response {
    pub(super) fn check_product(&self, expected: ProductIdentity) -> Result<(), LicenseError> {
        let meta = self.meta.as_ref().ok_or(LicenseError::InvalidResponse)?;
        if (meta.store, meta.product, meta.variant)
            != (expected.store, expected.product, expected.variant)
        {
            return Err(LicenseError::WrongProduct);
        }
        Ok(())
    }

    pub(super) fn license_id(&self, statuses: &[&str]) -> Result<u64, LicenseError> {
        let license = self
            .license_key
            .as_ref()
            .filter(|license| license.id != 0)
            .ok_or(LicenseError::InvalidResponse)?;
        if !statuses.contains(&license.status.as_str()) {
            return Err(LicenseError::Rejected);
        }
        Ok(license.id)
    }

    pub(super) fn activation(&self) -> Result<Activation, LicenseError> {
        let license_id = self.license_id(&["active"])?;
        let instance = self
            .instance
            .as_ref()
            .ok_or(LicenseError::InvalidResponse)?;
        super::client::check_argument(&instance.id).map_err(|_| LicenseError::InvalidResponse)?;
        Ok(Activation {
            license_id,
            instance_id: instance.id.clone(),
        })
    }
}
