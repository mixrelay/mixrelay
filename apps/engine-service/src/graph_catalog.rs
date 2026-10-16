//! Durable graph documents are owned by the engine, not by a sidebar component.
//! The controller's mutation lock serializes catalog edits with graph/session publication.
//! Selection only changes the edited view; live processor instances belong to the composed
//! catalog and are not destroyed or snapshotted merely because another graph is selected.

use std::{collections::BTreeSet, fs::File, io::Read, path::PathBuf};

use mixrelay_control_protocol::{
    GraphCatalogAction, GraphCatalogEntry, GraphCatalogRequest, GraphCatalogResponse, GraphLayout,
};
use serde::{Deserialize, Serialize};

use super::{
    ControlBackend, EngineGraphController, ErrorCode, GraphDocument, NodeConfig, PROTOCOL_VERSION,
    ProtocolError,
};
use crate::persistence::{StagedWrite, config_directory, stage_json_limited};

const MAX_BYTES: u64 = 16 * 1024 * 1024;

fn document_write_error(error: &std::io::Error) -> ProtocolError {
    if error.kind() == std::io::ErrorKind::InvalidData {
        invalid(&error.to_string())
    } else {
        unavailable(&error.to_string())
    }
}
const MAX_GRAPHS: usize = 256;

#[path = "graph_layout.rs"]
mod layout;

#[path = "graph_native_state.rs"]
mod native_state;

#[path = "graph_live_state.rs"]
pub(crate) mod live_state;

#[path = "graph_restore.rs"]
mod restore;
pub(super) use restore::PendingRestore;

#[path = "device_catalog.rs"]
mod devices;

mod state_assets;
mod workspace;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Catalog {
    version: u32,
    revision: u64,
    active_id: String,
    graphs: Vec<SavedGraph>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SavedGraph {
    id: String,
    name: String,
    document: GraphDocument,
    layout_revision: u64,
    layout: GraphLayout,
}

pub(super) struct CatalogStore {
    pub(super) path: PathBuf,
    pub(super) persistent: bool,
    pub(super) value: Result<Catalog, String>,
}

pub(super) fn invalid(reason: &str) -> ProtocolError {
    ProtocolError {
        code: ErrorCode::GraphInvalid,
        parameters: serde_json::json!({"reason": reason.to_string()}),
    }
}

pub(super) fn unavailable(reason: &str) -> ProtocolError {
    ProtocolError {
        code: ErrorCode::ControlUnavailable,
        parameters: serde_json::json!({"reason": reason.to_string(), "resource": "graph_catalog"}),
    }
}

fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.len() <= 256 && !name.chars().any(char::is_control)
}

impl Catalog {
    pub(super) fn selected_id(&self) -> &str {
        &self.active_id
    }

    pub(super) fn documents_mut(&mut self) -> impl Iterator<Item = (&str, &mut GraphDocument)> {
        self.graphs
            .iter_mut()
            .map(|graph| (graph.id.as_str(), &mut graph.document))
    }
    pub(super) fn active_document(&self) -> &GraphDocument {
        &self.active().document
    }

    pub(super) fn documents(&self) -> impl Iterator<Item = (&str, &GraphDocument)> {
        self.graphs
            .iter()
            .map(|graph| (graph.id.as_str(), &graph.document))
    }

    pub(super) fn control_documents(&self) -> impl Iterator<Item = (&str, &str, &GraphDocument)> {
        self.graphs
            .iter()
            .map(|graph| (graph.id.as_str(), graph.name.as_str(), &graph.document))
    }

    fn imported(document: GraphDocument) -> Self {
        Self {
            version: 1,
            revision: 1,
            active_id: "graph-1".into(),
            graphs: vec![SavedGraph {
                id: "graph-1".into(),
                name: "Untitled Graph".into(),
                document,
                layout_revision: 0,
                layout: GraphLayout::default(),
            }],
        }
    }

