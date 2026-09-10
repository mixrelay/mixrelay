//! Application intent survives registry stream replacement. Discovery and native
//! ownership changes run off RT; callback activation only swaps existing slots.
use super::{MAX_ENDPOINT_CHANNELS, native::NativeStream, sanitize_node_name};
use mixrelay_audio_graph::{ApplicationCaptureRequest, ApplicationSelector, SourcePlaybackPolicy};
use mixrelay_audio_io::{
    linux::{
        PipeWireTarget,
        process_identity::{ProcessIdentity, process_identity, same_process},
    },
    process::{InputActivity, ProcessReport, TransferOutcome},
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Default)]
pub(super) struct CaptureSet {
    pub generation: u64,
    nodes: BTreeMap<String, Capture>,
}

struct Capture {
    taps: Vec<NativeStream>,
    scratch: Box<[f32]>,
    healthy: Arc<AtomicBool>,
}

#[derive(Clone)]
struct WatchEntry {
    node_id: String,
    request: ApplicationCaptureRequest,
    pinned_process: Option<ProcessIdentity>,
    serials: Vec<String>,
    healthy: Arc<AtomicBool>,
    format: mixrelay_audio_graph::RenderFormat,
}

#[derive(Clone)]
pub(super) struct CaptureWatch {
    pub generation: u64,
    entries: Vec<WatchEntry>,
}

impl CaptureSet {
    pub(super) fn discard_inputs(&mut self) {
        for capture in self.nodes.values_mut() {
            for tap in &mut capture.taps {
                tap.discard_input();
            }
        }
    }
    pub(super) fn formats_ready(&self) -> bool {
        self.nodes
            .values()
            .all(|capture| capture.taps.iter().all(NativeStream::ready))
    }
    pub(super) fn shares_clock(&self, id: u32, rate: u32) -> bool {
        self.nodes.values().all(|capture| {
            !capture.taps.is_empty() && capture.taps.iter().all(|tap| tap.shares_clock(id, rate))
        })
    }
    pub(super) fn contains(&self, node_id: &str) -> bool {
        self.nodes.contains_key(node_id)
    }
    pub(super) fn state(&self) -> mixrelay_engine_service::session::ApplicationRuntimeState {
        use mixrelay_engine_service::session::ApplicationRuntimeState;
        if self
            .nodes
            .values()
            .any(|capture| capture.taps.is_empty() || !capture.taps.iter().all(NativeStream::ready))
        {
            ApplicationRuntimeState::Waiting
        } else {
            ApplicationRuntimeState::Ready
        }
    }
    #[cfg(test)]
    pub(super) fn open(
        requests: &[(String, ApplicationCaptureRequest)],
        generation: u64,
    ) -> Result<(Self, CaptureWatch), String> {
        Self::open_format(requests, generation, super::render_format())
    }

    pub(super) fn open_format(
        requests: &[(String, ApplicationCaptureRequest)],
        generation: u64,
        format: mixrelay_audio_graph::RenderFormat,
    ) -> Result<(Self, CaptureWatch), String> {
        let inventory = if requests
            .iter()
            .all(|(_, request)| request.requires_reselection)
        {
            Vec::new()
        } else {
            mixrelay_audio_io::linux::targets().map_err(|error| error.to_string())?
        };
        Self::open_inventory_format(requests, generation, &inventory, format)
    }

    #[cfg(test)]
    fn open_inventory(
        requests: &[(String, ApplicationCaptureRequest)],
        generation: u64,
        inventory: &[PipeWireTarget],
    ) -> Result<(Self, CaptureWatch), String> {
        Self::open_inventory_format(requests, generation, inventory, super::render_format())
    }

