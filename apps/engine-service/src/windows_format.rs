//! Control-plane-only format staging. A reservation owns a pending publication
//! slot; abandoning negotiation drops just newly prepared streams, not live I/O.
use super::{
    WindowsCaptureSet, WindowsGraphResources, WindowsResourceSpec, application_capture, input,
    render,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicPtr, Ordering},
};
use tonenari_audio_graph::{GraphDocument, NodeConfig, RenderFormat};
use tonenari_audio_io::HostAudioBackendError;
use tonenari_engine_service::PreparedGraphFormat;

pub(super) use tonenari_audio_io::formats::SAMPLE_RATES as GRAPH_RATES;

fn failure(message: &str) -> HostAudioBackendError {
    HostAudioBackendError::BackendFailure {
        message: message.into(),
    }
}

#[derive(Default)]
pub(super) struct ResourceMailbox {
    pub(super) pending: AtomicPtr<WindowsCaptureSet>,
    pub(super) retired: AtomicPtr<WindowsCaptureSet>,
    reserved: AtomicBool,
}

impl ResourceMailbox {
    pub(super) fn reserve(self: &Arc<Self>) -> Result<Reservation, HostAudioBackendError> {
        self.reserved
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| failure("a Windows native format transaction is already reserved"))?;
        let reservation = Reservation(self.clone());
        if !self.pending.load(Ordering::Acquire).is_null() {
            return Err(failure(
                "a Windows native resource update is already pending",
            ));
        }
        Ok(reservation)
    }
}

impl Drop for ResourceMailbox {
    fn drop(&mut self) {
        for pointer in [*self.pending.get_mut(), *self.retired.get_mut()] {
            if !pointer.is_null() {
                // Last Arc owns both remaining slots; no publisher or consumer
                // can reach either allocation after this mailbox is destroyed.
                drop(unsafe { Box::from_raw(pointer) });
            }
        }
    }
}

pub(super) struct Reservation(Arc<ResourceMailbox>);
impl Drop for Reservation {
    fn drop(&mut self) {
        self.0.reserved.store(false, Ordering::Release);
    }
}

struct Unchanged;
impl PreparedGraphFormat for Unchanged {
    fn commit(self: Box<Self>) {}
}

struct Prepared {
    reservation: Reservation,
    captures: Box<WindowsCaptureSet>,
    spec: WindowsResourceSpec,
    active_spec: Arc<Mutex<WindowsResourceSpec>>,
    captures_cache: Arc<Mutex<application_capture::CaptureCache>>,
    inputs_cache: Arc<Mutex<input::InputCache>>,
    outputs_cache: Arc<Mutex<render::RenderCache>>,
    next_captures: application_capture::CaptureCache,
    next_inputs: input::InputCache,
    next_outputs: render::RenderCache,
}

impl PreparedGraphFormat for Prepared {
    fn commit(self: Box<Self>) {
        let Self {
            reservation,
            captures,
            spec,
            active_spec,
            captures_cache,
            inputs_cache,
            outputs_cache,
            next_captures,
            next_inputs,
            next_outputs,
        } = *self;
        // Everything fallible (including native stream creation and mailbox
        // reservation) finished before DSP pause. Poisoned control locks retain
        // their owned storage; committing this already validated state cannot fail.
        *active_spec
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = spec;
        *captures_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = next_captures;
        *inputs_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = next_inputs;
        *outputs_cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = next_outputs;
        debug_assert!(reservation.0.pending.load(Ordering::Acquire).is_null());
        reservation
            .0
            .pending
            .store(Box::into_raw(captures), Ordering::Release);
    }
}

