use std::fs;

use mixrelay_audio_io::{NativeSampleFormat, VirtualEndpointId};
use mixrelay_test_support::FixtureDir;

use super::*;
use crate::test_backend::FakeBackend;

#[test]
fn maintenance_preserves_pause_and_recovers_only_failed_retained_devices() {
    let fixture = FixtureDir::new("driver-maintenance").unwrap();
    let path = fixture.join("drivers.json");
    let (backend, observed) = FakeBackend::ready();
    let mut manager = DriverManager::open(backend, &path).unwrap();
    let mut second = default_device();
    second.spec.id = VirtualEndpointId::from("healthy-other-device");
    second.spec.display_name = "Healthy other device".into();
    manager
        .mutate(DriverCommand::Create {
            expected_revision: manager.revision,
            spec: second.spec,
        })
        .unwrap();
    let settings = manager.settings.clone();
    let inventory = manager.inventory.clone();
    let saved = fs::read(&path).unwrap();
    let applies = observed.lock().applies;
    let readbacks = observed.lock().readbacks;
    observed.lock().state = DriverState::Unavailable; // Non-streaming, with no terminal failure.
    assert!(!manager.maintain().unwrap());
    assert_eq!(observed.lock().applies, applies);
    assert_eq!(observed.lock().readbacks, readbacks);
    assert_eq!(manager.settings, settings);
    assert_eq!(manager.inventory, inventory);
    let failed = inventory[0].spec.id.as_str().to_owned();
    observed.lock().failed_devices.insert(failed.clone());
    assert!(manager.maintain().unwrap());
    assert_eq!(observed.lock().recovered_devices, [failed]);
    assert_eq!(manager.settings, settings);
    assert_eq!(manager.inventory, inventory);
    assert_eq!(fs::read(&path).unwrap(), saved);
    assert!(!manager.maintain().unwrap());
    assert_eq!(observed.lock().applies, applies + 1);
    assert_eq!(observed.lock().readbacks, readbacks + 1);
}

#[test]
fn failed_recovery_retries_same_intent_without_saving_or_reviving_removed_devices() {
    let fixture = FixtureDir::new("driver-maintenance-retry").unwrap();
    let path = fixture.join("drivers.json");
    let (backend, observed) = FakeBackend::ready();
    let mut manager = DriverManager::open(backend, &path).unwrap();
    let desired = manager.inventory.clone();
    let saved = fs::read(&path).unwrap();
    let failed = desired[0].spec.id.as_str().to_owned();
    {
        let mut native = observed.lock();
        native.failed_devices.insert(failed.clone());
        native.state = DriverState::Unavailable;
        native.fail_apply = true;
    }
    assert!(manager.maintain().is_err());
    assert_eq!(manager.inventory, desired);
    assert_eq!(fs::read(&path).unwrap(), saved);
    observed.lock().fail_apply = false;
    assert!(manager.maintain().unwrap());
    manager
        .mutate(DriverCommand::Remove {
            expected_revision: manager.revision,
            endpoint_id: desired[0].spec.id.clone(),
        })
        .unwrap();
    let applies = observed.lock().applies;
    observed.lock().failed_devices.insert(failed);
    assert!(!manager.maintain().unwrap());
    assert!(manager.inventory.is_empty());
    assert!(observed.lock().settings.devices.is_empty());
    assert_eq!(observed.lock().applies, applies);
}

