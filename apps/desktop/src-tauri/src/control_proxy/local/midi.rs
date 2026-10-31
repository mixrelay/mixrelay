//! MIDI and Stream Deck share snapshot fences and the same capture-aware dispatch.
use mixrelay_midi_control::{Action, Binding, Target as MidiTarget};
use serde_json::{Value, json};

use super::{Command, LocalControl, Target};

pub(crate) fn node<'a>(snapshot: &'a Value, target: &MidiTarget) -> Option<&'a Value> {
    snapshot["graphs"]
        .as_array()?
        .iter()
        .find(|graph| graph["id"] == target.graph_id)?["nodes"]
        .as_array()?
        .iter()
        .find(|node| node["nodeId"] == target.node_id)
}

impl LocalControl {
    pub(crate) fn midi_liveness(&self) -> std::sync::Arc<tokio::sync::Notify> {
        std::sync::Arc::clone(&self.client.operation_liveness)
    }

    pub(crate) async fn midi_snapshot(&self) -> Option<Value> {
        self.snapshot()
            .await
            .ok()
            .filter(super::super::ControlProxyResponse::is_success)
            .map(|response| response.body)
    }

    pub(crate) async fn midi_action(
        &self,
        binding: &Binding,
        gain: Option<(f32, Option<f32>)>,
        admission: &crate::midi::admission::Admission<'_>,
    ) -> bool {
        let Some(Some(snapshot)) = admission.while_current(self.midi_snapshot()).await else {
            return false;
        };
        if let Some((_, observed)) = gain {
            let current = node(&snapshot, &binding.target)
                .and_then(|node| serde_json::from_value::<f32>(node["gainDb"].clone()).ok());
            if !matches!((observed, current), (Some(observed), Some(current)) if (observed - current).abs() <= 0.01)
            {
                return false;
            }
        }
        let Some(command) = prepare(&snapshot, binding, gain.map(|(gain, _)| gain)) else {
            return false;
        };
        // Mapping changes and the final write share admission. Once admitted, an
        // accepted mutation finishes once; stale/uncertain writes are never replayed.
        let Some(guard) = admission.enter().await else {
            return false;
        };
        let response = self.execute(command).await;
        drop(guard);
        match response {
            Ok(response) if response.is_success() => {
                if response.body["state"] == "pending" {
                    self.wait_for_midi_revision(&response, admission).await
                } else {
                    true
                }
            }
            Ok(_) => false,
            Err(error) => {
                tracing::warn!(%error, "MIDI control unavailable");
                false
            }
        }
    }

    async fn wait_for_midi_revision(
        &self,
        accepted: &super::super::ControlProxyResponse,
        admission: &crate::midi::admission::Admission<'_>,
    ) -> bool {
        let Some(revision) = accepted.body["revision"].as_u64() else {
            return false;
        };
        loop {
            let Some(Ok(observed)) = admission.while_current(self.snapshot()).await else {
                return false;
            };
            if !observed.is_success() || observed.engine != accepted.engine {
                return false;
            }
            if observed.body["activeRevision"]
                .as_u64()
                .is_some_and(|active| active >= revision)
            {
                return admission.current();
            }
            if admission
                .while_current(tokio::time::sleep(std::time::Duration::from_millis(5)))
                .await
                .is_none()
            {
                return false;
            }
        }
    }
}

fn prepare(snapshot: &Value, binding: &Binding, gain: Option<f32>) -> Option<Command> {
    let target_node = node(snapshot, &binding.target)?;
    let revision = snapshot["revision"].as_u64()?;
    if snapshot["activeRevision"].as_u64()? < revision {
        return None;
    }
    let session = snapshot["session"].as_str()?.to_owned();
    let target = Target {
        graph_id: binding.target.graph_id.clone(),
        node_id: binding.target.node_id.clone(),
    };
    let mut request = json!({"expectedRevision": revision});
    let capabilities = &target_node["capabilities"];
    if binding.action == Action::Power {
        if capabilities["power"] != true {
            return None;
        }
        request["enabled"] = json!(!target_node["control"]["enabled"].as_bool()?);
    } else {
        if !running(snapshot, target_node) {
            return None;
        }
        match binding.action {
            Action::Mute if capabilities["mute"] == true => {
                request["muted"] = json!(!target_node["control"]["muted"].as_bool()?);
            }
            Action::Volume if target_node["gainDb"].is_number() => {
                let gain = gain.filter(|gain| gain.is_finite() && (-60.0..=12.0).contains(gain))?;
                request["gainDb"] = json!(gain);
            }
            Action::Record | Action::Pause | Action::Stop if capabilities["recorder"] == true => {
                let recording = &target_node["recorder"];
                let state = recording["state"].as_str()?;
                let action = match binding.action {
                    Action::Record if state != "recording" => "record",
                    Action::Pause if state == "recording" => "pause",
                    Action::Stop if matches!(state, "recording" | "paused") => "stop",
                    _ => return None,
                };
                request["request"] = json!({"action": action});
                request["expectedRecording"] = json!({"state":state, "path":recording["path"]});
                return Some(Command::Recorder {
                    session,
                    target,
                    request: serde_json::from_value(request).ok()?,
                });
            }
            _ => return None,
        }
    }
    Some(Command::Node {
        session,
        target,
        request: serde_json::from_value(request).ok()?,
    })
}

#[cfg(test)]
mod tests;

fn running(snapshot: &Value, target: &Value) -> bool {
    snapshot["enginePower"]["enabled"] == true
        && snapshot["enginePower"]["transitioning"] == false
        && target["control"]["enabled"] == true
}

pub(crate) fn volume_ready(snapshot: &Value, target: &MidiTarget) -> bool {
    node(snapshot, target).is_some_and(|node| running(snapshot, node) && node["gainDb"].is_number())
        && matches!((snapshot["revision"].as_u64(), snapshot["activeRevision"].as_u64()), (Some(revision), Some(active)) if active >= revision)
}
