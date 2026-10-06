//! Graph device selection and deterministic aggregate membership, without IO.
use mixrelay_audio_graph::{DeviceSelector, GraphDocument, NodeConfig};
use mixrelay_engine_service::resource_requirements::DeviceRequirements;
use mixrelay_macos_transport::endpoint_identity::{endpoint_uid, parse_endpoint_id};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::CString,
};
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Spec {
    pub(super) selector: DeviceSelector,
    pub(super) input: bool,
    pub(super) channels: u16,
}
impl Spec {
    pub(super) fn native_uid(&self) -> Result<CString, String> {
        let uid = match (&self.selector, self.input) {
            (DeviceSelector::PersistentUid { uid }, _) if !uid.is_empty() => uid.as_str(),
            (DeviceSelector::DefaultInput, true) | (DeviceSelector::DefaultOutput, false) => "",
            _ => return Err("wrong device selector direction or empty UID".into()),
        };
        CString::new(uid).map_err(|error| error.to_string())
    }
}
pub(super) fn specs(document: &GraphDocument) -> BTreeMap<String, Spec> {
    let requirements = DeviceRequirements::from_document(document);
    let mut selected: BTreeMap<_, _> = requirements
        .inputs
        .into_iter()
        .map(|(id, config)| {
            (
                id,
                Spec {
                    selector: config.selector,
                    input: true,
                    channels: config.output_channels,
                },
            )
        })
        .chain(requirements.outputs.into_iter().map(|(id, config)| {
            (
                id,
                Spec {
                    selector: config.selector,
                    input: false,
                    channels: config.input_channels,
                },
            )
        }))
        .collect();
    for node in &document.nodes {
        let NodeConfig::VirtualEndpointTransport(endpoint) = &node.config else {
            continue;
        };
        let Some(id) = parse_endpoint_id(endpoint.id.as_str()) else {
            continue;
        };
        for (input, stream, suffix) in [
            (true, endpoint.capture, "/capture"),
            (false, endpoint.render, "/render"),
        ] {
            if let Some(stream) = stream {
                selected.insert(
                    format!("{}{suffix}", node.id.as_str()),
                    Spec {
                        selector: DeviceSelector::PersistentUid {
                            uid: endpoint_uid(id),
                        },
                        input,
                        channels: stream.channels,
                    },
                );
            }
        }
    }
    selected
}

/// Fixed selections only. A default selector continues following the OS default
/// rather than accidentally pinning yesterday's device inside an aggregate.
/// Prefer an output as primary; ordering is stable and shared with channel maps.
pub(super) fn aggregate_members(specs: &BTreeMap<String, Spec>) -> Vec<String> {
    let mut uids = BTreeSet::new();
    let mut primary = None;
    for spec in specs.values() {
        if let DeviceSelector::PersistentUid { uid } = &spec.selector {
            uids.insert(uid.clone());
            if !spec.input && primary.is_none() {
                primary = Some(uid.clone());
            }
        }
    }
    let primary = primary.or_else(|| uids.first().cloned());
    let mut members = Vec::new();
    if let Some(primary) = primary {
        uids.remove(&primary);
        members.push(primary);
    }
    members.extend(uids);
    members
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixrelay_audio_graph::{
        AudioDeviceInputConfig, AudioDeviceOutputConfig, NodeConfig, NodeSpec,
    };

    #[test]
    fn driver_clients_attach_native_capture_input_and_render_output_to_one_stable_uid() {
        let mut document: GraphDocument = serde_json::from_value(serde_json::json!({
            "schemaVersion": 2, "revision": 1, "nodes": [{
                "id":"driver", "type":"virtual_endpoint",
                "config":{"id":"01010101-0101-0101-0101-010101010101","displayName":"Fixture",
                    "reference":{"render":true,"capture":true},
                    "render":{"channels":7,"channelLayout":"discrete"},
                    "capture":{"channels":3,"channelLayout":"discrete"},
                    "supportedSampleRates":[48000],"defaultSampleRate":48000}
            }], "edges":[]
        }))
        .unwrap();
        if let NodeConfig::VirtualEndpoint(endpoint) = &document.nodes[0].config {
            document.nodes[0].config = NodeConfig::VirtualEndpointTransport(endpoint.spec.clone());
        }
        let selected = specs(&document);
        assert_eq!(selected.len(), 2);
        assert!(selected["driver/capture"].input);
        assert_eq!(selected["driver/capture"].channels, 3);
        assert!(!selected["driver/render"].input);
        assert_eq!(selected["driver/render"].channels, 7);
        let uid = endpoint_uid([1; 16]);
        assert_eq!(
            selected["driver/capture"]
                .native_uid()
                .unwrap()
                .to_str()
                .unwrap(),
            uid
        );
        assert_eq!(
            selected["driver/render"]
                .native_uid()
                .unwrap()
                .to_str()
                .unwrap(),
            uid
        );
        assert_eq!(aggregate_members(&selected), vec![uid]);
        document.nodes.clear();
        assert!(specs(&document).is_empty());
    }

    #[test]
    fn shared_requirements_preserve_native_direction_width_and_output_clock_priority() {
        let document = GraphDocument::new(
            1,
            vec![
                NodeSpec {
                    control: mixrelay_audio_graph::NodeControl::default(),
                    id: "mic".into(),
                    label: None,
                    config: NodeConfig::AudioDeviceInput(AudioDeviceInputConfig {
                        selector: DeviceSelector::PersistentUid {
                            uid: "a-mic".into(),
                        },
                        output_channels: 8,
                        gain_db: -6.0,
                    }),
                },
                NodeSpec {
                    control: mixrelay_audio_graph::NodeControl::default(),
                    id: "speaker".into(),
                    label: None,
                    config: NodeConfig::AudioDeviceOutput(AudioDeviceOutputConfig {
                        selector: DeviceSelector::PersistentUid {
                            uid: "z-speaker".into(),
                        },
                        input_channels: 2,
                    }),
                },
            ],
            vec![],
        );
        let selected = specs(&document);
        assert!(selected["mic"].input);
        assert_eq!(selected["mic"].channels, 8);
        assert!(!selected["speaker"].input);
        assert_eq!(selected["speaker"].channels, 2);
        assert_eq!(
            selected["mic"].native_uid().unwrap().to_str().unwrap(),
            "a-mic"
        );
        assert_eq!(aggregate_members(&selected), ["z-speaker", "a-mic"]);
    }
}
