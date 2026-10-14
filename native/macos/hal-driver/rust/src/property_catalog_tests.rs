use super::Fixture;
use crate::abi::OK;
use crate::properties::{Address, mr_hal_core_property_get, mr_hal_core_property_set};

fn control_address(selector: [u8; 4]) -> Address {
    Address {
        selector: u32::from_be_bytes(selector),
        scope: u32::from_be_bytes(*b"glob"),
        element: 0,
    }
}

fn control_set(fixture: &Fixture, object: u32, selector: [u8; 4], bytes: &[u8]) -> i32 {
    let address = control_address(selector);
    // SAFETY: the fixture owns the registry; the input bytes live for the synchronous call.
    unsafe {
        mr_hal_core_property_set(
            (&raw const *fixture.registry).cast_mut(),
            object,
            &raw const address,
            u32::try_from(bytes.len()).unwrap(),
            bytes.as_ptr().cast(),
        )
    }
}

fn conversion(fixture: &Fixture, object: u32, selector: [u8; 4], input: f32) -> f32 {
    let address = control_address(selector);
    let mut value = input;
    let mut written = 0;
    // SAFETY: the in/out Float32 is exclusively owned, aligned and initialized.
    assert_eq!(
        unsafe {
            mr_hal_core_property_get(
                (&raw const *fixture.registry).cast_mut(),
                object,
                &raw const address,
                0,
                std::ptr::null(),
                4,
                &raw mut written,
                (&raw mut value).cast(),
                0,
            )
        },
        OK
    );
    assert_eq!(written, 4);
    value
}

#[test]
fn output_controls_advertise_owned_sdk_classes_scope_and_native_state() {
    let fixture = Fixture::new();
    let id = fixture.add(1, 48_000);
    let second = fixture.add(2, 48_000);
    let read_u32 = |object, selector| {
        u32::from_ne_bytes(fixture.property::<4>(object, &control_address(selector)))
    };
    assert_eq!(
        fixture.property::<8>(id, &control_address(*b"ctrl")),
        [id + 3, id + 4]
            .into_iter()
            .flat_map(u32::to_ne_bytes)
            .collect::<Vec<_>>()[..]
    );
    assert_eq!(
        fixture.property::<16>(id, &control_address(*b"ownd")),
        [id + 1, id + 2, id + 3, id + 4]
            .into_iter()
            .flat_map(u32::to_ne_bytes)
            .collect::<Vec<_>>()[..]
    );
    assert_eq!(read_u32(id + 3, *b"clas"), u32::from_be_bytes(*b"vlme"));
    assert_eq!(read_u32(id + 3, *b"bcls"), u32::from_be_bytes(*b"levl"));
    assert_eq!(read_u32(id + 4, *b"clas"), u32::from_be_bytes(*b"mute"));
    assert_eq!(read_u32(id + 4, *b"bcls"), u32::from_be_bytes(*b"togl"));
    assert_eq!(read_u32(id + 3, *b"stdv"), id);
    assert_eq!(read_u32(id + 3, *b"cscp"), u32::from_be_bytes(*b"outp"));
    assert_eq!(read_u32(id + 4, *b"celm"), 0);
    assert_eq!(
        control_set(&fixture, id + 3, *b"lcsv", &0.5_f32.to_ne_bytes()),
        OK
    );
    let db = f32::from_ne_bytes(fixture.property::<4>(id + 3, &control_address(*b"lcdv")));
    assert!((db + 6.0206).abs() < 0.0001);
    assert!((conversion(&fixture, id + 3, *b"lcsd", 0.5) - db).abs() < 0.0001);
    assert!((conversion(&fixture, id + 3, *b"lcds", db) - 0.5).abs() < 0.0001);
    assert_eq!(
        control_set(&fixture, id + 4, *b"bcvl", &1_u32.to_ne_bytes()),
        OK
    );
    assert_eq!(read_u32(id + 4, *b"bcvl"), 1);
    assert_eq!(fixture.registry.output_control(id), Some((0.5, true)));
    assert_eq!(fixture.registry.output_control(second), Some((1.0, false)));
    assert!(super::lock(&fixture.host.changes).contains(&(id, crate::abi::OUTPUT_VOLUME)));
    assert!(super::lock(&fixture.host.changes).contains(&(id, crate::abi::OUTPUT_MUTE)));
}

