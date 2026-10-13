//! Control-thread capture ownership. Preparing never edits the active set;
//! rejecting a remote graph transaction drops only the provisional owners.
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

use mixrelay_audio_graph::{ApplicationCaptureRequest, ApplicationSelector, NodeId};
use mixrelay_control_protocol::{ApplicationCaptureManifest, PROTOCOL_VERSION};

pub(super) struct Entry<S> {
    pub(super) node_id: String,
    pub(super) request: ApplicationCaptureRequest,
    pub(super) session: Arc<S>,
    activate: bool,
}

pub(super) struct CaptureSet<S> {
    pub(super) revision: u64,
    pub(super) sources: Vec<Entry<S>>,
}

pub(super) fn prepare<S>(
    manifest: &ApplicationCaptureManifest,
    active: Option<&CaptureSet<S>>,
    retained: &[&CaptureSet<S>],
    mut open: impl FnMut(&NodeId, &ApplicationCaptureRequest, Option<&Arc<S>>) -> Result<Arc<S>, String>,
) -> Result<CaptureSet<S>, String> {
    validate_manifest(manifest)?;
    let mut sources = Vec::with_capacity(manifest.sources.len());
    for source in &manifest.sources {
        let request = &source.request;
        let node_id = NodeId::from(source.node_id.as_str());
        let mut candidates = active
            .into_iter()
            .chain(retained.iter().copied())
            .flat_map(|set| set.sources.iter().map(move |entry| (set.revision, entry)))
            .filter(|(_, entry)| entry.node_id == node_id.as_str());
        // A lost-response reservation can already own the exact successor. Find
        // that owner before considering replacement of an older active session.
        let reusable = candidates
            .clone()
            .find(|(revision, entry)| {
                super::same_capture_session(&entry.request, request)
                    && (request.source_playback
                        == mixrelay_audio_graph::SourcePlaybackPolicy::Preserve
                        || *revision == manifest.revision)
            })
            .map(|(_, entry)| entry);
        // Scoped mute admission is renewed only by a new explicit manifest.
        // A retry of that same revision retains its already prepared lease.
        let previous = reusable.or_else(|| candidates.next().map(|(_, entry)| entry));
        let session = if let Some(entry) = reusable {
            Arc::clone(&entry.session)
        } else {
            open(&node_id, request, previous.map(|entry| &entry.session))?
        };
        sources.push(Entry {
            node_id: node_id.as_str().to_owned(),
            request: request.clone(),
            session,
            activate: reusable.map_or(
                previous.is_some()
                    || request.source_playback
                        == mixrelay_audio_graph::SourcePlaybackPolicy::MuteWhileCaptured,
                |entry| entry.activate,
            ),
        });
    }
    Ok(CaptureSet {
        revision: manifest.revision,
        sources,
    })
}

