use super::*;
use mixrelay_macos_transport::{
    client_pcm::{
        ClientPcmMixer, ClientPcmReader, ClientPcmSnapshot, ClientPcmWriter, QUEUE_FRAMES,
    },
    endpoint_transport::EndpointTransportConfig,
};

fn word(frame: u64) -> f32 {
    f32::from(u16::try_from(frame % 8192).unwrap()) / 8192.0
}

fn queue() -> (ClientPcmWriter, Vec<ClientPcmReader>) {
    let mut writer = ClientPcmWriter::new(
        EndpointTransportConfig {
            endpoint_id: [1; 16],
            capacity_frames: QUEUE_FRAMES,
            render_channels: 2,
            capture_channels: 2,
        },
        42,
    )
    .unwrap();
    assert!(writer.set_clock((2, 10_000), 48_000, true, true));
    let mut snapshot = ClientPcmSnapshot::new().unwrap();
    assert!(snapshot.push(&writer));
    // SAFETY: the snapshot retains producer-created grants throughout decoding.
    let readers = unsafe { ClientPcmSnapshot::decode(snapshot.as_raw()) }.unwrap();
    (writer, readers)
}

fn clock(epoch: u64) -> ClientPcmClock {
    ClientPcmClock {
        incarnation: [3; 16],
        epoch,
        anchor: 10_000,
        rate: 48_000,
        running: true,
        first: Some(0),
        through: 0,
    }
}

fn window(epoch: u64, first: u64, frames: u32) -> MixWindow {
    MixWindow {
        clock: clock(epoch),
        first,
        frames,
        discontinuity: false,
    }
}

#[test]
fn app_pcm_written_while_the_graph_defers_is_recovered_once_it_is_ready() {
    // An app plays for 2.7 s at 48 kHz, twice the client queue, before the graph
    // acknowledges a format; then the converter becomes ready.
    let deferred = 2 * u64::from(QUEUE_FRAMES) + 1_000;
    let (writer, readers) = queue();
    let mut pump = ClientPcmMixer::default();
    // The previous pump left its cursor in the client queue while the graph was deferred.
    let mut waiting = ClientPcmMixer::default();
    let mut backlog = Backlog::new(2).unwrap();
    let mut input = [0.0; 256];
    let mut scratch = [0.0; 256];
    let mut published = 0;
    while published < deferred {
        let block: Vec<f32> = (published..published + 512)
            .flat_map(|frame| [word(frame); 2])
            .collect();
        assert!(writer.write(published, &block, 512));
        published += 512;
        while let Some(window) = pump.window(&readers, |_| Some(published.min(deferred))) {
            let frames = window.frames as usize;
            ClientPcmMixer::mix(&readers, window, &mut input[..frames * 2], &mut scratch).unwrap();
            assert_eq!(backlog.push(window, &input[..frames * 2]), 0);
            pump.commit(window);
        }
    }
    let late = waiting.window(&readers, |_| Some(deferred)).unwrap();
    assert!(late.first > 0, "the client queue alone retains every frame");
    let mut recovered = Vec::new();
    let mut output = [0.0; 256];
    while let Some(pending) = backlog.pending(128) {
        assert_eq!(pending.discontinuity, recovered.is_empty());
        assert_eq!(pending.first, recovered.len() as u64 / 2);
        backlog.copy(pending, &mut output);
        recovered.extend_from_slice(&output[..pending.frames * 2]);
        backlog.consume(pending.frames);
    }
    assert_eq!(recovered.len() as u64, deferred * 2);
    for (frame, samples) in recovered.chunks_exact(2).enumerate() {
        assert_eq!(samples, [word(frame as u64); 2], "frame {frame}");
    }
    assert!(backlog.is_empty());
}

#[test]
fn a_backlog_beyond_retention_drops_and_reports_only_the_oldest_frames() {
    let mut backlog = Backlog::new(1).unwrap();
    let input: Vec<f32> = (0..128).map(|frame| word(frame) + 1.0).collect();
    let mut first = 0;
    while first < Backlog::FRAMES as u64 {
        assert_eq!(backlog.push(window(2, first, 128), &input), 0);
        first += 128;
    }
    assert_eq!(backlog.push(window(2, first, 128), &input), 128);
    let pending = backlog.pending(128).unwrap();
    assert_eq!((pending.first, pending.discontinuity), (128, true));
    backlog.consume(128);
    assert!(!backlog.pending(128).unwrap().discontinuity);
}

#[test]
fn a_new_timeline_or_gap_replaces_pending_frames() {
    let mut backlog = Backlog::new(1).unwrap();
    let input = [0.5; 128];
    assert_eq!(backlog.push(window(2, 0, 128), &input), 0);
    backlog.consume(128);
    assert_eq!(backlog.push(window(2, 128, 128), &input), 0);
    assert!(!backlog.pending(128).unwrap().discontinuity);
    // A rate rearm opens epoch 3: the converter must restart there.
    assert_eq!(backlog.push(window(3, 0, 64), &input[..64]), 128);
    let pending = backlog.pending(128).unwrap();
    assert_eq!(
        (
            pending.clock.epoch,
            pending.first,
            pending.frames,
            pending.discontinuity
        ),
        (3, 0, 64, true)
    );
    assert_eq!(backlog.push(window(3, 100, 28), &input[..28]), 64);
    assert_eq!(backlog.pending(128).unwrap().first, 100);
}
