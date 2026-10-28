use super::*;
use crate::settlement::Wanted;
use std::ffi::CStr;

unsafe extern "C" {
    fn xpc_dictionary_create(
        keys: *const *const c_char,
        values: *const Object,
        count: usize,
    ) -> Object;
    fn xpc_dictionary_set_uint64(object: Object, key: *const c_char, value: u64);
    fn xpc_dictionary_set_string(object: Object, key: *const c_char, value: *const c_char);
}

fn message(operation: &CStr, fields: &[(&CStr, u64)]) -> Owned {
    // SAFETY: a new dictionary is exclusively owned here; keys and values are copied.
    unsafe {
        let message = Owned::new(xpc_dictionary_create(ptr::null(), ptr::null(), 0)).unwrap();
        xpc_dictionary_set_string(message.raw(), c"operation".as_ptr(), operation.as_ptr());
        for (key, value) in fields {
            xpc_dictionary_set_uint64(message.raw(), key.as_ptr(), *value);
        }
        message
    }
}

fn peer(log: Log) -> Peer {
    Peer {
        object: message(c"peer", &[]),
        subscribed: true,
        settled: None,
        log,
        incompatible: false,
    }
}

fn settled(incarnation: u64, sequence: u64, applied: bool) -> Configured {
    Configured {
        incarnation,
        endpoint: [1; 16],
        sequence,
        rate: 96_000,
        formats: [1, 3],
        applied,
    }
}

#[test]
fn a_stale_connection_neither_bounds_nor_answers_requests_of_a_new_hal_load() {
    let mut old = Log::default();
    old.record(settled(1, 5, true));
    old.replayed(1);
    // A new HAL load replays a rejection of the same value settled before any request.
    let mut new = Log::default();
    new.record(settled(2, 3, false));
    let mut peers = Peers::default();
    peers.values.push(peer(old));
    peers.values.push(peer(new));
    let since = peers.since().unwrap();
    assert_eq!(since.incarnation, 1);
    assert_eq!(peers.settled([1; 16], since, Wanted::Rate(96_000)), None);
    peers.values[1].log.replayed(2);
    let current = peers.values[1].object.raw();
    peers.retire_before(current);
    assert_eq!(peers.values.len(), 1);
    let since = peers.since().unwrap();
    assert_eq!((since.incarnation, since.sequence), (2, 3));
    assert_eq!(peers.settled([1; 16], since, Wanted::Rate(96_000)), None);
    peers.values[0].log.record(settled(2, 4, false));
    assert_eq!(
        peers.settled([1; 16], since, Wanted::Rate(96_000)),
        Some(settled(2, 4, false))
    );
}
