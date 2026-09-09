use std::ffi::c_void;
use std::io::{Read, Seek, SeekFrom, Write};
use std::mem::ManuallyDrop;
use std::os::fd::{FromRawFd, OwnedFd};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};

use crate::abi::{
    CONFIG_ACTION, Geometry, HostOps, INVALID, NATIVE_ERROR, NOT_FOUND, NativeOps, OK, Timestamp,
};
use crate::device::lock;
use crate::registry::{IoOperation, Registry};

#[path = "inventory_tests.rs"]
mod inventory_tests;
#[path = "native_tests.rs"]
mod native_tests;
#[path = "property_catalog_tests.rs"]
mod property_catalog_tests;

static SERIAL: Mutex<()> = Mutex::new(());
static NEXT: AtomicUsize = AtomicUsize::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static REPLACEMENTS: AtomicUsize = AtomicUsize::new(0);
static FAIL_REPLACE: AtomicBool = AtomicBool::new(false);
static FAIL_CREATE_ENDPOINT: AtomicU8 = AtomicU8::new(0);

struct Runtime {
    geometry: Geometry,
    rate: u32,
    running: bool,
    epoch: u64,
    value: f32,
}

unsafe extern "C" fn inspect(fd: i32, size: usize, output: *mut Geometry) -> bool {
    if size != 24 {
        return false;
    }
    let mut file = ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(fd) });
    let mut bytes = [0; 24];
    if file
        .seek(SeekFrom::Start(0))
        .and_then(|_| file.read_exact(&mut bytes))
        .is_err()
    {
        return false;
    }
    unsafe {
        output.write(Geometry {
            endpoint_id: bytes[..16].try_into().unwrap(),
            render_channels: u32::from_ne_bytes(bytes[16..20].try_into().unwrap()),
            capture_channels: u32::from_ne_bytes(bytes[20..24].try_into().unwrap()),
        });
    };
    true
}

unsafe extern "C" fn create(geometry: *const Geometry, rate: u32, _: i32, _: usize) -> *mut c_void {
    if unsafe { (*geometry).endpoint_id[0] } == FAIL_CREATE_ENDPOINT.load(Ordering::SeqCst) {
        return std::ptr::null_mut();
    }
    LIVE.fetch_add(1, Ordering::SeqCst);
    Box::into_raw(Box::new(Runtime {
        geometry: unsafe { *geometry },
        rate,
        running: false,
        epoch: 0,
        value: 0.0,
    }))
    .cast()
}
unsafe extern "C" fn destroy(runtime: *mut c_void) {
    drop(unsafe { Box::from_raw(runtime.cast::<Runtime>()) });
    LIVE.fetch_sub(1, Ordering::SeqCst);
}
unsafe extern "C" fn replace(
    runtime: *mut c_void,
    geometry: *const Geometry,
    rate: u32,
    _: i32,
    _: usize,
) -> bool {
    if FAIL_REPLACE.load(Ordering::SeqCst) {
        return false;
    }
    let runtime = unsafe { &mut *runtime.cast::<Runtime>() };
    runtime.geometry = unsafe { *geometry };
    runtime.rate = rate;
    runtime.epoch += 1;
    REPLACEMENTS.fetch_add(1, Ordering::SeqCst);
    true
}
unsafe extern "C" fn start(runtime: *mut c_void, _: u64) -> bool {
    let runtime = unsafe { &mut *runtime.cast::<Runtime>() };
    runtime.running = true;
    runtime.epoch += 1;
    true
}
unsafe extern "C" fn stop(runtime: *mut c_void) {
    unsafe { &mut *runtime.cast::<Runtime>() }.running = false;
}
unsafe extern "C" fn remaining(_: *mut c_void) {}
unsafe extern "C" fn requires_replacement(_: *mut c_void) -> bool {
    false
}
unsafe extern "C" fn same_transport(_: *mut c_void, _: i32, _: usize) -> bool {
    false
}
unsafe extern "C" fn set_rate(runtime: *mut c_void, rate: u32) -> bool {
    unsafe { &mut *runtime.cast::<Runtime>() }.rate = rate;
    true
}
unsafe extern "C" fn timestamp(runtime: *mut c_void, now: u64, output: *mut Timestamp) -> bool {
    let runtime = unsafe { &*runtime.cast::<Runtime>() };
    unsafe {
        output.write(Timestamp {
            sample_time: u64::from(runtime.rate),
            host_time: now,
            seed: runtime.epoch,
        });
    };
    runtime.running
}
unsafe extern "C" fn io(
    runtime: *mut c_void,
    render: bool,
    _: u64,
    samples: *mut f32,
    frames: u32,
) -> bool {
    let runtime = unsafe { &mut *runtime.cast::<Runtime>() };
    if render {
        runtime.value = unsafe { *samples };
    } else {
        for index in 0..frames as usize * runtime.geometry.capture_channels as usize {
            unsafe { *samples.add(index) = runtime.value };
        }
    }
    true
}

