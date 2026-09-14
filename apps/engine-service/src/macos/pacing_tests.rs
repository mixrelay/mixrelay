use super::*;
use mixrelay_audio_graph::GraphDocument;
use native_transport::{EndpointSnapshot, SnapshotError};

fn processor() -> BlockProcessor {
    *create_initial_processor(
        &GraphDocument::new(1, Vec::new(), Vec::new()),
        RenderFormat {
            sample_rate: 48_000,
            block_frames: 32,
        },
        default_peak_meter_bank(),
    )
    .unwrap()
}

fn endpoint(through: u64) -> EndpointSnapshot {
    let mut endpoint = EndpointSnapshot {
        incarnation: [1; 16],
        ..EndpointSnapshot::default()
    };
    endpoint.clock.epoch = 1;
    endpoint.clock.running = 1;
    endpoint.clock.sample_rate = 48_000;
    endpoint.clock.render_through = through;
    endpoint.clock.render_frames = 256;
    endpoint
}

pub(crate) fn assert_hardware_keeps_authority(devices: &crate::macos_devices::DeviceSet) {
    let processor = processor();
    let mut pacers = NativePacers::default();
    for through in [0, 256, 512, 0, 256, 1024] {
        let mut active = endpoint(through);
        active.clock.running = u32::from(through != 0);
        for observation in [
            Ok(&active),
            Err(SnapshotError::Pending),
            Err(SnapshotError::Missing),
        ] {
            let (pace, clock) =
                next_native_pace(&mut pacers, devices, processor.plan(), 16_384, observation);
            assert_eq!(
                (pace, clock),
                (Pace::Wait, 2),
                "HAL activity cannot invent hardware credit or take its clock"
            );
        }
    }
}

#[test]
fn contended_observations_preserve_every_partial_callback_pcm_coordinate() {
    let processor = processor();
    let devices = crate::macos_devices::DeviceSet::default();
    let mut pacers = NativePacers::default();
    let endpoint = endpoint(256);
    let pcm: Vec<_> = (0..256_u16).map(|frame| f32::from(frame) / 256.0).collect();
    let mut delivered = Vec::new();
    let mut discontinuities = 0;
    for _ in 0..8 {
        let (pace, clock) = next_native_pace(
            &mut pacers,
            &devices,
            processor.plan(),
            16_384,
            Ok(&endpoint),
        );
        assert_eq!(clock, 1);
        let Pace::Block {
            first_frame,
            frames,
            discontinuity,
            overrun,
            ..
        } = pace
        else {
            panic!("real callback credit was lost: {pace:?}");
        };
        assert!(!overrun);
        discontinuities += usize::from(discontinuity);
        delivered.extend_from_slice(
            &pcm[usize::try_from(first_frame).unwrap()
                ..usize::try_from(first_frame + u64::from(frames)).unwrap()],
        );
        for _ in 0..4 {
            assert_eq!(
                next_native_pace(
                    &mut pacers,
                    &devices,
                    processor.plan(),
                    16_384,
                    Err(SnapshotError::Pending),
                )
                .0,
                Pace::Wait
            );
        }
    }
    assert_eq!(
        delivered, pcm,
        "no duplicate, omitted or substituted PCM coordinates"
    );
    assert_eq!(
        discontinuities, 1,
        "only initial attachment changes the clock epoch"
    );
    assert_eq!(
        next_native_pace(
            &mut pacers,
            &devices,
            processor.plan(),
            16_384,
            Ok(&endpoint),
        )
        .0,
        Pace::Wait
    );
    eprintln!(
        "32 contended observations: 0 clock handoffs; 256/256 exact PCM coordinates; 0 replayed frames"
    );
}

#[test]
fn priming_waits_but_actual_stop_and_removal_release_the_clock() {
    let processor = processor();
    let devices = crate::macos_devices::DeviceSet::default();
    let mut pacers = NativePacers::default();
    let mut endpoint = endpoint(0);
    assert!(!native_transport::needs_capture_recovery(Err(
        &SnapshotError::Pending
    )));
    assert!(native_transport::needs_capture_recovery(Err(
        &SnapshotError::Missing
    )));
    assert_eq!(
        next_native_pace(
            &mut pacers,
            &devices,
            processor.plan(),
            16_384,
            Ok(&endpoint),
        )
        .0,
        Pace::Wait
    );
    endpoint.clock.running = 0;
    assert_eq!(
        next_native_pace(
            &mut pacers,
            &devices,
            processor.plan(),
            16_384,
            Ok(&endpoint),
        )
        .0,
        Pace::Idle
    );
    assert_eq!(
        next_native_pace(
            &mut pacers,
            &devices,
            processor.plan(),
            16_384,
            Err(SnapshotError::Missing),
        )
        .0,
        Pace::Unknown
    );
}
