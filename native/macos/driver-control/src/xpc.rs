//! Authenticated HAL inventory transport; no graph or application-capture API.
use super::{MacDriver, SERVICE, Service};
use mixrelay_macos_transport::control_inventory::{
    mr_hal_control_create_inventory_snapshot, mr_hal_control_parse_inventory_request,
    mr_hal_control_parse_transport_replacement_request,
};
use std::{
    collections::BTreeMap,
    ffi::{CStr, c_char, c_void},
    ptr::{self, NonNull},
};

type Object = *mut c_void;
unsafe extern "C" {
    static _xpc_type_error: c_char;
    static _xpc_type_dictionary: c_char;
    fn xpc_get_type(object: Object) -> *const c_void;
    fn xpc_retain(object: Object) -> Object;
    fn xpc_release(object: Object);
    fn xpc_connection_get_euid(peer: Object) -> u32;
    fn xpc_connection_cancel(peer: Object);
    fn xpc_connection_send_message(peer: Object, message: Object);
    fn xpc_dictionary_get_uint64(object: Object, key: *const c_char) -> u64;
    fn xpc_dictionary_get_string(object: Object, key: *const c_char) -> *const c_char;
}

struct Owned(NonNull<c_void>);
// SAFETY: XPC objects support thread-safe retain/release; control owns these refs.
unsafe impl Send for Owned {}
impl Owned {
    fn new(pointer: Object) -> Option<Self> {
        NonNull::new(pointer).map(Self)
    }
    fn raw(&self) -> Object {
        self.0.as_ptr()
    }
}
impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns exactly one retained or created reference.
        unsafe { xpc_release(self.raw()) };
    }
}

struct Peer {
    object: Owned,
    subscribed: bool,
    acknowledged: u64,
}

#[derive(Default)]
pub(super) struct Peers {
    values: Vec<Peer>,
    replacements: BTreeMap<[u8; 16], u64>,
}
impl Peers {
    pub(super) fn driver_connected(&self) -> bool {
        self.values.iter().any(|peer| peer.subscribed)
    }
    pub(super) fn acknowledged(&self, generation: u64) -> bool {
        self.values
            .iter()
            .any(|peer| peer.subscribed && peer.acknowledged == generation)
    }
    pub(super) fn send_inventory(&self, service: &Service) {
        let Some(message) = inventory(service) else {
            return;
        };
        for peer in self.values.iter().filter(|peer| peer.subscribed) {
            // SAFETY: this collection retains each connection and message.
            unsafe { xpc_connection_send_message(peer.object.raw(), message.raw()) };
        }
    }
}

fn audio_server(peer: Object) -> bool {
    if peer.is_null() {
        return false;
    }
    let mut account = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut buffer = [0_u8; 4096];
    let mut result = ptr::null_mut();
    // SAFETY: getpwnam_r initializes the account into caller-owned aligned
    // storage and only exposes it when status/result confirm success.
    let status = unsafe {
        libc::getpwnam_r(
            c"_coreaudiod".as_ptr(),
            account.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &raw mut result,
        )
    };
    status == 0 && !result.is_null() && unsafe { (*result).pw_uid == xpc_connection_get_euid(peer) }
}

fn inventory(service: &Service) -> Option<Owned> {
    let published = service.published.lock();
    let snapshot = published.as_ref()?.as_abi();
    // SAFETY: published retains every descriptor through synchronous message
    // creation; the codec duplicates them before this mutex guard is released.
    Owned::new(unsafe {
        mr_hal_control_create_inventory_snapshot(
            snapshot.entries.as_ptr(),
            snapshot.count,
            snapshot.generation,
        )
    })
}