    fn load(path: &std::path::Path) -> Result<Option<Self>, String> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        let mut bytes = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err("graph catalog exceeds size limit".into());
        }
        let mut value: Self = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        value.validate()?;
        for graph in &mut value.graphs {
            restore::require_pid_reselection(&mut graph.document);
        }
        Ok(Some(value))
    }

    fn validate(&self) -> Result<(), String> {
        let value = self;
        let mut ids = BTreeSet::new();
        if value.version != 1
            || value.graphs.is_empty()
            || value.graphs.len() > MAX_GRAPHS
            || !value.graphs.iter().all(|graph| {
                valid_name(&graph.name)
                    && !graph.id.is_empty()
                    && graph.id.len() <= 256
                    && ids.insert(graph.id.clone())
                    && layout::validate(&graph.layout).is_ok()
            })
            || !ids.contains(&value.active_id)
        {
            return Err("invalid graph catalog identity, name or version".into());
        }
        state_assets::referenced(self).map_err(|error| {
            error
                .parameters
                .get("reason")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("invalid processor state reference")
                .to_owned()
        })?;
        Ok(())
    }

    fn active(&self) -> &SavedGraph {
        self.graphs
            .iter()
            .find(|graph| graph.id == self.active_id)
            .expect("validated active graph")
    }

    /// Startup is a new publication, not a new revision namespace. Include
    /// inactive graphs so selecting another saved view cannot reuse an old ID.
    fn next_audio_revision(&self, submitted: u64) -> Option<u64> {
        self.graphs
            .iter()
            .map(|graph| graph.document.revision)
            .fold(submitted, u64::max)
            .checked_add(1)
    }

    pub(super) fn update_document(&mut self, document: GraphDocument) {
        self.graphs
            .iter_mut()
            .find(|graph| graph.id == self.active_id)
            .expect("validated active graph")
            .document = document;
    }

    fn response(&self) -> GraphCatalogResponse {
        GraphCatalogResponse {
            protocol_version: PROTOCOL_VERSION,
            catalog_revision: self.revision,
            active_id: self.active_id.clone(),
            graphs: self
                .graphs
                .iter()
                .map(|graph| GraphCatalogEntry {
                    id: graph.id.clone(),
                    name: graph.name.clone(),
                })
                .collect(),
            document: self.active().document.clone(),
            layout_revision: self.active().layout_revision,
            layout: self.active().layout.clone(),
            persistence_error: None,
        }
    }
}

impl EngineGraphController {
    /// Loads the durable catalog before the audio worker starts.
    /// Startup recall uses the normal native preparation transaction. Native failures retain
    /// the saved documents for automatic retry; invalid files remain protected from writes.
    #[must_use]
    pub fn with_graph_catalog(self) -> Self {
        match config_directory() {
            Ok(directory) => self.with_graph_catalog_path(directory.join("graphs.json")),
            Err(error) => {
                *self.catalog.lock() = Some(CatalogStore {
                    path: PathBuf::new(),
                    persistent: false,
                    value: Err(error.to_string()),
                });
                self
            }
        }
    }

    pub(super) fn with_graph_catalog_path(mut self, path: PathBuf) -> Self {
        // A paused engine restores its graph without claiming audio.
        self.restore_saved_pause(&path);
        let loaded = Catalog::load(&path);
        let protected = match &loaded {
            Ok(Some(catalog)) => self
                .validate_workspace_feature_usage(catalog)
                .is_err_and(|error| error.code == ErrorCode::FeatureLimit),
            Err(_) => true,
            Ok(None) => false,
        };
        if !self.is_paid() && protected {
            self.processor_controls
                .configure_state_store(path.with_extension("states"));
            *self.catalog.lock() = Some(CatalogStore {
                path,
                persistent: false,
                value: Ok(Catalog::imported(self.graph())),
            });
            self.refresh_control_snapshot();
            return self;
        }
        self.processor_controls
            .configure_state_store(path.with_extension("states"));
        match loaded {
            Ok(None) => {
                // Import the already prepared graph once. Never initialize an existing file.
                let value = Catalog::imported(self.graph());
                let saved =
                    stage_json_limited(&path, &value, MAX_BYTES).and_then(StagedWrite::commit);
                *self.catalog.lock() = Some(CatalogStore {
                    path,
                    persistent: true,
                    value: saved.map(|()| value).map_err(|error| error.to_string()),
                });
            }
            Ok(Some(value)) => {
                let document = value.active().document.clone();
                *self.catalog.lock() = Some(CatalogStore {
                    path,
                    persistent: true,
                    value: Ok(value),
                });
                // The editable document is durable user data, not the bootstrap audio
                // processor. Failed native startup must never substitute that processor's graph.
                *self.graph.write() = document;
                if let Err(error) = self.restore_catalog_locked() {
                    self.defer_catalog_restore(error);
                }
            }
            Err(error) => {
                *self.catalog.lock() = Some(CatalogStore {
                    path,
                    persistent: true,
                    value: Err(error),
                });
            }
        }
        self.refresh_control_snapshot();
        self
    }

