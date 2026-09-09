//! Reconcile desired selections against current presence without deleting graph intent.
use super::{
    Aggregate, Cache, Device, DeviceSet, MacDevices, Publication, Spec, aggregate_members,
    backend_error,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Weak},
};
use tonenari_audio_graph::DeviceSelector;
use tonenari_audio_io::process::{InputActivity, TransferOutcome};
use tonenari_audio_io::{AudioDeviceDescriptor, DeviceDirection, HostAudioBackendError};

pub(super) fn offline_transfer(samples: usize, channels: usize) -> TransferOutcome {
    TransferOutcome {
        frames: u32::try_from(samples / channels.max(1)).unwrap_or(0),
        discontinuity: false,
        activity: InputActivity::Inactive,
    }
}

struct Presence {
    online: BTreeMap<String, Spec>,
    offline: BTreeMap<String, Spec>,
}
impl Presence {
    fn observe(
        desired: &BTreeMap<String, Spec>,
        inventory: &[AudioDeviceDescriptor],
    ) -> Result<Self, HostAudioBackendError> {
        let mut presence = Self {
            online: BTreeMap::new(),
            offline: BTreeMap::new(),
        };
        for (id, spec) in desired {
            spec.native_uid().map_err(backend_error)?;
            if !(1..=64).contains(&spec.channels) {
                return Err(backend_error("invalid device channel count"));
            }
            let mut candidates = inventory.iter().filter(|device| match &spec.selector {
                DeviceSelector::PersistentUid { uid } => device.uid == *uid,
                DeviceSelector::DefaultInput => device.is_default_input,
                DeviceSelector::DefaultOutput => device.is_default_output,
            });
            let first = candidates.next();
            if candidates.next().is_some() {
                return Err(backend_error("ambiguous device selection"));
            }
            let found = first.is_some_and(|device| {
                if spec.input {
                    device.input_channels > 0
                } else {
                    device.output_channels > 0
                }
            });
            if found {
                // A present device with an invalid channel request is an error,
                // not evidence that the device was unplugged.
                spec.selector.resolve(
                    inventory,
                    if spec.input {
                        DeviceDirection::Input
                    } else {
                        DeviceDirection::Output
                    },
                    spec.channels,
                )?;
                presence.online.insert(id.clone(), spec.clone());
            } else {
                presence.offline.insert(id.clone(), spec.clone());
            }
        }
        Ok(presence)
    }
}

impl MacDevices {
    pub(super) fn prepare_devices(
        &self,
        cache: &mut Cache,
        desired: BTreeMap<String, Spec>,
        publication: Publication,
    ) -> Result<(), HostAudioBackendError> {
        // Inventory errors abort preparation; an empty successful inventory is
        // different and leaves every desired selection explicitly offline.
        let inventory = selected_inventory(&desired)?;
        let presence = Presence::observe(&desired, &inventory)?;
        let group = prepare_group(cache, &presence.online)?;
        let devices = self.open_present(cache, &presence.online, group.as_ref())?;
        if unchanged(cache, &desired, &devices) {
            return Ok(());
        }
        let next_cache = desired
            .into_iter()
            .map(|(id, spec)| {
                let owner = devices.get(&id).map_or_else(Weak::new, Arc::downgrade);
                (id, (spec, owner))
            })
            .collect();
        let online_bindings = devices.len();
        let offline_bindings = presence.offline.len();
        self.handoff
            .publish(publication, DeviceSet(devices, presence.offline))
            .map_err(|_| backend_error("CoreAudio resource publication failed"))?;
        *cache = next_cache;
        tracing::info!(
            online_bindings,
            offline_bindings,
            "CoreAudio device presence reconciled"
        );
        Ok(())
    }

    fn open_present(
        &self,
        cache: &Cache,
        online: &BTreeMap<String, Spec>,
        group: Option<&Arc<Aggregate>>,
    ) -> Result<BTreeMap<String, Arc<Device>>, HostAudioBackendError> {
        let mut devices = BTreeMap::new();
        for (id, spec) in online {
            let selected = matches!(spec.selector, DeviceSelector::PersistentUid { .. })
                .then(|| group.cloned())
                .flatten();
            let existing = cache
                .get(id)
                .filter(|(old, _)| old == spec)
                .and_then(|(_, weak)| weak.upgrade())
                .filter(|device| same_group(device.native.group.as_ref(), selected.as_ref()));
            let device = match existing {
                Some(device) => device,
                None => Arc::new(
                    Device::open_with_group(
                        spec,
                        self.format.current().sample_rate,
                        selected,
                        self.format.current().block_frames,
                    )
                    .map_err(backend_error)?,
                ),
            };
            devices.insert(id.clone(), device);
        }
        Ok(devices)
    }
}

fn same_group(left: Option<&Arc<Aggregate>>, right: Option<&Arc<Aggregate>>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => Arc::ptr_eq(left, right),
        (None, None) => true,
        _ => false,
    }
}

fn selected_inventory(
    desired: &BTreeMap<String, Spec>,
) -> Result<Vec<AudioDeviceDescriptor>, HostAudioBackendError> {
    let mut inventory = tonenari_audio_io::macos::devices()?;
    for spec in desired.values() {
        let DeviceSelector::PersistentUid { uid } = &spec.selector else {
            continue;
        };
        if inventory.iter().any(|device| device.uid == *uid) {
            continue;
        }
        // Explicit process-private selections are intentionally absent from
        // the public UI inventory. Resolve only that exact UID in this process.
        if let Some(info) = super::group_plan::GroupPlan::member(uid)
            && info.device != 0
            && info.alive == 1
        {
            inventory.push(AudioDeviceDescriptor {
                uid: uid.clone(),
                display_name: uid.clone(),
                input_channels: info.input,
                output_channels: info.output,
                is_default_input: false,
                is_default_output: false,
            });
        }
    }
    Ok(inventory)
}

fn prepare_group(
    cache: &Cache,
    online: &BTreeMap<String, Spec>,
) -> Result<Option<Arc<Aggregate>>, HostAudioBackendError> {
    let members = aggregate_members(online);
    if let Some(group) = cache
        .values()
        .filter_map(|(_, weak)| weak.upgrade())
        .find_map(|device| {
            device
                .native
                .group
                .clone()
                .filter(|group| group.members == members && group.ready())
        })
    {
        return Ok(Some(group));
    }
    Aggregate::create(members).map_err(backend_error)
}

fn unchanged(
    cache: &Cache,
    desired: &BTreeMap<String, Spec>,
    devices: &BTreeMap<String, Arc<Device>>,
) -> bool {
    cache.len() == desired.len()
        && desired.iter().all(|(id, spec)| {
            cache.get(id).is_some_and(|(old, weak)| {
                old == spec
                    && match (weak.upgrade(), devices.get(id)) {
                        (Some(old), Some(new)) => Arc::ptr_eq(&old, new),
                        (None, None) => true,
                        _ => false,
                    }
            })
        })
}

#[cfg(test)]
mod tests;
