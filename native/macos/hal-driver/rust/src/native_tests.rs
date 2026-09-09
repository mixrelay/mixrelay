//! No installed device, coreaudiod restart or filesystem fixture is involved:
//! these tests exercise the actual exported HAL runtime on anonymous mappings.
use std::ffi::c_void;
use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::atomic::Ordering;

use tonenari_macos_transport::control_inventory::{
    InventoryEntry, tn_hal_control_create_inventory_snapshot,
};
use tonenari_macos_transport::endpoint_transport::{
    EndpointClockSnapshot, EndpointTransport, EndpointTransportConfig,
    tn_endpoint_transport_capture_buffer, tn_endpoint_transport_clock_snapshot,
    tn_endpoint_transport_initialize, tn_endpoint_transport_render_buffer,
    tn_endpoint_transport_required_bytes,
};
use tonenari_macos_transport::shared_audio_buffer::{
    tn_shared_audio_buffer_read, tn_shared_audio_buffer_try_begin_epoch,
    tn_shared_audio_buffer_write,
};

use super::{Fixture, changed, request};
use crate::abi::{HostOps, OK};
use crate::registry::IoOperation;

#[path = "native_lifecycle_tests.rs"]
mod lifecycle;

unsafe extern "C" {
    fn mmap(
        address: *mut c_void,
        size: usize,
        protection: i32,
        flags: i32,
        fd: i32,
        offset: i64,
    ) -> *mut c_void;
    fn munmap(address: *mut c_void, size: usize) -> i32;
    fn xpc_release(message: *mut c_void);
}

struct Message(*mut c_void);
impl Message {
    fn snapshot(entries: &[(&Transport, &str, u32, u64)], generation: u64) -> Self {
        let entries: Vec<_> = entries
            .iter()
            .map(|(transport, name, rate, generation)| {
                let mut entry = InventoryEntry {
                    endpoint_id: transport.endpoint,
                    sample_rate: *rate,
                    descriptor: transport.fd.as_raw_fd(),
                    mapped_size: transport.size,
                    generation: *generation,
                    ..InventoryEntry::default()
                };
                for (output, byte) in entry.display_name.iter_mut().zip(name.bytes()) {
                    *output = std::ffi::c_char::from_ne_bytes([byte]);
                }
                entry
            })
            .collect();
        let message = unsafe {
            tn_hal_control_create_inventory_snapshot(entries.as_ptr(), entries.len(), generation)
        };
        assert!(!message.is_null());
        Self(message)
    }

    fn apply(&self, fixture: &Fixture) -> bool {
        connect(fixture, 1);
        self.apply_from(fixture, 1)
    }

    fn apply_from(&self, fixture: &Fixture, authority: u64) -> bool {
        unsafe {
            crate::inventory::tn_hal_core_control_message(
                (&raw const *fixture.registry).cast_mut(),
                authority,
                self.0,
            )
        }
    }
}

fn connect(fixture: &Fixture, authority: u64) {
    assert!(unsafe {
        crate::inventory::tn_hal_core_control_connected(
            (&raw const *fixture.registry).cast_mut(),
            authority,
        )
    });
}
impl Drop for Message {
    fn drop(&mut self) {
        unsafe { xpc_release(self.0) };
    }
}

struct Transport {
    fd: OwnedFd,
    pointer: *mut EndpointTransport,
    memory: *mut c_void,
    size: usize,
    endpoint: [u8; 16],
}
impl Transport {
    fn new(endpoint: u8, render: u32, capture: u32) -> Self {
        let config = EndpointTransportConfig {
            endpoint_id: [endpoint; 16],
            capacity_frames: 256,
            render_channels: render,
            capture_channels: capture,
        };
        let size = unsafe { tn_endpoint_transport_required_bytes(&raw const config) };
        assert!(size > 0);
        let fd = tonenari_macos_transport::create_shared_memory(size, true).unwrap();
        let memory = unsafe { mmap(std::ptr::null_mut(), size, 3, 1, fd.as_raw_fd(), 0) };
        assert_ne!(memory as isize, -1);
        let pointer = unsafe { tn_endpoint_transport_initialize(memory, size, &raw const config) };
        assert!(!pointer.is_null());
        Self {
            fd,
            pointer,
            memory,
            size,
            endpoint: config.endpoint_id,
        }
    }

