use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use mixrelay_audio_graph::GraphDocument;
use mixrelay_control_protocol::{
    AppStatus, ErrorCode, ExternalProcessorCatalog, GraphApplyRequest, GraphApplyResponse,
    GraphCatalogRequest, GraphCatalogResponse, GraphLayoutRequest, GraphLayoutResponse,
    GraphRuntimeStatus, MeterSnapshot, PROTOCOL_VERSION, ProcessorEditorFrame,
    ProcessorEditorInput, ProcessorEditorRequest, ProcessorEditorStatus, ProcessorParameterUpdate,
    ProcessorParameterUpdateResponse, ProcessorRetryRequest, ProcessorScanRequest, ProtocolError,
    SessionReconfigureRequest,
};
mod editor_surface;
mod engine_power;
mod ingress;
mod node_control;
mod operations;
mod recorder;
mod telemetry;
mod transport;
mod workspace;
pub use operations::ControlOperations;
pub use transport::bounded_listener;

pub const HEALTH_ROUTE: &str = "/health";
pub const STATUS_ROUTE: &str = "/api/v1/status";
pub const AUDIO_SESSION_ROUTE: &str = "/api/v1/audio/session";
pub const VIRTUAL_ENDPOINT_FORMAT_ROUTE: &str = "/api/v1/audio/virtual-endpoint/format";
/// Driver runtime native client buffer control.
pub const VIRTUAL_ENDPOINT_BUFFER_ROUTE: &str = "/api/v1/audio/virtual-endpoint/buffer";
pub const GRAPH_STATUS_ROUTE: &str = "/api/v1/graph/status";
/// Coherent read-side control catalog, independent of graph mutation admission.
pub const CONTROL_SNAPSHOT_ROUTE: &str = "/api/v1/controls/snapshot";
pub const GRAPH_ROUTE: &str = "/api/v1/graph";
pub const GRAPH_CATALOG_ROUTE: &str = "/api/v1/graphs";
pub const GRAPH_CATALOG_DOCUMENT_ROUTE: &str = "/api/v1/graphs/{id}";
pub const GRAPH_LAYOUT_ROUTE: &str = "/api/v1/graphs/{id}/layout";
pub const METERS_ROUTE: &str = "/api/v1/meters";
/// Server-sent meter windows, one per publication window; a push alternative to polling.
pub const CONTROL_TELEMETRY_ROUTE: &str = "/api/v1/telemetry";
pub const METER_TELEMETRY_ROUTE: &str = CONTROL_TELEMETRY_ROUTE;
/// The publication window pushed meters follow.
const METER_TELEMETRY_INTERVAL: std::time::Duration = std::time::Duration::from_millis(50);
pub const APPLICATION_CAPTURE_MANIFEST_ROUTE: &str = "/api/v1/application-capture/manifest";
pub const APPLICATION_CAPTURE_PREVIEW_ROUTE: &str = "/api/v1/application-capture/preview";
pub const PROCESSORS_ROUTE: &str = "/api/v1/processors";
pub const PROCESSORS_SCAN_ROUTE: &str = "/api/v1/processors/scan";
pub const PROCESSOR_PARAMETER_ROUTE: &str =
    "/api/v1/processors/{node_id}/parameters/{parameter_id}";
pub const PROCESSOR_EDITOR_ROUTE: &str = "/api/v1/processors/{node_id}/editor";
pub const PROCESSOR_RETRY_ROUTE: &str = "/api/v1/processors/retry";

pub trait ControlBackend: Send + Sync {
    /// Observes state without preparing native resources. A busy writer may defer this
    /// observation: the publisher retains the last committed snapshot until it succeeds.
    fn telemetry_state(&self) -> Option<mixrelay_control_protocol::ControlStateSnapshot> {
        Some(crate::telemetry::snapshot(self))
    }

    /// Reads all targets, controls and recorder states from one engine observation.
    /// # Errors
    /// Reports unavailable control state; ordinary concurrent mutations do not fail reads.
    fn controls_snapshot(
        &self,
    ) -> Result<mixrelay_control_protocol::EngineControlSnapshot, ProtocolError> {
        Err(node_control::unavailable())
    }

    /// Discovers authored node controls in one graph without changing selection.
    /// # Errors
    /// Reports an unknown graph or unavailable backend.
    fn node_controls(
        &self,
        _graph_id: &str,
    ) -> Result<mixrelay_control_protocol::GraphNodeControls, ProtocolError> {
        Err(node_control::unavailable())
    }

    /// Preflights a revision-guarded node control patch without publishing it.
    /// # Errors
    /// Rejects missing targets, stale revisions and unsupported controls.
    fn preview_node_control(
        &self,
        _graph_id: &str,
        _node_id: &str,
        _request: mixrelay_control_protocol::NodeControlRequest,
    ) -> Result<mixrelay_control_protocol::NodeControlPreview, ProtocolError> {
        Err(node_control::unavailable())
    }

    /// Publishes explicit node state through the shared control owner.
    /// # Errors
    /// Rejects missing targets, stale revisions and native preparation failures.
    fn update_node_control(
        &self,
        _graph_id: &str,
        _node_id: &str,
        _request: mixrelay_control_protocol::NodeControlRequest,
    ) -> Result<mixrelay_control_protocol::NodeControlResponse, ProtocolError> {
        Err(node_control::unavailable())
    }
    /// Preflights the next start without acquiring audio or changing execution.
    /// # Errors
    /// Rejects non-stopped engines and invalid graph/native selections.
    fn preview_engine_start(
        &self,
    ) -> Result<mixrelay_control_protocol::ApplicationCaptureManifest, ProtocolError> {
        Err(ProtocolError {
            code: ErrorCode::ControlUnavailable,
            parameters: serde_json::json!({"resource":"engine_power"}),
        })
    }
    /// Publishes or retires execution without changing authored graph contents.
    /// # Errors
    /// Reports unsupported control, publication contention or native preparation errors.
    fn set_engine_power(
        &self,
        _request: mixrelay_control_protocol::EnginePowerRequest,
    ) -> Result<GraphApplyResponse, ProtocolError> {
        Err(ProtocolError {
            code: ErrorCode::ControlUnavailable,
            parameters: serde_json::json!({"resource":"engine_power"}),
        })
    }
    /// Prepares capture metadata without publishing the requested workspace.
    /// # Errors
    /// Rejects invalid files, stale revisions and unavailable composition resources.
    fn preview_workspace_file(
        &self,
        _request: mixrelay_control_protocol::WorkspaceRequest,
    ) -> Result<mixrelay_control_protocol::WorkspacePreview, ProtocolError> {
        Err(ProtocolError {
            code: ErrorCode::ControlUnavailable,
            parameters: serde_json::json!({"resource":"workspace_preview"}),
        })
    }
    /// Saves or replaces a complete authored workspace on the control thread.
    /// # Errors
    /// Rejects stale revisions, invalid files, active recordings, and preparation failures.
    fn workspace_file(
        &self,
        _request: mixrelay_control_protocol::WorkspaceRequest,
    ) -> Result<GraphCatalogResponse, ProtocolError> {
        Err(ProtocolError {
            code: ErrorCode::ControlUnavailable,
            parameters: serde_json::json!({"resource":"workspace"}),
        })
    }
    /// Reads one graph-owned recording without changing the selected graph.
    /// # Errors
    /// Reports an unavailable recorder or unknown graph/node identity.
    fn recorder_status(
        &self,
        _graph_id: &str,
        _node_id: &str,
    ) -> Result<mixrelay_control_protocol::RecorderStatus, ProtocolError> {
        Err(recorder::unavailable())
    }

    /// Starts, resumes, pauses, or finalizes a recording on the control thread.
    /// # Errors
    /// Reports format, path, state, or file errors without interrupting audio output.
    fn control_recorder(
        &self,
        _graph_id: &str,
        _node_id: &str,
        _request: mixrelay_control_protocol::RecorderControlRequest,
    ) -> Result<mixrelay_control_protocol::RecorderStatus, ProtocolError> {
        Err(recorder::unavailable())
    }
    /// Returns the canonical capture owners for the complete running catalog.
    /// # Errors
    /// Reports unavailable discovery or invalid capture ownership without fabricating sources.
    fn application_capture_manifest(
        &self,
    ) -> Result<mixrelay_control_protocol::ApplicationCaptureManifest, ProtocolError> {
        Err(ProtocolError {
            code: ErrorCode::ControlUnavailable,
            parameters: serde_json::json!({"resource":"application_capture_manifest"}),
        })
    }

