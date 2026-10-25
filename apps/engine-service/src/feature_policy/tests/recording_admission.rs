use super::{free_controller, node};
use crate::EngineGraphController;
use mixrelay_audio_graph::{GraphDocument, NodeConfig};
use mixrelay_audio_recorder::{Encoding, RecorderAction, RecorderRequest, RecorderState};
use mixrelay_control_protocol::{ErrorCode, RecorderControlRequest};
use mixrelay_control_server::ControlBackend;
use mixrelay_test_support::FixtureDir;

fn engine(fixture: &FixtureDir) -> EngineGraphController {
    // Saved paid work survives downgrade; new takes still require free admission.
    free_controller(GraphDocument::new(
        1,
        vec![
            node("first", NodeConfig::Recorder { channels: 2 }),
            node("second", NodeConfig::Recorder { channels: 2 }),
            node("wide", NodeConfig::Recorder { channels: 3 }),
        ],
        vec![],
    ))
    .with_graph_catalog_path(fixture.join("graphs.json"))
}

fn command(action: RecorderAction, path: Option<String>) -> RecorderControlRequest {
    RecorderControlRequest {
        expected_revision: 1,
        expected_recording: None,
        request: RecorderRequest {
            action,
            path,
            sample_rate: None,
            encoding: Some(Encoding::Float32),
        },
    }
}

#[test]
fn recording_and_paused_takes_reserve_the_free_slot_until_stop() {
    let fixture = FixtureDir::new("rec-admit").unwrap();
    let engine = engine(&fixture);
    let first_id = engine.recorder_session_id("graph-1", "first").unwrap();
    let second_id = engine.recorder_session_id("graph-1", "second").unwrap();
    let first = engine.recorders.registry().prepare(&first_id, 2).unwrap();
    let second = engine.recorders.registry().prepare(&second_id, 2).unwrap();
    first.capture(&[], 48_000);
    second.capture(&[], 48_000);
    let first_path = fixture.join("first.wav");
    engine
        .control_recorder(
            "graph-1",
            "first",
            command(
                RecorderAction::Record,
                Some(first_path.to_str().unwrap().into()),
            ),
        )
        .unwrap();
    first.capture(&[0.25; 2], 48_000);
    for action in [None, Some(RecorderAction::Pause)] {
        if let Some(action) = action {
            engine
                .control_recorder("graph-1", "first", command(action, None))
                .unwrap();
        }
        let error = engine
            .control_recorder(
                "graph-1",
                "second",
                command(
                    RecorderAction::Record,
                    Some(fixture.join("denied.wav").to_str().unwrap().into()),
                ),
            )
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::FeatureLimit);
        assert_eq!(error.parameters["resource"], "recorders");
        assert!(!fixture.join("denied.wav").exists());
    }
    engine
        .control_recorder("graph-1", "first", command(RecorderAction::Record, None))
        .unwrap();
    first.capture(&[0.5; 2], 48_000);
    assert_eq!(
        engine
            .control_recorder("graph-1", "first", command(RecorderAction::Stop, None))
            .unwrap()
            .frames,
        2
    );
    let second_path = fixture.join("second.wav");
    assert_eq!(
        engine
            .control_recorder(
                "graph-1",
                "second",
                command(
                    RecorderAction::Record,
                    Some(second_path.to_str().unwrap().into())
                )
            )
            .unwrap()
            .state,
        RecorderState::Recording
    );
    second.capture(&[0.75; 2], 48_000);
    assert_eq!(
        engine
            .control_recorder("graph-1", "second", command(RecorderAction::Stop, None))
            .unwrap()
            .frames,
        1
    );
}

#[test]
fn a_third_recording_channel_is_rejected_before_creating_a_file() {
    let fixture = FixtureDir::new("rec-width").unwrap();
    let engine = engine(&fixture);
    let path = fixture.join("wide.wav");
    let error = engine
        .control_recorder(
            "graph-1",
            "wide",
            command(RecorderAction::Record, Some(path.to_str().unwrap().into())),
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::FeatureLimit);
    assert_eq!(error.parameters["resource"], "recordingChannels");
    assert!(!path.exists());
}
