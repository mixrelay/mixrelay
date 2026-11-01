//! Execution is independent of authored documents and control-service lifetime.
//! Empty publication uses the same acknowledged resource handoff as graph edits.
use mixrelay_control_protocol::{EnginePowerStatus, GraphApplyResponse, GraphApplyState};

use crate::{EngineGraphController, ProtocolError, session_error};

impl EngineGraphController {
    /// Starts paused because the service's crash guard found repeated unexpected ends.
    /// Apply before the durable catalog, which saves the pause and its reason.
    #[must_use]
    pub fn after_repeated_failures(self, tripped: bool) -> Self {
        if tripped {
            self.power.pause_after_failures();
        }
        self
    }

    pub(super) fn retry_pause_save(&self) {
        if !self.power.unsaved() {
            return;
        }
        // Serialized with power changes, so a retry cannot save a superseded choice.
        let _mutation = self.mutation.lock();
        self.power.persist(self.power.enabled());
    }

    pub(super) fn preview_engine_start(
        &self,
    ) -> Result<mixrelay_control_protocol::ApplicationCaptureManifest, ProtocolError> {
        let _mutation = self.mutation.lock();
        if self.power.enabled() || self.engine_power_status().transitioning {
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
        let enabled = self.power.enabled();
        let revisions = self.exchange.revisions();
        EnginePowerStatus {
            enabled,
            transitioning: revisions.active != revisions.submitted
                || (!enabled && !self.graph_resources.execution_released()),
            unsaved: self.power.unsaved(),
            pause_reason: self.power.pause_reason(enabled),
        }
    }

    pub(super) fn change_engine_power(
        &self,
        enabled: bool,
        expected_revision: Option<u64>,
    ) -> Result<GraphApplyResponse, ProtocolError> {
        let _mutation = self.mutation.lock();
        let previous = self.power.enabled();
        let revisions = self.exchange.revisions();
        if expected_revision.is_some_and(|expected| expected != revisions.submitted) {
            return Err(session_error("engine power preflight is stale"));
        }
        if enabled == previous {
            if enabled {
                self.recorders.resume()?;
            }
            return Ok(GraphApplyResponse {
                protocol_version: mixrelay_control_protocol::PROTOCOL_VERSION,
                revision: revisions.submitted,
                state: GraphApplyState::Pending,
                persistence_error: self.graph_persistence_warning(),
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
        let mut document = self.graph.read().clone();
        document.revision = revisions
            .submitted
            .checked_add(1)
            .ok_or_else(|| session_error("engine revision exhausted"))?;
        let catalog = self.catalog_candidate(&document);
        if !enabled {
            self.recorders.suspend()?;
        }
        self.power.set_enabled(enabled);
        let context = *self.context.read();
        // The choice is saved once published, before recorder resume can fail.
        let result = self
            .publish_graph_context(document, catalog, context, enabled)
            .map(|mut response| {
                self.power.persist(enabled);
                response.persistence_error = self.graph_persistence_warning();
                response
            });
        if result.is_err() {
            self.power.set_enabled(previous);
            if !enabled {
                self.recorders.resume()?;
            }
        } else if enabled {
            self.power.clear_failure_reason();
            self.recorders.resume()?;
        }
        if let Some(error) = checkpoint_error {
            tracing::error!(?error, "engine stop retained last saved plug-in state");
            self.catalog
                .record_warning(Some(format!("engine stop checkpoint: {error:?}")));
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
    use crate::execution_choice::SAVED_PAUSE;
    use mixrelay_audio_graph::GraphDocument;
    use mixrelay_audio_recorder::{RecorderAction, RecorderRequest, RecorderState};
    use mixrelay_control_server::ControlBackend;

    fn recorder_request(action: RecorderAction) -> RecorderRequest {
        RecorderRequest {
            action,
            path: None,
            sample_rate: None,
            encoding: None,
        }
    }

    fn graph(revision: u64) -> GraphDocument {
        serde_json::from_value(serde_json::json!({
            "schemaVersion":2,"revision":revision,
            "nodes":[{"id":"record","type":"recorder","config":{"channels":2}}],
            "edges":[]
        }))
        .unwrap()
    }

    #[test]
    fn a_pause_outlives_the_service_until_resume() {
        use crate::controller_test_support::settle;
        let directory = mixrelay_test_support::FixtureDir::new("engine-saved-pause").unwrap();
        let catalog = directory.join("graphs.json");
        let marker = directory.join(SAVED_PAUSE);
        let start = || {
            crate::endpoint_settings_tests::controller(
                crate::endpoint_settings_tests::endpoint_document(1, "Retained device"),
            )
            .with_graph_catalog_path(catalog.clone())
        };
        let engine = start();
        assert!(engine.engine_power_status().enabled);
        let inventory = engine.endpoint_settings().unwrap();
        let nodes = engine.graph().nodes;
        engine.change_engine_power(false, None).unwrap();
        settle(&engine);
        assert!(marker.is_file());
        drop(engine);
        let restarted = start();
        assert!(!restarted.engine_power_status().enabled);
        // The audio worker takes the restored graph before the user resumes.
        let paused = settle(&restarted);
        assert!(paused.plan().source_bindings().is_empty());
        assert!(paused.plan().sink_bindings().is_empty());
        assert_eq!(restarted.graph().nodes, nodes);
        assert_eq!(restarted.endpoint_settings().unwrap(), inventory);
        // Relaunching the whole controller repeats durable startup, without resuming audio.
        drop(paused);
        drop(restarted);
        let restarted = start();
        let paused = settle(&restarted);
        assert!(!restarted.engine_power_status().enabled);
        assert!(paused.plan().source_bindings().is_empty());
        assert_eq!(restarted.endpoint_settings().unwrap(), inventory);
        let resumed = restarted.change_engine_power(true, None).unwrap();
        assert_eq!(resumed.persistence_error, None);
        let running = settle(&restarted);
        assert!(!running.plan().source_bindings().is_empty());
        assert!(!running.plan().sink_bindings().is_empty());
        assert_eq!(restarted.graph().nodes, nodes);
        assert!(!marker.exists());
        drop(restarted);
        assert!(start().engine_power_status().enabled);
    }

    #[test]
    fn a_pause_after_repeated_failures_keeps_its_reason_until_resume() {
        use crate::controller_test_support::settle;
        use mixrelay_control_protocol::EnginePauseReason;
        let directory = mixrelay_test_support::FixtureDir::new("engine-crash-loop").unwrap();
        let catalog = directory.join("graphs.json");
        let start = |tripped| {
            crate::endpoint_settings_tests::controller(graph(1))
                .after_repeated_failures(tripped)
                .with_graph_catalog_path(catalog.clone())
        };
        let engine = start(true);
        let status = engine.engine_power_status();
        assert!(!status.enabled);
        assert_eq!(
            status.pause_reason,
            Some(EnginePauseReason::RepeatedFailures)
        );
        drop(engine);
        // A later clean start keeps both the pause and why it happened.
        let restarted = start(false);
        assert_eq!(
            restarted.engine_power_status().pause_reason,
            Some(EnginePauseReason::RepeatedFailures)
        );
        settle(&restarted);
        restarted.change_engine_power(true, None).unwrap();
        settle(&restarted);
        assert_eq!(restarted.engine_power_status().pause_reason, None);
        // A user pause carries no reason.
        restarted.change_engine_power(false, None).unwrap();
        settle(&restarted);
        drop(restarted);
        let paused = start(false);
        assert!(!paused.engine_power_status().enabled);
        assert_eq!(paused.engine_power_status().pause_reason, None);
    }

    #[test]
    fn an_unsaved_pause_choice_is_reported_until_maintenance_saves_it() {
        use crate::controller_test_support::settle;
        let directory = mixrelay_test_support::FixtureDir::new("engine-unsaved-pause").unwrap();
        let engine = crate::endpoint_settings_tests::controller(graph(1))
            .with_graph_catalog_path(directory.join("graphs.json"));
        let marker = directory.join(SAVED_PAUSE);
        let warning = || engine.graph_catalog().unwrap().persistence_error;
        let unsaved = |warning: Option<String>| {
            assert!(warning.unwrap().starts_with("engine pause: "));
        };
        // A directory in the marker's place can be neither replaced nor removed.
        std::fs::create_dir(&marker).unwrap();
        unsaved(
            engine
                .change_engine_power(false, None)
                .unwrap()
                .persistence_error,
        );
        assert!(!engine.engine_power_status().enabled);
        settle(&engine);
        // A successful catalog save does not clear the unsaved pause.
        engine
            .update_graph_layout(
                &engine.view_graph_id(),
                mixrelay_control_protocol::GraphLayoutRequest {
                    expected_layout_revision: 0,
                    layout: mixrelay_control_protocol::GraphLayout::default(),
                },
            )
            .unwrap();
        unsaved(warning());
        engine.retry_pause_save();
        unsaved(warning());
        assert!(engine.engine_power_status().unsaved);
        std::fs::remove_dir(&marker).unwrap();
        // A later successful change does not report the earlier failure.
        let resumed = engine.change_engine_power(true, None).unwrap();
        assert_eq!(resumed.persistence_error, None);
        settle(&engine);
        engine.change_engine_power(false, None).unwrap();
        settle(&engine);
        assert!(marker.is_file());
        std::fs::remove_file(&marker).unwrap();
        std::fs::create_dir(&marker).unwrap();
        unsaved(
            engine
                .change_engine_power(true, None)
                .unwrap()
                .persistence_error,
        );
        settle(&engine);
        std::fs::remove_dir(&marker).unwrap();
        engine.retry_pause_save();
        assert_eq!(warning(), None);
        assert!(!engine.engine_power_status().unsaved);
        assert!(!marker.exists());
        // Nothing left to retry.
        engine.retry_pause_save();
    }

    #[test]
    fn pause_retires_render_taps_but_resumes_the_same_rf64_without_the_gap() {
        let directory = mixrelay_test_support::FixtureDir::new("engine-pause-recording").unwrap();
        let path = directory.join("take.wav");
        let engine = crate::endpoint_settings_tests::controller(graph(1))
            .with_graph_catalog_path(directory.join("graphs.json"));
        let catalog = engine.graph_catalog().unwrap();
        let id = engine
            .recorder_session_id(&catalog.active_id, "record")
            .unwrap();
        let mut processor = engine
            .prepare_and_validate(&engine.graph(), *engine.context.read())
            .unwrap()
            .processor;
        let tap = engine.recorders.registry().prepare(&id, 2).unwrap();
        tap.capture(&[], 48_000);
        engine
            .recorders
            .registry()
            .command(
                &id,
                RecorderRequest {
                    path: Some(path.to_string_lossy().into_owned()),
                    encoding: Some(mixrelay_audio_recorder::Encoding::Float32),
                    ..recorder_request(RecorderAction::Record)
                },
            )
            .unwrap();
        let first = [0.25, -0.5, 0.125, -0.25];
        let second = [-0.75, 0.5, -0.125, 0.25];
        for _ in 0..3 {
            tap.capture(&first, 48_000);
            engine.change_engine_power(false, None).unwrap();
            assert!(engine.exchange.activate_pending(&mut processor));
            engine.reclaim_recorders();
            let paused = engine.recorders.status(&id);
            assert_eq!(paused.state, RecorderState::Paused);
            assert_eq!(paused.path.as_deref(), path.to_str());
            tap.capture(&[0.99; 16], 48_000);
            engine.change_engine_power(false, None).unwrap();
            engine.change_engine_power(true, None).unwrap();
            assert!(engine.exchange.activate_pending(&mut processor));
            assert_eq!(engine.recorders.status(&id).state, RecorderState::Recording);
            tap.capture(&second, 48_000);
        }
        // Simulate complete plan/tap retirement while a take is paused.
        engine.change_engine_power(false, None).unwrap();
        assert!(engine.exchange.activate_pending(&mut processor));
        drop(tap);
        engine.reclaim_recorders();
        assert_eq!(engine.recorders.status(&id).state, RecorderState::Paused);
        engine.change_engine_power(true, None).unwrap();
        assert!(engine.exchange.activate_pending(&mut processor));
        engine
            .recorders
            .registry()
            .command(&id, recorder_request(RecorderAction::Stop))
            .unwrap();
        let mut file = mixrelay_test_support::rf64::open(&path).unwrap();
        let samples: Vec<f32> = file.samples::<f32>().map(Result::unwrap).collect();
        assert_eq!(samples, [first, second].concat().repeat(3));
        assert_eq!(
            mixrelay_test_support::rf64::inspect(&path).unwrap().frames,
            12
        );
    }

    #[test]
    fn global_resume_preserves_manual_pause_and_stopping_while_paused_finalizes() {
        let directory = mixrelay_test_support::FixtureDir::new("engine-manual-pause").unwrap();
        let path = directory.join("take.wav");
        let engine = crate::endpoint_settings_tests::controller(graph(1))
            .with_graph_catalog_path(directory.join("graphs.json"));
        let catalog = engine.graph_catalog().unwrap();
        let id = engine
            .recorder_session_id(&catalog.active_id, "record")
            .unwrap();
        let mut processor = engine
            .prepare_and_validate(&engine.graph(), *engine.context.read())
            .unwrap()
            .processor;
        let tap = engine.recorders.registry().prepare(&id, 2).unwrap();
        tap.capture(&[], 48_000);
        engine
            .recorders
            .registry()
            .command(
                &id,
                RecorderRequest {
                    path: Some(path.to_string_lossy().into_owned()),
                    ..recorder_request(RecorderAction::Record)
                },
            )
            .unwrap();
        tap.capture(&[0.25; 4], 48_000);
        engine
            .recorders
            .registry()
            .command(&id, recorder_request(RecorderAction::Pause))
            .unwrap();
        engine.change_engine_power(false, None).unwrap();
        assert!(engine.exchange.activate_pending(&mut processor));
        drop(tap);
        engine.reclaim_recorders();
        assert_eq!(engine.recorders.status(&id).state, RecorderState::Paused);
        engine.change_engine_power(true, None).unwrap();
        assert!(engine.exchange.activate_pending(&mut processor));
        assert_eq!(engine.recorders.status(&id).state, RecorderState::Paused);
        engine.change_engine_power(false, None).unwrap();
        assert!(engine.exchange.activate_pending(&mut processor));
        engine.reclaim_recorders();
        engine.recorders.registry().stop_all().unwrap();
        assert_eq!(engine.recorders.status(&id).state, RecorderState::Stopped);
        assert_eq!(
            mixrelay_test_support::rf64::inspect(&path).unwrap().frames,
            2
        );
        engine.change_engine_power(true, None).unwrap();
        assert_eq!(engine.recorders.status(&id).state, RecorderState::Stopped);
    }

    #[test]
    fn failed_resume_keeps_the_open_take_paused() {
        let directory = mixrelay_test_support::FixtureDir::new("engine-resume-failure").unwrap();
        let path = directory.join("take.wav");
        let engine = crate::endpoint_settings_tests::controller(graph(1))
            .with_graph_catalog_path(directory.join("graphs.json"));
        let catalog = engine.graph_catalog().unwrap();
        let id = engine
            .recorder_session_id(&catalog.active_id, "record")
            .unwrap();
        let mut processor = engine
            .prepare_and_validate(&engine.graph(), *engine.context.read())
            .unwrap()
            .processor;
        let tap = engine.recorders.registry().prepare(&id, 2).unwrap();
        tap.capture(&[], 48_000);
        engine
            .recorders
            .registry()
            .command(
                &id,
                RecorderRequest {
                    path: Some(path.to_string_lossy().into_owned()),
                    ..recorder_request(RecorderAction::Record)
                },
            )
            .unwrap();
        tap.capture(&[0.25; 4], 48_000);
        engine.change_engine_power(false, None).unwrap();
        assert!(engine.exchange.activate_pending(&mut processor));
        drop(tap);
        engine.reclaim_recorders();
        engine.graph.write().nodes[0].config =
            mixrelay_audio_graph::NodeConfig::Recorder { channels: 0 };
        assert!(engine.change_engine_power(true, None).is_err());
        assert!(!engine.engine_power_status().enabled);
        assert_eq!(engine.recorders.status(&id).state, RecorderState::Paused);
        engine.graph.write().nodes[0].config =
            mixrelay_audio_graph::NodeConfig::Recorder { channels: 2 };
        engine.change_engine_power(true, None).unwrap();
        assert_eq!(engine.recorders.status(&id).state, RecorderState::Recording);
        engine.recorders.registry().stop_all().unwrap();
        assert_eq!(
            mixrelay_test_support::rf64::inspect(&path).unwrap().frames,
            2
        );
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
        engine.catalog.set_path(blocker.join("graphs.json"));
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
