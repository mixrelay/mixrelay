use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering},
};

use arc_swap::{ArcSwap, Guard};
use parking_lot::Mutex;
use thiserror::Error;
use tonenari_audio_graph::ExecutionPlan;

mod render;

pub use render::{
    BlockProcessor, ExternalBinding, ExternalIo, HostedAudioProcessor, HostedProcessorFactory,
    HostedProcessorPrepareRequest, MeterBinding, ProcessBlockError, RenderPlan, RenderPlanError,
    recording_session_key,
};

#[derive(Debug)]
struct PeakMeterCell {
    peak_bits: AtomicU32,
    revision: AtomicU64,
}

#[derive(Debug)]
pub struct PeakMeterBank {
    cells: Box<[PeakMeterCell]>,
    maximum_bindings: usize,
    maximum_channels: usize,
}

impl PeakMeterBank {
    #[must_use]
    pub fn new(maximum_bindings: usize, maximum_channels: usize) -> Self {
        let cell_count = maximum_bindings.saturating_mul(maximum_channels);
        let cells = (0..cell_count)
            .map(|_| PeakMeterCell {
                peak_bits: AtomicU32::new(0),
                revision: AtomicU64::new(0),
            })
            .collect();
        Self {
            cells,
            maximum_bindings,
            maximum_channels,
        }
    }

    pub(crate) fn record_interleaved(
        &self,
        revision: u64,
        binding_index: usize,
        channels: u16,
        samples: &[f32],
    ) {
        if binding_index >= self.maximum_bindings {
            return;
        }
        let channel_count = usize::from(channels).min(self.maximum_channels);
        for channel in 0..channel_count {
            let mut peak = 0.0_f32;
            for frame in samples.chunks_exact(usize::from(channels)) {
                let magnitude = frame[channel].abs();
                if magnitude.is_finite() {
                    peak = peak.max(magnitude);
                }
            }
            self.record_peak(revision, binding_index, channel, peak);
        }
    }

    fn record_peak(&self, revision: u64, binding_index: usize, channel: usize, peak: f32) {
        let Some(cell) = self.cell(binding_index, channel) else {
            return;
        };
        if cell.revision.load(Ordering::Relaxed) != revision {
            cell.peak_bits.store(0, Ordering::Relaxed);
            cell.revision.store(revision, Ordering::Release);
        }
        cell.peak_bits.fetch_max(peak.to_bits(), Ordering::Relaxed);
    }

    #[must_use]
    pub fn take_peaks(&self, revision: u64, binding_index: usize, channels: u16) -> Vec<f32> {
        (0..usize::from(channels).min(self.maximum_channels))
            .map(|channel| {
                let Some(cell) = self.cell(binding_index, channel) else {
                    return 0.0;
                };
                if cell.revision.load(Ordering::Acquire) != revision {
                    return 0.0;
                }
                f32::from_bits(cell.peak_bits.swap(0, Ordering::AcqRel))
            })
            .collect()
    }

    fn cell(&self, binding_index: usize, channel: usize) -> Option<&PeakMeterCell> {
        binding_index
            .checked_mul(self.maximum_channels)
            .and_then(|offset| offset.checked_add(channel))
            .and_then(|index| self.cells.get(index))
    }
}

pub struct ProcessorExchange {
    gate: AtomicBool,
    pending: AtomicPtr<BlockProcessor>,
    retired: AtomicPtr<BlockProcessor>,
    active_revision: AtomicU64,
    submitted_revision: AtomicU64,
}

struct ExchangeGateGuard<'a> {
    gate: &'a AtomicBool,
}

impl Drop for ExchangeGateGuard<'_> {
    fn drop(&mut self) {
        self.gate.store(false, Ordering::Release);
    }
}

impl ProcessorExchange {
    #[must_use]
    pub fn new(active_revision: u64) -> Self {
        Self {
            gate: AtomicBool::new(false),
            pending: AtomicPtr::new(std::ptr::null_mut()),
            retired: AtomicPtr::new(std::ptr::null_mut()),
            active_revision: AtomicU64::new(active_revision),
            submitted_revision: AtomicU64::new(active_revision),
        }
    }