    /// Validates the full candidate catalog and describes capture owners without publishing.
    /// # Errors
    /// Rejects stale/busy revisions, invalid topology and unsupported source policies.
    fn preview_application_capture(
        &self,
        _request: GraphApplyRequest,
    ) -> Result<mixrelay_control_protocol::ApplicationCaptureManifest, ProtocolError> {
        Err(ProtocolError {
            code: ErrorCode::ControlUnavailable,
            parameters: serde_json::json!({"resource":"application_capture_preview"}),
        })
    }
    /// Changes the own virtual device's format without targeting other devices.
    /// # Errors
    /// Rejects stale revisions, unsupported formats or native failures.
    fn reconfigure_virtual_endpoint(
        &self,
        _request: mixrelay_control_protocol::VirtualEndpointFormatRequest,
    ) -> Result<GraphApplyResponse, ProtocolError> {
        Err(ProtocolError {
            code: ErrorCode::AudioSessionUnsupported,
            parameters: serde_json::json!({"resource":"virtual_endpoint_format"}),
        })
    }
    /// Changes this runtime's own virtual-device I/O client buffer.
    /// # Errors
    /// Rejects stale observations, unavailable clients and unsupported native settings.
    fn reconfigure_virtual_endpoint_buffer(
        &self,
        _request: mixrelay_control_protocol::VirtualEndpointBufferRequest,
    ) -> Result<GraphApplyResponse, ProtocolError> {
        Err(ProtocolError {
            code: ErrorCode::AudioSessionUnsupported,
            parameters: serde_json::json!({"resource":"virtual_endpoint_buffer"}),
        })
    }
    /// Reads presentation metadata independently of the active DSP graph.
    /// # Errors
    /// Returns unavailable or unknown-identity errors without synthesizing layout entries.
    fn graph_layout(&self, _id: &str) -> Result<GraphLayoutResponse, ProtocolError> {
        Err(ProtocolError {
            code: ErrorCode::ControlUnavailable,
            parameters: serde_json::json!({"resource":"graph_layout"}),
        })
    }
    /// Atomically saves one graph's presentation without rebuilding its audio resources.
    /// # Errors
    /// Rejects unknown identities, invalid values, stale layout revisions and storage failures.
    fn update_graph_layout(
        &self,
        _id: &str,
        _request: GraphLayoutRequest,
    ) -> Result<GraphLayoutResponse, ProtocolError> {
        Err(ProtocolError {
            code: ErrorCode::ControlUnavailable,
            parameters: serde_json::json!({"resource":"graph_layout"}),
        })
    }
    /// Returns an inactive document for native preflight without changing the active graph.
    /// # Errors
    /// Rejects missing identities or unavailable persistence.
    fn graph_catalog_document(&self, _id: &str) -> Result<GraphDocument, ProtocolError> {
        Err(ProtocolError {
            code: ErrorCode::ControlUnavailable,
            parameters: serde_json::json!({}),
        })
    }
    /// Returns the durable graph collection, never an invented UI-only list.
    /// # Errors
    /// Returns an unavailable error if catalog persistence could not be loaded.
    fn graph_catalog(&self) -> Result<GraphCatalogResponse, ProtocolError> {
        Err(ProtocolError {
            code: ErrorCode::ControlUnavailable,
            parameters: serde_json::json!({"reason": "graph catalog unavailable"}),
        })
    }
    /// Serializes catalog edits with graph publication.
    /// Accepted execution changes return a response even if durable replacement fails;
    /// report that failure through `persistence_error`, never through `Err`.
    /// # Errors
    /// Rejects stale writers and invalid or unavailable graphs without changing the
    /// catalog, selected document, or execution. HTTP clients rely on `Err` as not applied.
    fn mutate_graph_catalog(
        &self,
        _request: GraphCatalogRequest,
    ) -> Result<GraphCatalogResponse, ProtocolError> {
        self.graph_catalog().and_then(|_| {
            Err(ProtocolError {
                code: ErrorCode::ControlUnavailable,
                parameters: serde_json::json!({"reason": "graph catalog mutations unavailable"}),
            })
        })
    }
    /// Reconfigures one engine-wide audio session, independently from endpoint channel maps.
    /// # Errors
    /// Returns a protocol error without pretending an unsupported adapter changed formats.
    fn reconfigure_session(
        &self,
        _request: SessionReconfigureRequest,
    ) -> Result<GraphApplyResponse, ProtocolError> {
        Err(ProtocolError {
            code: ErrorCode::AudioSessionUnsupported,
            parameters: serde_json::json!({}),
        })
    }
    fn status(&self) -> AppStatus;
    fn graph(&self) -> GraphDocument;
    fn graph_status(&self) -> GraphRuntimeStatus;
    fn meters(&self) -> MeterSnapshot;
    /// Identifies this engine process; a restarted engine has a new one.
    fn incarnation(&self) -> String {
        String::new()
    }
    fn processors(&self) -> ExternalProcessorCatalog;

    /// Rebuilds the external processor catalog from standard and explicit directories.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError`] when a path is invalid or discovery cannot be started.
    fn rescan_processors(
        &self,
        request: ProcessorScanRequest,
    ) -> Result<ExternalProcessorCatalog, ProtocolError>;

    /// Updates one parameter on an active hosted processor.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError`] when the node or parameter is unavailable, or when the
    /// processor worker cannot accept the value.
    fn set_processor_parameter(
        &self,
        node_id: &str,
        parameter_id: u32,
        update: ProcessorParameterUpdate,
    ) -> Result<ProcessorParameterUpdateResponse, ProtocolError>;

    /// Returns the current native editor state for one active processor node.
    ///
    /// # Errors
    ///
    /// Returns a stable protocol failure when the node or worker is unavailable.
    fn processor_editor_status(
        &self,
        node_id: &str,
    ) -> Result<ProcessorEditorStatus, ProtocolError>;

    /// Requests an idempotent native editor transition for one active processor node.
    ///
    /// # Errors
    ///
    /// Returns a stable protocol failure when the node or worker is unavailable.
    fn set_processor_editor(
        &self,
        node_id: &str,
        request: ProcessorEditorRequest,
    ) -> Result<ProcessorEditorStatus, ProtocolError>;

    /// Reprepares one exact active failed provider from its committed saved state.
    /// # Errors
    /// Rejects stale graph/catalog revisions, disabled or removed nodes, and stale worker IDs.
    fn retry_failed_processor(
        &self,
        _request: ProcessorRetryRequest,
    ) -> Result<GraphApplyResponse, ProtocolError> {
        Err(ProtocolError {
            code: ErrorCode::ProcessorUnavailable,
            parameters: serde_json::json!({"reason":"processor retry is unsupported"}),
        })
    }

    /// Captures a bounded native image without blocking the async HTTP executor.
    /// # Errors
    /// Returns a protocol failure for an unavailable or busy editor.
    fn processor_editor_frame(&self, node_id: &str) -> Result<ProcessorEditorFrame, ProtocolError> {
        Err(processor_unavailable(node_id))
    }

    /// Publishes ordered input for one identified native editor session.
    /// # Errors
    /// Rejects unavailable sessions and invalid or saturated input.
    fn processor_editor_input(
        &self,
        node_id: &str,
        _input: ProcessorEditorInput,
    ) -> Result<(), ProtocolError> {
        Err(processor_unavailable(node_id))
    }

    /// Validates and submits a graph document and its explicit parameter intent to the runtime.
    ///
    /// # Errors
    ///
    /// Returns a stable [`ProtocolError`] when validation, binding, revision,
    /// authorization, or processor submission fails.
    fn apply_graph_request(
        &self,
        request: GraphApplyRequest,
    ) -> Result<GraphApplyResponse, ProtocolError>;
}

#[derive(Clone)]
struct ControlState {
    telemetry: Arc<telemetry::Publisher>,
    backend: Arc<dyn ControlBackend>,
}

#[derive(Debug, Default)]
struct UnavailableBackend;

impl ControlBackend for UnavailableBackend {
    fn status(&self) -> AppStatus {
        AppStatus::default()
    }

    fn graph_status(&self) -> GraphRuntimeStatus {
        GraphRuntimeStatus {
            protocol_version: PROTOCOL_VERSION,
            active_revision: 0,
            submitted_revision: 0,
        }
    }

    fn graph(&self) -> GraphDocument {
        GraphDocument::new(0, vec![], vec![])
    }

    fn meters(&self) -> MeterSnapshot {
        MeterSnapshot {
            protocol_version: PROTOCOL_VERSION,
            revision: 0,
            nodes: Vec::new(),
        }
    }

