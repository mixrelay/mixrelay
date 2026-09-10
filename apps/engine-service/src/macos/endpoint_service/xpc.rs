//! XPC policy is control-plane Rust. C only provides Objective-C block adapters.
use super::SERVICE;
use mixrelay_macos_transport::control_inventory::{
    mr_hal_control_create_inventory_snapshot, mr_hal_control_parse_inventory_request,
    mr_hal_control_parse_transport_replacement_request,
};
use mixrelay_macos_transport::source_control::{
    SourceRequest, mr_engine_control_create_source_attachment_reply,
    mr_engine_control_parse_source_request,
};
use std::{
    collections::BTreeMap,
    ffi::{c_char, c_void},
    os::fd::AsRawFd,
    os::unix::fs::MetadataExt,
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
    fn xpc_dictionary_create_reply(request: Object) -> Object;
}
struct Owned(NonNull<c_void>);
// SAFETY: XPC retains/releases are thread safe; use is restricted to control.
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
        unsafe { xpc_release(self.raw()) };
    }
}
#[derive(Clone, Copy)]
struct Authority {
    audio_server: bool,
    console: bool,
}
struct Peer {
    object: Owned,
    authority: Authority,
    subscribed: bool,
}
#[derive(Default)]
pub(super) struct Peers {
    values: Vec<Peer>,
    replacements: BTreeMap<[u8; 16], u64>,
}

impl Peers {
    pub(super) fn driver_connected(&self) -> bool {
        self.values
            .iter()
            .any(|peer| peer.authority.audio_server && peer.subscribed)
    }
}

/// Whether an authenticated HAL peer has requested the device inventory.
/// This control-plane observation includes an empty inventory: removing the last
/// graph reference removes a device, not the loaded driver. Disconnect events
/// remove the peer before the next observation. Never call this mutex-backed query on RT.
pub fn driver_connected() -> bool {
    SERVICE
        .get()
        .is_some_and(|service| service.peers.lock().driver_connected())
}

fn authority(peer: Object) -> Option<Authority> {
    if peer.is_null() {
        return None;
    }
    let uid = unsafe { xpc_connection_get_euid(peer) };
    let console = std::fs::metadata("/dev/console").is_ok_and(|metadata| metadata.uid() == uid);
    let mut account = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut buffer = [0_u8; 4096];
    let mut result = ptr::null_mut();
    let status = unsafe {
        libc::getpwnam_r(
            c"_coreaudiod".as_ptr(),
            account.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &raw mut result,
        )
    };
    let audio_server = status == 0 && !result.is_null() && unsafe { (*result).pw_uid } == uid;
    (console || audio_server).then_some(Authority {
        audio_server,
        console,
    })
}

/// # Safety
/// Peer is a borrowed live XPC connection from the listener adapter.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_engine_service_accept_peer(peer: Object) -> bool {
    let Some(service) = SERVICE.get() else {
        return false;
    };
    let Some(authority) = authority(peer) else {
        return false;
    };
    let mut peers = service.peers.lock();
    if peers.values.iter().any(|entry| entry.object.raw() == peer) {
        return true;
    }
    if peers.values.len() == 128 {
        return false;
    }
    let Some(object) = Owned::new(unsafe { xpc_retain(peer) }) else {
        return false;
    };
    peers.values.push(Peer {
        object,
        authority,
        subscribed: false,
    });
    true
}

fn inventory() -> Option<Owned> {
    let snapshot = SERVICE.get()?.runtime.snapshot().ok()?;
    let abi = snapshot.as_abi();
    Owned::new(unsafe {
        mr_hal_control_create_inventory_snapshot(abi.entries.as_ptr(), abi.count, abi.generation)
    })
}

fn source_reply(event: Object) -> Option<Owned> {
    let service = SERVICE.get()?;
    let mut request = SourceRequest {
        source_id: [0; 128],
        capacity_frames: 0,
        channels: 0,
        transport_version: 0,
    };
    if !unsafe { mr_engine_control_parse_source_request(event, &raw mut request) } {
        return None;
    }
    let Ok(attachment) = service.sources.attach(&request) else {
        return Owned::new(unsafe { xpc_dictionary_create_reply(event) });
    };
    Owned::new(unsafe {
        mr_engine_control_create_source_attachment_reply(
            event,
            attachment.descriptor.as_raw_fd(),
            attachment.mapped_size,
            attachment.producer_lease,
            attachment.source_epoch,
        )
    })
}

