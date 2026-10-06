//! Execution is independent of authored documents and control-service lifetime.
//! Empty publication uses the same acknowledged resource handoff as graph edits.
use std::sync::atomic::Ordering;

use mixrelay_control_protocol::{EnginePowerStatus, GraphApplyResponse, GraphApplyState};

use crate::{EngineGraphController, ProtocolError, session_error};

impl EngineGraphController {
    pub(super) fn preview_engine_start(
        &self,
    ) -> Result<mixrelay_control_protocol::ApplicationCaptureManifest, ProtocolError> {
        let _mutation = self.mutation.lock();
        if self.execution_enabled.load(Ordering::Acquire)
            || self.engine_power_status().transitioning
        {
            return Err(session_error("engine must be off before start preflight"));
        }
        let mut document = self.graph.read().clone();
        document.revision = self
            .exchange
            .revisions()
            .submitted
            .checked_add(1)
            .ok_or_else(|| session_error("engine revision exhausted"))?;
        let catalog = self.catalog_candidate(&document);
        let composed =
            self.compose_enabled_execution(&document, catalog.as_ref(), *self.context.read())?;
        crate::execution_control::composed_capture_manifest(&composed)
    }
    pub(super) fn engine_power_status(&self) -> EnginePowerStatus {
        let enabled = self.execution_enabled.load(Ordering::Acquire);
        let revisions = self.exchange.revisions();
        EnginePowerStatus {
            enabled,
            transitioning: revisions.active != revisions.submitted
                || (!enabled && !self.graph_resources.execution_released()),
        }
    }

    pub(super) fn change_engine_power(
        &self,
        enabled: bool,
        expected_revision: Option<u64>,
    ) -> Result<GraphApplyResponse, ProtocolError> {
        let _mutation = self.mutation.lock();
        let previous = self.execution_enabled.load(Ordering::Acquire);
        let revisions = self.exchange.revisions();
        if expected_revision.is_some_and(|expected| expected != revisions.submitted) {
            return Err(session_error("engine power preflight is stale"));
        }
        if enabled == previous {
            return Ok(GraphApplyResponse {
                protocol_version: mixrelay_control_protocol::PROTOCOL_VERSION,
                revision: revisions.submitted,
                state: GraphApplyState::Pending,
                persistence_error: self.catalog_write_error.lock().clone(),
            });
        }
        if revisions.active != revisions.submitted
            || (!previous && !self.graph_resources.execution_released())
        {
            return Err(session_error("engine execution is still transitioning"));
        }
        // A broken plug-in checkpoint must not prevent the user releasing audio.
        let checkpoint_error = if previous {
            self.checkpoint_processor_state_locked(None).err()
        } else {
            None
        };
        if !enabled && let Err(reason) = self.recorders.stop_all() {
            tracing::error!(%reason, "engine stop recorder finalization failed");
            *self.recorder_cleanup_error.lock() = Some(reason);
        }
        let mut document = self.graph.read().clone();
        document.revision = revisions
            .submitted
            .checked_add(1)
            .ok_or_else(|| session_error("engine revision exhausted"))?;
        let catalog = self.catalog_candidate(&document);
        self.execution_enabled.store(enabled, Ordering::Release);
        let context = *self.context.read();
        let result = self.publish_graph_context(document, catalog, context, enabled);
        if result.is_err() {
            self.execution_enabled.store(previous, Ordering::Release);
        }
        if let Some(error) = checkpoint_error {
            tracing::error!(?error, "engine stop retained last saved plug-in state");
            *self.catalog_write_error.lock() = Some(format!("engine stop checkpoint: {error:?}"));
        }
        tracing::info!(
            enabled,
            accepted = result.is_ok(),
            "engine execution switch"
        );
        result
    }
}

#[cfg(test)]
mod tests {
    use mixrelay_audio_graph::GraphDocument;
    use mixrelay_control_server::ControlBackend;

    fn graph(revision: u64) -> GraphDocument {
        serde_json::from_value(serde_json::json!({
            "schemaVersion":2,"revision":revision,
            "nodes":[{"id":"record","type":"recorder","config":{"channels":2}}],
            "edges":[]
        }))
        .unwrap()
    }