    fn open_inventory_format(
        requests: &[(String, ApplicationCaptureRequest)],
        generation: u64,
        inventory: &[PipeWireTarget],
        format: mixrelay_audio_graph::RenderFormat,
    ) -> Result<(Self, CaptureWatch), String> {
        let mut set = Self {
            generation,
            ..Self::default()
        };
        let mut watch = CaptureWatch {
            generation,
            entries: Vec::new(),
        };
        for (node_id, request) in requests {
            request
                .validate_contract(MAX_ENDPOINT_CHANNELS)
                .map_err(|error| error.to_string())?;
            if request.source_playback == SourcePlaybackPolicy::MuteWhileCaptured {
                return Err("PipeWire source muting is not implemented".into());
            }
            let pin = match request.selector {
                _ if request.requires_reselection => None,
                ApplicationSelector::ProcessId { process_id, .. } => {
                    Some(process_identity(process_id).ok_or_else(|| {
                        format!("cannot verify selected process identity for {node_id}")
                    })?)
                }
                _ => None,
            };
            let targets = if request.requires_reselection {
                Vec::new()
            } else {
                matching_targets(inventory, &request.selector, pin.as_ref(), process_identity)
            };
            let healthy = Arc::new(AtomicBool::new(true));
            let capture = prepare(node_id, request, &targets, healthy.clone(), format)?;
            watch.entries.push(WatchEntry {
                node_id: node_id.clone(),
                request: request.clone(),
                pinned_process: pin,
                serials: serials(&targets),
                healthy,
                format,
            });
            set.nodes.insert(node_id.clone(), capture);
        }
        Ok((set, watch))
    }

    pub(super) fn replace(&mut self, next: &mut Self) {
        for (node, capture) in &mut next.nodes {
            if let Some(current) = self.nodes.get_mut(node) {
                for tap in &mut capture.taps {
                    tap.discard_input();
                }
                std::mem::swap(current, capture);
            }
        }
    }

    pub(super) fn read(&mut self, node: &str, destination: &mut [f32]) -> TransferOutcome {
        destination.fill(0.0);
        let Some(capture) = self.nodes.get_mut(node) else {
            return TransferOutcome {
                activity: InputActivity::Unknown,
                ..TransferOutcome::complete(0)
            };
        };
        capture.healthy.store(
            capture.taps.iter().all(NativeStream::ready),
            Ordering::Release,
        );
        let mut transfer = TransferOutcome::complete(0);
        transfer.activity =
            if capture.taps.is_empty() || !capture.taps.iter().all(NativeStream::ready) {
                InputActivity::Unknown
            } else {
                InputActivity::Inactive
            };
        let scratch = &mut capture.scratch[..destination.len()];
        for tap in &mut capture.taps {
            scratch.fill(0.0);
            let activity = tap.input_activity();
            if activity != InputActivity::Active {
                tap.discard_input();
                // PAUSED is explicit native inactivity, not an empty-ring guess.
                transfer.discontinuity |= activity == InputActivity::Unknown;
                if activity == InputActivity::Unknown && transfer.activity != InputActivity::Active
                {
                    transfer.activity = InputActivity::Unknown;
                }
                continue;
            }
            transfer.activity = InputActivity::Active;
            let frames = tap.read(scratch);
            transfer.frames = transfer.frames.max(frames);
            transfer.discontinuity |= frames == 0;
            for (mixed, sample) in destination.iter_mut().zip(scratch.iter()) {
                *mixed += sample;
            }
        }
        transfer
    }

    pub(super) fn take_report(&mut self) -> ProcessReport {
        let mut report = ProcessReport::default();
        for capture in self.nodes.values_mut() {
            capture.healthy.store(
                capture.taps.iter().all(NativeStream::ready),
                Ordering::Release,
            );
            for tap in &mut capture.taps {
                let next = tap.take_report();
                report.input_starvations += next.input_starvations;
                report.output_overflows += next.output_overflows;
                report.discontinuities += next.discontinuities;
            }
        }
        report
    }
}

impl CaptureWatch {
    pub(super) fn recover(
        &self,
        stopping: &AtomicBool,
    ) -> Result<Option<(CaptureSet, Self)>, String> {
        if self
            .entries
            .iter()
            .all(|entry| entry.request.requires_reselection)
            || stopping.load(Ordering::Acquire)
        {
            return Ok(None);
        }
        let inventory = mixrelay_audio_io::linux::targets().map_err(|error| error.to_string())?;
        let mut updates = CaptureSet {
            generation: self.generation,
            ..CaptureSet::default()
        };
        let mut next_watch = self.clone();
        for entry in &mut next_watch.entries {
            if stopping.load(Ordering::Acquire) {
                return Ok(None);
            }
            let Some(targets) = entry.changed_targets(&inventory, process_identity) else {
                continue;
            };
            let next_serials = serials(&targets);
            // Removing the final stream is also an update: empty capture remains
            // watched, produces silence, and reconnects when the app resumes.
            let Ok(capture) = prepare(
                &entry.node_id,
                &entry.request,
                &targets,
                entry.healthy.clone(),
                entry.format,
            ) else {
                continue;
            };
            entry.serials = next_serials;
            updates.nodes.insert(entry.node_id.clone(), capture);
        }
        Ok((!updates.nodes.is_empty()).then_some((updates, next_watch)))
    }
}