pub(super) fn prepare(
    resources: &WindowsGraphResources,
    document: &GraphDocument,
    current: RenderFormat,
    requested: RenderFormat,
) -> Result<Box<dyn PreparedGraphFormat>, HostAudioBackendError> {
    if current == requested {
        return Ok(Box::new(Unchanged));
    }
    tonenari_audio_io::format_contract::validate_format(
        requested.sample_rate,
        requested.block_frames,
    )
    .map_err(|error| failure(&error.to_string()))?;
    if requested.block_frames != super::ENGINE_BLOCK_FRAMES {
        return Err(HostAudioBackendError::CapabilityUnavailable {
            capability: "requested Windows graph format".into(),
        });
    }
    if document
        .nodes
        .iter()
        .any(|node| matches!(node.config, NodeConfig::VirtualEndpointTransport(_)))
    {
        // The installed legacy LAB driver cannot confirm another format. Do not
        // report an engine-only rate change as a successful virtual-device change.
        return Err(HostAudioBackendError::CapabilityUnavailable {
            capability: "installed Windows virtual endpoint format negotiation".into(),
        });
    }
    let reservation = resources.mailbox.reserve()?;
    let mut spec = WindowsResourceSpec {
        graph_rate: requested.sample_rate,
        ..Default::default()
    };
    for node in &document.nodes {
        match &node.config {
            NodeConfig::ApplicationCapture(config) => {
                let mut config = config.clone();
                config.gain_db = 0.0;
                spec.captures.push((node.id.as_str().into(), config));
            }
            NodeConfig::AudioDeviceInput(config) => {
                let mut config = config.clone();
                config.gain_db = 0.0;
                spec.inputs.push((node.id.as_str().into(), config));
            }
            NodeConfig::AudioDeviceOutput(config) => {
                spec.outputs.push((node.id.as_str().into(), config.clone()));
            }
            _ => {}
        }
    }
    let mut next_captures = resources
        .capture_cache
        .lock()
        .map_err(|_| failure("capture cache poisoned"))?
        .clone();
    let mut next_inputs = resources
        .input_cache
        .lock()
        .map_err(|_| failure("input cache poisoned"))?
        .clone();
    let mut next_outputs = resources
        .render_cache
        .lock()
        .map_err(|_| failure("output cache poisoned"))?
        .clone();
    let captures = Box::new(WindowsCaptureSet {
        revision: document.revision,
        captures: application_capture::prepare_at_rate(
            &spec.captures,
            &mut next_captures,
            requested.sample_rate,
        )
        .map_err(|error| failure(&error))?,
        inputs: input::prepare_inputs_at_rate(
            &spec.inputs,
            &mut next_inputs,
            requested.sample_rate,
        )
        .map_err(|error| failure(&error))?,
        outputs: render::prepare_outputs_at_rate(
            &spec.outputs,
            &mut next_outputs,
            &resources.render_counters,
            requested.sample_rate,
        )
        .map_err(|error| failure(&error))?,
        ..Default::default()
    });
    Ok(Box::new(Prepared {
        reservation,
        captures,
        spec,
        active_spec: resources.active_spec.clone(),
        captures_cache: resources.capture_cache.clone(),
        inputs_cache: resources.input_cache.clone(),
        outputs_cache: resources.render_cache.clone(),
        next_captures,
        next_inputs,
        next_outputs,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abandoned_format_preparation_preserves_live_state_and_releases_reservation() {
        let resources = WindowsGraphResources::default();
        let requested = RenderFormat {
            sample_rate: 44_100,
            block_frames: 128,
        };
        let document = GraphDocument::new(7, vec![], vec![]);
        let pending = prepare(
            &resources,
            &document,
            super::super::render_format(),
            requested,
        )
        .unwrap();
        assert_eq!(resources.active_spec.lock().unwrap().graph_rate, 48_000);
        assert!(resources.mailbox.pending.load(Ordering::Acquire).is_null());
        assert!(resources.mailbox.reserve().is_err());
        drop(pending);
        assert!(resources.mailbox.reserve().is_ok());
        assert_eq!(resources.active_spec.lock().unwrap().graph_rate, 48_000);
    }

    #[test]
    fn committed_native_resources_wait_for_the_matching_dsp_revision() {
        let resources = WindowsGraphResources::default();
        let requested = RenderFormat {
            sample_rate: 96_000,
            block_frames: 128,
        };
        let document = GraphDocument::new(9, vec![], vec![]);
        prepare(
            &resources,
            &document,
            super::super::render_format(),
            requested,
        )
        .unwrap()
        .commit();
        let mut active = Box::<WindowsCaptureSet>::default();
        assert!(resources.activate_pending(&mut active, 8).is_err());
        assert_eq!(active.revision, 0);
        assert!(!resources.mailbox.pending.load(Ordering::Acquire).is_null());
        resources.activate_pending(&mut active, 9).unwrap();
        assert_eq!(active.revision, 9);
        assert_eq!(resources.active_spec.lock().unwrap().graph_rate, 96_000);
        resources.reclaim_retired();
        assert!(resources.mailbox.reserve().is_ok());
    }
}
