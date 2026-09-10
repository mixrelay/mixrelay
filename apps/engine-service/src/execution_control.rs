//! Builds one execution snapshot from authored catalog documents. Native resource and
//! meter identities come from that same snapshot; editor selection never owns capture.

use std::collections::BTreeMap;

use tonenari_audio_engine::BlockProcessor;
use tonenari_audio_graph::{
    CompileContext, GraphDocument, NodeConfig, NodeId, VirtualEndpointSpec,
};
use tonenari_audio_io::ApplicationCaptureRequest;
use tonenari_control_protocol::{
    ApplicationCaptureManifest, ApplicationCaptureSource, ErrorCode, GraphApplyRequest,
    PROTOCOL_VERSION, ProtocolError,
};

use crate::catalog_execution::{
    ApplicationIdentityResolver, CatalogExecutionContext, compose_catalog,
};
use crate::graph_catalog::Catalog;
use crate::graph_composition::{ComposedGraph, CompositionError, NodeOrigin, SharedSource};
use crate::{
    EngineGraphController, GraphResourceBackend, prepare_error, validate_endpoint_bindings,
    virtual_endpoint_specs,
};

pub(super) struct PreparedExecution {
    pub processor: Box<BlockProcessor>,
    pub document: GraphDocument,
    pub endpoints: Vec<VirtualEndpointSpec>,
    pub view: ExecutionView,
}

pub(super) struct ExecutionView {
    pub origins: BTreeMap<NodeId, NodeOrigin>,
    pub captures: ApplicationCaptureManifest,
}

struct NativeIdentities<'a>(&'a dyn GraphResourceBackend);

impl ApplicationIdentityResolver for NativeIdentities<'_> {
    fn resolve(
        &self,
        request: &ApplicationCaptureRequest,
    ) -> Result<SharedSource, CompositionError> {
        self.0
            .resolve_application_capture(request)
            .map_err(|error| CompositionError::Invalid(error.to_string()))
    }
}

impl EngineGraphController {
    pub(crate) fn view_graph_id(&self) -> String {
        self.catalog
            .lock()
            .as_ref()
            .and_then(|store| store.value.as_ref().ok())
            .map_or_else(
                || "session".into(),
                |catalog| catalog.selected_id().to_owned(),
            )
    }

    pub(super) fn compose_execution(
        &self,
        document: &GraphDocument,
        catalog: Option<&Catalog>,
        context: CompileContext,
    ) -> Result<ComposedGraph, ProtocolError> {
        let documents: Vec<_> = catalog.map_or_else(
            || vec![(self.view_graph_id(), document.clone())],
            |catalog| {
                catalog
                    .documents()
                    .map(|(id, graph)| (id.to_owned(), graph.clone()))
                    .collect()
            },
        );
        for (_, graph) in &documents {
            self.validate_session_bindings(graph, context.format)?;
        }
        let needs_devices = documents.iter().any(|(_, graph)| {
            graph.nodes.iter().any(|node| {
                matches!(
                    node.config,
                    NodeConfig::AudioDeviceInput(_) | NodeConfig::AudioDeviceOutput(_)
                )
            })
        });
        let devices = if needs_devices {
            self.graph_resources.device_inventory().map_err(invalid)?
        } else {
            Vec::new()
        };
        let compiler = self.factory.compiler();
        let composed = compose_catalog(
            &documents,
            CatalogExecutionContext {
                revision: document.revision,
                compile: context,
                compiler: &compiler,
                devices: &devices,
                applications: &NativeIdentities(self.graph_resources.as_ref()),
            },
        )
        .map_err(composition_error)?;
        self.validate_required_endpoint(document)?;
        Ok(composed)
    }

