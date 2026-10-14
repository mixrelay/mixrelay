//! One shared publisher; subscriptions start with its complete current observation.
use crate::{ControlBackend, ControlOperations, ControlState, METER_TELEMETRY_INTERVAL};
use axum::{
    extract::State,
    response::sse::{Event, Sse},
};
use futures_util::StreamExt as _;
use mixrelay_control_protocol::{ControlStateSnapshot, ControlTelemetry, TelemetryEncoder};
use std::sync::{Arc, OnceLock};
use tokio::sync::{broadcast, watch};

/// Windows a subscriber may fall behind by before its stream ends (and it resubscribes).
const BACKLOG: usize = 64;

/// The current complete packet and the window broadcast. Receivers only, so both close
/// when the publisher ends.
type Channels = (
    watch::Receiver<Option<ControlTelemetry>>,
    broadcast::Receiver<ControlTelemetry>,
);

#[derive(Default)]
pub(super) struct Publisher {
    channels: OnceLock<Channels>,
}

/// One subscriber's view: the current complete packet, then every later window.
pub(super) struct Subscription {
    latest: watch::Receiver<Option<ControlTelemetry>>,
    windows: broadcast::Receiver<ControlTelemetry>,
    started: bool,
    sent: u64,
}

impl Publisher {
    fn subscribe(
        &self,
        backend: Arc<dyn ControlBackend>,
        operations: ControlOperations,
    ) -> Subscription {
        let (latest, windows) = self.channels.get_or_init(|| {
            let (sender, latest) = watch::channel(None);
            let (windows, receiver) = broadcast::channel(BACKLOG);
            tokio::spawn(publish(backend, operations, sender, windows));
            (latest, receiver)
        });
        // Subscribe to windows before reading the snapshot, so none falls between them.
        let windows = windows.resubscribe();
        Subscription {
            latest: latest.clone(),
            windows,
            started: false,
            sent: 0,
        }
    }
}

impl Subscription {
    /// The current snapshot first, then each later window. Windows that piled up while
    /// this client was slow fold into one (meters keep the interval maximum, state the
    /// latest), so backpressure never discards a peak. `None` once the publisher ends or
    /// this client fell [`BACKLOG`] windows behind.
    async fn next(&mut self) -> Option<ControlTelemetry> {
        let mut first = if self.started {
            None
        } else {
            self.started = true;
            self.latest.borrow().clone()
        };
        let mut packet = loop {
            let next = match first.take() {
                Some(packet) => packet,
                None => self.windows.recv().await.ok()?,
            };
            if next.sequence > self.sent {
                break next;
            }
        };
        loop {
            match self.windows.try_recv() {
                Ok(next) if next.sequence > packet.sequence => packet.absorb(next),
                Ok(_) => {}
                // Peaks were overwritten; resubscribing restarts from the snapshot.
                Err(broadcast::error::TryRecvError::Lagged(_)) => return None,
                // A closed publisher still delivers what it published.
                Err(_) => break,
            }
        }
        self.sent = packet.sequence;
        Some(packet)
    }
}

