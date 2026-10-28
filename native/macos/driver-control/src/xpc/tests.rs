use super::*;

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

fn read(message: &Owned) -> Option<Report> {
    // SAFETY: the test owns the live dictionary for this call.
    unsafe { report(message.raw()) }
}

#[test]
fn reports_of_another_protocol_version_are_surfaced_not_ignored() {
    for version in [1, 3] {
        // An older HAL's acknowledgement used to be dropped, leaving every change to
        // wait out its whole budget and report pending.
        let ack = message(
            c"inventory_ack",
            &[(c"version", version), (c"generation", 9), (c"applied", 1)],
        );
        assert_eq!(read(&ack), Some(Report::Incompatible));
        let settled = message(c"configuration_settled", &[(c"version", version)]);
        assert_eq!(read(&settled), Some(Report::Incompatible));
    }
    assert_eq!(
        read(&message(c"inventory_request", &[(c"version", 1)])),
        None
    );
}

#[test]
fn current_reports_carry_the_requested_values() {
    let ack = message(
        c"inventory_ack",
        &[(c"version", PROTOCOL), (c"generation", 9), (c"applied", 0)],
    );
    assert_eq!(
        read(&ack),
        Some(Report::Inventory {
            generation: 9,
            settlement: Settlement::Rejected
        })
    );
    let settled = message(
        c"configuration_settled",
        &[
            (c"version", PROTOCOL),
            (c"incarnation", 7),
            (c"endpoint_high", u64::from_be_bytes([1; 8])),
            (c"endpoint_low", u64::from_be_bytes([2; 8])),
            (c"sequence", 12),
            (c"rate", 96_000),
            (c"capture_format", 1),
            (c"render_format", 3),
            (c"applied", 1),
        ],
    );
    let mut endpoint = [1; 16];
    endpoint[8..].fill(2);
    assert_eq!(
        read(&settled),
        Some(Report::Configured(Configured {
            incarnation: 7,
            endpoint,
            sequence: 12,
            rate: 96_000,
            formats: [1, 3],
            applied: true,
        }))
    );
}

#[test]
fn the_replay_marker_names_its_hal_incarnation() {
    let marker = message(
        c"configuration_replayed",
        &[(c"version", PROTOCOL), (c"incarnation", 7), (c"through", 3)],
    );
    assert_eq!(read(&marker), Some(Report::Replayed { incarnation: 7 }));
    let old = message(c"configuration_replayed", &[(c"version", 1)]);
    assert_eq!(read(&old), Some(Report::Incompatible));
}