#[test]
fn output_control_setters_reject_wrong_objects_sizes_and_invalid_levels() {
    let fixture = Fixture::new();
    let id = fixture.add(1, 48_000);
    for value in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
        assert_ne!(
            control_set(&fixture, id + 3, *b"lcsv", &value.to_ne_bytes()),
            OK
        );
    }
    for value in [f32::NAN, -97.0, 1.0] {
        assert_ne!(
            control_set(&fixture, id + 3, *b"lcdv", &value.to_ne_bytes()),
            OK
        );
    }
    assert_ne!(control_set(&fixture, id + 3, *b"lcsv", &[0; 3]), OK);
    assert_ne!(
        control_set(&fixture, id + 4, *b"lcsv", &0.5_f32.to_ne_bytes()),
        OK
    );
    assert_ne!(
        control_set(&fixture, id, *b"bcvl", &1_u32.to_ne_bytes()),
        OK
    );
    assert_eq!(
        control_set(&fixture, id + 3, *b"lcdv", &(-6.0206_f32).to_ne_bytes()),
        OK
    );
    assert!((fixture.registry.output_control(id).unwrap().0 - 0.5).abs() < 0.0001);
    assert_eq!(fixture.registry.remove([1; 16]), OK);
    fixture.registry.tick();
    assert_ne!(
        control_set(&fixture, id + 3, *b"lcsv", &0.5_f32.to_ne_bytes()),
        OK
    );
}

#[test]
fn capture_only_devices_do_not_advertise_native_output_controls() {
    let fixture = Fixture::new();
    let id = fixture
        .registry
        .upsert([1; 16], "Capture only", 48_000, (fixture.fd(1, 0, 2), 24))
        .unwrap();
    assert_eq!(fixture.property::<0>(id, &control_address(*b"ctrl")), []);
    assert_eq!(
        fixture.property::<4>(id, &control_address(*b"ownd")),
        (id + 1).to_ne_bytes()
    );
    let address = control_address(*b"lcsv");
    assert!(!unsafe {
        crate::properties::mr_hal_core_has_property(
            (&raw const *fixture.registry).cast_mut(),
            id + 3,
            &raw const address,
        )
    });
    assert_eq!(
        fixture.registry.set_output_volume(id + 3, 0.5),
        crate::abi::NOT_FOUND
    );
}

fn catalog(fixture: &Fixture, object: u32, selector: [u8; 4], stride: usize, channels: u32) {
    let address = Address {
        selector: u32::from_be_bytes(selector),
        scope: u32::from_be_bytes(*b"glob"),
        element: 0,
    };
    let total = if stride == 16 { 6 } else { 24 };
    for count in 0..=total + 1 {
        let mut output = [0xa5_u8; 1400];
        let mut written = u32::MAX;
        assert_eq!(
            unsafe {
                mr_hal_core_property_get(
                    (&raw const *fixture.registry).cast_mut(),
                    object,
                    &raw const address,
                    0,
                    std::ptr::null(),
                    u32::try_from(count * stride).unwrap(),
                    &raw mut written,
                    output.as_mut_ptr().cast(),
                    0,
                )
            },
            0
        );
        let expected = count.min(total) * stride;
        assert_eq!(written as usize, expected);
        assert!(output[expected..].iter().all(|value| *value == 0xa5));
        for (index, entry) in output[..expected].chunks_exact(stride).enumerate() {
            let rate_index = if stride == 16 { index } else { index / 4 };
            let rate = f64::from(crate::formats::SAMPLE_RATES[rate_index]);
            assert_eq!(
                f64::from_ne_bytes(entry[..8].try_into().unwrap()).to_bits(),
                rate.to_bits()
            );
            if stride == 16 {
                assert_eq!(entry[..8], entry[8..16]);
            } else {
                let encoding = crate::properties::NATIVE_FORMATS[index % 4];
                assert_eq!(
                    &entry[..40],
                    crate::properties::format(
                        crate::formats::SAMPLE_RATES[rate_index],
                        channels,
                        encoding
                    )
                );
                assert_eq!(
                    u32::from_ne_bytes(entry[28..32].try_into().unwrap()),
                    channels
                );
                assert_eq!(
                    f64::from_ne_bytes(entry[40..48].try_into().unwrap()).to_bits(),
                    rate.to_bits()
                );
                assert_eq!(entry[40..48], entry[48..56]);
            }
        }
    }
}