    /// Submits a fully prepared processor from the control thread.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessorSubmitError::Busy`] when another submission or block-boundary
    /// activation owns the non-blocking exchange gate, or a processor is already pending.
    pub fn submit(&self, processor: Box<BlockProcessor>) -> Result<(), ProcessorSubmitError> {
        self.submit_configured(processor, || Ok::<(), std::convert::Infallible>(()))
            .map_err(|error| match error {
                ProcessorConfigurationError::Busy { proposed_revision } => {
                    ProcessorSubmitError::Busy { proposed_revision }
                }
                ProcessorConfigurationError::Configuration(error) => match error {},
            })
    }

    /// Runs a control-plane configuration step and publishes a prepared processor as one
    /// submission. The audio thread never waits for the configuration step: it keeps using the
    /// active processor when the exchange gate is occupied.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessorConfigurationError::Busy`] when a processor cannot be queued, or
    /// [`ProcessorConfigurationError::Configuration`] when `configure` rejects the associated
    /// control-plane state.
    pub fn submit_configured<E>(
        &self,
        processor: Box<BlockProcessor>,
        configure: impl FnOnce() -> Result<(), E>,
    ) -> Result<(), ProcessorConfigurationError<E>> {
        let proposed_revision = processor.plan().revision();
        if self
            .gate
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return Err(ProcessorConfigurationError::Busy { proposed_revision });
        }
        let gate_guard = ExchangeGateGuard { gate: &self.gate };
        self.reclaim_retired();
        if !self.pending.load(Ordering::Acquire).is_null() {
            return Err(ProcessorConfigurationError::Busy { proposed_revision });
        }
        if let Err(error) = configure() {
            return Err(ProcessorConfigurationError::Configuration(error));
        }
        self.pending
            .store(Box::into_raw(processor), Ordering::Release);
        self.submitted_revision
            .store(proposed_revision, Ordering::Release);
        drop(gate_guard);
        Ok(())
    }

    /// Activates a pending processor at an audio block boundary without allocation,
    /// deallocation, blocking, or waiting.
    pub fn activate_pending(&self, current: &mut Box<BlockProcessor>) -> bool {
        self.activate_pending_configured(current, |_| Ok::<(), std::convert::Infallible>(()))
            .unwrap_or_else(|error| match error {})
    }

    /// Control-side hint for callback-owner reconfiguration. Activation still
    /// goes through the serialized exchange; this does not consume a plan.
    #[must_use]
    pub fn has_pending(&self) -> bool {
        !self.pending.load(Ordering::Acquire).is_null()
    }

    /// Activates a pending processor and its pre-staged external configuration at one block
    /// boundary. `activate` must perform only bounded, real-time-safe work. The active revision is
    /// published only after both swaps succeed.
    ///
    /// # Errors
    ///
    /// Returns the error from `activate` without replacing the current processor. The pending
    /// processor remains queued for a later activation attempt.
    pub fn activate_pending_configured<E>(
        &self,
        current: &mut Box<BlockProcessor>,
        activate: impl FnOnce(u64) -> Result<(), E>,
    ) -> Result<bool, E> {
        if self
            .gate
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return Ok(false);
        }
        let gate_guard = ExchangeGateGuard { gate: &self.gate };
        let pending = self.pending.swap(std::ptr::null_mut(), Ordering::AcqRel);
        if pending.is_null() {
            return Ok(false);
        }
        // SAFETY: only `submit` creates pending pointers and the exchange gate grants
        // this audio thread unique ownership after removing the pointer from the slot.
        let next = unsafe { Box::from_raw(pending) };
        let revision = next.plan().revision();
        if let Err(error) = activate(revision) {
            self.pending.store(Box::into_raw(next), Ordering::Release);
            return Err(error);
        }
        let previous = std::mem::replace(current, next);
        debug_assert!(self.retired.load(Ordering::Relaxed).is_null());
        self.retired
            .store(Box::into_raw(previous), Ordering::Release);
        self.active_revision.store(revision, Ordering::Release);
        drop(gate_guard);
        Ok(true)
    }

    #[must_use]
    pub fn revisions(&self) -> ProcessorRevisions {
        ProcessorRevisions {
            active: self.active_revision.load(Ordering::Acquire),
            submitted: self.submitted_revision.load(Ordering::Acquire),
        }
    }

    /// Reclaims the processor retired by the audio thread.
    ///
    /// This must run on a control/background thread because dropping a hosted processor may stop
    /// a child process and release native plug-in resources. Calling it periodically prevents a
    /// removed plug-in from remaining alive until the next graph edit.
    pub fn reclaim_retired(&self) {
        let retired = self.retired.swap(std::ptr::null_mut(), Ordering::AcqRel);
        if !retired.is_null() {
            // SAFETY: the exchange gate prevents the audio thread from accessing this
            // retired pointer, and this control thread removed the sole stored owner.
            drop(unsafe { Box::from_raw(retired) });
        }
    }
}