    fn add(&self, fixture: &Fixture, name: &str, rate: u32) -> u32 {
        fixture
            .registry
            .upsert(
                self.endpoint,
                name,
                rate,
                (self.fd.try_clone().unwrap(), self.size),
            )
            .unwrap()
    }

    fn clock(&self) -> EndpointClockSnapshot {
        let mut clock = EndpointClockSnapshot::default();
        assert!(unsafe { tn_endpoint_transport_clock_snapshot(self.pointer, &raw mut clock) });
        clock
    }

    fn capture<const N: usize>(&self, samples: &[f32; N], frames: u32) {
        let buffer = unsafe { tn_endpoint_transport_capture_buffer(self.pointer) };
        let epoch = unsafe { tn_shared_audio_buffer_try_begin_epoch(buffer) };
        assert!(epoch > 0);
        assert!(unsafe {
            tn_shared_audio_buffer_write(buffer, epoch, 0, samples.as_ptr(), frames)
        });
    }

    fn render<const N: usize>(&self, frames: u32) -> [f32; N] {
        let mut samples = [0.0; N];
        let buffer = unsafe { tn_endpoint_transport_render_buffer(self.pointer) };
        assert_eq!(
            unsafe { tn_shared_audio_buffer_read(buffer, 0, samples.as_mut_ptr(), frames) },
            frames
        );
        samples
    }
}
impl Drop for Transport {
    fn drop(&mut self) {
        assert_eq!(unsafe { munmap(self.memory, self.size) }, 0);
    }
}

fn native_fixture() -> Fixture {
    let mut fixture = Fixture::new();
    let host = HostOps {
        context: (&raw const *fixture.host).cast_mut().cast(),
        changed,
        request_configuration: request,
    };
    let registry = unsafe { crate::native::tn_hal_core_create_native(&raw const host) };
    assert!(!registry.is_null());
    fixture.registry = unsafe { Box::from_raw(registry) };
    fixture.host.registry.store(registry, Ordering::Release);
    fixture
}

fn start(fixture: &Fixture, device: u32) {
    assert_eq!(fixture.registry.client(device, 42, true), OK);
    assert_eq!(fixture.registry.start(device, 42, 1), OK);
}

#[test]
fn native_duplex_mappings_are_isolated_and_publish_independent_clocks() {
    let fixture = native_fixture();
    let first = Transport::new(10, 2, 2);
    let second = Transport::new(20, 7, 3);
    let a = first.add(&fixture, "Stereo", 48_000);
    let b = second.add(&fixture, "Seven", 96_000);
    start(&fixture, a);
    start(&fixture, b);
    let mut source_a = [0.25; 64];
    let mut source_b = [0.75; 224];
    for (device, samples) in [(a, source_a.as_mut_ptr()), (b, source_b.as_mut_ptr())] {
        assert!(unsafe {
            fixture.registry.io(IoOperation {
                device,
                stream: device + 2,
                render: true,
                frame: 0,
                samples,
                frames: 32,
            })
        });
    }
    assert_eq!(
        first.render::<64>(32).map(f32::to_bits),
        source_a.map(f32::to_bits)
    );
    assert_eq!(
        second.render::<224>(32).map(f32::to_bits),
        source_b.map(f32::to_bits)
    );
    first.capture(&[0.5; 64], 32);
    second.capture(&[0.125; 96], 32);
    let mut capture_a = [0.0; 64];
    let mut capture_b = [0.0; 96];
    for (device, samples) in [(a, capture_a.as_mut_ptr()), (b, capture_b.as_mut_ptr())] {
        assert!(unsafe {
            fixture.registry.io(IoOperation {
                device,
                stream: device + 1,
                render: false,
                frame: 0,
                samples,
                frames: 32,
            })
        });
    }
    assert_eq!(capture_a.map(f32::to_bits), [0.5_f32.to_bits(); 64]);
    assert_eq!(capture_b.map(f32::to_bits), [0.125_f32.to_bits(); 96]);
    assert_eq!(
        (first.clock().sample_rate, second.clock().sample_rate),
        (48_000, 96_000)
    );
    assert_eq!(
        (first.clock().render_through, second.clock().capture_through),
        (32, 32)
    );
}

