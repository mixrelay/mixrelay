//! Decode provider decisions without exposing customer data or error text.

use serde::Deserialize;

use crate::{Activation, LicenseError, ProductIdentity};

// Omit echoed keys and customer data. Error text is used only for exact API classification.
#[derive(Deserialize)]
pub(super) struct Response {
    error: Option<String>,
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
    pub(super) fn check_validation(&self, product: ProductIdentity) -> Result<(), LicenseError> {
        self.check_validation_failure(self.valid, product)?;
        self.check_product(product)
    }

    pub(super) fn check_validation_failure(
        &self,
        decision: Option<bool>,
        product: ProductIdentity,
    ) -> Result<(), LicenseError> {
        if decision == Some(true) {
            return Ok(());
        }
        if decision != Some(false) {
            return Err(LicenseError::InvalidResponse);
        }
        // A foreign product response cannot revoke this product's saved entitlement.
        if self.meta.is_some() {
            self.check_product(product)?;
        }
        match self.error.as_deref() {
            Some(
                "license_key not found." | "license_key is disabled." | "license_key is expired.",
            ) => return Err(LicenseError::Revoked),
            Some("instance not found." | "instance_id not found.") => {
                return Err(LicenseError::ActivationDeactivated);
            }
            _ => {}
        }
        if self
            .license_key
            .as_ref()
            .is_some_and(|license| matches!(license.status.as_str(), "disabled" | "expired"))
            && self.meta.is_some()
        {
            return Err(LicenseError::Revoked);
        }
        if self
            .license_key
            .as_ref()
            .is_some_and(|license| license.status == "inactive")
            && self.meta.is_some()
        {
            return Err(LicenseError::ActivationDeactivated);
        }
        Err(LicenseError::Rejected)
    }

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
            return Err(
                if matches!(license.status.as_str(), "disabled" | "expired") {
                    LicenseError::Revoked
                } else {
                    LicenseError::Rejected
                },
            );
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
            purchased_major: env!("CARGO_PKG_VERSION_MAJOR")
                .parse()
                .map_err(|_| LicenseError::InvalidConfiguration)?,
            instance_id: instance.id.clone(),
        })
    }
}