    #[test]
    fn off_edits_keep_authored_catalog_and_resume_the_latest_document() {
        let directory = mixrelay_test_support::FixtureDir::new("engine-power").unwrap();
        let engine = crate::endpoint_settings_tests::controller(graph(1))
            .with_graph_catalog_path(directory.join("graphs.json"));
        let original = engine.graph_catalog().unwrap();
        let mut processor = engine
            .prepare_and_validate(&engine.graph(), *engine.context.read())
            .unwrap()
            .processor;
        for _ in 0..3 {
            let before = engine.graph();
            let off = engine.change_engine_power(false, None).unwrap();
            assert!(!engine.engine_power_status().enabled);
            assert!(engine.engine_power_status().transitioning);
            assert_eq!(
                engine.change_engine_power(false, None).unwrap().revision,
                off.revision
            );
            assert!(engine.change_engine_power(true, None).is_err());
            assert!(engine.exchange.activate_pending(&mut processor));
            engine.reclaim_recorders();
            assert!(!engine.engine_power_status().transitioning);
            assert!(
                engine
                    .current_capture_manifest()
                    .unwrap()
                    .sources
                    .is_empty()
            );
            let catalog = engine.graph_catalog().unwrap();
            assert_eq!(catalog.active_id, original.active_id);
            assert_eq!(catalog.document.nodes, before.nodes);
            assert_eq!(catalog.document.edges, original.document.edges);
            let mut edited = engine.graph();
            edited.revision += 1;
            edited.nodes[0].label = Some("Saved while off".into());
            engine.apply_graph_request(edited.into()).unwrap();
            assert!(engine.exchange.activate_pending(&mut processor));
            let composed = engine
                .compose_execution(&engine.graph(), None, *engine.context.read())
                .unwrap();
            assert!(composed.document.nodes.is_empty());
            engine.change_engine_power(true, None).unwrap();
            assert!(engine.exchange.activate_pending(&mut processor));
            assert!(engine.engine_power_status().enabled);
            assert_eq!(
                engine.graph().nodes[0].label.as_deref(),
                Some("Saved while off")
            );
        }
    }

    #[test]
    fn failed_enable_keeps_execution_off() {
        let engine =
            crate::endpoint_settings_tests::controller(GraphDocument::new(1, vec![], vec![]));
        let mut processor = engine
            .prepare_and_validate(&engine.graph(), *engine.context.read())
            .unwrap()
            .processor;
        engine.change_engine_power(false, None).unwrap();
        engine.exchange.activate_pending(&mut processor);
        // Authored invalid processor cannot be silently activated on power-on.
        engine.graph.write().nodes = graph(1).nodes;
        engine.graph.write().nodes[0].config =
            mixrelay_audio_graph::NodeConfig::Recorder { channels: 0 };
        assert!(engine.change_engine_power(true, None).is_err());
        assert!(!engine.engine_power_status().enabled);
    }

    #[test]
    fn storage_failure_cannot_block_stop_and_stale_start_preflight_cannot_publish() {
        let directory = mixrelay_test_support::FixtureDir::new("engine-power-disk").unwrap();
        let engine = crate::endpoint_settings_tests::controller(graph(1))
            .with_graph_catalog_path(directory.join("graphs.json"));
        let mut processor = engine
            .prepare_and_validate(&engine.graph(), *engine.context.read())
            .unwrap()
            .processor;
        let blocker = directory.join("file-not-directory");
        std::fs::write(&blocker, b"occupied").unwrap();
        engine.catalog.lock().as_mut().unwrap().path = blocker.join("graphs.json");
        let response = engine.change_engine_power(false, None).unwrap();
        assert!(response.persistence_error.is_some());
        engine.exchange.activate_pending(&mut processor);
        assert!(!engine.engine_power_status().enabled);
        let preview = engine.preview_engine_start().unwrap();
        assert_eq!(preview.revision, response.revision + 1);
        assert!(
            engine
                .change_engine_power(true, Some(response.revision - 1))
                .is_err()
        );
        assert!(!engine.engine_power_status().enabled);
    }
}