#[test]
fn create_configure_remove_preserves_explicit_empty_after_restart() {
    let fixture = FixtureDir::new("driver-inventory-lifetime").unwrap();
    let path = fixture.join("drivers.json");
    fs::write(&path, br#"{"revision":0,"devices":[]}"#).unwrap();
    let (backend, _) = FakeBackend::ready();
    let mut manager = DriverManager::open(backend, &path).unwrap();
    let mut spec = default_device().spec;
    spec.id = VirtualEndpointId::from("507b3da3-265a-45c7-b8a2-84b6463b913c");
    spec.display_name = "My device".into();
    spec.render = Some(StreamSpec {
        channels: 64,
        channel_layout: ChannelLayout::Discrete,
    });
    spec.capture = None;
    let revision = manager.snapshot().unwrap().revision;
    let created = manager
        .mutate(DriverCommand::Create {
            expected_revision: revision,
            spec,
        })
        .unwrap();
    assert_eq!(
        created.settings.devices[0].spec.render.unwrap().channels,
        64
    );
    let mut spec = created.settings.devices[0].spec.clone();
    spec.display_name = "Renamed".into();
    spec.render.as_mut().unwrap().channels = 32;
    let configured = manager
        .mutate(DriverCommand::Configure {
            expected_revision: created.revision,
            spec,
        })
        .unwrap();
    assert_eq!(configured.settings.devices[0].spec.display_name, "Renamed");
    assert_eq!(
        configured.settings.devices[0]
            .native_formats
            .render
            .current
            .unwrap()
            .channels,
        32
    );
    manager
        .mutate(DriverCommand::Remove {
            expected_revision: configured.revision,
            endpoint_id: configured.settings.devices[0].spec.id.clone(),
        })
        .unwrap();
    drop(manager);
    let (backend, _) = FakeBackend::ready();
    let mut reopened = DriverManager::open(backend, &path).unwrap();
    assert!(reopened.snapshot().unwrap().settings.devices.is_empty());
}

#[test]
fn absent_store_creates_fixed_default_but_existing_empty_stays_empty() {
    let fixture = FixtureDir::new("driver-default").unwrap();
    let path = fixture.join("drivers.json");
    let (backend, _) = FakeBackend::ready();
    let mut manager = DriverManager::open(backend, &path).unwrap();
    let snapshot = manager.snapshot().unwrap();
    let device = &snapshot.settings.devices[0];
    assert_eq!(
        device.spec.id.as_str(),
        "17e59d73-42c1-4f11-a8b4-6c2d913357e0"
    );
    assert_eq!(device.spec.display_name, "MixRelay IO");
    assert_eq!(
        device.native_formats.render.current.unwrap().sample_format,
        NativeSampleFormat::Float32
    );
    assert!(path.is_file());
    fs::write(&path, br#"{"revision":9,"devices":[]}"#).unwrap();
    let (backend, observed) = FakeBackend::ready();
    let mut manager = DriverManager::open(backend, &path).unwrap();
    assert!(manager.snapshot().unwrap().settings.devices.is_empty());
    assert!(observed.lock().settings.devices.is_empty());
}

#[test]
fn malformed_store_is_preserved_without_native_apply() {
    let fixture = FixtureDir::new("driver-invalid-store").unwrap();
    let path = fixture.join("drivers.json");
    fs::write(&path, b"not JSON").unwrap();
    let (backend, observed) = FakeBackend::ready();
    assert!(DriverManager::open(backend, &path).is_err());
    assert_eq!(fs::read(&path).unwrap(), b"not JSON");
    assert_eq!(observed.lock().applies, 0);
}

#[test]
fn unavailable_open_never_persists_defaults_or_accepts_mutations() {
    let fixture = FixtureDir::new("driver-unavailable").unwrap();
    let path = fixture.join("drivers.json");
    let (backend, observed) = FakeBackend::ready();
    observed.lock().state = DriverState::InstalledNotLoaded;
    observed.lock().fail_read = true;
    let mut manager = DriverManager::open(backend, &path).unwrap();
    let snapshot = manager.snapshot().unwrap();
    assert_eq!(snapshot.state, DriverState::InstalledNotLoaded);
    assert!(!snapshot.settings.can_create);
    assert!(snapshot.settings.devices.is_empty());
    assert!(matches!(
        manager.mutate(DriverCommand::Create {
            expected_revision: snapshot.revision,
            spec: default_device().spec
        }),
        Err(DriverError::Unavailable(_))
    ));
    assert_eq!(observed.lock().applies, 0);
    assert!(!path.exists());
}

#[test]
fn pending_default_waits_for_native_apply_before_exposing_or_saving_settings() {
    let fixture = FixtureDir::new("driver-pending-default").unwrap();
    let path = fixture.join("drivers.json");
    let (backend, observed) = FakeBackend::ready();
    observed.lock().state = DriverState::InstalledNotLoaded;
    let mut manager = DriverManager::open(backend, &path).unwrap();
    assert!(!manager.initialize().unwrap());
    let waiting = manager.snapshot().unwrap();
    assert_eq!(waiting.state, DriverState::InstalledNotLoaded);
    assert!(!waiting.settings.can_create);
    assert!(waiting.settings.devices.is_empty());
    assert_eq!(manager.inventory, vec![default_device()]);
    assert_eq!(observed.lock().applies, 0);
    assert!(!path.exists());

    observed.lock().state = DriverState::Ready;
    let connected = manager.snapshot().unwrap();
    assert_eq!(connected.state, DriverState::Unavailable);
    assert!(!connected.settings.can_create);
    assert!(!path.exists());
    assert!(manager.initialize().unwrap());
    assert!(manager.initialize().unwrap());
    let ready = manager.snapshot().unwrap();
    assert_eq!(ready.state, DriverState::Ready);
    assert_eq!(ready.settings.devices.len(), 1);
    assert_eq!(observed.lock().applies, 1);
    assert!(path.exists());
}

#[test]
fn pending_stored_configuration_and_explicit_empty_survive_late_readiness() {
    let mut saved = default_device();
    saved.spec.display_name = "Saved user device".into();
    saved.render_format.as_mut().unwrap().sample_format = NativeSampleFormat::Signed16;
    for devices in [vec![saved], vec![]] {
        let fixture = FixtureDir::new("driver-pending-stored").unwrap();
        let path = fixture.join("drivers.json");
        store::save(&path, 7, &devices).unwrap();
        let bytes = fs::read(&path).unwrap();
        let (backend, observed) = FakeBackend::ready();
        observed.lock().state = DriverState::InstalledNotLoaded;
        let mut manager = DriverManager::open(backend, &path).unwrap();
        assert_eq!(manager.inventory, devices);
        assert!(manager.snapshot().unwrap().settings.devices.is_empty());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(observed.lock().applies, 0);

        observed.lock().state = DriverState::Ready;
        assert!(manager.initialize().unwrap());
        assert_eq!(
            manager.snapshot().unwrap().settings.devices.len(),
            devices.len()
        );
        assert_eq!(store::load(&path).unwrap().unwrap().devices, devices);
        assert_eq!(observed.lock().applies, 1);
    }
}

#[test]
fn failed_initial_apply_and_readback_preserve_pending_intent_and_store() {
    for fail_read in [false, true] {
        let fixture = FixtureDir::new("driver-pending-failure").unwrap();
        let path = fixture.join("drivers.json");
        let desired = vec![default_device()];
        store::save(&path, 7, &desired).unwrap();
        let bytes = fs::read(&path).unwrap();
        let (backend, observed) = FakeBackend::ready();
        observed.lock().state = DriverState::InstalledNotLoaded;
        let mut manager = DriverManager::open(backend, &path).unwrap();
        {
            let mut observation = observed.lock();
            observation.state = DriverState::Ready;
            observation.fail_apply = !fail_read;
            observation.fail_read = fail_read;
        }
        assert!(manager.initialize().is_err());
        assert!(manager.initial_inventory_pending);
        assert_eq!(manager.inventory, desired);
        let waiting = manager.snapshot().unwrap();
        assert_eq!(waiting.state, DriverState::Unavailable);
        assert!(!waiting.settings.can_create);
        assert!(waiting.settings.devices.is_empty());
        assert!(waiting.persistence_error.is_none());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        {
            let mut observation = observed.lock();
            observation.fail_apply = false;
            observation.fail_read = false;
        }
        assert!(manager.initialize().unwrap());
        assert_eq!(manager.snapshot().unwrap().state, DriverState::Ready);
        assert_eq!(observed.lock().applies, 2);
    }
}

#[test]
fn invalid_geometry_rate_duplicates_and_missing_ids_have_no_side_effects() {
    let fixture = FixtureDir::new("driver-invalid-commands").unwrap();
    let path = fixture.join("drivers.json");
    let (backend, observed) = FakeBackend::ready();
    let mut manager = DriverManager::open(backend, &path).unwrap();
    let snapshot = manager.snapshot().unwrap();
    let before = fs::read(&path).unwrap();
    for channels in [0, 65] {
        let mut spec = default_device().spec;
        spec.render.as_mut().unwrap().channels = channels;
        assert!(
            manager
                .mutate(DriverCommand::Configure {
                    expected_revision: snapshot.revision,
                    spec
                })
                .is_err()
        );
    }
    let mut spec = default_device().spec;
    spec.supported_sample_rates = vec![12_345];
    spec.default_sample_rate = 12_345;
    assert!(
        manager
            .mutate(DriverCommand::Configure {
                expected_revision: snapshot.revision,
                spec
            })
            .is_err()
    );
    assert!(
        manager
            .mutate(DriverCommand::Create {
                expected_revision: snapshot.revision,
                spec: default_device().spec
            })
            .is_err()
    );
    assert!(matches!(
        manager.mutate(DriverCommand::Remove {
            expected_revision: snapshot.revision,
            endpoint_id: VirtualEndpointId::from("absent")
        }),
        Err(DriverError::NotFound(_))
    ));
    assert_eq!(observed.lock().applies, 1);
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn native_apply_failure_preserves_active_inventory_and_store() {
    let fixture = FixtureDir::new("driver-apply-failure").unwrap();
    let path = fixture.join("drivers.json");
    let (backend, observed) = FakeBackend::ready();
    let mut manager = DriverManager::open(backend, &path).unwrap();
    let before = manager.snapshot().unwrap();
    let bytes = fs::read(&path).unwrap();
    observed.lock().fail_apply = true;
    assert!(
        manager
            .mutate(DriverCommand::Remove {
                expected_revision: before.revision,
                endpoint_id: before.settings.devices[0].spec.id.clone()
            })
            .is_err()
    );
    assert_eq!(manager.snapshot().unwrap(), before);
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn external_native_change_invalidates_revision_and_survives_name_change() {
    let fixture = FixtureDir::new("driver-external-rate").unwrap();
    let path = fixture.join("drivers.json");
    let (backend, observed) = FakeBackend::ready();
    let mut manager = DriverManager::open(backend, &path).unwrap();
    let before = manager.snapshot().unwrap();
    {
        let mut observation = observed.lock();
        let device = &mut observation.settings.devices[0];
        device.spec.default_sample_rate = 44_100;
        device.observed_sample_rate = Some(44_100);
        for formats in [
            &mut device.native_formats.render,
            &mut device.native_formats.capture,
        ] {
            formats.current.as_mut().unwrap().sample_rate = 44_100;
            for format in &mut formats.supported {
                format.sample_rate = 44_100;
            }
        }
    }
    assert!(matches!(
        manager.mutate(DriverCommand::Remove {
            expected_revision: before.revision,
            endpoint_id: before.settings.devices[0].spec.id.clone()
        }),
        Err(DriverError::RevisionMismatch { .. })
    ));
    let actual = manager.snapshot().unwrap();
    assert_eq!(
        store::load(&path).unwrap().unwrap().devices[0]
            .spec
            .default_sample_rate,
        44_100
    );
    let mut spec = actual.settings.devices[0].spec.clone();
    spec.display_name = "User device".into();
    let after = manager
        .mutate(DriverCommand::Configure {
            expected_revision: actual.revision,
            spec,
        })
        .unwrap();
    assert_eq!(after.settings.devices[0].observed_sample_rate, Some(44_100));
    assert_eq!(
        store::load(&path).unwrap().unwrap().devices[0]
            .spec
            .default_sample_rate,
        44_100
    );
}

#[test]
fn set_format_requires_exact_supported_tuple_and_preserves_other_pcm() {
    let fixture = FixtureDir::new("driver-format-tuples").unwrap();
    let (backend, observed) = FakeBackend::ready();
    let mut manager = DriverManager::open(backend, fixture.join("drivers.json")).unwrap();
    let snapshot = manager.snapshot().unwrap();
    let id = snapshot.settings.devices[0].spec.id.clone();
    let current = snapshot.settings.devices[0]
        .native_formats
        .render
        .current
        .unwrap();
    let unsupported = NativeStreamFormat {
        channels: 3,
        ..current
    };
    assert!(
        manager
            .mutate(DriverCommand::SetFormat {
                expected_revision: snapshot.revision,
                endpoint_id: id.clone(),
                direction: NativeDirection::Render,
                format: unsupported
            })
            .is_err()
    );
    assert_eq!(observed.lock().applies, 1);
    let integer = NativeStreamFormat {
        sample_format: NativeSampleFormat::Signed16,
        ..current
    };
    let after = manager
        .mutate(DriverCommand::SetFormat {
            expected_revision: snapshot.revision,
            endpoint_id: id,
            direction: NativeDirection::Render,
            format: integer,
        })
        .unwrap();
    assert_eq!(
        after.settings.devices[0].native_formats.render.current,
        Some(integer)
    );
    assert_eq!(
        after.settings.devices[0].native_formats.capture.current,
        Some(current)
    );
}

#[test]
fn shared_sample_format_applies_duplex_once_and_preserves_rate_and_channels() {
    let fixture = FixtureDir::new("driver-shared-format").unwrap();
    let path = fixture.join("drivers.json");
    let mut saved = default_device();
    saved.spec.default_sample_rate = 44_100;
    saved.spec.capture.as_mut().unwrap().channels = 4;
    saved.spec.capture.as_mut().unwrap().channel_layout = ChannelLayout::Discrete;
    saved.render_format.as_mut().unwrap().sample_rate = 44_100;
    saved.capture_format.as_mut().unwrap().sample_rate = 44_100;
    saved.capture_format.as_mut().unwrap().channels = 4;
    store::save(&path, 7, &[saved]).unwrap();
    let (backend, observed) = FakeBackend::ready();
    let mut manager = DriverManager::open(backend, &path).unwrap();
    let before = manager.snapshot().unwrap();
    let device = &before.settings.devices[0];
    let applies = observed.lock().applies;

    let after = manager
        .mutate(DriverCommand::SetSampleFormat {
            expected_revision: before.revision,
            endpoint_id: device.spec.id.clone(),
            sample_format: NativeSampleFormat::Signed16,
        })
        .unwrap();

    assert_eq!(after.revision, before.revision + 1);
    assert_eq!(observed.lock().applies, applies + 1);
    assert_eq!(after.settings.devices[0].spec, device.spec);
    for (previous, current) in [
        (
            device.native_formats.render.current,
            after.settings.devices[0].native_formats.render.current,
        ),
        (
            device.native_formats.capture.current,
            after.settings.devices[0].native_formats.capture.current,
        ),
    ] {
        assert_eq!(
            current,
            Some(NativeStreamFormat {
                sample_format: NativeSampleFormat::Signed16,
                ..previous.unwrap()
            })
        );
    }
    let persisted = store::load(&path).unwrap().unwrap();
    assert_eq!(persisted.revision, after.revision);
    assert_eq!(persisted.devices, manager.inventory);
}

#[test]
fn shared_sample_format_rejects_unsupported_capture_tuple_without_partial_apply() {
    for change_channels in [false, true] {
        let fixture = FixtureDir::new("driver-shared-unsupported").unwrap();
        let path = fixture.join("drivers.json");
        let (backend, observed) = FakeBackend::ready();
        let mut manager = DriverManager::open(backend, &path).unwrap();
        {
            let mut observation = observed.lock();
            let supported = &mut observation.settings.devices[0]
                .native_formats
                .capture
                .supported;
            let integer = supported
                .iter_mut()
                .find(|format| format.sample_format == NativeSampleFormat::Signed16)
                .unwrap();
            if change_channels {
                integer.channels = 4;
            } else {
                integer.sample_rate = 44_100;
            }
        }
        let before = manager.snapshot().unwrap();
        let bytes = fs::read(&path).unwrap();
        let applies = observed.lock().applies;

        assert!(matches!(
            manager.mutate(DriverCommand::SetSampleFormat {
                expected_revision: before.revision,
                endpoint_id: before.settings.devices[0].spec.id.clone(),
                sample_format: NativeSampleFormat::Signed16,
            }),
            Err(DriverError::Invalid(_))
        ));
        assert_eq!(manager.snapshot().unwrap(), before);
        assert_eq!(observed.lock().applies, applies);
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn shared_sample_format_does_not_create_an_absent_direction() {
    for direction in [NativeDirection::Render, NativeDirection::Capture] {
        let fixture = FixtureDir::new("driver-shared-single-direction").unwrap();
        let path = fixture.join("drivers.json");
        let mut saved = default_device();
        match direction {
            NativeDirection::Render => {
                saved.spec.capture = None;
                saved.capture_format = None;
            }
            NativeDirection::Capture => {
                saved.spec.render = None;
                saved.render_format = None;
            }
        }
        store::save(&path, 7, std::slice::from_ref(&saved)).unwrap();
        let (backend, observed) = FakeBackend::ready();
        let mut manager = DriverManager::open(backend, &path).unwrap();
        let before = manager.snapshot().unwrap();
        let applies = observed.lock().applies;

        let after = manager
            .mutate(DriverCommand::SetSampleFormat {
                expected_revision: before.revision,
                endpoint_id: saved.spec.id.clone(),
                sample_format: NativeSampleFormat::Signed16,
            })
            .unwrap();

        assert_eq!(after.revision, before.revision + 1);
        assert_eq!(observed.lock().applies, applies + 1);
        assert_eq!(after.settings.devices[0].spec, saved.spec);
        for (stream, native) in [
            (
                saved.spec.render,
                &after.settings.devices[0].native_formats.render,
            ),
            (
                saved.spec.capture,
                &after.settings.devices[0].native_formats.capture,
            ),
        ] {
            if stream.is_some() {
                assert_eq!(
                    native.current.unwrap().sample_format,
                    NativeSampleFormat::Signed16
                );
            } else {
                assert!(native.current.is_none());
                assert!(native.supported.is_empty());
            }
        }
    }
}

#[test]
fn shared_sample_format_requires_current_observation_for_every_present_direction() {
    for direction in [NativeDirection::Render, NativeDirection::Capture] {
        let fixture = FixtureDir::new("driver-shared-missing-current").unwrap();
        let path = fixture.join("drivers.json");
        let (backend, observed) = FakeBackend::ready();
        let mut manager = DriverManager::open(backend, &path).unwrap();
        {
            let mut observation = observed.lock();
            let formats = &mut observation.settings.devices[0].native_formats;
            match direction {
                NativeDirection::Render => formats.render.current = None,
                NativeDirection::Capture => formats.capture.current = None,
            }
        }
        let before = manager.snapshot().unwrap();
        let bytes = fs::read(&path).unwrap();
        let applies = observed.lock().applies;

        assert!(matches!(
            manager.mutate(DriverCommand::SetSampleFormat {
                expected_revision: before.revision,
                endpoint_id: before.settings.devices[0].spec.id.clone(),
                sample_format: NativeSampleFormat::Signed16,
            }),
            Err(DriverError::Invalid(_))
        ));
        assert_eq!(manager.snapshot().unwrap(), before);
        assert_eq!(observed.lock().applies, applies);
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn shared_sample_format_rejects_stale_revision_and_missing_identity() {
    let fixture = FixtureDir::new("driver-shared-command-errors").unwrap();
    let path = fixture.join("drivers.json");
    let (backend, observed) = FakeBackend::ready();
    let mut manager = DriverManager::open(backend, &path).unwrap();
    let before = manager.snapshot().unwrap();
    let bytes = fs::read(&path).unwrap();
    let applies = observed.lock().applies;

    assert!(matches!(
        manager.mutate(DriverCommand::SetSampleFormat {
            expected_revision: before.revision - 1,
            endpoint_id: before.settings.devices[0].spec.id.clone(),
            sample_format: NativeSampleFormat::Signed16,
        }),
        Err(DriverError::RevisionMismatch { .. })
    ));
    assert!(matches!(
        manager.mutate(DriverCommand::SetSampleFormat {
            expected_revision: before.revision,
            endpoint_id: VirtualEndpointId::from("absent"),
            sample_format: NativeSampleFormat::Signed16,
        }),
        Err(DriverError::NotFound(_))
    ));
    assert_eq!(manager.snapshot().unwrap(), before);
    assert_eq!(observed.lock().applies, applies);
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn shared_sample_format_native_failure_preserves_both_directions_and_store() {
    let fixture = FixtureDir::new("driver-shared-native-failure").unwrap();
    let path = fixture.join("drivers.json");
    let (backend, observed) = FakeBackend::ready();
    let mut manager = DriverManager::open(backend, &path).unwrap();
    let before = manager.snapshot().unwrap();
    let bytes = fs::read(&path).unwrap();
    let applies = observed.lock().applies;
    observed.lock().fail_apply = true;

    assert!(matches!(
        manager.mutate(DriverCommand::SetSampleFormat {
            expected_revision: before.revision,
            endpoint_id: before.settings.devices[0].spec.id.clone(),
            sample_format: NativeSampleFormat::Signed16,
        }),
        Err(DriverError::Backend(_))
    ));
    assert_eq!(manager.snapshot().unwrap(), before);
    assert_eq!(observed.lock().applies, applies + 1);
    assert_eq!(fs::read(&path).unwrap(), bytes);
}

#[test]
fn persistence_failure_reports_committed_native_state_without_rollback() {
    let fixture = FixtureDir::new("driver-persist-failure").unwrap();
    let path = fixture.join("drivers.json");
    let (backend, observed) = FakeBackend::ready();
    let mut manager = DriverManager::open(backend, &path).unwrap();
    let before = manager.snapshot().unwrap();
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    let after = manager
        .mutate(DriverCommand::Remove {
            expected_revision: before.revision,
            endpoint_id: before.settings.devices[0].spec.id.clone(),
        })
        .unwrap();
    assert!(after.settings.devices.is_empty());
    assert!(observed.lock().settings.devices.is_empty());
    assert!(after.persistence_error.is_some());
    assert!(after.revision > before.revision);
    assert!(path.is_dir());
    assert_eq!(fs::read_dir(fixture.path()).unwrap().count(), 1);
}

#[test]
fn absent_native_formats_are_not_invented_during_snapshot_or_save() {
    let fixture = FixtureDir::new("driver-unknown-format").unwrap();
    let path = fixture.join("drivers.json");
    let (backend, observed) = FakeBackend::ready();
    let mut manager = DriverManager::open(backend, &path).unwrap();
    observed.lock().settings.devices[0].native_formats.render =
        mixrelay_audio_io::NativeStreamFormats::default();
    let snapshot = manager.snapshot().unwrap();
    assert!(
        snapshot.settings.devices[0]
            .native_formats
            .render
            .current
            .is_none()
    );
    assert!(manager.inventory[0].render_format.is_none());
    assert!(
        store::load(&path).unwrap().unwrap().devices[0]
            .render_format
            .is_none()
    );
}
