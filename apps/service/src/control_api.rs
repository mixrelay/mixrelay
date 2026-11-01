//! Protocol admission and response projection for the engine controller.

use crate::{
    AppStatus, ControlBackend, EngineGraphController, ErrorCode, ExternalProcessorCatalog,
    GraphApplyRequest, GraphApplyResponse, GraphDocument, GraphRuntimeStatus, MeterSnapshot,
    PROTOCOL_VERSION, ProcessorEditorStatus, ProcessorParameterUpdateResponse,
    ProcessorRetryRequest, ProcessorScanRequest, ProtocolError, SessionReconfigureRequest,
    feature_policy, node_control, processor_editor_error, processor_parameter_error,
};

impl ControlBackend for EngineGraphController {
    fn telemetry_state(&self) -> Option<mixrelay_control_protocol::ControlStateSnapshot> {
        self.observe_control_state()
    }
    fn controls_snapshot(
        &self,
    ) -> Result<mixrelay_control_protocol::EngineControlSnapshot, ProtocolError> {
        self.snapshot_controls()
    }
    fn node_controls(
        &self,
        graph_id: &str,
    ) -> Result<mixrelay_control_protocol::GraphNodeControls, ProtocolError> {
        self.read_node_controls(graph_id)
    }

    fn update_node_control(
        &self,
        graph_id: &str,
        node_id: &str,
        request: mixrelay_control_protocol::NodeControlRequest,
    ) -> Result<mixrelay_control_protocol::NodeControlResponse, ProtocolError> {
        self.change_node_control(graph_id, node_id, &request)
    }

    fn preview_node_control(
        &self,
        graph_id: &str,
        node_id: &str,
        request: mixrelay_control_protocol::NodeControlRequest,
    ) -> Result<mixrelay_control_protocol::NodeControlPreview, ProtocolError> {
        self.preview_node_switch(graph_id, node_id, &request)
    }
    fn preview_workspace_file(
        &self,
        request: mixrelay_control_protocol::WorkspaceRequest,
    ) -> Result<mixrelay_control_protocol::WorkspacePreview, ProtocolError> {
        self.preview_workspace(request)
    }
    fn workspace_file(
        &self,
        request: mixrelay_control_protocol::WorkspaceRequest,
    ) -> Result<mixrelay_control_protocol::GraphCatalogResponse, ProtocolError> {
        self.workspace_operation(request).map_err(|mut error| {
            // Every fallible preparation precedes publication. Once accepted, storage
            // warnings travel in GraphCatalogResponse rather than rejecting the request.
            if let Some(parameters) = error.parameters.as_object_mut() {
                parameters.insert("workspaceOutcome".into(), serde_json::json!("not_applied"));
            }
            error
        })
    }
    fn recorder_status(
        &self,
        graph_id: &str,
        node_id: &str,
    ) -> Result<mixrelay_control_protocol::RecorderStatus, ProtocolError> {
        self.snapshot_recorder_status(graph_id, node_id)
    }

    fn control_recorder(
        &self,
        graph_id: &str,
        node_id: &str,
        request: mixrelay_control_protocol::RecorderControlRequest,
    ) -> Result<mixrelay_control_protocol::RecorderStatus, ProtocolError> {
        let _mutation = self.try_node_control()?;
        self.validate_control_revision(request.expected_revision)?;
        let id = self.recorder_session_id(graph_id, node_id)?;
        if let Some(expected) = request.expected_recording {
            let current = self.recorders.status(&id);
            if expected.state != current.state || expected.path != current.path {
                return Err(node_control::node_error(
                    ErrorCode::GraphRevisionStale,
                    graph_id,
                    node_id,
                ));
            }
        }
        let request = request.request;
        let free = !self.is_paid();
        if free && request.action == mixrelay_audio_recorder::RecorderAction::Record {
            self.validate_free_record_start(graph_id, node_id, &id)?;
        }
        if matches!(
            request.action,
            mixrelay_audio_recorder::RecorderAction::Stop
        ) && self.recorders.status(&id).state == mixrelay_audio_recorder::RecorderState::Stopped
        {
            return Ok(self.recorders.status(&id));
        }
        if matches!(
            request.action,
            mixrelay_audio_recorder::RecorderAction::Record
                | mixrelay_audio_recorder::RecorderAction::Pause
        ) {
            let graph = self.catalog_document_locked(graph_id)?;
            if !self.power.enabled()
                || !graph
                    .nodes
                    .iter()
                    .any(|node| node.id.as_str() == node_id && node.control.enabled)
            {
                return Err(node_control::node_error(
                    ErrorCode::NodeDisabled,
                    graph_id,
                    node_id,
                ));
            }
        }
        self.recorders.control(
            id.as_str(),
            request,
            free.then_some(feature_policy::FREE_RECORDING_SECONDS),
        )
    }
    fn application_capture_manifest(
        &self,
    ) -> Result<mixrelay_control_protocol::ApplicationCaptureManifest, ProtocolError> {
        self.current_capture_manifest()
    }