    pub(super) fn catalog_candidate(&self, document: &GraphDocument) -> Option<Catalog> {
        let catalog = self.catalog.lock();
        let store = catalog.as_ref()?;
        // A corrupt catalog is read-only, not an engine-wide editing lock.
        let Ok(value) = store.value.as_ref() else {
            return None;
        };
        let mut value = value.clone();
        value.update_document(document.clone());
        Some(value)
    }

    pub(super) fn stage_catalog(
        &self,
        candidate: Option<&Catalog>,
    ) -> Result<Option<StagedWrite>, ProtocolError> {
        if self
            .catalog
            .lock()
            .as_ref()
            .is_some_and(|store| !store.persistent)
        {
            return Ok(None);
        }
        let Some(candidate) = candidate else {
            return Ok(None);
        };
        if self.validate_workspace_export_usage(candidate).is_err() {
            return Ok(None);
        }
        let references = state_assets::referenced(candidate)?;
        let (path, accepted_references) = {
            let catalog = self.catalog.lock();
            let store = catalog
                .as_ref()
                .ok_or_else(|| unavailable("graph catalog not enabled"))?;
            let accepted_references = store
                .value
                .as_ref()
                .ok()
                .map(state_assets::referenced)
                .transpose()?
                .unwrap_or_default();
            (store.path.clone(), accepted_references)
        };
        let mut verified = self.verified_catalog_state_refs.lock();
        let next_verified = if references.is_empty() {
            std::collections::BTreeSet::new()
        } else {
            let states = self
                .processor_controls
                .state_store()
                .map_err(|error| unavailable(&error))?;
            state_assets::verify_catalog_delta(
                &references,
                &accepted_references,
                &verified,
                |reference| {
                    states
                        .verify(reference)
                        .map_err(|error| invalid(&error.to_string()))
                },
            )?
        };
        let staged = stage_json_limited(&path, candidate, MAX_BYTES)
            .map_err(|error| document_write_error(&error))?;
        *verified = next_verified;
        Ok(Some(staged))
    }

    pub(super) fn commit_catalog_memory(&self, candidate: Option<Catalog>) {
        self.replace_catalog_memory(candidate);
        self.refresh_control_snapshot();
    }

    /// Updates authored selection before a prepared plan can become active.
    pub(super) fn replace_catalog_memory(&self, candidate: Option<Catalog>) {
        if let Some(candidate) = candidate
            && let Some(store) = self.catalog.lock().as_mut()
        {
            store.value = Ok(candidate);
        }
    }

    pub(super) fn finish_catalog_write(&self, staged: Option<StagedWrite>) {
        if let Some(staged) = staged {
            // Native preparation may already own pending resources. Disk replacement failure
            // must not reject publication and leave those resources without their processor.
            *self.catalog_write_error.lock() = staged.commit().err().map(|error| error.to_string());
        }
    }

    /// Called with the controller mutation lock after a parameter was accepted natively.
    /// Disk failures are durability warnings, not failed audio edits. Keep memory current so
    /// a later successful edit also persists every previously accepted parameter ID.
    pub(super) fn persist_parameter_document(&self, document: &GraphDocument) -> Option<String> {
        let candidate = {
            let catalog = self.catalog.lock();
            let store = catalog.as_ref()?;
            match &store.value {
                Ok(value) => {
                    let mut candidate = value.clone();
                    candidate.update_document(document.clone());
                    candidate
                }
                Err(error) => {
                    let error = format!("graph catalog is unavailable: {error}");
                    *self.catalog_write_error.lock() = Some(error.clone());
                    return Some(error);
                }
            }
        };
        let result = self
            .stage_catalog(Some(&candidate))
            .map_err(|error| {
                error
                    .parameters
                    .get("reason")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("graph catalog staging failed")
                    .to_owned()
            })
            .and_then(|staged| {
                staged.map_or(Ok(()), |write| {
                    write.commit().map_err(|error| error.to_string())
                })
            });
        self.commit_catalog_memory(Some(candidate));
        let error = result.err();
        self.catalog_write_error.lock().clone_from(&error);
        error
    }

