use super::*;
use crate::{
    DEFAULT_ENDPOINT_NODE_ID, GraphResourceBackend, HostAudioBackendError, PlaybackBindings,
    default_duplex_document, endpoint_settings_tests,
};
use mixrelay_audio_graph::GraphDocument;
use mixrelay_audio_io::{NativeBufferSettings, VirtualEndpointId};
use mixrelay_control_server::ControlBackend;
use parking_lot::Mutex;
use std::sync::Arc;
struct Resources {
    settings: Mutex<NativeBufferSettings>,
    writes: Mutex<Vec<VirtualEndpointId>>,
    fail: bool,
    persistence_error: Mutex<Option<String>>,
}
impl GraphResourceBackend for Resources {
    fn reconcile(
        &self,
        _: &GraphDocument,
        _: PlaybackBindings<'_>,
    ) -> Result<(), HostAudioBackendError> {
        Ok(())
    }
    fn virtual_endpoint_buffer(&self, id: &VirtualEndpointId) -> Option<NativeBufferSettings> {
        (id.as_str() == DEFAULT_ENDPOINT_NODE_ID).then(|| *self.settings.lock())
    }
    fn set_virtual_endpoint_buffer(
        &self,
        id: &VirtualEndpointId,
        expected: Option<u32>,
        expected_preferred: Option<u32>,
        frames: u32,
    ) -> Result<Option<String>, HostAudioBackendError> {
        if self.fail {
            return Err(HostAudioBackendError::BackendFailure {
                message: "native buffer readback failed".into(),
            });
        }
        assert_eq!(self.settings.lock().current_frames, expected);
        assert_eq!(self.settings.lock().preferred_frames, expected_preferred);
        let mut settings = self.settings.lock();
        if settings.current_frames.is_some() {
            settings.current_frames = Some(frames);
        }
        settings.preferred_frames = Some(frames);
        self.writes.lock().push(id.clone());
        Ok(self.persistence_error.lock().clone())
    }
}
fn fixture(fail: bool) -> (EngineGraphController, Arc<Resources>) {
    let mut engine = endpoint_settings_tests::controller(default_duplex_document(1, None));
    let resources = Arc::new(Resources {
        settings: Mutex::new(NativeBufferSettings {
            current_frames: Some(512),
            preferred_frames: None,
            minimum_frames: 32,
            maximum_frames: 4096,
            can_set: true,
        }),
        writes: Mutex::new(vec![]),
        fail,
        persistence_error: Mutex::new(None),
    });
    engine.graph_resources = resources.clone();
    (engine, resources)
}
fn request() -> VirtualEndpointBufferRequest {
    VirtualEndpointBufferRequest {
        expected_revision: 1,
        endpoint_id: VirtualEndpointId::from(DEFAULT_ENDPOINT_NODE_ID),
        expected_buffer_frames: Some(512),
        expected_preferred_frames: None,
        buffer_frames: 128,
    }
}
#[test]
fn endpoint_buffer_does_not_republish_graph_or_change_dsp_format() {
    let (engine, resources) = fixture(false);
    let graph = engine.graph();
    let format = engine.context.read().format;
    let response = engine.change_endpoint_buffer(&request()).unwrap();
    assert_eq!(response.revision, 1);
    assert_eq!(engine.graph(), graph);
    assert_eq!(engine.context.read().format, format);
    assert_eq!(resources.settings.lock().current_frames, Some(128));
    assert_eq!(
        resources.writes.lock().as_slice(),
        &[VirtualEndpointId::from(DEFAULT_ENDPOINT_NODE_ID)]
    );
}
#[test]
fn stale_graph_buffer_or_unsupported_geometry_never_writes_native_client() {
    let (engine, resources) = fixture(false);
    let mut stale = request();
    stale.expected_revision = 0;
    assert!(engine.change_endpoint_buffer(&stale).is_err());
    let mut stale = request();
    stale.expected_buffer_frames = Some(256);
    assert!(engine.change_endpoint_buffer(&stale).is_err());
    let mut outside = request();
    outside.buffer_frames = 8192;
    assert_eq!(
        engine.change_endpoint_buffer(&outside).unwrap_err().code,
        ErrorCode::AudioSessionUnsupported
    );
    resources.settings.lock().can_set = false;
    assert!(engine.change_endpoint_buffer(&request()).is_err());
    assert!(resources.writes.lock().is_empty());
}
#[test]
fn native_readback_failure_keeps_graph_and_observation() {
    let (engine, resources) = fixture(true);
    assert!(engine.change_endpoint_buffer(&request()).is_err());
    assert_eq!(resources.settings.lock().current_frames, Some(512));
    assert_eq!(engine.graph().revision, 1);
    assert!(resources.writes.lock().is_empty());
}

#[test]
fn unused_device_saves_preference_without_claiming_live_io_or_graph_membership() {
    let (engine, resources) = fixture(false);
    engine.graph.write().nodes.clear();
    resources.settings.lock().current_frames = None;
    let graph = engine.graph();
    let mut request = request();
    request.expected_buffer_frames = None;
    engine.change_endpoint_buffer(&request).unwrap();
    assert_eq!(engine.graph(), graph);
    assert_eq!(resources.settings.lock().current_frames, None);
    assert_eq!(resources.settings.lock().preferred_frames, Some(128));
    assert_eq!(resources.writes.lock().len(), 1);
}
#[test]
fn unknown_device_identity_never_writes() {
    let (engine, resources) = fixture(false);
    let mut request = request();
    request.endpoint_id = VirtualEndpointId::from("absent");
    assert_eq!(
        engine.change_endpoint_buffer(&request).unwrap_err().code,
        ErrorCode::AudioSessionUnsupported
    );
    assert!(resources.writes.lock().is_empty());
}

#[test]
fn unused_device_preference_fences_concurrent_edits_with_null_live_value() {
    let (engine, resources) = fixture(false);
    resources.settings.lock().current_frames = None;
    let mut first = request();
    first.expected_buffer_frames = None;
    engine.change_endpoint_buffer(&first).unwrap();
    first.buffer_frames = 256;
    assert!(engine.change_endpoint_buffer(&first).is_err());
    assert_eq!(resources.settings.lock().preferred_frames, Some(128));
    assert_eq!(resources.writes.lock().len(), 1);
    first.expected_preferred_frames = Some(128);
    engine.change_endpoint_buffer(&first).unwrap();
    assert_eq!(resources.settings.lock().current_frames, None);
    assert_eq!(resources.settings.lock().preferred_frames, Some(256));
}

#[test]
fn persistence_failure_reports_applied_native_value_without_graph_republication() {
    let (engine, resources) = fixture(false);
    *resources.persistence_error.lock() = Some("settings directory read-only".into());
    let graph = engine.graph();
    let response = engine.change_endpoint_buffer(&request()).unwrap();
    assert_eq!(response.state, GraphApplyState::Active);
    assert_eq!(
        response.persistence_error.as_deref(),
        Some("settings directory read-only")
    );
    assert_eq!(resources.settings.lock().current_frames, Some(128));
    assert_eq!(engine.graph(), graph);
}