// Native editor closures, failures and recorder transitions happen outside HTTP mutations.
// Observe them on a blocking worker, once per shared publication window, never on audio
// or reactor threads. Only changed state is transmitted; the watch slot is a full snapshot.
async fn publish(
    backend: Arc<dyn ControlBackend>,
    operations: ControlOperations,
    sender: watch::Sender<Option<ControlTelemetry>>,
    windows: broadcast::Sender<ControlTelemetry>,
) {
    let mut interval = tokio::time::interval(METER_TELEMETRY_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut previous = None;
    let mut sequence = 0;
    loop {
        tokio::select! {
            () = operations.closed() => return,
            _ = interval.tick() => {},
        }
        let backend = Arc::clone(&backend);
        let Ok(mut packet) = tokio::task::spawn_blocking(move || observe(backend.as_ref())).await
        else {
            return;
        };
        sequence += 1;
        packet.sequence = sequence;
        if let Some(state) = &mut packet.state {
            advance(state, previous.as_ref());
            previous = Some(state.clone());
        }
        packet.state.clone_from(&previous);
        if packet.state.is_none() {
            continue;
        }
        sender.send_replace(Some(packet.clone()));
        // No subscriber is not an error; the next one starts from the snapshot.
        let _ = windows.send(packet);
    }
}

fn advance(state: &mut ControlStateSnapshot, previous: Option<&ControlStateSnapshot>) {
    state.generation = previous.map_or(1, |old| old.generation);
    if previous.is_some_and(|old| old != state) {
        state.generation += 1;
    }
}

fn observe(backend: &dyn ControlBackend) -> ControlTelemetry {
    ControlTelemetry {
        engine_incarnation: backend.incarnation(),
        sequence: 0,
        meters: backend.meters(),
        state: backend.telemetry_state(),
    }
}

pub(super) fn snapshot(backend: &(impl ControlBackend + ?Sized)) -> ControlStateSnapshot {
    let editors = backend
        .graph()
        .nodes
        .iter()
        .filter_map(|node| backend.processor_editor_status(node.id.as_str()).ok())
        .collect();
    ControlStateSnapshot {
        generation: 0,
        status: backend.status(),
        graph: backend.graph_status(),
        controls: backend.controls_snapshot().ok(),
        editors,
    }
}

pub(super) async fn subscribe(
    State(state): State<ControlState>,
    axum::Extension(operations): axum::Extension<ControlOperations>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let subscription = state.telemetry.subscribe(state.backend, operations.clone());
    // Each stream has its own encoder: meter bindings follow only when they change.
    let stream = futures_util::stream::unfold(
        (subscription, None, TelemetryEncoder::default()),
        |(mut subscription, generation, mut encoder)| async move {
            let mut packet = subscription.next().await?;
            let next_generation = packet.state.as_ref().map(|state| state.generation);
            if generation == next_generation {
                packet.state = None;
            }
            let event = Event::default()
                .event("telemetry")
                .data(encoder.json(&packet).to_string());
            Some((Ok(event), (subscription, next_generation, encoder)))
        },
    )
    .take_until(operations.closed());
    Sse::new(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn closing_admission_ends_the_shared_publisher_itself() {
        let publisher = Publisher::default();
        let operations = ControlOperations::new();
        operations.close_admission();
        let mut subscription = publisher.subscribe(Arc::new(crate::UnavailableBackend), operations);
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(2), subscription.next())
                .await
                .unwrap()
                .is_none()
        );
    }

    fn window(sequence: u64, peak: f32) -> ControlTelemetry {
        serde_json::from_value(serde_json::json!({
            "engineIncarnation": "engine",
            "sequence": sequence,
            "meters": {"protocolVersion": 1, "revision": 1, "nodes": [
                {"nodeId": "n", "portId": "out", "peaks": [peak]}
            ]}
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn a_slow_subscriber_folds_backlogged_windows_and_ends_when_too_far_behind() {
        let (sender, latest) = watch::channel(Some(window(5, 0.2)));
        let (windows, receiver) = broadcast::channel(BACKLOG);
        let mut subscription = Subscription {
            latest,
            windows: receiver,
            started: false,
            sent: 0,
        };
        for (sequence, peak) in [(4, 1.0), (6, 0.9), (7, 0.1)] {
            windows.send(window(sequence, peak)).unwrap();
        }
        // The snapshot first, folded with what piled up; older windows are skipped.
        let first = subscription.next().await.unwrap();
        assert_eq!((first.sequence, first.meters.nodes[0].peaks[0]), (7, 0.9));
        windows.send(window(7, 1.0)).unwrap();
        windows.send(window(8, 0.3)).unwrap();
        assert_eq!(subscription.next().await.unwrap().sequence, 8);
        for sequence in 9..=9 + BACKLOG as u64 {
            windows.send(window(sequence, 0.0)).unwrap();
        }
        assert!(subscription.next().await.is_none());
        drop(sender);
        // A snapshot whose windows were overwritten before the first read also ends.
        let (_sender, latest) = watch::channel(Some(window(1, 0.0)));
        let (windows, receiver) = broadcast::channel(BACKLOG);
        for sequence in 2..=2 + BACKLOG as u64 {
            windows.send(window(sequence, 0.0)).unwrap();
        }
        let mut lagged = Subscription {
            latest,
            windows: receiver,
            started: false,
            sent: 0,
        };
        assert!(lagged.next().await.is_none());
        // A publisher that ended still delivers its last window, then the stream ends.
        let (_sender, latest) = watch::channel(None);
        let (windows, receiver) = broadcast::channel(BACKLOG);
        windows.send(window(1, 0.5)).unwrap();
        drop(windows);
        let mut ended = Subscription {
            latest,
            windows: receiver,
            started: false,
            sent: 0,
        };
        assert_eq!(ended.next().await.unwrap().sequence, 1);
        assert!(ended.next().await.is_none());
    }

    #[test]
    fn generation_changes_for_each_state_family_but_not_steady_state() {
        let packet = observe(&crate::UnavailableBackend);
        assert_eq!(crate::UnavailableBackend.telemetry_state(), packet.state);
        let mut state = packet.state.unwrap();
        advance(&mut state, None);
        assert_eq!(state.generation, 1);
        let old = state.clone();
        advance(&mut state, Some(&old));
        assert_eq!(state, old);
        state.status.sample_rate = Some(48_000);
        advance(&mut state, Some(&old));
        assert_eq!(state.generation, 2);
        let old = state.clone();
        state.graph.active_revision += 1;
        advance(&mut state, Some(&old));
        assert_eq!(state.generation, 3);
        let old = state.clone();
        state.controls = Some(mixrelay_control_protocol::EngineControlSnapshot {
            revision: 3,
            active_revision: 3,
            structure_generation: 1,
            engine_power: mixrelay_control_protocol::EnginePowerStatus {
                enabled: true,
                transitioning: false,
                unsaved: false,
                pause_reason: None,
            },
            graphs: vec![],
        });
        advance(&mut state, Some(&old));
        assert_eq!(state.generation, 4);
        let old = state.clone();
        state.editors = vec![mixrelay_control_protocol::ProcessorEditorStatus {
            protocol_version: 1,
            node_id: "effect".into(),
            state: mixrelay_control_protocol::ProcessorEditorState::Closed,
            width: None,
            height: None,
            resizable: false,
            failure: None,
        }];
        advance(&mut state, Some(&old));
        assert_eq!(state.generation, 5);
        let old = state.clone();
        state.controls = None;
        state.editors.clear();
        advance(&mut state, Some(&old));
        assert_eq!(state.generation, 6);
    }
}