impl Drop for ProcessorExchange {
    fn drop(&mut self) {
        for pointer in [*self.pending.get_mut(), *self.retired.get_mut()] {
            if !pointer.is_null() {
                // SAFETY: dropping the exchange requires exclusive access, so no thread can
                // access pointers that remain owned by its pending and retired slots.
                drop(unsafe { Box::from_raw(pointer) });
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ProcessorSubmitError {
    #[error(
        "processor revision {proposed_revision} could not be queued because the exchange is busy"
    )]
    Busy { proposed_revision: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProcessorConfigurationError<E> {
    #[error(
        "processor revision {proposed_revision} could not be queued because the exchange is busy"
    )]
    Busy { proposed_revision: u64 },
    #[error("processor control-plane configuration failed: {0}")]
    Configuration(E),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessorRevisions {
    pub active: u64,
    pub submitted: u64,
}

pub struct PlanStore {
    active: ArcSwap<ExecutionPlan>,
    writer: Mutex<()>,
}

impl PlanStore {
    #[must_use]
    pub fn new(initial: Arc<ExecutionPlan>) -> Self {
        Self {
            active: ArcSwap::from(initial),
            writer: Mutex::new(()),
        }
    }

    #[must_use]
    pub fn load(&self) -> Guard<Arc<ExecutionPlan>> {
        self.active.load()
    }

    /// Atomically installs a newer immutable execution plan.
    ///
    /// # Errors
    ///
    /// Returns [`PlanInstallError::StaleRevision`] when the proposed revision
    /// is not newer than the plan currently in use.
    pub fn install(&self, proposed: Arc<ExecutionPlan>) -> Result<(), PlanInstallError> {
        let _writer = self.writer.lock();
        let active_revision = self.active.load().revision();
        if proposed.revision() <= active_revision {
            return Err(PlanInstallError::StaleRevision {
                active: active_revision,
                proposed: proposed.revision(),
            });
        }
        self.active.store(proposed);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PlanInstallError {
    #[error("plan revision {proposed} is not newer than active revision {active}")]
    StaleRevision { active: u64, proposed: u64 },
}

#[derive(Debug, Default)]
pub struct RuntimeCounters {
    underruns: AtomicU64,
    overruns: AtomicU64,
}

impl RuntimeCounters {
    pub fn record_underrun(&self) {
        self.underruns.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_overrun(&self) {
        self.overruns.fetch_add(1, Ordering::Relaxed);
    }

    #[must_use]
    pub fn snapshot(&self) -> XrunCounters {
        XrunCounters {
            underruns: self.underruns.load(Ordering::Relaxed),
            overruns: self.overruns.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct XrunCounters {
    pub underruns: u64,
    pub overruns: u64,
}

pub struct AudioEngine {
    plans: PlanStore,
    counters: RuntimeCounters,
}

impl AudioEngine {
    #[must_use]
    pub fn new(initial_plan: Arc<ExecutionPlan>) -> Self {
        Self {
            plans: PlanStore::new(initial_plan),
            counters: RuntimeCounters::default(),
        }
    }

    #[must_use]
    pub fn active_plan(&self) -> Guard<Arc<ExecutionPlan>> {
        self.plans.load()
    }

    /// Atomically installs a newer execution plan without mutating the plan
    /// observed by an in-flight audio render block.
    ///
    /// # Errors
    ///
    /// Returns [`PlanInstallError`] when the proposed plan is stale.
    pub fn install_plan(&self, plan: Arc<ExecutionPlan>) -> Result<(), PlanInstallError> {
        self.plans.install(plan)
    }

    #[must_use]
    pub const fn counters(&self) -> &RuntimeCounters {
        &self.counters
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tonenari_audio_graph::{
        CompileContext, CompileLimits, GraphCompiler, GraphDocument, RenderFormat,
    };

    fn plan(revision: u64) -> Arc<ExecutionPlan> {
        Arc::new(
            GraphCompiler::default()
                .compile(
                    &GraphDocument::new(revision, vec![], vec![]),
                    CompileContext {
                        format: RenderFormat {
                            sample_rate: 192_000,
                            block_frames: 64,
                        },
                        limits: CompileLimits::default(),
                    },
                )
                .expect("empty graph should compile"),
        )
    }

    fn processor(revision: u64) -> BlockProcessor {
        let render = RenderPlan::prepare(&plan(revision)).expect("empty plan should lower");
        BlockProcessor::new(Arc::new(render))
    }

    #[test]
    fn installs_only_newer_immutable_plans() {
        let engine = AudioEngine::new(plan(3));

        engine
            .install_plan(plan(4))
            .expect("newer revision should install");
        assert_eq!(engine.active_plan().revision(), 4);
        assert_eq!(engine.active_plan().format().sample_rate, 192_000);

        assert_eq!(
            engine.install_plan(plan(4)),
            Err(PlanInstallError::StaleRevision {
                active: 4,
                proposed: 4,
            })
        );
        assert_eq!(
            engine.install_plan(plan(2)),
            Err(PlanInstallError::StaleRevision {
                active: 4,
                proposed: 2,
            })
        );
    }

    #[test]
    fn snapshots_lock_free_runtime_counters() {
        let engine = AudioEngine::new(plan(1));
        assert_eq!(engine.counters().snapshot(), XrunCounters::default());

        engine.counters().record_underrun();
        engine.counters().record_overrun();
        engine.counters().record_overrun();

        assert_eq!(
            engine.counters().snapshot(),
            XrunCounters {
                underruns: 1,
                overruns: 2,
            }
        );
    }

    #[test]
    fn exchanges_prepared_processors_without_blocking_the_audio_boundary() {
        let exchange = ProcessorExchange::new(1);
        let mut current = Box::new(processor(1));

        assert!(!exchange.activate_pending(&mut current));
        assert_eq!(
            exchange.revisions(),
            ProcessorRevisions {
                active: 1,
                submitted: 1,
            }
        );
        exchange
            .submit(Box::new(processor(2)))
            .expect("revision should queue");
        assert_eq!(
            exchange.submit(Box::new(processor(3))),
            Err(ProcessorSubmitError::Busy {
                proposed_revision: 3,
            })
        );
        assert!(exchange.activate_pending(&mut current));
        assert_eq!(current.plan().revision(), 2);
        assert_eq!(
            exchange.revisions(),
            ProcessorRevisions {
                active: 2,
                submitted: 2,
            }
        );

        exchange
            .submit(Box::new(processor(3)))
            .expect("submitting again should reclaim the retired processor");
        assert!(exchange.activate_pending(&mut current));
        assert_eq!(current.plan().revision(), 3);
    }

    #[test]
    fn exchange_gate_never_makes_the_audio_thread_wait() {
        let exchange = ProcessorExchange::new(1);
        let mut current = Box::new(processor(1));
        exchange.gate.store(true, Ordering::Release);

        assert_eq!(
            exchange.submit(Box::new(processor(2))),
            Err(ProcessorSubmitError::Busy {
                proposed_revision: 2,
            })
        );
        assert!(!exchange.activate_pending(&mut current));
        exchange.gate.store(false, Ordering::Release);
    }

    #[test]
    fn dropping_an_exchange_reclaims_pending_and_retired_processors() {
        let pending = ProcessorExchange::new(1);
        pending
            .submit(Box::new(processor(2)))
            .expect("processor should queue");
        drop(pending);

        let retired = ProcessorExchange::new(1);
        let mut current = Box::new(processor(1));
        retired
            .submit(Box::new(processor(2)))
            .expect("processor should queue");
        assert!(retired.activate_pending(&mut current));
        drop(retired);
    }
}