    fn preview_application_capture(
        &self,
        request: GraphApplyRequest,
    ) -> Result<mixrelay_control_protocol::ApplicationCaptureManifest, ProtocolError> {
        self.preview_capture_manifest(request)
    }
    fn reconfigure_virtual_endpoint(
        &self,
        request: mixrelay_control_protocol::VirtualEndpointFormatRequest,
    ) -> Result<GraphApplyResponse, ProtocolError> {
        self.change_endpoint_format(&request)
    }

    fn graph_layout(
        &self,
        id: &str,
    ) -> Result<mixrelay_control_protocol::GraphLayoutResponse, ProtocolError> {
        self.read_graph_layout(id)
    }

    fn update_graph_layout(
        &self,
        id: &str,
        request: mixrelay_control_protocol::GraphLayoutRequest,
    ) -> Result<mixrelay_control_protocol::GraphLayoutResponse, ProtocolError> {
        self.write_graph_layout(id, request)
    }
    fn graph_catalog(
        &self,
    ) -> Result<mixrelay_control_protocol::GraphCatalogResponse, ProtocolError> {
        self.read_catalog()
    }

    fn graph_catalog_document(&self, id: &str) -> Result<GraphDocument, ProtocolError> {
        self.read_catalog_document(id)
    }

    fn mutate_graph_catalog(
        &self,
        request: mixrelay_control_protocol::GraphCatalogRequest,
    ) -> Result<mixrelay_control_protocol::GraphCatalogResponse, ProtocolError> {
        self.mutate_catalog(request)
    }
    fn reconfigure_session(
        &self,
        request: SessionReconfigureRequest,
    ) -> Result<GraphApplyResponse, ProtocolError> {
        self.change_session(request)
    }
    fn status(&self) -> AppStatus {
        let format = self.context.read().format;
        let mut status = self.runtime_status.status(format);
        status.engine_power = Some(self.engine_power_status());
        status.processor_timings = self.processor_timings();
        status.processor_latencies = self.processor_latencies();
        status.failed_processors = self.failed_processor_status();
        status.recorder_error = self.recorders.cleanup_error();
        if let Some(session) = &mut status.audio_session {
            session.virtual_endpoint_settings = self.endpoint_settings();
            if status
                .engine_power
                .is_some_and(|power| !power.enabled && !power.transitioning)
            {
                session.processing_state = mixrelay_audio_io::process::ProcessingState::Stopped;
            }
        }
        status
    }

    fn graph_status(&self) -> GraphRuntimeStatus {
        let revisions = self.exchange.revisions();
        GraphRuntimeStatus {
            pending_reason: self
                .runtime_status
                .awaiting_microphone_permission()
                .then_some(
                    mixrelay_control_protocol::GraphPendingReason::AwaitingMicrophonePermission,
                ),
            protocol_version: PROTOCOL_VERSION,
            active_revision: revisions.active,
            submitted_revision: revisions.submitted,
        }
    }

    fn graph(&self) -> GraphDocument {
        self.graph.read().clone()
    }

    fn meters(&self) -> MeterSnapshot {
        self.meter_snapshot_at(std::time::Instant::now())
    }

    fn incarnation(&self) -> String {
        self.incarnation.clone()
    }

    fn processors(&self) -> ExternalProcessorCatalog {
        self.processor_catalog.protocol_catalog()
    }

    fn rescan_processors(
        &self,
        request: ProcessorScanRequest,
    ) -> Result<ExternalProcessorCatalog, ProtocolError> {
        let paths = request
            .paths
            .into_iter()
            .map(std::path::PathBuf::from)
            .collect::<Vec<_>>();
        if let Some(path) = paths.iter().find(|path| !path.is_dir()) {
            return Err(ProtocolError {
                code: ErrorCode::ProcessorUnavailable,
                parameters: serde_json::json!({
                    "reason": "processor scan path is not a directory",
                    "path": path,
                }),
            });
        }
        self.processor_catalog.rescan(&paths);
        Ok(self.processor_catalog.protocol_catalog())
    }

