use super::{CLOSED, Flight, PACKETS, Packet, Producer, RecorderTap, Transport};
use std::sync::{Arc, atomic::Ordering};
use std::time::{Duration, Instant};

mod retention;

#[test]
fn cutoff_waits_for_admitted_packet_and_observes_its_late_failure() {
    for late_failure in [false, true] {
        let shared = Transport::new(1).unwrap();
        shared.gate.store(1, Ordering::SeqCst);
        shared.producer.store(true, Ordering::SeqCst);
        shared.in_flight.store(1, Ordering::SeqCst);
        let flight = Flight(&shared.in_flight);
        let producer = Producer(&shared.producer);
        let other = Arc::clone(&shared);
        let control = std::thread::spawn(move || other.cutoff());
        let deadline = Instant::now() + Duration::from_secs(2);
        while shared.gate.load(Ordering::SeqCst) & CLOSED == 0 {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(!control.is_finished());
        assert!(shared.packets.publish(&[0.25], 48_000));
        if late_failure {
            shared.fail(1, 1);
        }
        drop(producer);
        drop(flight);
        assert_eq!(control.join().unwrap().is_some(), late_failure);
        let mut packet = Packet::new(1).unwrap();
        assert!(shared.read_packet(&mut packet));
        assert_eq!(
            (packet.frames, packet.samples[0].to_bits()),
            (1, 0.25_f32.to_bits())
        );
    }
}

#[test]
fn a_full_packet_pool_fails_without_allocating_freeing_or_retagging() {
    let shared = Transport::new(1).unwrap();
    let tap = RecorderTap::new(Arc::clone(&shared));
    shared.gate.store(1, Ordering::SeqCst);
    for _ in 0..PACKETS {
        tap.capture(&[0.5; 256], 48_000);
    }
    let allocations = allocation_counter::measure(|| tap.capture(&[0.5; 256], 48_000));
    assert_eq!((allocations.count_total, allocations.count_current), (0, 0));
    assert_eq!(shared.cutoff(), Some("recorder packet queue overrun"));
    shared.gate.store(2, Ordering::SeqCst);
    shared.fail(1, 2);
    assert_eq!(shared.gate.load(Ordering::SeqCst), 2);
}

#[test]
fn two_hours_of_input_frames_exhaust_exactly_at_every_supported_clock_boundary() {
    use super::{DURATION_TICKS_PER_SECOND, admit_duration};
    for &first_rate in mixrelay_audio_io::formats::SAMPLE_RATES {
        for &second_rate in mixrelay_audio_io::formats::SAMPLE_RATES {
            let ticks = 7_200 * DURATION_TICKS_PER_SECOND;
            let first_hour = usize::try_from(first_rate).unwrap() * 3_600;
            let (accepted, left) = admit_duration(ticks, first_hour, first_rate);
            assert_eq!(accepted, first_hour);
            let last_hour = usize::try_from(second_rate).unwrap() * 3_600;
            let (accepted, left) = admit_duration(left, last_hour - 1, second_rate);
            assert_eq!(accepted, last_hour - 1);
            assert!(left > 0);
            let (accepted, left) = admit_duration(left, 1, second_rate);
            assert_eq!((accepted, left), (1, 0));
            assert_eq!(admit_duration(left, 1, second_rate), (0, 0));
            // Crossing the boundary inside a callback retains only the valid prefix.
            let (_, left) = admit_duration(ticks, first_hour, first_rate);
            assert_eq!(
                admit_duration(left, last_hour + 1, second_rate),
                (last_hour, 0)
            );
        }
    }
}
