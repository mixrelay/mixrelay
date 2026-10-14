//! Runtime publication lives on a control task. Unstamped PCM queues remain unknown.
use mixrelay_audio_engine::ProcessorExchange;
use mixrelay_driver_control::{DriverClient, EngineLatencyLease, LatencyCommand, RuntimeLatency};
use std::sync::Arc;
use tokio::{sync::oneshot, task::JoinHandle};

pub(super) struct Reporter {
    stop: Option<oneshot::Sender<()>>,
    worker: Option<JoinHandle<()>>,
}
impl Reporter {
    pub(super) fn start(
        exchange: Arc<ProcessorExchange>,
        incarnation: String,
    ) -> Result<Self, String> {
        let client = DriverClient::new().map_err(|error| error.to_string())?;
        let (stop, receive) = oneshot::channel();
        let worker = tokio::spawn(observe(client, exchange, incarnation, receive));
        Ok(Self {
            stop: Some(stop),
            worker: Some(worker),
        })
    }
    pub(super) async fn finish(mut self) -> Result<(), String> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(worker) = self.worker.take() {
            worker.await.map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}
impl Drop for Reporter {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        // The task releases its lease when signalled. Abrupt process death uses service expiry.
    }
}
async fn observe(
    client: DriverClient,
    exchange: Arc<ProcessorExchange>,
    incarnation: String,
    mut stop: oneshot::Receiver<()>,
) {
    let mut lease: Option<EngineLatencyLease> = None;
    let mut previous_error = None;
    loop {
        let result = publish(&client, &exchange, &incarnation, &mut lease).await;
        match result {
            Ok(true) => previous_error = None,
            Ok(false) => break, // compatible old service has no runtime route
            Err(error) => {
                if previous_error.as_ref() != Some(&error) {
                    tracing::warn!(%error, "input latency publication unavailable");
                }
                previous_error = Some(error);
            }
        }
        tokio::select! { _ = &mut stop => break, () = tokio::time::sleep(std::time::Duration::from_secs(1)) => {} }
    }
    if let Some(lease) = lease {
        let _ = client
            .publish_latency(&LatencyCommand::Release { lease })
            .await;
    }
}
async fn publish(
    client: &DriverClient,
    exchange: &ProcessorExchange,
    incarnation: &str,
    lease: &mut Option<EngineLatencyLease>,
) -> Result<bool, String> {
    if lease.is_none() {
        let reply = client
            .publish_latency(&LatencyCommand::Claim {
                incarnation: incarnation.into(),
                process_id: std::process::id(),
            })
            .await
            .map_err(|error| error.to_string())?;
        let Some(reply) = reply else {
            return Ok(false);
        };
        if !reply.accepted {
            return Err("previous engine latency lease has not expired".into());
        }
        *lease = reply.lease;
    }
    let owner = lease
        .as_ref()
        .ok_or("Driver omitted the engine latency lease")?;
    let mut destinations = std::collections::BTreeMap::new();
    if let Some(plan) = exchange.latency_plan() {
        let settings = client.snapshot().await.map_err(|error| error.to_string())?;
        for sink in plan.sink_bindings() {
            let Some(endpoint_id) = sink.virtual_endpoint_id() else {
                continue;
            };
            let Some(rate) = settings
                .settings
                .devices
                .iter()
                .find(|device| &device.spec.id == endpoint_id)
                .and_then(|device| device.observed_sample_rate)
            else {
                continue;
            };
            destinations.insert(
                endpoint_id.clone(),
                RuntimeLatency {
                    endpoint_id: endpoint_id.clone(),
                    rate,
                    engine_lease: owner.clone(),
                    applied_graph_revision: exchange.revisions().active,
                    // No FIFO carries first-content host-time provenance yet. Reporting a
                    // prepared processor delay alone would falsely label an unresolved path.
                    input_latency_frames: None,
                },
            );
        }
    }
    let reply = client
        .publish_latency(&LatencyCommand::Publish {
            lease: owner.clone(),
            destinations: destinations.into_values().collect(),
        })
        .await
        .map_err(|error| error.to_string())?;
    if let Some(reply) = reply {
        if !reply.accepted {
            *lease = None;
        }
        Ok(true)
    } else {
        Ok(false)
    }
}
