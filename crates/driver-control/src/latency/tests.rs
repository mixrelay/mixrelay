use super::*;
use crate::test_backend::FakeBackend;

#[test]
fn leases_fence_stale_publications_release_and_expiry() {
    let (mut backend, _) = FakeBackend::ready();
    let now = Instant::now();
    let mut authority = LatencyAuthority::default();
    let claim = |name: &str| LatencyCommand::Claim {
        incarnation: name.into(),
        process_id: 10,
    };
    let first = authority
        .command(claim("first"), now, &mut backend)
        .unwrap()
        .lease
        .unwrap();
    assert!(
        !authority
            .command(claim("other"), now, &mut backend)
            .unwrap()
            .accepted
    );
    authority.expire(now + LEASE_TIMEOUT, &mut backend).unwrap();
    let second = authority
        .command(claim("second"), now + LEASE_TIMEOUT, &mut backend)
        .unwrap()
        .lease
        .unwrap();
    assert_ne!(first.generation, second.generation);
    assert!(
        !authority
            .command(
                LatencyCommand::Publish {
                    lease: first.clone(),
                    destinations: vec![]
                },
                now + LEASE_TIMEOUT,
                &mut backend
            )
            .unwrap()
            .accepted
    );
    assert!(
        !authority
            .command(
                LatencyCommand::Release { lease: first },
                now + LEASE_TIMEOUT,
                &mut backend
            )
            .unwrap()
            .accepted
    );
    assert!(
        authority
            .command(
                LatencyCommand::Publish {
                    lease: second.clone(),
                    destinations: vec![]
                },
                now + LEASE_TIMEOUT,
                &mut backend
            )
            .unwrap()
            .accepted
    );
    assert!(
        authority
            .command(
                LatencyCommand::Release { lease: second },
                now + LEASE_TIMEOUT,
                &mut backend
            )
            .unwrap()
            .accepted
    );
}

#[test]
fn runtime_codec_is_additive_and_keeps_unknown_distinct_from_zero() {
    let lease = EngineLatencyLease {
        generation: 1,
        incarnation: "run".into(),
        process_id: 2,
    };
    let entry = RuntimeLatency {
        endpoint_id: VirtualEndpointId::new("device".into()),
        rate: 48_000,
        engine_lease: lease.clone(),
        applied_graph_revision: 3,
        input_latency_frames: Some(0),
    };
    let mut json = serde_json::to_value(&entry).unwrap();
    json["futureField"] = serde_json::json!(true);
    assert_eq!(
        serde_json::from_value::<RuntimeLatency>(json).unwrap(),
        entry
    );
    let unknown = RuntimeLatency {
        input_latency_frames: None,
        ..entry
    };
    assert_eq!(
        serde_json::from_str::<RuntimeLatency>(&serde_json::to_string(&unknown).unwrap()).unwrap(),
        unknown
    );
}

#[test]
fn stale_graphs_duplicates_and_expiry_keep_native_publication_correct() {
    let entry = example_publication();
    let (mut backend, observed) = FakeBackend::ready();
    let mut authority = LatencyAuthority::default();
    let now = Instant::now();
    let lease = authority
        .command(
            LatencyCommand::Claim {
                incarnation: "run".into(),
                process_id: 2,
            },
            now,
            &mut backend,
        )
        .unwrap()
        .lease
        .unwrap();
    let entry = RuntimeLatency {
        engine_lease: lease.clone(),
        ..entry
    };
    authority
        .command(
            LatencyCommand::Publish {
                lease: lease.clone(),
                destinations: vec![entry.clone()],
            },
            now,
            &mut backend,
        )
        .unwrap();
    authority
        .command(
            LatencyCommand::Publish {
                lease: lease.clone(),
                destinations: vec![RuntimeLatency {
                    applied_graph_revision: 2,
                    input_latency_frames: None,
                    ..entry.clone()
                }],
            },
            now,
            &mut backend,
        )
        .unwrap();
    assert_eq!(authority.destinations[&entry.endpoint_id], entry);
    assert!(
        authority
            .command(
                LatencyCommand::Publish {
                    lease: lease.clone(),
                    destinations: vec![entry.clone(), entry]
                },
                now,
                &mut backend
            )
            .is_err()
    );
    authority.expire(now + LEASE_TIMEOUT, &mut backend).unwrap();
    assert!(authority.destinations.is_empty());
    assert_eq!(
        observed
            .lock()
            .latencies
            .last()
            .unwrap()
            .input_latency_frames,
        None
    );
}

#[test]
fn invalid_identity_is_rejected_before_claiming_a_lease() {
    let (mut backend, _) = FakeBackend::ready();
    let mut authority = LatencyAuthority::default();
    let now = Instant::now();
    assert!(
        authority
            .command(
                LatencyCommand::Claim {
                    incarnation: String::new(),
                    process_id: 0
                },
                now,
                &mut backend
            )
            .is_err()
    );
}

fn example_publication() -> RuntimeLatency {
    RuntimeLatency {
        endpoint_id: VirtualEndpointId::new("device".into()),
        rate: 48_000,
        engine_lease: EngineLatencyLease {
            generation: 1,
            incarnation: "run".into(),
            process_id: 2,
        },
        applied_graph_revision: 3,
        input_latency_frames: Some(0),
    }
}
