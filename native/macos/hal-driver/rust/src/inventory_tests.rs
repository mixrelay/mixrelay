use std::ffi::{c_char, c_void};
use std::os::fd::AsRawFd;
use std::sync::atomic::Ordering;

use mixrelay_macos_transport::control_inventory::{
    InventoryEntry, mr_hal_control_create_inventory_snapshot,
};

use super::{FAIL_CREATE_ENDPOINT, Fixture, LIVE};

unsafe extern "C" {
    fn xpc_release(message: *mut c_void);
}

struct Message(*mut c_void);
impl Message {
    fn new(fixture: &Fixture, endpoints: &[u8], generation: u64) -> Self {
        let descriptors: Vec<_> = endpoints
            .iter()
            .map(|endpoint| fixture.fd(*endpoint, 2, 2))
            .collect();
        let entries: Vec<_> = endpoints
            .iter()
            .zip(&descriptors)
            .map(|(endpoint, descriptor)| {
                let mut entry = InventoryEntry {
                    endpoint_id: [*endpoint; 16],
                    sample_rate: 48_000,
                    descriptor: descriptor.as_raw_fd(),
                    mapped_size: 24,
                    generation,
                    ..InventoryEntry::default()
                };
                for (slot, byte) in entry
                    .display_name
                    .iter_mut()
                    .zip(format!("Device {endpoint}").bytes())
                {
                    *slot = c_char::from_ne_bytes([byte]);
                }
                entry
            })
            .collect();
        let message = unsafe {
            mr_hal_control_create_inventory_snapshot(entries.as_ptr(), entries.len(), generation)
        };
        assert!(!message.is_null());
        Self(message)
    }

    fn apply(&self, fixture: &Fixture) -> bool {
        unsafe {
            assert!(crate::inventory::mr_hal_core_control_connected(
                (&raw const *fixture.registry).cast_mut(),
                1,
            ));
            crate::inventory::mr_hal_core_control_message(
                (&raw const *fixture.registry).cast_mut(),
                1,
                self.0,
            )
        }
    }
}
impl Drop for Message {
    fn drop(&mut self) {
        unsafe { xpc_release(self.0) };
    }
}

fn descriptor_count() -> usize {
    std::fs::read_dir("/dev/fd").unwrap().count()
}

#[test]
fn replacement_requests_require_current_validated_inventory() {
    unsafe extern "C" fn requires_replacement(_: *mut c_void) -> bool {
        true
    }
    let mut fixture = Fixture::new();
    fixture.registry.native.requires_replacement = requires_replacement;
    let inventory = Message::new(&fixture, &[1], 90);
    assert!(inventory.apply(&fixture));
    let request = |authority| {
        let mut endpoint = [0; 16];
        let mut generation = 0;
        let accepted = unsafe {
            crate::inventory::mr_hal_core_replacement_request(
                (&raw const *fixture.registry).cast_mut(),
                authority,
                &raw mut endpoint,
                &raw mut generation,
            )
        };
        (accepted, endpoint, generation)
    };
    assert_eq!(request(1), (true, [1; 16], 90));
    assert!(unsafe {
        crate::inventory::mr_hal_core_control_connected(
            (&raw const *fixture.registry).cast_mut(),
            2,
        )
    });
    assert_eq!(request(1), (false, [0; 16], 0));
    assert_eq!(request(2), (false, [0; 16], 0));
    let new = Message::new(&fixture, &[1], 2);
    assert!(unsafe {
        crate::inventory::mr_hal_core_control_message(
            (&raw const *fixture.registry).cast_mut(),
            2,
            new.0,
        )
    });
    assert_eq!(fixture.perform(), crate::abi::OK);
    assert_eq!(request(2), (true, [1; 16], 2));
}

#[test]
fn partial_publication_is_removed_by_next_empty_inventory_without_leaks() {
    let fixture = Fixture::new();
    let before = descriptor_count();
    FAIL_CREATE_ENDPOINT.store(2, Ordering::SeqCst);
    let partial = Message::new(&fixture, &[1, 2], 2);
    assert!(!partial.apply(&fixture));
    assert_eq!(fixture.registry.devices().len(), 1);
    assert_eq!(LIVE.load(Ordering::SeqCst), 1);
    // An older complete inventory must not erase partially applied newer intent.
    let stale = Message::new(&fixture, &[], 1);
    assert!(stale.apply(&fixture));
    assert_eq!(fixture.registry.devices().len(), 1);
    let empty = Message::new(&fixture, &[], 3);
    assert!(empty.apply(&fixture));
    assert!(fixture.registry.devices().is_empty());
    assert_eq!(LIVE.load(Ordering::SeqCst), 0);
    drop((partial, stale, empty));
    assert_eq!(descriptor_count(), before);
}

#[test]
fn same_generation_retry_completes_after_native_allocation_recovers() {
    let fixture = Fixture::new();
    FAIL_CREATE_ENDPOINT.store(2, Ordering::SeqCst);
    let inventory = Message::new(&fixture, &[1, 2], 1);
    assert!(!inventory.apply(&fixture));
    let first = fixture.registry.devices()[0];
    FAIL_CREATE_ENDPOINT.store(0, Ordering::SeqCst);
    assert!(inventory.apply(&fixture));
    assert_eq!(fixture.registry.devices()[0], first);
    assert_eq!(LIVE.load(Ordering::SeqCst), 2);
}

#[test]
fn full_registry_can_replace_one_absent_idle_device() {
    let fixture = Fixture::new();
    let initial = Message::new(&fixture, &(1..=64).collect::<Vec<_>>(), 1);
    assert!(initial.apply(&fixture));
    let original = fixture.registry.devices();
    assert_eq!(original.len(), 64);
    let replacement = Message::new(&fixture, &(2..=65).collect::<Vec<_>>(), 2);
    assert!(replacement.apply(&fixture));
    let current = fixture.registry.devices();
    assert_eq!(current.len(), 64);
    assert!(!current.contains(&original[0]));
    assert_eq!(LIVE.load(Ordering::SeqCst), 64);
    let empty = Message::new(&fixture, &[], 3);
    assert!(empty.apply(&fixture));
    assert_eq!(LIVE.load(Ordering::SeqCst), 0);
}
