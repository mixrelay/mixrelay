use super::*;
use crate::endpoint_transport::EndpointTransportConfig;

fn writer(client: u32) -> ClientPcmWriter {
    let mut writer = ClientPcmWriter::new(
        EndpointTransportConfig {
            endpoint_id: [1; 16],
            capacity_frames: 16_384,
            render_channels: 2,
            capture_channels: 2,
        },
        client,
    )
    .unwrap();
    assert!(writer.set_clock((2, 10_000), 48_000, true, true));
    writer
}

fn reader(writer: &ClientPcmWriter) -> ClientPcmReader {
    let mut snapshot = ClientPcmSnapshot::new().unwrap();
    assert!(snapshot.push(writer));
    // SAFETY: snapshot retains the complete, producer-created VM grant for this decode.
    unsafe { ClientPcmSnapshot::decode(snapshot.as_raw()) }
        .unwrap()
        .remove(0)
}

#[test]
fn retained_remote_mapping_reads_silence_and_signal_at_absolute_positions() {
    let writer = writer(42);
    let reader = reader(&writer);
    assert!(writer.write(200_000, &[0.375; 256], 128));
    assert!(writer.write(200_128, &[0.0; 256], 128));
    assert_eq!(reader.clock().unwrap().first, Some(200_000));
    let measured = allocation_counter::measure(|| {
        let clock = reader.clock().unwrap();
        let mut output = [7.0; 512];
        let mut coverage = [false; 256];
        assert!(reader.read(clock, 200_000, &mut output, &mut coverage));
        assert_eq!(output[..256], [0.375; 256]);
        assert_eq!(output[256..], [0.0; 256]);
        assert!(coverage.into_iter().all(|valid| valid));
    });
    assert_eq!(measured.count_total, 0);
}

#[test]
fn source_coordinates_survive_a_consumers_small_read_gap_without_replaying_the_old_period() {
    let writer = writer(42);
    let reader = reader(&writer);
    // The old CoreAudio cache failure reused PCM exactly 16,384 frames earlier.
    let first = 400_000;
    assert!(writer.write(first - 16_384, &[0.75; 1024], 512));
    assert!(writer.write(first, &[0.0; 1024], 512));
    let clock = reader.clock().unwrap();
    let mut short = [1.0; 256];
    let mut short_coverage = [false; 128];
    assert!(reader.read(clock, first, &mut short, &mut short_coverage));
    // A separate reader asks for the entire interval after a partial consumer skipped it.
    let mut output = [1.0; 1024];
    let mut coverage = [false; 512];
    assert!(reader.read(clock, first, &mut output, &mut coverage));
    assert_eq!(output.map(f32::to_bits), [0.0_f32.to_bits(); 1024]);
    assert!(coverage.into_iter().all(|valid| valid));
}

#[test]
fn producer_removal_invalidates_a_retained_reader_before_its_vm_is_reclaimed() {
    let writer = writer(7);
    let reader = reader(&writer);
    assert!(writer.write(100, &[0.5; 16], 8));
    let clock = reader.clock().unwrap();
    drop(writer);
    assert!(reader.clock().is_none());
    let mut output = [9.0; 16];
    let mut coverage = [true; 8];
    assert!(!reader.read(clock, 100, &mut output, &mut coverage));
    assert_eq!(output.map(f32::to_bits), [0.0_f32.to_bits(); 16]);
    assert_eq!(coverage, [false; 8]);
}

#[test]
fn restart_rejects_the_previous_clock_and_never_compacts_unpublished_frames() {
    let mut writer = writer(7);
    let reader = reader(&writer);
    assert!(writer.write(100, &[0.5; 16], 8));
    let old = reader.clock().unwrap();
    assert!(writer.set_clock((3, 20_000), 96_000, true, true));
    assert!(writer.write(100, &[0.25; 8], 4));
    assert!(writer.write(106, &[0.375; 4], 2));
    let mut output = [9.0; 16];
    let mut coverage = [true; 8];
    assert!(!reader.read(old, 100, &mut output, &mut coverage));
    assert_eq!(output.map(f32::to_bits), [0.0_f32.to_bits(); 16]);
    assert!(reader.read(reader.clock().unwrap(), 100, &mut output, &mut coverage));
    assert_eq!(
        output.map(f32::to_bits),
        [
            0.25_f32, 0.25, 0.25, 0.25, 0.25, 0.25, 0.25, 0.25, 0.0, 0.0, 0.0, 0.0, 0.375, 0.375,
            0.375, 0.375
        ]
        .map(f32::to_bits)
    );
    assert_eq!(coverage, [true, true, true, true, false, false, true, true]);
}

#[test]
fn malformed_or_duplicate_snapshots_do_not_publish_a_partial_authoritative_list() {
    let writer = writer(7);
    let mut snapshot = ClientPcmSnapshot::new().unwrap();
    assert!(snapshot.push(&writer));
    assert!(snapshot.push(&writer));
    // SAFETY: both pointers are either null or owned live XPC dictionaries.
    unsafe {
        assert!(matches!(
            ClientPcmSnapshot::decode(std::ptr::null_mut()),
            Err(SnapshotError::Invalid)
        ));
        assert!(matches!(
            ClientPcmSnapshot::decode(snapshot.as_raw()),
            Err(SnapshotError::Invalid)
        ));
    }
}