#[test]
fn canonical_catalog_preserves_partial_read_bounds_for_both_stream_directions() {
    let fixture = Fixture::new();
    for channels in [1, 2, 7, 64] {
        let endpoint = u8::try_from(channels).unwrap();
        let id = fixture
            .registry
            .upsert(
                [endpoint; 16],
                "Catalog",
                48_000,
                (fixture.fd(endpoint, channels, channels), 24),
            )
            .unwrap();
        catalog(&fixture, id, *b"nsr#", 16, channels);
        catalog(&fixture, id + 1, *b"sfma", 56, channels);
        catalog(&fixture, id + 2, *b"sfma", 56, channels);
        catalog(&fixture, id + 1, *b"pfta", 56, channels);
        catalog(&fixture, id + 2, *b"pfta", 56, channels);
    }
}

#[test]
fn native_property_rate_setter_accepts_exact_catalog_only() {
    let fixture = Fixture::new();
    let id = fixture.add(1, 48_000);
    let address = Address {
        selector: u32::from_be_bytes(*b"nsrt"),
        scope: u32::from_be_bytes(*b"glob"),
        element: 0,
    };
    for &rate in crate::formats::SAMPLE_RATES {
        let value = f64::from(rate);
        assert_eq!(
            unsafe {
                mr_hal_core_property_set(
                    (&raw const *fixture.registry).cast_mut(),
                    id,
                    &raw const address,
                    8,
                    (&raw const value).cast(),
                )
            },
            0
        );
        assert_eq!(fixture.perform(), OK);
        assert_eq!(fixture.registry.snapshot(id).unwrap().sample_rate, rate);
    }
    for value in [0.0, 48_000.5, 352_800.0, 384_000.0, f64::NAN, f64::INFINITY] {
        assert_ne!(
            unsafe {
                mr_hal_core_property_set(
                    (&raw const *fixture.registry).cast_mut(),
                    id,
                    &raw const address,
                    8,
                    (&raw const value).cast(),
                )
            },
            0
        );
        assert_eq!(fixture.registry.snapshot(id).unwrap().sample_rate, 192_000);
    }
}

#[test]
fn native_formats_publish_only_after_perform_and_remain_directional() {
    let fixture = Fixture::new();
    let id = fixture.add(1, 48_000);
    for stream in [id + 1, id + 2] {
        for rate in crate::formats::SAMPLE_RATES {
            for encoding in crate::properties::NATIVE_FORMATS {
                let before = fixture.property::<40>(stream, &control_address(*b"pft "));
                let requested = crate::properties::format(*rate, 2, encoding);
                assert_eq!(control_set(&fixture, stream, *b"pft ", &requested), OK);
                assert_eq!(
                    fixture.property::<40>(stream, &control_address(*b"sfmt")),
                    before
                );
                assert_eq!(fixture.perform(), OK);
                assert_eq!(
                    &fixture.property::<40>(stream, &control_address(*b"pft "))[..],
                    requested
                );
                assert_eq!(
                    &fixture.property::<40>(stream, &control_address(*b"sfmt"))[..],
                    requested
                );
            }
        }
    }
    let snapshot = fixture.registry.snapshot(id).unwrap();
    assert_eq!(
        snapshot.capture_format,
        mixrelay_pcm_ring::SampleFormat::Signed32 as u32
    );
    assert_eq!(
        snapshot.render_format,
        mixrelay_pcm_ring::SampleFormat::Signed32 as u32
    );
}

