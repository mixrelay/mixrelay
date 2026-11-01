use mixrelay_audio_graph::NodeConfig;
use mixrelay_control_protocol::{ErrorCode, ProtocolError};

use super::{EngineGraphController, graph_composition, recorder_sessions::recorder_error};

impl EngineGraphController {
    pub(super) fn validate_free_record_start(
        &self,
        graph_id: &str,
        node_id: &str,
        id: &str,
    ) -> Result<(), ProtocolError> {
        use mixrelay_audio_recorder::RecorderState;
        if self.recorders.status(id).state == RecorderState::Paused {
            return Ok(());
        }
        let document = self.catalog_document_locked(graph_id)?;
        if let Some(channels) = document
            .nodes
            .iter()
            .find(|node| node.id.as_str() == node_id)
            .and_then(|node| match node.config {
                NodeConfig::Recorder { channels } => Some(channels),
                _ => None,
            })
            && channels > 2
        {
            return Err(crate::feature_policy::limit(
                "recordingChannels",
                2,
                usize::from(channels),
            ));
        }
        let current = self.graph.read().clone();
        let catalog = self.catalog_candidate(&current);
        let graphs: Vec<_> = catalog.as_ref().map_or_else(
            || vec![(self.view_graph_id(), &current)],
            |catalog| {
                catalog
                    .documents()
                    .map(|(id, graph)| (id.to_owned(), graph))
                    .collect()
            },
        );
        for (graph_id, graph) in graphs {
            for node in &graph.nodes {
                let NodeConfig::Recorder { channels } = node.config else {
                    continue;
                };
                let key = mixrelay_audio_engine::recording_session_key(
                    &graph_composition::runtime_node_id(&graph_id, &node.id),
                    channels,
                );
                if key != id
                    && matches!(
                        self.recorders.status(&key).state,
                        RecorderState::Recording | RecorderState::Paused
                    )
                {
                    return Err(crate::feature_policy::limit("recorders", 1, 2));
                }
            }
        }
        Ok(())
    }

    pub(super) fn stop_inactive_recorders(
        &self,
        proposed: &mixrelay_audio_graph::GraphDocument,
        catalog: Option<&crate::graph_catalog::Catalog>,
    ) {
        let current = self.graph.read().clone();
        let before = self.catalog_candidate(&current);
        let documents: Vec<_> = before.as_ref().map_or_else(
            || vec![(self.view_graph_id(), &current)],
            |value| {
                value
                    .documents()
                    .map(|(id, graph)| (id.to_owned(), graph))
                    .collect()
            },
        );
        for (graph_id, document) in documents {
            let next = catalog.map_or(Some(proposed), |value| {
                value
                    .documents()
                    .find(|(id, _)| *id == graph_id)
                    .map(|(_, graph)| graph)
            });
            for node in &document.nodes {
                let NodeConfig::Recorder { channels } = node.config else {
                    continue;
                };
                if !node.control.enabled || next.is_some_and(|graph| graph.nodes.iter().any(|next| {
                    next.id == node.id && next.control.enabled
                        && matches!(next.config, NodeConfig::Recorder { channels: width } if channels == width)
                })) { continue; }
                let id = mixrelay_audio_engine::recording_session_key(
                    &graph_composition::runtime_node_id(&graph_id, &node.id),
                    channels,
                );
                if !matches!(
                    self.recorders.status(&id).state,
                    mixrelay_audio_recorder::RecorderState::Recording
                        | mixrelay_audio_recorder::RecorderState::Paused
                ) {
                    continue;
                }
                self.recorders.stop_inactive(&id, &graph_id, &node.id);
            }
        }
    }

    pub(super) fn reclaim_recorders(&self) {
        self.exchange.reclaim_retired();
        self.recorders.reclaim();
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
            .ok_or_else(|| {
                crate::node_control::node_error(ErrorCode::NodeNotFound, graph_id, node_id)
            })?;
        Ok(mixrelay_audio_engine::recording_session_key(
            &graph_composition::runtime_node_id(graph_id, id),
            channels,
        ))
    }

