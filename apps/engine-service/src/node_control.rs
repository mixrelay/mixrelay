//! Node switches edit the authored catalog and publish through the graph transaction.
//! Native resources follow the composed active references, never a second control registry.

use mixrelay_audio_graph::{GraphDocument, NodeConfig, NodeSpec};
use mixrelay_control_protocol::{
    ApplicationCaptureManifest, ErrorCode, GraphApplyResponse, GraphNodeControls,
    NodeControlCapabilities, NodeControlDescriptor, NodeControlRequest, ProtocolError,
};

use crate::{
    EngineGraphController,
    graph_catalog::{Catalog, invalid, unavailable},
};

impl EngineGraphController {
    /// A controller must never queue stale hardware intent behind native restoration.
    pub(super) fn try_node_control(
        &self,
    ) -> Result<parking_lot::MutexGuard<'_, ()>, ProtocolError> {
        self.mutation.try_lock().ok_or_else(|| ProtocolError {
            code: ErrorCode::GraphExchangeBusy,
            parameters: serde_json::json!({"reason":"graph mutation in progress"}),
        })
    }
    pub(super) fn validate_control_revision(&self, expected: u64) -> Result<(), ProtocolError> {
        let revisions = self.exchange.revisions();
        if expected != revisions.submitted {
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
        Ok(())
    }
    pub(super) fn read_node_controls(
        &self,
        graph_id: &str,
    ) -> Result<GraphNodeControls, ProtocolError> {
        let _mutation = self.try_node_control()?;
        let document = self.catalog_document_locked(graph_id)?;
        let nodes = document
            .nodes
            .iter()
            .filter(|node| controllable(node))
            .map(|node| {
                Ok(NodeControlDescriptor {
                    node_id: node.id.as_str().to_owned(),
                    label: node.label.clone(),
                    kind: serde_json::to_value(node.config.kind())
                        .map_err(|error| invalid(&error.to_string()))?
                        .as_str()
                        .ok_or_else(|| invalid("invalid node kind"))?
                        .to_owned(),
                    control: node.control,
                    gain_db: node_gain(&node.config),
                    capabilities: NodeControlCapabilities {
                        power: true,
                        mute: true,
                        recorder: matches!(node.config, NodeConfig::Recorder { .. }),
                    },
                })
            })
            .collect::<Result<Vec<_>, ProtocolError>>()?;
        Ok(GraphNodeControls {
            graph_id: graph_id.to_owned(),
            revision: self.exchange.revisions().submitted,
            nodes,
        })
    }

    pub(super) fn preview_node_switch(
        &self,
        graph_id: &str,
        node_id: &str,
        request: &NodeControlRequest,
    ) -> Result<ApplicationCaptureManifest, ProtocolError> {
        let _mutation = self.try_node_control()?;
        let candidate = self.node_control_candidate(graph_id, node_id, request)?;
        let composed = self.compose_execution(
            candidate.active_document(),
            Some(&candidate),
            *self.context.read(),
        )?;
        crate::execution_control::capture_manifest(&composed.document)
    }

    pub(super) fn change_node_control(
        &self,
        graph_id: &str,
        node_id: &str,
        request: &NodeControlRequest,
    ) -> Result<GraphApplyResponse, ProtocolError> {
        let _mutation = self.try_node_control()?;
        let mut candidate = self.node_control_candidate(graph_id, node_id, request)?;
        let mut selected = candidate.active_document().clone();
        self.preserve_live_processor_state(&mut selected, &[])?;
        candidate.update_document(selected.clone());
        self.apply_graph_locked(selected, Some(candidate))
    }

    fn node_control_candidate(
        &self,
        graph_id: &str,
        node_id: &str,
        request: &NodeControlRequest,
    ) -> Result<Catalog, ProtocolError> {
        self.validate_control_revision(request.expected_revision)?;
        let revisions = self.exchange.revisions();
        if request.enabled.is_none() && request.muted.is_none() && request.gain_db.is_none() {
            return Err(invalid("node control update is empty"));
        }
        let mut candidate = self
            .catalog_candidate(&self.graph.read())
            .ok_or_else(|| unavailable("graph catalog is unavailable"))?;
        let (_, graph) = candidate
            .documents_mut()
            .find(|(id, _)| *id == graph_id)
            .ok_or_else(|| invalid("graph does not exist"))?;
        let node = graph
            .nodes
            .iter_mut()
            .find(|node| node.id.as_str() == node_id)
            .filter(|node| controllable(node))
            .ok_or_else(|| node_error(ErrorCode::NodeNotFound, graph_id, node_id))?;
        if let Some(enabled) = request.enabled {
            node.control.enabled = enabled;
        }
        if let Some(muted) = request.muted {
            node.control.muted = muted;
        }
        if let Some(gain) = request.gain_db {
            set_node_gain(&mut node.config, gain)?;
        }
        let next = revisions
            .submitted
            .checked_add(1)
            .ok_or_else(|| invalid("graph revision exhausted"))?;
        graph.revision = next;
        let mut selected: GraphDocument = candidate.active_document().clone();
        selected.revision = next;
        candidate.update_document(selected);
        Ok(candidate)
    }
}

