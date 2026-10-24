//! Remote decisions update entitlement only after a conclusive provider response.

use super::LicensingState;
use mixrelay_licensing::{AccessSnapshot, LicenseClient, LicenseError, SecretString};

pub(super) fn public_error(error: LicenseError) -> LicenseError {
    match error {
        LicenseError::Revoked | LicenseError::ActivationDeactivated => LicenseError::Rejected,
        _ => error,
    }
}

pub(super) async fn activate_new(
    state: &LicensingState,
    client: &LicenseClient,
    key: &SecretString,
    name: &str,
) -> Result<mixrelay_licensing::Activation, LicenseError> {
    if state.recoverable_activation(key)?.is_some() {
        return Err(LicenseError::InvalidInput);
    }
    // The receipt is durable before the only request that can create a new activation.
    let result = client
        .activate(key, name, || state.begin_activation(key))
        .await;
    if matches!(
        result,
        Err(LicenseError::Rejected | LicenseError::RateLimited)
    ) {
        state.discard_pending_activation()?;
    }
    result
}

pub(super) async fn validate_saved(
    state: &LicensingState,
    client: &LicenseClient,
) -> Result<AccessSnapshot, LicenseError> {
    let Some((key, seat)) = state.saved_activation()? else {
        return restore_salvaged(state, client).await;
    };
    match validate_existing(client, &key, &seat).await {
        Ok(activation) => state.verified(key, activation, state.now()?),
        Err(LicenseError::Revoked | LicenseError::ActivationDeactivated) => {
            state.clear_paid(state.now()?)
        }
        Err(error) => Err(error),
    }
}

/// Restores a seat recovered from an unreadable stored record by validating it again. A
/// transient failure keeps it for the next check; a conclusive answer ends recovery.
async fn restore_salvaged(
    state: &LicensingState,
    client: &LicenseClient,
) -> Result<AccessSnapshot, LicenseError> {
    let seat = state.salvaged_seat().ok_or(LicenseError::InvalidInput)?;
    match client.validate(&seat.key, &seat.instance_id).await {
        Ok(activation) if activation.license_id() == seat.license_id => {
            // Saving the validated seat replaces the unreadable record. If that save fails,
            // the old record remains and the next start recovers the seat again.
            state.forget_salvaged_seat();
            state.verified(seat.key, activation, state.now()?)
        }
        Ok(_) => {
            state.forget_salvaged_seat();
            Err(LicenseError::InvalidResponse)
        }
        Err(error @ (LicenseError::Revoked | LicenseError::ActivationDeactivated)) => {
            state.forget_salvaged_seat();
            Err(error)
        }
        Err(error) => Err(error),
    }
}

pub(super) async fn validate_existing(
    client: &LicenseClient,
    key: &SecretString,
    seat: &mixrelay_licensing::Activation,
) -> Result<mixrelay_licensing::Activation, LicenseError> {
    let activation = client.validate(key, seat.instance_id()).await?;
    if activation.license_id() != seat.license_id()
        || activation.purchased_major() != seat.purchased_major()
    {
        return Err(LicenseError::InvalidResponse);
    }
    Ok(activation)
}

pub(super) async fn deactivate_saved(
    state: &LicensingState,
    client: &LicenseClient,
) -> Result<AccessSnapshot, LicenseError> {
    let (key, seat) = state
        .saved_activation()?
        .ok_or(LicenseError::InvalidInput)?;
    client.deactivate(&key, seat.instance_id()).await?;
    state.clear_paid(state.now()?)
}