    pub(super) fn validate_recorder_channel_change(
        &self,
        proposed: &mixrelay_audio_graph::GraphDocument,
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
            let id = mixrelay_audio_engine::recording_session_key(
                &graph_composition::runtime_node_id(&graph_id, &previous.id),
                channels,
            );
            if matches!(
                self.recorders.status(&id).state,
                mixrelay_audio_recorder::RecorderState::Recording
                    | mixrelay_audio_recorder::RecorderState::Paused
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
    use mixrelay_audio_graph::{GraphDocument, NodeId, NodeSpec};
    use mixrelay_audio_recorder::{RecorderAction, RecorderRequest, RecorderState};
    use mixrelay_control_protocol::{
        GraphCatalogAction, GraphCatalogRequest, RecorderControlRequest,
    };
    use mixrelay_control_server::ControlBackend;

    fn command(
        revision: u64,
        action: RecorderAction,
        path: Option<String>,
    ) -> RecorderControlRequest {
        RecorderControlRequest {
            expected_recording: None,
            expected_revision: revision,
            request: RecorderRequest {
                action,
                path,
                sample_rate: None,
                encoding: None,
            },
        }
    }

    #[test]
    fn channels_can_change_only_after_the_take_is_stopped() {
        let directory = mixrelay_test_support::FixtureDir::new("recorder-channel-control").unwrap();
        let document = GraphDocument::new(
            1,
            vec![NodeSpec {
                control: mixrelay_audio_graph::NodeControl::default(),
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
        let tap = engine.recorders.registry().prepare(&id, 2).unwrap();
        tap.capture(&[0.0; 4], 48_000);
        engine
            .control_recorder(
                &graph_id,
                "record",
                command(
                    catalog.document.revision,
                    RecorderAction::Record,
                    Some(directory.join("take.wav").to_string_lossy().into_owned()),
                ),
            )
            .unwrap();
        let mut changed = catalog.document;
        changed.revision += 1;
        changed.nodes[0].config = NodeConfig::Recorder { channels: 8 };
        assert_eq!(
            engine
                .apply_graph_request(changed.clone().into())
                .unwrap_err()
                .code,
            ErrorCode::RecorderFailed
        );
        engine
            .control_recorder(&graph_id, "record", command(1, RecorderAction::Pause, None))
            .unwrap();
        assert_eq!(
            engine
                .apply_graph_request(changed.clone().into())
                .unwrap_err()
                .code,
            ErrorCode::RecorderFailed
        );
        engine
            .control_recorder(&graph_id, "record", command(1, RecorderAction::Stop, None))
            .unwrap();
        assert!(engine.apply_graph_request(changed.into()).is_ok());
    }

    #[test]
    fn controls_target_original_graph_even_after_another_graph_is_selected() {
        let directory = mixrelay_test_support::FixtureDir::new("recorder-control").unwrap();
        let document = GraphDocument::new(
            1,
            vec![NodeSpec {
                control: mixrelay_audio_graph::NodeControl::default(),
                id: NodeId::from("record"),
                label: None,
                config: NodeConfig::Recorder { channels: 2 },
            }],
            vec![],
        );
        let engine = crate::endpoint_settings_tests::controller(document)
            .with_graph_catalog_path(directory.join("graphs.json"));
        let original = engine.graph_catalog().unwrap();
        let id = mixrelay_audio_engine::recording_session_key(
            &graph_composition::runtime_node_id(&original.active_id, &NodeId::from("record")),
            2,
        );
        let tap = engine.recorders.registry().prepare(id.as_str(), 2).unwrap();
        tap.capture(&[0.0; 4], 48_000);
        let path = directory.join("take.wav").to_string_lossy().into_owned();
        engine
            .control_recorder(
                &original.active_id,
                "record",
                command(
                    original.document.revision,
                    RecorderAction::Record,
                    Some(path),
                ),
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
                    command(second.document.revision, RecorderAction::Stop, None)
                )
                .is_err()
        );
        tap.capture(&[0.25; 4], 48_000);
        let stopped = engine
            .control_recorder(
                &original.active_id,
                "record",
                command(second.document.revision, RecorderAction::Stop, None),
            )
            .unwrap();
        assert_eq!(stopped.state, RecorderState::Stopped);
        assert_eq!(stopped.frames, 2);
    }
}
