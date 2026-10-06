//! Prepared invalidation scope for one external input's timeline change.

use std::{collections::BTreeMap, ops::Range};

use mixrelay_audio_graph::{ExecutionPlan, NodeConfig};

use super::{DeviceBusBinding, ExternalBinding};

/// Only histories reachable from the restarted input belong to its invalidation scope.
#[derive(Debug)]
pub(super) struct Targets {
    pub(super) device_buses: Box<[Range<usize>]>,
    pub(super) delay_routes: Box<[usize]>,
}

/// Walks the compiled topology off the audio thread, retaining only reset indices.
/// A visited set bounds causal feedback traversal; no graph walk occurs during rendering.
pub(super) fn prepare(
    execution: &ExecutionPlan,
    sources: &[ExternalBinding],
    buses: &[DeviceBusBinding],
) -> Box<[Targets]> {
    let indices: BTreeMap<_, _> = execution
        .nodes()
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id(), index))
        .collect();
    let routes: Vec<_> = execution
        .edges()
        .iter()
        .map(|edge| {
            (
                indices[&edge.source().node_id],
                indices[&edge.destination().node_id],
            )
        })
        .collect();
    let mut outgoing = vec![Vec::new(); indices.len()];
    for &(source, destination) in &routes {
        outgoing[source].push(destination);
    }
    let mut reached = vec![false; indices.len()];
    let mut pending = Vec::with_capacity(indices.len());
    sources
        .iter()
        .map(|source| {
            reached.fill(false);
            let start = indices[&source.node_id];
            reached[start] = true;
            pending.push(start);
            while let Some(node) = pending.pop() {
                for &next in &outgoing[node] {
                    // A transport's graph-to-OS sink does not feed its independent
                    // OS-to-graph source. Canonical mixing is represented by real bus edges.
                    let terminal = matches!(
                        execution.nodes()[next].config(),
                        NodeConfig::VirtualEndpointTransport(_)
                    );
                    if !reached[next] && !terminal {
                        reached[next] = true;
                        pending.push(next);
                    }
                }
            }
            Targets {
                device_buses: buses
                    .iter()
                    .filter(|bus| reached[indices[&bus.node_id]])
                    .map(|bus| bus.samples.clone())
                    .collect(),
                delay_routes: routes
                    .iter()
                    .enumerate()
                    .filter_map(|(index, &(from, _))| reached[from].then_some(index))
                    .collect(),
            }
        })
        .collect()
}