struct Host {
    requests: Mutex<Vec<(u32, u64)>>,
    changes: Mutex<Vec<(u32, u32)>>,
    fail: AtomicBool,
    registry: AtomicPtr<Registry>,
    replace_during_request: AtomicBool,
}
unsafe extern "C" fn changed(context: *mut c_void, object: u32, change: u32) {
    let host = unsafe { &*context.cast::<Host>() };
    lock(&host.changes).push((object, change));
    if let Some(registry) = unsafe { host.registry.load(Ordering::Acquire).as_ref() } {
        let _ = registry.devices();
    }
}
unsafe extern "C" fn request(
    context: *mut c_void,
    object: u32,
    action: u64,
    generation: u64,
) -> bool {
    assert_eq!(action, CONFIG_ACTION);
    let host = unsafe { &*context.cast::<Host>() };
    lock(&host.requests).push((object, generation));
    if host.replace_during_request.swap(false, Ordering::SeqCst) {
        let registry = unsafe { &*host.registry.load(Ordering::Acquire) };
        assert_eq!(registry.set_rate(object, 96_000), OK);
    }
    !host.fail.load(Ordering::SeqCst)
}

struct Fixture {
    registry: Box<Registry>,
    host: Box<Host>,
    directory: PathBuf,
    _serial: MutexGuard<'static, ()>,
}
impl Fixture {
    fn new() -> Self {
        let serial = lock(&SERIAL);
        assert_eq!(LIVE.load(Ordering::SeqCst), 0);
        FAIL_REPLACE.store(false, Ordering::SeqCst);
        FAIL_CREATE_ENDPOINT.store(0, Ordering::SeqCst);
        REPLACEMENTS.store(0, Ordering::SeqCst);
        let host = Box::new(Host {
            requests: Mutex::new(Vec::new()),
            changes: Mutex::new(Vec::new()),
            fail: AtomicBool::new(false),
            registry: AtomicPtr::default(),
            replace_during_request: AtomicBool::new(false),
        });
        let ops = NativeOps {
            inspect,
            create,
            destroy,
            replace,
            start,
            stop,
            remaining_client: remaining,
            set_rate,
            timestamp,
            io,
            requires_replacement,
            same_transport,
        };
        let registry = Box::new(Registry::new(
            ops,
            HostOps {
                context: (&raw const *host).cast_mut().cast(),
                changed,
                request_configuration: request,
            },
        ));
        host.registry
            .store((&raw const *registry).cast_mut(), Ordering::Release);
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../target/test-artifacts/hal-core")
            .join(format!(
                "{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
        std::fs::create_dir_all(&directory).unwrap();
        Self {
            registry,
            host,
            directory,
            _serial: serial,
        }
    }

    fn fd(&self, endpoint: u8, render: u32, capture: u32) -> OwnedFd {
        let path = self
            .directory
            .join(NEXT.fetch_add(1, Ordering::SeqCst).to_string());
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(path)
            .unwrap();
        file.write_all(&[endpoint; 16]).unwrap();
        file.write_all(&render.to_ne_bytes()).unwrap();
        file.write_all(&capture.to_ne_bytes()).unwrap();
        file.into()
    }

    fn add(&self, endpoint: u8, rate: u32) -> u32 {
        self.registry
            .upsert(
                [endpoint; 16],
                &format!("Device {endpoint}"),
                rate,
                (self.fd(endpoint, 2, 2), 24),
            )
            .unwrap()
    }

    fn pending(&self) -> (u32, u64) {
        *lock(&self.host.requests).last().unwrap()
    }
    fn perform(&self) -> i32 {
        let (id, generation) = self.pending();
        self.registry.perform(id, CONFIG_ACTION, generation)
    }

    fn property<const N: usize>(
        &self,
        object: u32,
        address: &crate::properties::Address,
    ) -> [u8; N] {
        let mut bytes = [0; N];
        let mut written = 0;
        assert_eq!(
            unsafe {
                crate::properties::tn_hal_core_property_get(
                    (&raw const *self.registry).cast_mut(),
                    object,
                    address,
                    0,
                    std::ptr::null(),
                    u32::try_from(N).unwrap(),
                    &raw mut written,
                    bytes.as_mut_ptr().cast(),
                )
            },
            0
        );
        assert_eq!(written as usize, N);
        bytes
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
}

#[test]
fn independent_devices_enumerate_and_route_without_cross_talk() {
    let fixture = Fixture::new();
    let a = fixture.add(1, 48_000);
    let b = fixture.add(2, 96_000);
    assert_eq!(fixture.registry.devices(), [a, b]);
    for id in [a, b] {
        assert_eq!(fixture.registry.client(id, 3, true), OK);
        assert_eq!(fixture.registry.start(id, 3, 100), OK);
    }
    let mut first = [0.25; 64];
    let mut second = [0.75; 64];
    unsafe {
        assert!(fixture.registry.io(IoOperation {
            device: a,
            stream: a + 2,
            render: true,
            frame: 0,
            samples: first.as_mut_ptr(),
            frames: 32
        }));
        assert!(fixture.registry.io(IoOperation {
            device: b,
            stream: b + 2,
            render: true,
            frame: 0,
            samples: second.as_mut_ptr(),
            frames: 32
        }));
        assert!(fixture.registry.io(IoOperation {
            device: a,
            stream: a + 1,
            render: false,
            frame: 0,
            samples: second.as_mut_ptr(),
            frames: 32
        }));
        assert!(fixture.registry.io(IoOperation {
            device: b,
            stream: b + 1,
            render: false,
            frame: 0,
            samples: first.as_mut_ptr(),
            frames: 32
        }));
    }
    assert_eq!(first.map(f32::to_bits), [0.75_f32.to_bits(); 64]);
    assert_eq!(second.map(f32::to_bits), [0.25_f32.to_bits(); 64]);
    assert_eq!(fixture.registry.snapshot(a).unwrap().sample_rate, 48_000);
    assert_eq!(fixture.registry.snapshot(b).unwrap().sample_rate, 96_000);
}

#[test]
fn stale_configuration_callbacks_cannot_drop_new_transport() {
    let fixture = Fixture::new();
    let id = fixture.add(1, 48_000);
    fixture
        .registry
        .upsert([1; 16], "Old", 48_000, (fixture.fd(1, 7, 3), 24))
        .unwrap();
    let old = fixture.pending().1;
    fixture
        .registry
        .upsert([1; 16], "Newest", 48_000, (fixture.fd(1, 13, 33), 24))
        .unwrap();
    assert_eq!(fixture.registry.abort(id, CONFIG_ACTION, old), OK);
    assert_eq!(fixture.registry.perform(id, CONFIG_ACTION, old), OK);
    assert_eq!(fixture.perform(), OK);
    assert_eq!(fixture.perform(), OK);
    let snapshot = fixture.registry.snapshot(id).unwrap();
    assert_eq!(
        (snapshot.render_channels, snapshot.capture_channels),
        (13, 33)
    );
    assert_eq!(&snapshot.name[..6], b"Newest");
    assert_eq!(REPLACEMENTS.load(Ordering::SeqCst), 1);
}

#[test]
fn pending_descriptor_ownership_closes_superseded_and_aborted_fds() {
    use std::os::fd::AsRawFd;
    unsafe extern "C" {
        fn fcntl(fd: i32, command: i32) -> i32;
    }
    let fixture = Fixture::new();
    let id = fixture.add(1, 48_000);
    let first = fixture.fd(1, 7, 3);
    let old_fd = first.as_raw_fd();
    fixture
        .registry
        .upsert([1; 16], "Old", 48_000, (first, 24))
        .unwrap();
    assert!(unsafe { fcntl(old_fd, 1) } >= 0);
    let second = fixture.fd(1, 8, 2);
    let mut new_fd = second.as_raw_fd();
    fixture
        .registry
        .upsert([1; 16], "New", 48_000, (second, 24))
        .unwrap();
    assert_eq!(unsafe { fcntl(old_fd, 1) }, -1);
    assert!(unsafe { fcntl(new_fd, 1) } >= 0);
    for _ in 0..1000 {
        let next = fixture.fd(1, 8, 2);
        let next_fd = next.as_raw_fd();
        fixture
            .registry
            .upsert([1; 16], "Newest", 48_000, (next, 24))
            .unwrap();
        assert_eq!(unsafe { fcntl(new_fd, 1) }, -1);
        new_fd = next_fd;
    }
    assert_eq!(
        fixture
            .registry
            .abort(id, CONFIG_ACTION, fixture.pending().1),
        OK
    );
    assert_eq!(unsafe { fcntl(new_fd, 1) }, -1);
}

#[test]
fn failed_host_requests_retry_and_new_generation_resets_backoff() {
    let fixture = Fixture::new();
    let id = fixture.add(1, 48_000);
    fixture.host.fail.store(true, Ordering::SeqCst);
    fixture
        .registry
        .upsert([1; 16], "Pending", 48_000, (fixture.fd(1, 7, 3), 24))
        .unwrap();
    for _ in 0..1000 {
        fixture.registry.tick();
    }
    assert!(lock(&fixture.host.requests).len() > 10);
    fixture.host.fail.store(false, Ordering::SeqCst);
    fixture
        .registry
        .upsert([1; 16], "Recovered", 48_000, (fixture.fd(1, 8, 2), 24))
        .unwrap();
    assert_eq!(fixture.perform(), OK);
    assert_eq!(fixture.registry.snapshot(id).unwrap().render_channels, 8);
}

#[test]
fn acknowledged_configuration_without_perform_retries_until_cancelled() {
    use crate::registry::CONFIG_ACK_TIMEOUT_TICKS;

    let fixture = Fixture::new();
    let id = fixture.add(1, 48_000);
    assert_eq!(fixture.registry.set_rate(id, 96_000), OK);
    let token = fixture.pending();
    for _ in 0..CONFIG_ACK_TIMEOUT_TICKS - 1 {
        fixture.registry.tick();
    }
    assert_eq!(lock(&fixture.host.requests).len(), 1);
    fixture.registry.tick();
    assert_eq!(&*lock(&fixture.host.requests), &[token, token]);
    for _ in 0..1000 {
        fixture.registry.tick();
    }
    assert_eq!(lock(&fixture.host.requests).len(), 22);
    assert_eq!(fixture.registry.abort(id, CONFIG_ACTION, token.1), OK);
    for _ in 0..CONFIG_ACK_TIMEOUT_TICKS * 2 {
        fixture.registry.tick();
    }
    assert_eq!(lock(&fixture.host.requests).len(), 22);
    assert_eq!(fixture.registry.snapshot(id).unwrap().sample_rate, 48_000);
}

#[test]
fn reentrant_replacement_failure_cannot_backoff_the_new_generation_twice() {
    let fixture = Fixture::new();
    let id = fixture.add(1, 48_000);
    fixture.host.fail.store(true, Ordering::SeqCst);
    fixture
        .host
        .replace_during_request
        .store(true, Ordering::SeqCst);
    assert_eq!(fixture.registry.set_rate(id, 44_100), OK);
    let requests = lock(&fixture.host.requests).clone();
    assert_eq!(requests.len(), 2);
    assert_ne!(requests[0].1, requests[1].1);
    let device = fixture.registry.pin(id).unwrap();
    let control = lock(&device.control);
    let pending = control.pending.as_ref().unwrap();
    assert_eq!((pending.failures, pending.retry_after), (1, 2));
    assert_eq!(pending.rate, 96_000);
    drop(control);
    drop(device);
    fixture.host.fail.store(false, Ordering::SeqCst);
    fixture.registry.tick();
    fixture.registry.tick();
    assert_eq!(fixture.perform(), OK);
    assert_eq!(fixture.registry.snapshot(id).unwrap().sample_rate, 96_000);
}

#[test]
fn remove_keeps_client_lifetime_but_never_reuses_stale_object_id() {
    let fixture = Fixture::new();
    let id = fixture.add(1, 48_000);
    assert_eq!(fixture.registry.client(id, 4, true), OK);
    assert_eq!(fixture.registry.start(id, 4, 100), OK);
    assert_eq!(fixture.registry.remove([1; 16]), OK);
    assert!(fixture.registry.devices().is_empty());
    assert_eq!(LIVE.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.registry.start(id, 5, 200), NOT_FOUND);
    assert_eq!(fixture.registry.stop(id, 4), OK);
    assert_eq!(fixture.registry.client(id, 4, false), OK);
    fixture.registry.tick();
    assert_eq!(LIVE.load(Ordering::SeqCst), 0);
    let replacement = fixture.add(1, 96_000);
    assert_ne!(replacement, id);
    assert!(fixture.registry.snapshot(id).is_none());
    assert_eq!(
        fixture.registry.snapshot(replacement).unwrap().sample_rate,
        96_000
    );
}

#[test]
fn duplicate_client_does_not_reset_running_membership() {
    let fixture = Fixture::new();
    let id = fixture.add(1, 48_000);
    assert_eq!(fixture.registry.client(id, 4, true), OK);
    assert_eq!(fixture.registry.start(id, 4, 100), OK);
    assert_eq!(fixture.registry.client(id, 4, true), INVALID);
    assert_eq!(fixture.registry.stop(id, 4), OK);
    assert!(!fixture.registry.snapshot(id).unwrap().running);
}

#[test]
fn crashed_client_removal_releases_retired_runtime_without_stop_notification() {
    let fixture = Fixture::new();
    let id = fixture.add(1, 48_000);
    assert_eq!(fixture.registry.client(id, 1, true), OK);
    assert_eq!(fixture.registry.start(id, 1, 100), OK);
    assert_eq!(fixture.registry.remove([1; 16]), OK);
    assert_eq!(fixture.registry.client(id, 1, false), OK);
    fixture.registry.tick();
    assert_eq!(LIVE.load(Ordering::SeqCst), 0);
}

#[test]
fn reconnect_retains_live_clock_and_native_rate_failure_is_atomic() {
    let fixture = Fixture::new();
    let id = fixture.add(1, 48_000);
    fixture.registry.client(id, 4, true);
    fixture.registry.start(id, 4, 100);
    assert_eq!(fixture.registry.set_rate(id, 192_000), OK);
    assert_eq!(fixture.perform(), OK);
    fixture
        .registry
        .upsert([1; 16], "Reconnect", 48_000, (fixture.fd(1, 7, 3), 24))
        .unwrap();
    FAIL_REPLACE.store(true, Ordering::SeqCst);
    assert_eq!(fixture.perform(), NATIVE_ERROR);
    let snapshot = fixture.registry.snapshot(id).unwrap();
    assert_eq!(
        (snapshot.render_channels, snapshot.sample_rate),
        (2, 192_000)
    );
    FAIL_REPLACE.store(false, Ordering::SeqCst);
    fixture
        .registry
        .upsert([1; 16], "Reconnect", 48_000, (fixture.fd(1, 7, 3), 24))
        .unwrap();
    assert_eq!(fixture.perform(), OK);
    let mut timestamp = Timestamp::default();
    assert!(fixture.registry.timestamp(id, 200, &mut timestamp));
    assert_eq!(timestamp.sample_time, 192_000);
    assert!(fixture.registry.snapshot(id).unwrap().running);
}

#[test]
fn all_product_rates_are_independent_and_transport_identity_is_validated() {
    let fixture = Fixture::new();
    let id = fixture.add(1, 48_000);
    for &rate in crate::formats::SAMPLE_RATES {
        assert_eq!(fixture.registry.set_rate(id, rate), OK);
        assert_eq!(fixture.perform(), OK);
        assert_eq!(fixture.registry.snapshot(id).unwrap().sample_rate, rate);
    }
    assert_eq!(fixture.registry.set_rate(id, 384_000), INVALID);
    assert_eq!(
        fixture
            .registry
            .upsert([2; 16], "Wrong identity", 48_000, (fixture.fd(3, 2, 2), 24)),
        Err(INVALID)
    );
    assert_eq!(fixture.registry.devices(), [id]);
}

#[test]
fn property_abi_exposes_per_device_stream_shape_and_partial_lists() {
    use crate::properties::{Address, tn_hal_core_property_get, tn_hal_core_property_size};
    let fixture = Fixture::new();
    let first = fixture.add(1, 48_000);
    let second = fixture.add(2, 96_000);
    let registry = (&raw const *fixture.registry).cast_mut();
    let list = Address {
        selector: u32::from_be_bytes(*b"dev#"),
        scope: u32::from_be_bytes(*b"glob"),
        element: 0,
    };
    let mut size = 0;
    assert_eq!(
        unsafe { tn_hal_core_property_size(registry, 1, &raw const list, &raw mut size) },
        0
    );
    assert_eq!(size, 8);
    let mut one = 0_u32;
    assert_eq!(
        unsafe {
            tn_hal_core_property_get(
                registry,
                1,
                &raw const list,
                0,
                std::ptr::null(),
                4,
                &raw mut size,
                (&raw mut one).cast(),
            )
        },
        0
    );
    assert_eq!((size, one), (4, first));
    let format = Address {
        selector: u32::from_be_bytes(*b"sfmt"),
        ..list
    };
    let bytes = fixture.property::<40>(second + 2, &format);
    assert_eq!(
        f64::from_ne_bytes(bytes[..8].try_into().unwrap()).to_bits(),
        96_000_f64.to_bits()
    );
    assert_eq!(u32::from_ne_bytes(bytes[28..32].try_into().unwrap()), 2);
    fixture
        .registry
        .upsert([2; 16], "Discrete", 96_000, (fixture.fd(2, 7, 33), 24))
        .unwrap();
    assert_eq!(fixture.perform(), OK);
    let bytes = fixture.property::<40>(second + 2, &format);
    assert_eq!(u32::from_ne_bytes(bytes[28..32].try_into().unwrap()), 7);
    let layout = Address {
        selector: u32::from_be_bytes(*b"srnd"),
        scope: u32::from_be_bytes(*b"inpt"),
        element: 0,
    };
    assert_eq!(
        unsafe { tn_hal_core_property_size(registry, second, &raw const layout, &raw mut size) },
        0
    );
    assert_eq!(size, 12 + 33 * 20);
}

#[test]
fn property_abi_validates_formats_and_cf_string_ownership() {
    use crate::properties::{Address, tn_hal_core_property_get, tn_hal_core_property_set};
    unsafe extern "C" {
        fn CFRelease(value: *const c_void);
    }
    let fixture = Fixture::new();
    let id = fixture.add(1, 48_000);
    let registry = (&raw const *fixture.registry).cast_mut();
    let property = Address {
        selector: u32::from_be_bytes(*b"nsrt"),
        scope: u32::from_be_bytes(*b"glob"),
        element: 0,
    };
    let invalid = 44_100.5_f64;
    assert_ne!(
        unsafe {
            tn_hal_core_property_set(
                registry,
                id,
                &raw const property,
                8,
                (&raw const invalid).cast(),
            )
        },
        0
    );
    assert!(lock(&fixture.host.requests).is_empty());
    let requested = 192_000_f64;
    assert_eq!(
        unsafe {
            tn_hal_core_property_set(
                registry,
                id,
                &raw const property,
                8,
                (&raw const requested).cast(),
            )
        },
        0
    );
    assert_eq!(fixture.perform(), OK);
    let property = Address {
        selector: u32::from_be_bytes(*b"lnam"),
        ..property
    };
    let mut name: *const c_void = std::ptr::null();
    let mut written = 0;
    assert_eq!(
        unsafe {
            tn_hal_core_property_get(
                registry,
                id,
                &raw const property,
                0,
                std::ptr::null(),
                8,
                &raw mut written,
                (&raw mut name).cast(),
            )
        },
        0
    );
    assert!(!name.is_null());
    assert_eq!(written, 8);
    unsafe { CFRelease(name) };
}

#[test]
fn ffi_rejects_foreign_transport_and_closes_consumed_descriptor() {
    use crate::abi::tn_hal_core_upsert;
    use std::os::fd::IntoRawFd;
    unsafe extern "C" {
        fn fcntl(fd: i32, command: i32) -> i32;
    }
    let fixture = Fixture::new();
    let fd = fixture.fd(2, 2, 2).into_raw_fd();
    let mut output = 99;
    assert_eq!(
        unsafe {
            tn_hal_core_upsert(
                (&raw const *fixture.registry).cast_mut(),
                &[1; 16],
                c"Foreign".as_ptr(),
                48_000,
                fd,
                24,
                &raw mut output,
            )
        },
        INVALID
    );
    assert_eq!(unsafe { fcntl(fd, 1) }, -1);
    assert_eq!(output, 99);
    assert!(fixture.registry.devices().is_empty());
}

#[test]
fn io_device_query_completes_while_metadata_control_is_locked() {
    use crate::abi::tn_hal_core_device_alive;
    use std::sync::mpsc;
    use std::time::Duration;

    let fixture = Fixture::new();
    let id = fixture.add(1, 48_000);
    let registry = fixture.registry.as_ref();
    let pin = registry.pin(id).unwrap();
    let control = lock(&pin.control);
    let (send, receive) = mpsc::channel();
    let response = std::thread::scope(|scope| {
        scope.spawn(|| {
            send.send(unsafe { tn_hal_core_device_alive(registry, id) })
                .unwrap();
        });
        let response = receive.recv_timeout(Duration::from_secs(1));
        // Release before joining even on regression, so a failure cannot hang
        // the test runner behind the deliberately held control mutex.
        drop(control);
        response
    });
    assert_eq!(response, Ok(true));
    drop(pin);
    assert!(!unsafe { tn_hal_core_device_alive(registry, id + 1) });
    assert!(!unsafe { tn_hal_core_device_alive(registry, id + 2) });
    assert!(!unsafe { tn_hal_core_device_alive(std::ptr::null(), id) });
    assert_eq!(registry.remove([1; 16]), OK);
    assert!(!unsafe { tn_hal_core_device_alive(registry, id) });
}

#[test]
fn callback_readers_drain_before_retired_runtime_is_freed() {
    let fixture = Fixture::new();
    let id = fixture.add(1, 48_000);
    assert_eq!(fixture.registry.client(id, 10, true), OK);
    assert_eq!(fixture.registry.start(id, 10, 1), OK);
    let registry = fixture.registry.as_ref();
    let stop = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let reader = scope.spawn(|| {
            let mut timestamp = Timestamp::default();
            while !stop.load(Ordering::Acquire) {
                registry.timestamp(id, 100, &mut timestamp);
            }
        });
        for _ in 0..100 {
            registry.set_rate(id, 96_000);
            fixture.perform();
        }
        assert_eq!(registry.remove([1; 16]), OK);
        assert_eq!(registry.stop(id, 10), OK);
        assert_eq!(registry.client(id, 10, false), OK);
        registry.tick();
        stop.store(true, Ordering::Release);
        reader.join().unwrap();
    });
    assert_eq!(LIVE.load(Ordering::SeqCst), 0);
    assert!(registry.snapshot(id).is_none());
}