#[test]
fn invalid_and_aborted_formats_preserve_prior_native_state() {
    let fixture = Fixture::new();
    let id = fixture.add(1, 48_000);
    let stream = id + 2;
    let before = fixture.property::<40>(stream, &control_address(*b"pft "));
    let valid = crate::properties::format(96_000, 2, mixrelay_pcm_ring::SampleFormat::Signed24);
    for offset in [8, 12, 16, 20, 24, 28, 32, 36] {
        let mut invalid = valid.clone();
        invalid[offset] ^= 0x80;
        assert_ne!(control_set(&fixture, stream, *b"pft ", &invalid), OK);
        assert_eq!(
            fixture.property::<40>(stream, &control_address(*b"pft ")),
            before
        );
        assert!(super::lock(&fixture.host.requests).is_empty());
    }
    assert_eq!(control_set(&fixture, stream, *b"sfmt", &valid), OK);
    let (_, generation) = fixture.pending();
    assert_eq!(
        fixture
            .registry
            .abort(id, crate::abi::CONFIG_ACTION, generation),
        OK
    );
    assert_eq!(fixture.perform(), OK);
    assert_eq!(
        fixture.property::<40>(stream, &control_address(*b"pft ")),
        before
    );
    assert_eq!(control_set(&fixture, stream, *b"pft ", &valid), OK);
    fixture
        .registry
        .upsert([1; 16], "Replacement", 48_000, (fixture.fd(1, 7, 3), 24))
        .unwrap();
    super::FAIL_REPLACE.store(true, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(fixture.perform(), crate::abi::NATIVE_ERROR);
    assert_eq!(
        fixture.property::<40>(stream, &control_address(*b"pft ")),
        before
    );
    assert_eq!(fixture.registry.snapshot(id).unwrap().sample_rate, 48_000);
}

#[test]
fn transport_presentation_is_per_requester_and_preserves_property_contract() {
    let fixture = Fixture::new();
    let id = fixture.add(1, 48_000);
    let pid = i32::try_from(std::process::id()).unwrap();
    let registry = (&raw const *fixture.registry).cast_mut();
    fixture
        .registry
        .transport_policy(vec!["com.test.App".into()]);
    assert!(super::lock(&fixture.host.changes).contains(&(id, crate::abi::TRANSPORT)));
    assert_eq!(
        unsafe {
            crate::abi::mr_hal_core_client_identity(
                registry,
                id,
                1,
                pid,
                c"com.test.App".as_ptr(),
                true,
            )
        },
        OK
    );
    let read = |pid| transport_read(registry, id, pid);
    assert_eq!(read(pid), u32::from_be_bytes(*b"usb "));
    assert_eq!(read(0), u32::from_be_bytes(*b"virt"));
    assert_eq!(read(i32::MAX), u32::from_be_bytes(*b"virt"));
    assert_eq!(read(pid), u32::from_be_bytes(*b"usb "));
    assert_eq!(
        unsafe {
            crate::abi::mr_hal_core_client_identity(registry, id, 1, 0, std::ptr::null(), false)
        },
        OK
    );
    fixture.registry.transport_policy(vec![]);
    assert_eq!(read(pid), u32::from_be_bytes(*b"virt"));
    assert_eq!(
        unsafe {
            crate::abi::mr_hal_core_client_identity(
                std::ptr::null_mut(),
                id,
                1,
                pid,
                std::ptr::null(),
                true,
            )
        },
        crate::abi::INVALID
    );
    assert_eq!(
        unsafe { crate::abi::mr_hal_core_client_identity(registry, 0, 1, pid, c"".as_ptr(), true) },
        crate::abi::NOT_FOUND
    );
}

fn transport_read(registry: *mut crate::registry::Registry, id: u32, pid: i32) -> u32 {
    let address = control_address(*b"tran");
    let mut value = 0_u32;
    let mut written = 0;
    assert_eq!(
        unsafe {
            mr_hal_core_property_get(
                registry,
                id,
                &raw const address,
                0,
                std::ptr::null(),
                4,
                &raw mut written,
                (&raw mut value).cast(),
                pid,
            )
        },
        OK
    );
    assert_eq!(written, 4);
    value
}

#[test]
fn transport_property_contract_is_unchanged() {
    let fixture = Fixture::new();
    let id = fixture.add(1, 48_000);
    let registry = (&raw const *fixture.registry).cast_mut();
    let address = control_address(*b"tran");
    let mut size = 0;
    assert_eq!(
        unsafe {
            crate::properties::mr_hal_core_property_size(
                registry,
                id,
                &raw const address,
                &raw mut size,
            )
        },
        OK
    );
    assert_eq!(size, 4);
    let mut settable = true;
    assert_eq!(
        unsafe {
            crate::properties::mr_hal_core_property_settable(
                registry,
                id,
                &raw const address,
                &raw mut settable,
            )
        },
        OK
    );
    assert!(!settable);
    assert!(unsafe {
        crate::properties::mr_hal_core_has_property(registry, id, &raw const address)
    });
}

#[test]
fn one_device_reports_usb_and_virtual_to_two_live_processes() {
    let fixture = Fixture::new();
    let id = fixture.add(1, 48_000);
    let registry = (&raw const *fixture.registry).cast_mut();
    let pid = i32::try_from(std::process::id()).unwrap();
    let mut child = std::process::Command::new("/bin/cat")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let other = i32::try_from(child.id()).unwrap();
    fixture
        .registry
        .transport_policy(vec!["com.test.App".into()]);
    for (client, pid, bundle) in [(1, pid, c"com.test.App"), (2, other, c"com.test.Other")] {
        assert_eq!(
            unsafe {
                crate::abi::mr_hal_core_client_identity(
                    registry,
                    id,
                    client,
                    pid,
                    bundle.as_ptr(),
                    true,
                )
            },
            OK
        );
    }
    assert_eq!(
        transport_read(registry, id, pid),
        u32::from_be_bytes(*b"usb ")
    );
    assert_eq!(
        transport_read(registry, id, other),
        u32::from_be_bytes(*b"virt")
    );
    assert_eq!(
        transport_read(registry, id, pid),
        u32::from_be_bytes(*b"usb ")
    );
    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
fn io_transport_property_contract_avoids_control_and_resolver_locks() {
    use std::sync::mpsc;
    use std::time::Duration;

    let fixture = Fixture::new();
    let id = fixture.add(1, 48_000);
    let registry = &*fixture.registry;
    let pid = i32::try_from(std::process::id()).unwrap();
    registry.transport_policy(vec!["com.test.App".into()]);
    registry
        .transport
        .client(id, 1, pid, Some("com.test.App".into()));
    assert_eq!(
        transport_read((&raw const *registry).cast_mut(), id, pid),
        u32::from_be_bytes(*b"usb ")
    );
    let device = registry.pin(id).unwrap();
    let io_pin = device.io_pin().unwrap();
    let control = super::lock(&device.control);
    let resolver = super::lock(&registry.transport.state);
    let identity_gate = super::lock(&registry.client_identity_gate);
    let inventory = super::lock(&registry.inventory);
    let (done, finished) = mpsc::channel();
    let timely = std::thread::scope(|scope| {
        scope.spawn(|| {
            let _audio = crate::transport::AudioScope::enter();
            let registry = (&raw const *registry).cast_mut();
            let address = control_address(*b"tran");
            let mut size = 0;
            let mut settable = true;
            let value = transport_read(registry, id, pid);
            let has = unsafe {
                crate::properties::mr_hal_core_has_property(registry, id, &raw const address)
            };
            let sized = unsafe {
                crate::properties::mr_hal_core_property_size(
                    registry,
                    id,
                    &raw const address,
                    &raw mut size,
                )
            };
            let writable = unsafe {
                crate::properties::mr_hal_core_property_settable(
                    registry,
                    id,
                    &raw const address,
                    &raw mut settable,
                )
            };
            done.send((value, has, sized, size, writable, settable))
                .unwrap();
        });
        let result = finished.recv_timeout(Duration::from_millis(200));
        // Release locks before joining even on failure, so this regression cannot hang.
        drop(inventory);
        drop(identity_gate);
        drop(resolver);
        drop(control);
        drop(io_pin);
        result
    });
    assert_eq!(
        timely.unwrap(),
        (u32::from_be_bytes(*b"virt"), true, OK, 4, OK, false)
    );
    assert_eq!(
        transport_read((&raw const *registry).cast_mut(), id, pid),
        u32::from_be_bytes(*b"usb ")
    );
}

#[test]
fn transport_validation_preserves_errors_and_scopes_without_snapshot() {
    let fixture = Fixture::new();
    let id = fixture.add(1, 48_000);
    let registry = (&raw const *fixture.registry).cast_mut();
    let device = fixture.registry.pin(id).unwrap();
    let control = super::lock(&device.control);
    let _audio = crate::transport::AudioScope::enter();
    let check = |object, scope, element, capacity| {
        let address = Address {
            selector: u32::from_be_bytes(*b"tran"),
            scope,
            element,
        };
        let mut written = 99;
        let mut output = 0;
        let status = unsafe {
            mr_hal_core_property_get(
                registry,
                object,
                &raw const address,
                0,
                std::ptr::null(),
                capacity,
                &raw mut written,
                (&raw mut output).cast(),
                0,
            )
        };
        if status == OK {
            assert_eq!(written, 4);
            assert_eq!(output, u32::from_be_bytes(*b"virt"));
        } else {
            assert_eq!(written, 99);
        }
        status
    };
    for scope in [*b"glob", *b"inpt", *b"outp"] {
        assert_eq!(check(id, u32::from_be_bytes(scope), 0, 4), OK);
    }
    let global = u32::from_be_bytes(*b"glob");
    let unknown = i32::from_be_bytes(*b"who?");
    let bad_object = i32::from_be_bytes(*b"!obj");
    for object in [1, id + 1, id + 2, id + 3, id + 4] {
        assert_eq!(check(object, global, 0, 4), unknown);
    }
    assert_eq!(check(id + 5, global, 0, 4), bad_object);
    assert_eq!(check(id + 1_024, global, 0, 4), bad_object);
    assert_eq!(check(id, global, 1, 4), unknown);
    assert_eq!(check(id, u32::from_be_bytes(*b"oops"), 0, 4), unknown);
    assert_eq!(check(id, global, 0, 3), i32::from_be_bytes(*b"!siz"));
    // Zero-channel child objects retain their previous !obj responses.
    device
        .capture_channels
        .store(0, std::sync::atomic::Ordering::Release);
    device
        .render_channels
        .store(0, std::sync::atomic::Ordering::Release);
    for object in id + 1..=id + 4 {
        assert_eq!(check(object, global, 0, 4), bad_object);
    }
    // Retired device metadata remains queryable until its slot is reclaimed.
    device
        .alive
        .store(false, std::sync::atomic::Ordering::Release);
    assert_eq!(check(id, global, 0, 4), OK);
    device
        .alive
        .store(true, std::sync::atomic::Ordering::Release);
    drop(control);
}

#[test]
fn non_io_transport_read_also_avoids_snapshot_and_budgetless_locks() {
    use std::sync::mpsc;
    use std::time::Duration;

    let fixture = Fixture::new();
    let id = fixture.add(1, 48_000);
    let pid = i32::try_from(std::process::id()).unwrap();
    let registry = &*fixture.registry;
    registry.transport_policy(vec!["com.test.App".into()]);
    registry
        .transport
        .client(id, 1, pid, Some("com.test.App".into()));
    let device = registry.pin(id).unwrap();
    let control = super::lock(&device.control);
    let resolver = super::lock(&registry.transport.state);
    let (done, finished) = mpsc::channel();
    let timely = std::thread::scope(|scope| {
        scope.spawn(|| {
            done.send(transport_read((&raw const *registry).cast_mut(), id, pid))
                .unwrap();
        });
        let result = finished.recv_timeout(Duration::from_millis(200));
        drop(resolver);
        drop(control);
        result
    });
    assert_eq!(timely.unwrap(), u32::from_be_bytes(*b"virt"));
    assert_eq!(
        transport_read((&raw const *registry).cast_mut(), id, pid),
        u32::from_be_bytes(*b"usb ")
    );
}

#[test]
fn io_transport_read_completes_while_stop_waits_for_callback_pin() {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let fixture = Fixture::new();
    let id = fixture.add(1, 48_000);
    let registry = &*fixture.registry;
    assert_eq!(registry.client(id, 1, true), OK);
    assert_eq!(registry.start(id, 1, 0), OK);
    let device = registry.pin(id).unwrap();
    // Model the callback's pin separately so a regression can be unwound:
    // release it on timeout, allowing StopIO to unlock control and both threads
    // to join. IO/timestamp both hold this same pin through native callbacks.
    let io_pin = device.io_pin().unwrap();
    let (done, finished) = mpsc::channel();
    let (held_control, timely, stopped) = std::thread::scope(|scope| {
        let stop = scope.spawn(|| registry.stop(id, 1));
        let deadline = Instant::now() + Duration::from_secs(1);
        let held_control = loop {
            if device.control.try_lock().is_err() {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            std::thread::yield_now();
        };
        scope.spawn(|| {
            let _audio = crate::transport::AudioScope::enter();
            let pid = i32::try_from(std::process::id()).unwrap();
            done.send(transport_read((&raw const *registry).cast_mut(), id, pid))
                .unwrap();
        });
        let timely = finished.recv_timeout(Duration::from_millis(200));
        drop(io_pin);
        (held_control, timely, stop.join().unwrap())
    });
    assert!(held_control);
    assert_eq!(timely.unwrap(), u32::from_be_bytes(*b"virt"));
    assert_eq!(stopped, OK);
    assert_eq!(registry.client(id, 1, false), OK);
}
