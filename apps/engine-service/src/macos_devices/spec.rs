//! Graph device selection and deterministic aggregate membership, without IO.
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::CString,
};
use tonenari_audio_graph::{DeviceSelector, GraphDocument, NodeConfig};
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
    document
        .nodes
        .iter()
        .filter_map(|node| {
            let spec = match &node.config {
                NodeConfig::AudioDeviceInput(config) => Spec {
                    selector: config.selector.clone(),
                    input: true,
                    channels: config.output_channels,
                },
                NodeConfig::AudioDeviceOutput(config) => Spec {
                    selector: config.selector.clone(),
                    input: false,
                    channels: config.input_channels,
                },
                _ => return None,
            };
            Some((node.id.as_str().to_owned(), spec))
        })
        .collect()
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