    pub(super) fn prepare_execution(
        &self,
        document: &GraphDocument,
        catalog: Option<&Catalog>,
        context: CompileContext,
    ) -> Result<PreparedExecution, ProtocolError> {
        // Production callers retain `mutation` through preparation and submission. With
        // no pending plan, the audio thread cannot retire a reuse owner while we prepare.
        let revisions = self.exchange.revisions();
        if revisions.active != revisions.submitted {
            return Err(ProtocolError {
                code: ErrorCode::GraphExchangeBusy,
                parameters: serde_json::json!({
                    "activeRevision": revisions.active,
                    "proposedRevision": document.revision,
                }),
            });
        }
        let composed = self.compose_execution(document, catalog, context)?;
        let captures = capture_manifest(&composed.document)?;
        let endpoints =
            virtual_endpoint_specs(&composed.document, self.endpoint_backend.capabilities())?;
        let processor = self
            .factory
            .prepare(&composed.execution)
            .map_err(prepare_error)?;
        validate_endpoint_bindings(processor.plan(), &endpoints)?;
        Ok(PreparedExecution {
            processor,
            document: composed.document,
            endpoints: endpoints.into_iter().map(|(_, spec)| spec).collect(),
            view: ExecutionView {
                origins: composed.node_origins,
                captures,
            },
        })
    }

    pub(super) fn current_capture_manifest(
        &self,
    ) -> Result<ApplicationCaptureManifest, ProtocolError> {
        let _mutation = self.mutation.lock();
        let revision = self.exchange.revisions().submitted;
        if let Some(view) = self.execution_views.read().get(&revision) {
            return Ok(view.captures.clone());
        }
        let document = self.graph.read().clone();
        let catalog = self.catalog_candidate(&document);
        capture_manifest(
            &self
                .compose_execution(&document, catalog.as_ref(), *self.context.read())?
                .document,
        )
    }

    pub(super) fn preview_capture_manifest(
        &self,
        request: GraphApplyRequest,
    ) -> Result<ApplicationCaptureManifest, ProtocolError> {
        let _mutation = self.mutation.lock();
        let revisions = self.exchange.revisions();
        if request.document.revision <= revisions.submitted {
            return Err(ProtocolError {
                code: ErrorCode::GraphRevisionStale,
                parameters: serde_json::json!({"submittedRevision": revisions.submitted}),
            });
        }
        if revisions.active != revisions.submitted {
            return Err(ProtocolError {
                code: ErrorCode::GraphExchangeBusy,
                parameters: serde_json::json!({"activeRevision": revisions.active}),
            });
        }
        let mut document = request.document;
        let mut catalog = self.catalog_candidate(&document);
        if let Some(candidate) = &mut catalog {
            self.reconcile_catalog_devices(candidate)?;
            document = candidate.active_document().clone();
        }
        let composed = self.compose_execution(&document, catalog.as_ref(), *self.context.read())?;
        capture_manifest(&composed.document)
    }
}

fn capture_manifest(document: &GraphDocument) -> Result<ApplicationCaptureManifest, ProtocolError> {
    let sources = document
        .nodes
        .iter()
        .filter_map(|node| match &node.config {
            NodeConfig::ApplicationCapture(request) => Some((node, request)),
            _ => None,
        })
        .map(|(node, request)| {
            // The current native capture transport uses a bounded source-token ABI. Validate
            // the final canonical identity, not an authored local ID before namespacing.
            if node.id.as_str().len() > 127 {
                return Err(invalid(
                    "canonical application capture identity exceeds native capacity",
                ));
            }
            Ok(ApplicationCaptureSource {
                node_id: node.id.as_str().to_owned(),
                request: request.clone(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ApplicationCaptureManifest {
        protocol_version: PROTOCOL_VERSION,
        revision: document.revision,
        sources,
    })
}

fn invalid(reason: impl std::fmt::Display) -> ProtocolError {
    ProtocolError {
        code: ErrorCode::GraphInvalid,
        parameters: serde_json::json!({"reason": reason.to_string()}),
    }
}

fn composition_error(error: CompositionError) -> ProtocolError {
    match error {
        CompositionError::Compile(error) => ProtocolError {
            code: ErrorCode::GraphInvalid,
            parameters: serde_json::json!({"issues":error.issues()}),
        },
        CompositionError::Invalid(reason) => invalid(reason),
    }
}

#[cfg(test)]
#[path = "execution_control_tests.rs"]
mod tests;