    pub(super) fn read_catalog(&self) -> Result<GraphCatalogResponse, ProtocolError> {
        // Read one committed document/layout snapshot; native preparation owns mutation,
        // but must never prevent the UI from displaying durable user data.
        let catalog = self.catalog.lock();
        let mut response = catalog
            .as_ref()
            .ok_or_else(|| unavailable("graph catalog not enabled"))?
            .value
            .as_ref()
            .map(Catalog::response)
            .map_err(|error| unavailable(error))?;
        response.persistence_error = self.graph_persistence_warning();
        Ok(response)
    }

    pub(super) fn graph_persistence_warning(&self) -> Option<String> {
        self.catalog_write_error
            .lock()
            .clone()
            .or_else(|| self.pause_save_warning.lock().clone())
            .or_else(|| self.node_control_warning.lock().clone())
    }

    pub(super) fn read_catalog_document(&self, id: &str) -> Result<GraphDocument, ProtocolError> {
        let catalog = self.catalog.lock();
        catalog
            .as_ref()
            .ok_or_else(|| unavailable("graph catalog not enabled"))?
            .value
            .as_ref()
            .map_err(|error| unavailable(error))?
            .graphs
            .iter()
            .find(|graph| graph.id == id)
            .map(|graph| graph.document.clone())
            .ok_or_else(|| invalid("graph does not exist"))
    }

    // Caller holds mutation across document validation and its control operation.
    pub(super) fn catalog_document_locked(&self, id: &str) -> Result<GraphDocument, ProtocolError> {
        let catalog = self.catalog.lock();
        let value = catalog
            .as_ref()
            .ok_or_else(|| unavailable("graph catalog not enabled"))?
            .value
            .as_ref()
            .map_err(|error| unavailable(error))?;
        if id == value.active_id {
            return Ok(self.graph.read().clone());
        }
        value
            .graphs
            .iter()
            .find(|graph| graph.id == id)
            .map(|graph| graph.document.clone())
            .ok_or_else(|| invalid("graph does not exist"))
    }

