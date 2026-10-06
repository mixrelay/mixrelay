//! XPC policy is control-plane Rust. C only provides Objective-C block adapters.
use super::SERVICE;
use mixrelay_macos_transport::source_control::{
    SourceRequest, mr_engine_control_create_source_attachment_reply,
    mr_engine_control_parse_source_request,
};
use std::{
    ffi::{c_char, c_void},
    os::fd::AsRawFd,
    os::unix::fs::MetadataExt,
    ptr::NonNull,
};
type Object = *mut c_void;
unsafe extern "C" {
    static _xpc_type_error: c_char;
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
struct Peer {
    object: Owned,
}
#[derive(Default)]
pub(super) struct Peers {
    values: Vec<Peer>,
}

fn authorized(peer: Object) -> bool {
    if peer.is_null() {
        return false;
    }
    // SAFETY: the listener supplies a retained live peer for this control call.
    let uid = unsafe { xpc_connection_get_euid(peer) };
    std::fs::metadata("/dev/console").is_ok_and(|metadata| metadata.uid() == uid)
}

/// # Safety
/// Peer is a borrowed live XPC connection from the listener adapter.
#[unsafe(no_mangle)]
pub(crate) unsafe extern "C" fn mr_engine_service_accept_peer(peer: Object) -> bool {
    let Some(service) = SERVICE.get() else {
        return false;
    };
    if !authorized(peer) {
        return false;
    }
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
    peers.values.push(Peer { object });
    true
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
        // SAFETY: this borrowed dictionary request is retained for the callback;
        // an empty owned reply lets the source client reject the failed attachment.
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
    if !service
        .peers
        .lock()
        .values
        .iter()
        .any(|entry| entry.object.raw() == peer)
    {
        // SAFETY: peer remains retained through this listener callback.
        unsafe { xpc_connection_cancel(peer) };
        return;
    }
    if let Some(response) = source_reply(event) {
        // SAFETY: response retains the owned reply until this synchronous send.
        unsafe { xpc_connection_send_message(peer, response.raw()) };
    }
}
