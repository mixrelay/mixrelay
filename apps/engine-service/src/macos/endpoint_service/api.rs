//! Safe, identity-directed control and PCM boundaries. No default-device lookup exists.
use super::{SERVICE, audio, device};
use crate::endpoint_registry::{EndpointId, EndpointSpec};
use mixrelay_macos_transport::{
    endpoint_transport::{
        EndpointClockSnapshot, mr_endpoint_transport_requires_replacement,
        mr_endpoint_transport_touch_engine,
    },
    shared_audio_buffer::mr_shared_audio_buffer_epoch_pending,
};
use std::{io, sync::atomic::Ordering};

/// A mapping-coherent observation includes stable identity as well as incarnation/epochs.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct EndpointSnapshot {
    pub endpoint_id: EndpointId,
    pub clock: EndpointClockSnapshot,
    pub render_epoch: u64,
    pub capture_epoch: u64,
    pub published_through: u64,
    pub render_activity: u32,
    pub incarnation: [u8; 16],
    pub render_channels: u32,
    pub capture_channels: u32,
}

/// Absence permits another clock owner; an unready or contended observation does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SnapshotError {
    #[error("endpoint is not present")]
    Missing,
    #[error("endpoint clock observation is not coherent yet")]
    Pending,
}
/// Actual storage geometry for one endpoint, not a graph-global audio configuration.
#[derive(Debug, Default, Clone, Copy)]
pub struct AudioConfig {
    pub capacity_frames: u32,
    pub render_channels: u32,
    pub capture_channels: u32,
}

/// Stages exactly the complete graph-referenced inventory, including final removal via `[]`.
/// # Errors
/// Missing service, invalid specs, allocation failure or an outstanding transaction.
pub fn prepare_endpoints(specs: &[EndpointSpec]) -> io::Result<()> {
    SERVICE
        .get()
        .ok_or_else(|| io::Error::other("endpoint service not started"))?
        .runtime
        .prepare_inventory(specs)
}
/// Atomically publishes the prepared inventory at the sole DSP activation boundary.
#[must_use]
pub fn activate_prepared_endpoints() -> bool {
    SERVICE
        .get()
        .is_some_and(|service| service.runtime.activate())
}
/// Publishes only an exhausted native mapping replacement, never a pending graph edit.
/// Returns whether an inventory changed at this sole callback/drained-worker boundary.
#[must_use]
pub fn activate_recovered_endpoints() -> bool {
    SERVICE
        .get()
        .is_some_and(|service| service.runtime.activate_recovery())
}
/// Cancels only a not-yet-published inventory; active mapping readers are unaffected.
pub fn cancel_prepared_endpoints() {
    if let Some(service) = SERVICE.get() {
        service.runtime.cancel();
    }
}
/// Returns control-owned copies of all active specs; never call from a real-time callback.
#[must_use]
pub fn endpoint_specs() -> Vec<EndpointSpec> {
    SERVICE
        .get()
        .map_or_else(Vec::new, |service| service.runtime.read().specs())
}
/// Reads exact endpoint storage geometry; absence does not select another device.
#[must_use]
pub fn audio_config(id: EndpointId) -> Option<AudioConfig> {
    let read = SERVICE.get()?.runtime.read();
    let spec = read.endpoint(id)?.spec();
    Some(AudioConfig {
        capacity_frames: spec.capacity_frames,
        render_channels: spec.render_channels,
        capture_channels: spec.capture_channels,
    })
}
/// Observes a coherent native clock/mapping for the exact stable endpoint identity.
/// # Errors
/// Missing service/identity, or a pending clock transition/concurrent publication.
pub fn snapshot(id: EndpointId) -> Result<EndpointSnapshot, SnapshotError> {
    let service = SERVICE.get().ok_or(SnapshotError::Missing)?;
    let read = service.runtime.read();
    let endpoint = read.endpoint(id).ok_or(SnapshotError::Missing)?;
    audio::snapshot(endpoint).ok_or(SnapshotError::Pending)
}
/// Reads PCM only from the snapshot's identity and incarnation, silencing stale/missing data.
pub fn read_render(
    expected: &EndpointSnapshot,
    frame: u64,
    samples: &mut [f32],
    channels: u32,
) -> u32 {
    let Some(service) = SERVICE.get() else {
        samples.fill(0.0);
        return 0;
    };
    audio::read_for(
        service.runtime.read().endpoint(expected.endpoint_id),
        expected,
        frame,
        samples,
        channels,
    )
}
/// Publishes PCM only into the snapshot's still-current identity and incarnation.
#[must_use]
pub fn write_capture(
    expected: &EndpointSnapshot,
    frame: u64,
    samples: &[f32],
    channels: u32,
) -> bool {
    let Some(service) = SERVICE.get() else {
        return false;
    };
    let read = service.runtime.read();
    read.endpoint(expected.endpoint_id)
        .is_some_and(|endpoint| audio::write_for(endpoint, expected, frame, samples, channels))
}
/// Acknowledges capture rearm only for the exact snapshot, then returns its fresh generation.
#[must_use]
pub fn rearm_capture(expected: &EndpointSnapshot) -> Option<EndpointSnapshot> {
    let read = SERVICE.get()?.runtime.read();
    let endpoint = read.endpoint(expected.endpoint_id)?;
    audio::begin_for(endpoint, expected)
        .then(|| audio::snapshot(endpoint))
        .flatten()
}
/// Reports stale/missing mappings as pending, never implicitly switches native device.
#[must_use]
pub fn capture_pending(expected: &EndpointSnapshot) -> bool {
    let Some(service) = SERVICE.get() else {
        return true;
    };
    let read = service.runtime.read();
    read.endpoint(expected.endpoint_id).is_none_or(|endpoint| {
        // SAFETY: read pins these initialized shared mappings through this synchronous check.
        !audio::matches(endpoint, expected)
            || unsafe {
                mr_shared_audio_buffer_epoch_pending(endpoint.capture_buffer())
                    || mr_endpoint_transport_requires_replacement(endpoint.transport())
            }
    })
}
/// Control-only recovery after the sole writer drained. Exhausted storage is replaced by ID.
/// # Errors
/// Missing identity/service or a competing inventory transaction.
pub fn recover_capture_epoch(id: EndpointId) -> io::Result<bool> {
    let service = SERVICE
        .get()
        .ok_or_else(|| io::Error::other("endpoint service not started"))?;
    let changed = service.runtime.recover_capture(id)?;
    if changed {
        service.notify.store(true, Ordering::Release);
    }
    Ok(changed)
}
/// Keeps only currently published endpoint leases alive using bounded lock-free accesses.
pub fn touch_audio() {
    if let Some(service) = SERVICE.get() {
        let read = service.runtime.read();
        for endpoint in read.endpoints() {
            // SAFETY: each endpoint mapping is initialized and retained by this read guard.
            unsafe { mr_endpoint_transport_touch_engine(endpoint.transport()) };
        }
    }
}
/// Queries this device's actual Core Audio nominal rate on control; unknown stays absent.
#[must_use]
pub fn device_sample_rate(id: EndpointId) -> Option<u32> {
    device::rate(id)
}
/// Checks only the requested device's native rate ranges.
#[must_use]
pub fn device_supports_sample_rate(id: EndpointId, rate: u32) -> bool {
    device::supports(id, rate)
}
/// Requests a rate on the exact own virtual device; caller verifies actual readback.
#[must_use]
pub fn set_device_sample_rate(id: EndpointId, rate: u32) -> bool {
    device::set_rate(id, rate)
}