    fn processors(&self) -> ExternalProcessorCatalog {
        ExternalProcessorCatalog {
            protocol_version: PROTOCOL_VERSION,
            processors: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn rescan_processors(
        &self,
        _request: ProcessorScanRequest,
    ) -> Result<ExternalProcessorCatalog, ProtocolError> {
        Err(ProtocolError {
            code: ErrorCode::ControlUnavailable,
            parameters: serde_json::json!({}),
        })
    }

    fn set_processor_parameter(
        &self,
        node_id: &str,
        parameter_id: u32,
        _update: ProcessorParameterUpdate,
    ) -> Result<ProcessorParameterUpdateResponse, ProtocolError> {
        Err(ProtocolError {
            code: ErrorCode::ProcessorUnavailable,
            parameters: serde_json::json!({
                "nodeId": node_id,
                "parameterId": parameter_id,
            }),
        })
    }

    fn processor_editor_status(
        &self,
        node_id: &str,
    ) -> Result<ProcessorEditorStatus, ProtocolError> {
        Err(processor_unavailable(node_id))
    }

    fn set_processor_editor(
        &self,
        node_id: &str,
        _request: ProcessorEditorRequest,
    ) -> Result<ProcessorEditorStatus, ProtocolError> {
        Err(processor_unavailable(node_id))
    }

    fn retry_failed_processor(
        &self,
        _request: ProcessorRetryRequest,
    ) -> Result<GraphApplyResponse, ProtocolError> {
        Err(ProtocolError {
            code: ErrorCode::ControlUnavailable,
            parameters: serde_json::json!({"resource":"processor_retry"}),
        })
    }

    fn apply_graph_request(
        &self,
        _request: GraphApplyRequest,
    ) -> Result<GraphApplyResponse, ProtocolError> {
        Err(ProtocolError {
            code: ErrorCode::ControlUnavailable,
            parameters: serde_json::json!({}),
        })
    }
}

pub fn router(operations: ControlOperations) -> Router {
    router_with_backend(Arc::new(UnavailableBackend), operations)
}

pub fn router_with_backend(
    backend: Arc<dyn ControlBackend>,
    operations: ControlOperations,
) -> Router {
    operation_middleware(control_routes(), operations, backend.incarnation()).with_state(
        ControlState {
            backend,
            telemetry: Arc::default(),
        },
    )
}

fn operation_middleware<S: Clone + Send + Sync + 'static>(
    routes: Router<S>,
    operations: ControlOperations,
    incarnation: String,
) -> Router<S> {
    routes
        // The last layer is outermost in axum. Keep body admission outside the operation
        // dispatcher so malformed/incomplete bodies never consume an operation receipt.
        // Ingress resolves existing client IDs before acquiring a body permit.
        .layer(axum::middleware::from_fn(operations::dispatch))
        .layer(axum::middleware::from_fn(ingress::read_body))
        // Ingress already bounds every body; avoid Json's separate 2 MiB default.
        .layer(DefaultBodyLimit::disable())
        .layer(axum::Extension(operations))
        .layer(axum::middleware::from_fn_with_state(
            incarnation,
            operations::engine_identity,
        ))
}

fn control_routes() -> Router<ControlState> {
    Router::new()
        .route(
            "/api/v1/operations/{id}",
            get(operations::get).delete(operations::delete),
        )
        .route(
            "/api/v1/graphs/{id}/nodes/controls",
            get(node_control::list),
        )
        .route(
            "/api/v1/graphs/{id}/nodes/{node_id}/control",
            put(node_control::update),
        )
        .route(
            "/api/v1/graphs/{id}/nodes/{node_id}/control/preview",
            post(node_control::preview),
        )
        .route("/api/v1/workspace", post(workspace::operate))
        .route("/api/v1/workspace/preview", post(workspace::preview))
        .route(
            "/api/v1/graphs/{id}/recorders/{node_id}",
            get(recorder::status).put(recorder::control),
        )
        .route(HEALTH_ROUTE, get(|| async { "ok" }))
        .route(STATUS_ROUTE, get(status))
        .route(AUDIO_SESSION_ROUTE, put(reconfigure_session))
        .route("/api/v1/audio/power", put(engine_power::set))
        .route("/api/v1/audio/power/preview", post(engine_power::preview))
        .route(
            VIRTUAL_ENDPOINT_FORMAT_ROUTE,
            put(reconfigure_virtual_endpoint),
        )
        .route(
            VIRTUAL_ENDPOINT_BUFFER_ROUTE,
            put(reconfigure_virtual_endpoint_buffer),
        )
        .route(GRAPH_STATUS_ROUTE, get(graph_status))
        .route(CONTROL_SNAPSHOT_ROUTE, get(node_control::snapshot))
        .route(METERS_ROUTE, get(meters))
        .route(CONTROL_TELEMETRY_ROUTE, get(telemetry::subscribe))
        .route(
            APPLICATION_CAPTURE_MANIFEST_ROUTE,
            get(application_capture_manifest),
        )
        .route(
            APPLICATION_CAPTURE_PREVIEW_ROUTE,
            post(preview_application_capture),
        )
        .route(PROCESSORS_ROUTE, get(processors))
        .route(PROCESSORS_SCAN_ROUTE, put(rescan_processors))
        .route(PROCESSOR_PARAMETER_ROUTE, put(set_processor_parameter))
        .route(PROCESSOR_RETRY_ROUTE, put(retry_failed_processor))
        .route(
            PROCESSOR_EDITOR_ROUTE,
            get(processor_editor_status).put(set_processor_editor),
        )
        .route(GRAPH_ROUTE, get(graph).put(apply_graph))
        .route(
            "/api/v1/processors/{node_id}/editor/frame",
            get(editor_surface::frame),
        )
        .route(
            "/api/v1/processors/{node_id}/editor/input",
            post(editor_surface::input),
        )
        .route(
            GRAPH_CATALOG_ROUTE,
            get(graph_catalog).put(mutate_graph_catalog),
        )
        .route(GRAPH_CATALOG_DOCUMENT_ROUTE, get(graph_catalog_document))
        .route(
            GRAPH_LAYOUT_ROUTE,
            get(graph_layout).put(update_graph_layout),
        )
}

async fn status(State(state): State<ControlState>) -> Response {
    catalog_task(move || Json(state.backend.status()).into_response()).await
}

async fn graph_status(State(state): State<ControlState>) -> Json<GraphRuntimeStatus> {
    Json(state.backend.graph_status())
}

async fn graph(State(state): State<ControlState>) -> Json<GraphDocument> {
    Json(state.backend.graph())
}

async fn application_capture_manifest(State(state): State<ControlState>) -> Response {
    catalog_task(move || match state.backend.application_capture_manifest() {
        Ok(response) => Json(response).into_response(),
        Err(error) => (protocol_error_status(error.code), Json(error)).into_response(),
    })
    .await
}

async fn preview_application_capture(
    State(state): State<ControlState>,
    request: Result<Json<GraphApplyRequest>, JsonRejection>,
) -> Response {
    let request = match request {
        Ok(Json(request)) => request,
        Err(rejection) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ProtocolError {
                    code: ErrorCode::GraphInvalid,
                    parameters: serde_json::json!({"reason": rejection.body_text()}),
                }),
            )
                .into_response();
        }
    };
    catalog_task(
        move || match state.backend.preview_application_capture(request) {
            Ok(response) => Json(response).into_response(),
            Err(error) => (protocol_error_status(error.code), Json(error)).into_response(),
        },
    )
    .await
}

async fn graph_catalog(State(state): State<ControlState>) -> Response {
    catalog_task(move || match state.backend.graph_catalog() {
        Ok(response) => Json(response).into_response(),
        Err(error) => (protocol_error_status(error.code), Json(error)).into_response(),
    })
    .await
}

async fn graph_layout(State(state): State<ControlState>, Path(id): Path<String>) -> Response {
    catalog_task(move || match state.backend.graph_layout(&id) {
        Ok(response) => Json(response).into_response(),
        Err(error) => (protocol_error_status(error.code), Json(error)).into_response(),
    })
    .await
}

async fn update_graph_layout(
    State(state): State<ControlState>,
    Path(id): Path<String>,
    request: Result<Json<GraphLayoutRequest>, JsonRejection>,
) -> Response {
    let request = match request {
        Ok(Json(request)) => request,
        Err(rejection) => return (StatusCode::UNPROCESSABLE_ENTITY, Json(ProtocolError {
            code: ErrorCode::GraphInvalid, parameters: serde_json::json!({"resource":"graph_layout","graphId":id,"reason":rejection.body_text()}),
        })).into_response(),
    };
    catalog_task(
        move || match state.backend.update_graph_layout(&id, request) {
            Ok(response) => Json(response).into_response(),
            Err(error) => (protocol_error_status(error.code), Json(error)).into_response(),
        },
    )
    .await
}

async fn graph_catalog_document(
    State(state): State<ControlState>,
    Path(id): Path<String>,
) -> Response {
    catalog_task(move || match state.backend.graph_catalog_document(&id) {
        Ok(response) => Json(response).into_response(),
        Err(error) => (protocol_error_status(error.code), Json(error)).into_response(),
    })
    .await
}