#[test]
fn native_replacement_retires_old_mapping_and_preserves_device_identity() {
    let fixture = native_fixture();
    let first = Transport::new(10, 2, 2);
    let id = first.add(&fixture, "Before", 48_000);
    start(&fixture, id);
    let original = fixture.registry.snapshot(id).unwrap();
    assert_eq!(fixture.registry.set_rate(id, 192_000), OK);
    assert_eq!(fixture.perform(), OK);
    let replacement = Transport::new(10, 7, 33);
    assert_eq!(replacement.add(&fixture, "After", 48_000), id);
    assert_eq!(fixture.registry.snapshot(id).unwrap().render_channels, 2);
    assert_eq!(fixture.perform(), OK);
    let current = fixture.registry.snapshot(id).unwrap();
    assert_eq!(original.uid, current.uid);
    assert_eq!(
        (
            current.render_channels,
            current.capture_channels,
            current.sample_rate
        ),
        (7, 33, 192_000)
    );
    assert_eq!(first.clock().running, 0);
    assert_eq!(
        (replacement.clock().running, replacement.clock().sample_rate),
        (1, 192_000)
    );
    let mut source = [0.75; 224];
    assert!(unsafe {
        fixture.registry.io(IoOperation {
            device: id,
            stream: id + 2,
            render: true,
            frame: 0,
            samples: source.as_mut_ptr(),
            frames: 32,
        })
    });
    assert_eq!(
        replacement.render::<224>(32).map(f32::to_bits),
        source.map(f32::to_bits)
    );
    let previous_epoch = replacement.clock().epoch;
    replacement.add(&fixture, "Same mapping", 48_000);
    assert_eq!(
        (replacement.clock().epoch, replacement.clock().running),
        (previous_epoch, 1)
    );
    assert_eq!(fixture.registry.stop(id, 42), OK);
    assert_eq!(replacement.clock().running, 0);
}

#[test]
fn authoritative_inventory_validates_every_entry_before_removal() {
    let fixture = native_fixture();
    let first = Transport::new(10, 2, 2);
    let second = Transport::new(20, 7, 3);
    let initial = Message::snapshot(
        &[(&first, "First", 48_000, 1), (&second, "Second", 96_000, 2)],
        1,
    );
    assert!(initial.apply(&fixture));
    let original = fixture.registry.devices();
    assert_eq!(original.len(), 2);
    let invalid = Message::snapshot(&[(&first, "Invalid", 384_000, 3)], 2);
    assert!(!invalid.apply(&fixture));
    assert_eq!(fixture.registry.devices(), original);
    let valid = Message::snapshot(&[(&first, "Renamed", 48_000, 3)], 2);
    assert!(valid.apply(&fixture));
    assert_eq!(fixture.registry.devices(), [original[0]]);
    assert_eq!(
        &fixture.registry.snapshot(original[0]).unwrap().name[..7],
        b"Renamed"
    );
    assert!(initial.apply(&fixture));
    assert_eq!(fixture.registry.devices(), [original[0]]);
    let empty = Message::snapshot(&[], 3);
    assert!(empty.apply(&fixture));
    assert!(fixture.registry.devices().is_empty());
}
