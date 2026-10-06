//! Graph-owned application source attachments behind the dispatch/XPC ABI adapter.
mod dispatch;
mod source;
#[cfg(test)]
mod tests;
mod xpc;
use dispatch::Dispatch;
pub use mixrelay_macos_transport::source_registry::SourceClock;
use source::SourceRegistry;
use std::sync::OnceLock;

static SERVICE: OnceLock<Service> = OnceLock::new();
static START_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
struct Service {
    sources: SourceRegistry,
    peers: parking_lot::Mutex<xpc::Peers>,
    dispatch: parking_lot::Mutex<Option<Dispatch>>,
}

/// Starts graph-owned application-source XPC. Driver inventory and basic loopback
/// are owned by the independent Driver service and are never created here.
#[unsafe(no_mangle)]
pub extern "C" fn mr_engine_service_start() -> i32 {
    let _start = START_LOCK.lock();
    if SERVICE.get().is_none()
        && SERVICE
            .set(Service {
                sources: SourceRegistry::new(),
                peers: parking_lot::Mutex::new(xpc::Peers::default()),
                dispatch: parking_lot::Mutex::new(None),
            })
            .is_err()
    {
        return 2;
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

/// Observes one graph-owned application's current native clock.
/// Returns false when the listener or identified source is absent.
/// # Safety
/// Output points to writable source clock storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_engine_service_source_clock(
    token: u64,
    output: *mut SourceClock,
) -> bool {
    SERVICE.get().is_some_and(|service| unsafe {
        source::mr_engine_source_registry_clock(
            std::ptr::from_ref(&service.sources).cast_mut(),
            token,
            output,
        )
    })
}
/// Selects one source as the graph clock authority using a bounded atomic update.
/// Returns false when the identified source is absent.
#[unsafe(no_mangle)]
pub extern "C" fn mr_engine_service_set_source_clock_master(token: u64, master: bool) -> bool {
    SERVICE.get().is_some_and(|service| unsafe {
        source::mr_engine_source_registry_set_clock_master(
            std::ptr::from_ref(&service.sources).cast_mut(),
            token,
            master,
        )
    })
}
/// Reads clocked Float32 PCM from the one graph-owned source consumer.
/// Returns the number of valid frames, or zero when source PCM is unavailable.
/// # Safety
/// Samples has frames*channels writable floats.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_engine_service_read_source_clocked(
    token: u64,
    samples: *mut f32,
    frames: u32,
    channels: u32,
    rate: u32,
) -> u32 {
    SERVICE.get().map_or(0, |service| unsafe {
        source::mr_engine_source_registry_read_clocked(
            std::ptr::from_ref(&service.sources).cast_mut(),
            token,
            samples,
            frames,
            channels,
            rate,
        )
    })
}
