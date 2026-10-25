//! Control-plane maintenance for retired recording sessions and native editor checkpoints.
//! Resource reclamation and persistence remain active when the desktop is closed.

use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

use mixrelay_audio_graph::NodeId;
use mixrelay_control_protocol::ProtocolError;
use mixrelay_processor_host::ProcessorEditorCloseObservation;

use crate::graph_composition::runtime_node_id;
use crate::{EngineGraphController, NodeConfig};

#[derive(Default)]
struct CloseLedger {
    saved: BTreeMap<NodeId, ProcessorEditorCloseObservation>,
    last_attempt: Option<NodeId>,
}

impl CloseLedger {
    fn next(
        &mut self,
        current: &BTreeMap<NodeId, ProcessorEditorCloseObservation>,
    ) -> Option<(NodeId, ProcessorEditorCloseObservation)> {
        self.saved.retain(|id, _| current.contains_key(id));
        let pending = |(id, observation): &(&NodeId, &ProcessorEditorCloseObservation)| {
            observation.close_generation != 0 && self.saved.get(*id) != Some(*observation)
        };
        // One export per poll bounds control-thread latency; a broken vendor must not
        // starve another closed editor forever. Failed generations remain retryable.
        let candidate = current
            .iter()
            .filter(pending)
            .find(|(id, _)| self.last_attempt.as_ref().is_none_or(|last| *id > last))
            .or_else(|| current.iter().find(pending))
            .map(|(id, observation)| (id.clone(), *observation));
        self.last_attempt = candidate.as_ref().map(|(id, _)| id.clone());
        candidate
    }
}

/// Dropped before the audio worker so final native exports still have a live owner.
/// Snapshot IPC has a deadline; filesystem I/O and a concurrent control mutation
/// can still delay joining. This owner never abandons an in-flight file replacement.
/// This is never constructed or destroyed on the real-time audio thread.
pub struct ControlMaintenance {
    observing: Arc<AtomicBool>,
    stop: Option<mpsc::Sender<()>>,
    thread: Option<thread::JoinHandle<Result<(), ProtocolError>>>,
    // Deliberately retain the controller through the final shutdown checkpoint.
    _controller: Arc<EngineGraphController>,
}

impl EngineGraphController {
    /// Reclaims retired recording sessions and checkpoints closed native editors off-RT.
    /// # Errors
    /// Returns a thread creation error without starting a detached observer.
    pub fn start_control_maintenance(self: &Arc<Self>) -> Result<ControlMaintenance, String> {
        let (stop, receive) = mpsc::channel();
        let weak = Arc::downgrade(self);
        let observing = Arc::new(AtomicBool::new(true));
        let worker_observing = observing.clone();
        let thread = thread::Builder::new()
            .name("mixrelay-control-maintenance".into())
            .spawn(move || {
                let mut ledger = CloseLedger::default();
                while matches!(
                    receive.recv_timeout(Duration::from_millis(500)),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    if !worker_observing.load(Ordering::Acquire) {
                        continue;
                    }
                    let Some(controller) = weak.upgrade() else {
                        return Ok(());
                    };
                    controller.reclaim_recorders();
                    controller.retry_catalog_restore();
                    controller.retry_pause_save();
                    if !controller.catalog_restore.is_pending() {
                        controller.checkpoint_closed_editor(&mut ledger);
                    }
                }
                weak.upgrade().map_or(Ok(()), |controller| {
                    let recording = controller.recorders.finalize_all();
                    let checkpoint = controller.checkpoint_processor_state(None);
                    recording.and(checkpoint)
                })
            })
            .map_err(|error| format!("control maintenance failed to start: {error}"))?;
        Ok(ControlMaintenance {
            observing,
            stop: Some(stop),
            thread: Some(thread),
            _controller: self.clone(),
        })
    }

    fn checkpoint_closed_editor(&self, ledger: &mut CloseLedger) {
        let _mutation = self.mutation.lock();
        let Some(catalog) = self.catalog_candidate(&self.graph.read()) else {
            return;
        };
        let targets = catalog
            .documents()
            .flat_map(|(graph_id, document)| {
                document
                    .nodes
                    .iter()
                    .filter(|node| matches!(node.config, NodeConfig::Provider(_)))
                    .map(move |node| (runtime_node_id(graph_id, &node.id), (graph_id, &node.id)))
            })
            .collect::<BTreeMap<_, _>>();
        let current = targets
            .keys()
            .filter_map(|runtime_id| {
                self.processor_controls
                    .editor_close_observation(runtime_id)
                    .ok()
                    .map(|observation| (runtime_id.clone(), observation))
            })
            .collect();
        let Some((id, observation)) = ledger.next(&current) else {
            return;
        };
        let Some((graph_id, authored_id)) = targets.get(&id) else {
            return;
        };
        let result = self.checkpoint_graph_processor_state_with(
            graph_id,
            authored_id.as_str(),
            |node, timeout| {
                self.processor_controls
                    .snapshot_state_for_editor_close(node, observation, timeout)
            },
        );
        if result.is_ok() {
            ledger.saved.insert(id, observation);
        }
        // Transaction failure is already exposed through the catalog durability warning.
        // Do not consume the observation or spam logs on each retry.
    }
}