async fn mutate_graph_catalog(
    State(state): State<ControlState>,
    request: Result<Json<GraphCatalogRequest>, JsonRejection>,
) -> Response {
    let request = match request {
        Ok(Json(request)) => request,
        Err(error) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ProtocolError {
                    code: ErrorCode::GraphInvalid,
                    parameters: serde_json::json!({"reason": error.body_text()}),
                }),
            )
                .into_response();
        }
    };
    catalog_task(move || match state.backend.mutate_graph_catalog(request) {
        Ok(response) => Json(response).into_response(),
        Err(mut error) => {
            error.parameters["catalogOutcome"] = serde_json::json!("not_applied");
            (protocol_error_status(error.code), Json(error)).into_response()
        }
    })
    .await
}

// Catalog reads may wait behind a native snapshot and writes perform filesystem I/O.
// Neither may block a Tokio reactor, even when there is only one asynchronous worker.
async fn catalog_task(work: impl FnOnce() -> Response + Send + 'static) -> Response {
    tokio::task::spawn_blocking(work).await.unwrap_or_else(|_| {
        operations::task_failure(
            ErrorCode::ControlUnavailable,
            "graph catalog control task failed",
        )
    })
}

async fn meters(State(state): State<ControlState>) -> Json<MeterSnapshot> {
    Json(state.backend.meters())
}

async fn reconfigure_session(
    State(state): State<ControlState>,
    request: Result<Json<SessionReconfigureRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match request {
        Ok(request) => request,
        Err(error) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ProtocolError {
                    code: ErrorCode::AudioSessionUnsupported,
                    parameters: serde_json::json!({"reason": error.body_text()}),
                }),
            )
                .into_response();
        }
    };
    // Driver negotiation and plug-in preparation can block. Do not occupy a Tokio I/O worker.
    match tokio::task::spawn_blocking(move || state.backend.reconfigure_session(request)).await {
        Ok(Ok(response)) => (StatusCode::ACCEPTED, Json(response)).into_response(),
        Ok(Err(error)) => (protocol_error_status(error.code), Json(error)).into_response(),
        Err(_) => operations::task_failure(
            ErrorCode::AudioSessionReconfigureFailed,
            "session control task failed",
        ),
    }
}

async fn processors(State(state): State<ControlState>) -> Json<ExternalProcessorCatalog> {
    Json(state.backend.processors())
}

async fn reconfigure_virtual_endpoint(
    State(state): State<ControlState>,
    request: Result<Json<mixrelay_control_protocol::VirtualEndpointFormatRequest>, JsonRejection>,
) -> Response {
    let request = match request {
        Ok(Json(request)) => request,
        Err(error) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ProtocolError {
                    code: ErrorCode::AudioSessionUnsupported,
                    parameters: serde_json::json!({"reason": error.body_text()}),
                }),
            )
                .into_response();
        }
    };
    match tokio::task::spawn_blocking(move || state.backend.reconfigure_virtual_endpoint(request))
        .await
    {
        Ok(Ok(response)) => (StatusCode::ACCEPTED, Json(response)).into_response(),
        Ok(Err(error)) => (protocol_error_status(error.code), Json(error)).into_response(),
        Err(_) => operations::task_failure(
            ErrorCode::AudioSessionReconfigureFailed,
            "virtual endpoint control task failed",
        ),
    }
}

async fn reconfigure_virtual_endpoint_buffer(
    State(state): State<ControlState>,
    request: Result<Json<mixrelay_control_protocol::VirtualEndpointBufferRequest>, JsonRejection>,
) -> Response {
    let request = match request {
        Ok(Json(request)) => request,
        Err(error) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ProtocolError {
                    code: ErrorCode::AudioSessionUnsupported,
                    parameters: serde_json::json!({"reason":error.body_text()}),
                }),
            )
                .into_response();
        }
    };
    match tokio::task::spawn_blocking(move || {
        state.backend.reconfigure_virtual_endpoint_buffer(request)
    })
    .await
    {
        Ok(Ok(response)) => (StatusCode::ACCEPTED, Json(response)).into_response(),
        Ok(Err(error)) => (protocol_error_status(error.code), Json(error)).into_response(),
        Err(_) => operations::task_failure(
            ErrorCode::AudioSessionReconfigureFailed,
            "virtual endpoint buffer task failed",
        ),
    }
}

async fn rescan_processors(
    State(state): State<ControlState>,
    request: Result<Json<ProcessorScanRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match request {
        Ok(request) => request,
        Err(rejection) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ProtocolError {
                    code: ErrorCode::GraphInvalid,
                    parameters: serde_json::json!({ "reason": rejection.body_text() }),
                }),
            )
                .into_response();
        }
    };
    match state.backend.rescan_processors(request) {
        Ok(catalog) => (StatusCode::OK, Json(catalog)).into_response(),
        Err(error) => (protocol_error_status(error.code), Json(error)).into_response(),
    }
}

async fn set_processor_parameter(
    State(state): State<ControlState>,
    Path((node_id, parameter_id)): Path<(String, u32)>,
    update: Result<Json<ProcessorParameterUpdate>, JsonRejection>,
) -> Response {
    let Json(update) = match update {
        Ok(update) => update,
        Err(rejection) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ProtocolError {
                    code: ErrorCode::GraphInvalid,
                    parameters: serde_json::json!({ "reason": rejection.body_text() }),
                }),
            )
                .into_response();
        }
    };
    // Accepted parameter updates also checkpoint durable graph state. Keep filesystem work
    // off Tokio's I/O workers; the backend still publishes the native value before saving.
    match tokio::task::spawn_blocking(move || {
        state
            .backend
            .set_processor_parameter(&node_id, parameter_id, update)
    })
    .await
    {
        Ok(Ok(response)) => (StatusCode::OK, Json(response)).into_response(),
        Ok(Err(error)) => (protocol_error_status(error.code), Json(error)).into_response(),
        Err(_) => operations::task_failure(
            ErrorCode::ControlUnavailable,
            "processor parameter control task failed",
        ),
    }
}

async fn processor_editor_status(
    State(state): State<ControlState>,
    Path(node_id): Path<String>,
) -> Response {
    match state.backend.processor_editor_status(&node_id) {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => (protocol_error_status(error.code), Json(error)).into_response(),
    }
}

async fn set_processor_editor(
    State(state): State<ControlState>,
    Path(node_id): Path<String>,
    request: Result<Json<ProcessorEditorRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match request {
        Ok(request) => request,
        Err(rejection) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ProtocolError {
                    code: ErrorCode::GraphInvalid,
                    parameters: serde_json::json!({ "reason": rejection.body_text() }),
                }),
            )
                .into_response();
        }
    };
    // Closing includes a bounded vendor state export and durable filesystem write.
    // Do not stall meter/status HTTP requests on the asynchronous I/O executor.
    match tokio::task::spawn_blocking(move || state.backend.set_processor_editor(&node_id, request))
        .await
    {
        Ok(Ok(response)) => (StatusCode::OK, Json(response)).into_response(),
        Ok(Err(error)) => (protocol_error_status(error.code), Json(error)).into_response(),
        Err(_) => operations::task_failure(
            ErrorCode::ControlUnavailable,
            "processor editor control task failed",
        ),
    }
}

async fn retry_failed_processor(
    State(state): State<ControlState>,
    request: Result<Json<ProcessorRetryRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match request {
        Ok(request) => request,
        Err(rejection) => {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(ProtocolError {
                    code: ErrorCode::GraphInvalid,
                    parameters: serde_json::json!({ "reason": rejection.body_text() }),
                }),
            )
                .into_response();
        }
    };
    match tokio::task::spawn_blocking(move || state.backend.retry_failed_processor(request)).await {
        Ok(Ok(response)) => (StatusCode::OK, Json(response)).into_response(),
        Ok(Err(error)) => (protocol_error_status(error.code), Json(error)).into_response(),
        Err(_) => {
            operations::task_failure(ErrorCode::ControlUnavailable, "processor retry task failed")
        }
    }
}

fn processor_unavailable(node_id: &str) -> ProtocolError {
    ProtocolError {
        code: ErrorCode::ProcessorUnavailable,
        parameters: serde_json::json!({ "nodeId": node_id }),
    }
}