pub(super) fn node_error(code: ErrorCode, graph_id: &str, node_id: &str) -> ProtocolError {
    ProtocolError {
        code,
        parameters: serde_json::json!({"graphId": graph_id, "nodeId": node_id}),
    }
}

fn controllable(node: &NodeSpec) -> bool {
    !matches!(
        node.config,
        NodeConfig::VirtualEndpointTransport(_) | NodeConfig::DeviceBus { .. }
    )
}

fn node_gain(config: &NodeConfig) -> Option<f32> {
    match config {
        NodeConfig::Gain(value) => Some(value.gain_db),
        NodeConfig::VirtualEndpoint(value) => Some(value.gain_db),
        NodeConfig::AudioDeviceInput(value) => Some(value.gain_db),
        NodeConfig::ApplicationCapture(value) => Some(value.gain_db),
        _ => None,
    }
}

fn set_node_gain(config: &mut NodeConfig, gain: f32) -> Result<(), ProtocolError> {
    if !gain.is_finite() || !(-60.0..=12.0).contains(&gain) {
        return Err(invalid("gain must be between -60 and 12 dB"));
    }
    let target = match config {
        NodeConfig::Gain(value) => &mut value.gain_db,
        NodeConfig::VirtualEndpoint(value) => &mut value.gain_db,
        NodeConfig::AudioDeviceInput(value) => &mut value.gain_db,
        NodeConfig::ApplicationCapture(value) => &mut value.gain_db,
        _ => return Err(invalid("node does not support gain")),
    };
    *target = gain;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixrelay_control_server::ControlBackend;

    fn graph() -> GraphDocument {
        serde_json::from_value(serde_json::json!({
            "schemaVersion":2,"revision":1,
            "nodes":[{"id":"gain","type":"gain","config":{"channels":2,"gainDb":-3.0}},
                {"id":"record","type":"recorder","config":{"channels":2}}],
            "edges":[{"id":"wire","source":{"nodeId":"gain","portId":"audio"},
                "destination":{"nodeId":"record","portId":"audio"}}]
        }))
        .unwrap()
    }

    fn request(revision: u64, enabled: Option<bool>, muted: Option<bool>) -> NodeControlRequest {
        NodeControlRequest {
            expected_revision: revision,
            enabled,
            muted,
            gain_db: None,
        }
    }

    #[test]
    fn native_restoration_rejects_control_admission_without_waiting_for_its_lock() {
        use mixrelay_audio_recorder::{RecorderAction, RecorderRequest};
        use mixrelay_control_protocol::RecorderControlRequest;
        let engine = std::sync::Arc::new(crate::endpoint_settings_tests::controller(graph()));
        let held = engine.mutation.lock();
        let (send, receive) = std::sync::mpsc::channel();
        let worker_engine = engine.clone();
        let worker = std::thread::spawn(move || {
            let patch = request(1, Some(false), None);
            let errors = [
                worker_engine.read_node_controls("graph").unwrap_err(),
                worker_engine
                    .preview_node_switch("graph", "gain", &patch)
                    .unwrap_err(),
                worker_engine
                    .change_node_control("graph", "gain", &patch)
                    .unwrap_err(),
                worker_engine
                    .recorder_status("graph", "record")
                    .unwrap_err(),
                worker_engine
                    .control_recorder(
                        "graph",
                        "record",
                        RecorderControlRequest {
                            expected_revision: 1,
                            expected_recording: None,
                            request: RecorderRequest {
                                action: RecorderAction::Stop,
                                path: None,
                                sample_rate: None,
                                encoding: None,
                            },
                        },
                    )
                    .unwrap_err(),
            ];
            send.send(errors).unwrap();
        });
        let result = receive.recv_timeout(std::time::Duration::from_secs(1));
        drop(held);
        worker.join().unwrap();
        for error in result.expect("control requests must finish while restoration owns mutation") {
            assert_eq!(error.code, ErrorCode::GraphExchangeBusy);
        }
    }

    #[test]
    fn gain_is_discovered_and_validated_before_publication() {
        let directory = mixrelay_test_support::FixtureDir::new("node-gain").unwrap();
        let engine = crate::endpoint_settings_tests::controller(graph())
            .with_graph_catalog_path(directory.join("graphs.json"));
        let id = engine.graph_catalog().unwrap().active_id;
        let mut processor = engine
            .prepare_and_validate(&engine.graph(), *engine.context.read())
            .unwrap()
            .processor;
        let revision = engine.exchange.revisions().submitted;
        assert_eq!(
            engine.read_node_controls(&id).unwrap().nodes[0].gain_db,
            Some(-3.0)
        );
        let original = engine.graph();
        for (node, gain) in [
            ("gain", f32::NAN),
            ("gain", 13.0),
            ("gain", -61.0),
            ("record", 0.0),
        ] {
            let mut patch = request(revision, None, None);
            patch.gain_db = Some(gain);
            assert_eq!(
                engine
                    .change_node_control(&id, node, &patch)
                    .unwrap_err()
                    .code,
                ErrorCode::GraphInvalid
            );
            assert_eq!(engine.graph(), original);
        }
        let mut patch = request(revision, None, None);
        patch.gain_db = Some(-9.0);
        engine.change_node_control(&id, "gain", &patch).unwrap();
        assert!(engine.exchange.activate_pending(&mut processor));
        assert_eq!(
            engine.read_node_controls(&id).unwrap().nodes[0].gain_db,
            Some(-9.0)
        );
        assert_eq!(engine.graph().edges, original.edges);
    }

    #[test]
    fn preview_is_read_only_and_switches_preserve_authored_connections() {
        let directory = mixrelay_test_support::FixtureDir::new("node-controls").unwrap();
        let engine = crate::endpoint_settings_tests::controller(graph())
            .with_graph_catalog_path(directory.join("graphs.json"));
        let catalog = engine.graph_catalog().unwrap();
        let mut processor = engine
            .prepare_and_validate(&engine.graph(), *engine.context.read())
            .unwrap()
            .processor;
        let original = engine.graph();
        let update = request(
            engine.exchange.revisions().submitted,
            Some(false),
            Some(true),
        );
        let preview = engine
            .preview_node_switch(&catalog.active_id, "gain", &update)
            .unwrap();
        assert_eq!(engine.graph(), original);
        let applied = engine
            .change_node_control(&catalog.active_id, "gain", &update)
            .unwrap();
        assert_eq!(preview.revision, applied.revision);
        assert!(engine.exchange.activate_pending(&mut processor));
        assert_eq!(engine.graph().edges, original.edges);
        let composed = engine
            .compose_execution(
                &engine.graph(),
                engine.catalog_candidate(&engine.graph()).as_ref(),
                *engine.context.read(),
            )
            .unwrap();
        assert_eq!(composed.document.nodes.len(), 1);
        assert!(composed.document.edges.is_empty());
        let controls = engine.read_node_controls(&catalog.active_id).unwrap();
        assert!(!controls.nodes[0].control.enabled);
        assert!(controls.nodes[0].control.muted);
        engine
            .change_node_control(
                &catalog.active_id,
                "gain",
                &request(applied.revision, Some(true), None),
            )
            .unwrap();
        assert!(engine.exchange.activate_pending(&mut processor));
        assert_eq!(engine.graph().edges, original.edges);
        assert!(engine.graph().nodes[0].control.muted);
        let disk: serde_json::Value =
            serde_json::from_slice(&std::fs::read(directory.join("graphs.json")).unwrap()).unwrap();
        assert_eq!(
            disk["graphs"][0]["document"]["nodes"][0]["control"]["muted"],
            true
        );
    }

    #[test]
    fn missing_stale_and_empty_commands_leave_catalog_untouched() {
        let directory = mixrelay_test_support::FixtureDir::new("node-control-errors").unwrap();
        let engine = crate::endpoint_settings_tests::controller(graph())
            .with_graph_catalog_path(directory.join("graphs.json"));
        let id = engine.graph_catalog().unwrap().active_id;
        let revision = engine.exchange.revisions().submitted;
        let original = engine.graph();
        assert_eq!(
            engine
                .change_node_control(&id, "deleted", &request(revision, Some(true), None))
                .unwrap_err()
                .code,
            ErrorCode::NodeNotFound
        );
        assert_eq!(
            engine
                .change_node_control(&id, "gain", &request(revision + 1, Some(true), None))
                .unwrap_err()
                .code,
            ErrorCode::GraphRevisionStale
        );
        assert_eq!(
            engine
                .change_node_control(&id, "gain", &request(revision, None, None))
                .unwrap_err()
                .code,
            ErrorCode::GraphInvalid
        );
        assert_eq!(engine.graph(), original);
    }

    #[test]
    fn background_node_switch_does_not_follow_editor_selection() {
        let directory = mixrelay_test_support::FixtureDir::new("node-control-graphs").unwrap();
        let path = directory.join("graphs.json");
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "version":1,"revision":1,"activeId":"a", "graphs":[
                    {"id":"a","name":"A","document":graph()},
                    {"id":"b","name":"B","document":graph()}
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        let engine =
            crate::endpoint_settings_tests::controller(graph()).with_graph_catalog_path(path);
        let revision = engine.exchange.revisions().submitted;
        let mut processor = crate::controller_test_support::settle(&engine);
        engine
            .change_node_control("b", "gain", &request(revision, Some(false), None))
            .unwrap();
        assert!(engine.exchange.activate_pending(&mut processor));
        assert!(
            engine.read_node_controls("a").unwrap().nodes[0]
                .control
                .enabled
        );
        assert!(
            !engine.read_node_controls("b").unwrap().nodes[0]
                .control
                .enabled
        );
        assert_eq!(engine.graph_catalog().unwrap().active_id, "a");
    }

    #[test]
    fn previous_take_stop_cannot_stop_a_new_recording_at_same_graph_revision() {
        use mixrelay_audio_recorder::{RecorderAction, RecorderRequest, RecorderState};
        use mixrelay_control_protocol::{ObservedRecording, RecorderControlRequest};
        let directory = mixrelay_test_support::FixtureDir::new("take-fence").unwrap();
        let engine = crate::endpoint_settings_tests::controller(graph())
            .with_graph_catalog_path(directory.join("graphs.json"));
        let graph_id = engine.graph_catalog().unwrap().active_id;
        let id = engine.recorder_session_id(&graph_id, "record").unwrap();
        let tap = engine.recorders.prepare(&id, 2).unwrap();
        tap.capture(&[0.25; 8], 48_000);
        let revision = engine.exchange.revisions().submitted;
        let command = |action, path, expected_recording| RecorderControlRequest {
            expected_revision: revision,
            expected_recording,
            request: RecorderRequest {
                action,
                path,
                sample_rate: None,
                encoding: None,
            },
        };
        let a = engine
            .control_recorder(
                &graph_id,
                "record",
                command(
                    RecorderAction::Record,
                    Some(directory.join("a.wav").to_string_lossy().into_owned()),
                    None,
                ),
            )
            .unwrap();
        let observed = ObservedRecording {
            state: a.state,
            path: a.path,
        };
        engine
            .control_recorder(
                &graph_id,
                "record",
                command(RecorderAction::Stop, None, Some(observed.clone())),
            )
            .unwrap();
        let b = engine
            .control_recorder(
                &graph_id,
                "record",
                command(
                    RecorderAction::Record,
                    Some(directory.join("b.wav").to_string_lossy().into_owned()),
                    None,
                ),
            )
            .unwrap();
        let error = engine
            .control_recorder(
                &graph_id,
                "record",
                command(RecorderAction::Stop, None, Some(observed)),
            )
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::GraphRevisionStale);
        assert_eq!(engine.recorders.status(&id).state, RecorderState::Recording);
        assert_eq!(engine.recorders.status(&id).path, b.path);
        engine
            .control_recorder(
                &graph_id,
                "record",
                command(
                    RecorderAction::Stop,
                    None,
                    Some(ObservedRecording {
                        state: b.state,
                        path: b.path,
                    }),
                ),
            )
            .unwrap();
    }

    #[test]
    fn recorder_off_finalizes_and_on_does_not_restart_recording() {
        use mixrelay_audio_recorder::{RecorderAction, RecorderRequest, RecorderState};
        use mixrelay_control_protocol::RecorderControlRequest;
        let directory = mixrelay_test_support::FixtureDir::new("node-control-recorder").unwrap();
        let engine = crate::endpoint_settings_tests::controller(graph())
            .with_graph_catalog_path(directory.join("graphs.json"));
        let graph_id = engine.graph_catalog().unwrap().active_id;
        let mut processor = engine
            .prepare_and_validate(&engine.graph(), *engine.context.read())
            .unwrap()
            .processor;
        let id = engine.recorder_session_id(&graph_id, "record").unwrap();
        let tap = engine.recorders.prepare(&id, 2).unwrap();
        tap.capture(&[0.25; 8], 48_000);
        let start = RecorderRequest {
            action: RecorderAction::Record,
            path: Some(directory.join("take.wav").to_string_lossy().into_owned()),
            sample_rate: None,
            encoding: None,
        };
        engine
            .control_recorder(
                &graph_id,
                "record",
                RecorderControlRequest {
                    expected_recording: None,
                    expected_revision: engine.exchange.revisions().submitted,
                    request: start.clone(),
                },
            )
            .unwrap();
        tap.capture(&[0.25; 8], 48_000);
        let off = engine
            .change_node_control(
                &graph_id,
                "record",
                &request(engine.exchange.revisions().submitted, Some(false), None),
            )
            .unwrap();
        assert!(engine.exchange.activate_pending(&mut processor));
        assert_eq!(
            engine.recorder_status(&graph_id, "record").unwrap().state,
            RecorderState::Stopped
        );
        assert_eq!(
            engine
                .control_recorder(
                    &graph_id,
                    "record",
                    RecorderControlRequest {
                        expected_recording: None,
                        expected_revision: off.revision,
                        request: start,
                    }
                )
                .unwrap_err()
                .code,
            ErrorCode::NodeDisabled
        );
        engine
            .change_node_control(
                &graph_id,
                "record",
                &request(off.revision, Some(true), None),
            )
            .unwrap();
        assert!(engine.exchange.activate_pending(&mut processor));
        assert_eq!(
            engine.recorder_status(&graph_id, "record").unwrap().state,
            RecorderState::Stopped
        );
        assert!(std::fs::metadata(directory.join("take.wav")).unwrap().len() > 44);
    }

    fn recorder_start(revision: u64) -> mixrelay_control_protocol::RecorderControlRequest {
        mixrelay_control_protocol::RecorderControlRequest {
            expected_recording: None,
            expected_revision: revision,
            request: mixrelay_audio_recorder::RecorderRequest {
                action: mixrelay_audio_recorder::RecorderAction::Record,
                path: None,
                sample_rate: None,
                encoding: None,
            },
        }
    }

    #[test]
    fn deleted_and_readded_identity_rejects_delayed_node_and_recorder_commands() {
        let directory = mixrelay_test_support::FixtureDir::new("node-control-recreated").unwrap();
        let engine = crate::endpoint_settings_tests::controller(graph())
            .with_graph_catalog_path(directory.join("graphs.json"));
        let id = engine.graph_catalog().unwrap().active_id;
        let old_revision = engine.exchange.revisions().submitted;
        let mut processor = engine
            .prepare_and_validate(&engine.graph(), *engine.context.read())
            .unwrap()
            .processor;
        let mut deleted = engine.graph();
        deleted.revision += 1;
        deleted.nodes.retain(|node| node.id.as_str() != "record");
        deleted.edges.clear();
        engine.apply_graph(deleted.clone()).unwrap();
        assert!(engine.exchange.activate_pending(&mut processor));
        assert_eq!(
            engine
                .change_node_control(&id, "record", &request(deleted.revision, Some(false), None))
                .unwrap_err()
                .code,
            ErrorCode::NodeNotFound
        );
        assert_eq!(
            engine
                .control_recorder(&id, "record", recorder_start(deleted.revision))
                .unwrap_err()
                .code,
            ErrorCode::NodeNotFound
        );
        assert_eq!(engine.graph(), deleted);
        let mut replaced = graph();
        replaced.revision = deleted.revision + 1;
        replaced.nodes[1].label = Some("New recorder instance".into());
        engine.apply_graph(replaced.clone()).unwrap();
        assert!(engine.exchange.activate_pending(&mut processor));
        assert_eq!(
            engine
                .change_node_control(
                    &id,
                    "record",
                    &request(old_revision, Some(false), Some(true))
                )
                .unwrap_err()
                .code,
            ErrorCode::GraphRevisionStale
        );
        assert_eq!(
            engine
                .control_recorder(&id, "record", recorder_start(old_revision))
                .unwrap_err()
                .code,
            ErrorCode::GraphRevisionStale
        );
        assert_eq!(engine.graph(), replaced);
        assert_eq!(
            engine.recorder_status(&id, "record").unwrap().state,
            mixrelay_audio_recorder::RecorderState::Stopped
        );
        let off = engine
            .change_node_control(
                &id,
                "record",
                &request(replaced.revision, Some(false), None),
            )
            .unwrap();
        assert!(engine.exchange.activate_pending(&mut processor));
        let disabled = engine.graph();
        assert_eq!(
            engine
                .control_recorder(&id, "record", recorder_start(off.revision))
                .unwrap_err()
                .code,
            ErrorCode::NodeDisabled
        );
        assert_eq!(engine.graph(), disabled);
        assert_eq!(engine.exchange.revisions().submitted, off.revision);
    }
}