fn validate_manifest(manifest: &ApplicationCaptureManifest) -> Result<(), String> {
    if manifest.protocol_version != PROTOCOL_VERSION || manifest.sources.len() > 64 {
        return Err("unsupported application capture manifest".into());
    }
    let mut ids = BTreeSet::new();
    for source in &manifest.sources {
        let id = &source.node_id;
        if id.is_empty()
            || id.len() >= 128
            || id.chars().any(char::is_control)
            || !ids.insert(id)
            || source.request.gain_db != 0.0
            || source.request.requires_reselection
            || source.request.stream.channels != 2
            || !matches!(
                &source.request.selector,
                ApplicationSelector::BundleId {
                    include_descendants: false,
                    ..
                }
            )
        {
            return Err("invalid canonical application capture identity or policy".into());
        }
        source
            .request
            .validate_contract(64)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Active PCM owners and accepted-but-not-yet-active successors have separate lifetimes.
/// The prior graph may still be rendering after HTTP acceptance.
pub(super) struct CaptureOwners<S> {
    pub(super) active: Option<CaptureSet<S>>,
    pub(super) pending: Option<CaptureSet<S>>,
    reserved: Option<CaptureSet<S>>,
    retiring: Vec<CaptureSet<S>>,
}
impl<S> Default for CaptureOwners<S> {
    fn default() -> Self {
        Self {
            active: None,
            pending: None,
            reserved: None,
            retiring: Vec::new(),
        }
    }
}
impl<S> CaptureOwners<S> {
    pub(super) fn retained(&self) -> Result<Vec<&CaptureSet<S>>, String> {
        // Backpressure precedes native preparation and HTTP publication. An
        // unacknowledged engine cannot accumulate unlimited provisional owners.
        if self.retiring.len() >= 64 {
            return Err("too many unacknowledged application capture revisions".into());
        }
        Ok(self
            .reserved
            .iter()
            .chain(self.active.iter().chain(self.retiring.iter().rev()))
            .collect())
    }
    /// Reserve before sending a mutation, while failure can still safely drop new taps.
    pub(super) fn ensure_reservable(&self) -> Result<(), String> {
        if self.reserved.is_some() {
            return Err("application capture mutation outcome is still unresolved".into());
        }
        if self.retiring.len() >= 64 {
            return Err("too many unacknowledged application capture revisions".into());
        }
        Ok(())
    }
    pub(super) fn reserve(&mut self, prepared: CaptureSet<S>) -> Result<(), String> {
        self.ensure_reservable()?;
        self.validate_successor(&prepared)?;
        self.retiring
            .try_reserve(1)
            .map_err(|error| format!("capture retirement reservation failed: {error}"))?;
        self.reserved = Some(prepared);
        Ok(())
    }
    /// Reservation already checked revision/capacity before PUT. Publication does
    /// not allocate, revalidate, or discard an accepted owner on a later error.
    pub(super) fn accept_reserved(&mut self) -> Result<Option<CaptureSet<S>>, String> {
        let prepared = self
            .reserved
            .take()
            .ok_or("application capture reservation is missing")?;
        Ok(self.publish(prepared))
    }
    pub(super) fn reject_reserved(&mut self) -> Option<CaptureSet<S>> {
        self.reserved.take()
    }
    pub(super) fn has_unacknowledged(&self) -> bool {
        self.pending.is_some() || self.reserved.is_some()
    }
    pub(super) fn requires_manifest(&self, submitted_revision: u64) -> bool {
        self.reserved.is_some()
            || self.latest().map(|latest| latest.revision) != Some(submitted_revision)
    }
    pub(super) fn latest(&self) -> Option<&CaptureSet<S>> {
        self.pending.as_ref().or(self.active.as_ref())
    }
    pub(super) fn stage(
        &mut self,
        prepared: CaptureSet<S>,
    ) -> Result<Option<CaptureSet<S>>, String> {
        self.validate_successor(&prepared)?;
        if self
            .reserved
            .as_ref()
            .is_some_and(|reserved| reserved.revision > prepared.revision)
        {
            return Err("application capture mutation outcome is still unresolved".into());
        }
        self.retiring
            .try_reserve(2)
            .map_err(|error| format!("capture retirement reservation failed: {error}"))?;
        // A newer authoritative manifest may supersede a lost-response mutation
        // that is currently rendering. Retire that reservation only after its successor ACK.
        self.retiring.extend(self.reserved.take());
        Ok(self.publish(prepared))
    }
    fn validate_successor(&self, prepared: &CaptureSet<S>) -> Result<(), String> {
        if self
            .latest()
            .is_some_and(|latest| latest.revision > prepared.revision)
        {
            return Err("stale application capture manifest".into());
        }
        if let Some(latest) = self
            .latest()
            .filter(|latest| latest.revision == prepared.revision)
            && (latest.sources.len() != prepared.sources.len()
                || latest
                    .sources
                    .iter()
                    .zip(&prepared.sources)
                    .any(|(left, right)| {
                        left.node_id != right.node_id || left.request != right.request
                    }))
        {
            return Err("one capture revision has conflicting source definitions".into());
        }
        Ok(())
    }
    fn publish(&mut self, prepared: CaptureSet<S>) -> Option<CaptureSet<S>> {
        let revision = prepared.revision;
        let previous = self.pending.replace(prepared);
        if previous
            .as_ref()
            .is_some_and(|previous| previous.revision != revision)
        {
            self.retiring.extend(previous);
            return None;
        }
        previous
    }
    pub(super) fn acknowledge(
        &mut self,
        active_revision: u64,
        activate: impl FnMut(&S),
    ) -> Vec<CaptureSet<S>> {
        if self
            .pending
            .as_ref()
            .is_none_or(|pending| pending.revision > active_revision)
        {
            return Vec::new();
        }
        let Some(prepared) = self.pending.take() else {
            return Vec::new();
        };
        let mut retired = std::mem::take(&mut self.retiring);
        retired.extend(commit(&mut self.active, prepared, activate));
        retired
    }
}

/// The activation callback must be infallible and perform only a control handoff.
/// Return retired owners so their worker joins happen outside the state lock.
pub(super) fn commit<S>(
    active: &mut Option<CaptureSet<S>>,
    mut prepared: CaptureSet<S>,
    mut activate: impl FnMut(&S),
) -> Option<CaptureSet<S>> {
    for entry in &mut prepared.sources {
        if entry.activate {
            activate(&entry.session);
            entry.activate = false;
        }
    }
    active.replace(prepared)
}

/// Latest committed capture for a non-real-time pump. Only its worker drains the
/// SPSC ring; owners are released on control/pump threads, never an audio callback.
pub(super) struct CaptureHandoff<S>(Mutex<Option<S>>);

impl<S> Default for CaptureHandoff<S> {
    fn default() -> Self {
        Self(Mutex::new(None))
    }
}

impl<S> CaptureHandoff<S> {
    pub(super) fn publish(&self, capture: S) {
        // No user code runs while locked. Recovering poison cannot violate the
        // single Option invariant, and a successful remote commit cannot fail here.
        let retired = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .replace(capture);
        drop(retired);
    }

    /// For a realtime consumer: never waits for a publisher. A contended handoff stays
    /// published and is taken on a later pass.
    pub(super) fn try_take(&self) -> Option<S> {
        match self.0.try_lock() {
            Ok(mut slot) => slot.take(),
            Err(std::sync::TryLockError::Poisoned(slot)) => slot.into_inner().take(),
            Err(std::sync::TryLockError::WouldBlock) => None,
        }
    }
}

#[cfg(test)]
#[path = "capture_transaction_tests.rs"]
mod tests;
