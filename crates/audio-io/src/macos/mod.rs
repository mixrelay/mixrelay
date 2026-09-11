//! Synchronous macOS device discovery for control-plane identity resolution.
use crate::{AudioDeviceDescriptor, DeviceSelector, HostAudioBackendError};

mod coreaudio;

#[derive(Debug, Clone)]
struct DeviceInfo {
    descriptor: AudioDeviceDescriptor,
    alive: bool,
    hidden: bool,
}

trait InventoryReader {
    fn device_ids(&self) -> Result<Vec<u32>, HostAudioBackendError>;
    fn default_input(&self) -> Option<u32>;
    fn default_output(&self) -> Option<u32>;
    fn device(&self, id: u32) -> Option<DeviceInfo>;
    fn resolve_uid(&self, uid: &str) -> Result<Option<u32>, HostAudioBackendError>;
}

#[derive(Default)]
struct SelectedDevice {
    explicit: bool,
    default_input: bool,
    default_output: bool,
}

/// Reads only the selected device identities, without enumerating other devices.
///
/// Default aliases and repeated UIDs share one descriptor. Explicit UIDs may
/// select process-private devices; hidden devices are not selected by a default
/// alias alone. Missing devices are omitted without substituting another device.
/// Like [`devices`], this performs synchronous control-thread OS reads, never DSP work.
///
/// # Errors
/// Returns invalid UID or native UID-translation errors. A selected device that
/// disappears during property observation is omitted.
pub fn selected_devices<'a>(
    selectors: impl IntoIterator<Item = &'a DeviceSelector>,
) -> Result<Vec<AudioDeviceDescriptor>, HostAudioBackendError> {
    selected_inventory(&coreaudio::CoreAudioReader, selectors)
}

fn selected_inventory<'a>(
    reader: &impl InventoryReader,
    selectors: impl IntoIterator<Item = &'a DeviceSelector>,
) -> Result<Vec<AudioDeviceDescriptor>, HostAudioBackendError> {
    // Resolve identities first so explicit/default aliases observe geometry once.
    let mut selected = std::collections::BTreeMap::<u32, SelectedDevice>::new();
    for selector in selectors {
        let id = match selector {
            DeviceSelector::PersistentUid { uid } => reader.resolve_uid(uid)?,
            DeviceSelector::DefaultInput => reader.default_input(),
            DeviceSelector::DefaultOutput => reader.default_output(),
        };
        if let Some(id) = id.filter(|id| *id != 0) {
            let flags = selected.entry(id).or_default();
            match selector {
                DeviceSelector::PersistentUid { .. } => flags.explicit = true,
                DeviceSelector::DefaultInput => flags.default_input = true,
                DeviceSelector::DefaultOutput => flags.default_output = true,
            }
        }
    }
    Ok(selected
        .into_iter()
        .filter_map(|(id, selection)| {
            let info = reader.device(id)?;
            let mut device = info.descriptor;
            if !info.alive || (info.hidden && !selection.explicit) || device.uid.is_empty() {
                return None;
            }
            device.is_default_input = selection.default_input;
            device.is_default_output = selection.default_output;
            Some(device)
        })
        .collect())
}

/// Reads visible, alive physical and virtual devices from `CoreAudio`.
///
/// Returns owned device UIDs, names, input/output channel counts and current
/// default flags. Resolve graph selectors using [`crate::DeviceSelector::resolve`]
/// against this same returned inventory so default aliases share persistent
/// device identity. No native resource is retained after the call.
///
/// Call on a control thread: discovery performs synchronous OS reads and bounded
/// allocations, and is not suitable for an audio callback. A device disappearing
/// while its properties are read is omitted, matching desktop discovery.
///
/// # Errors
/// Returns a backend error if the system device list cannot be read or its
/// returned size is invalid. Individual unavailable device properties do not
/// cause another device or a fabricated channel count to be substituted.
pub fn devices() -> Result<Vec<AudioDeviceDescriptor>, HostAudioBackendError> {
    inventory(&coreaudio::CoreAudioReader)
}

fn inventory(
    reader: &impl InventoryReader,
) -> Result<Vec<AudioDeviceDescriptor>, HostAudioBackendError> {
    let ids = reader.device_ids()?;
    let default_input = reader.default_input();
    let default_output = reader.default_output();
    let mut devices = Vec::with_capacity(ids.len());
    for id in ids {
        let Some(info) = reader.device(id) else {
            continue;
        };
        let mut device = info.descriptor;
        if !info.alive
            || info.hidden
            || device.uid.is_empty()
            || device.display_name.is_empty()
            || (device.input_channels == 0 && device.output_channels == 0)
        {
            continue;
        }
        device.is_default_input = default_input == Some(id);
        device.is_default_output = default_output == Some(id);
        devices.push(device);
    }
    Ok(devices)
}

#[cfg(test)]
mod tests;
