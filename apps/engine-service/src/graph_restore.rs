//! Startup recall retains saved documents independently of transient native availability.

use std::time::{Duration, Instant};

use mixrelay_audio_io::ApplicationSelector;
use mixrelay_control_protocol::{GraphApplyResponse, ProtocolError};

use super::{EngineGraphController, GraphDocument, NodeConfig, invalid, unavailable};

/// Retry deadline and last reported native restore failure; never persisted as catalog data.
#[derive(Debug)]
pub(crate) struct PendingRestore {
    retry_after: Instant,
    error: ProtocolError,
}

impl EngineGraphController {
    /// The caller owns the mutation lock, or exclusive startup builder ownership.
    pub(super) fn restore_catalog_locked(&self) -> Result<GraphApplyResponse, ProtocolError> {
        let mut document = self.graph.read().clone();
        document.revision = self
            .catalog
            .lock()
            .as_ref()
            .ok_or_else(|| unavailable("graph catalog not enabled"))?
            .value
            .as_ref()
            .map_err(|error| unavailable(error))?
            .next_audio_revision(self.exchange.revisions().submitted)
            .ok_or_else(|| invalid("audio revision overflow"))?;
        let candidate = self.observed_catalog_candidate(&mut document)?;
        self.apply_graph_locked(document, candidate)
    }

    /// Records the failure without invalidating the durable catalog, logging only changes.
    pub(crate) fn defer_catalog_restore(&self, error: ProtocolError) {
        let mut pending = self.catalog_restore.lock();
        if pending.as_ref().is_none_or(|previous| {
            previous.error.code != error.code || previous.error.parameters != error.parameters
        }) {
            tracing::warn!(
                ?error,
                "saved graph native restore deferred; catalog retained"
            );
        }
        *pending = Some(PendingRestore {
            retry_after: Instant::now() + Duration::from_secs(1),
            error,
        });
    }

    /// Retries startup recall only on the existing non-real-time maintenance worker.
    /// A concurrent control mutation defers this attempt; status reads never trigger it.
    pub(crate) fn retry_catalog_restore(&self) {
        let Some(_mutation) = self.mutation.try_lock() else {
            return;
        };
        if self
            .catalog_restore
            .lock()
            .as_ref()
            .is_none_or(|pending| Instant::now() < pending.retry_after)
        {
            return;
        }
        match self.restore_catalog_locked() {
            Ok(response) => tracing::info!(
                revision = response.revision,
                "saved graph native restore recovered"
            ),
            Err(error) => self.defer_catalog_restore(error),
        }
    }
}

pub(super) fn require_pid_reselection(document: &mut GraphDocument) {
    for node in &mut document.nodes {
        if let NodeConfig::ApplicationCapture(request) = &mut node.config
            && matches!(request.selector, ApplicationSelector::ProcessId { .. })
        {
            request.requires_reselection = true;
        }
    }
}

#[cfg(test)]
#[path = "graph_restore_tests.rs"]
mod tests;