impl ControlMaintenance {
    /// Stops admitting periodic restore/checkpoint work without abandoning an
    /// in-flight operation. Final persistence begins only when `finish` is called.
    pub fn stop_observing(&self) {
        self.observing.store(false, Ordering::Release);
    }

    /// Explicit shutdown reports persistence failure to the service caller.
    /// # Errors
    /// Returns a failed final checkpoint or an observer panic after joining the thread.
    pub fn finish(mut self) -> Result<(), String> {
        self.join().inspect_err(|error| {
            tracing::error!(error, "control maintenance shutdown failed");
        })
    }

    fn join(&mut self) -> Result<(), String> {
        self.stop_observing();
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        self.thread.take().map_or(Ok(()), |thread| {
            crate::worker_join::join(thread)?
                .map_err(|error| format!("control shutdown finalization failed: {error:?}"))
        })
    }
}

impl Drop for ControlMaintenance {
    fn drop(&mut self) {
        if let Err(error) = self.join() {
            tracing::error!(error, "control maintenance shutdown failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finalization_preserves_control_worker_panic_reason_and_releases_owner() {
        let controller = Arc::new(EngineGraphController::new(
            Arc::new(mixrelay_audio_engine::ProcessorExchange::new(1)),
            "endpoint",
            mixrelay_audio_graph::RenderFormat {
                sample_rate: 48_000,
                block_frames: 128,
            },
            Arc::new(|| true),
        ));
        let weak = Arc::downgrade(&controller);
        let monitor = ControlMaintenance {
            observing: Arc::new(AtomicBool::new(false)),
            stop: None,
            thread: Some(
                thread::Builder::new()
                    .name("failed-control-worker".into())
                    .spawn(|| panic!("injected checkpoint failure"))
                    .unwrap(),
            ),
            _controller: controller,
        };

        assert_eq!(
            monitor.finish(),
            Err("failed-control-worker panicked: injected checkpoint failure".into())
        );
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn monitor_joins_and_releases_its_controller_without_a_desktop_client() {
        let controller = Arc::new(EngineGraphController::new(
            Arc::new(mixrelay_audio_engine::ProcessorExchange::new(1)),
            "endpoint",
            mixrelay_audio_graph::RenderFormat {
                sample_rate: 48_000,
                block_frames: 128,
            },
            Arc::new(|| true),
        ));
        let weak = Arc::downgrade(&controller);
        let monitor = controller.start_control_maintenance().unwrap();
        drop(controller);
        assert!(weak.upgrade().is_some());
        monitor.finish().unwrap();
        assert!(weak.upgrade().is_none());
    }

    fn observation(instance_id: u64, close_generation: u64) -> ProcessorEditorCloseObservation {
        ProcessorEditorCloseObservation {
            instance_id,
            close_generation,
        }
    }

    #[test]
    fn retries_failed_closes_without_starving_other_editors() {
        let mut ledger = CloseLedger::default();
        let current = BTreeMap::from([
            (NodeId::from("a"), observation(1, 1)),
            (NodeId::from("b"), observation(2, 1)),
        ]);
        let first = ledger.next(&current).unwrap();
        assert_eq!(first.0.as_str(), "a"); // Failed export is deliberately not acknowledged.
        let second = ledger.next(&current).unwrap();
        assert_eq!(second.0.as_str(), "b");
        ledger.saved.insert(second.0, second.1);
        assert_eq!(ledger.next(&current), Some(first.clone()));
        ledger.saved.insert(first.0, first.1);
        assert!(ledger.next(&current).is_none());
    }

    #[test]
    fn observes_new_close_or_worker_instance_and_prunes_removed_nodes() {
        let mut ledger = CloseLedger::default();
        let id = NodeId::from("a");
        assert!(
            ledger
                .next(&BTreeMap::from([(id.clone(), observation(1, 0))]))
                .is_none()
        );
        ledger.saved.insert(id.clone(), observation(1, 1));
        for token in [observation(1, 2), observation(2, 1)] {
            assert_eq!(
                ledger.next(&BTreeMap::from([(id.clone(), token)])),
                Some((id.clone(), token))
            );
        }
        assert!(ledger.next(&BTreeMap::new()).is_none());
        assert!(ledger.saved.is_empty());
    }
}
