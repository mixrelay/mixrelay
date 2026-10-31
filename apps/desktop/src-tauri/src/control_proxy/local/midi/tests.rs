use super::*;
use mixrelay_midi_control::{Control, Device, Kind, Mode};

fn binding(action: Action) -> Binding {
    Binding {
        control: Control {
            device: Device {
                id: "id".into(),
                name: "controller".into(),
            },
            channel: 0,
            kind: Kind::Cc,
            number: 7,
        },
        target: MidiTarget {
            graph_id: "graph".into(),
            node_id: "gain".into(),
        },
        action,
        mode: Mode::Absolute,
        step_db: 1.0,
        graph_name: "Programme".into(),
        node_name: "Microphone".into(),
        min_db: -60.0,
        max_db: 12.0,
    }
}

fn snapshot() -> Value {
    json!({"session":"same-local-controller", "revision":7,"activeRevision":7,
    "enginePower":{"enabled":true,"transitioning":false},
    "graphs":[{"id":"graph","nodes":[{"nodeId":"gain","control":{"enabled":true,"muted":false},
    "capabilities":{"power":true,"mute":true,"recorder":true},"gainDb":0,
    "recorder":{"state":"paused","path":"take.wav"}}]}]})
}

#[test]
fn midi_prepares_the_same_typed_revision_fenced_node_commands_as_stream_deck() {
    for (action, expected) in [
        (Action::Power, json!({"expectedRevision":7,"enabled":false})),
        (Action::Mute, json!({"expectedRevision":7,"muted":true})),
        (Action::Volume, json!({"expectedRevision":7,"gainDb":-3.0})),
    ] {
        let Command::Node {
            session,
            target,
            request,
        } = prepare(&snapshot(), &binding(action), Some(-3.0)).unwrap()
        else {
            panic!("node command");
        };
        assert_eq!(session, "same-local-controller");
        assert_eq!(
            (target.graph_id.as_str(), target.node_id.as_str()),
            ("graph", "gain")
        );
        assert_eq!(serde_json::to_value(request).unwrap(), expected);
    }
}

#[test]
fn recorder_resumes_the_observed_take_and_rejects_inapplicable_transport() {
    for (state, action, allowed) in [
        ("paused", Action::Record, true),
        ("recording", Action::Record, false),
        ("recording", Action::Pause, true),
        ("paused", Action::Pause, false),
        ("recording", Action::Stop, true),
        ("paused", Action::Stop, true),
        ("stopped", Action::Stop, false),
    ] {
        let mut snapshot = snapshot();
        snapshot["graphs"][0]["nodes"][0]["recorder"]["state"] = json!(state);
        let command = prepare(&snapshot, &binding(action), None);
        assert_eq!(command.is_some(), allowed);
        if let Some(Command::Recorder { request, .. }) = command {
            assert_eq!(
                serde_json::to_value(request).unwrap()["expectedRecording"],
                json!({"state":state,"path":"take.wav"})
            );
        }
    }
}

#[test]
fn missing_capabilities_pending_revisions_off_nodes_and_invalid_gains_do_not_dispatch() {
    for change in 0..7 {
        let mut snapshot = snapshot();
        match change {
            0 => snapshot["graphs"] = json!([]),
            1 => snapshot["activeRevision"] = json!(6),
            2 => snapshot["enginePower"]["enabled"] = json!(false),
            3 => snapshot["enginePower"]["transitioning"] = json!(true),
            4 => snapshot["graphs"][0]["nodes"][0]["control"]["enabled"] = json!(false),
            5 => snapshot["graphs"][0]["nodes"][0]["capabilities"]["mute"] = json!(false),
            _ => snapshot["session"] = Value::Null,
        }
        assert!(prepare(&snapshot, &binding(Action::Mute), None).is_none());
    }
    for gain in [None, Some(f32::NAN), Some(-60.1), Some(12.1)] {
        assert!(prepare(&snapshot(), &binding(Action::Volume), gain).is_none());
    }
    let mut snapshot = snapshot();
    snapshot["enginePower"]["enabled"] = json!(false);
    assert!(prepare(&snapshot, &binding(Action::Power), None).is_some());
}