impl WatchEntry {
    fn changed_targets<'a>(
        &self,
        inventory: &'a [PipeWireTarget],
        lookup: impl Fn(u32) -> Option<ProcessIdentity>,
    ) -> Option<Vec<&'a PipeWireTarget>> {
        if self.request.requires_reselection {
            return None;
        }
        let targets = matching_targets(
            inventory,
            &self.request.selector,
            self.pinned_process.as_ref(),
            lookup,
        );
        (serials(&targets) != self.serials || !self.healthy.load(Ordering::Acquire))
            .then_some(targets)
    }
}

fn serials(targets: &[&PipeWireTarget]) -> Vec<String> {
    let mut serials = targets
        .iter()
        .map(|target| target.serial.clone())
        .collect::<Vec<_>>();
    serials.sort_unstable();
    serials.dedup();
    serials
}

fn prepare(
    node: &str,
    request: &ApplicationCaptureRequest,
    targets: &[&PipeWireTarget],
    healthy: Arc<AtomicBool>,
    format: mixrelay_audio_graph::RenderFormat,
) -> Result<Capture, String> {
    let taps = targets
        .iter()
        .map(|target| {
            NativeStream::open_format(
                &format!("{}.capture.{}", sanitize_node_name(node), target.serial),
                &target.display_name,
                Some(&target.serial),
                2,
                request.stream.channels,
                format,
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Capture {
        taps,
        healthy,
        scratch: vec![0.0; usize::from(request.stream.channels) * format.block_frames as usize]
            .into_boxed_slice(),
    })
}

fn matching_targets<'a>(
    inventory: &'a [PipeWireTarget],
    selector: &ApplicationSelector,
    pin: Option<&ProcessIdentity>,
    lookup: impl Fn(u32) -> Option<ProcessIdentity>,
) -> Vec<&'a PipeWireTarget> {
    let pinned_alive = pin.is_some_and(|pin| {
        lookup(pin.process_id).is_some_and(|current| same_process(pin, &current))
    });
    let matches = inventory
        .iter()
        .filter(|target| {
            target.media_class == "Stream/Output/Audio"
                && target.process_id != Some(std::process::id())
        })
        .filter(|target| match selector {
            ApplicationSelector::BundleId { bundle_id, .. } => {
                target.application_id.as_deref() == Some(bundle_id)
            }
            ApplicationSelector::ExecutablePath { path } => target
                .process_id
                .and_then(&lookup)
                .and_then(|process| process.executable_path)
                .is_some_and(|actual| {
                    actual == *path
                        && target.executable.as_ref().is_some_and(|reported| {
                            reported == &actual
                                || std::path::Path::new(&actual)
                                    .file_name()
                                    .and_then(|name| name.to_str())
                                    == Some(reported)
                        })
                }),
            ApplicationSelector::ProcessId {
                process_id,
                include_descendants,
            } => {
                pinned_alive
                    && target.process_id.is_some_and(|pid| {
                        pid == *process_id
                            || (*include_descendants && descendant(pid, *process_id, &lookup))
                    })
            }
            ApplicationSelector::AllExcept { bundle_ids } => !target
                .application_id
                .as_ref()
                .or(target.executable.as_ref())
                .is_some_and(|id| bundle_ids.contains(id)),
        })
        .collect();
    // Recheck after ancestry walking: an exit/reused root during discovery may
    // not transfer the legacy process intent to a different process family.
    if pin.is_some_and(|pin| {
        lookup(pin.process_id).is_none_or(|current| !same_process(pin, &current))
    }) {
        Vec::new()
    } else {
        matches
    }
}

fn descendant(
    mut candidate: u32,
    ancestor: u32,
    lookup: &impl Fn(u32) -> Option<ProcessIdentity>,
) -> bool {
    for _ in 0..64 {
        let Some(identity) = lookup(candidate) else {
            return false;
        };
        if identity.parent_id == ancestor {
            return true;
        }
        if identity.parent_id <= 1 || identity.parent_id == candidate {
            return false;
        }
        candidate = identity.parent_id;
    }
    false
}

#[cfg(test)]
#[path = "linux_application_tests.rs"]
pub(crate) mod tests;
