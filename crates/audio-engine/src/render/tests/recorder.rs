use super::*;
use mixrelay_audio_recorder::{Encoding, RecorderAction, RecorderRequest, RecorderState};

fn record_key() -> String {
    recording_session_key(&NodeId::from("record"), 2)
}

fn isolated_recorder(registry: &RecorderRegistry, channels: u16) -> BlockProcessor {
    let execution = compile(
        vec![node("record", NodeConfig::Recorder { channels })],
        vec![],
    );
    let mut processor = BlockProcessor::new(Arc::new(RenderPlan::prepare(&execution).unwrap()));
    processor.attach_recorders(registry).unwrap();
    processor
}

#[test]
fn channel_change_can_be_cancelled_then_committed_without_losing_previous_transport() {
    let directory = mixrelay_test_support::FixtureDir::new("recording-channels").unwrap();
    let registry = RecorderRegistry::default();
    let mut previous = isolated_recorder(&registry, 2);
    let discarded = isolated_recorder(&registry, 8);
    let new_key = recording_session_key(&NodeId::from("record"), 8);
    drop(discarded);
    registry.reap_unused().unwrap();
    previous.process_block(2, &[], &mut []).unwrap();
    let old_path = directory.join("previous.wav");
    registry
        .command(
            &record_key(),
            request(
                RecorderAction::Record,
                Some(old_path.to_string_lossy().into_owned()),
            ),
        )
        .unwrap();
    previous.process_block(2, &[], &mut []).unwrap();
    registry
        .command(&record_key(), request(RecorderAction::Stop, None))
        .unwrap();
    let old_file = hound::WavReader::open(old_path).unwrap();
    assert_eq!(old_file.spec().channels, 2);
    assert_eq!(old_file.duration(), 2);
    assert!(
        registry
            .command(&new_key, request(RecorderAction::Stop, None))
            .is_err()
    );

    let mut accepted = isolated_recorder(&registry, 8);
    drop(previous);
    registry.reap_unused().unwrap();
    accepted.process_block(2, &[], &mut []).unwrap();
    let new_path = directory.join("eight.wav");
    registry
        .command(
            &new_key,
            request(
                RecorderAction::Record,
                Some(new_path.to_string_lossy().into_owned()),
            ),
        )
        .unwrap();
    accepted.process_block(2, &[], &mut []).unwrap();
    registry
        .command(&new_key, request(RecorderAction::Stop, None))
        .unwrap();
    let new_file = hound::WavReader::open(new_path).unwrap();
    assert_eq!(new_file.spec().channels, 8);
    assert_eq!(new_file.duration(), 2);
}

fn request(action: RecorderAction, path: Option<String>) -> RecorderRequest {
    RecorderRequest {
        action,
        path,
        sample_rate: None,
        encoding: None,
    }
}

#[test]
fn muted_recorder_writes_silence_without_stopping_recording() {
    let directory = mixrelay_test_support::FixtureDir::new("muted-recording").unwrap();
    let path = directory.join("muted.wav");
    let registry = RecorderRegistry::default();
    let mut recorder = node("record", NodeConfig::Recorder { channels: 2 });
    recorder.control.muted = true;
    let execution = compile(
        vec![device_input("in"), recorder],
        vec![edge("record", "in", AUDIO_PORT_ID, "record", AUDIO_PORT_ID)],
    );
    let mut processor = BlockProcessor::new(Arc::new(RenderPlan::prepare(&execution).unwrap()));
    processor.attach_recorders(&registry).unwrap();
    processor.process_block(2, &[&[1.0; 4]], &mut []).unwrap();
    registry
        .command(
            &record_key(),
            RecorderRequest {
                encoding: Some(Encoding::Float32),
                ..request(
                    RecorderAction::Record,
                    Some(path.to_string_lossy().into_owned()),
                )
            },
        )
        .unwrap();
    processor.process_block(2, &[&[1.0; 4]], &mut []).unwrap();
    registry
        .command(&record_key(), request(RecorderAction::Stop, None))
        .unwrap();
    let mut file = hound::WavReader::open(path).unwrap();
    assert_eq!(file.duration(), 2);
    assert!(
        file.samples::<f32>()
            .all(|sample| sample.unwrap().to_bits() == 0.0_f32.to_bits())
    );
}

