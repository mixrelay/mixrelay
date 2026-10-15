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

struct Claimed {
    authority: LatencyAuthority,
    backend: FakeBackend,
    observed: std::sync::Arc<parking_lot::Mutex<crate::test_backend::Observation>>,
    lease: EngineLatencyLease,
}
impl Claimed {
    fn new() -> Self {
        let (mut backend, observed) = FakeBackend::ready();
        let mut authority = LatencyAuthority::default();
        let lease = authority
            .command(
                LatencyCommand::Claim {
                    incarnation: "run".into(),
                    process_id: 2,
                },
                Instant::now(),
                &mut backend,
            )
            .unwrap()
            .lease
            .unwrap();
        Self {
            authority,
            backend,
            observed,
            lease,
        }
    }
    fn entry(&self, id: &str, revision: u64, frames: Option<u32>) -> RuntimeLatency {
        RuntimeLatency {
            endpoint_id: VirtualEndpointId::new(id.into()),
            engine_lease: self.lease.clone(),
            applied_graph_revision: revision,
            input_latency_frames: frames,
            ..example_publication()
        }
    }
    fn publish(&mut self, destinations: Vec<RuntimeLatency>) -> Result<LatencyReply, DriverError> {
        self.authority.command(
            LatencyCommand::Publish {
                lease: self.lease.clone(),
                destinations,
            },
            Instant::now(),
            &mut self.backend,
        )
    }
    /// The value the HAL last received for an endpoint.
    fn reported(&self, id: &str) -> Option<u32> {
        self.observed
            .lock()
            .latencies
            .iter()
            .rev()
            .find(|entry| entry.endpoint_id.as_str() == id)
            .and_then(|entry| entry.input_latency_frames)
    }
}

#[test]
fn an_endpoint_missing_from_the_next_publication_becomes_unknown() {
    let mut claimed = Claimed::new();
    let all = vec![
        claimed.entry("a", 10, Some(384)),
        claimed.entry("b", 10, Some(96)),
    ];
    claimed.publish(all).unwrap();
    assert_eq!(
        (claimed.reported("a"), claimed.reported("b")),
        (Some(384), Some(96))
    );

    // Removing the route to `a` leaves only `b` in the engine's complete set.
    let only_b = vec![claimed.entry("b", 11, Some(96))];
    claimed.publish(only_b.clone()).unwrap();
    assert_eq!(
        (claimed.reported("a"), claimed.reported("b")),
        (None, Some(96))
    );
    // Repeating that set sends nothing more for the already unknown endpoint.
    let count = claimed.observed.lock().latencies.len();
    claimed.publish(only_b).unwrap();
    assert_eq!(claimed.observed.lock().latencies.len(), count + 1);

    // The cleared endpoint keeps its revision fence: an older value cannot return.
    claimed
        .publish(vec![claimed.entry("a", 9, Some(512))])
        .unwrap();
    assert_eq!(claimed.reported("a"), None);
    claimed
        .publish(vec![claimed.entry("a", 12, Some(384))])
        .unwrap();
    assert_eq!(claimed.reported("a"), Some(384));
}

#[test]
fn a_failed_clear_is_retried_by_the_next_publication() {
    let mut claimed = Claimed::new();
    claimed
        .publish(vec![
            claimed.entry("a", 10, Some(384)),
            claimed.entry("b", 10, Some(96)),
        ])
        .unwrap();
    // `b` fails after `a` was cleared; its measured entry stays so the next publication
    // retries it, and the already cleared `a` is not sent again.
    claimed.observed.lock().failed_latencies.insert("b".into());
    assert!(claimed.publish(vec![]).is_err());
    let b = VirtualEndpointId::new("b".into());
    assert_eq!(claimed.reported("a"), None);
    assert_eq!(
        claimed.authority.destinations[&b].input_latency_frames,
        Some(96)
    );
    claimed.observed.lock().failed_latencies.clear();
    let count = claimed.observed.lock().latencies.len();
    claimed.publish(vec![]).unwrap();
    assert_eq!(claimed.observed.lock().latencies.len(), count + 1);
    assert_eq!((claimed.reported("a"), claimed.reported("b")), (None, None));
    assert!(
        claimed
            .authority
            .destinations
            .values()
            .all(|entry| entry.input_latency_frames.is_none())
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
