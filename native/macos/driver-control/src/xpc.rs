//! Authenticated HAL inventory transport; no graph or application-capture API.
use super::settlement::{Configured, Log, PROTOCOL, Since, Wanted};
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

/// How the HAL reported finishing one inventory generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Settlement {
    Applied,
    Rejected,
}

struct Peer {
    object: Owned,
    subscribed: bool,
    settled: Option<(u64, Settlement)>,
    log: Log,
    incompatible: bool,
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
    pub(super) fn settlement(&self, generation: u64) -> Option<Settlement> {
        self.values
            .iter()
            .filter(|peer| peer.subscribed)
            .find_map(|peer| {
                peer.settled
                    .filter(|(settled, _)| *settled == generation)
                    .map(|(_, settlement)| settlement)
            })
    }
    fn subscribed(&self) -> impl Iterator<Item = &Peer> {
        self.values.iter().filter(|peer| peer.subscribed)
    }
    /// The bound for a request issued now, once the subscribed HAL finished its replay.
    pub(super) fn since(&self) -> Option<Since> {
        self.subscribed().find_map(|peer| peer.log.since())
    }
    /// The first settlement after `since` of a request for exactly `wanted`.
    pub(super) fn settled(
        &self,
        endpoint: [u8; 16],
        since: Since,
        wanted: Wanted,
    ) -> Option<Configured> {
        self.subscribed()
            .find_map(|peer| peer.log.find(endpoint, since, wanted))
    }
    /// Whether a subscribed HAL reports with another protocol version.
    pub(super) fn incompatible(&self) -> bool {
        self.subscribed().any(|peer| peer.incompatible)
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
    let message = Owned::new(unsafe {
        mr_hal_control_create_inventory_snapshot(
            snapshot.entries.as_ptr(),
            snapshot.count,
            snapshot.generation,
        )
    })?;
    let latencies: Vec<_> = service
        .latencies
        .lock()
        .values()
        .filter(|entry| {
            snapshot.entries[..snapshot.count]
                .iter()
                .any(|device| device.endpoint_id == entry.endpoint_id)
        })
        .copied()
        .collect();
    // SAFETY: this newly created message is exclusively owned until send.
    unsafe {
        mixrelay_macos_transport::control_inventory::write_input_latencies(
            message.raw(),
            &latencies,
        )
    }
    .then_some(message)
}

/// One HAL report on the control connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Report {
    Inventory {
        generation: u64,
        settlement: Settlement,
    },
    Configured(Configured),
    /// The HAL resent every settlement it retained on this connection.
    Replayed {
        incarnation: u64,
    },
    /// Clients holding IO on `endpoint` that the HAL could not give a queue after a
    /// configuration change; they are silent until it retries. 0 reports recovery.
    Unprovisioned {
        endpoint: [u8; 16],
        clients: u32,
    },
    /// A report this service cannot read: the HAL comes from another package version.
    Incompatible,
}

/// Reads a HAL report, or `None` for any other message.
/// # Safety
/// `event` is a live XPC object borrowed for this call.
pub(super) unsafe fn report(event: Object) -> Option<Report> {
    let uint = |key: &CStr| unsafe { xpc_dictionary_get_uint64(event, key.as_ptr()) };
    // SAFETY: the caller borrows a live event; XPC owns each returned string extent.
    let operation = unsafe {
        if xpc_get_type(event) != (&raw const _xpc_type_dictionary).cast() {
            return None;
        }
        let operation = xpc_dictionary_get_string(event, c"operation".as_ptr());
        if operation.is_null() {
            return None;
        }
        CStr::from_ptr(operation)
    };
    if operation != c"inventory_ack"
        && operation != c"configuration_settled"
        && operation != c"configuration_replayed"
        && operation != c"clients_unprovisioned"
    {
        return None;
    }
    if uint(c"version") != PROTOCOL {
        return Some(Report::Incompatible);
    }
    let applied = uint(c"applied") == 1;
    if operation == c"inventory_ack" {
        return Some(Report::Inventory {
            generation: uint(c"generation"),
            settlement: if applied {
                Settlement::Applied
            } else {
                Settlement::Rejected
            },
        });
    }
    if operation == c"configuration_replayed" {
        return Some(Report::Replayed {
            incarnation: uint(c"incarnation"),
        });
    }
    let mut endpoint = [0; 16];
    endpoint[..8].copy_from_slice(&uint(c"endpoint_high").to_be_bytes());
    endpoint[8..].copy_from_slice(&uint(c"endpoint_low").to_be_bytes());
    let format = |key| u32::try_from(uint(key)).unwrap_or(0);
    if operation == c"clients_unprovisioned" {
        return Some(Report::Unprovisioned {
            endpoint,
            clients: format(c"clients"),
        });
    }
    Some(Report::Configured(Configured {
        incarnation: uint(c"incarnation"),
        endpoint,
        sequence: uint(c"sequence"),
        rate: format(c"rate"),
        formats: [format(c"capture_format"), format(c"render_format")],
        applied,
    }))
}

impl Peer {
    fn receive(&mut self, service: &Service, report: Report) {
        match report {
            Report::Inventory {
                generation,
                settlement,
            } => {
                // Never accept a settlement for anything but the current publication.
                if service
                    .published
                    .lock()
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.as_abi().generation == generation)
                {
                    self.settled = Some((generation, settlement));
                }
            }
            Report::Configured(configured) => self.log.record(configured),
            Report::Replayed { incarnation } => self.log.replayed(incarnation),
            Report::Unprovisioned { endpoint, clients } => {
                let endpoint = endpoint.map(|byte| format!("{byte:02x}")).concat();
                if clients == 0 {
                    tracing::info!(%endpoint, "HAL clients regained their output queues");
                } else {
                    tracing::warn!(
                        %endpoint,
                        clients,
                        "HAL clients hold IO without an output queue and are silent until the HAL retries"
                    );
                }
            }
            Report::Incompatible => self.incompatible = true,
        }
    }
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
        settled: None,
        log: Log::default(),
        incompatible: false,
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
    // Only the authenticated HAL sends reports, after its Perform callbacks settled.
    // SAFETY: the dispatch block retains the event for this callback.
    if let Some(report) = unsafe { report(event) } {
        entry.receive(service, report);
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

#[cfg(test)]
mod tests;
