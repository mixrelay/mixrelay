use mixrelay_audio_graph::NodeId;
use mixrelay_audio_recorder::{
    RecorderAction, RecorderRegistry, RecorderRequest, RecorderState, RecorderStatus, RecorderTap,
};
use mixrelay_control_protocol::{ErrorCode, ProtocolError};
use parking_lot::Mutex;

pub(super) fn recorder_error(reason: String) -> ProtocolError {
    ProtocolError {
        code: ErrorCode::RecorderFailed,
        parameters: serde_json::json!({"reason": serde_json::Value::String(reason)}),
    }
}

pub(super) struct RecorderSessions {
    registry: RecorderRegistry,
    suspended: Mutex<Vec<SuspendedRecorder>>,
    cleanup_error: Mutex<Option<String>>,
}

struct SuspendedRecorder {
    id: String,
    // The empty paused graph owns no taps. Keep the open take alive until resume.
    _tap: RecorderTap,
    resume: bool,
}

fn recorder_request(action: RecorderAction) -> RecorderRequest {
    RecorderRequest {
        action,
        path: None,
        sample_rate: None,
        encoding: None,
    }
}

impl RecorderSessions {
    pub(super) fn new(registry: RecorderRegistry) -> Self {
        Self {
            registry,
            suspended: Mutex::new(Vec::new()),
            cleanup_error: Mutex::new(None),
        }
    }

    pub(super) fn status(&self, id: &str) -> RecorderStatus {
        self.registry.status(id)
    }

    pub(super) fn control(
        &self,
        id: &str,
        request: RecorderRequest,
        duration_limit: Option<u64>,
    ) -> Result<RecorderStatus, ProtocolError> {
        self.registry
            .command_with_duration_limit(id, request, duration_limit)
            .map_err(recorder_error)
    }

    pub(super) fn stop_inactive(&self, id: &str, graph_id: &str, node_id: &NodeId) {
        if let Err(reason) = self
            .registry
            .command(id, recorder_request(RecorderAction::Stop))
        {
            tracing::error!(%graph_id, %node_id, %reason, "disabled recorder finalization failed");
            *self.cleanup_error.lock() = Some(reason);
        }
    }

    pub(super) fn finalize_all(&self) -> Result<(), ProtocolError> {
        self.registry.stop_all().map_err(recorder_error)
    }

    pub(super) fn cleanup_error(&self) -> Option<String> {
        self.cleanup_error.lock().clone()
    }

    pub(super) fn reclaim(&self) {
        self.suspended.lock().retain(|recorder| {
            matches!(
                self.registry.status(&recorder.id).state,
                RecorderState::Recording | RecorderState::Paused
            )
        });
        if let Err(reason) = self.registry.reap_unused() {
            tracing::error!(reason, "recorder finalization failed");
            *self.cleanup_error.lock() = Some(reason);
        }
    }

    pub(super) fn suspend(&self) -> Result<(), ProtocolError> {
        let mut held = self.suspended.lock();
        *held = self
            .registry
            .open_take_taps()
            .into_iter()
            .map(|(id, tap, state)| SuspendedRecorder {
                id,
                _tap: tap,
                resume: state == RecorderState::Recording,
            })
            .collect();
        for recorder in held.iter().filter(|recorder| recorder.resume) {
            if let Err(reason) = self
                .registry
                .command(&recorder.id, recorder_request(RecorderAction::Pause))
            {
                drop(held);
                let rollback = self.resume();
                return Err(recorder_error(format!(
                    "cannot pause recording: {reason}; rollback: {rollback:?}"
                )));
            }
        }
        Ok(())
    }

    pub(super) fn resume(&self) -> Result<(), ProtocolError> {
        let mut held = self.suspended.lock();
        let mut failures = Vec::new();
        held.retain(|recorder| {
            if !recorder.resume || self.registry.status(&recorder.id).state != RecorderState::Paused
            {
                return false;
            }
            match self
                .registry
                .command(&recorder.id, recorder_request(RecorderAction::Record))
            {
                Ok(_) => false,
                Err(reason) => {
                    failures.push(format!("{}: {reason}", recorder.id));
                    true
                }
            }
        });
        if failures.is_empty() {
            Ok(())
        } else {
            Err(recorder_error(failures.join("; ")))
        }
    }

    #[cfg(test)]
    pub(super) fn registry(&self) -> &RecorderRegistry {
        &self.registry
    }
}