async fn apply_graph(
    State(state): State<ControlState>,
    document: Result<Json<GraphApplyRequest>, JsonRejection>,
) -> Response {
    let Json(document) = match document {
        Ok(document) => document,
        Err(rejection) => {
            let error = ProtocolError {
                code: ErrorCode::GraphInvalid,
                parameters: serde_json::json!({ "reason": rejection.body_text() }),
            };
            return (StatusCode::UNPROCESSABLE_ENTITY, Json(error)).into_response();
        }
    };
    match tokio::task::spawn_blocking(move || state.backend.apply_graph_request(document)).await {
        Ok(Ok(response)) => (StatusCode::ACCEPTED, Json(response)).into_response(),
        Ok(Err(error)) => (protocol_error_status(error.code), Json(error)).into_response(),
        Err(_) => {
            operations::task_failure(ErrorCode::ControlUnavailable, "graph control task failed")
        }
    }
}

fn protocol_error_status(code: ErrorCode) -> StatusCode {
    match code {
        ErrorCode::InvalidOperationId => StatusCode::BAD_REQUEST,
        ErrorCode::GraphRevisionStale
        | ErrorCode::GraphExchangeBusy
        | ErrorCode::NodeDisabled
        | ErrorCode::EngineReplaced => StatusCode::CONFLICT,
        ErrorCode::NodeNotFound => StatusCode::NOT_FOUND,
        ErrorCode::FeatureLimit => StatusCode::FORBIDDEN,
        ErrorCode::GraphInvalid
        | ErrorCode::GraphBindingUnsupported
        | ErrorCode::AudioSessionUnsupported => StatusCode::UNPROCESSABLE_ENTITY,
        ErrorCode::Unauthorized | ErrorCode::PairingRequired => StatusCode::UNAUTHORIZED,
        _ => StatusCode::SERVICE_UNAVAILABLE,
    }
}

#[cfg(test)]
mod tests {
    mod application_capture;
    mod endpoint_buffer;
    mod endpoint_format;
    mod node_control;
    mod recorder;
    use std::sync::Mutex;

    use axum::{
        body::Body,
        http::{Method, Request, header::CONTENT_TYPE},
    };
    use mixrelay_control_protocol::GraphApplyState;
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use super::*;

    mod telemetry;

    struct TestBackend {
        incarnation: String,
        last_node_control: Mutex<Option<node_control::RecordedControl>>,
        last_recorder: Mutex<
            Option<(
                String,
                String,
                mixrelay_control_protocol::RecorderControlRequest,
            )>,
        >,
        apply_result: Mutex<Result<GraphApplyResponse, ProtocolError>>,
        last_apply: Mutex<Option<GraphApplyRequest>>,
        last_preview: Mutex<Option<GraphApplyRequest>>,
        last_endpoint_format:
            Mutex<Option<mixrelay_control_protocol::VirtualEndpointFormatRequest>>,
        last_endpoint_buffer:
            Mutex<Option<mixrelay_control_protocol::VirtualEndpointBufferRequest>>,
        last_retry: Mutex<Option<ProcessorRetryRequest>>,
        editor_gate: Option<Arc<EditorGate>>,
    }

    #[derive(Default)]
    struct EditorGate {
        started: std::sync::atomic::AtomicBool,
        released: std::sync::atomic::AtomicBool,
        finished: std::sync::atomic::AtomicBool,
    }

    impl TestBackend {
        fn with_result(result: Result<GraphApplyResponse, ProtocolError>) -> Arc<Self> {
            Arc::new(Self {
                incarnation: String::new(),
                last_node_control: Mutex::new(None),
                last_recorder: Mutex::new(None),
                apply_result: Mutex::new(result),
                last_apply: Mutex::new(None),
                last_preview: Mutex::new(None),
                last_endpoint_format: Mutex::new(None),
                last_endpoint_buffer: Mutex::new(None),
                last_retry: Mutex::new(None),
                editor_gate: None,
            })
        }
    }

    impl ControlBackend for TestBackend {
        fn telemetry_state(&self) -> Option<mixrelay_control_protocol::ControlStateSnapshot> {
            if self.incarnation == "deferred-observation" {
                let mut admitted = self.last_preview.lock().unwrap();
                if admitted.is_none() {
                    *admitted = Some(document().into());
                    return None;
                }
            }
            Some(crate::telemetry::snapshot(self))
        }
        fn incarnation(&self) -> String {
            self.incarnation.clone()
        }
        fn controls_snapshot(
            &self,
        ) -> Result<mixrelay_control_protocol::EngineControlSnapshot, ProtocolError> {
            Ok(node_control::snapshot())
        }

        fn node_controls(
            &self,
            graph_id: &str,
        ) -> Result<mixrelay_control_protocol::GraphNodeControls, ProtocolError> {
            Ok(node_control::discovery(graph_id))
        }

        fn preview_node_control(
            &self,
            graph_id: &str,
            node_id: &str,
            request: mixrelay_control_protocol::NodeControlRequest,
        ) -> Result<mixrelay_control_protocol::NodeControlPreview, ProtocolError> {
            self.update_node_control(graph_id, node_id, request)
                .map(|response| mixrelay_control_protocol::NodeControlPreview {
                    capture_manifest: Some(mixrelay_control_protocol::ApplicationCaptureManifest {
                        protocol_version: PROTOCOL_VERSION,
                        revision: response.application.revision,
                        sources: Vec::new(),
                    }),
                })
        }

        fn update_node_control(
            &self,
            graph_id: &str,
            node_id: &str,
            request: mixrelay_control_protocol::NodeControlRequest,
        ) -> Result<mixrelay_control_protocol::NodeControlResponse, ProtocolError> {
            *self.last_node_control.lock().expect("test mutex") =
                Some(node_control::RecordedControl {
                    graph_id: graph_id.to_owned(),
                    node_id: node_id.to_owned(),
                    request,
                });
            self.apply_graph_request(document().into())
                .map(
                    |application| mixrelay_control_protocol::NodeControlResponse {
                        application,
                        requires_graph_reload: false,
                    },
                )
        }
        fn recorder_status(
            &self,
            graph_id: &str,
            node_id: &str,
        ) -> Result<mixrelay_control_protocol::RecorderStatus, ProtocolError> {
            Ok(mixrelay_control_protocol::RecorderStatus {
                path: Some(format!("{graph_id}/{node_id}")),
                ..Default::default()
            })
        }

        fn control_recorder(
            &self,
            graph_id: &str,
            node_id: &str,
            request: mixrelay_control_protocol::RecorderControlRequest,
        ) -> Result<mixrelay_control_protocol::RecorderStatus, ProtocolError> {
            *self.last_recorder.lock().unwrap() =
                Some((graph_id.to_owned(), node_id.to_owned(), request));
            self.recorder_status(graph_id, node_id)
        }
        fn application_capture_manifest(
            &self,
        ) -> Result<mixrelay_control_protocol::ApplicationCaptureManifest, ProtocolError> {
            Ok(mixrelay_control_protocol::ApplicationCaptureManifest {
                protocol_version: PROTOCOL_VERSION,
                revision: 5,
                sources: vec![],
            })
        }

        fn preview_application_capture(
            &self,
            request: GraphApplyRequest,
        ) -> Result<mixrelay_control_protocol::ApplicationCaptureManifest, ProtocolError> {
            let revision = request.document.revision;
            *self.last_preview.lock().unwrap() = Some(request);
            self.apply_result.lock().unwrap().clone()?;
            Ok(mixrelay_control_protocol::ApplicationCaptureManifest {
                protocol_version: PROTOCOL_VERSION,
                revision,
                sources: vec![],
            })
        }
        fn reconfigure_virtual_endpoint(
            &self,
            request: mixrelay_control_protocol::VirtualEndpointFormatRequest,
        ) -> Result<GraphApplyResponse, ProtocolError> {
            *self.last_endpoint_format.lock().unwrap() = Some(request);
            self.apply_result.lock().unwrap().clone()
        }
        fn reconfigure_virtual_endpoint_buffer(
            &self,
            request: mixrelay_control_protocol::VirtualEndpointBufferRequest,
        ) -> Result<GraphApplyResponse, ProtocolError> {
            *self.last_endpoint_buffer.lock().unwrap() = Some(request);
            self.apply_result.lock().unwrap().clone()
        }
        fn graph_layout(&self, id: &str) -> Result<GraphLayoutResponse, ProtocolError> {
            if id != "saved" {
                return Err(ProtocolError {
                    code: ErrorCode::GraphInvalid,
                    parameters: json!({"resource":"graph_layout"}),
                });
            }
            Ok(GraphLayoutResponse {
                graph_id: id.into(),
                layout_revision: 4,
                layout: mixrelay_control_protocol::GraphLayout::default(),
            })
        }