    fn set_processor_parameter(
        &self,
        node_id: &str,
        parameter_id: u32,
        request: mixrelay_control_protocol::ProcessorParameterUpdate,
    ) -> Result<ProcessorParameterUpdateResponse, ProtocolError> {
        self.apply_processor_parameter_update(
            node_id,
            parameter_id,
            request.value,
            |provider, runtime_id| {
                self.validate_control_revision(request.expected_revision)?;
                if !self
                    .processor_catalog
                    .parameter_is_writable(provider.type_id.as_str(), parameter_id)
                {
                    return Err(processor_parameter_error(
                        node_id,
                        parameter_id,
                        "processor parameter is unavailable or read-only",
                    ));
                }
                self.processor_controls
                    .set_parameter(runtime_id, parameter_id, request.value)
                    .map_err(|reason| processor_parameter_error(node_id, parameter_id, &reason))
            },
        )
    }

    fn processor_editor_status(
        &self,
        node_id: &str,
    ) -> Result<ProcessorEditorStatus, ProtocolError> {
        let _mutation = self.mutation.lock();
        self.processor_editor_status_locked(node_id)
    }

    fn retry_failed_processor(
        &self,
        request: ProcessorRetryRequest,
    ) -> Result<GraphApplyResponse, ProtocolError> {
        EngineGraphController::retry_failed_processor(self, &request)
    }

    fn set_processor_editor(
        &self,
        node_id: &str,
        request: mixrelay_control_protocol::ProcessorEditorRequest,
    ) -> Result<ProcessorEditorStatus, ProtocolError> {
        let _mutation = self.mutation.lock();
        self.validate_control_revision(request.expected_revision)?;
        let open = request.open;
        let runtime_id = self.external_processor_node(node_id)?;
        if !open {
            self.processor_controls
                .cancel_editor_gesture(&runtime_id)
                .map_err(|reason| processor_editor_error(node_id, &reason))?;
            // Preserve vendor edits while the instance is still available. A failed
            // durable checkpoint must remain retryable, not silently close the editor.
            self.checkpoint_processor_state_locked(Some(node_id))?;
        }
        let mut status = self
            .processor_controls
            .set_editor_open(&runtime_id, open)
            .map_err(|reason| processor_editor_error(node_id, &reason))?;
        node_id.clone_into(&mut status.node_id);
        Ok(status)
    }

    fn processor_editor_frame(
        &self,
        node_id: &str,
    ) -> Result<mixrelay_control_protocol::ProcessorEditorFrame, ProtocolError> {
        let runtime_id = {
            let _mutation = self.mutation.lock();
            self.external_processor_node(node_id)?
        };
        self.processor_controls
            .editor_frame(&runtime_id)
            .map_err(|reason| processor_editor_error(node_id, &reason))
    }

    fn processor_editor_input(
        &self,
        node_id: &str,
        input: mixrelay_control_protocol::ProcessorEditorInput,
    ) -> Result<(), ProtocolError> {
        let runtime_id = {
            let _mutation = self.mutation.lock();
            self.external_processor_node(node_id)?
        };
        self.processor_controls
            .editor_input(&runtime_id, &input)
            .map_err(|reason| processor_editor_error(node_id, &reason))
    }

    fn set_engine_power(
        &self,
        request: mixrelay_control_protocol::EnginePowerRequest,
    ) -> Result<GraphApplyResponse, ProtocolError> {
        self.change_engine_power(request.enabled, request.expected_revision)
    }

    fn preview_engine_start(
        &self,
    ) -> Result<mixrelay_control_protocol::ApplicationCaptureManifest, ProtocolError> {
        EngineGraphController::preview_engine_start(self)
    }

    fn apply_graph_request(
        &self,
        request: GraphApplyRequest,
    ) -> Result<GraphApplyResponse, ProtocolError> {
        let _mutation = self.mutation.lock();
        // Selection changes take the same mutation lock, so this check and the apply are atomic.
        if let Some(expected) = &request.expected_graph_id {
            let selected = self.view_graph_id();
            if *expected != selected {
                return Err(ProtocolError {
                    code: ErrorCode::GraphRevisionStale,
                    parameters: serde_json::json!({
                        "expectedGraphId": expected,
                        "selectedGraphId": selected,
                    }),
                });
            }
        }
        let reuse = self.processor_controls.reuse_transaction();
        let mut document = request.document;
        self.validate_feature_usage(&document, self.catalog_candidate(&document).as_ref())?;
        self.preserve_live_processor_state(&mut document, &request.processor_parameter_changes)?;
        let catalog = self.catalog_candidate(&document);
        let result = self.apply_graph_locked(document, catalog)?;
        reuse.commit();
        Ok(result)
    }
}
