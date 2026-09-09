use std::collections::BTreeMap;

use crate::{DeviceDirection, DeviceSelector};

use super::{AudioDeviceDescriptor, DeviceInfo, HostAudioBackendError, InventoryReader, inventory};

#[derive(Default)]
struct FakeReader {
    devices: BTreeMap<u32, DeviceInfo>,
    input: Option<u32>,
    output: Option<u32>,
    failed: bool,
}

impl InventoryReader for FakeReader {
    fn device_ids(&self) -> Result<Vec<u32>, HostAudioBackendError> {
        if self.failed {
            return Err(HostAudioBackendError::BackendFailure {
                message: "injected discovery failure".into(),
            });
        }
        // 999 models a device that vanished after the system ID list was read.
        Ok(self.devices.keys().copied().chain([999]).collect())
    }
    fn default_input(&self) -> Option<u32> {
        self.input
    }
    fn default_output(&self) -> Option<u32> {
        self.output
    }
    fn device(&self, id: u32) -> Option<DeviceInfo> {
        self.devices.get(&id).cloned()
    }
}

fn info(uid: &str, input: u32, output: u32) -> DeviceInfo {
    DeviceInfo {
        descriptor: AudioDeviceDescriptor {
            uid: uid.into(),
            display_name: format!("Name {uid}"),
            input_channels: input,
            output_channels: output,
            is_default_input: false,
            is_default_output: false,
        },
        alive: true,
        hidden: false,
    }
}

#[test]
fn default_and_explicit_uid_resolve_to_same_physical_capture_owner() {
    let reader = FakeReader {
        devices: [
            (7, info("physical-interface", 7, 2)),
            (8, info("output-only", 0, 8)),
        ]
        .into(),
        input: Some(7),
        output: Some(7),
        failed: false,
    };
    let devices = inventory(&reader).unwrap();
    let explicit = DeviceSelector::PersistentUid {
        uid: "physical-interface".into(),
    };
    let by_default = DeviceSelector::DefaultInput
        .resolve(&devices, DeviceDirection::Input, 7)
        .unwrap();
    let by_uid = explicit
        .resolve(&devices, DeviceDirection::Input, 7)
        .unwrap();
    assert!(std::ptr::eq(by_default, by_uid));
    assert_eq!(by_uid.output_channels, 2);
    assert!(
        explicit
            .resolve(&devices, DeviceDirection::Input, 8)
            .is_err()
    );
    assert!(
        DeviceSelector::DefaultOutput
            .resolve(&devices, DeviceDirection::Output, 2)
            .is_ok()
    );
}

#[test]
fn absent_and_filtered_defaults_do_not_fall_back_to_other_devices() {
    let mut reader = FakeReader::default();
    reader.devices.insert(1, info("live", 1, 2));
    let mut dead = info("dead", 2, 2);
    dead.alive = false;
    reader.devices.insert(2, dead);
    let mut hidden = info("private-aggregate", 2, 2);
    hidden.hidden = true;
    reader.devices.insert(3, hidden);
    reader.devices.insert(4, info("", 2, 2));
    reader.devices.insert(5, info("no-streams", 0, 0));
    let mut unnamed = info("unnamed", 2, 2);
    unnamed.descriptor.display_name.clear();
    reader.devices.insert(6, unnamed);
    reader.input = Some(2);
    reader.output = Some(999);
    let devices = inventory(&reader).unwrap();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].uid, "live");
    assert!(
        DeviceSelector::DefaultInput
            .resolve(&devices, DeviceDirection::Input, 1)
            .is_err()
    );
    assert!(
        DeviceSelector::DefaultOutput
            .resolve(&devices, DeviceDirection::Output, 1)
            .is_err()
    );
    reader.failed = true;
    assert!(inventory(&reader).is_err());
}

#[test]
fn actual_coreaudio_inventory_has_resolvable_device_identity_without_mutation() {
    let devices = super::devices().expect("actual CoreAudio inventory must be readable");
    assert!(
        !devices.is_empty(),
        "this macOS host must expose at least one audio device"
    );
    let mut uids = std::collections::BTreeSet::new();
    for device in &devices {
        assert!(uids.insert(&device.uid), "device UIDs must be unique");
        assert!(!device.uid.is_empty() && !device.display_name.is_empty());
        assert!(device.input_channels > 0 || device.output_channels > 0);
        let direction = if device.input_channels > 0 {
            DeviceDirection::Input
        } else {
            DeviceDirection::Output
        };
        let selected = DeviceSelector::PersistentUid {
            uid: device.uid.clone(),
        };
        assert!(std::ptr::eq(
            selected.resolve(&devices, direction, 1).unwrap(),
            device
        ));
    }
    assert!(
        devices
            .iter()
            .filter(|device| device.is_default_input)
            .count()
            <= 1
    );
    assert!(
        devices
            .iter()
            .filter(|device| device.is_default_output)
            .count()
            <= 1
    );
}
