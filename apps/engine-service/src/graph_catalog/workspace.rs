//! Versioned user files share authored catalog data, never native runtime handles.
use std::{
    fs::File,
    io::Read,
    path::Path,
    time::{Duration, Instant},
};

use mixrelay_control_protocol::{GraphCatalogResponse, WorkspaceAction, WorkspaceRequest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    Catalog, EngineGraphController, GraphDocument, MAX_BYTES, NodeConfig, ProtocolError, invalid,
    restore, unavailable,
};
use crate::persistence::stage_json;
mod preview;
mod rates;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WorkspaceFile {
    format: String,
    version: u32,
    catalog: Catalog,
}

fn file_path(path: &str) -> Result<&Path, ProtocolError> {
    let path = Path::new(path);
    if !path.is_absolute()
        || !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("mix"))
    {
        return Err(invalid("workspace path must be an absolute .mix filename"));
    }
    Ok(path)
}

#[cfg(test)]
fn load(path: &Path) -> Result<Catalog, ProtocolError> {
    load_fingerprinted(path).map(|(catalog, _)| catalog)
}

fn load_fingerprinted(path: &Path) -> Result<(Catalog, String), ProtocolError> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(MAX_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|error| unavailable(&error.to_string()))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid("workspace exceeds size limit"));
    }
    let mut file: WorkspaceFile =
        serde_json::from_slice(&bytes).map_err(|_| invalid("invalid MixRelay workspace file"))?;
    if file.format != "MixRelay" || file.version != 1 {
        return Err(invalid("unsupported MixRelay workspace format or version"));
    }
    file.catalog.validate().map_err(|error| invalid(&error))?;
    file.catalog.device_specs()?;
    for (_, document) in file.catalog.documents_mut() {
        restore::require_pid_reselection(document);
    }
    Ok((file.catalog, format!("{:x}", Sha256::digest(&bytes))))
}

impl EngineGraphController {
    pub(crate) fn workspace_operation(
        &self,
        request: WorkspaceRequest,
    ) -> Result<GraphCatalogResponse, ProtocolError> {
        let _mutation = self.mutation.lock();
        let revisions = self.exchange.revisions();
        let current = self
            .catalog_candidate(&self.graph.read())
            .ok_or_else(|| unavailable("graph catalog is unavailable"))?;
        if revisions.active != revisions.submitted {
            return Err(ProtocolError {
                code: super::ErrorCode::GraphExchangeBusy,
                parameters: serde_json::json!({}),
            });
        }
        if request.expected_revision != revisions.submitted
            || request.expected_catalog_revision != current.revision
        {
            return Err(ProtocolError {
                code: super::ErrorCode::GraphRevisionStale,
                parameters: serde_json::json!({}),
            });
        }
        match request.operation {
            WorkspaceAction::Save { path } => self.save_workspace(file_path(&path)?, current)?,
            WorkspaceAction::Open { path } => {
                let (candidate, digest) = load_fingerprinted(file_path(&path)?)?;
                if request
                    .file_digest
                    .is_some_and(|expected| expected != digest)
                {
                    return Err(invalid("workspace file changed after capture preparation"));
                }
                self.replace_workspace(candidate, &current)?;
            }
            WorkspaceAction::New => self.replace_workspace(
                Catalog::imported(GraphDocument::new(0, vec![], vec![])),
                &current,
            )?,
        }
        self.read_catalog()
    }

    fn save_workspace(&self, path: &Path, mut candidate: Catalog) -> Result<(), ProtocolError> {
        let deadline = Instant::now() + Duration::from_secs(5);
        for (id, document) in candidate.documents_mut() {
            let previous = document.clone();
            self.capture_background_graph(
                (id, document),
                &previous,
                deadline,
                &mut |node, timeout| self.processor_controls.snapshot_state(node, timeout),
            )?;
            for node in &mut document.nodes {
                if let NodeConfig::Provider(provider) = &mut node.config {
                    provider.ports = self
                        .processor_catalog
                        .ports(provider.type_id.as_str())
                        .or(provider.ports);
                }
            }
        }
        let file = WorkspaceFile {
            format: "MixRelay".into(),
            version: 1,
            catalog: candidate,
        };
        if serde_json::to_vec(&file)
            .map_err(|error| unavailable(&error.to_string()))?
            .len() as u64
            > MAX_BYTES
        {
            return Err(invalid("workspace exceeds size limit"));
        }
        stage_json(path, &file)
            .and_then(crate::persistence::StagedWrite::commit)
            .map_err(|error| unavailable(&error.to_string()))
    }

    fn replace_workspace(
        &self,
        mut candidate: Catalog,
        current: &Catalog,
    ) -> Result<(), ProtocolError> {
        self.require_stopped_recorders(current)?;
        let revision = candidate
            .next_audio_revision(self.exchange.revisions().submitted)
            .ok_or_else(|| invalid("audio revision overflow"))?;
        candidate.revision = current
            .revision
            .max(candidate.revision)
            .checked_add(1)
            .ok_or_else(|| invalid("catalog revision overflow"))?;
        let layout_revision = current
            .graphs
            .iter()
            .chain(&candidate.graphs)
            .map(|graph| graph.layout_revision)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(|| invalid("layout revision overflow"))?;
        for graph in &mut candidate.graphs {
            graph.document.revision = revision;
            graph.layout_revision = layout_revision;
        }
        // A file replaces authored state. Never merge old device definitions or live presets.
        self.processor_controls.invalidate_reuse();
        self.publish_workspace_rates(candidate)?;
        Ok(())
    }

    fn require_stopped_recorders(&self, current: &Catalog) -> Result<(), ProtocolError> {
        for (graph_id, graph) in current.documents() {
            for node in &graph.nodes {
                let NodeConfig::Recorder { channels } = node.config else {
                    continue;
                };
                let id = crate::graph_composition::runtime_node_id(graph_id, &node.id);
                let key = mixrelay_audio_engine::recording_session_key(&id, channels);
                if matches!(
                    self.recorders.status(&key).state,
                    mixrelay_audio_recorder::RecorderState::Recording
                        | mixrelay_audio_recorder::RecorderState::Paused
                ) {
                    return Err(invalid("stop recording before replacing the workspace"));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
