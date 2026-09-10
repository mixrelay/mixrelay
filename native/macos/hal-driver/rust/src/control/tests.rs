use super::*;
use std::{cell::Cell, rc::Rc};

#[derive(Default)]
struct Counts {
    connections: Cell<usize>,
    resumed: Cell<usize>,
    released: Cell<usize>,
    requests: RefCell<Vec<Request>>,
    fail_connection: Cell<bool>,
}
struct Peer(Rc<Counts>);
impl Drop for Peer {
    fn drop(&mut self) {
        self.0.released.set(self.0.released.get() + 1);
    }
}
struct Fake(Rc<Counts>);
impl Backend for Fake {
    type Connection = Peer;
    fn connect(&self, _: *const c_void, _: u64) -> Option<Peer> {
        self.0.connections.set(self.0.connections.get() + 1);
        (!self.0.fail_connection.get()).then(|| Peer(Rc::clone(&self.0)))
    }
    fn resume(&self, _: &Peer) {
        self.0.resumed.set(self.0.resumed.get() + 1);
    }
    fn send(&self, _: &Peer, request: Request) {
        self.0.requests.borrow_mut().push(request);
    }
}

fn owner() -> (Control<Fake>, Rc<Counts>) {
    unsafe extern "C" fn changed(_: *mut c_void, _: u32, _: u32) {}
    unsafe extern "C" fn request(_: *mut c_void, _: u32, _: u64, _: u64) -> bool {
        false
    }
    let host = HostOps {
        context: ptr::null_mut(),
        changed,
        request_configuration: request,
    };
    // SAFETY: create copies the stack callbacks. The empty registry performs no
    // audio-device discovery or mutation; Box adopts the sole returned allocation.
    let registry = unsafe { mr_hal_core_create_native(&raw const host) };
    assert!(!registry.is_null());
    let counts = Rc::new(Counts::default());
    // SAFETY: no callbacks or competing owner reference the fresh allocation.
    let registry = unsafe { Box::from_raw(registry) };
    (Control::new(registry, Fake(Rc::clone(&counts))), counts)
}

fn heartbeat(owner: &Control<Fake>) {
    for _ in 0..10 {
        owner.tick();
    }
}

#[test]
fn thousand_errors_and_stale_callbacks_do_not_reconnect_or_disconnect_current_peer() {
    let (owner, counts) = owner();
    owner.open();
    assert_eq!(counts.connections.get(), 1);
    assert_eq!(counts.requests.borrow().len(), 1);
    for _ in 0..1000 {
        owner.disconnected(1);
    }
    assert_eq!(owner.session.borrow().authority(), 0);
    assert_eq!(counts.connections.get(), 1);
    heartbeat(&owner);
    assert_eq!(owner.session.borrow().authority(), 2);
    for _ in 0..1000 {
        owner.disconnected(1);
    }
    assert_eq!(owner.session.borrow().authority(), 2);
    assert_eq!(counts.connections.get(), 2);
    assert_eq!(counts.released.get(), 1);
    // SAFETY: stale authority must reject before dereferencing even a null payload.
    assert!(!unsafe { owner.message(1, ptr::null_mut()) });
    owner.disconnected(2);
    // SAFETY: disconnected authority also rejects before payload inspection.
    assert!(!unsafe { owner.message(2, ptr::null_mut()) });
    heartbeat(&owner);
    assert_eq!(counts.connections.get(), 3);
    heartbeat(&owner);
    assert_eq!(counts.connections.get(), 3);
    assert_eq!(
        counts.requests.borrow().as_slice(),
        &[Request::Inventory; 4]
    );
    drop(owner);
    assert_eq!(counts.released.get(), 3);
}

#[test]
fn failed_connection_releases_previous_and_waits_for_full_heartbeat() {
    let (owner, counts) = owner();
    owner.open();
    owner.disconnected(1);
    counts.fail_connection.set(true);
    heartbeat(&owner);
    assert_eq!(owner.session.borrow().authority(), 0);
    assert!(owner.connection.borrow().is_none());
    assert_eq!(counts.released.get(), 1);
    assert_eq!(counts.resumed.get(), 1);
    for _ in 0..9 {
        owner.tick();
    }
    assert_eq!(counts.connections.get(), 2);
    counts.fail_connection.set(false);
    owner.tick();
    assert_eq!(owner.session.borrow().authority(), 3);
    owner.disconnected(1);
    assert_eq!(owner.session.borrow().authority(), 3);
    assert_eq!(counts.connections.get(), 3);
    assert_eq!(counts.requests.borrow().len(), 2);
}

#[test]
fn actual_registry_fence_contention_prevents_socket_publication() {
    let (owner, counts) = owner();
    let lock = owner.registry.inventory.lock().unwrap();
    owner.open();
    assert_eq!(owner.session.borrow().authority(), 0);
    assert_eq!(counts.connections.get(), 0);
    drop(lock);
    heartbeat(&owner);
    assert_eq!(owner.session.borrow().authority(), 2);
    assert_eq!(counts.connections.get(), 1);
}

#[test]
fn null_host_initialization_does_not_publish_native_work() {
    // SAFETY: null is an explicitly rejected ABI argument, with no host access.
    let mut registry = ptr::null_mut();
    assert!(!unsafe { mr_hal_control_initialize(ptr::null(), &raw mut registry) });
    assert!(registry.is_null());
}
