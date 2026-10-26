//! Active owner selection while a replacement is prepared for the same graph node.

use super::*;
use crate::{AudioLayout, shared_transport, worker::broker::consume_parameters};

#[test]
fn prepared_replacement_cannot_steal_controls_from_active_instance() {
    let directory = mixrelay_test_support::FixtureDir::new("control-owner").unwrap();
    let registry = ProcessorControlRegistry::default();
    let node = NodeId::from("effect");
    let layout = AudioLayout {
        inputs: 2,
        outputs: 2,
        block_frames: 4,
    };
    let (_, active_mailbox) = shared_transport::create(&directory.join("active"), layout).unwrap();
    let active = Arc::new(HostedProcessorControl::new(active_mailbox));
    active.active_owner.store(true, Ordering::Release);
    active
        .shared
        .header()
        .worker_status
        .store(WORKER_READY, Ordering::Release);
    let _active_registration = registry.register(&node, &active);
    let (_, candidate_mailbox) =
        shared_transport::create(&directory.join("candidate"), layout).unwrap();
    let candidate = Arc::new(HostedProcessorControl::new(candidate_mailbox));
    candidate
        .shared
        .header()
        .worker_status
        .store(WORKER_READY, Ordering::Release);
    let _candidate_registration = registry.register(&node, &candidate);

    registry.set_parameter(&node, 7, 0.25).unwrap();
    let mut active_values = Vec::new();
    consume_parameters(active.shared.header(), |id, value| {
        active_values.push((id, value));
    });
    assert_eq!(active_values, [(7, 0.25)]);
    consume_parameters(candidate.shared.header(), |_, _| {
        panic!("candidate must remain untouched")
    });
    assert_eq!(
        registry
            .live_control(&node)
            .unwrap()
            .shared
            .header()
            .instance_id,
        active.shared.header().instance_id
    );

    candidate.active_owner.store(true, Ordering::Release);
    assert!(registry.set_parameter(&node, 7, 0.5).is_err());
    assert!(registry.live_control(&node).is_err());
    active.active_owner.store(false, Ordering::Release);
    registry.set_parameter(&node, 7, 0.75).unwrap();
    let mut candidate_values = Vec::new();
    consume_parameters(candidate.shared.header(), |id, value| {
        candidate_values.push((id, value));
    });
    assert_eq!(candidate_values, [(7, 0.75)]);
    consume_parameters(active.shared.header(), |_, _| {
        panic!("retired owner must remain untouched")
    });
}
