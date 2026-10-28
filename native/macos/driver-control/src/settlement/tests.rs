use super::*;

fn settled(endpoint: u8, sequence: u64, rate: u32, applied: bool) -> Configured {
    Configured {
        endpoint: [endpoint; 16],
        sequence,
        rate,
        formats: [1, 3],
        applied,
    }
}

#[test]
fn a_rejection_followed_by_other_settlements_is_still_attributed_to_its_request() {
    let mut log = Log::default();
    // Reporting only the newest settlement per device lost this rejection.
    log.record(settled(1, 5, 96_000, false));
    log.record(settled(1, 6, 48_000, true));
    log.record(settled(2, 7, 96_000, true));
    assert_eq!(
        log.find([1; 16], 4, Wanted::Rate(96_000)),
        Some(settled(1, 5, 96_000, false))
    );
    assert_eq!(log.newest(), 7);
}

#[test]
fn an_earlier_settlement_for_another_value_or_device_is_not_attributed() {
    let mut log = Log::default();
    log.record(settled(1, 5, 88_200, false));
    log.record(settled(2, 6, 96_000, false));
    assert_eq!(log.find([1; 16], 4, Wanted::Rate(96_000)), None);
    // Settled before the request was issued.
    log.record(settled(1, 7, 96_000, false));
    assert_eq!(log.find([1; 16], 7, Wanted::Rate(96_000)), None);
    log.record(settled(1, 8, 96_000, true));
    assert_eq!(
        log.find([1; 16], 7, Wanted::Rate(96_000)),
        Some(settled(1, 8, 96_000, true))
    );
}

#[test]
fn format_requests_match_their_direction_rate_and_sample_format() {
    let mut log = Log::default();
    log.record(settled(1, 5, 48_000, false));
    let wanted = |render, rate, format| Wanted::Format {
        render,
        rate,
        format,
    };
    assert!(log.find([1; 16], 4, wanted(true, 48_000, 3)).is_some());
    assert!(log.find([1; 16], 4, wanted(false, 48_000, 1)).is_some());
    assert!(log.find([1; 16], 4, wanted(true, 48_000, 1)).is_none());
    assert!(log.find([1; 16], 4, wanted(true, 96_000, 3)).is_none());
}

#[test]
fn settlements_for_many_endpoints_over_time_keep_arriving() {
    let mut log = Log::default();
    // A per-endpoint map capped at 64 entries silently dropped every later device.
    for sequence in 1..=1_000_u64 {
        let endpoint = u8::try_from(sequence % 100).unwrap();
        log.record(settled(endpoint, sequence, 96_000, sequence != 1_000));
    }
    assert_eq!(
        log.find([0; 16], 999, Wanted::Rate(96_000)),
        Some(settled(0, 1_000, 96_000, false))
    );
    assert_eq!(log.0.len(), RETAINED);
}

#[test]
fn settlements_resent_after_a_reconnect_are_kept_once() {
    let mut log = Log::default();
    for sequence in [5, 6, 5, 6, 7] {
        log.record(settled(1, sequence, 48_000, true));
    }
    assert_eq!(
        log.0.iter().map(|entry| entry.sequence).collect::<Vec<_>>(),
        [5, 6, 7]
    );
}