    pub(super) fn mutate_catalog(
        &self,
        request: GraphCatalogRequest,
    ) -> Result<GraphCatalogResponse, ProtocolError> {
        let _mutation = self.mutation.lock();
        let mut value = self
            .catalog
            .lock()
            .as_ref()
            .ok_or_else(|| unavailable("graph catalog not enabled"))?
            .value
            .as_ref()
            .map_err(|error| unavailable(error))?
            .clone();
        let revisions = self.exchange.revisions();
        if request.expected_revision != revisions.submitted
            || revisions.active != revisions.submitted
            || request.expected_catalog_revision != value.revision
        {
            return Err(ProtocolError {
                code: ErrorCode::GraphRevisionStale,
                parameters: serde_json::json!({"submittedRevision": revisions.submitted, "catalogRevision": value.revision}),
            });
        }
        if matches!(&request.action, GraphCatalogAction::Select { id } if *id == value.active_id) {
            return self.current_catalog_response();
        }
        let next_catalog_revision = value
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("catalog revision overflow"))?;
        value.update_document(self.graph.read().clone());
        let removes_execution = matches!(request.action, GraphCatalogAction::Delete { .. });
        value.apply_action(request.action, revisions.submitted, next_catalog_revision)?;
        value.revision = next_catalog_revision;
        if removes_execution {
            return self.publish_catalog_removal(value, revisions.submitted);
        }
        let mut document = value.active_document().clone();
        document.revision = revisions.submitted;
        value.update_document(document.clone());
        self.validate_feature_usage(&document, Some(&value))?;
        if let Some(staged) = self.stage_catalog(Some(&value))? {
            staged
                .commit()
                .map_err(|error| unavailable(&error.to_string()))?;
        }
        *self.graph.write() = document;
        *self.catalog_write_error.lock() = None;
        self.commit_catalog_memory(Some(value.clone()));
        let mut response = value.response();
        response.persistence_error = self.graph_persistence_warning();
        Ok(response)
    }

    fn current_catalog_response(&self) -> Result<GraphCatalogResponse, ProtocolError> {
        let catalog = self.catalog.lock();
        let mut response = catalog
            .as_ref()
            .expect("catalog enabled")
            .value
            .as_ref()
            .map_err(|error| unavailable(error))?
            .response();
        response.document = self.graph.read().clone();
        response.persistence_error = self.graph_persistence_warning();
        Ok(response)
    }

    fn publish_catalog_removal(
        &self,
        mut candidate: Catalog,
        revision: u64,
    ) -> Result<GraphCatalogResponse, ProtocolError> {
        let reuse = self.processor_controls.reuse_transaction();
        let mut document = candidate.active_document().clone();
        document.revision = revision
            .checked_add(1)
            .ok_or_else(|| invalid("graph revision overflow"))?;
        if candidate.selected_id() == self.view_graph_id() {
            self.preserve_live_processor_state(&mut document, &[])?;
        }
        candidate.update_document(document);
        self.validate_feature_usage(candidate.active_document(), Some(&candidate))?;
        self.reconcile_catalog_devices(&mut candidate)?;
        // The selected identity may have changed. Preserve providers by catalog owner,
        // not by comparing graph-local node IDs against the previously selected graph.
        self.preserve_catalog_provider_state(&mut candidate)?;
        self.publish_graph_locked(candidate.active_document().clone(), Some(candidate))?;
        reuse.commit();
        self.current_catalog_response()
    }
}

impl Catalog {
    fn apply_action(
        &mut self,
        action: GraphCatalogAction,
        next_revision: u64,
        next_catalog_revision: u64,
    ) -> Result<bool, ProtocolError> {
        let mut switch = true;
        match action {
            GraphCatalogAction::Create { name } => {
                if !valid_name(&name) || self.graphs.len() >= MAX_GRAPHS {
                    return Err(invalid("invalid graph name or catalog full"));
                }
                let id = format!("graph-{next_catalog_revision}");
                if self.graphs.iter().any(|graph| graph.id == id) {
                    return Err(invalid("graph identity collision"));
                }
                self.graphs.push(SavedGraph {
                    id: id.clone(),
                    name: name.trim().into(),
                    document: GraphDocument::new(next_revision, vec![], vec![]),
                    layout_revision: 0,
                    layout: GraphLayout::default(),
                });
                self.active_id = id;
            }
            GraphCatalogAction::Select { id } => {
                if !self.graphs.iter().any(|graph| graph.id == id) {
                    return Err(invalid("graph does not exist"));
                }
                self.active_id = id;
            }
            GraphCatalogAction::Rename { id, name } => {
                if !valid_name(&name) {
                    return Err(invalid("invalid graph name"));
                }
                let graph = self
                    .graphs
                    .iter_mut()
                    .find(|graph| graph.id == id)
                    .ok_or_else(|| invalid("graph does not exist"))?;
                graph.name = name.trim().into();
                switch = false;
            }
            GraphCatalogAction::Delete { id } => {
                self.remove_graph(&id, next_revision, next_catalog_revision)?;
            }
        }
        Ok(switch)
    }

    fn remove_graph(
        &mut self,
        id: &str,
        revision: u64,
        catalog_revision: u64,
    ) -> Result<(), ProtocolError> {
        let index = self
            .graphs
            .iter()
            .position(|graph| graph.id == id)
            .ok_or_else(|| invalid("graph does not exist"))?;
        self.graphs.remove(index);
        if self.graphs.is_empty() {
            self.apply_action(
                GraphCatalogAction::Create {
                    name: "Untitled Graph".into(),
                },
                revision,
                catalog_revision,
            )?;
        } else if self.active_id == id {
            self.active_id
                .clone_from(&self.graphs[index.min(self.graphs.len() - 1)].id);
        }
        Ok(())
    }
}
