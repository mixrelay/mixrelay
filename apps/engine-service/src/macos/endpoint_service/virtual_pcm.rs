//! Authenticated HAL grants and independently paced virtual-device source owners.
mod converter;
mod pump;
mod scheduling;
use mixrelay_macos_transport::{
    application_source_transport::SourceTransport, client_pcm::ClientPcmReader,
    endpoint_identity::format_endpoint_id, source_control::SourceRequest,
    source_registry::SourceRegistry,
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

#[derive(Debug, thiserror::Error)]
pub(super) enum Error {
    #[error("virtual PCM scheduling failed: {0}")]
    Scheduling(std::io::Error),
    #[error("{0}")]
    Snapshot(#[from] mixrelay_macos_transport::client_pcm::SnapshotError),
    #[error("virtual PCM snapshot has inconsistent endpoint geometry")]
    Geometry,
    #[error("virtual PCM source preparation failed: {0}")]
    Prepare(#[from] std::io::Error),
    #[error("virtual PCM conversion failed: {0}")]
    Bridge(#[from] mixrelay_audio_io::clock_bridge::BridgeError),
    #[error("virtual PCM producer grant is stale")]
    Stale,
}

struct Endpoint {
    channels: u32,
    capacity: u32,
    update: Arc<parking_lot::Mutex<Option<Vec<ClientPcmReader>>>>,
    updated: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Endpoint {
    fn start(sources: &SourceRegistry, queues: Vec<ClientPcmReader>) -> Result<Self, Error> {
        let first = queues.first().ok_or(Error::Geometry)?;
        let channels = first.capture_channels();
        let capacity = first.capacity();
        let source_id = format!("virtual:{}", format_endpoint_id(first.endpoint()));
        let mut request = SourceRequest {
            channels,
            capacity_frames: capacity,
            transport_version: mixrelay_macos_transport::application_clock::VERSION,
            ..SourceRequest::default()
        };
        for (target, byte) in request.source_id.iter_mut().zip(source_id.bytes()) {
            *target = std::ffi::c_char::from_ne_bytes([byte]);
        }
        let transport = SourceTransport::create(sources, &request)?;
        let update = Arc::new(parking_lot::Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let updated = Arc::new(AtomicBool::new(false));
        let context = pump::Pump::prepare(
            queues,
            transport,
            update.clone(),
            stop.clone(),
            updated.clone(),
        )?;
        let worker = thread::Builder::new().name("mixrelay-virtual-input".into()).spawn(move || {
            if let Err(error) = context.run() {
                tracing::error!(boundary = "hal_shared_pcm", stage = "source_pump", %error, "virtual input stopped");
            }
        })?;
        Ok(Self {
            channels,
            capacity,
            update,
            updated,
            stop,
            worker: Some(worker),
        })
    }
}
impl Drop for Endpoint {
    fn drop(&mut self) {
        // Release asks the sole pump to end all reads/writes before control joins and reclaims it.
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = crate::worker_join::join(worker);
        }
    }
}

pub(super) struct VirtualPcm {
    pub(super) sources: SourceRegistry,
    endpoints: parking_lot::Mutex<BTreeMap<[u8; 16], Endpoint>>,
}
impl VirtualPcm {
    pub(super) fn new() -> Self {
        Self {
            sources: SourceRegistry::new(),
            endpoints: parking_lot::Mutex::new(BTreeMap::new()),
        }
    }
    pub(super) fn replace(&self, queues: Vec<ClientPcmReader>) -> Result<(), Error> {
        let mut groups: BTreeMap<_, Vec<_>> = BTreeMap::new();
        for queue in queues {
            if queue.capture_channels() != 0 {
                groups.entry(queue.endpoint()).or_default().push(queue);
            }
        }
        // Validate the complete decoded list before changing any active source owner.
        if groups.len() > 64
            || groups.values().any(|queues| {
                let first = &queues[0];
                queues.len() > 256
                    || queues.iter().any(|queue| {
                        queue.capture_channels() != first.capture_channels()
                            || queue.capacity() != first.capacity()
                            || queue.channels() != first.channels()
                    })
            })
        {
            return Err(Error::Geometry);
        }
        let mut endpoints = self.endpoints.lock();
        endpoints.retain(|id, endpoint| {
            groups.get(id).is_some_and(|queues| {
                endpoint.channels == queues[0].capture_channels()
                    && endpoint.capacity == queues[0].capacity()
            })
        });
        for (id, queues) in groups {
            if let Some(endpoint) = endpoints.get(&id) {
                *endpoint.update.lock() = Some(queues);
                // Release publishes the prepared replacement before the pump takes it on control.
                endpoint.updated.store(true, Ordering::Release);
            } else {
                endpoints.insert(id, Endpoint::start(&self.sources, queues)?);
            }
        }
        Ok(())
    }
    pub(super) fn clear(&self) {
        self.endpoints.lock().clear();
    }
}