        fn update_graph_layout(
            &self,
            id: &str,
            request: GraphLayoutRequest,
        ) -> Result<GraphLayoutResponse, ProtocolError> {
            let mut response = self.graph_layout(id)?;
            if request.expected_layout_revision != response.layout_revision {
                return Err(ProtocolError {
                    code: ErrorCode::GraphRevisionStale,
                    parameters: json!({"resource":"graph_layout"}),
                });
            }
            response.layout = request.layout;
            response.layout_revision += 1;
            Ok(response)
        }
        fn graph_catalog(&self) -> Result<GraphCatalogResponse, ProtocolError> {
            Ok(GraphCatalogResponse {
                protocol_version: PROTOCOL_VERSION,
                catalog_revision: 7,
                active_id: "saved".into(),
                graphs: vec![mixrelay_control_protocol::GraphCatalogEntry {
                    id: "saved".into(),
                    name: "Ableton".into(),
                }],
                document: document(),
                layout_revision: 0,
                layout: mixrelay_control_protocol::GraphLayout::default(),
                persistence_error: None,
            })
        }

        fn graph_catalog_document(&self, id: &str) -> Result<GraphDocument, ProtocolError> {
            if id == "saved" {
                return Ok(document());
            }
            Err(ProtocolError {
                code: ErrorCode::GraphInvalid,
                parameters: json!({}),
            })
        }

        fn mutate_graph_catalog(
            &self,
            request: GraphCatalogRequest,
        ) -> Result<GraphCatalogResponse, ProtocolError> {
            if request.expected_revision != 5 || request.expected_catalog_revision != 7 {
                return Err(ProtocolError {
                    code: ErrorCode::GraphRevisionStale,
                    parameters: json!({}),
                });
            }
            let mixrelay_control_protocol::GraphCatalogAction::Rename { id, name } = request.action
            else {
                return Err(ProtocolError {
                    code: ErrorCode::GraphInvalid,
                    parameters: json!({}),
                });
            };
            let mut response = self.graph_catalog()?;
            assert_eq!(id, "saved");
            response.graphs[0].name = name;
            response.catalog_revision += 1;
            Ok(response)
        }
        fn status(&self) -> AppStatus {
            AppStatus {
                driver_state: mixrelay_control_protocol::DriverState::Ready,
                sample_rate: Some(48_000),
                buffer_frames: Some(128),
                processor_latencies: vec![mixrelay_control_protocol::ProcessorLatency {
                    graph_id: "graph-a".to_owned(),
                    node_id: "effect".to_owned(),
                    transport_frames: 128,
                    intrinsic_frames: None,
                }],
                failed_processors: vec![mixrelay_control_protocol::FailedProcessor {
                    graph_id: "graph-a".to_owned(),
                    node_id: "effect".to_owned(),
                    instance_id: "18446744073709551615".to_owned(),
                }],
                ..AppStatus::default()
            }
        }

        fn graph_status(&self) -> GraphRuntimeStatus {
            GraphRuntimeStatus {
                protocol_version: PROTOCOL_VERSION,
                active_revision: 4,
                submitted_revision: 5,
            }
        }

        fn graph(&self) -> GraphDocument {
            assert_ne!(self.incarnation, "panic-observation");
            let mut graph = document();
            if self.incarnation == "editor-observation" {
                graph.nodes = serde_json::from_value(serde_json::json!([{
                    "id": "effect", "type": "gain", "config": {"channels": 2, "gainDb": 0.0}
                }]))
                .unwrap();
            }
            graph
        }

        fn meters(&self) -> MeterSnapshot {
            MeterSnapshot {
                protocol_version: PROTOCOL_VERSION,
                revision: 4,
                nodes: Vec::new(),
            }
        }

        fn processors(&self) -> ExternalProcessorCatalog {
            ExternalProcessorCatalog {
                protocol_version: PROTOCOL_VERSION,
                processors: Vec::new(),
                diagnostics: Vec::new(),
            }
        }

        fn rescan_processors(
            &self,
            _request: ProcessorScanRequest,
        ) -> Result<ExternalProcessorCatalog, ProtocolError> {
            Ok(self.processors())
        }

        fn set_processor_parameter(
            &self,
            node_id: &str,
            parameter_id: u32,
            update: ProcessorParameterUpdate,
        ) -> Result<ProcessorParameterUpdateResponse, ProtocolError> {
            Ok(ProcessorParameterUpdateResponse {
                protocol_version: PROTOCOL_VERSION,
                node_id: node_id.to_owned(),
                parameter_id,
                value: update.value,
                persistence_error: (node_id == "unsaved-node")
                    .then(|| "fixture disk unavailable".to_owned()),
            })
        }

        fn processor_editor_status(
            &self,
            node_id: &str,
        ) -> Result<ProcessorEditorStatus, ProtocolError> {
            Ok(test_editor_status(node_id, false))
        }

        fn set_processor_editor(
            &self,
            node_id: &str,
            request: ProcessorEditorRequest,
        ) -> Result<ProcessorEditorStatus, ProtocolError> {
            if let Some(gate) = &self.editor_gate {
                use std::{
                    sync::atomic::Ordering,
                    time::{Duration, Instant},
                };
                gate.started.store(true, Ordering::Release);
                let deadline = Instant::now() + Duration::from_secs(2);
                while !gate.released.load(Ordering::Acquire) && Instant::now() < deadline {
                    std::thread::park_timeout(Duration::from_millis(1));
                }
                gate.finished.store(true, Ordering::Release);
            }
            Ok(test_editor_status(node_id, request.open))
        }

        fn retry_failed_processor(
            &self,
            request: ProcessorRetryRequest,
        ) -> Result<GraphApplyResponse, ProtocolError> {
            *self.last_retry.lock().unwrap() = Some(request);
            self.apply_result.lock().unwrap().clone()
        }

        fn apply_graph_request(
            &self,
            request: GraphApplyRequest,
        ) -> Result<GraphApplyResponse, ProtocolError> {
            *self.last_apply.lock().unwrap() = Some(request);
            self.apply_result
                .lock()
                .expect("test result should not be poisoned")
                .clone()
        }
    }