#[tokio::test]
async fn midi_and_stream_deck_reach_the_same_in_process_node_control_boundary() {
    use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};
    let fixture = mixrelay_test_support::FixtureDir::ipc("midi").unwrap();
    let path = fixture.join("engine.sock");
    let listener = tokio::net::UnixListener::bind(&path).unwrap();
    let mut snapshot = snapshot();
    snapshot.as_object_mut().unwrap().remove("session");
    snapshot["structureGeneration"] = json!(1);
    snapshot["graphs"][0]["name"] = json!("Programme");
    snapshot["graphs"][0]["nodes"][0]["kind"] = json!("gain");
    snapshot["graphs"][0]["nodes"][0]["label"] = json!(null);
    snapshot["graphs"][0]["nodes"][0]
        .as_object_mut()
        .unwrap()
        .remove("recorder");
    let server = tokio::spawn(async move {
        let mut requests = Vec::new();
        for _ in 0..3 {
            let (stream, _) = listener.accept().await.unwrap();
            let mut reader = BufReader::new(stream);
            let mut route = String::new();
            reader.read_line(&mut route).await.unwrap();
            let mut length = 0;
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).await.unwrap();
                if header == "\r\n" {
                    break;
                }
                if let Some((name, value)) = header.split_once(':')
                    && name.eq_ignore_ascii_case("content-length")
                {
                    length = value.trim().parse::<usize>().unwrap();
                }
            }
            let mut bytes = vec![0; length];
            reader.read_exact(&mut bytes).await.unwrap();
            let body = if bytes.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(&bytes).unwrap()
            };
            let response = if route.starts_with("GET ") {
                snapshot.clone()
            } else {
                json!({"revision":8})
            };
            requests.push((route, body));
            let response = response.to_string();
            reader.get_mut().write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",response.len()).as_bytes()).await.unwrap();
        }
        requests
    });
    let control = LocalControl::new(
        crate::control_proxy::ControlProxyClient::with_socket(Some(path)),
        #[cfg(target_os = "macos")]
        std::sync::Arc::default(),
    )
    .unwrap();
    let (_directory, midi) = crate::midi::tests::fixture();
    control
        .midi_action(&binding(Action::Mute), None, &midi.admission())
        .await;
    let request = serde_json::from_value(json!({"id":1,"command":{"type":"node","session":control.session,"target":{"graphId":"graph","nodeId":"gain"},"request":{"expectedRevision":7,"muted":true}}})).unwrap();
    let reply = control.dispatch(request).await;
    assert_eq!(reply["status"], 200);
    let requests = tokio::time::timeout(std::time::Duration::from_secs(3), server)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(requests[1], requests[2]);
    assert_eq!(
        requests[1].0.trim(),
        "PUT /api/v1/graphs/graph/nodes/gain/control HTTP/1.1"
    );
    assert_eq!(requests[1].1, json!({"expectedRevision":7,"muted":true}));
}

#[test]
fn soft_takeover_cannot_arm_against_an_off_pending_or_missing_volume_target() {
    let target = binding(Action::Volume).target;
    assert!(volume_ready(&snapshot(), &target));
    for change in 0..5 {
        let mut snapshot = snapshot();
        match change {
            0 => snapshot["graphs"][0]["nodes"][0]["control"]["enabled"] = json!(false),
            1 => snapshot["enginePower"]["enabled"] = json!(false),
            2 => snapshot["activeRevision"] = json!(6),
            3 => snapshot["graphs"][0]["nodes"][0]["gainDb"] = Value::Null,
            _ => snapshot["graphs"] = json!([]),
        }
        assert!(!volume_ready(&snapshot, &target));
    }
}

#[test]
fn shared_control_toggles_each_targets_own_state_and_restored_ids_resume_without_rebinding() {
    let first = binding(Action::Mute);
    let mut second = first.clone();
    second.target.node_id = "other".into();
    let mut live = snapshot();
    let mut other = live["graphs"][0]["nodes"][0].clone();
    other["nodeId"] = json!("other");
    other["control"]["muted"] = json!(true);
    live["graphs"][0]["nodes"]
        .as_array_mut()
        .unwrap()
        .push(other);
    for (mapping, expected) in [(&first, true), (&second, false)] {
        let Some(Command::Node { request, .. }) = prepare(&live, mapping, None) else {
            panic!("node toggle");
        };
        assert_eq!(serde_json::to_value(request).unwrap()["muted"], expected);
    }
    for whole_graph in [false, true] {
        for _ in 0..2 {
            let mut deleted = live.clone();
            if whole_graph {
                deleted["graphs"] = json!([]);
            } else {
                deleted["graphs"][0]["nodes"] = json!([]);
            }
            assert!(prepare(&deleted, &first, None).is_none());
            assert!(prepare(&live, &first, None).is_some());
        }
    }
}

mod dispatch_regressions;
