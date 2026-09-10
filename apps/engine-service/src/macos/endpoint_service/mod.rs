//! Endpoint and source ownership behind the dispatch/XPC ABI adapter.
mod api;
mod audio;
mod device;
mod dispatch;
mod runtime;
mod source;
#[cfg(test)]
mod tests;
mod xpc;
use crate::endpoint_registry::EndpointRegistry;
pub use api::{
    AudioConfig, EndpointSnapshot, activate_prepared_endpoints, activate_recovered_endpoints,
    audio_config, cancel_prepared_endpoints, capture_pending, device_sample_rate,
    device_supports_sample_rate, endpoint_specs, prepare_endpoints, read_render, rearm_capture,
    recover_capture_epoch, set_device_sample_rate, snapshot, touch_audio, write_capture,
};
use dispatch::Dispatch;
use runtime::Runtime;
use source::SourceRegistry;
use std::sync::{OnceLock, atomic::AtomicBool};
pub use tonenari_macos_transport::source_registry::SourceClock;
pub use xpc::driver_connected;

static REGISTRY: OnceLock<EndpointRegistry> = OnceLock::new();
static SERVICE: OnceLock<Service> = OnceLock::new();
static START_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
struct Service {
    runtime: Runtime<'static>,
    sources: SourceRegistry,
    peers: parking_lot::Mutex<xpc::Peers>,
    notify: AtomicBool,
    dispatch: parking_lot::Mutex<Option<Dispatch>>,
}

/// Starts XPC with an empty endpoint inventory. Graph references create devices;
/// startup performs no Core Audio lookup and never manufactures a default device.
#[unsafe(no_mangle)]
pub extern "C" fn tn_engine_service_start() -> i32 {
    let _start = START_LOCK.lock();
    if SERVICE.get().is_none() {
        let registry = REGISTRY.get_or_init(EndpointRegistry::new);
        if SERVICE
            .set(Service {
                runtime: Runtime::new(registry),
                sources: SourceRegistry::new(),
                peers: parking_lot::Mutex::new(xpc::Peers::default()),
                notify: AtomicBool::new(false),
                dispatch: parking_lot::Mutex::new(None),
            })
            .is_err()
        {
            return 2;
        }
    }
    let Some(service) = SERVICE.get() else {
        return 2;
    };
    let mut dispatch = service.dispatch.lock();
    if dispatch.is_some() {
        return 0;
    }
    let Ok(owner) = Dispatch::start() else {
        return 3;
    };
    *dispatch = Some(owner);
    0
}

/// # Safety
/// Output points to writable source clock storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tn_engine_service_source_clock(
    token: u64,
    output: *mut SourceClock,
) -> bool {
    SERVICE.get().is_some_and(|service| unsafe {
        source::tn_engine_source_registry_clock(
            std::ptr::from_ref(&service.sources).cast_mut(),
            token,
            output,
        )
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn tn_engine_service_set_source_clock_master(token: u64, master: bool) -> bool {
    SERVICE.get().is_some_and(|service| unsafe {
        source::tn_engine_source_registry_set_clock_master(
            std::ptr::from_ref(&service.sources).cast_mut(),
            token,
            master,
        )
    })
}
/// # Safety
/// Samples has frames*channels writable floats.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tn_engine_service_read_source_clocked(
    token: u64,
    samples: *mut f32,
    frames: u32,
    channels: u32,
    rate: u32,
) -> u32 {
    SERVICE.get().map_or(0, |service| unsafe {
        source::tn_engine_source_registry_read_clocked(
            std::ptr::from_ref(&service.sources).cast_mut(),
            token,
            samples,
            frames,
            channels,
            rate,
        )
    })
}
