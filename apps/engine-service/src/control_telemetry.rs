//! Batched committed state observations, independent of native mutation preparation.
use crate::{EngineGraphController, NodeConfig, processor_editor_error};
use mixrelay_control_protocol::{
    AppStatus, ControlStateSnapshot, GraphRuntimeStatus, ProcessorEditorStatus, ProtocolError,
};
use mixrelay_control_server::ControlBackend as _;
use std::time::{Duration, Instant};

pub(super) struct CachedStatus {
    observed: Instant,
    graph: GraphRuntimeStatus,
    enabled: bool,
    status: AppStatus,
}

impl EngineGraphController {
    pub(super) fn observe_control_state(&self) -> Option<ControlStateSnapshot> {
        // One admission for the complete batch. A native writer cannot block meter publication.
        let _mutation = self.mutation.try_lock()?;
        let graph = self.graph_status();
        let editors = self
            .graph()
            .nodes
            .iter()
            .filter(|node| matches!(node.config, NodeConfig::Provider(_)))
            .filter_map(|node| self.processor_editor_status_locked(node.id.as_str()).ok())
            .collect();
        Some(ControlStateSnapshot {
            generation: 0,
            status: self.telemetry_status(graph),
            graph,
            controls: self.snapshot_controls().ok(),
            editors,
        })
    }

    fn telemetry_status(&self, graph: GraphRuntimeStatus) -> AppStatus {
        let mut cached = self.telemetry_status.lock();
        let enabled = self.engine_power_status().enabled;
        // Native Driver inventory remains a one-second observation, rather than twenty
        // socket round trips per second. Revision changes force immediate refresh.
        let fresh = cached.as_ref().filter(|old| {
            old.observed.elapsed() < Duration::from_secs(1)
                && old.graph == graph
                && old.enabled == enabled
        });
        let mut status = if let Some(fresh) = fresh {
            fresh.status.clone()
        } else {
            let status = self.status();
            *cached = Some(CachedStatus {
                observed: Instant::now(),
                graph,
                enabled,
                status: status.clone(),
            });
            status
        };
        status.engine_power = Some(self.engine_power_status());
        status.processor_timings = self.processor_timings();
        status.processor_latencies = self.processor_latencies();
        status.failed_processors = self.failed_processor_status();
        status
            .recorder_error
            .clone_from(&self.recorder_cleanup_error.lock());
        status
    }

    /// The caller holds mutation admission for the selected graph identity.
    pub(super) fn processor_editor_status_locked(
        &self,
        node_id: &str,
    ) -> Result<ProcessorEditorStatus, ProtocolError> {
        let runtime_id = self.external_processor_node(node_id)?;
        let mut status = self
            .processor_controls
            .editor_status(&runtime_id)
            .map_err(|reason| processor_editor_error(node_id, &reason))?;
        node_id.clone_into(&mut status.node_id);
        Ok(status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GraphDocument;

    #[test]
    fn telemetry_defers_native_writers_and_caches_inventory_until_expiry_or_power_change() {
        let controller =
            crate::endpoint_settings_tests::controller(GraphDocument::new(7, vec![], vec![]));
        let writer = controller.mutation.lock();
        assert!(controller.telemetry_state().is_none());
        assert_eq!(controller.meters().protocol_version, 1);
        drop(writer);
        let state = controller.telemetry_state().unwrap();
        assert_eq!(state.graph.submitted_revision, 7);
        assert_eq!(state.controls.unwrap().revision, 7);
        let first = controller
            .telemetry_status
            .lock()
            .as_ref()
            .unwrap()
            .observed;
        assert!(controller.telemetry_state().is_some());
        assert_eq!(
            controller
                .telemetry_status
                .lock()
                .as_ref()
                .unwrap()
                .observed,
            first
        );
        controller
            .telemetry_status
            .lock()
            .as_mut()
            .unwrap()
            .observed -= Duration::from_secs(2);
        assert!(controller.telemetry_state().is_some());
        assert!(
            controller
                .telemetry_status
                .lock()
                .as_ref()
                .unwrap()
                .observed
                >= first
        );
        controller
            .execution_enabled
            .store(false, std::sync::atomic::Ordering::Release);
        let state = controller.telemetry_state().unwrap();
        assert!(!state.status.engine_power.unwrap().enabled);
        assert!(!controller.telemetry_status.lock().as_ref().unwrap().enabled);
    }
}