    #[tokio::test]
    async fn engine_header_and_fence_cover_success_errors_and_fallbacks() {
        let mut backend = TestBackend::with_result(Ok(GraphApplyResponse {
            protocol_version: PROTOCOL_VERSION,
            persistence_error: None,
            revision: 2,
            state: mixrelay_control_protocol::GraphApplyState::Pending,
        }));
        Arc::get_mut(&mut backend).unwrap().incarnation = "engine-one".into();
        let operations = ControlOperations::new();
        let router = router_with_backend(backend.clone(), operations);
        for expected in [Some("old-engine"), Some("engine-one"), None] {
            *backend.last_apply.lock().unwrap() = None;
            let mut request = Request::put(GRAPH_ROUTE).header(CONTENT_TYPE, "application/json");
            if let Some(expected) = expected {
                request = request.header("x-mixrelay-expected-engine", expected);
            }
            let response = router
                .clone()
                .oneshot(
                    request
                        .body(Body::from(serde_json::to_vec(&document()).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.headers()["x-mixrelay-engine"], "engine-one");
            if expected == Some("old-engine") {
                assert_eq!(response.status(), StatusCode::CONFLICT);
                let bytes = axum::body::to_bytes(response.into_body(), 4096)
                    .await
                    .unwrap();
                let error: ProtocolError = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(error.code, ErrorCode::EngineReplaced);
                assert_eq!(error.parameters["operationOutcome"], "not_admitted");
                assert!(backend.last_apply.lock().unwrap().is_none());
            } else {
                assert_eq!(response.status(), StatusCode::ACCEPTED);
                assert!(backend.last_apply.lock().unwrap().is_some());
            }
        }
        for (method, path, status) in [
            (Method::GET, HEALTH_ROUTE, StatusCode::OK),
            (Method::GET, "/missing", StatusCode::NOT_FOUND),
            (Method::POST, HEALTH_ROUTE, StatusCode::METHOD_NOT_ALLOWED),
            (
                Method::GET,
                "/api/v1/operations/unknown",
                StatusCode::NOT_FOUND,
            ),
            (
                Method::DELETE,
                "/api/v1/operations/unknown",
                StatusCode::NOT_FOUND,
            ),
            (Method::PUT, GRAPH_ROUTE, StatusCode::UNPROCESSABLE_ENTITY),
        ] {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .header(CONTENT_TYPE, "application/json")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.headers()["x-mixrelay-engine"], "engine-one");
            assert_eq!(response.status(), status);
        }
        let response = super::router(ControlOperations::new())
            .oneshot(Request::get(HEALTH_ROUTE).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert!(!response.headers().contains_key("x-mixrelay-engine"));
    }

    fn document() -> GraphDocument {
        GraphDocument::new(5, vec![], vec![])
    }

    #[tokio::test]
    async fn graph_apply_forwards_explicit_parameter_intent_and_rejects_malformed_entries() {
        let backend = TestBackend::with_result(Ok(GraphApplyResponse {
            protocol_version: PROTOCOL_VERSION,
            revision: 5,
            state: GraphApplyState::Active,
            persistence_error: None,
        }));
        let mut request = GraphApplyRequest::from(document());
        request.processor_parameter_changes.push(
            mixrelay_control_protocol::GraphProcessorParameterChange {
                node_id: "effect".into(),
                type_id: "vst3:fixture".into(),
                parameter_id: 7,
                value: 0.25,
            },
        );
        let router = router_with_backend(backend.clone(), ControlOperations::new());
        let (status, _) = response_json(
            router.clone(),
            Request::builder()
                .method("PUT")
                .uri(GRAPH_ROUTE)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&request).unwrap()))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(*backend.last_apply.lock().unwrap(), Some(request.clone()));
        let mut invalid = serde_json::to_value(&request).unwrap();
        invalid["processorParameterChanges"][0]["value"] = Value::Null;
        let (status, body) = response_json(
            router,
            Request::builder()
                .method("PUT")
                .uri(GRAPH_ROUTE)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&invalid).unwrap()))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["code"], "graph_invalid");
        assert_eq!(*backend.last_apply.lock().unwrap(), Some(request.clone()));
        assert_eq!(
            UnavailableBackend
                .apply_graph_request(request)
                .unwrap_err()
                .code,
            ErrorCode::ControlUnavailable,
            "an unavailable backend rejects the complete request"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn native_editor_checkpoint_does_not_block_status_on_the_io_executor() {
        use std::{sync::atomic::Ordering, time::Duration};
        let gate = Arc::new(EditorGate::default());
        let backend = Arc::new(TestBackend {
            incarnation: String::new(),
            last_node_control: Mutex::new(None),
            last_recorder: Mutex::new(None),
            apply_result: Mutex::new(Err(processor_unavailable("unused"))),
            last_apply: Mutex::new(None),
            last_preview: Mutex::new(None),
            last_endpoint_format: Mutex::new(None),
            last_endpoint_buffer: Mutex::new(None),
            last_retry: Mutex::new(None),
            editor_gate: Some(gate.clone()),
        });
        let router = router_with_backend(backend, ControlOperations::new());
        let close_router = router.clone();
        let close = tokio::spawn(async move {
            response_json(
                close_router,
                Request::builder()
                    .method("PUT")
                    .uri("/api/v1/processors/effect/editor")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"expectedRevision":5,"open":false}"#))
                    .unwrap(),
            )
            .await
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            while !gate.started.load(Ordering::Acquire) {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
        let status = tokio::time::timeout(
            Duration::from_millis(100),
            router.oneshot(
                Request::builder()
                    .uri(STATUS_ROUTE)
                    .body(Body::empty())
                    .unwrap(),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        let was_waiting = !gate.finished.load(Ordering::Acquire);
        gate.released.store(true, Ordering::Release);
        assert_eq!(status.status(), StatusCode::OK);
        assert!(
            was_waiting,
            "status must complete before the native checkpoint is released"
        );
        assert_eq!(close.await.unwrap().0, StatusCode::OK);
    }

    fn test_editor_status(node_id: &str, open: bool) -> ProcessorEditorStatus {
        ProcessorEditorStatus {
            protocol_version: PROTOCOL_VERSION,
            node_id: node_id.to_owned(),
            state: if open {
                mixrelay_control_protocol::ProcessorEditorState::Open
            } else {
                mixrelay_control_protocol::ProcessorEditorState::Closed
            },
            width: open.then_some(640),
            height: open.then_some(480),
            resizable: open,
            failure: None,
        }
    }

    async fn response_json(router: Router, request: Request<Body>) -> (StatusCode, Value) {
        let response = router.oneshot(request).await.expect("request should run");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("response should have a body");
        let value = serde_json::from_slice(&bytes).expect("response should be JSON");
        (status, value)
    }

    #[tokio::test]
    async fn meter_telemetry_streams_every_window_until_admission_closes() {
        use futures_util::StreamExt as _;
        let backend = TestBackend::with_result(Ok(GraphApplyResponse {
            protocol_version: PROTOCOL_VERSION,
            revision: 4,
            state: GraphApplyState::Active,
            persistence_error: None,
        }));
        let operations = ControlOperations::new();
        let response = router_with_backend(backend, operations.clone())
            .oneshot(
                Request::builder()
                    .uri(METER_TELEMETRY_ROUTE)
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("request should run");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CONTENT_TYPE], "text/event-stream");
        let mut events = response.into_body().into_data_stream();
        let first = tokio::time::timeout(std::time::Duration::from_secs(2), events.next())
            .await
            .expect("first window")
            .expect("open stream")
            .expect("event bytes");
        let text = String::from_utf8(first.to_vec()).expect("utf-8 event");
        assert!(text.starts_with("event: telemetry\n"), "{text}");
        let data = text
            .lines()
            .find_map(|line| line.strip_prefix("data: "))
            .expect("event data");
        let telemetry: mixrelay_control_protocol::ControlTelemetry =
            serde_json::from_str(data).expect("telemetry JSON");
        assert_eq!(telemetry.sequence, 1);
        assert_eq!(telemetry.meters.revision, 4);
        // The test backend has no engine identity.
        assert_eq!(telemetry.engine_incarnation, "");
        // A steady window is sent again, so subscribers can tell it from a stall.
        let second = tokio::time::timeout(std::time::Duration::from_secs(2), events.next())
            .await
            .expect("second window")
            .expect("open stream")
            .expect("event bytes");
        assert!(String::from_utf8_lossy(&second).contains("\"sequence\":2"));
        operations.close_admission();
        let ended = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while events.next().await.is_some() {}
        })
        .await;
        assert!(ended.is_ok(), "closing admission ends the stream");
    }

    #[tokio::test]
    async fn graph_layout_routes_preserve_ui_identities_and_use_independent_revision() {
        let backend = TestBackend::with_result(Ok(GraphApplyResponse {
            protocol_version: PROTOCOL_VERSION,
            revision: 5,
            state: GraphApplyState::Active,
            persistence_error: None,
        }));
        let route = "/api/v1/graphs/saved/layout";
        let (status, body) = response_json(
            router_with_backend(backend.clone(), ControlOperations::new()),
            Request::builder().uri(route).body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["graphId"], "saved");
        assert_eq!(body["layoutRevision"], 4);
        let layout = json!({"positions":{"duplex:input":{"x":-50.0,"y":75.0}},"viewport":{"x":-100.0,"y":80.0,"zoom":0.5}});
        for (revision, expected_status) in [(4, StatusCode::OK), (3, StatusCode::CONFLICT)] {
            let (status, body) = response_json(
                router_with_backend(backend.clone(), ControlOperations::new()),
                Request::builder()
                    .method(Method::PUT)
                    .uri(route)
                    .header(CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        json!({"expectedLayoutRevision":revision,"layout":layout}).to_string(),
                    ))
                    .unwrap(),
            )
            .await;
            assert_eq!(status, expected_status);
            if status == StatusCode::OK {
                assert_eq!(body["layout"], layout);
                assert_eq!(body["layoutRevision"], 5);
            } else {
                assert_eq!(body["code"], "graph_revision_stale");
                assert_eq!(body["parameters"]["resource"], "graph_layout");
            }
        }
        let invalid =
            json!({"expectedLayoutRevision":4,"layout":{"positions":{"node":{"x":null,"y":1}}}});
        let (status, body) = response_json(
            router_with_backend(backend, ControlOperations::new()),
            Request::builder()
                .method(Method::PUT)
                .uri(route)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(invalid.to_string()))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(body["parameters"]["resource"], "graph_layout");
        assert_eq!(body["parameters"]["graphId"], "saved");
    }

    #[tokio::test]
    async fn catalog_routes_preserve_document_identity_and_both_concurrency_baselines() {
        let backend = TestBackend::with_result(Ok(GraphApplyResponse {
            protocol_version: PROTOCOL_VERSION,
            revision: 5,
            state: GraphApplyState::Active,
            persistence_error: None,
        }));
        for route in [GRAPH_CATALOG_ROUTE, "/api/v1/graphs/saved"] {
            let (status, body) = response_json(
                router_with_backend(backend.clone(), ControlOperations::new()),
                Request::builder().uri(route).body(Body::empty()).unwrap(),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            if route == GRAPH_CATALOG_ROUTE {
                assert_eq!(body["activeId"], "saved");
                assert_eq!(body["catalogRevision"], 7);
                assert_eq!(body["document"]["revision"], 5);
            } else {
                assert_eq!(body["revision"], 5);
            }
        }
        for (revision, expected_status) in [(7, StatusCode::OK), (6, StatusCode::CONFLICT)] {
            let (status, body) = response_json(router_with_backend(backend.clone(), ControlOperations::new()), Request::builder().method(Method::PUT).uri(GRAPH_CATALOG_ROUTE).header(CONTENT_TYPE, "application/json")
                .body(Body::from(json!({"action":"rename","id":"saved","name":"SPAT","expectedRevision":5,"expectedCatalogRevision":revision}).to_string())).unwrap()).await;
            assert_eq!(status, expected_status);
            if status == StatusCode::OK {
                assert_eq!(body["graphs"][0]["name"], "SPAT");
            } else {
                assert_eq!(body["code"], "graph_revision_stale");
                assert_eq!(body["parameters"]["catalogOutcome"], "not_applied");
            }
        }
        let (status, _) = response_json(
            router_with_backend(backend, ControlOperations::new()),
            Request::builder()
                .method(Method::PUT)
                .uri(GRAPH_CATALOG_ROUTE)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn default_routes_are_loopback_safe_and_versioned() {
        let health = router(ControlOperations::new())
            .oneshot(
                Request::builder()
                    .uri(HEALTH_ROUTE)
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("health request should run");
        let health_body = axum::body::to_bytes(health.into_body(), 16)
            .await
            .expect("health should have a body");
        assert_eq!(&health_body[..], b"ok");

        let (_, status) = response_json(
            router(ControlOperations::new()),
            Request::builder()
                .uri(STATUS_ROUTE)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await;
        assert_eq!(status["driverState"], "not_installed");

        let (_, graph) = response_json(
            router(ControlOperations::new()),
            Request::builder()
                .uri(GRAPH_STATUS_ROUTE)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await;
        assert_eq!(graph["activeRevision"], 0);

        let (_, graph_document) = response_json(
            router(ControlOperations::new()),
            Request::builder()
                .uri(GRAPH_ROUTE)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await;
        assert_eq!(graph_document["revision"], 0);

        let (status, error) = apply_request(router(ControlOperations::new()), &document()).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(error["code"], "control_unavailable");
    }

    #[tokio::test]
    async fn backend_status_and_graph_application_are_exposed() {
        let backend = TestBackend::with_result(Ok(GraphApplyResponse {
            protocol_version: PROTOCOL_VERSION,
            revision: 5,
            state: GraphApplyState::Pending,
            persistence_error: None,
        }));
        let (_, status) = response_json(
            router_with_backend(backend.clone(), ControlOperations::new()),
            Request::builder()
                .uri(STATUS_ROUTE)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await;
        assert_eq!(status["driverState"], "ready");
        assert_eq!(status["processorLatencies"][0]["graphId"], "graph-a");
        assert_eq!(status["processorLatencies"][0]["nodeId"], "effect");
        assert_eq!(status["processorLatencies"][0]["transportFrames"], 128);
        assert!(status["processorLatencies"][0]["intrinsicFrames"].is_null());
        assert_eq!(
            status["failedProcessors"][0]["instanceId"],
            "18446744073709551615"
        );
        assert!(status.get("processorTimings").is_none());

        let (_, graph) = response_json(
            router_with_backend(backend.clone(), ControlOperations::new()),
            Request::builder()
                .uri(GRAPH_STATUS_ROUTE)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await;
        assert_eq!(graph["activeRevision"], 4);
        assert_eq!(graph["submittedRevision"], 5);

        let (_, graph_document) = response_json(
            router_with_backend(backend.clone(), ControlOperations::new()),
            Request::builder()
                .uri(GRAPH_ROUTE)
                .body(Body::empty())
                .expect("request should build"),
        )
        .await;
        assert_eq!(graph_document["revision"], 5);

        let (status, response) = apply_request(
            router_with_backend(backend, ControlOperations::new()),
            &document(),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
        assert_eq!(response["revision"], 5);
        assert_eq!(response["state"], "pending");
    }

    #[tokio::test]
    async fn processor_parameters_use_a_typed_node_route() {
        let backend = TestBackend::with_result(Ok(GraphApplyResponse {
            protocol_version: PROTOCOL_VERSION,
            revision: 5,
            state: GraphApplyState::Pending,
            persistence_error: None,
        }));
        let (status, response) = response_json(
            router_with_backend(backend, ControlOperations::new()),
            Request::builder()
                .method(Method::PUT)
                .uri("/api/v1/processors/vst-node/parameters/42")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"expectedRevision":5,"value":0.75}"#))
                .expect("request should build"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(response["nodeId"], "vst-node");
        assert_eq!(response["parameterId"], 42);
        assert_eq!(response["value"], 0.75);
    }

    #[tokio::test]
    async fn failed_processor_retry_uses_a_typed_revision_and_instance_guarded_route() {
        let backend = TestBackend::with_result(Ok(GraphApplyResponse {
            protocol_version: PROTOCOL_VERSION,
            revision: 6,
            state: GraphApplyState::Pending,
            persistence_error: None,
        }));
        let (status, response) = response_json(
            router_with_backend(backend.clone(), ControlOperations::new()),
            Request::builder()
                .method(Method::PUT)
                .uri(PROCESSOR_RETRY_ROUTE)
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"graphId":"graph-a","nodeId":"effect","expectedRevision":5,"expectedCatalogRevision":7,"expectedInstanceId":"18446744073709551615"}"#,
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(response["revision"], 6);
        assert_eq!(
            *backend.last_retry.lock().unwrap(),
            Some(ProcessorRetryRequest {
                graph_id: "graph-a".into(),
                node_id: "effect".into(),
                expected_revision: 5,
                expected_catalog_revision: 7,
                expected_instance_id: "18446744073709551615".into(),
            })
        );
    }

    #[tokio::test]
    async fn accepted_processor_parameter_storage_warning_remains_http_success() {
        let backend = TestBackend::with_result(Ok(GraphApplyResponse {
            protocol_version: PROTOCOL_VERSION,
            revision: 5,
            state: GraphApplyState::Active,
            persistence_error: None,
        }));
        let (status, response) = response_json(
            router_with_backend(backend, ControlOperations::new()),
            Request::builder()
                .method(Method::PUT)
                .uri("/api/v1/processors/unsaved-node/parameters/42")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"expectedRevision":5,"value":0.75}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(response["value"], 0.75);
        assert_eq!(response["persistenceError"], "fixture disk unavailable");
    }

    #[tokio::test]
    async fn processor_editor_uses_typed_get_and_idempotent_put_routes() {
        let backend = TestBackend::with_result(Ok(GraphApplyResponse {
            protocol_version: PROTOCOL_VERSION,
            revision: 5,
            state: GraphApplyState::Pending,
            persistence_error: None,
        }));
        let (status, closed) = response_json(
            router_with_backend(backend.clone(), ControlOperations::new()),
            Request::builder()
                .uri("/api/v1/processors/processor-node/editor")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(closed["state"], "closed");

        let (status, opened) = response_json(
            router_with_backend(backend, ControlOperations::new()),
            Request::builder()
                .method(Method::PUT)
                .uri("/api/v1/processors/processor-node/editor")
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"expectedRevision":5,"open":true}"#))
                .expect("request should build"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(opened["nodeId"], "processor-node");
        assert_eq!(opened["state"], "open");
        assert_eq!(opened["width"], 640);
        assert_eq!(opened["height"], 480);
        assert_eq!(opened["resizable"], true);
    }

    #[tokio::test]
    async fn typed_graph_failures_map_to_stable_http_statuses() {
        let cases = [
            (ErrorCode::EngineReplaced, StatusCode::CONFLICT),
            (ErrorCode::InvalidOperationId, StatusCode::BAD_REQUEST),
            (ErrorCode::GraphRevisionStale, StatusCode::CONFLICT),
            (ErrorCode::GraphExchangeBusy, StatusCode::CONFLICT),
            (ErrorCode::GraphInvalid, StatusCode::UNPROCESSABLE_ENTITY),
            (
                ErrorCode::GraphBindingUnsupported,
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
            (ErrorCode::Unauthorized, StatusCode::UNAUTHORIZED),
            (ErrorCode::PairingRequired, StatusCode::UNAUTHORIZED),
            (ErrorCode::DriverNotLoaded, StatusCode::SERVICE_UNAVAILABLE),
        ];
        for (code, expected_status) in cases {
            let backend = TestBackend::with_result(Err(ProtocolError {
                code,
                parameters: json!({ "revision": 5 }),
            }));
            let (status, response) = apply_request(
                router_with_backend(backend, ControlOperations::new()),
                &document(),
            )
            .await;
            assert_eq!(status, expected_status);
            assert_eq!(response["parameters"]["revision"], 5);
        }
    }

    async fn apply_request(router: Router, document: &GraphDocument) -> (StatusCode, Value) {
        response_json(
            router,
            Request::builder()
                .method("PUT")
                .uri(GRAPH_ROUTE)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(document).expect("document should serialize"),
                ))
                .expect("request should build"),
        )
        .await
    }
}