/// # Safety
/// Both values are live borrowed XPC objects for this dispatch callback.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_engine_service_peer_event(peer: Object, event: Object) {
    let Some(service) = SERVICE.get() else {
        return;
    };
    if event.is_null() {
        return;
    }
    if unsafe { xpc_get_type(event) } == (&raw const _xpc_type_error).cast() {
        service
            .peers
            .lock()
            .values
            .retain(|entry| entry.object.raw() != peer);
        return;
    }
    let authority = service
        .peers
        .lock()
        .values
        .iter()
        .find(|entry| entry.object.raw() == peer)
        .map(|entry| entry.authority);
    let Some(authority) = authority else {
        unsafe { xpc_connection_cancel(peer) };
        return;
    };
    let mut id = [0; 16];
    let mut generation = 0;
    let inventory_request = unsafe { mr_hal_control_parse_inventory_request(event) };
    let replacement = unsafe {
        mr_hal_control_parse_transport_replacement_request(
            event,
            id.as_mut_ptr(),
            &raw mut generation,
        )
    };
    let response = if authority.audio_server && (inventory_request || replacement) {
        if let Some(entry) = service
            .peers
            .lock()
            .values
            .iter_mut()
            .find(|entry| entry.object.raw() == peer)
        {
            entry.subscribed = true;
        }
        if replacement {
            // Bound the queue to real identities in the current inventory. A
            // graph transaction can defer the request, never silently lose it.
            if service.runtime.snapshot().is_ok_and(|snapshot| {
                snapshot
                    .entries()
                    .iter()
                    .any(|entry| entry.endpoint_id == id && entry.generation == generation)
            }) {
                service.peers.lock().replacements.insert(id, generation);
            }
            None
        } else {
            inventory()
        }
    } else if authority.console && !inventory_request && !replacement {
        source_reply(event).or_else(|| {
            (unsafe { xpc_get_type(event) } == (&raw const _xpc_type_dictionary).cast())
                .then(|| Owned::new(unsafe { xpc_dictionary_create_reply(event) }))
                .flatten()
        })
    } else {
        None
    };
    if let Some(response) = response {
        unsafe { xpc_connection_send_message(peer, response.raw()) };
    }
}

#[unsafe(no_mangle)]
pub(crate) extern "C" fn mr_engine_service_control_tick() {
    let Some(service) = SERVICE.get() else {
        return;
    };
    match service.runtime.tick() {
        Ok(true) => service
            .notify
            .store(true, std::sync::atomic::Ordering::Release),
        Ok(false) => {}
        Err(error) => {
            tracing::error!(%error, "endpoint commit failed");
            return;
        }
    }
    let requests = std::mem::take(&mut service.peers.lock().replacements);
    for (id, generation) in requests {
        match service.runtime.replace(id, generation) {
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                service.peers.lock().replacements.insert(id, generation);
            }
            Ok(_) | Err(_) => {}
        }
    }
    if !service
        .notify
        .swap(false, std::sync::atomic::Ordering::AcqRel)
    {
        return;
    }
    let Some(snapshot) = inventory() else {
        service
            .notify
            .store(true, std::sync::atomic::Ordering::Release);
        return;
    };
    for peer in &service.peers.lock().values {
        if peer.subscribed {
            unsafe { xpc_connection_send_message(peer.object.raw(), snapshot.raw()) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Authority, Object, Owned, Peer, Peers};
    use std::{ffi::c_char, ptr};

    unsafe extern "C" {
        fn xpc_dictionary_create(
            keys: *const *const c_char,
            values: *const Object,
            count: usize,
        ) -> Object;
    }

    #[test]
    fn driver_presence_requires_subscription_and_tracks_disconnect() {
        let mut peers = Peers::default();
        assert!(!peers.driver_connected());
        // The query only examines peer metadata. A real +1 XPC object exercises
        // ownership/drop without opening a connection or touching the installed HAL.
        let object = unsafe { xpc_dictionary_create(ptr::null(), ptr::null(), 0) };
        peers.values.push(Peer {
            object: Owned::new(object).expect("empty XPC dictionary allocation"),
            authority: Authority {
                audio_server: false,
                console: true,
            },
            subscribed: true,
        });
        assert!(!peers.driver_connected());
        peers.values[0].authority.audio_server = true;
        peers.values[0].subscribed = false;
        assert!(!peers.driver_connected());
        peers.values[0].subscribed = true;
        assert!(peers.driver_connected());
        peers.values.clear();
        assert!(!peers.driver_connected());
    }
}