#[test]
fn endpoint_clock_transition_excludes_every_client_until_the_complete_set_is_ready() {
    use std::sync::Arc;
    let owner = Arc::new(ClientPcmClockOwner::new().unwrap());
    let config = EndpointTransportConfig {
        endpoint_id: [1; 16],
        capacity_frames: 16_384,
        render_channels: 2,
        capture_channels: 2,
    };
    let mut first = ClientPcmWriter::with_clock(config, 7, owner.clone()).unwrap();
    let mut second = ClientPcmWriter::with_clock(config, 8, owner.clone()).unwrap();
    let old = ClientPcmTimeline {
        epoch: 2,
        anchor: 1_000,
        rate: 48_000,
        running: true,
    };
    let publication = owner.begin(old).unwrap();
    assert!(publication.configure(&mut first, true));
    assert!(publication.configure(&mut second, true));
    publication.commit();
    let readers = [reader(&first), reader(&second)];
    assert!(first.write(100, &[0.5; 16], 8));
    assert!(second.write(100, &[0.25; 16], 8));
    assert!(readers.iter().all(|reader| reader.clock().is_some()));
    let next = ClientPcmTimeline {
        epoch: 3,
        anchor: 2_000,
        rate: 96_000,
        running: true,
    };
    let publication = owner.begin(next).unwrap();
    assert!(publication.configure(&mut first, true));
    // The second queue still contains old PCM. No consumer may use it while
    // the endpoint is between generations, even though its own header is stable.
    assert!(readers.iter().all(|reader| reader.clock().is_none()));
    assert!(publication.configure(&mut second, true));
    publication.commit();
    assert!(readers.iter().all(|reader| {
        reader
            .clock()
            .is_some_and(|clock| clock.epoch == 3 && clock.first.is_none())
    }));
    owner.retire();
    assert!(readers.iter().all(|reader| reader.clock().is_none()));
}

#[test]
fn abandoned_clock_transition_stays_silent_and_replacement_cannot_reuse_its_identity() {
    use std::sync::Arc;
    let owner = Arc::new(ClientPcmClockOwner::new().unwrap());
    let config = EndpointTransportConfig {
        endpoint_id: [1; 16],
        capacity_frames: 16_384,
        render_channels: 2,
        capture_channels: 2,
    };
    let mut first = ClientPcmWriter::with_clock(config, 7, owner.clone()).unwrap();
    let timeline = ClientPcmTimeline {
        epoch: 2,
        anchor: 1_000,
        rate: 48_000,
        running: true,
    };
    let publication = owner.begin(timeline).unwrap();
    assert!(publication.configure(&mut first, true));
    publication.commit();
    let original = reader(&first);
    let old_identity = original.clock().unwrap().incarnation;
    let incomplete = owner
        .begin(ClientPcmTimeline {
            epoch: 3,
            ..timeline
        })
        .unwrap();
    assert!(incomplete.configure(&mut first, true));
    drop(incomplete);
    assert!(original.clock().is_none());
    let mut replacement = writer(7);
    assert!(replacement.set_clock((2, 1_000), 48_000, true, true));
    assert_ne!(
        reader(&replacement).clock().unwrap().incarnation,
        old_identity
    );
}

#[test]
fn membership_handoff_keeps_the_window_until_all_client_grants_arrive() {
    use std::sync::Arc;
    let owner = Arc::new(ClientPcmClockOwner::new().unwrap());
    let config = EndpointTransportConfig {
        endpoint_id: [1; 16],
        capacity_frames: 16_384,
        render_channels: 2,
        capture_channels: 2,
    };
    let mut first = ClientPcmWriter::with_clock(config, 7, owner.clone()).unwrap();
    let timeline = ClientPcmTimeline {
        epoch: 2,
        anchor: 1_000,
        rate: 48_000,
        running: true,
    };
    let publication = owner.begin(timeline).unwrap();
    assert!(publication.configure(&mut first, true));
    publication.commit();
    assert!(first.write(100, &[0.125; 256], 128));
    let old = vec![reader(&first)];
    let mut mixer = ClientPcmMixer::default();
    let window = mixer.window(&old, |_| Some(48_000)).unwrap();
    owner.publish_membership(2);
    let mut joined = ClientPcmWriter::with_clock(config, 8, owner.clone()).unwrap();
    let publication = owner.begin(timeline).unwrap();
    assert!(publication.configure(&mut joined, true));
    publication.commit();
    assert!(joined.write(100, &[0.25; 256], 128));
    let mut output = [9.0; 256];
    let mut scratch = [0.0; 256];
    assert_eq!(
        ClientPcmMixer::mix(&old, window, &mut output, &mut scratch),
        Err(MixError::Changed)
    );
    assert_eq!(output.map(f32::to_bits), [0.0_f32.to_bits(); 256]);
    assert!(mixer.window(&old, |_| Some(48_000)).is_none());
    let current = vec![reader(&first), reader(&joined)];
    let current_window = mixer.window(&current, |_| Some(48_000)).unwrap();
    assert_eq!(current_window.first, window.first);
    assert!(current_window.discontinuity);
    ClientPcmMixer::mix(&current, current_window, &mut output, &mut scratch).unwrap();
    assert_eq!(output.map(f32::to_bits), [0.375_f32.to_bits(); 256]);
    mixer.commit(current_window);
    assert!(mixer.window(&current, |_| Some(48_000)).is_none());
}
