//! Immutable authored control views, published only at committed writer boundaries.
//! Readers never acquire the graph mutation lock or perform native preparation.

use mixrelay_audio_graph::{GraphDocument, NodeConfig, NodeSpec};
use mixrelay_control_protocol::{
    EngineControlSnapshot, GraphControlSnapshot, GraphNodeControls, NodeControlCapabilities,
    NodeControlDescriptor, NodeControlSnapshot, ProtocolError,
};

use crate::{EngineGraphController, graph_catalog::unavailable, node_control};

#[derive(Clone)]
pub(super) struct ControlReadView {
    snapshot: EngineControlSnapshot,
    recorder_ids: std::collections::BTreeMap<(String, String), String>,
}

impl EngineGraphController {
    pub(super) fn snapshot_recorder_status(
        &self,
        graph_id: &str,
        node_id: &str,
    ) -> Result<mixrelay_control_protocol::RecorderStatus, ProtocolError> {
        let id = self
            .control_snapshot
            .read()
            .as_ref()
            .and_then(|view| {
                view.recorder_ids
                    .get(&(graph_id.to_owned(), node_id.to_owned()))
                    .cloned()
            })
            .ok_or_else(|| {
                node_control::node_error(
                    mixrelay_control_protocol::ErrorCode::NodeNotFound,
                    graph_id,
                    node_id,
                )
            })?;
        Ok(self.recorders.status(&id))
    }

    pub(super) fn refresh_control_snapshot(&self) {
        let graph = self.graph.read();
        let catalog = self.catalog.lock();
        let documents = catalog
            .as_ref()
            .and_then(|store| store.value.as_ref().ok())
            .map_or_else(
                || vec![("session", "Untitled Graph", &*graph)],
                |value| value.control_documents().collect(),
            );
        let graphs = documents
            .iter()
            .map(|(id, name, document)| graph_snapshot(id, name, document))
            .collect();
        let recorder_ids = documents
            .iter()
            .flat_map(|(id, _, document)| {
                document.nodes.iter().filter_map(move |node| {
                    let NodeConfig::Recorder { channels } = node.config else {
                        return None;
                    };
                    Some((
                        ((*id).to_owned(), node.id.as_str().to_owned()),
                        mixrelay_audio_engine::recording_session_key(
                            &crate::graph_composition::runtime_node_id(id, &node.id),
                            channels,
                        ),
                    ))
                })
            })
            .collect();
        let snapshot = EngineControlSnapshot {
            revision: graph.revision,
            active_revision: self.exchange.revisions().active,
            structure_generation: self.exchange.plan_revisions().submitted,
            engine_power: self.engine_power_status(),
            graphs,
        };
        drop(catalog);
        drop(graph);
        *self.control_snapshot.write() = Some(ControlReadView {
            snapshot,
            recorder_ids,
        });
    }

    pub(super) fn snapshot_controls(&self) -> Result<EngineControlSnapshot, ProtocolError> {
        let ControlReadView {
            mut snapshot,
            recorder_ids,
        } = self
            .control_snapshot
            .read()
            .clone()
            .ok_or_else(|| unavailable("control snapshot is not initialized"))?;
        snapshot.active_revision = self.exchange.revisions().active.min(snapshot.revision);
        snapshot.engine_power = self.engine_power_status();
        for graph in &mut snapshot.graphs {
            for node in &mut graph.nodes {
                if node.descriptor.capabilities.recorder {
                    // Registry identity is stable across plan swaps. Missing/removed sessions
                    // are represented by their stopped status, never a native lookup.
                    if let Some(id) =
                        recorder_ids.get(&(graph.id.clone(), node.descriptor.node_id.clone()))
                    {
                        node.recorder = Some(self.recorders.status(id));
                    }
                }
            }
        }
        Ok(snapshot)
    }

    pub(super) fn read_snapshot_node_controls(
        &self,
        graph_id: &str,
    ) -> Result<GraphNodeControls, ProtocolError> {
        let snapshot = self.snapshot_controls()?;
        let graph = snapshot
            .graphs
            .into_iter()
            .find(|graph| graph.id == graph_id)
            .ok_or_else(|| crate::graph_catalog::invalid("graph does not exist"))?;
        Ok(GraphNodeControls {
            graph_id: graph.id,
            revision: snapshot.revision,
            nodes: graph
                .nodes
                .into_iter()
                .map(|node| node.descriptor)
                .collect(),
        })
    }
}

fn graph_snapshot(id: &str, name: &str, graph: &GraphDocument) -> GraphControlSnapshot {
    GraphControlSnapshot {
        id: id.to_owned(),
        name: name.to_owned(),
        nodes: graph
            .nodes
            .iter()
            .filter(|node| node_control::controllable(node))
            .map(|node| NodeControlSnapshot {
                descriptor: descriptor(node),
                recorder: None,
            })
            .collect(),
    }
}

fn descriptor(node: &NodeSpec) -> NodeControlDescriptor {
    NodeControlDescriptor {
        node_id: node.id.as_str().to_owned(),
        label: node.label.clone(),
        kind: serde_json::to_value(node.config.kind())
            .expect("unit node kind serializes to JSON")
            .as_str()
            .expect("unit node kind is a JSON string")
            .to_owned(),
        control: node.control,
        gain_db: node_control::node_gain(&node.config),
        capabilities: NodeControlCapabilities {
            power: true,
            mute: true,
            recorder: matches!(node.config, NodeConfig::Recorder { .. }),
        },
    }
}