fn recorder_plan() -> Arc<RenderPlan> {
    let execution = compile(
        vec![
            device_input("mic"),
            device_output("monitor"),
            node("record", NodeConfig::Recorder { channels: 2 }),
        ],
        vec![
            edge("listen", "mic", AUDIO_PORT_ID, "monitor", AUDIO_PORT_ID),
            edge("save", "mic", AUDIO_PORT_ID, "record", AUDIO_PORT_ID),
        ],
    );
    Arc::new(RenderPlan::prepare(&execution).unwrap())
}

#[test]
fn mixed_sources_record_without_any_hardware_output() {
    let directory = mixrelay_test_support::FixtureDir::new("record-only-mix").unwrap();
    let path = directory.join("mix.wav");
    let execution = compile(
        vec![
            device_input("a"),
            device_input("b"),
            node("record", NodeConfig::Recorder { channels: 2 }),
        ],
        vec![
            edge("a-record", "a", AUDIO_PORT_ID, "record", AUDIO_PORT_ID),
            edge("b-record", "b", AUDIO_PORT_ID, "record", AUDIO_PORT_ID),
        ],
    );
    let plan = Arc::new(RenderPlan::prepare(&execution).unwrap());
    assert!(plan.sink_bindings().is_empty());
    let registry = RecorderRegistry::default();
    let mut processor = BlockProcessor::new(plan);
    processor.attach_recorders(&registry).unwrap();
    let left = [0.125, -0.25, 0.25, 0.5];
    let right = [0.125, 0.5, -0.5, 0.25];
    processor
        .process_block(2, &[&left, &right], &mut [])
        .unwrap();
    registry
        .command(
            &record_key(),
            RecorderRequest {
                encoding: Some(Encoding::Float32),
                ..request(
                    RecorderAction::Record,
                    Some(path.to_string_lossy().into_owned()),
                )
            },
        )
        .unwrap();
    processor
        .process_block(2, &[&left, &right], &mut [])
        .unwrap();
    registry
        .command(&record_key(), request(RecorderAction::Stop, None))
        .unwrap();
    let mut file = hound::WavReader::open(path).unwrap();
    let expected: Vec<f32> = left.iter().zip(right).map(|(a, b)| a + b).collect();
    assert_eq!(
        file.samples::<f32>()
            .map(Result::unwrap)
            .collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn recorder_captures_final_routing_without_native_sink_or_callback_allocation() {
    let directory = mixrelay_test_support::FixtureDir::new("render-recording").unwrap();
    let path = directory.join("take.wav");
    let registry = RecorderRegistry::default();
    let plan = recorder_plan();
    // Only the real monitor is a native output. A file has no hardware clock.
    assert_eq!(plan.sink_bindings().len(), 1);
    let mut processor = BlockProcessor::new(plan);
    processor.attach_recorders(&registry).unwrap();
    let input = [0.25, -0.25, 0.5, -0.5];
    let mut output = [0.0; 4];
    processor
        .process_block(2, &[&input], &mut [&mut output])
        .unwrap();
    registry
        .command(
            &record_key(),
            RecorderRequest {
                encoding: Some(Encoding::Float32),
                ..request(
                    RecorderAction::Record,
                    Some(path.to_string_lossy().into_owned()),
                )
            },
        )
        .unwrap();
    HEAP_OPERATIONS.with(|count| count.set(Some(0)));
    processor
        .process_block(2, &[&input], &mut [&mut output])
        .unwrap();
    let allocations = HEAP_OPERATIONS.with(|count| count.replace(None));
    assert_eq!(allocations, Some(0));
    assert_eq!(input.map(f32::to_bits), output.map(f32::to_bits));
    let stopped = registry
        .command(&record_key(), request(RecorderAction::Stop, None))
        .unwrap();
    assert_eq!(stopped.state, RecorderState::Stopped);
    assert_eq!(stopped.frames, 2);
    let mut reader = hound::WavReader::open(path).unwrap();
    assert_eq!(reader.spec().sample_rate, 48_000);
    assert_eq!(
        reader
            .samples::<f32>()
            .map(Result::unwrap)
            .collect::<Vec<_>>(),
        input
    );
}

#[test]
fn rebuild_and_pause_preserve_take_and_final_tap_removal_finalizes_it() {
    let directory = mixrelay_test_support::FixtureDir::new("recording-rebuild").unwrap();
    let path = directory.join("take.wav");
    let registry = RecorderRegistry::default();
    let mut current = BlockProcessor::new(recorder_plan());
    current.attach_recorders(&registry).unwrap();
    let input = [0.25; 4];
    let mut output = [0.0; 4];
    current
        .process_block(2, &[&input], &mut [&mut output])
        .unwrap();
    registry
        .command(
            &record_key(),
            request(
                RecorderAction::Record,
                Some(path.to_string_lossy().into_owned()),
            ),
        )
        .unwrap();
    current
        .process_block(2, &[&input], &mut [&mut output])
        .unwrap();
    let mut replacement = BlockProcessor::new(recorder_plan());
    replacement.attach_recorders(&registry).unwrap();
    drop(current);
    registry.reap_unused().unwrap();
    assert_eq!(
        registry.status(&record_key()).state,
        RecorderState::Recording
    );
    registry
        .command(&record_key(), request(RecorderAction::Pause, None))
        .unwrap();
    replacement
        .process_block(2, &[&input], &mut [&mut output])
        .unwrap();
    registry
        .command(&record_key(), request(RecorderAction::Record, None))
        .unwrap();
    replacement
        .process_block(2, &[&input], &mut [&mut output])
        .unwrap();
    drop(replacement);
    registry.reap_unused().unwrap();
    let reader = hound::WavReader::open(path).unwrap();
    assert_eq!(reader.duration(), 4);
}

#[test]
fn control_publications_preserve_recording_and_off_skips_capture_instead_of_writing_silence() {
    let directory = mixrelay_test_support::FixtureDir::new("live-recording-controls").unwrap();
    let path = directory.join("take.wav");
    let registry = RecorderRegistry::default();
    let plan = recorder_plan();
    let mut processor = BlockProcessor::new(Arc::clone(&plan));
    processor.attach_recorders(&registry).unwrap();
    let input = [0.25; 4];
    let mut output = [0.0; 4];
    processor
        .process_block(2, &[&input], &mut [&mut output])
        .unwrap();
    registry
        .command(
            &record_key(),
            RecorderRequest {
                encoding: Some(Encoding::Float32),
                ..request(
                    RecorderAction::Record,
                    Some(path.to_string_lossy().into_owned()),
                )
            },
        )
        .unwrap();
    let exchange = crate::ProcessorExchange::new(1);
    for revision in 2..102 {
        exchange
            .publish_controls(
                plan.controls(),
                revision,
                &[crate::RuntimeControlUpdate {
                    node_id: NodeId::from("monitor"),
                    control: mixrelay_audio_graph::NodeControl {
                        enabled: revision % 2 == 0,
                        muted: false,
                    },
                    gain_db: None,
                }],
            )
            .unwrap();
        HEAP_OPERATIONS.with(|count| count.set(Some(0)));
        processor
            .process_block(2, &[&input], &mut [&mut output])
            .unwrap();
        assert_eq!(HEAP_OPERATIONS.with(|count| count.replace(None)), Some(0));
    }
    for (revision, enabled, muted) in [(102, false, false), (103, true, true)] {
        exchange
            .publish_controls(
                plan.controls(),
                revision,
                &[crate::RuntimeControlUpdate {
                    node_id: NodeId::from("record"),
                    control: mixrelay_audio_graph::NodeControl { enabled, muted },
                    gain_db: None,
                }],
            )
            .unwrap();
        processor
            .process_block(2, &[&input], &mut [&mut output])
            .unwrap();
    }
    let stopped = registry
        .command(&record_key(), request(RecorderAction::Stop, None))
        .unwrap();
    assert_eq!(stopped.frames, 202);
    assert!(Arc::ptr_eq(processor.plan(), &plan));
    assert_continuous_recording_with_final_muted_block(&path);
}

fn assert_continuous_recording_with_final_muted_block(path: &std::path::Path) {
    let mut reader = hound::WavReader::open(path).unwrap();
    let samples = reader
        .samples::<f32>()
        .map(Result::unwrap)
        .collect::<Vec<_>>();
    assert_eq!(samples.len(), 404);
    assert!(
        samples[..400]
            .iter()
            .all(|sample| (*sample - 0.25).abs() < f32::EPSILON)
    );
    assert!(
        samples[400..]
            .iter()
            .all(|sample| sample.abs() < f32::EPSILON)
    );
}
