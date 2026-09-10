use super::*;

#[test]
fn topology_loss_bypasses_periodic_delay_but_not_publication_ownership() {
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};
    let status = Arc::new(mixrelay_engine_service::session::AdapterRuntimeStatus::new(
        crate::macos::adapter_profile(),
    ));
    let resources = MacDevices::new(status, 48_000, 32);
    let later = Instant::now() + Duration::from_mins(1);
    *resources.next_group_check.lock().unwrap() = later;
    resources.reclaim();
    assert_eq!(*resources.next_group_check.lock().unwrap(), later);
    let reserved = resources.handoff.reserve_format().unwrap();
    resources.request_recovery();
    resources.reclaim();
    assert!(resources.recovery_requested.load(Ordering::Acquire));
    assert_eq!(*resources.next_group_check.lock().unwrap(), later);
    drop(reserved);
    resources.reclaim();
    assert!(!resources.recovery_requested.load(Ordering::Acquire));
    let next = *resources.next_group_check.lock().unwrap();
    assert!(next < later);
    assert!(next <= Instant::now() + Duration::from_secs(2));
    resources.reclaim();
    assert_eq!(*resources.next_group_check.lock().unwrap(), next);
}

#[test]
fn pending_bridge_is_inactive_without_hiding_a_ready_owner_transfer_failure() {
    use std::sync::atomic::Ordering;
    let (output, context) = super::super::tests::synthetic_output(2, 48_000, 48_000);
    let devices = DeviceSet(
        BTreeMap::from([("output".into(), output.clone())]),
        BTreeMap::new(),
    );
    output.ready_rate.store(0, Ordering::Release);
    assert!(!output.failed.load(Ordering::Acquire));
    assert!(devices.is_inactive("output"));
    assert!(!devices.is_offline("output"));
    assert!(!devices.is_inactive("unknown"));
    let transfer = devices.write("output", &[0.5; 64]).unwrap();
    assert_eq!(transfer.frames, 32);
    assert_eq!(transfer.activity, InputActivity::Inactive);
    output.ready_rate.store(48_000, Ordering::Release);
    assert!(!devices.is_inactive("output"));
    assert_eq!(devices.clock_synchronized("output"), Some(false));
    // A live transfer still uses its real native result, including invalid widths.
    assert_eq!(devices.write("output", &[0.5; 3]).unwrap().frames, 0);
    drop((devices, output, context));
}

fn selected(uid: &str, input: bool) -> Spec {
    Spec {
        selector: DeviceSelector::PersistentUid { uid: uid.into() },
        input,
        channels: if input { 1 } else { 2 },
    }
}

fn inventory() -> Vec<AudioDeviceDescriptor> {
    vec![
        AudioDeviceDescriptor {
            uid: "mic".into(),
            display_name: "Microphone".into(),
            input_channels: 1,
            output_channels: 0,
            is_default_input: true,
            is_default_output: false,
        },
        AudioDeviceDescriptor {
            uid: "speaker".into(),
            display_name: "Speaker".into(),
            input_channels: 0,
            output_channels: 2,
            is_default_input: false,
            is_default_output: true,
        },
    ]
}

#[test]
fn missing_output_does_not_replace_uid_or_remove_healthy_input() {
    let desired = BTreeMap::from([
        ("input".into(), selected("mic", true)),
        ("output".into(), selected("headphone", false)),
    ]);
    let presence = Presence::observe(&desired, &inventory()).unwrap();
    assert_eq!(aggregate_members(&presence.online), ["mic"]);
    assert_eq!(presence.offline["output"], desired["output"]);
    assert_eq!(desired.len(), 2);
    let mut returned = inventory();
    returned[1].uid = "headphone".into();
    let restored = Presence::observe(&desired, &returned).unwrap();
    assert!(restored.offline.is_empty());
    assert_eq!(aggregate_members(&restored.online), ["headphone", "mic"]);
}

#[test]
fn present_bad_geometry_and_invalid_selection_are_not_unplugged_devices() {
    let mut spec = selected("mic", true);
    spec.channels = 2;
    assert!(Presence::observe(&BTreeMap::from([("input".into(), spec)]), &inventory()).is_err());
    let mut spec = selected("missing", false);
    spec.channels = 0;
    assert!(Presence::observe(&BTreeMap::from([("output".into(), spec)]), &[]).is_err());
    let mut spec = selected("missing", false);
    spec.selector = DeviceSelector::DefaultInput;
    assert!(Presence::observe(&BTreeMap::from([("output".into(), spec)]), &[]).is_err());
}

#[test]
fn default_absence_stays_offline_and_follows_only_the_returned_default() {
    let mut spec = selected("unused", false);
    spec.selector = DeviceSelector::DefaultOutput;
    let desired = BTreeMap::from([("output".into(), spec)]);
    assert!(Presence::observe(&desired, &[]).unwrap().online.is_empty());
    let mut devices = inventory();
    devices[1].is_default_output = false;
    assert!(
        Presence::observe(&desired, &devices)
            .unwrap()
            .online
            .is_empty()
    );
    devices[1].is_default_output = true;
    assert!(
        Presence::observe(&desired, &devices)
            .unwrap()
            .offline
            .is_empty()
    );
}

#[test]
fn offline_transfers_silence_and_discard_complete_quanta_without_native_owners() {
    let offline = BTreeMap::from([
        ("input".into(), selected("mic", true)),
        ("output".into(), selected("headphone", false)),
    ]);
    let devices = DeviceSet(BTreeMap::new(), offline);
    for frames in [32, 256, 2048] {
        let mut input = vec![0.75; frames];
        let result = devices.read("input", &mut input).unwrap();
        assert_eq!(result.frames, u32::try_from(frames).unwrap());
        assert_eq!(result.activity, InputActivity::Inactive);
        assert!(input.iter().all(|sample| *sample == 0.0));
        let result = devices.write("output", &vec![0.25; frames * 2]).unwrap();
        assert_eq!(result.frames, u32::try_from(frames).unwrap());
        assert!(!result.discontinuity);
    }
    assert!(devices.is_offline("input"));
    assert!(devices.read("application", &mut [1.0; 32]).is_none());
    assert!(devices.hardware_clock_progress().is_none());
}

#[test]
fn presence_changes_publish_once_and_keep_healthy_owner_identity() {
    let (mic, context) = super::super::tests::synthetic_output(1, 48_000, 48_000);
    let desired: BTreeMap<String, Spec> = BTreeMap::from([
        ("input".into(), selected("mic", true)),
        ("output".into(), selected("headphone", false)),
    ]);
    let devices = BTreeMap::from([("input".into(), mic.clone())]);
    let mut cache: Cache = desired
        .iter()
        .map(|(id, spec)| {
            (
                id.clone(),
                (
                    spec.clone(),
                    devices.get(id).map_or_else(Weak::new, Arc::downgrade),
                ),
            )
        })
        .collect();
    assert!(unchanged(&cache, &desired, &devices));
    let (headphone, output_context) = super::super::tests::synthetic_output(2, 48_000, 48_000);
    let mut returned = devices.clone();
    returned.insert("output".into(), headphone.clone());
    assert!(!unchanged(&cache, &desired, &returned));
    cache.get_mut("output").unwrap().1 = Arc::downgrade(&headphone);
    assert!(unchanged(&cache, &desired, &returned));
    assert!(!unchanged(&cache, &desired, &devices));
    assert!(Arc::ptr_eq(&returned["input"], &mic));
    drop((
        returned,
        devices,
        cache,
        headphone,
        mic,
        context,
        output_context,
    ));
}
