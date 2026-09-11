use mixrelay_audio_graph::NodeControl;
use serde::{Deserialize, Serialize};

/// Explicit node state changes guarded by the current engine publication revision.
/// Omitted fields retain their authored values; an empty patch is invalid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeControlRequest {
    pub expected_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub muted: Option<bool>,
    /// Absolute graph-local gain in decibels; supported only where discovery exposes gain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gain_db: Option<f32>,
}

/// A recording command fenced to the graph revision observed by its caller.
/// The recorder's file/transport request remains independent of graph publication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecorderControlRequest {
    pub expected_revision: u64,
    pub request: mixrelay_audio_recorder::RecorderRequest,
    /// Optional observed take fence. Remote controllers must supply this so a delayed
    /// stop/resume cannot operate on a newer recording in the same graph revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_recording: Option<ObservedRecording>,
}

/// Stable take identity and transport state, excluding continuously advancing frames.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObservedRecording {
    pub state: mixrelay_audio_recorder::RecorderState,
    pub path: Option<String>,
}

/// Controls discovered from one current authored graph, never a static action catalog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GraphNodeControls {
    pub graph_id: String,
    pub revision: u64,
    pub nodes: Vec<NodeControlDescriptor>,
}

/// One stable graph-local target and its persisted control state.
/// Capabilities describe supported commands, not current native-device availability.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeControlDescriptor {
    pub node_id: String,
    pub label: Option<String>,
    pub kind: String,
    pub control: NodeControl,
    pub capabilities: NodeControlCapabilities,
    /// Current gain, or absent when this node has no graph-local gain control.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gain_db: Option<f32>,
}

/// Existing control surfaces available for the addressed node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeControlCapabilities {
    pub power: bool,
    pub mute: bool,
    /// The graph-scoped recorder status and record/pause/stop route is supported.
    pub recorder: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn patch_retains_omitted_fields_and_requires_revision() {
        let patch: NodeControlRequest = serde_json::from_value(json!({
            "expectedRevision": 7, "enabled": false
        }))
        .expect("explicit patch");
        assert_eq!(patch.enabled, Some(false));
        assert_eq!(patch.muted, None);
        assert_eq!(
            serde_json::to_value(patch).expect("serialize"),
            json!({
                "expectedRevision": 7, "enabled": false
            })
        );
        for invalid in [
            json!({"enabled":true}),
            json!({"expectedRevision":7,"toggle":true}),
        ] {
            assert!(serde_json::from_value::<NodeControlRequest>(invalid).is_err());
        }
    }

    #[test]
    fn discovery_preserves_graph_identity_and_capabilities() {
        let controls = GraphNodeControls {
            graph_id: "stage".into(),
            revision: 7,
            nodes: vec![NodeControlDescriptor {
                node_id: "recorder".into(),
                label: None,
                kind: "recorder".into(),
                control: NodeControl {
                    enabled: false,
                    muted: true,
                },
                capabilities: NodeControlCapabilities {
                    power: true,
                    mute: true,
                    recorder: true,
                },
                gain_db: None,
            }],
        };
        let value = serde_json::to_value(&controls).expect("serialize controls");
        assert_eq!(value["nodes"][0]["control"]["enabled"], false);
        assert_eq!(
            serde_json::from_value::<GraphNodeControls>(value).expect("round trip"),
            controls
        );
    }

    #[test]
    fn runtime_commands_require_observed_revision_and_reject_unknown_fields() {
        use crate::{ProcessorEditorRequest, ProcessorParameterUpdate};

        assert!(serde_json::from_value::<ProcessorParameterUpdate>(json!({"value":0.5})).is_err());
        assert!(serde_json::from_value::<ProcessorEditorRequest>(json!({"open":true})).is_err());
        assert!(
            serde_json::from_value::<RecorderControlRequest>(json!({"request":{"action":"stop"}}))
                .is_err()
        );
        assert!(
            serde_json::from_value::<RecorderControlRequest>(json!({
                "expectedRevision":7,"request":{"action":"stop"},"toggle":true
            }))
            .is_err()
        );
        let request: RecorderControlRequest = serde_json::from_value(json!({
            "expectedRevision":7,"request":{"action":"stop"}
        }))
        .expect("fenced recorder stop");
        assert_eq!(request.expected_revision, 7);
        assert_eq!(
            request.request.action,
            mixrelay_audio_recorder::RecorderAction::Stop
        );
    }
}
