//! Ephemeral latency authority is independent of persisted inventory revisions.
use crate::{DriverBackend, DriverError};
use mixrelay_audio_io::VirtualEndpointId;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

/// Additive runtime route; old services return 404 and engines continue without reporting.
pub const LATENCY_ROUTE: &str = "/api/v1/driver/latency";
const LEASE_TIMEOUT: Duration = Duration::from_secs(3);

/// Service-issued authority; an engine incarnation cannot reuse a previous lease.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineLatencyLease {
    pub generation: u64,
    pub incarnation: String,
    pub process_id: u32,
}

/// Destination-rate frames for a validated common content arrival. `None` is unknown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeLatency {
    pub endpoint_id: VirtualEndpointId,
    pub rate: u32,
    pub engine_lease: EngineLatencyLease,
    pub applied_graph_revision: u64,
    pub input_latency_frames: Option<u32>,
}

/// Control-only runtime publication; no audio callback performs HTTP.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "action",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum LatencyCommand {
    Claim {
        incarnation: String,
        process_id: u32,
    },
    Publish {
        lease: EngineLatencyLease,
        destinations: Vec<RuntimeLatency>,
    },
    Release {
        lease: EngineLatencyLease,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LatencyReply {
    pub accepted: bool,
    pub lease: Option<EngineLatencyLease>,
}

#[derive(Default)]
pub(crate) struct LatencyAuthority {
    current: Option<(EngineLatencyLease, Instant)>,
    destinations: BTreeMap<VirtualEndpointId, RuntimeLatency>,
    generation: u64,
}
impl LatencyAuthority {
    pub(crate) fn expire(
        &mut self,
        now: Instant,
        backend: &mut dyn DriverBackend,
    ) -> Result<(), DriverError> {
        if self
            .current
            .as_ref()
            .is_some_and(|(_, deadline)| now >= *deadline)
        {
            self.clear(backend)?;
        }
        Ok(())
    }
    fn clear(&mut self, backend: &mut dyn DriverBackend) -> Result<(), DriverError> {
        for publication in self.destinations.values() {
            backend.publish_latency(&RuntimeLatency {
                input_latency_frames: None,
                ..publication.clone()
            })?;
        }
        self.destinations.clear();
        self.current = None;
        Ok(())
    }
    pub(crate) fn command(
        &mut self,
        command: LatencyCommand,
        now: Instant,
        backend: &mut dyn DriverBackend,
    ) -> Result<LatencyReply, DriverError> {
        self.expire(now, backend)?;
        match command {
            LatencyCommand::Claim {
                incarnation,
                process_id,
            } => {
                if incarnation.is_empty()
                    || incarnation.len() > 128
                    || process_id == 0
                    || i32::try_from(process_id).is_err()
                {
                    return Err(DriverError::Invalid(
                        "invalid engine latency identity".into(),
                    ));
                }
                if self.current.is_some() {
                    return Ok(self.reply(false));
                }
                let epoch = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos();
                self.generation = self
                    .generation
                    .checked_add(1)
                    .ok_or_else(|| DriverError::Unavailable("latency lease exhausted".into()))?
                    .max(u64::try_from(epoch).unwrap_or(u64::MAX));
                self.current = Some((
                    EngineLatencyLease {
                        generation: self.generation,
                        incarnation,
                        process_id,
                    },
                    now + LEASE_TIMEOUT,
                ));
            }
            LatencyCommand::Publish {
                lease,
                destinations,
            } => {
                if !self.matches(&lease) {
                    return Ok(self.reply(false));
                }
                if destinations.len() > 64
                    || destinations
                        .iter()
                        .any(|entry| entry.rate == 0 || entry.engine_lease != lease)
                {
                    return Err(DriverError::Invalid("invalid latency publication".into()));
                }
                let mut seen = std::collections::BTreeSet::new();
                if destinations
                    .iter()
                    .any(|entry| !seen.insert(entry.endpoint_id.clone()))
                {
                    return Err(DriverError::Invalid("duplicate latency endpoint".into()));
                }
                for entry in destinations {
                    if self
                        .destinations
                        .get(&entry.endpoint_id)
                        .is_some_and(|old| {
                            old.applied_graph_revision > entry.applied_graph_revision
                        })
                    {
                        continue;
                    }
                    backend.publish_latency(&entry)?;
                    self.destinations.insert(entry.endpoint_id.clone(), entry);
                }
                self.current = Some((lease, now + LEASE_TIMEOUT));
            }
            LatencyCommand::Release { lease } => {
                if !self.matches(&lease) {
                    return Ok(self.reply(false));
                }
                self.clear(backend)?;
            }
        }
        Ok(self.reply(true))
    }
    fn matches(&self, lease: &EngineLatencyLease) -> bool {
        self.current
            .as_ref()
            .is_some_and(|(current, _)| current == lease)
    }
    fn reply(&self, accepted: bool) -> LatencyReply {
        LatencyReply {
            accepted,
            lease: self.current.as_ref().map(|(lease, _)| lease.clone()),
        }
    }
}

#[cfg(test)]
mod tests;
