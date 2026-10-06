use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering},
};

use arc_swap::{ArcSwap, Guard};
use mixrelay_audio_graph::ExecutionPlan;
use parking_lot::Mutex;
use thiserror::Error;

mod latency_exchange;
mod latency_plan;
mod render;
mod runtime_control;

pub use runtime_control::{RuntimeControlBank, RuntimeControlError, RuntimeControlUpdate};

pub use render::{
    BlockProcessor, CompensationState, CycleAdmission, ExternalBinding, ExternalControlState,
    ExternalIo, HostedAudioProcessor, HostedLatencySource, HostedProcessorFactory,
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
    control_revisions: Arc<runtime_control::ControlRevisionClock>,
    control_writer: Mutex<()>,
    latency: latency_exchange::LatencyExchange,
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
            control_revisions: Arc::default(),
            control_writer: Mutex::new(()),
            latency: latency_exchange::LatencyExchange::default(),
        }
    }

    /// Submits a fully prepared processor from the control thread.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessorSubmitError::Busy`] when another submission or block-boundary
    /// activation owns the non-blocking exchange gate, or a processor is already pending.
    /// Rejects stale revisions or a control bank already owned by another exchange.
    pub fn submit(&self, processor: Box<BlockProcessor>) -> Result<(), ProcessorSubmitError> {
        self.submit_configured(processor, || Ok::<(), std::convert::Infallible>(()))
            .map_err(|error| match error {
                ProcessorConfigurationError::Busy { proposed_revision } => {
                    ProcessorSubmitError::Busy { proposed_revision }
                }
                ProcessorConfigurationError::WrongExchange { proposed_revision } => {
                    ProcessorSubmitError::WrongExchange { proposed_revision }
                }
                ProcessorConfigurationError::StaleRevision { current, proposed } => {
                    ProcessorSubmitError::StaleRevision { current, proposed }
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
    /// control-plane state. Stale revisions and banks from another exchange are rejected before
    /// the configuration closure is called.
    pub fn submit_configured<E>(
        &self,
        processor: Box<BlockProcessor>,
        configure: impl FnOnce() -> Result<(), E>,
    ) -> Result<(), ProcessorConfigurationError<E>> {
        let _control_writer = self.control_writer.lock();
        let proposed_revision = processor.plan().revision();
        let current = self.revisions().submitted;
        if proposed_revision <= current {
            return Err(ProcessorConfigurationError::StaleRevision {
                current,
                proposed: proposed_revision,
            });
        }
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
        processor
            .plan()
            .controls()
            .bind_clock(&self.control_revisions)
            .map_err(|_| ProcessorConfigurationError::WrongExchange { proposed_revision })?;
        if let Err(error) = configure() {
            return Err(ProcessorConfigurationError::Configuration(error));
        }
        self.latency.set_plan(Arc::clone(processor.plan()));
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

    /// Includes only derived route-history publication, not a native topology change.
    #[must_use]
    pub fn has_pending_latency(&self) -> bool {
        self.latency.has_pending()
    }

    /// Registers the initial prepared plan before the sole audio owner starts.
    /// Later plans are registered atomically with successful topology submission.
    pub fn observe_plan(&self, plan: Arc<RenderPlan>) {
        self.latency.set_plan(plan);
    }

    /// Refreshes cached worker delay observations on a control/background thread.
    /// No native processor preparation or graph/control revision change occurs.
    /// # Errors
    /// Returns a preparation diagnostic when bounded route storage cannot be allocated.
    pub fn refresh_latency(&self) -> Result<bool, String> {
        self.latency.refresh()
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
        self.activate_pending_if_configured(current, |_| true, activate)
    }

    /// Activates only a prepared topology compatible with the current callback owner.
    /// Both closures must be bounded and real-time safe. An incompatible topology stays pending
    /// for the control thread to rebind external resources without stopping compatible edits.
    ///
    /// # Errors
    /// Returns the external activation error and leaves the current processor untouched.
    pub fn activate_pending_if_configured<E>(
        &self,
        current: &mut Box<BlockProcessor>,
        compatible: impl FnOnce(&RenderPlan) -> bool,
        activate: impl FnOnce(u64) -> Result<(), E>,
    ) -> Result<bool, E> {
        // Idle callbacks must not reserve the publication gate and spuriously
        // reject a control writer. A concurrent publication missed by this
        // acquire is simply observed at the next block; the gated swap below
        // remains the sole owner transfer and rechecks the pending slot.
        if !self.has_pending() {
            self.latency.activate(current);
            return Ok(false);
        }
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
        let mut next = unsafe { Box::from_raw(pending) };
        if !compatible(next.plan()) || !next.can_adopt_hosted(current) {
            self.pending.store(Box::into_raw(next), Ordering::Release);
            return Ok(false);
        }
        let revision = next.plan().revision();
        if let Err(error) = activate(revision) {
            self.pending.store(Box::into_raw(next), Ordering::Release);
            return Err(error);
        }
        current.set_hosted_active(false);
        next.adopt_hosted(current);
        next.retain_dynamics_state(current);
        next.retain_device_bus_state(current);
        next.set_hosted_active(true);
        let previous = std::mem::replace(current, next);
        debug_assert!(self.retired.load(Ordering::Relaxed).is_null());
        self.retired
            .store(Box::into_raw(previous), Ordering::Release);
        self.active_revision.store(revision, Ordering::Release);
        self.latency.activate(current);
        drop(gate_guard);
        Ok(true)
    }

    #[must_use]
    pub fn revisions(&self) -> ProcessorRevisions {
        let plans = self.plan_revisions();
        let active = plans
            .active
            .max(self.control_revisions.applied.load(Ordering::Acquire));
        ProcessorRevisions {
            active,
            submitted: plans
                .submitted
                .max(active)
                .max(self.control_revisions.published.load(Ordering::Acquire)),
        }
    }

    /// Revisions of prepared topologies only, for native attachments and meter layouts.
    #[must_use]
    pub fn plan_revisions(&self) -> ProcessorRevisions {
        ProcessorRevisions {
            active: self.active_revision.load(Ordering::Acquire),
            submitted: self.submitted_revision.load(Ordering::Acquire),
        }
    }

    /// Publishes graph-local controls without preparing or exchanging an execution plan.
    /// The bank must belong to the most recently submitted topology. The audio reader acknowledges
    /// its complete batch at a block boundary; publication never calls hosted/native processors.
    ///
    /// # Errors
    /// Rejects stale topology handles, invalid targets/values, or non-increasing revisions.
    pub fn publish_controls(
        &self,
        bank: &RuntimeControlBank,
        revision: u64,
        updates: &[RuntimeControlUpdate],
    ) -> Result<(), RuntimeControlError> {
        let _writer = self.control_writer.lock();
        let current = self.plan_revisions().submitted;
        if bank.plan_revision() != current {
            return Err(RuntimeControlError::StalePlan {
                current,
                proposed: bank.plan_revision(),
            });
        }
        let current = self.revisions().submitted;
        if revision <= current {
            return Err(RuntimeControlError::StaleRevision {
                current,
                proposed: revision,
            });
        }
        bank.bind_clock(&self.control_revisions)?;
        bank.publish(revision, updates)
    }

    /// Reclaims the processor retired by the audio thread.
    ///
    /// This must run on a control/background thread because dropping a hosted processor may stop
    /// a child process and release native plug-in resources. Calling it periodically prevents a
    /// removed plug-in from remaining alive until the next graph edit.
    pub fn reclaim_retired(&self) {
        self.latency.reclaim();
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
    /// Prepared control storage cannot be shared across independent execution owners.
    #[error("processor revision {proposed_revision} belongs to another exchange")]
    WrongExchange { proposed_revision: u64 },
    /// Topology revisions must follow all accepted topology and control publications.
    #[error("processor revision {proposed} is not newer than {current}")]
    StaleRevision { current: u64, proposed: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProcessorConfigurationError<E> {
    #[error(
        "processor revision {proposed_revision} could not be queued because the exchange is busy"
    )]
    Busy { proposed_revision: u64 },
    /// The proposed bank belongs to another exchange; configuration has not run.
    #[error("processor revision {proposed_revision} belongs to another exchange")]
    WrongExchange { proposed_revision: u64 },
    /// A stale topology cannot run its associated configuration side effects.
    #[error("processor revision {proposed} is not newer than {current}")]
    StaleRevision { current: u64, proposed: u64 },
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
    use mixrelay_audio_graph::{
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
    fn idle_audio_polling_does_not_reject_a_first_publication() {
        use std::sync::mpsc::sync_channel;

        // A fresh exchange per trial excludes real pending/activation conflicts.
        // Both processors are prepared before racing the native-style idle poll
        // against the one permitted control writer. Never retry a rejected plan.
        let (work_tx, work_rx) =
            sync_channel::<(Arc<ProcessorExchange>, Box<BlockProcessor>, Arc<AtomicBool>)>(0);
        let (ready_tx, ready_rx) = sync_channel(0);
        let (drained_tx, drained_rx) = sync_channel(0);
        let audio = std::thread::spawn(move || {
            for (exchange, mut current, stop) in work_rx {
                ready_tx.send(()).unwrap();
                while !stop.load(Ordering::Acquire) {
                    exchange.activate_pending(&mut current);
                }
                drained_tx.send(()).unwrap();
            }
        });
        let mut rejected = 0;
        for _ in 0..4096 {
            let exchange = Arc::new(ProcessorExchange::new(1));
            let stop = Arc::new(AtomicBool::new(false));
            let current = Box::new(processor(1));
            let next = Box::new(processor(2));
            work_tx
                .send((exchange.clone(), current, stop.clone()))
                .unwrap();
            ready_rx.recv().unwrap();
            let result = exchange.submit(next);
            stop.store(true, Ordering::Release);
            drained_rx.recv().unwrap();
            match result {
                Ok(()) => {}
                Err(ProcessorSubmitError::Busy { .. }) => rejected += 1,
                Err(error) => panic!("unexpected first-publication error: {error}"),
            }
        }
        drop(work_tx);
        audio.join().unwrap();
        assert_eq!(rejected, 0, "idle polling rejected {rejected}/4096 plans");
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

    #[test]
    fn control_revisions_follow_block_ack_without_changing_topology_or_reviving_retired_banks() {
        let exchange = ProcessorExchange::new(1);
        let mut current = Box::new(processor(1));
        let old_bank = Arc::clone(current.plan().controls());
        exchange.publish_controls(&old_bank, 2, &[]).unwrap();
        assert_eq!(
            exchange.revisions(),
            ProcessorRevisions {
                active: 1,
                submitted: 2
            }
        );
        current.process_block(1, &[], &mut []).unwrap();
        assert_eq!(
            exchange.revisions(),
            ProcessorRevisions {
                active: 2,
                submitted: 2
            }
        );
        assert_eq!(
            exchange.plan_revisions(),
            ProcessorRevisions {
                active: 1,
                submitted: 1
            }
        );
        let next = Box::new(processor(3));
        let next_bank = Arc::clone(next.plan().controls());
        exchange.submit(next).unwrap();
        assert_eq!(
            exchange.publish_controls(&old_bank, 4, &[]),
            Err(RuntimeControlError::StalePlan {
                current: 3,
                proposed: 1
            })
        );
        exchange.publish_controls(&next_bank, 4, &[]).unwrap();
        current.process_block(1, &[], &mut []).unwrap();
        assert_eq!(
            exchange.revisions(),
            ProcessorRevisions {
                active: 2,
                submitted: 4
            }
        );
        assert!(exchange.activate_pending(&mut current));
        assert_eq!(
            exchange.revisions(),
            ProcessorRevisions {
                active: 3,
                submitted: 4
            }
        );
        current.process_block(1, &[], &mut []).unwrap();
        assert_eq!(
            exchange.revisions(),
            ProcessorRevisions {
                active: 4,
                submitted: 4
            }
        );
        assert_eq!(
            exchange.plan_revisions(),
            ProcessorRevisions {
                active: 3,
                submitted: 3
            }
        );
    }

    #[test]
    fn audio_activation_does_not_lock_the_control_plane_writer() {
        let exchange = ProcessorExchange::new(1);
        let mut current = Box::new(processor(1));
        exchange.submit(Box::new(processor(2))).unwrap();
        let _writer = exchange.control_writer.lock();
        assert!(exchange.activate_pending(&mut current));
    }

    #[test]
    fn rejected_bank_owner_and_stale_topology_never_run_configuration_side_effects() {
        let first = ProcessorExchange::new(1);
        let next = processor(2);
        let shared_plan = Arc::clone(next.plan());
        first.submit(Box::new(next)).unwrap();
        let second = ProcessorExchange::new(1);
        assert_eq!(
            second.submit_configured(
                Box::new(BlockProcessor::new(shared_plan)),
                || -> Result<(), ()> {
                    panic!("wrong-owner configuration must never run");
                }
            ),
            Err(ProcessorConfigurationError::WrongExchange {
                proposed_revision: 2
            })
        );

        let bank = processor(1);
        let exchange = ProcessorExchange::new(1);
        exchange
            .publish_controls(bank.plan().controls(), 5, &[])
            .unwrap();
        assert_eq!(
            exchange.submit_configured(Box::new(processor(2)), || -> Result<(), ()> {
                panic!("stale topology configuration must never run");
            }),
            Err(ProcessorConfigurationError::StaleRevision {
                current: 5,
                proposed: 2
            })
        );
    }
}
