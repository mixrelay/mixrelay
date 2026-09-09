use tonenari_audio_graph::NodeConfig;
use tonenari_control_protocol::{ErrorCode, ProtocolError};

use super::{EngineGraphController, graph_composition};

pub(super) fn recorder_error(reason: String) -> ProtocolError {
    ProtocolError {
        code: ErrorCode::RecorderFailed,
        parameters: serde_json::json!({"reason": serde_json::Value::String(reason)}),
    }
}

impl EngineGraphController {
    pub(super) fn reclaim_recorders(&self) {
        self.exchange.reclaim_retired();
        if let Err(reason) = self.recorders.reap_unused() {
            tracing::error!(reason, "recorder finalization failed");
            *self.recorder_cleanup_error.lock() = Some(reason);
        }
    }
    pub(super) fn recorder_session_id(
        &self,
        graph_id: &str,
        node_id: &str,
    ) -> Result<String, ProtocolError> {
        let document = self.catalog_document_locked(graph_id)?;
        let (id, channels) = document
            .nodes
            .iter()
            .find(|node| node.id.as_str() == node_id)
            .and_then(|node| match node.config {
                NodeConfig::Recorder { channels } => Some((&node.id, channels)),
                _ => None,
            })
            .ok_or_else(|| recorder_error("recorder node not found".to_owned()))?;
        Ok(tonenari_audio_engine::recording_session_key(
            &graph_composition::runtime_node_id(graph_id, id),
            channels,
        ))
    }

    pub(super) fn validate_recorder_channel_change(
        &self,
        proposed: &tonenari_audio_graph::GraphDocument,
    ) -> Result<(), ProtocolError> {
        let graph_id = self.view_graph_id();
        for previous in &self.graph.read().nodes {
            let NodeConfig::Recorder { channels } = previous.config else {
                continue;
            };
            let changed = proposed.nodes.iter().any(|node| {
                node.id == previous.id
                    && matches!(node.config, NodeConfig::Recorder { channels: next } if next != channels)
            });
            if !changed {
                continue;
            }
            let id = tonenari_audio_engine::recording_session_key(
                &graph_composition::runtime_node_id(&graph_id, &previous.id),
                channels,
            );
            if matches!(
                self.recorders.status(&id).state,
                tonenari_audio_recorder::RecorderState::Recording
                    | tonenari_audio_recorder::RecorderState::Paused
            ) {
                return Err(recorder_error(
                    "stop recording before changing channels".into(),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonenari_audio_graph::{GraphDocument, NodeId, NodeSpec};
    use tonenari_audio_recorder::{RecorderAction, RecorderRequest, RecorderState};
    use tonenari_control_protocol::{GraphCatalogAction, GraphCatalogRequest};
    use tonenari_control_server::ControlBackend;

    fn command(action: RecorderAction, path: Option<String>) -> RecorderRequest {
        RecorderRequest {
            action,
            path,
            sample_rate: None,
            encoding: None,
        }
    }

    #[test]
    fn channels_can_change_only_after_the_take_is_stopped() {
        let directory = tonenari_test_support::FixtureDir::new("recorder-channel-control").unwrap();
        let document = GraphDocument::new(
            1,
            vec![NodeSpec {
                id: NodeId::from("record"),
                label: None,
                config: NodeConfig::Recorder { channels: 2 },
            }],
            vec![],
        );
        let engine = crate::endpoint_settings_tests::controller(document)
            .with_graph_catalog_path(directory.join("graphs.json"));
        let catalog = engine.graph_catalog().unwrap();
        let graph_id = catalog.active_id;
        let id = engine.recorder_session_id(&graph_id, "record").unwrap();
        let tap = engine.recorders.prepare(&id, 2).unwrap();
        tap.capture(&[0.0; 4], 48_000);
        engine
            .control_recorder(
                &graph_id,
                "record",
                command(
                    RecorderAction::Record,
                    Some(directory.join("take.wav").to_string_lossy().into_owned()),
                ),
            )
            .unwrap();
        let mut changed = catalog.document;
        changed.revision += 1;
        changed.nodes[0].config = NodeConfig::Recorder { channels: 8 };
        assert_eq!(
            engine.apply_graph(changed.clone()).unwrap_err().code,
            ErrorCode::RecorderFailed
        );
        engine
            .control_recorder(&graph_id, "record", command(RecorderAction::Pause, None))
            .unwrap();
        assert_eq!(
            engine.apply_graph(changed.clone()).unwrap_err().code,
            ErrorCode::RecorderFailed
        );
        engine
            .control_recorder(&graph_id, "record", command(RecorderAction::Stop, None))
            .unwrap();
        assert!(engine.apply_graph(changed).is_ok());
    }

    #[test]
    fn controls_target_original_graph_even_after_another_graph_is_selected() {
        let directory = tonenari_test_support::FixtureDir::new("recorder-control").unwrap();
        let document = GraphDocument::new(
            1,
            vec![NodeSpec {
                id: NodeId::from("record"),
                label: None,
                config: NodeConfig::Recorder { channels: 2 },
            }],
            vec![],
        );
        let engine = crate::endpoint_settings_tests::controller(document)
            .with_graph_catalog_path(directory.join("graphs.json"));
        let original = engine.graph_catalog().unwrap();
        let id = tonenari_audio_engine::recording_session_key(
            &graph_composition::runtime_node_id(&original.active_id, &NodeId::from("record")),
            2,
        );
        let tap = engine.recorders.prepare(id.as_str(), 2).unwrap();
        tap.capture(&[0.0; 4], 48_000);
        let path = directory.join("take.wav").to_string_lossy().into_owned();
        engine
            .control_recorder(
                &original.active_id,
                "record",
                command(RecorderAction::Record, Some(path)),
            )
            .unwrap();
        let second = engine
            .mutate_graph_catalog(GraphCatalogRequest {
                expected_revision: original.document.revision,
                expected_catalog_revision: original.catalog_revision,
                action: GraphCatalogAction::Create {
                    name: "Another graph".to_owned(),
                },
            })
            .unwrap();
        assert_ne!(original.active_id, second.active_id);
        assert_eq!(
            engine
                .recorder_status(&original.active_id, "record")
                .unwrap()
                .state,
            RecorderState::Recording
        );
        assert!(engine.recorder_status(&second.active_id, "record").is_err());
        assert!(
            engine
                .control_recorder(
                    &original.active_id,
                    "missing",
                    command(RecorderAction::Stop, None)
                )
                .is_err()
        );
        tap.capture(&[0.25; 4], 48_000);
        let stopped = engine
            .control_recorder(
                &original.active_id,
                "record",
                command(RecorderAction::Stop, None),
            )
            .unwrap();
        assert_eq!(stopped.state, RecorderState::Stopped);
        assert_eq!(stopped.frames, 2);
    }
}