fn acknowledge(service: &Service, entry: &mut Peer, event: Object) -> bool {
    // SAFETY: peer_event has authenticated the retained peer and borrows its
    // live event throughout this helper. XPC owns each returned string extent.
    unsafe {
        if xpc_get_type(event) != (&raw const _xpc_type_dictionary).cast() {
            return false;
        }
        let operation = xpc_dictionary_get_string(event, c"operation".as_ptr());
        if operation.is_null()
            || CStr::from_ptr(operation) != c"inventory_ack"
            || xpc_dictionary_get_uint64(event, c"version".as_ptr()) != 1
        {
            return false;
        }
        let generation = xpc_dictionary_get_uint64(event, c"generation".as_ptr());
        if service
            .published
            .lock()
            .as_ref()
            .is_some_and(|snapshot| snapshot.as_abi().generation == generation)
        {
            entry.acknowledged = generation;
        }
    }
    true
}

/// # Safety
/// Peer is a borrowed live XPC connection from the listener block.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_driver_service_accept_peer(peer: Object) -> bool {
    let Some(service) = SERVICE.get() else {
        return false;
    };
    if !audio_server(peer) {
        return false;
    }
    let mut peers = service.peers.lock();
    if peers.values.iter().any(|entry| entry.object.raw() == peer) {
        return true;
    }
    if peers.values.len() >= 128 || peers.values.try_reserve(1).is_err() {
        return false;
    }
    // SAFETY: listener callback retains its borrowed peer throughout this call.
    let Some(object) = Owned::new(unsafe { xpc_retain(peer) }) else {
        return false;
    };
    peers.values.push(Peer {
        object,
        subscribed: false,
        acknowledged: 0,
    });
    true
}

/// # Safety
/// Peer and event are live borrowed XPC objects for this dispatch callback.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_driver_service_peer_event(peer: Object, event: Object) {
    let Some(service) = SERVICE.get() else {
        return;
    };
    if event.is_null() {
        return;
    }
    // SAFETY: event is retained by the block; the error type singleton is static.
    if unsafe { xpc_get_type(event) } == (&raw const _xpc_type_error).cast() {
        service
            .peers
            .lock()
            .values
            .retain(|entry| entry.object.raw() != peer);
        return;
    }
    let mut peers = service.peers.lock();
    let Some(entry) = peers
        .values
        .iter_mut()
        .find(|entry| entry.object.raw() == peer)
    else {
        // SAFETY: borrowed peer remains live through the callback.
        unsafe { xpc_connection_cancel(peer) };
        return;
    };
    // Only the authenticated HAL sends ACKs, after all attachment perform
    // callbacks have drained and committed. Never accept a future generation.
    if acknowledge(service, entry, event) {
        return;
    }
    let mut id = [0; 16];
    let mut generation = 0;
    // SAFETY: codec borrows event and writes only the exact initialized outputs.
    let request = unsafe { mr_hal_control_parse_inventory_request(event) };
    let replacement = unsafe {
        mr_hal_control_parse_transport_replacement_request(
            event,
            id.as_mut_ptr(),
            &raw mut generation,
        )
    };
    if !request && !replacement {
        return;
    }
    entry.subscribed = true;
    if replacement {
        if service
            .registry
            .callback(id)
            .is_some_and(|pin| pin.generation() == generation)
        {
            peers.replacements.insert(id, generation);
        }
    } else if let Some(message) = inventory(service) {
        // SAFETY: both peer and the created message remain retained here.
        unsafe { xpc_connection_send_message(peer, message.raw()) };
    }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn mr_driver_service_control_tick() {
    let Some(service) = SERVICE.get() else {
        return;
    };
    // A transaction owns provisional inventory; never start mapping recovery
    // against it. The timer is control-only and does not own PCM publication.
    let Some(transaction) = service.transaction.try_lock() else {
        return;
    };
    let requests = std::mem::take(&mut service.peers.lock().replacements);
    drop(transaction);
    for (id, generation) in requests {
        if service
            .registry
            .callback(id)
            .is_none_or(|pin| pin.generation() != generation)
        {
            continue;
        }
        let desired = service.configuration.lock().clone();
        let mut driver = MacDriver { service };
        // The next HAL heartbeat requests recovery again if still terminal.
        let _ = driver.replace_inventory_change(&desired, Some(id));
    }
    service.registry.collect();
}
