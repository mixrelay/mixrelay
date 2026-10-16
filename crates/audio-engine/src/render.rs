use std::{collections::BTreeMap, fmt, sync::Arc};

use mixrelay_audio_graph::{
    AUDIO_PORT_ID, AudioEndpoint, CompiledNode, ExecutionPlan, FROM_OS_PORT_ID,
    MAX_PROVIDER_BUSES_PER_DIRECTION, MAX_PROVIDER_CHANNELS_PER_DIRECTION, NodeConfig, NodeId,
    PortDirection, PortId, ProviderBus, ProviderBusKind, ProviderNodeConfig, ProviderPorts,
    RenderFormat, TO_OS_PORT_ID,
};
use mixrelay_audio_io::VirtualEndpointId;
use mixrelay_audio_io::pcm::sanitize_samples;
use mixrelay_audio_io::process::{ProcessCycle, ProcessReport, SampleFaults, TransferOutcome};
use mixrelay_audio_recorder::{RecorderRegistry, RecorderTap};
use thiserror::Error;

use crate::runtime_control::{ControlValue, RuntimeControlBank};
mod buffers;
mod compensation;
pub use compensation::ArrivalError;
mod effects;
mod history;
mod input_timeline;
mod kernels;
mod route_delay;
mod routing;
mod schedule;
use buffers::port_pair;
pub(crate) use compensation::DelayBank;
pub use compensation::{CompensationState, HostedLatencySource};
use kernels::{KernelResources, apply_kernel};
use routing::{RouteOp, apply_routes};
use schedule::RenderSchedule;

/// Stable transport identity for a recorder's channel geometry. An inactive
/// replacement can coexist with the previous plan until publication or rollback.
#[must_use]
pub fn recording_session_key(node: &NodeId, channels: u16) -> String {
    format!("{}#recorder-channels={channels}", node.as_str())
}

/// A fully prepared third-party processor owned by the audio worker.
///
/// Implementations must reuse storage and must not wait on the UI or control plane from
/// `process`. A processor without a usable output requests the graph's channel-preserving
/// fallback. Buffered processors must supply their own latency-aligned fallback instead.
pub trait HostedAudioProcessor: Send {
    /// Renders `frames` of interleaved input into the corresponding interleaved output.
    /// Returns `true` when output is complete, including intentional silence or an aligned
    /// dry fallback; `false` requests the graph's immediate dry fallback. A complete output
    /// does not mean the native plug-in is healthy; health is observed separately.
    fn process(&mut self, input: &[f32], output: &mut [f32], frames: usize) -> bool;

    /// Discards buffered audio history while this node is OFF.
    ///
    /// Called from the audio thread and may be called on every disabled callback. Implementations
    /// must not allocate, lock, block, or call native plug-in code. This suspends transport history
    /// only; it does not transfer ownership or change graph state. The default is appropriate for
    /// processors without buffered transport.
    fn suspend_processing(&mut self) {}

    /// Process-unique instance identity, never reused during this engine lifetime.
    fn reuse_identity(&self) -> Option<u64> {
        None
    }

    /// An inactive ownership-transfer request, not a renderable processor.
    fn requests_reuse(&self) -> bool {
        false
    }

    /// Publishes ownership only; must not allocate, lock, or call native code.
    fn set_active(&mut self, _active: bool) {}

    /// Returns a retained cached-latency reader for this exact prepared worker.
    /// Called only during control-thread preparation, never while rendering.
    /// Missing readers mean unknown latency, not a reported zero.
    fn latency_source(&self) -> Option<Arc<dyn HostedLatencySource>> {
        None
    }
}

/// Immutable preparation data passed across the engine/provider boundary.
pub struct HostedProcessorPrepareRequest<'a> {
    pub node_id: &'a NodeId,
    pub config: &'a ProviderNodeConfig,
    pub format: RenderFormat,
    pub input_channels: u16,
    pub output_channels: u16,
}

/// Creates hosted processors while a graph is prepared on the control thread.
pub trait HostedProcessorFactory: Send + Sync {
    /// Loads and configures one provider node for the current render format.
    ///
    /// # Errors
    ///
    /// Returns a stable diagnostic when the provider is missing or cannot be prepared.
    fn prepare(
        &self,
        request: HostedProcessorPrepareRequest<'_>,
    ) -> Result<Box<dyn HostedAudioProcessor>, String>;

    /// Retains an already loaded inactive node during an unrelated topology edit.
    /// Never instantiates a new provider here: `None` keeps an initially OFF node unprepared.
    ///
    /// # Errors
    /// Returns a diagnostic if an existing reusable owner cannot be retained safely.
    fn prepare_inactive(
        &self,
        _request: HostedProcessorPrepareRequest<'_>,
    ) -> Result<Option<Box<dyn HostedAudioProcessor>>, String> {
        Ok(None)
    }
}

type HostedProcessors = Box<[Box<dyn HostedAudioProcessor>]>;
type PreparedRenderPlan = (RenderPlan, HostedProcessors);

#[cfg(test)]
#[path = "hosted_reuse_tests.rs"]
mod hosted_reuse_tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalBinding {
    node_id: NodeId,
    kind: mixrelay_audio_graph::NodeKind,
    port_id: PortId,
    channels: u16,
    virtual_endpoint_id: Option<VirtualEndpointId>,
}

#[derive(Debug, Clone)]
pub struct MeterBinding {
    node_id: NodeId,
    port_id: PortId,
    channels: u16,
    slot: PortSlot,
    dynamics: Option<DynamicsMeterBinding>,
}

#[derive(Debug, Clone, Copy)]
struct DynamicsMeterBinding {
    input: PortSlot,
    state_index: usize,
    threshold_db: f32,
}

impl MeterBinding {
    #[must_use]
    pub const fn has_dynamics(&self) -> bool {
        self.dynamics.is_some()
    }
    #[must_use]
    pub const fn node_id(&self) -> &NodeId {
        &self.node_id
    }

    #[must_use]
    pub const fn port_id(&self) -> &PortId {
        &self.port_id
    }

    #[must_use]
    pub const fn channels(&self) -> u16 {
        self.channels
    }
}

impl ExternalBinding {
    #[must_use]
    pub const fn kind(&self) -> mixrelay_audio_graph::NodeKind {
        self.kind
    }

    /// Canonical device identity from the node configuration, independent of
    /// graph-local node names. Hardware and application bindings return `None`.
    #[must_use]
    pub const fn virtual_endpoint_id(&self) -> Option<&VirtualEndpointId> {
        self.virtual_endpoint_id.as_ref()
    }

    #[must_use]
    pub const fn node_id(&self) -> &NodeId {
        &self.node_id
    }

    #[must_use]
    pub const fn port_id(&self) -> &PortId {
        &self.port_id
    }

    #[must_use]
    pub const fn channels(&self) -> u16 {
        self.channels
    }
}

#[derive(Debug)]
pub struct RenderPlan {
    revision: u64,
    format: RenderFormat,
    steps: Box<[NodeStep]>,
    schedule: RenderSchedule,
    sink_routes: Box<[RouteOp]>,
    source_bindings: Box<[ExternalBinding]>,
    sink_bindings: Box<[ExternalBinding]>,
    meter_bindings: Box<[MeterBinding]>,
    recorder_bindings: Box<[MeterBinding]>,
    arena_samples: usize,
    /// Every port slot, so a block shorter than the prepared quantum clears only
    /// the frames it renders instead of the whole arena.
    arena_slots: Box<[PortSlot]>,
    dynamics_state_count: usize,
    dynamics_bindings: Box<[DynamicsBinding]>,
    effects: Box<[effects::Binding]>,
    device_bus_samples: usize,
    device_bus_bindings: Box<[DeviceBusBinding]>,
    history: Box<[history::Binding]>,
    source_history: Box<[history::Targets]>,
    hosted_input_scratch_samples: usize,
    hosted_scratch_samples: usize,
    controls: Arc<RuntimeControlBank>,
    source_controls: Box<[usize]>,
    sink_controls: Box<[usize]>,
    recorder_controls: Box<[usize]>,
    compensation: compensation::CompensationTopology,
}

impl RenderPlan {
    /// Lowers a validated graph plan into numeric buffer offsets and render operations.
    ///
    /// # Errors
    ///
    /// Returns [`RenderPlanError`] when a custom resolver produced ports that do not match
    /// the node contract or the render arena size overflows.
    pub fn prepare(execution: &ExecutionPlan) -> Result<Self, RenderPlanError> {
        let (plan, processors) = Self::prepare_internal(execution, None)?;
        debug_assert!(processors.is_empty());
        Ok(plan)
    }

    #[allow(clippy::too_many_lines)] // Ordered control-thread preparation of independent bounded resources.
    fn prepare_internal(
        execution: &ExecutionPlan,
        hosted_factory: Option<&dyn HostedProcessorFactory>,
    ) -> Result<PreparedRenderPlan, RenderPlanError> {
        let block_frames = execution.format().block_frames as usize;
        let (slots, arena_samples) = allocate_slots(execution.nodes(), block_frames)?;
        let mut routes = lower_routes(execution, &slots);
        let source_bindings = collect_external_bindings(execution, &slots, PortDirection::Output)?;
        let sink_bindings = collect_external_bindings(execution, &slots, PortDirection::Input)?;
        let meter_bindings = collect_meter_bindings(execution, &slots)?;
        let recorder_bindings = collect_recorder_bindings(execution, &slots)?;
        let mut steps = Vec::with_capacity(execution.nodes().len());
        let mut prepared = Vec::with_capacity(execution.nodes().len());
        let mut dynamics_state_count = 0;
        let mut effect_count = 0;
        let effects = prepare_effects(execution)?;
        let mut device_bus_samples = 0;
        let mut hosted_input_scratch_samples = 0;
        let mut hosted_output_scratch_samples = 0;
        let mut hosted_processors = Vec::new();

        let context = LoweringContext {
            slots: &slots,
            source_bindings: &source_bindings,
            sink_bindings: &sink_bindings,
            format: execution.format(),
            hosted_factory,
        };

        for node in execution.nodes() {
            let incoming_routes = routes.incoming.remove(node.id()).unwrap_or_default();
            let mut state = LoweringState {
                dynamics_state_count: &mut dynamics_state_count,
                effect_count: &mut effect_count,
                device_bus_samples: &mut device_bus_samples,
                hosted_input_scratch_samples: &mut hosted_input_scratch_samples,
                hosted_output_scratch_samples: &mut hosted_output_scratch_samples,
                hosted_processors: &mut hosted_processors,
            };
            let kernels = lower_kernels(node, &context, &mut state)?;
            prepared.push(
                !matches!(node.config(), NodeConfig::Provider(_)) || !kernels.processing.is_empty(),
            );
            steps.push(NodeStep::new(node, kernels, incoming_routes, &slots)?);
        }

        let controls = Arc::new(RuntimeControlBank::new(execution, &prepared));
        let source_controls =
            binding_controls(&controls, source_bindings.iter().map(|b| &b.node_id));
        let sink_controls = binding_controls(&controls, sink_bindings.iter().map(|b| &b.node_id));
        let recorder_controls =
            binding_controls(&controls, recorder_bindings.iter().map(|b| &b.node_id));
        let device_bus_bindings = device_bus_bindings(execution, &slots)?;
        let dynamics_bindings = dynamics_bindings(execution);
        let history = history::bindings(execution, &effects, &dynamics_bindings);
        let source_history =
            history::prepare(execution, &source_bindings, &device_bus_bindings, &history);
        let hosted_scratch_samples = hosted_input_scratch_samples
            .checked_add(hosted_output_scratch_samples)
            .ok_or(RenderPlanError::BufferSizeOverflow)?;

        Ok((
            Self {
                revision: execution.revision(),
                format: execution.format(),
                schedule: RenderSchedule::new(&steps),
                compensation: compensation::CompensationTopology::new(
                    execution,
                    &steps,
                    &hosted_processors,
                ),
                steps: steps.into_boxed_slice(),
                sink_routes: routes.sinks.into_boxed_slice(),
                recorder_bindings: recorder_bindings.into_boxed_slice(),
                source_bindings: source_bindings.into_boxed_slice(),
                sink_bindings: sink_bindings.into_boxed_slice(),
                meter_bindings: meter_bindings.into_boxed_slice(),
                arena_samples,
                arena_slots: slots.values().copied().collect(),
                dynamics_state_count,
                dynamics_bindings,
                effects,
                device_bus_samples,
                device_bus_bindings,
                history,
                source_history,
                hosted_input_scratch_samples,
                hosted_scratch_samples,
                controls,
                source_controls,
                sink_controls,
                recorder_controls,
            },
            hosted_processors.into_boxed_slice(),
        ))
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Stable control handles for this prepared topology, shared with its audio processor.
    #[must_use]
    pub const fn controls(&self) -> &Arc<RuntimeControlBank> {
        &self.controls
    }

    #[must_use]
    pub const fn format(&self) -> RenderFormat {
        self.format
    }

    #[must_use]
    pub fn source_bindings(&self) -> &[ExternalBinding] {
        &self.source_bindings
    }

    #[must_use]
    pub fn sink_bindings(&self) -> &[ExternalBinding] {
        &self.sink_bindings
    }

    #[must_use]
    pub fn meter_bindings(&self) -> &[MeterBinding] {
        &self.meter_bindings
    }

    #[must_use]
    pub const fn arena_samples(&self) -> usize {
        self.arena_samples
    }

    /// Computes terminal arrivals including processing and inserted compensation on control.
    /// Supply measured causal source delays in source-binding order, in graph-rate frames.
    /// Unresolved sources propagate `None` only to destinations they feed.
    /// # Errors
    /// Returns invalid source geometry or an unrepresentable accumulated delay.
    pub fn destination_arrivals(
        &self,
        sources: &[Option<u32>],
    ) -> Result<Vec<Option<u32>>, String> {
        self.compensation
            .destination_arrivals(&self.source_bindings, sources, &self.sink_bindings)
    }

    /// Samples cached worker delays on the control thread, retaining prepared owner order.
    pub(crate) fn latency_observations(&self) -> Vec<Option<u32>> {
        self.compensation.observations()
    }

    /// Control-side causal source binding indexes for each destination binding.
    #[must_use]
    pub fn destination_source_indexes(&self) -> Vec<Vec<usize>> {
        self.compensation.destination_source_indexes(self)
    }

    /// Control-side content arrivals in nanoseconds under existing route compensation.
    /// Unlike integer source-frame observations, fractional measured roots are retained.
    /// Contributors must agree within the supplied nanosecond threshold.
    ///
    /// # Errors
    /// Rejects invalid source counts, rate or an invalid compensation plan, and reports
    /// reads that kept overlapping the audio thread's acknowledgement as contended.
    pub fn measured_destination_arrivals(
        &self,
        sources_ns: &[Option<f64>],
        threshold_ns: f64,
    ) -> Result<Vec<Option<f64>>, ArrivalError> {
        self.compensation
            .measured_destination_arrivals(self, sources_ns, threshold_ns)
    }

    /// Compares cached worker delays without allocating or querying native plug-ins.
    pub(crate) fn latency_matches(&self, expected: &[Option<u32>]) -> bool {
        self.compensation.matches(expected)
    }
}

pub struct BlockProcessor {
    recorders: Box<[RecorderTap]>,
    cycle_tracker: mixrelay_audio_io::process::CycleTracker,
    plan: Arc<RenderPlan>,
    arena: Box<[f32]>,
    source_buffers: ExternalBuffers,
    sink_buffers: ExternalBuffers,
    source_latency_faults: Box<[bool]>,
    source_timelines: Box<[input_timeline::State]>,
    sink_latency_faults: Box<[bool]>,
    sink_latency_routes: Vec<Vec<usize>>,
    meters: Option<Arc<crate::PeakMeterBank>>,
    dynamics_envelopes: Box<[f32]>,
    dynamics_minimum_gains: Box<[f32]>,
    dynamics_wet: Box<[f32]>,
    effect_states: Box<[effects::State]>,
    device_bus_state: Box<[f32]>,
    device_bus_cursor: usize,
    hosted_scratch: Box<[f32]>,
    hosted_input_scratch_samples: usize,
    hosted_processors: Box<[Box<dyn HostedAudioProcessor>]>,
    hosted_identities: Box<[(u64, usize)]>,
    controls: Box<[ControlValue]>,
    control_scratch: Box<[ControlValue]>,
    sampled_control_epoch: Option<u64>,
    delay_bank: Option<Box<DelayBank>>,
}

impl fmt::Debug for BlockProcessor {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Third-party processor trait objects deliberately do not expose implementation details.
        // Reporting their count still keeps exchange diagnostics useful without expanding the
        // real-time interface solely for formatting.
        formatter
            .debug_struct("BlockProcessor")
            .field("recorders", &self.recorders.len())
            .field("cycle_tracker", &self.cycle_tracker)
            .field("plan", &self.plan)
            .field("arena_samples", &self.arena.len())
            .field("source_buffers", &self.source_buffers.len())
            .field("sink_buffers", &self.sink_buffers.len())
            .field("source_latency_faults", &self.source_latency_faults)
            .field("source_timelines", &self.source_timelines)
            .field("sink_latency_faults", &self.sink_latency_faults)
            .field("sink_latency_routes", &self.sink_latency_routes)
            .field("has_meters", &self.meters.is_some())
            .field("dynamics_envelopes", &self.dynamics_envelopes.len())
            .field("dynamics_minimum_gains", &self.dynamics_minimum_gains.len())
            .field("dynamics_wet", &self.dynamics_wet.len())
            .field("effect_states", &self.effect_states.len())
            .field("device_bus_state", &self.device_bus_state.len())
            .field("device_bus_cursor", &self.device_bus_cursor)
            .field("hosted_scratch", &self.hosted_scratch.len())
            .field(
                "hosted_input_scratch_samples",
                &self.hosted_input_scratch_samples,
            )
            .field("hosted_processors", &self.hosted_processors.len())
            .field("hosted_identities", &self.hosted_identities)
            .field("controls", &self.controls)
            .field("control_scratch", &self.control_scratch)
            .field("sampled_control_epoch", &self.sampled_control_epoch)
            .field(
                "delay_bank",
                &self.delay_bank.as_ref().map(|bank| bank.state()),
            )
            .finish()
    }
}

/// Borrowed source/sink gates from exactly the control snapshot used for this audio block.
/// Adapters may invalidate a stopped transport cursor before reading resumed PCM. No bank lookup,
/// independent atomic snapshot, allocation, or lock is required at this boundary.
#[derive(Debug, Clone, Copy)]
pub struct ExternalControlState<'a> {
    plan: &'a RenderPlan,
    controls: &'a [ControlValue],
}

impl ExternalControlState<'_> {
    /// Returns the gate for a source-binding index, or `None` outside the prepared layout.
    #[must_use]
    pub fn source_enabled(&self, index: usize) -> Option<bool> {
        self.plan
            .source_controls
            .get(index)
            .map(|&control| self.controls[control].control.enabled)
    }

    /// Returns the gate for a sink-binding index, or `None` outside the prepared layout.
    #[must_use]
    pub fn sink_enabled(&self, index: usize) -> Option<bool> {
        self.plan
            .sink_controls
            .get(index)
            .map(|&control| self.controls[control].control.enabled)
    }

    /// Iterates prepared source identities and their block-coherent execution gates.
    #[must_use]
    pub fn sources(&self) -> impl ExactSizeIterator<Item = (&ExternalBinding, bool)> {
        self.plan
            .source_bindings
            .iter()
            .zip(&self.plan.source_controls)
            .map(|(binding, &index)| (binding, self.controls[index].control.enabled))
    }

    /// Iterates prepared sink identities and their block-coherent execution gates.
    #[must_use]
    pub fn sinks(&self) -> impl ExactSizeIterator<Item = (&ExternalBinding, bool)> {
        self.plan
            .sink_bindings
            .iter()
            .zip(&self.plan.sink_controls)
            .map(|(binding, &index)| (binding, self.controls[index].control.enabled))
    }
}

/// Whether native transport capacity permits this validated quantum to consume PCM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CycleAdmission {
    /// The adapter has installed its gates and reserved the required transport capacity.
    Process,
    /// Only the pacing owner changed: native PCM queues remained continuous.
    /// Validate the new clock coordinates but retain bus/delay history. Source-local
    /// transfer discontinuities still invalidate their affected histories normally.
    ClockHandoff,
    /// Gates were observed, but transport capacity is unavailable. Retry without advancing time.
    Retry,
}

pub trait ExternalIo {
    /// Observes the same block-boundary gates the processor uses before any external transfer.
    /// Implementations may reset transport cursors on OFF transitions, but must not allocate,
    /// block, or query a separately published control snapshot. Return `Retry` only after gate
    /// changes have been observed, before consuming PCM or running conversion/DSP work.
    fn begin_cycle(
        &mut self,
        _cycle: &ProcessCycle,
        _controls: ExternalControlState<'_>,
    ) -> CycleAdmission {
        CycleAdmission::Process
    }

    /// Optional source-session identity, observed before and after each PCM read.
    /// `None` leaves discontinuity tracking with the adapter. `Some(0)` means
    /// the source is unavailable; nonzero epochs must never be reused. Reads
    /// spanning an epoch change are rejected. This is not a content timestamp.
    /// Implementations must use bounded, nonblocking snapshots only.
    fn source_epoch(&self, _binding: &ExternalBinding) -> Option<u64> {
        None
    }

    /// Native I/O must consume this cycle's timeline, not infer a second clock
    /// from the destination capacity. Never block or allocate at this boundary.
    /// The destination arrives zeroed, so an adapter fills only what it reads and
    /// need not clear it again.
    fn read_source(
        &mut self,
        binding: &ExternalBinding,
        destination: &mut [f32],
        cycle: &ProcessCycle,
    ) -> TransferOutcome;

    fn write_sink(
        &mut self,
        binding: &ExternalBinding,
        source: &[f32],
        cycle: &ProcessCycle,
    ) -> TransferOutcome;
}

impl BlockProcessor {
    #[must_use]
    /// # Panics
    /// Panics if built-in storage cannot be prepared. Runtime publication uses `try_new`.
    pub fn new(plan: Arc<RenderPlan>) -> Self {
        Self::try_new(plan).expect("built-in effect storage must be prepared before rendering")
    }

    /// # Errors
    /// Returns a storage reservation error before any audio ownership is published.
    pub fn try_new(plan: Arc<RenderPlan>) -> Result<Self, RenderPlanError> {
        let effect_states = plan
            .effects
            .iter()
            .map(|binding| effects::State::new(&binding.design))
            .collect::<Result<Vec<_>, _>>()
            .map_err(RenderPlanError::BuiltinPreparation)?
            .into_boxed_slice();
        let hosted_input_scratch_samples = plan.hosted_input_scratch_samples;
        let hosted_scratch_samples = plan.hosted_scratch_samples;
        let arena = buffers::filled(plan.arena_samples, 0.0)?;
        let source_buffers = allocate_external_buffers(&plan.source_bindings, plan.format)?;
        let sink_buffers = allocate_external_buffers(&plan.sink_bindings, plan.format)?;
        let dynamics_envelopes = buffers::filled(plan.dynamics_state_count, 0.0)?;
        let dynamics_minimum_gains = buffers::filled(plan.dynamics_state_count, 1.0)?;
        let mut dynamics_wet = buffers::filled(plan.dynamics_state_count, 1.0)?;
        for binding in &plan.dynamics_bindings {
            dynamics_wet[binding.state_index] = match &binding.config {
                NodeConfig::Compressor(config) => f32::from(!config.bypass),
                NodeConfig::Limiter(config) => f32::from(!config.bypass),
                _ => unreachable!("only dynamics nodes own dynamics state"),
            };
        }
        let device_bus_state = buffers::filled(plan.device_bus_samples, 0.0)?;
        let controls = plan.controls.values();
        let control_scratch = controls.clone();
        let source_timelines =
            vec![input_timeline::State::default(); plan.source_bindings.len()].into_boxed_slice();
        let source_latency_faults = vec![false; plan.source_bindings.len()].into_boxed_slice();
        let sink_latency_faults = vec![false; plan.sink_bindings.len()].into_boxed_slice();
        let sink_latency_routes = plan
            .sink_bindings
            .iter()
            .map(|binding| plan.compensation.binding_routes(binding.node_id()))
            .collect();
        Ok(Self {
            source_timelines,
            source_latency_faults,
            sink_latency_faults,
            sink_latency_routes,
            recorders: Box::new([]),
            cycle_tracker: mixrelay_audio_io::process::CycleTracker::default(),
            plan,
            arena,
            source_buffers,
            sink_buffers,
            meters: None,
            dynamics_envelopes,
            dynamics_minimum_gains,
            dynamics_wet,
            effect_states,
            device_bus_state,
            device_bus_cursor: 0,
            hosted_scratch: buffers::filled(hosted_scratch_samples, 0.0)?,
            hosted_input_scratch_samples,
            hosted_processors: Box::new([]),
            hosted_identities: Box::new([]),
            controls,
            control_scratch,
            sampled_control_epoch: None,
            delay_bank: None,
        })
    }

    #[must_use]
    pub fn with_peak_meters(plan: Arc<RenderPlan>, meters: Arc<crate::PeakMeterBank>) -> Self {
        let mut processor = Self::new(plan);
        processor.meters = Some(meters);
        processor
    }

    /// Prepares a graph containing provider nodes and installs their already-configured
    /// processor instances without doing plug-in work on the audio thread.
    ///
    /// # Errors
    ///
    /// Returns [`RenderPlanError`] when lowering or provider preparation fails.
    pub fn prepare_with_hosted_processors(
        execution: &ExecutionPlan,
        meters: Arc<crate::PeakMeterBank>,
        factory: &dyn HostedProcessorFactory,
    ) -> Result<Self, RenderPlanError> {
        let (plan, hosted_processors) = RenderPlan::prepare_internal(execution, Some(factory))?;
        let mut processor = Self::try_new(Arc::new(plan))?;
        processor.meters = Some(meters);
        processor.hosted_processors = hosted_processors;
        let mut identities: Vec<_> = processor
            .hosted_processors
            .iter()
            .enumerate()
            .filter_map(|(index, hosted)| hosted.reuse_identity().map(|id| (id, index)))
            .collect();
        identities.sort_unstable();
        processor.hosted_identities = identities.into_boxed_slice();
        processor.delay_bank = Some(Box::new(
            DelayBank::prepare(
                Arc::clone(processor.plan()),
                processor.plan().latency_observations(),
            )
            .map_err(RenderPlanError::CompensationPreparation)?,
        ));
        Ok(processor)
    }

    /// Transfers only route history; the caller retires the returned owner off the audio thread.
    pub(crate) fn replace_delay_bank(&mut self, bank: Box<DelayBank>) -> Option<Box<DelayBank>> {
        // The observer's first publication may repeat the bank prepared with this topology.
        // Retire that duplicate, not the warmed-up active history: otherwise startup causes
        // a second silence window after the first successful audio blocks.
        if self
            .delay_bank
            .as_ref()
            .is_some_and(|active| active.equivalent(&bank))
        {
            return Some(bank);
        }
        self.delay_bank.replace(bank)
    }

    /// Status of the derived alignment; unknown latency is never represented as known zero.
    #[must_use]
    pub fn compensation_state(&self) -> CompensationState {
        self.delay_bank
            .as_ref()
            .map_or(CompensationState::Unprepared, |bank| bank.state())
    }

    /// Audio-owner acknowledgement of the exact installed history observations.
    /// Returns false while compensation is pending, unprepared or priming.
    /// Atomic publication only; call after the processed cycle.
    #[must_use]
    pub fn acknowledge_content_latency(&self) -> bool {
        self.delay_bank
            .as_ref()
            .is_some_and(|bank| bank.acknowledge_content_latency())
    }

    /// Block-local transfer and history readiness for one external path binding.
    /// The reporter combines only the source/sink bindings contributing to its destination.
    #[must_use]
    pub fn binding_content_latency_ready(&self, id: &str, input: bool) -> bool {
        let (bindings, faults, routes): (_, _, &[Vec<usize>]) = if input {
            (
                &self.plan.source_bindings,
                &self.source_latency_faults,
                &[][..],
            )
        } else {
            (
                &self.plan.sink_bindings,
                &self.sink_latency_faults,
                &self.sink_latency_routes,
            )
        };
        bindings
            .iter()
            .enumerate()
            .filter(|(_, binding)| {
                id == binding.node_id().as_str()
                    || id.strip_suffix(if input { "/capture" } else { "/render" })
                        == Some(binding.node_id().as_str())
            })
            .any(|(index, _)| {
                !faults[index]
                    && self.delay_bank.as_ref().is_some_and(|bank| {
                        bank.content_ready_for_routes(if input { &[] } else { &routes[index] })
                    })
            })
    }

    pub(crate) fn can_adopt_hosted(&self, previous: &Self) -> bool {
        self.hosted_identities.iter().all(|&(identity, index)| {
            !self.hosted_processors[index].requests_reuse()
                || previous
                    .hosted_identities
                    .binary_search_by_key(&identity, |&(id, _)| id)
                    .is_ok_and(|found| {
                        !previous.hosted_processors[previous.hosted_identities[found].1]
                            .requests_reuse()
                    })
        })
    }

    pub(crate) fn set_hosted_active(&mut self, active: bool) {
        for processor in &mut self.hosted_processors {
            processor.set_active(active);
        }
    }

    pub(crate) fn retain_history(&mut self, previous: &mut Self) {
        history::retain(self, previous);
    }

    pub(crate) fn retain_device_bus_state(&mut self, previous: &Self) {
        if self.plan.format != previous.plan.format {
            return;
        }
        self.device_bus_cursor = previous.device_bus_cursor;
        for binding in &self.plan.device_bus_bindings {
            let Ok(index) = previous
                .plan
                .device_bus_bindings
                .binary_search_by(|candidate| candidate.node_id.cmp(&binding.node_id))
            else {
                continue;
            };
            let source = &previous.plan.device_bus_bindings[index];
            if source.samples.len() == binding.samples.len()
                && source.input_path == binding.input_path
            {
                self.device_bus_state[binding.samples.clone()]
                    .copy_from_slice(&previous.device_bus_state[source.samples.clone()]);
            }
        }
    }

    /// Moves complete processor ownership at activation; no allocation, lock, native call,
    /// or worker destruction occurs on the audio thread. Retired placeholders drop later.
    pub(crate) fn adopt_hosted(&mut self, previous: &mut Self) {
        for &(identity, index) in &self.hosted_identities {
            if self.hosted_processors[index].requests_reuse() {
                let found = previous
                    .hosted_identities
                    .binary_search_by_key(&identity, |&(id, _)| id)
                    .expect("reuse ownership was validated before activation");
                let previous_index = previous.hosted_identities[found].1;
                std::mem::swap(
                    &mut self.hosted_processors[index],
                    &mut previous.hosted_processors[previous_index],
                );
            }
        }
    }

    /// Attaches app-owned recording sessions while preparing an inactive processor.
    /// File creation and recording transitions are explicit control-plane operations.
    /// # Errors
    /// Returns an error if a surviving recorder's channel contract cannot be retained.
    pub fn attach_recorders(&mut self, registry: &RecorderRegistry) -> Result<(), String> {
        self.recorders = self
            .plan
            .recorder_bindings
            .iter()
            .map(|binding| {
                registry.prepare(
                    &recording_session_key(&binding.node_id, binding.channels),
                    binding.channels,
                )
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_boxed_slice();
        Ok(())
    }

    #[must_use]
    pub const fn plan(&self) -> &Arc<RenderPlan> {
        &self.plan
    }

    /// Processes one interleaved audio block without allocating or locking.
    ///
    /// Inputs and outputs must follow the order exposed by [`RenderPlan::source_bindings`]
    /// and [`RenderPlan::sink_bindings`].
    ///
    /// # Errors
    ///
    /// Returns [`ProcessBlockError`] before mutating output buffers when the frame count,
    /// binding count, or supplied buffer lengths do not match the prepared plan.
    pub fn process_block(
        &mut self,
        frames: u32,
        sources: &[&[f32]],
        sinks: &mut [&mut [f32]],
    ) -> Result<(), ProcessBlockError> {
        let frames = self.validate_buffers(frames, sources, sinks)?;
        self.sample_controls();
        for (binding, (source, destination)) in self
            .plan
            .source_bindings
            .iter()
            .zip(sources.iter().zip(&mut self.source_buffers))
        {
            let samples = sample_count(frames, binding.channels);
            destination[..samples].copy_from_slice(&source[..samples]);
        }
        self.process_loaded_block(frames);
        for (binding, (source, destination)) in self
            .plan
            .sink_bindings
            .iter()
            .zip(self.sink_buffers.iter().zip(sinks))
        {
            let samples = sample_count(frames, binding.channels);
            destination[..samples].copy_from_slice(&source[..samples]);
        }
        Ok(())
    }

    /// Reads external sources, processes one block, and writes external sinks without allocating.
    /// Returns `None` when native admission requests a retry: controls and adapter gates are
    /// observed, but no PCM, DSP, recording, meters, or cycle-tracker position are advanced.
    ///
    /// # Errors
    ///
    /// Returns [`ProcessBlockError`] for an invalid clock/format/quantum or a
    /// native adapter reporting an impossible transfer size. Contract failures
    /// stop the owning processing session; they are not retryable audio blocks.
    pub fn process_external_cycle(
        &mut self,
        cycle: ProcessCycle,
        external: &mut impl ExternalIo,
    ) -> Result<Option<ProcessReport>, ProcessBlockError> {
        let Some(reset_history) = self.admit_external_cycle(cycle, external)? else {
            return Ok(None);
        };
        let frames = cycle.frames;
        let frames_usize = frames as usize;
        let mut report = self.read_external_sources(cycle, external)?;
        // An individual input invalidates only its downstream histories above.
        // A global clock loss invalidates the complete processing timeline.
        if reset_history {
            for binding in &self.plan.history {
                binding
                    .state
                    .reset(&mut self.effect_states, &mut self.dynamics_envelopes);
            }
            self.device_bus_state.fill(0.0);
            self.device_bus_cursor = 0;
            if let Some(bank) = &mut self.delay_bank {
                bank.reset();
            }
        }
        let external_source_faults = report.sample_faults.source_samples;
        report.sample_faults = self.process_loaded_block(frames_usize);
        report.sample_faults.source_samples = report
            .sample_faults
            .source_samples
            .saturating_add(external_source_faults);
        if external_source_faults != 0 {
            report.sample_faults.blocks = 1;
        }
        for (sink_index, ((binding, source), &control_index)) in self
            .plan
            .sink_bindings
            .iter()
            .zip(&self.sink_buffers)
            .zip(&self.plan.sink_controls)
            .enumerate()
        {
            if !self.controls[control_index].control.enabled {
                continue;
            }
            let samples = sample_count(frames_usize, binding.channels);
            let transfer = external.write_sink(binding, &source[..samples], &cycle);
            if transfer.frames > frames {
                return Err(ProcessBlockError::InvalidTransfer);
            }
            self.sink_latency_faults[sink_index] =
                transfer.discontinuity || transfer.frames < frames;
            report.output_overflows += u64::from(transfer.frames < frames);
            report.discontinuities += u64::from(transfer.discontinuity);
        }
        Ok(Some(report))
    }

    fn read_external_sources(
        &mut self,
        cycle: ProcessCycle,
        external: &mut impl ExternalIo,
    ) -> Result<ProcessReport, ProcessBlockError> {
        let frames = cycle.frames;
        let frames_usize = frames as usize;
        let mut report = ProcessReport::default();
        self.source_latency_faults.fill(false);
        self.sink_latency_faults.fill(false);
        for (source_index, ((binding, destination), &control_index)) in self
            .plan
            .source_bindings
            .iter()
            .zip(&mut self.source_buffers)
            .zip(&self.plan.source_controls)
            .enumerate()
        {
            let samples = sample_count(frames_usize, binding.channels);
            destination[..samples].fill(0.0);
            if !self.controls[control_index].control.enabled {
                continue;
            }
            let transfer = self.source_timelines[source_index].read(
                external,
                binding,
                &mut destination[..samples],
                &cycle,
            )?;
            // An adapter may fill only a prefix. Never process stale data from
            // another cycle, even if it wrote beyond the reported prefix.
            destination[sample_count(transfer.frames as usize, binding.channels)..samples]
                .fill(0.0);
            let starved = transfer.activity == mixrelay_audio_io::process::InputActivity::Active
                && transfer.frames < frames;
            report.input_starvations += u64::from(starved);
            self.source_latency_faults[source_index] = transfer.discontinuity || starved;
            report.discontinuities += u64::from(transfer.discontinuity);
            report.sample_faults.source_samples = report
                .sample_faults
                .source_samples
                .saturating_add(transfer.source_sample_faults);
            if transfer.discontinuity {
                let targets = &self.plan.source_history[source_index];
                for range in &targets.device_buses {
                    self.device_bus_state[range.clone()].fill(0.0);
                }
                if let Some(bank) = &mut self.delay_bank {
                    bank.reset_routes(&targets.delay_routes);
                }
                for &state in &targets.histories {
                    state.reset(&mut self.effect_states, &mut self.dynamics_envelopes);
                }
            }
        }
        Ok(report)
    }

    fn admit_external_cycle(
        &mut self,
        cycle: ProcessCycle,
        external: &mut impl ExternalIo,
    ) -> Result<Option<bool>, ProcessBlockError> {
        if cycle.frames > self.plan.format.block_frames {
            return Err(ProcessBlockError::FrameCountExceeded {
                maximum: self.plan.format.block_frames,
                found: cycle.frames,
            });
        }
        // Validation is speculative: a capacity retry must accept the same timeline coordinates
        // next time. The fixed-size candidate contains no heap storage or native resources.
        let mut candidate = self.cycle_tracker.clone();
        candidate
            .accept(
                cycle,
                mixrelay_audio_io::session::SessionFormat {
                    sample_rate: self.plan.format.sample_rate,
                    block_frames: self.plan.format.block_frames,
                },
            )
            .map_err(ProcessBlockError::InvalidCycle)?;
        self.sample_controls();
        match external.begin_cycle(
            &cycle,
            ExternalControlState {
                plan: &self.plan,
                controls: &self.controls,
            },
        ) {
            CycleAdmission::Retry => Ok(None),
            CycleAdmission::Process => {
                self.cycle_tracker = candidate;
                Ok(Some(cycle.discontinuity))
            }
            CycleAdmission::ClockHandoff => {
                self.cycle_tracker = candidate;
                Ok(Some(false))
            }
        }
    }

    fn process_loaded_block(&mut self, frames: usize) -> SampleFaults {
        let mut faults = SampleFaults::default();
        for ((binding, source), &control_index) in self
            .plan
            .source_bindings
            .iter()
            .zip(&mut self.source_buffers)
            .zip(&self.plan.source_controls)
        {
            if self.controls[control_index].control.enabled {
                faults.source_samples +=
                    sanitize_samples(&mut source[..sample_count(frames, binding.channels)]);
            }
        }
        if let Some(bank) = &mut self.delay_bank {
            bank.validate();
        }
        if frames == self.plan.format.block_frames as usize {
            self.arena.fill(0.0);
        } else {
            silence_slots(&mut self.arena, &self.plan.arena_slots, frames);
        }
        self.dynamics_minimum_gains.fill(1.0);
        for (sink, &control_index) in self.sink_buffers.iter_mut().zip(&self.plan.sink_controls) {
            if !self.controls[control_index].control.enabled {
                sink.fill(0.0);
            }
        }
        for phase in [
            RenderPhase::Sources,
            RenderPhase::Processing,
            RenderPhase::Sinks,
        ] {
            if matches!(phase, RenderPhase::Sinks) {
                apply_routes(
                    &mut self.arena,
                    &self.plan.sink_routes,
                    &self.controls,
                    frames,
                    self.delay_bank.as_deref_mut(),
                );
            }
            self.process_phase(phase, frames, &mut faults);
        }
        if !self.device_bus_state.is_empty() {
            // All buses read before publishing at the same prepared-quantum
            // delay, independent of how the native callback partitions frames.
            self.device_bus_cursor =
                (self.device_bus_cursor + frames) % self.plan.format.block_frames as usize;
        }
        self.capture_and_meter(frames);
        faults.blocks = u64::from(
            faults.source_samples != 0 || faults.input_samples != 0 || faults.output_samples != 0,
        );
        faults
    }

    fn sample_controls(&mut self) {
        if !self.plan.controls.sample(
            &mut self.controls,
            &mut self.control_scratch,
            &mut self.sampled_control_epoch,
        ) {
            return;
        }
        // Device snapshots are persistent audio, unlike the per-block cleared arena. OFF must
        // forget its last quantum so re-enabling cannot replay audio from before the pause.
        if let Some(bank) = &mut self.delay_bank {
            bank.observe_controls(&self.controls);
        }
        for binding in &self.plan.device_bus_bindings {
            if !self.controls[binding.control_index].control.enabled {
                self.device_bus_state[binding.samples.clone()].fill(0.0);
            }
        }
    }

    fn process_phase(&mut self, phase: RenderPhase, frames: usize, faults: &mut SampleFaults) {
        for &index in self.plan.schedule.nodes(phase) {
            let step = &self.plan.steps[index];
            let control = self.controls[index];
            if !control.control.enabled {
                for kernel in step.kernels(phase) {
                    if let Kernel::Hosted { processor, .. } = kernel {
                        self.hosted_processors[*processor].suspend_processing();
                    }
                    let history = match kernel {
                        Kernel::Effect { state, .. } => Some(history::State::Effect(*state)),
                        Kernel::Compressor {
                            state_index,
                            bypass,
                            ..
                        }
                        | Kernel::Limiter {
                            state_index,
                            bypass,
                            ..
                        } => {
                            // OFF settles the parameter transition; timeline resets keep it.
                            self.dynamics_wet[*state_index] = f32::from(!*bypass);
                            Some(history::State::Dynamics(*state_index))
                        }
                        _ => None,
                    };
                    if let Some(state) = history {
                        state.reset(&mut self.effect_states, &mut self.dynamics_envelopes);
                    }
                }
                continue;
            }
            if matches!(phase, RenderPhase::Processing) {
                apply_routes(
                    &mut self.arena,
                    &step.incoming_routes,
                    &self.controls,
                    frames,
                    self.delay_bank.as_deref_mut(),
                );
            }
            // Routes can overflow even when each upstream sample is finite.
            // Protect every consumer, including sidechains and native/recorder sinks.
            faults.input_samples += sanitize_slots(&mut self.arena, &step.inputs, frames);
            if matches!(phase, RenderPhase::Sinks) && control.control.muted {
                silence_slots(&mut self.arena, &step.inputs, frames);
            }
            for kernel in step.kernels(phase) {
                apply_kernel(
                    KernelResources {
                        arena: &mut self.arena,
                        sources: &self.source_buffers,
                        sinks: &mut self.sink_buffers,
                        dynamics_envelopes: &mut self.dynamics_envelopes,
                        dynamics_minimum_gains: &mut self.dynamics_minimum_gains,
                        dynamics_wet: &mut self.dynamics_wet,
                        effect_states: &mut self.effect_states,
                        effects: &self.plan.effects,
                        device_bus_state: &mut self.device_bus_state,
                        device_bus_cursor: self.device_bus_cursor,
                        hosted_scratch: &mut self.hosted_scratch,
                        hosted_input_scratch_samples: self.hosted_input_scratch_samples,
                        hosted_processors: &mut self.hosted_processors,
                    },
                    kernel,
                    control.linear_gain,
                    frames,
                );
            }
            // Source gain, mixing and hosted processors share this boundary.
            // Invalid output must not become the next node's recursive history.
            faults.output_samples += sanitize_slots(&mut self.arena, &step.outputs, frames);
            if !matches!(phase, RenderPhase::Sinks) && control.control.muted {
                silence_slots(&mut self.arena, &step.outputs, frames);
            }
        }
    }

    fn capture_and_meter(&self, frames: usize) {
        for ((binding, recorder), &control_index) in self
            .plan
            .recorder_bindings
            .iter()
            .zip(&self.recorders)
            .zip(&self.plan.recorder_controls)
        {
            if !self.controls[control_index].control.enabled {
                continue;
            }
            let samples = sample_count(frames, binding.channels);
            recorder.capture(
                &self.arena[binding.slot.offset..binding.slot.offset + samples],
                self.plan.format.sample_rate,
            );
        }
        if let Some(meters) = &self.meters {
            if frames == 0 {
                return;
            }
            let revision = self
                .plan
                .revision
                .max(self.plan.controls.applied_revision());
            meters.begin_publication();
            for (index, binding) in self.plan.meter_bindings.iter().enumerate() {
                let samples = sample_count(frames, binding.channels);
                meters.record_interleaved(
                    revision,
                    index,
                    binding.channels,
                    &self.arena[binding.slot.offset..binding.slot.offset + samples],
                );
                if let Some(dynamics) = binding.dynamics {
                    meters.record_dynamics(
                        revision,
                        index,
                        &self.arena[dynamics.input.offset
                            ..dynamics.input.offset
                                + sample_count(frames, dynamics.input.channels)],
                        crate::DynamicsMeterBlock {
                            channels: dynamics.input.channels,
                            minimum_gain: self.dynamics_minimum_gains[dynamics.state_index],
                            threshold_db: dynamics.threshold_db,
                        },
                    );
                }
            }
            meters.finish_publication(revision);
        }
    }

    fn validate_buffers(
        &self,
        frames: u32,
        sources: &[&[f32]],
        sinks: &[&mut [f32]],
    ) -> Result<usize, ProcessBlockError> {
        if frames > self.plan.format.block_frames {
            return Err(ProcessBlockError::FrameCountExceeded {
                maximum: self.plan.format.block_frames,
                found: frames,
            });
        }
        if sources.len() != self.plan.source_bindings.len() {
            return Err(ProcessBlockError::SourceCountMismatch {
                expected: self.plan.source_bindings.len(),
                found: sources.len(),
            });
        }
        if sinks.len() != self.plan.sink_bindings.len() {
            return Err(ProcessBlockError::SinkCountMismatch {
                expected: self.plan.sink_bindings.len(),
                found: sinks.len(),
            });
        }

        let frames = frames as usize;
        validate_source_lengths(frames, &self.plan.source_bindings, sources)?;
        validate_sink_lengths(frames, &self.plan.sink_bindings, sinks)?;
        Ok(frames)
    }
}

type ExternalBuffers = Box<[Box<[f32]>]>;

fn allocate_external_buffers(
    bindings: &[ExternalBinding],
    format: RenderFormat,
) -> Result<ExternalBuffers, RenderPlanError> {
    let mut result = buffers::reserve(bindings.len())?;
    for binding in bindings {
        result.push(buffers::filled(
            sample_count(format.block_frames as usize, binding.channels),
            0.0,
        )?);
    }
    Ok(result.into_boxed_slice())
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RenderPlanError {
    #[error("render buffer preparation failed: {0}")]
    BufferPreparation(String),
    #[error("built-in effect preparation failed: {0}")]
    BuiltinPreparation(String),
    /// Derived route alignment could not be prepared before publication.
    #[error("route delay preparation failed: {0}")]
    CompensationPreparation(String),
    #[error("render buffer size overflowed")]
    BufferSizeOverflow,
    #[error(
        "virtual device source at node {node_id} must be lowered by graph composition before rendering"
    )]
    UnloweredVirtualEndpoint { node_id: NodeId },
    #[error("node {node_id} does not expose required {port_id} port")]
    MissingPort { node_id: NodeId, port_id: PortId },
    #[error("duplicate compiled port {node_id}:{port_id}")]
    DuplicatePort { node_id: NodeId, port_id: PortId },
    #[error("provider node {node_id} requires a registered render kernel")]
    UnsupportedProvider { node_id: NodeId },
    /// A hosted provider direction has zero or too many buses.
    #[error("provider node {node_id} has {found} {direction} buses; expected 1..={maximum}")]
    InvalidProviderBusCount {
        /// Provider whose layout failed validation.
        node_id: NodeId,
        /// Input or output direction.
        direction: &'static str,
        /// Maximum accepted bus count.
        maximum: usize,
        /// Actual bus count.
        found: usize,
    },
    /// A hosted provider bus has zero or too many channels.
    #[error("provider node {node_id} has invalid {direction} bus channel count {channels}")]
    InvalidProviderBusChannels {
        /// Provider whose layout failed validation.
        node_id: NodeId,
        /// Input or output direction.
        direction: &'static str,
        /// Invalid bus channel count.
        channels: u16,
    },
    /// A hosted provider direction exceeds the aggregate channel bound.
    #[error(
        "provider node {node_id} has {channels} total {direction} channels; maximum is {maximum}"
    )]
    InvalidProviderTotalChannels {
        /// Provider whose layout failed validation.
        node_id: NodeId,
        /// Input or output direction.
        direction: &'static str,
        /// Maximum accepted aggregate channels.
        maximum: u16,
        /// Actual aggregate channel count.
        channels: u16,
    },
    /// A multibus provider lacks a selected layout contract.
    #[error("provider node {node_id} has no selected multibus layout")]
    MissingProviderLayout {
        /// Provider whose layout is missing.
        node_id: NodeId,
    },
    /// The selected provider layout does not match the compiled port order.
    #[error("provider node {node_id} layout does not match its compiled ports")]
    ProviderLayoutMismatch {
        /// Provider whose compiled geometry differs from its saved layout.
        node_id: NodeId,
    },
    /// The saved multibus provider layout failed graph contract validation.
    #[error("provider node {node_id} has invalid selected layout: {reason}")]
    InvalidProviderLayout {
        /// Provider whose saved layout was invalid.
        node_id: NodeId,
        /// Typed layout diagnostic.
        reason: String,
    },
    #[error("provider node {node_id} could not be prepared: {reason}")]
    ProviderPreparation { node_id: NodeId, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProcessBlockError {
    #[error("native adapter reported more frames than requested")]
    InvalidTransfer,
    #[error("invalid audio process cycle: {0}")]
    InvalidCycle(mixrelay_audio_io::process::CycleError),
    #[error("frame count {found} exceeds prepared maximum {maximum}")]
    FrameCountExceeded { maximum: u32, found: u32 },
    #[error("expected {expected} source buffers, found {found}")]
    SourceCountMismatch { expected: usize, found: usize },
    #[error("expected {expected} sink buffers, found {found}")]
    SinkCountMismatch { expected: usize, found: usize },
    #[error("source buffer {index} requires {required} samples, found {found}")]
    SourceBufferTooShort {
        index: usize,
        required: usize,
        found: usize,
    },
    #[error("sink buffer {index} requires {required} samples, found {found}")]
    SinkBufferTooShort {
        index: usize,
        required: usize,
        found: usize,
    },
}

#[derive(Debug, Clone, Copy)]
struct PortSlot {
    offset: usize,
    channels: u16,
}

#[derive(Debug, Clone, Copy)]
struct DryBusMap {
    source_channel_offset: usize,
    destination_channel_offset: usize,
    channels: u16,
}

#[derive(Debug)]
struct DynamicsBinding {
    node_id: NodeId,
    config: NodeConfig,
    state_index: usize,
}

#[derive(Debug)]
struct DeviceBusBinding {
    node_id: NodeId,
    samples: std::ops::Range<usize>,
    control_index: usize,
    input_path: history::InputPath,
}

fn device_bus_bindings(
    execution: &ExecutionPlan,
    slots: &BTreeMap<PortKey, PortSlot>,
) -> Result<Box<[DeviceBusBinding]>, RenderPlanError> {
    let mut offset = 0;
    let mut bindings = Vec::new();
    for (control_index, node) in execution.nodes().iter().enumerate() {
        let NodeConfig::DeviceBus { .. } = node.config() else {
            continue;
        };
        let input = node_slot(
            node,
            &PortId::from(AUDIO_PORT_ID),
            PortDirection::Input,
            slots,
        )?;
        // Called only after lowering has checked the same capacities for overflow.
        let end = offset + sample_count(execution.format().block_frames as usize, input.channels);
        bindings.push(DeviceBusBinding {
            node_id: node.id().clone(),
            samples: offset..end,
            control_index,
            input_path: history::InputPath::prepare(execution, node.id()),
        });
        offset = end;
    }
    bindings.sort_unstable_by(|left, right| left.node_id.cmp(&right.node_id));
    Ok(bindings.into_boxed_slice())
}

fn dynamics_bindings(execution: &ExecutionPlan) -> Box<[DynamicsBinding]> {
    // State indices follow lowering order; only the common history registry is sorted.
    let bindings: Vec<_> = execution
        .nodes()
        .iter()
        .filter(|node| {
            matches!(
                node.config(),
                NodeConfig::Compressor(_) | NodeConfig::Limiter(_)
            )
        })
        .enumerate()
        .map(|(state_index, node)| DynamicsBinding {
            node_id: node.id().clone(),
            config: node.config().clone(),
            state_index,
        })
        .collect();
    bindings.into_boxed_slice()
}

fn prepare_effects(execution: &ExecutionPlan) -> Result<Box<[effects::Binding]>, RenderPlanError> {
    let mut bindings = Vec::new();
    let mut bytes = 0_usize;
    for node in execution.nodes() {
        if let NodeConfig::Effect(config) = node.config() {
            let design = effects::Design::new(config, execution.format())
                .map_err(RenderPlanError::BuiltinPreparation)?;
            bytes = bytes
                .checked_add(
                    design
                        .storage_bytes()
                        .map_err(RenderPlanError::BuiltinPreparation)?,
                )
                .ok_or(RenderPlanError::BufferSizeOverflow)?;
            if bytes > effects::MAX_EFFECT_BYTES {
                return Err(RenderPlanError::BuiltinPreparation(
                    "aggregate built-in effect memory capacity exceeded".into(),
                ));
            }
            bindings.push(effects::Binding {
                node_id: node.id().clone(),
                design,
            });
        }
    }
    Ok(bindings.into_boxed_slice())
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PortKey {
    endpoint: AudioEndpoint,
    direction: PortDirection,
}

#[derive(Debug)]
struct NodeStep {
    incoming_routes: Box<[RouteOp]>,
    sources: Box<[Kernel]>,
    processing: Box<[Kernel]>,
    sinks: Box<[Kernel]>,
    inputs: Box<[PortSlot]>,
    outputs: Box<[PortSlot]>,
}

#[derive(Clone, Copy)]
enum RenderPhase {
    Sources,
    Processing,
    Sinks,
}

impl NodeStep {
    fn new(
        node: &CompiledNode,
        kernels: LoweredKernels,
        incoming_routes: Vec<RouteOp>,
        slots: &BTreeMap<PortKey, PortSlot>,
    ) -> Result<Self, RenderPlanError> {
        let ports = |direction| {
            let ports = match direction {
                PortDirection::Input => node.inputs(),
                PortDirection::Output => node.outputs(),
            };
            ports
                .iter()
                .map(|port| node_slot(node, port.id(), direction, slots))
                .collect::<Result<Box<[_]>, _>>()
        };
        Ok(Self {
            incoming_routes: incoming_routes.into_boxed_slice(),
            sources: kernels.sources.into_boxed_slice(),
            processing: kernels.processing.into_boxed_slice(),
            sinks: kernels.sinks.into_boxed_slice(),
            inputs: ports(PortDirection::Input)?,
            outputs: ports(PortDirection::Output)?,
        })
    }

    fn kernels(&self, phase: RenderPhase) -> &[Kernel] {
        match phase {
            RenderPhase::Sources => &self.sources,
            RenderPhase::Processing => &self.processing,
            RenderPhase::Sinks => &self.sinks,
        }
    }
}

fn binding_controls<'a>(
    controls: &RuntimeControlBank,
    ids: impl Iterator<Item = &'a NodeId>,
) -> Box<[usize]> {
    ids.map(|id| {
        controls
            .index(id)
            .expect("a lowered binding belongs to a compiled node")
    })
    .collect()
}

fn silence_slots(arena: &mut [f32], slots: &[PortSlot], frames: usize) {
    for slot in slots {
        arena[slot.offset..slot.offset + sample_count(frames, slot.channels)].fill(0.0);
    }
}

fn sanitize_slots(arena: &mut [f32], slots: &[PortSlot], frames: usize) -> u64 {
    slots.iter().fold(0, |count, slot| {
        count
            + sanitize_samples(
                &mut arena[slot.offset..slot.offset + sample_count(frames, slot.channels)],
            )
    })
}

#[derive(Debug, Default)]
struct LoweredRoutes {
    incoming: BTreeMap<NodeId, Vec<RouteOp>>,
    sinks: Vec<RouteOp>,
}

#[derive(Debug, Default)]
struct LoweredKernels {
    sources: Vec<Kernel>,
    processing: Vec<Kernel>,
    sinks: Vec<Kernel>,
}

#[derive(Debug)]
enum Kernel {
    Effect {
        input: PortSlot,
        output: PortSlot,
        sidechain: Option<PortSlot>,
        state: usize,
    },
    DeviceBus(DeviceBusKernel),
    Source {
        binding: usize,
        output: PortSlot,
    },
    Sink {
        binding: usize,
        input: PortSlot,
    },
    Gain {
        input: PortSlot,
        output: PortSlot,
    },
    Compressor {
        input: PortSlot,
        output: PortSlot,
        state_index: usize,
        threshold_linear: f32,
        exponent: f32,
        attack_coefficient: f32,
        release_coefficient: f32,
        makeup_gain: f32,
        bypass: bool,
        bypass_coefficient: f32,
    },
    Limiter {
        input: PortSlot,
        output: PortSlot,
        state_index: usize,
        ceiling_linear: f32,
        release_coefficient: f32,
        bypass: bool,
        bypass_coefficient: f32,
    },
    Mixer {
        inputs: Box<[PortSlot]>,
        output: PortSlot,
    },
    Hosted {
        io: HostedIo,
        processor: usize,
    },
}

#[derive(Debug)]
enum HostedIo {
    Single {
        input: PortSlot,
        output: PortSlot,
    },
    Multiple {
        inputs: Box<[PortSlot]>,
        outputs: Box<[PortSlot]>,
        input_channels: u16,
        output_channels: u16,
        dry_maps: Box<[DryBusMap]>,
    },
}

struct HostedProcessorIo {
    io: HostedIo,
    input_channels: u16,
    output_channels: u16,
}

#[derive(Debug)]
enum DeviceBusKernel {
    Read {
        snapshot: PortSlot,
        output: PortSlot,
        capacity: usize,
    },
    Publish {
        snapshot: PortSlot,
        input: PortSlot,
        capacity: usize,
    },
}

impl DeviceBusKernel {
    fn apply(&self, arena: &mut [f32], state: &mut [f32], frames: usize, cursor: usize) {
        match self {
            Self::Read {
                snapshot,
                output,
                capacity,
            } => {
                let samples = sample_count(frames, output.channels);
                let offset = sample_count(cursor, output.channels);
                let head = samples.min(capacity - offset);
                let source = &state[snapshot.offset..snapshot.offset + capacity];
                let destination = &mut arena[output.offset..output.offset + samples];
                destination[..head].copy_from_slice(&source[offset..offset + head]);
                destination[head..].copy_from_slice(&source[..samples - head]);
            }
            Self::Publish {
                snapshot,
                input,
                capacity,
            } => {
                let samples = sample_count(frames, input.channels);
                let offset = sample_count(cursor, input.channels);
                let head = samples.min(capacity - offset);
                let destination = &mut state[snapshot.offset..snapshot.offset + capacity];
                let source = &arena[input.offset..input.offset + samples];
                // Only replace the frames consumed in the source phase. A short
                // quantum leaves the unread tail owned by the following cycle.
                destination[offset..offset + head].copy_from_slice(&source[..head]);
                destination[..samples - head].copy_from_slice(&source[head..]);
            }
        }
    }
}

fn allocate_slots(
    nodes: &[CompiledNode],
    block_frames: usize,
) -> Result<(BTreeMap<PortKey, PortSlot>, usize), RenderPlanError> {
    let mut slots = BTreeMap::new();
    let mut next_offset = 0_usize;
    for node in nodes {
        for port in node.inputs().iter().chain(node.outputs()) {
            let key = PortKey {
                endpoint: AudioEndpoint {
                    node_id: node.id().clone(),
                    port_id: port.id().clone(),
                },
                direction: port.direction(),
            };
            let sample_count = block_frames
                .checked_mul(usize::from(port.channels()))
                .ok_or(RenderPlanError::BufferSizeOverflow)?;
            let slot = PortSlot {
                offset: next_offset,
                channels: port.channels(),
            };
            next_offset = next_offset
                .checked_add(sample_count)
                .ok_or(RenderPlanError::BufferSizeOverflow)?;
            if slots.insert(key.clone(), slot).is_some() {
                return Err(RenderPlanError::DuplicatePort {
                    node_id: key.endpoint.node_id,
                    port_id: key.endpoint.port_id,
                });
            }
        }
    }
    Ok((slots, next_offset))
}

fn lower_routes(execution: &ExecutionPlan, slots: &BTreeMap<PortKey, PortSlot>) -> LoweredRoutes {
    let mut lowered = LoweredRoutes::default();
    let controls: BTreeMap<_, _> = execution
        .nodes()
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id(), index))
        .collect();
    for (route_index, edge) in execution.edges().iter().enumerate() {
        let source_control = controls[&edge.source().node_id];
        let destination_control = controls[&edge.destination().node_id];
        let source = compiled_edge_slot(slots, edge.source(), PortDirection::Output);
        let destination = compiled_edge_slot(slots, edge.destination(), PortDirection::Input);
        let channels = edge
            .routes()
            .iter()
            .map(|route| (route.source_channel, route.destination_channel));
        let route = RouteOp::new(
            source,
            destination,
            [source_control, destination_control],
            channels,
            route_index,
        );
        if is_external_sink(execution, edge.destination()) {
            lowered.sinks.push(route);
            continue;
        }
        lowered
            .incoming
            .entry(edge.destination().node_id.clone())
            .or_default()
            .push(route);
    }
    lowered
}

fn is_external_sink(execution: &ExecutionPlan, endpoint: &AudioEndpoint) -> bool {
    execution.nodes().iter().any(|node| {
        node.id() == &endpoint.node_id
            && matches!(
                node.config(),
                NodeConfig::AudioDeviceOutput(_)
                    | NodeConfig::Recorder { .. }
                    | NodeConfig::VirtualEndpointTransport(_)
                    | NodeConfig::DeviceBus { .. }
            )
    })
}

fn collect_external_bindings(
    execution: &ExecutionPlan,
    slots: &BTreeMap<PortKey, PortSlot>,
    direction: PortDirection,
) -> Result<Vec<ExternalBinding>, RenderPlanError> {
    let mut bindings = Vec::new();
    for node in execution.nodes() {
        if matches!(node.config(), NodeConfig::VirtualEndpoint(_)) {
            return Err(RenderPlanError::UnloweredVirtualEndpoint {
                node_id: node.id().clone(),
            });
        }
        let port_id = match (node.config(), direction) {
            (
                NodeConfig::ApplicationCapture(_) | NodeConfig::AudioDeviceInput(_),
                PortDirection::Output,
            )
            | (NodeConfig::AudioDeviceOutput(_), PortDirection::Input) => Some(AUDIO_PORT_ID),
            (NodeConfig::VirtualEndpointTransport(config), PortDirection::Output)
                if config.capture.is_some() =>
            {
                Some(FROM_OS_PORT_ID)
            }
            (NodeConfig::VirtualEndpointTransport(config), PortDirection::Input)
                if config.render.is_some() =>
            {
                Some(TO_OS_PORT_ID)
            }
            _ => None,
        };
        if let Some(port_id) = port_id {
            let port_id = PortId::from(port_id);
            // Sources must keep being drained and metered before a cable is
            // attached. Only an unconnected external sink may be omitted.
            if direction == PortDirection::Input
                && !matches!(node.config(), NodeConfig::VirtualEndpointTransport(_))
                && !external_port_is_connected(execution, node.id(), &port_id, direction)
            {
                continue;
            }
            let slot = node_slot(node, &port_id, direction, slots)?;
            bindings.push(ExternalBinding {
                node_id: node.id().clone(),
                kind: node.kind(),
                port_id,
                channels: slot.channels,
                virtual_endpoint_id: match node.config() {
                    NodeConfig::VirtualEndpointTransport(endpoint) => Some(endpoint.id.clone()),
                    _ => None,
                },
            });
        }
    }
    bindings.sort_unstable_by(|left, right| {
        left.node_id
            .cmp(&right.node_id)
            .then(left.port_id.cmp(&right.port_id))
    });
    Ok(bindings)
}

fn collect_meter_bindings(
    execution: &ExecutionPlan,
    slots: &BTreeMap<PortKey, PortSlot>,
) -> Result<Vec<MeterBinding>, RenderPlanError> {
    let mut bindings = Vec::with_capacity(execution.nodes().len());
    let mut dynamics_index = 0;
    for node in execution.nodes() {
        let threshold = match node.config() {
            NodeConfig::Compressor(config) => Some(config.threshold_db),
            NodeConfig::Limiter(config) => Some(config.ceiling_db),
            _ => None,
        };
        let dynamics = if let Some(threshold_db) = threshold {
            let binding = DynamicsMeterBinding {
                input: node_slot(
                    node,
                    &PortId::from(AUDIO_PORT_ID),
                    PortDirection::Input,
                    slots,
                )?,
                state_index: dynamics_index,
                threshold_db,
            };
            dynamics_index += 1;
            Some(binding)
        } else {
            None
        };
        for port in node.outputs() {
            let mut binding = meter_binding(node, port.id(), PortDirection::Output, slots)?;
            binding.dynamics = dynamics;
            bindings.push(binding);
        }
        if node.outputs().is_empty()
            || matches!(node.config(), NodeConfig::VirtualEndpointTransport(_))
        {
            for port in node.inputs() {
                bindings.push(meter_binding(node, port.id(), PortDirection::Input, slots)?);
            }
        }
    }
    Ok(bindings)
}

fn collect_recorder_bindings(
    execution: &ExecutionPlan,
    slots: &BTreeMap<PortKey, PortSlot>,
) -> Result<Vec<MeterBinding>, RenderPlanError> {
    execution
        .nodes()
        .iter()
        .filter(|node| matches!(node.config(), NodeConfig::Recorder { .. }))
        .map(|node| {
            meter_binding(
                node,
                &PortId::from(AUDIO_PORT_ID),
                PortDirection::Input,
                slots,
            )
        })
        .collect()
}

fn meter_binding(
    node: &CompiledNode,
    port_id: &PortId,
    direction: PortDirection,
    slots: &BTreeMap<PortKey, PortSlot>,
) -> Result<MeterBinding, RenderPlanError> {
    let slot = node_slot(node, port_id, direction, slots)?;
    Ok(MeterBinding {
        node_id: node.id().clone(),
        port_id: port_id.clone(),
        channels: slot.channels,
        slot,
        dynamics: None,
    })
}

fn external_port_is_connected(
    execution: &ExecutionPlan,
    node_id: &NodeId,
    port_id: &PortId,
    direction: PortDirection,
) -> bool {
    execution.edges().iter().any(|edge| {
        let endpoint = match direction {
            PortDirection::Input => edge.destination(),
            PortDirection::Output => edge.source(),
        };
        &endpoint.node_id == node_id && &endpoint.port_id == port_id
    })
}

fn compiled_edge_slot(
    slots: &BTreeMap<PortKey, PortSlot>,
    endpoint: &AudioEndpoint,
    direction: PortDirection,
) -> PortSlot {
    slots[&PortKey {
        endpoint: endpoint.clone(),
        direction,
    }]
}

struct LoweringContext<'a> {
    slots: &'a BTreeMap<PortKey, PortSlot>,
    source_bindings: &'a [ExternalBinding],
    sink_bindings: &'a [ExternalBinding],
    format: RenderFormat,
    hosted_factory: Option<&'a dyn HostedProcessorFactory>,
}

struct LoweringState<'a> {
    dynamics_state_count: &'a mut usize,
    effect_count: &'a mut usize,
    device_bus_samples: &'a mut usize,
    hosted_input_scratch_samples: &'a mut usize,
    hosted_output_scratch_samples: &'a mut usize,
    hosted_processors: &'a mut Vec<Box<dyn HostedAudioProcessor>>,
}

fn lower_kernels(
    node: &CompiledNode,
    context: &LoweringContext<'_>,
    state: &mut LoweringState<'_>,
) -> Result<LoweredKernels, RenderPlanError> {
    match node.config() {
        NodeConfig::ApplicationCapture(_) | NodeConfig::AudioDeviceInput(_) => {
            lower_source_node(node, context.slots, context.source_bindings)
        }
        NodeConfig::AudioDeviceOutput(_) => Ok(LoweredKernels {
            sources: vec![],
            processing: vec![],
            sinks: sink_kernel(node, AUDIO_PORT_ID, context.slots, context.sink_bindings)?
                .into_iter()
                .collect(),
        }),
        NodeConfig::Recorder { .. } => Ok(LoweredKernels {
            sources: vec![],
            processing: vec![],
            sinks: vec![],
        }),
        NodeConfig::Compressor(_)
        | NodeConfig::Effect(_)
        | NodeConfig::Limiter(_)
        | NodeConfig::Mixer(_)
        | NodeConfig::Gain(_) => lower_builtin_processor(node, context, state),
        NodeConfig::DeviceBus { .. } => lower_device_bus(node, context, state),
        NodeConfig::VirtualEndpointTransport(config) => {
            lower_virtual_endpoint(node, config, context)
        }
        NodeConfig::VirtualEndpoint(_) => Err(RenderPlanError::UnloweredVirtualEndpoint {
            node_id: node.id().clone(),
        }),
        NodeConfig::Provider(config) => lower_hosted_processor(node, config, context, state),
    }
}

fn lower_effect(
    node: &CompiledNode,
    config: &mixrelay_audio_graph::EffectConfig,
    context: &LoweringContext<'_>,
    state: &mut LoweringState<'_>,
) -> Result<LoweredKernels, RenderPlanError> {
    let (input, output) = processor_slots(node, context.slots)?;
    if input.channels != config.channels || output.channels != config.output_channels() {
        return Err(RenderPlanError::BuiltinPreparation(
            "effect port geometry does not match configuration".into(),
        ));
    }
    let sidechain = if matches!(config.effect, mixrelay_audio_graph::Effect::Ducking(_)) {
        Some(node_slot(
            node,
            &PortId::from(mixrelay_audio_graph::SIDECHAIN_PORT_ID),
            PortDirection::Input,
            context.slots,
        )?)
    } else {
        None
    };
    if sidechain.is_some_and(|port| port.channels != 1) {
        return Err(RenderPlanError::BuiltinPreparation(
            "ducking control input must be mono".into(),
        ));
    }
    let index = *state.effect_count;
    *state.effect_count += 1;
    Ok(LoweredKernels {
        sources: vec![],
        processing: vec![Kernel::Effect {
            input,
            output,
            sidechain,
            state: index,
        }],
        sinks: vec![],
    })
}

fn lower_builtin_processor(
    node: &CompiledNode,
    context: &LoweringContext<'_>,
    state: &mut LoweringState<'_>,
) -> Result<LoweredKernels, RenderPlanError> {
    match node.config() {
        NodeConfig::Effect(config) => lower_effect(node, config, context, state),
        NodeConfig::Compressor(config) => lower_compressor(node, config, context, state),
        NodeConfig::Limiter(config) => {
            let (input, output) = processor_slots(node, context.slots)?;
            let state_index = *state.dynamics_state_count;
            *state.dynamics_state_count += 1;
            Ok(LoweredKernels {
                sources: vec![],
                processing: vec![Kernel::Limiter {
                    input,
                    output,
                    state_index,
                    ceiling_linear: db_to_linear(config.ceiling_db),
                    release_coefficient: smoothing_coefficient(
                        config.release_ms,
                        context.format.sample_rate,
                    ),
                    bypass: config.bypass,
                    bypass_coefficient: 1.0
                        - smoothing_coefficient(5.0, context.format.sample_rate),
                }],
                sinks: vec![],
            })
        }
        NodeConfig::Mixer(config) => {
            let inputs = config
                .inputs
                .iter()
                .map(|port_id| node_slot(node, port_id, PortDirection::Input, context.slots))
                .collect::<Result<Vec<_>, _>>()?
                .into_boxed_slice();
            let output = node_slot(
                node,
                &PortId::from(AUDIO_PORT_ID),
                PortDirection::Output,
                context.slots,
            )?;
            Ok(LoweredKernels {
                sources: vec![],
                processing: vec![Kernel::Mixer { inputs, output }],
                sinks: vec![],
            })
        }
        NodeConfig::Gain(_) => Ok(LoweredKernels {
            sources: vec![],
            processing: vec![Kernel::Gain {
                input: node_slot(
                    node,
                    &PortId::from(AUDIO_PORT_ID),
                    PortDirection::Input,
                    context.slots,
                )?,
                output: node_slot(
                    node,
                    &PortId::from(AUDIO_PORT_ID),
                    PortDirection::Output,
                    context.slots,
                )?,
            }],
            sinks: vec![],
        }),
        _ => unreachable!("non-built-in node passed to built-in lowering"),
    }
}

fn lower_compressor(
    node: &CompiledNode,
    config: &mixrelay_audio_graph::CompressorConfig,
    context: &LoweringContext<'_>,
    state: &mut LoweringState<'_>,
) -> Result<LoweredKernels, RenderPlanError> {
    let (input, output) = processor_slots(node, context.slots)?;
    let state_index = *state.dynamics_state_count;
    *state.dynamics_state_count += 1;
    Ok(LoweredKernels {
        sources: vec![],
        processing: vec![Kernel::Compressor {
            input,
            output,
            state_index,
            threshold_linear: db_to_linear(config.threshold_db),
            exponent: 1.0 - 1.0 / config.ratio,
            attack_coefficient: smoothing_coefficient(config.attack_ms, context.format.sample_rate),
            release_coefficient: smoothing_coefficient(
                config.release_ms,
                context.format.sample_rate,
            ),
            makeup_gain: db_to_linear(config.makeup_gain_db),
            bypass: config.bypass,
            bypass_coefficient: 1.0 - smoothing_coefficient(5.0, context.format.sample_rate),
        }],
        sinks: vec![],
    })
}

/// Device buses retain one prepared quantum of PCM, regardless of callback partition or topology.
fn lower_device_bus(
    node: &CompiledNode,
    context: &LoweringContext<'_>,
    state: &mut LoweringState<'_>,
) -> Result<LoweredKernels, RenderPlanError> {
    let (input, output) = processor_slots(node, context.slots)?;
    let capacity = sample_count(context.format.block_frames as usize, input.channels);
    let snapshot = PortSlot {
        offset: *state.device_bus_samples,
        channels: input.channels,
    };
    *state.device_bus_samples = state
        .device_bus_samples
        .checked_add(capacity)
        .ok_or(RenderPlanError::BufferSizeOverflow)?;
    Ok(LoweredKernels {
        sources: vec![Kernel::DeviceBus(DeviceBusKernel::Read {
            snapshot,
            output,
            capacity,
        })],
        processing: vec![],
        sinks: vec![Kernel::DeviceBus(DeviceBusKernel::Publish {
            snapshot,
            input,
            capacity,
        })],
    })
}

fn lower_virtual_endpoint(
    node: &CompiledNode,
    config: &mixrelay_audio_graph::VirtualEndpointSpec,
    context: &LoweringContext<'_>,
) -> Result<LoweredKernels, RenderPlanError> {
    let mut kernels = LoweredKernels::default();
    if config.capture.is_some() {
        kernels.sources.extend(source_kernel(
            node,
            FROM_OS_PORT_ID,
            context.slots,
            context.source_bindings,
        )?);
    }
    if config.render.is_some() {
        kernels.sinks.extend(sink_kernel(
            node,
            TO_OS_PORT_ID,
            context.slots,
            context.sink_bindings,
        )?);
    }
    Ok(kernels)
}

fn lower_hosted_processor(
    node: &CompiledNode,
    config: &ProviderNodeConfig,
    context: &LoweringContext<'_>,
    state: &mut LoweringState<'_>,
) -> Result<LoweredKernels, RenderPlanError> {
    let Some(factory) = context.hosted_factory else {
        if !node.control().enabled {
            return Ok(LoweredKernels::default());
        }
        return Err(RenderPlanError::UnsupportedProvider {
            node_id: node.id().clone(),
        });
    };
    let prepared_io = prepare_hosted_processor_io(node, config, context, state)?;
    let request = HostedProcessorPrepareRequest {
        node_id: node.id(),
        config,
        format: context.format,
        input_channels: prepared_io.input_channels,
        output_channels: prepared_io.output_channels,
    };
    let processor = if node.control().enabled {
        factory.prepare(request).map(Some)
    } else {
        factory.prepare_inactive(request)
    }
    .map_err(|reason| RenderPlanError::ProviderPreparation {
        node_id: node.id().clone(),
        reason,
    })?;
    let Some(processor) = processor else {
        return Ok(LoweredKernels::default());
    };
    let processor_index = state.hosted_processors.len();
    state.hosted_processors.push(processor);
    Ok(LoweredKernels {
        sources: vec![],
        processing: vec![Kernel::Hosted {
            io: prepared_io.io,
            processor: processor_index,
        }],
        sinks: vec![],
    })
}

fn prepare_hosted_processor_io(
    node: &CompiledNode,
    config: &ProviderNodeConfig,
    context: &LoweringContext<'_>,
    state: &mut LoweringState<'_>,
) -> Result<HostedProcessorIo, RenderPlanError> {
    let inputs = provider_port_slots(node, PortDirection::Input, context.slots)?;
    let outputs = provider_port_slots(node, PortDirection::Output, context.slots)?;
    let input_channels = provider_total_channels(node, "input", &inputs)?;
    let output_channels = provider_total_channels(node, "output", &outputs)?;
    let saved_layout = provider_selected_layout(node, config)?;
    let single_main_bus = inputs.len() == 1
        && outputs.len() == 1
        && saved_layout.is_none_or(|layout| {
            layout.inputs[0].kind == ProviderBusKind::Main
                && layout.outputs[0].kind == ProviderBusKind::Main
        });
    let io = if single_main_bus {
        HostedIo::Single {
            input: inputs[0],
            output: outputs[0],
        }
    } else {
        let layout = saved_layout.ok_or_else(|| RenderPlanError::MissingProviderLayout {
            node_id: node.id().clone(),
        })?;
        let block_frames = context.format.block_frames as usize;
        let input_samples = block_frames
            .checked_mul(usize::from(input_channels))
            .ok_or(RenderPlanError::BufferSizeOverflow)?;
        let output_samples = block_frames
            .checked_mul(usize::from(output_channels))
            .ok_or(RenderPlanError::BufferSizeOverflow)?;
        *state.hosted_input_scratch_samples =
            (*state.hosted_input_scratch_samples).max(input_samples);
        *state.hosted_output_scratch_samples =
            (*state.hosted_output_scratch_samples).max(output_samples);
        HostedIo::Multiple {
            inputs: inputs.into_boxed_slice(),
            outputs: outputs.into_boxed_slice(),
            input_channels,
            output_channels,
            dry_maps: make_dry_bus_maps(layout),
        }
    };
    Ok(HostedProcessorIo {
        io,
        input_channels,
        output_channels,
    })
}

fn provider_selected_layout<'a>(
    node: &CompiledNode,
    config: &'a ProviderNodeConfig,
) -> Result<Option<&'a ProviderPorts>, RenderPlanError> {
    let Some(layout) = config.ports.as_ref() else {
        return Ok(None);
    };
    layout
        .validate()
        .map_err(|error| RenderPlanError::InvalidProviderLayout {
            node_id: node.id().clone(),
            reason: error.to_string(),
        })?;
    if !provider_bus_list_matches(&layout.inputs, node.inputs())
        || !provider_bus_list_matches(&layout.outputs, node.outputs())
    {
        return Err(RenderPlanError::ProviderLayoutMismatch {
            node_id: node.id().clone(),
        });
    }
    Ok(Some(layout))
}

fn provider_bus_list_matches(
    buses: &[ProviderBus],
    ports: &[mixrelay_audio_graph::CompiledPort],
) -> bool {
    buses.len() == ports.len()
        && buses
            .iter()
            .zip(ports)
            .all(|(bus, port)| bus.id == *port.id() && bus.channels == port.channels())
}

fn make_dry_bus_maps(layout: &ProviderPorts) -> Box<[DryBusMap]> {
    let mut main_inputs = Vec::new();
    let mut input_offset = 0;
    for bus in &layout.inputs {
        if bus.kind == ProviderBusKind::Main {
            main_inputs.push((input_offset, bus.channels));
        }
        input_offset += usize::from(bus.channels);
    }
    let mut mappings = Vec::new();
    let mut main_ordinal = 0;
    let mut output_offset = 0;
    for bus in &layout.outputs {
        if bus.kind == ProviderBusKind::Main {
            if let Some(&(source_channel_offset, input_channels)) = main_inputs.get(main_ordinal) {
                let channels = input_channels.min(bus.channels);
                if channels > 0 {
                    mappings.push(DryBusMap {
                        source_channel_offset,
                        destination_channel_offset: output_offset,
                        channels,
                    });
                }
            }
            main_ordinal += 1;
        }
        output_offset += usize::from(bus.channels);
    }
    mappings.into_boxed_slice()
}

fn provider_port_slots(
    node: &CompiledNode,
    direction: PortDirection,
    slots: &BTreeMap<PortKey, PortSlot>,
) -> Result<Vec<PortSlot>, RenderPlanError> {
    node_ports(node, direction)
        .iter()
        .map(|port| node_slot(node, port.id(), direction, slots))
        .collect()
}

fn node_ports(
    node: &CompiledNode,
    direction: PortDirection,
) -> &[mixrelay_audio_graph::CompiledPort] {
    match direction {
        PortDirection::Input => node.inputs(),
        PortDirection::Output => node.outputs(),
    }
}

fn provider_total_channels(
    node: &CompiledNode,
    direction: &'static str,
    ports: &[PortSlot],
) -> Result<u16, RenderPlanError> {
    if ports.is_empty() || ports.len() > MAX_PROVIDER_BUSES_PER_DIRECTION {
        return Err(RenderPlanError::InvalidProviderBusCount {
            node_id: node.id().clone(),
            direction,
            maximum: MAX_PROVIDER_BUSES_PER_DIRECTION,
            found: ports.len(),
        });
    }
    let mut total = 0_u16;
    for port in ports {
        if !(1..=MAX_PROVIDER_CHANNELS_PER_DIRECTION).contains(&port.channels) {
            return Err(RenderPlanError::InvalidProviderBusChannels {
                node_id: node.id().clone(),
                direction,
                channels: port.channels,
            });
        }
        total = total
            .checked_add(port.channels)
            .ok_or(RenderPlanError::BufferSizeOverflow)?;
        if total > MAX_PROVIDER_CHANNELS_PER_DIRECTION {
            return Err(RenderPlanError::InvalidProviderTotalChannels {
                node_id: node.id().clone(),
                direction,
                maximum: MAX_PROVIDER_CHANNELS_PER_DIRECTION,
                channels: total,
            });
        }
    }
    Ok(total)
}

fn processor_slots(
    node: &CompiledNode,
    slots: &BTreeMap<PortKey, PortSlot>,
) -> Result<(PortSlot, PortSlot), RenderPlanError> {
    let port = PortId::from(AUDIO_PORT_ID);
    Ok((
        node_slot(node, &port, PortDirection::Input, slots)?,
        node_slot(node, &port, PortDirection::Output, slots)?,
    ))
}

fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

// Render-format validation caps the sample rate far below f32's exact-integer boundary.
#[allow(clippy::cast_precision_loss)]
fn smoothing_coefficient(milliseconds: f32, sample_rate: u32) -> f32 {
    (-1.0 / (milliseconds * 0.001 * sample_rate as f32)).exp()
}

fn lower_source_node(
    node: &CompiledNode,
    slots: &BTreeMap<PortKey, PortSlot>,
    source_bindings: &[ExternalBinding],
) -> Result<LoweredKernels, RenderPlanError> {
    Ok(LoweredKernels {
        sources: source_kernel(node, AUDIO_PORT_ID, slots, source_bindings)?
            .into_iter()
            .collect(),
        processing: vec![],
        sinks: vec![],
    })
}

fn source_kernel(
    node: &CompiledNode,
    port_id: &str,
    slots: &BTreeMap<PortKey, PortSlot>,
    bindings: &[ExternalBinding],
) -> Result<Option<Kernel>, RenderPlanError> {
    let port_id = PortId::from(port_id);
    let output = node_slot(node, &port_id, PortDirection::Output, slots)?;
    let Some(binding) = binding_index(bindings, node.id(), &port_id) else {
        return Ok(None);
    };
    Ok(Some(Kernel::Source { binding, output }))
}

fn sink_kernel(
    node: &CompiledNode,
    port_id: &str,
    slots: &BTreeMap<PortKey, PortSlot>,
    bindings: &[ExternalBinding],
) -> Result<Option<Kernel>, RenderPlanError> {
    let port_id = PortId::from(port_id);
    let input = node_slot(node, &port_id, PortDirection::Input, slots)?;
    let Some(binding) = binding_index(bindings, node.id(), &port_id) else {
        return Ok(None);
    };
    Ok(Some(Kernel::Sink { binding, input }))
}

fn binding_index(
    bindings: &[ExternalBinding],
    node_id: &NodeId,
    port_id: &PortId,
) -> Option<usize> {
    bindings
        .iter()
        .position(|binding| binding.node_id() == node_id && binding.port_id() == port_id)
}

fn node_slot(
    node: &CompiledNode,
    port_id: &PortId,
    direction: PortDirection,
    slots: &BTreeMap<PortKey, PortSlot>,
) -> Result<PortSlot, RenderPlanError> {
    required_slot(
        slots,
        &AudioEndpoint {
            node_id: node.id().clone(),
            port_id: port_id.clone(),
        },
        direction,
    )
}

fn required_slot(
    slots: &BTreeMap<PortKey, PortSlot>,
    endpoint: &AudioEndpoint,
    direction: PortDirection,
) -> Result<PortSlot, RenderPlanError> {
    slots
        .get(&PortKey {
            endpoint: endpoint.clone(),
            direction,
        })
        .copied()
        .ok_or_else(|| RenderPlanError::MissingPort {
            node_id: endpoint.node_id.clone(),
            port_id: endpoint.port_id.clone(),
        })
}

fn validate_source_lengths(
    frames: usize,
    bindings: &[ExternalBinding],
    sources: &[&[f32]],
) -> Result<(), ProcessBlockError> {
    for (index, (binding, samples)) in bindings.iter().zip(sources).enumerate() {
        let required = sample_count(frames, binding.channels);
        if samples.len() < required {
            return Err(ProcessBlockError::SourceBufferTooShort {
                index,
                required,
                found: samples.len(),
            });
        }
    }
    Ok(())
}

fn validate_sink_lengths(
    frames: usize,
    bindings: &[ExternalBinding],
    sinks: &[&mut [f32]],
) -> Result<(), ProcessBlockError> {
    for (index, (binding, samples)) in bindings.iter().zip(sinks).enumerate() {
        let required = sample_count(frames, binding.channels);
        if samples.len() < required {
            return Err(ProcessBlockError::SinkBufferTooShort {
                index,
                required,
                found: samples.len(),
            });
        }
    }
    Ok(())
}

const fn sample_count(frames: usize, channels: u16) -> usize {
    frames * channels as usize
}

#[cfg(test)]
mod tests {
    mod builtin_effects;
    mod compensation;
    mod controls;
    mod dynamics_telemetry;
    mod history_policy;
    mod memory;
    mod memory_pressure;
    mod nonfinite;
    mod provider_buses;
    mod recorder;
    mod scaling;
    use super::*;
    use mixrelay_audio_graph::{
        ApplicationCaptureRequest, ApplicationSelector, AudioDeviceInputConfig,
        AudioDeviceOutputConfig, BuiltInNodeResolver, ChannelLayout, ChannelMapping, ChannelRoute,
        CompileContext, CompileLimits, CompiledPort, DeviceSelector, EdgeId, EdgeSpec, GainConfig,
        GraphCompiler, GraphDocument, MixerConfig, NodeResolutionError, NodeResolver, NodeSpec,
        PortDirection, ProviderNodeConfig, ResolvedNode, SourcePlaybackPolicy, StreamSpec,
        VirtualEndpointId, VirtualEndpointSpec,
    };

    const FORMAT: RenderFormat = RenderFormat {
        sample_rate: 48_000,
        block_frames: 4,
    };

    // Thread-local instrumentation keeps parallel tests out of the measured
    // callback window. All native/plugin preparation stays outside that window.
    std::thread_local! {
        static HEAP_OPERATIONS: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
        static HEAP_BALANCE: std::cell::Cell<Option<HeapBalance>> = const { std::cell::Cell::new(None) };
    }
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    struct HeapBalance {
        allocated_bytes: usize,
        freed_bytes: usize,
        allocations: usize,
        frees: usize,
    }
    fn account_heap_bytes(allocated: Option<usize>, freed: Option<usize>) {
        let _ = HEAP_BALANCE.try_with(|count| {
            if let Some(mut balance) = count.get() {
                if let Some(bytes) = allocated {
                    balance.allocated_bytes += bytes;
                    balance.allocations += 1;
                }
                if let Some(bytes) = freed {
                    balance.freed_bytes += bytes;
                    balance.frees += 1;
                }
                count.set(Some(balance));
            }
        });
    }
    struct CountedAllocator;
    fn count_heap_operation() {
        let _ = HEAP_OPERATIONS.try_with(|count| {
            if let Some(value) = count.get() {
                count.set(Some(value + 1));
            }
        });
    }
    // SAFETY: ownership, alignment and sizes are delegated unchanged to System.
    unsafe impl std::alloc::GlobalAlloc for CountedAllocator {
        unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
            count_heap_operation();
            let pointer = unsafe { std::alloc::System.alloc(layout) };
            if !pointer.is_null() {
                account_heap_bytes(Some(layout.size()), None);
            }
            pointer
        }
        unsafe fn dealloc(&self, pointer: *mut u8, layout: std::alloc::Layout) {
            count_heap_operation();
            account_heap_bytes(None, Some(layout.size()));
            unsafe { std::alloc::System.dealloc(pointer, layout) };
        }
        unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
            count_heap_operation();
            let pointer = unsafe { std::alloc::System.alloc_zeroed(layout) };
            if !pointer.is_null() {
                account_heap_bytes(Some(layout.size()), None);
            }
            pointer
        }
        unsafe fn realloc(
            &self,
            pointer: *mut u8,
            layout: std::alloc::Layout,
            size: usize,
        ) -> *mut u8 {
            count_heap_operation();
            let replacement = unsafe { std::alloc::System.realloc(pointer, layout, size) };
            if !replacement.is_null() {
                account_heap_bytes(Some(size), Some(layout.size()));
            }
            replacement
        }
    }
    #[global_allocator]
    static ALLOCATOR: CountedAllocator = CountedAllocator;

    struct CallbackIo {
        read_frames: u32,
        discontinuity: bool,
        activity: mixrelay_audio_io::process::InputActivity,
        write_frames: u32,
        value: f32,
        output: [f32; 8],
        reads: usize,
        writes: usize,
        observed: Option<ProcessCycle>,
    }

    impl ExternalIo for CallbackIo {
        fn read_source(
            &mut self,
            _: &ExternalBinding,
            destination: &mut [f32],
            cycle: &ProcessCycle,
        ) -> TransferOutcome {
            self.reads += 1;
            self.observed = Some(*cycle);
            destination.fill(self.value);
            TransferOutcome {
                activity: self.activity,
                discontinuity: self.discontinuity,
                ..TransferOutcome::complete(self.read_frames)
            }
        }
        fn write_sink(
            &mut self,
            _: &ExternalBinding,
            source: &[f32],
            cycle: &ProcessCycle,
        ) -> TransferOutcome {
            self.writes += 1;
            assert_eq!(self.observed, Some(*cycle));
            self.output[..source.len()].copy_from_slice(source);
            TransferOutcome::complete(self.write_frames)
        }
    }

    fn callback_fixture() -> (BlockProcessor, CallbackIo, ProcessCycle) {
        let execution = compile(
            vec![virtual_endpoint("io")],
            vec![edge("loop", "io", FROM_OS_PORT_ID, "io", TO_OS_PORT_ID)],
        );
        let processor = BlockProcessor::new(Arc::new(RenderPlan::prepare(&execution).unwrap()));
        let io = CallbackIo {
            read_frames: 4,
            discontinuity: false,
            activity: mixrelay_audio_io::process::InputActivity::Active,
            write_frames: 4,
            value: 0.0,
            output: [99.0; 8],
            reads: 0,
            writes: 0,
            observed: None,
        };
        let cycle = ProcessCycle {
            clock: mixrelay_audio_io::process::ClockDomain {
                authority: mixrelay_audio_io::session::ClockAuthority::Device,
                id: 1,
                epoch: 1,
            },
            frames: 4,
            sample_rate: 48_000,
            first_frame: 0,
            monotonic_ns: Some(123),
            discontinuity: false,
        };
        (processor, io, cycle)
    }

    #[test]
    fn device_snapshot_never_replays_pcm_from_a_discontinued_input_timeline() {
        for (clock_restart, source_restart) in [(false, false), (true, false), (false, true)] {
            let execution = compile(
                vec![
                    device_input("in"),
                    node("bus", NodeConfig::DeviceBus { channels: 2 }),
                    device_output("out"),
                ],
                vec![
                    edge("publish", "in", AUDIO_PORT_ID, "bus", AUDIO_PORT_ID),
                    edge("read", "bus", AUDIO_PORT_ID, "out", AUDIO_PORT_ID),
                ],
            );
            let mut processor =
                BlockProcessor::new(Arc::new(RenderPlan::prepare(&execution).unwrap()));
            let (_, mut io, mut cycle) = callback_fixture();
            io.value = 1.0;
            processor.process_external_cycle(cycle, &mut io).unwrap();
            assert_eq!(io.output.map(f32::to_bits), [0.0_f32; 8].map(f32::to_bits));
            cycle.first_frame += 4;
            if clock_restart {
                cycle.clock.epoch += 1;
                cycle.discontinuity = true;
            }
            io.value = 0.0;
            io.discontinuity = source_restart;
            HEAP_OPERATIONS.with(|count| count.set(Some(0)));
            processor.process_external_cycle(cycle, &mut io).unwrap();
            assert_eq!(HEAP_OPERATIONS.with(|count| count.replace(None)), Some(0));
            let expected = if clock_restart || source_restart {
                0.0_f32
            } else {
                1.0
            };
            assert_eq!(io.output.map(f32::to_bits), [expected; 8].map(f32::to_bits));
        }
    }

    #[test]
    fn only_explicit_active_demand_counts_missing_input_over_375_cycles() {
        use mixrelay_audio_io::process::InputActivity;
        for (activity, read_frames, expected) in [
            (InputActivity::Unknown, 0, 0),
            (InputActivity::Inactive, 0, 0),
            (InputActivity::Priming, 0, 0),
            (InputActivity::Active, 0, 375),
            (InputActivity::Active, 4, 0),
        ] {
            let (mut processor, mut io, cycle) = callback_fixture();
            io.activity = activity;
            io.read_frames = read_frames;
            let mut before = 0;
            let mut after = 0;
            for index in 0..375 {
                let report = processor
                    .process_external_cycle(
                        ProcessCycle {
                            first_frame: index * u64::from(cycle.frames),
                            ..cycle
                        },
                        &mut io,
                    )
                    .unwrap()
                    .expect("fixture admits every valid cycle");
                before += u64::from(read_frames < cycle.frames);
                after += report.input_starvations;
                assert_eq!(report.output_overflows, 0);
                assert_eq!(io.output.map(f32::to_bits), [0.0_f32; 8].map(f32::to_bits));
            }
            assert_eq!(after, expected, "activity={activity:?}");
            assert_eq!(before, if read_frames == 0 { 375 } else { 0 });
            eprintln!(
                "activity={activity:?} frames={read_frames}: previous={before}, current={after}"
            );
        }
    }

    #[test]
    fn silent_callback_is_success_but_short_transfers_are_reported() {
        let (mut processor, mut io, cycle) = callback_fixture();
        assert_eq!(
            processor.process_external_cycle(cycle, &mut io).unwrap(),
            Some(ProcessReport::default())
        );
        assert_eq!(io.output.map(f32::to_bits), [0.0_f32; 8].map(f32::to_bits));
        io.read_frames = 2;
        io.write_frames = 3;
        io.value = 1.0;
        let next = ProcessCycle {
            first_frame: 4,
            ..cycle
        };
        let report = processor
            .process_external_cycle(next, &mut io)
            .unwrap()
            .expect("fixture admits every valid cycle");
        assert_eq!(report.input_starvations, 1);
        assert_eq!(report.output_overflows, 1);
        assert_eq!(
            io.output.map(f32::to_bits),
            [1.0_f32, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0].map(f32::to_bits)
        );
    }

    #[test]
    fn bad_clock_or_format_never_touches_native_io() {
        let (mut processor, mut io, cycle) = callback_fixture();
        for bad in [
            ProcessCycle {
                sample_rate: 44_100,
                ..cycle
            },
            ProcessCycle { frames: 8, ..cycle },
            ProcessCycle { frames: 0, ..cycle },
        ] {
            assert!(processor.process_external_cycle(bad, &mut io).is_err());
        }
        assert_eq!((io.reads, io.writes), (0, 0));
        processor.process_external_cycle(cycle, &mut io).unwrap();
        assert!(processor.process_external_cycle(cycle, &mut io).is_err());
        assert_eq!((io.reads, io.writes), (1, 1));
    }

    #[test]
    fn oversized_transfer_is_an_adapter_contract_failure() {
        let (mut processor, mut io, cycle) = callback_fixture();
        io.read_frames = 5;
        assert_eq!(
            processor.process_external_cycle(cycle, &mut io),
            Err(ProcessBlockError::InvalidTransfer)
        );
        assert_eq!(io.writes, 0);
    }

    #[test]
    fn validated_audio_cycles_do_not_allocate_or_free() {
        let (mut processor, mut io, mut cycle) = callback_fixture();
        HEAP_OPERATIONS.with(|count| count.set(Some(0)));
        for _ in 0..10_000 {
            processor.process_external_cycle(cycle, &mut io).unwrap();
            cycle.first_frame += u64::from(cycle.frames);
        }
        let operations = HEAP_OPERATIONS.with(|count| count.replace(None));
        assert_eq!(operations, Some(0));
    }

    fn compile(nodes: Vec<NodeSpec>, edges: Vec<EdgeSpec>) -> ExecutionPlan {
        GraphCompiler::default()
            .compile(
                &GraphDocument::new(1, nodes, edges),
                CompileContext {
                    format: FORMAT,
                    limits: CompileLimits::default(),
                },
            )
            .expect("test graph should compile")
    }

    fn node(id: &str, config: NodeConfig) -> NodeSpec {
        NodeSpec {
            id: NodeId::from(id),
            label: None,
            control: mixrelay_audio_graph::NodeControl::default(),
            config,
        }
    }

    fn endpoint(node_id: &str, port_id: &str) -> AudioEndpoint {
        AudioEndpoint {
            node_id: NodeId::from(node_id),
            port_id: PortId::from(port_id),
        }
    }

    fn edge(
        id: &str,
        source_node: &str,
        source_port: &str,
        destination_node: &str,
        destination_port: &str,
    ) -> EdgeSpec {
        EdgeSpec {
            id: EdgeId::from(id),
            source: endpoint(source_node, source_port),
            destination: endpoint(destination_node, destination_port),
            channel_mapping: ChannelMapping::Auto,
        }
    }

    fn stereo_stream() -> StreamSpec {
        StreamSpec {
            channels: 2,
            channel_layout: ChannelLayout::Stereo,
        }
    }

    fn app_source(id: &str) -> NodeSpec {
        node(
            id,
            NodeConfig::ApplicationCapture(ApplicationCaptureRequest {
                selector: ApplicationSelector::BundleId {
                    bundle_id: format!("io.mixrelay.{id}"),
                    include_descendants: true,
                },
                stream: stereo_stream(),
                source_playback: SourcePlaybackPolicy::Preserve,
                forwarding_endpoint: None,
                gain_db: 0.0,
                requires_reselection: false,
            }),
        )
    }

    fn device_input(id: &str) -> NodeSpec {
        node(
            id,
            NodeConfig::AudioDeviceInput(AudioDeviceInputConfig {
                selector: DeviceSelector::DefaultInput,
                output_channels: 2,
                gain_db: 0.0,
            }),
        )
    }

    fn device_output(id: &str) -> NodeSpec {
        node(
            id,
            NodeConfig::AudioDeviceOutput(AudioDeviceOutputConfig {
                selector: DeviceSelector::DefaultOutput,
                input_channels: 2,
            }),
        )
    }

    fn virtual_endpoint(id: &str) -> NodeSpec {
        node(
            id,
            NodeConfig::VirtualEndpointTransport(VirtualEndpointSpec {
                id: VirtualEndpointId::from(id),
                display_name: id.into(),
                render: Some(stereo_stream()),
                capture: Some(stereo_stream()),
                supported_sample_rates: vec![48_000],
                default_sample_rate: 48_000,
            }),
        )
    }

    #[test]
    fn external_bindings_keep_device_identity_separate_from_graph_node_names() {
        let mut endpoint = virtual_endpoint("canonical-device");
        endpoint.id = NodeId::from("graph-b::local-reference");
        let execution = compile(
            vec![endpoint, app_source("application"), device_input("mic")],
            vec![],
        );
        let plan = RenderPlan::prepare(&execution).expect("valid independent references");
        for binding in plan.source_bindings().iter().chain(plan.sink_bindings()) {
            let expected = (binding.node_id().as_str() == "graph-b::local-reference")
                .then_some("canonical-device");
            assert_eq!(
                binding.virtual_endpoint_id().map(VirtualEndpointId::as_str),
                expected
            );
        }
    }

    #[test]
    fn native_transport_bindings_keep_asymmetric_device_geometry() {
        let mut endpoint = virtual_endpoint("device");
        if let NodeConfig::VirtualEndpointTransport(spec) = &mut endpoint.config {
            spec.capture = Some(StreamSpec {
                channels: 16,
                channel_layout: ChannelLayout::Discrete,
            });
        }
        let plan = RenderPlan::prepare(&compile(vec![endpoint], vec![])).unwrap();
        assert_eq!(plan.source_bindings()[0].channels(), 16);
        assert_eq!(plan.sink_bindings()[0].channels(), 2);
    }

    #[test]
    fn authored_virtual_source_must_be_composed_before_rendering() {
        let mut endpoint = virtual_endpoint("device");
        if let NodeConfig::VirtualEndpointTransport(spec) = &endpoint.config {
            endpoint.config = NodeConfig::VirtualEndpoint(spec.clone().into());
        }
        let execution = compile(vec![endpoint], vec![]);
        assert!(matches!(RenderPlan::prepare(&execution),
            Err(RenderPlanError::UnloweredVirtualEndpoint {node_id}) if node_id.as_str()=="device"));
    }

    #[test]
    fn device_bus_preserves_pcm_across_unequal_callback_lengths() {
        for channels in [1, 2, 8, 32, 64] {
            let mut processor = device_bus_processor(channels);
            let channels = usize::from(channels);
            let mut input = vec![0.0; 4 * channels];
            let mut output = vec![0.0; input.len()];
            let mut position = 0;
            let tag = |sample| f32::from(u16::try_from(sample % 16_384 + 1).unwrap());
            for frames in [1, 2, 4, 1, 3, 2, 2, 1, 4].into_iter().cycle().take(128) {
                for (sample, value) in input.iter_mut().enumerate() {
                    *value = tag(position * channels + sample);
                }
                HEAP_OPERATIONS.with(|count| count.set(Some(0)));
                processor
                    .process_block(frames, &[&input], &mut [&mut output])
                    .unwrap();
                assert_eq!(HEAP_OPERATIONS.with(|count| count.replace(None)), Some(0));
                for (sample, actual) in output.iter().enumerate().take(frames as usize * channels) {
                    let absolute = position * channels + sample;
                    let expected = absolute.checked_sub(4 * channels).map_or(0.0, tag);
                    assert_eq!(
                        actual.to_bits(),
                        expected.to_bits(),
                        "channels={channels}, position={position}, sample={sample}"
                    );
                }
                position += frames as usize;
            }
        }
    }

    fn device_bus_processor(channels: u16) -> BlockProcessor {
        let execution = compile(
            vec![
                node(
                    "in",
                    NodeConfig::AudioDeviceInput(AudioDeviceInputConfig {
                        selector: DeviceSelector::DefaultInput,
                        output_channels: channels,
                        gain_db: 0.0,
                    }),
                ),
                node("bus", NodeConfig::DeviceBus { channels }),
                node(
                    "out",
                    NodeConfig::AudioDeviceOutput(AudioDeviceOutputConfig {
                        selector: DeviceSelector::DefaultOutput,
                        input_channels: channels,
                    }),
                ),
            ],
            vec![
                edge("publish", "in", AUDIO_PORT_ID, "bus", AUDIO_PORT_ID),
                edge("read", "bus", AUDIO_PORT_ID, "out", AUDIO_PORT_ID),
            ],
        );
        BlockProcessor::new(Arc::new(RenderPlan::prepare(&execution).unwrap()))
    }

    #[test]
    fn device_bus_replacement_retains_unread_tail_and_ring_position() {
        let mut previous = device_bus_processor(1);
        let mut output = [0.0; 4];
        previous
            .process_block(4, &[&[1.0, 2.0, 3.0, 4.0]], &mut [&mut output])
            .unwrap();
        previous
            .process_block(1, &[&[5.0]], &mut [&mut output])
            .unwrap();
        assert_eq!(output[0].to_bits(), 1.0_f32.to_bits());
        let mut next = device_bus_processor(1);
        HEAP_OPERATIONS.with(|count| count.set(Some(0)));
        next.retain_device_bus_state(&previous);
        next.process_block(4, &[&[6.0, 7.0, 8.0, 9.0]], &mut [&mut output])
            .unwrap();
        assert_eq!(HEAP_OPERATIONS.with(|count| count.replace(None)), Some(0));
        assert_eq!(
            output.map(f32::to_bits),
            [2.0_f32, 3.0, 4.0, 5.0].map(f32::to_bits)
        );
    }

    struct IndependentBusIo {
        source_a: [f32; 8],
        source_b: [f32; 8],
        output_a: [f32; 8],
        output_b: [f32; 8],
        source_a_discontinuity: bool,
        admission: CycleAdmission,
    }

    impl ExternalIo for IndependentBusIo {
        fn begin_cycle(
            &mut self,
            _cycle: &ProcessCycle,
            _controls: ExternalControlState<'_>,
        ) -> CycleAdmission {
            self.admission
        }

        fn read_source(
            &mut self,
            binding: &ExternalBinding,
            destination: &mut [f32],
            cycle: &ProcessCycle,
        ) -> TransferOutcome {
            let (source, discontinuity) = match binding.node_id().as_str() {
                "source-a" => (&self.source_a, self.source_a_discontinuity),
                "source-b" => (&self.source_b, false),
                other => panic!("unexpected test source binding {other}"),
            };
            destination.copy_from_slice(&source[..destination.len()]);
            TransferOutcome {
                discontinuity,
                ..TransferOutcome::complete(cycle.frames)
            }
        }

        fn write_sink(
            &mut self,
            binding: &ExternalBinding,
            source: &[f32],
            _cycle: &ProcessCycle,
        ) -> TransferOutcome {
            let destination = match binding.node_id().as_str() {
                "sink-a" => &mut self.output_a,
                "sink-b" => &mut self.output_b,
                other => panic!("unexpected test sink binding {other}"),
            };
            destination[..source.len()].copy_from_slice(source);
            TransferOutcome::complete(u32::try_from(source.len() / 2).unwrap())
        }
    }

    fn independent_bus_fixture() -> (BlockProcessor, IndependentBusIo, ProcessCycle) {
        let execution = compile(
            vec![
                device_input("source-a"),
                node("bus-a", NodeConfig::DeviceBus { channels: 2 }),
                device_output("sink-a"),
                device_input("source-b"),
                node("bus-b", NodeConfig::DeviceBus { channels: 2 }),
                device_output("sink-b"),
            ],
            vec![
                edge(
                    "publish-a",
                    "source-a",
                    AUDIO_PORT_ID,
                    "bus-a",
                    AUDIO_PORT_ID,
                ),
                edge("read-a", "bus-a", AUDIO_PORT_ID, "sink-a", AUDIO_PORT_ID),
                edge(
                    "publish-b",
                    "source-b",
                    AUDIO_PORT_ID,
                    "bus-b",
                    AUDIO_PORT_ID,
                ),
                edge("read-b", "bus-b", AUDIO_PORT_ID, "sink-b", AUDIO_PORT_ID),
            ],
        );
        (
            BlockProcessor::new(Arc::new(RenderPlan::prepare(&execution).unwrap())),
            IndependentBusIo {
                source_a: [1.0; 8],
                source_b: [2.0; 8],
                output_a: [0.0; 8],
                output_b: [0.0; 8],
                source_a_discontinuity: false,
                admission: CycleAdmission::Process,
            },
            ProcessCycle {
                clock: mixrelay_audio_io::process::ClockDomain {
                    authority: mixrelay_audio_io::session::ClockAuthority::Device,
                    id: 1,
                    epoch: 0,
                },
                sample_rate: FORMAT.sample_rate,
                frames: FORMAT.block_frames,
                first_frame: 0,
                monotonic_ns: None,
                discontinuity: false,
            },
        )
    }

    #[test]
    fn one_source_discontinuity_does_not_clear_unrelated_device_bus_history() {
        let (mut processor, mut io, mut cycle) = independent_bus_fixture();
        processor.process_external_cycle(cycle, &mut io).unwrap();
        cycle.first_frame += u64::from(cycle.frames);
        processor.process_external_cycle(cycle, &mut io).unwrap();
        assert_eq!(io.output_a.map(f32::to_bits), [1.0_f32.to_bits(); 8]);
        assert_eq!(io.output_b.map(f32::to_bits), [2.0_f32.to_bits(); 8]);

        cycle.first_frame += u64::from(cycle.frames);
        io.source_a_discontinuity = true;
        HEAP_OPERATIONS.with(|count| count.set(Some(0)));
        let report = processor
            .process_external_cycle(cycle, &mut io)
            .unwrap()
            .unwrap();
        assert_eq!(HEAP_OPERATIONS.with(|count| count.replace(None)), Some(0));
        assert_eq!(report.discontinuities, 1);
        assert_eq!(io.output_a.map(f32::to_bits), [0.0_f32.to_bits(); 8]);
        assert_eq!(io.output_b.map(f32::to_bits), [2.0_f32.to_bits(); 8]);
    }

    #[test]
    fn lossless_clock_handoff_preserves_unrelated_bus_history_without_allocation() {
        for source_a_discontinuity in [false, true] {
            let (mut processor, mut io, mut cycle) = independent_bus_fixture();
            processor.process_external_cycle(cycle, &mut io).unwrap();
            cycle.first_frame += u64::from(cycle.frames);
            processor.process_external_cycle(cycle, &mut io).unwrap();

            io.source_a_discontinuity = source_a_discontinuity;
            io.admission = CycleAdmission::ClockHandoff;
            cycle.clock.id = 2;
            cycle.first_frame = 960;
            cycle.discontinuity = true;
            HEAP_OPERATIONS.with(|count| count.set(Some(0)));
            processor.process_external_cycle(cycle, &mut io).unwrap();
            assert_eq!(HEAP_OPERATIONS.with(|count| count.replace(None)), Some(0));
            assert_eq!(io.output_b.map(f32::to_bits), [2.0_f32.to_bits(); 8]);
            let expected_a = if source_a_discontinuity {
                0.0_f32
            } else {
                1.0_f32
            };
            assert_eq!(io.output_a.map(f32::to_bits), [expected_a.to_bits(); 8]);

            // Retaining PCM history must still commit the new clock coordinates.
            io.admission = CycleAdmission::Process;
            io.source_a_discontinuity = false;
            cycle.first_frame += u64::from(cycle.frames);
            cycle.discontinuity = false;
            processor.process_external_cycle(cycle, &mut io).unwrap();
            assert_eq!(io.output_b.map(f32::to_bits), [2.0_f32.to_bits(); 8]);
        }
    }

    #[test]
    fn global_clock_discontinuity_clears_both_independent_bus_snapshots() {
        let (mut processor, mut io, mut cycle) = independent_bus_fixture();
        processor.process_external_cycle(cycle, &mut io).unwrap();
        cycle.first_frame += u64::from(cycle.frames);
        processor.process_external_cycle(cycle, &mut io).unwrap();
        assert_eq!(io.output_a.map(f32::to_bits), [1.0_f32.to_bits(); 8]);
        assert_eq!(io.output_b.map(f32::to_bits), [2.0_f32.to_bits(); 8]);

        cycle.first_frame += u64::from(cycle.frames);
        cycle.clock.epoch += 1;
        cycle.discontinuity = true;
        processor.process_external_cycle(cycle, &mut io).unwrap();
        assert_eq!(io.output_a.map(f32::to_bits), [0.0_f32.to_bits(); 8]);
        assert_eq!(io.output_b.map(f32::to_bits), [0.0_f32.to_bits(); 8]);
    }

    #[test]
    fn local_source_restart_retains_other_bus_partial_quantum_position() {
        let (mut processor, mut io, mut cycle) = independent_bus_fixture();
        io.source_b = [2.0, 2.0, 3.0, 3.0, 4.0, 4.0, 5.0, 5.0];
        processor.process_external_cycle(cycle, &mut io).unwrap();
        cycle.first_frame += 4;
        cycle.frames = 2;
        processor.process_external_cycle(cycle, &mut io).unwrap();
        assert_eq!(
            io.output_b.map(f32::to_bits)[..4],
            [2.0_f32, 2.0, 3.0, 3.0].map(f32::to_bits)
        );

        cycle.first_frame += 2;
        io.source_a_discontinuity = true;
        HEAP_OPERATIONS.with(|count| count.set(Some(0)));
        processor.process_external_cycle(cycle, &mut io).unwrap();
        assert_eq!(HEAP_OPERATIONS.with(|count| count.replace(None)), Some(0));
        assert_eq!(
            io.output_b.map(f32::to_bits)[..4],
            [4.0_f32, 4.0, 5.0, 5.0].map(f32::to_bits)
        );
    }

    #[test]
    fn source_history_scope_follows_causal_cycles_without_reaching_other_branches() {
        let execution = compile(
            vec![
                device_input("source-a"),
                device_input("source-b"),
                node("bus-a", NodeConfig::DeviceBus { channels: 2 }),
                node("cycle-a", NodeConfig::DeviceBus { channels: 2 }),
                node("bus-b", NodeConfig::DeviceBus { channels: 2 }),
            ],
            vec![
                edge("a", "source-a", AUDIO_PORT_ID, "bus-a", AUDIO_PORT_ID),
                edge("forward", "bus-a", AUDIO_PORT_ID, "cycle-a", AUDIO_PORT_ID),
                edge("feedback", "cycle-a", AUDIO_PORT_ID, "bus-a", AUDIO_PORT_ID),
                edge("b", "source-b", AUDIO_PORT_ID, "bus-b", AUDIO_PORT_ID),
            ],
        );
        let plan = RenderPlan::prepare(&execution).unwrap();
        for (binding, targets) in plan.source_bindings.iter().zip(&plan.source_history) {
            let expected = match binding.node_id().as_str() {
                "source-a" => (2, 3),
                "source-b" => (1, 1),
                other => panic!("unexpected source {other}"),
            };
            assert_eq!(targets.device_buses.len(), expected.0);
            assert_eq!(targets.delay_routes.len(), expected.1);
        }
    }

    #[test]
    fn source_history_does_not_cross_independent_duplex_transport_directions() {
        let execution = compile(
            vec![
                device_input("source-a"),
                virtual_endpoint("duplex-b"),
                node("bus-b", NodeConfig::DeviceBus { channels: 2 }),
                device_output("sink-b"),
            ],
            vec![
                edge(
                    "a-sink",
                    "source-a",
                    AUDIO_PORT_ID,
                    "duplex-b",
                    TO_OS_PORT_ID,
                ),
                edge(
                    "b-source",
                    "duplex-b",
                    FROM_OS_PORT_ID,
                    "bus-b",
                    AUDIO_PORT_ID,
                ),
                edge("b-sink", "bus-b", AUDIO_PORT_ID, "sink-b", AUDIO_PORT_ID),
            ],
        );
        let plan = RenderPlan::prepare(&execution).unwrap();
        for (binding, targets) in plan.source_bindings.iter().zip(&plan.source_history) {
            let expected = match binding.node_id().as_str() {
                "source-a" => (0, 1),
                "duplex-b" => (1, 2),
                other => panic!("unexpected source {other}"),
            };
            assert_eq!(targets.device_buses.len(), expected.0);
            assert_eq!(targets.delay_routes.len(), expected.1);
        }
    }

    #[test]
    fn device_bus_has_one_prepared_quantum_latency_without_delaying_direct_hardware() {
        let nodes = vec![
            device_input("mic"),
            device_output("out"),
            node("bus", NodeConfig::DeviceBus { channels: 2 }),
            node(
                "gain",
                NodeConfig::Gain(GainConfig {
                    channels: 2,
                    gain_db: -6.020_600_3,
                }),
            ),
        ];
        let edges = vec![
            edge("publish", "mic", AUDIO_PORT_ID, "bus", AUDIO_PORT_ID),
            edge("snapshot", "bus", AUDIO_PORT_ID, "gain", AUDIO_PORT_ID),
            edge("direct", "mic", AUDIO_PORT_ID, "out", AUDIO_PORT_ID),
            edge("read", "gain", AUDIO_PORT_ID, "out", AUDIO_PORT_ID),
        ];
        let plan = Arc::new(RenderPlan::prepare(&compile(nodes, edges)).unwrap());
        let mut processor = BlockProcessor::new(plan);
        let first = [0.2, 0.4, 0.6, 0.8, 0.1, 0.3, 0.5, 0.7];
        let second = [1.0; 8];
        let mut output = [0.0; 8];
        HEAP_OPERATIONS.with(|count| count.set(Some(0)));
        processor
            .process_block(4, &[&first], &mut [&mut output])
            .unwrap();
        assert_eq!(output.map(f32::to_bits), first.map(f32::to_bits));
        processor
            .process_block(1, &[&second], &mut [&mut output])
            .unwrap();
        assert!((output[0] - 1.1).abs() < 0.000_001);
        assert!((output[1] - 1.2).abs() < 0.000_001);
        processor
            .process_block(3, &[&second], &mut [&mut output])
            .unwrap();
        for (actual, expected) in output.iter().zip([1.3, 1.4, 1.05, 1.15, 1.25, 1.35]) {
            assert!((actual - expected).abs() < 0.000_001);
        }
        assert_eq!(HEAP_OPERATIONS.with(|count| count.replace(None)), Some(0));
    }

    fn processor_graph() -> ExecutionPlan {
        let nodes = vec![
            app_source("app"),
            device_input("mic"),
            node(
                "mix",
                NodeConfig::Mixer(MixerConfig {
                    inputs: vec![PortId::from("app"), PortId::from("mic")],
                    channels: 2,
                }),
            ),
            node(
                "gain",
                NodeConfig::Gain(GainConfig {
                    channels: 2,
                    gain_db: -6.020_600_3,
                }),
            ),
            virtual_endpoint("virtual"),
            device_output("monitor"),
        ];
        let mut swapped = edge(
            "virtual-monitor",
            "virtual",
            FROM_OS_PORT_ID,
            "monitor",
            AUDIO_PORT_ID,
        );
        swapped.channel_mapping = ChannelMapping::Explicit {
            routes: vec![
                ChannelRoute {
                    source_channel: 0,
                    destination_channel: 1,
                },
                ChannelRoute {
                    source_channel: 1,
                    destination_channel: 0,
                },
            ],
        };
        compile(
            nodes,
            vec![
                edge("app-mix", "app", AUDIO_PORT_ID, "mix", "app"),
                edge("mic-mix", "mic", AUDIO_PORT_ID, "mix", "mic"),
                edge("mix-gain", "mix", AUDIO_PORT_ID, "gain", AUDIO_PORT_ID),
                edge(
                    "gain-virtual",
                    "gain",
                    AUDIO_PORT_ID,
                    "virtual",
                    TO_OS_PORT_ID,
                ),
                swapped,
            ],
        )
    }

    #[test]
    fn lowers_bindings_and_processes_gain_mix_and_channel_routes() {
        let plan = Arc::new(RenderPlan::prepare(&processor_graph()).expect("plan should lower"));
        assert_eq!(plan.revision(), 1);
        assert_eq!(plan.format(), FORMAT);
        assert_eq!(plan.arena_samples(), 80);
        assert_eq!(
            plan.source_bindings()
                .iter()
                .map(|binding| (
                    binding.node_id().as_str(),
                    binding.port_id().as_str(),
                    binding.channels()
                ))
                .collect::<Vec<_>>(),
            [
                ("app", AUDIO_PORT_ID, 2),
                ("mic", AUDIO_PORT_ID, 2),
                ("virtual", FROM_OS_PORT_ID, 2),
            ]
        );
        assert_eq!(
            plan.sink_bindings()
                .iter()
                .map(|binding| (binding.node_id().as_str(), binding.port_id().as_str()))
                .collect::<Vec<_>>(),
            [("monitor", AUDIO_PORT_ID), ("virtual", TO_OS_PORT_ID)]
        );

        let app = [1.0, 2.0, 3.0, 4.0];
        let mic = [0.5, 0.5, 0.5, 0.5];
        let from_os = [10.0, 20.0, 30.0, 40.0];
        let sources: [&[f32]; 3] = [&app, &mic, &from_os];
        let mut to_os = [0.0; 4];
        let mut monitor = [0.0; 4];
        let mut sinks: [&mut [f32]; 2] = [&mut monitor, &mut to_os];
        let mut processor = BlockProcessor::new(Arc::clone(&plan));
        assert!(Arc::ptr_eq(processor.plan(), &plan));
        processor
            .process_block(2, &sources, &mut sinks)
            .expect("block should process");

        for (actual, expected) in monitor.into_iter().zip([20.0, 10.0, 40.0, 30.0]) {
            assert!((actual - expected).abs() < 0.000_001);
        }
        for (actual, expected) in to_os.into_iter().zip([0.75, 1.25, 1.75, 2.25]) {
            assert!((actual - expected).abs() < 0.000_001);
        }
    }

    struct TestProviderResolver;

    impl NodeResolver for TestProviderResolver {
        fn resolve(
            &self,
            node: &NodeSpec,
            context: &CompileContext,
        ) -> Result<ResolvedNode, NodeResolutionError> {
            if matches!(node.config, NodeConfig::Provider(_)) {
                return Ok(ResolvedNode {
                    inputs: vec![CompiledPort::new(
                        PortId::from(AUDIO_PORT_ID),
                        PortDirection::Input,
                        2,
                    )],
                    outputs: vec![CompiledPort::new(
                        PortId::from(AUDIO_PORT_ID),
                        PortDirection::Output,
                        2,
                    )],
                });
            }
            BuiltInNodeResolver.resolve(node, context)
        }
    }

    struct DoubleProcessor;

    impl HostedAudioProcessor for DoubleProcessor {
        fn process(&mut self, input: &[f32], output: &mut [f32], frames: usize) -> bool {
            let samples = frames * 2;
            for index in 0..samples {
                output[index] = input[index] * 2.0;
            }
            true
        }
    }

    struct TestProviderFactory;

    impl HostedProcessorFactory for TestProviderFactory {
        fn prepare(
            &self,
            _request: HostedProcessorPrepareRequest<'_>,
        ) -> Result<Box<dyn HostedAudioProcessor>, String> {
            Ok(Box::new(DoubleProcessor))
        }
    }

    #[test]
    fn hosted_provider_uses_prepared_instance_in_the_render_plan() {
        let document = GraphDocument::new(
            1,
            vec![
                app_source("app"),
                node(
                    "provider",
                    NodeConfig::Provider(ProviderNodeConfig {
                        ports: None,
                        type_id: mixrelay_audio_graph::NodeTypeId::from("test:double"),
                        parameters: serde_json::json!({}),
                    }),
                ),
                device_output("speaker"),
            ],
            vec![
                edge(
                    "app-provider",
                    "app",
                    AUDIO_PORT_ID,
                    "provider",
                    AUDIO_PORT_ID,
                ),
                edge(
                    "provider-speaker",
                    "provider",
                    AUDIO_PORT_ID,
                    "speaker",
                    AUDIO_PORT_ID,
                ),
            ],
        );
        let execution = GraphCompiler::new(TestProviderResolver)
            .compile(
                &document,
                CompileContext {
                    format: FORMAT,
                    limits: CompileLimits::default(),
                },
            )
            .expect("provider graph should compile");
        let meters = Arc::new(crate::PeakMeterBank::new(8, 8));
        let mut processor = BlockProcessor::prepare_with_hosted_processors(
            &execution,
            meters,
            &TestProviderFactory,
        )
        .expect("provider should prepare");
        let source = [1.0, -1.0, 0.25, -0.25];
        let mut sink = [0.0; 4];
        processor
            .process_block(2, &[&source], &mut [&mut sink])
            .expect("provider block should process");
        for (actual, expected) in sink.iter().zip([2.0, -2.0, 0.5, -0.5]) {
            assert!((actual - expected).abs() <= f32::EPSILON);
        }
    }

    #[test]
    fn processes_a_duplex_virtual_endpoint_through_gain_without_a_false_cycle() {
        let execution = compile(
            vec![
                virtual_endpoint("virtual"),
                node(
                    "gain",
                    NodeConfig::Gain(GainConfig {
                        channels: 2,
                        gain_db: -6.020_600_3,
                    }),
                ),
            ],
            vec![
                edge(
                    "from-os-to-gain",
                    "virtual",
                    FROM_OS_PORT_ID,
                    "gain",
                    AUDIO_PORT_ID,
                ),
                edge(
                    "gain-to-os",
                    "gain",
                    AUDIO_PORT_ID,
                    "virtual",
                    TO_OS_PORT_ID,
                ),
            ],
        );
        let plan = Arc::new(RenderPlan::prepare(&execution).expect("gain path should lower"));
        let mut processor = BlockProcessor::new(plan);
        let source = [1.0, -1.0, 0.5, -0.5];
        let mut sink = [0.0; 4];

        processor
            .process_block(2, &[&source], &mut [&mut sink])
            .expect("gain path should process");

        for (actual, expected) in sink.into_iter().zip([0.5, -0.5, 0.25, -0.25]) {
            assert!((actual - expected).abs() < 0.000_001);
        }
    }

    #[test]
    fn virtual_source_gain_scales_pcm_and_both_meter_banks_once_without_allocations() {
        for channels in [2_u16, 8, 16, 32, 64] {
            let mut endpoint = virtual_endpoint("virtual");
            let NodeConfig::VirtualEndpointTransport(config) = &mut endpoint.config else {
                unreachable!();
            };
            let stream = StreamSpec {
                channels,
                channel_layout: ChannelLayout::Discrete,
            };
            config.render = Some(stream);
            config.capture = Some(stream);
            let execution = compile(
                vec![
                    endpoint,
                    node(
                        "local-gain",
                        NodeConfig::Gain(GainConfig {
                            channels,
                            gain_db: -6.020_600_3,
                        }),
                    ),
                ],
                vec![
                    edge(
                        "read",
                        "virtual",
                        FROM_OS_PORT_ID,
                        "local-gain",
                        AUDIO_PORT_ID,
                    ),
                    edge(
                        "write",
                        "local-gain",
                        AUDIO_PORT_ID,
                        "virtual",
                        TO_OS_PORT_ID,
                    ),
                ],
            );
            let plan = Arc::new(RenderPlan::prepare(&execution).unwrap());
            let meters = Arc::new(crate::PeakMeterBank::new(plan.meter_bindings().len(), 64));
            let mut processor =
                BlockProcessor::with_peak_meters(Arc::clone(&plan), Arc::clone(&meters));
            let source: Vec<f32> = (0..usize::from(channels) * 2)
                .map(|index| if index % 2 == 0 { 1.0 } else { -1.0 })
                .collect();
            let mut sink = vec![0.0; source.len()];
            HEAP_OPERATIONS.with(|count| count.set(Some(0)));
            for _ in 0..100 {
                processor
                    .process_block(2, &[&source], &mut [&mut sink])
                    .unwrap();
            }
            let operations = HEAP_OPERATIONS.with(|count| count.replace(None));
            assert_eq!(operations, Some(0));
            for (input, output) in source.iter().zip(sink) {
                assert!((output - input * 0.5).abs() < 0.000_001);
            }
            for (index, binding) in plan.meter_bindings().iter().enumerate() {
                let expected = if binding.port_id().as_str() == FROM_OS_PORT_ID {
                    1.0
                } else {
                    0.5
                };
                for peak in meters.take_peaks(1, index, channels) {
                    assert!((peak - expected).abs() < 0.000_001);
                }
            }
        }
    }

    #[test]
    fn virtual_source_gain_does_not_attenuate_an_independent_application_capture_path() {
        let mut endpoint = virtual_endpoint("virtual");
        let NodeConfig::VirtualEndpointTransport(config) = &mut endpoint.config else {
            unreachable!();
        };
        let _ = config;
        let execution = compile(
            vec![
                endpoint,
                app_source("app"),
                node(
                    "local-gain",
                    NodeConfig::Gain(GainConfig {
                        channels: 2,
                        gain_db: -12.0,
                    }),
                ),
            ],
            vec![
                edge(
                    "source-gain",
                    "virtual",
                    FROM_OS_PORT_ID,
                    "local-gain",
                    AUDIO_PORT_ID,
                ),
                edge("app-to-os", "app", AUDIO_PORT_ID, "virtual", TO_OS_PORT_ID),
            ],
        );
        let plan = Arc::new(RenderPlan::prepare(&execution).unwrap());
        let app = [1.0; 4];
        let virtual_input = [2.0; 4];
        let sources: Vec<&[f32]> = plan
            .source_bindings()
            .iter()
            .map(|binding| {
                if binding.port_id().as_str() == FROM_OS_PORT_ID {
                    virtual_input.as_slice()
                } else {
                    app.as_slice()
                }
            })
            .collect();
        let mut processor = BlockProcessor::new(plan);
        let mut output = [0.0; 4];
        processor
            .process_block(2, &sources, &mut [&mut output])
            .unwrap();
        for sample in output {
            assert!((sample - 1.0).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn rejects_invalid_block_shapes_before_writing_outputs() {
        let plan = Arc::new(RenderPlan::prepare(&processor_graph()).expect("plan should lower"));
        let mut processor = BlockProcessor::new(plan);
        let full = [0.0; 8];
        let sources: [&[f32]; 3] = [&full, &full, &full];
        let mut first = [9.0; 8];
        let mut second = [9.0; 8];

        assert_eq!(
            processor.process_block(5, &sources, &mut [&mut first, &mut second]),
            Err(ProcessBlockError::FrameCountExceeded {
                maximum: 4,
                found: 5,
            })
        );
        assert_eq!(
            processor.process_block(2, &sources[..2], &mut [&mut first, &mut second]),
            Err(ProcessBlockError::SourceCountMismatch {
                expected: 3,
                found: 2,
            })
        );
        assert_eq!(
            processor.process_block(2, &sources, &mut [&mut first]),
            Err(ProcessBlockError::SinkCountMismatch {
                expected: 2,
                found: 1,
            })
        );
        let short = [0.0; 3];
        let short_sources: [&[f32]; 3] = [&short, &full, &full];
        assert_eq!(
            processor.process_block(2, &short_sources, &mut [&mut first, &mut second]),
            Err(ProcessBlockError::SourceBufferTooShort {
                index: 0,
                required: 4,
                found: 3,
            })
        );
        let mut short_sink = [0.0; 3];
        assert_eq!(
            processor.process_block(2, &sources, &mut [&mut short_sink, &mut second]),
            Err(ProcessBlockError::SinkBufferTooShort {
                index: 0,
                required: 4,
                found: 3,
            })
        );
        assert!(
            first
                .into_iter()
                .all(|sample| (sample - 9.0).abs() < f32::EPSILON)
        );
        assert!(
            second
                .into_iter()
                .all(|sample| (sample - 9.0).abs() < f32::EPSILON)
        );
    }

    #[derive(Clone, Copy)]
    enum ResolverShape {
        MissingSourceOutput,
        MissingSinkInput,
        MissingMixerInput,
        MissingMixerOutput,
        MissingGainInput,
        MissingVirtualRender,
        MissingVirtualCapture,
        MissingGainOutput,
        DuplicateProviderPort,
        ValidProvider,
    }

    struct ShapeResolver(ResolverShape);

    impl NodeResolver for ShapeResolver {
        fn resolve(
            &self,
            _node: &NodeSpec,
            _context: &CompileContext,
        ) -> Result<ResolvedNode, NodeResolutionError> {
            let input = CompiledPort::new(PortId::from(AUDIO_PORT_ID), PortDirection::Input, 2);
            let output = CompiledPort::new(PortId::from(AUDIO_PORT_ID), PortDirection::Output, 2);
            Ok(match self.0 {
                ResolverShape::MissingSourceOutput | ResolverShape::MissingSinkInput => {
                    ResolvedNode {
                        inputs: vec![],
                        outputs: vec![],
                    }
                }
                ResolverShape::MissingMixerInput | ResolverShape::MissingGainInput => {
                    ResolvedNode {
                        inputs: vec![],
                        outputs: vec![output],
                    }
                }
                ResolverShape::MissingMixerOutput => ResolvedNode {
                    inputs: vec![CompiledPort::new(
                        PortId::from("one"),
                        PortDirection::Input,
                        2,
                    )],
                    outputs: vec![],
                },
                ResolverShape::MissingVirtualRender => ResolvedNode {
                    inputs: vec![CompiledPort::new(
                        PortId::from(TO_OS_PORT_ID),
                        PortDirection::Input,
                        2,
                    )],
                    outputs: vec![],
                },
                ResolverShape::MissingVirtualCapture => ResolvedNode {
                    inputs: vec![],
                    outputs: vec![CompiledPort::new(
                        PortId::from(FROM_OS_PORT_ID),
                        PortDirection::Output,
                        2,
                    )],
                },
                ResolverShape::MissingGainOutput => ResolvedNode {
                    inputs: vec![input],
                    outputs: vec![],
                },
                ResolverShape::DuplicateProviderPort => ResolvedNode {
                    inputs: vec![input.clone(), input],
                    outputs: vec![output],
                },
                ResolverShape::ValidProvider => ResolvedNode {
                    inputs: vec![input],
                    outputs: vec![output],
                },
            })
        }
    }

    fn custom_plan(config: NodeConfig, shape: ResolverShape) -> ExecutionPlan {
        GraphCompiler::new(ShapeResolver(shape))
            .compile(
                &GraphDocument::new(1, vec![node("custom", config)], vec![]),
                CompileContext {
                    format: FORMAT,
                    limits: CompileLimits::default(),
                },
            )
            .expect("custom graph should compile")
    }

    #[test]
    fn rejects_custom_resolver_shapes_that_cannot_render_safely() {
        let missing_port = |config, shape, port_id| {
            assert_eq!(
                RenderPlan::prepare(&custom_plan(config, shape))
                    .expect_err("required port must be present"),
                RenderPlanError::MissingPort {
                    node_id: NodeId::from("custom"),
                    port_id: PortId::from(port_id),
                }
            );
        };

        missing_port(
            app_source("custom").config,
            ResolverShape::MissingSourceOutput,
            AUDIO_PORT_ID,
        );
        missing_port(
            device_output("custom").config,
            ResolverShape::MissingSinkInput,
            AUDIO_PORT_ID,
        );
        missing_port(
            NodeConfig::Mixer(MixerConfig {
                inputs: vec![PortId::from("one")],
                channels: 2,
            }),
            ResolverShape::MissingMixerInput,
            "one",
        );
        missing_port(
            NodeConfig::Mixer(MixerConfig {
                inputs: vec![PortId::from("one")],
                channels: 2,
            }),
            ResolverShape::MissingMixerOutput,
            AUDIO_PORT_ID,
        );
        let gain = NodeConfig::Gain(GainConfig {
            channels: 2,
            gain_db: 0.0,
        });
        missing_port(gain.clone(), ResolverShape::MissingGainInput, AUDIO_PORT_ID);
        missing_port(gain, ResolverShape::MissingGainOutput, AUDIO_PORT_ID);
        missing_port(
            virtual_endpoint("custom").config,
            ResolverShape::MissingVirtualRender,
            FROM_OS_PORT_ID,
        );
        let capture_only = NodeConfig::VirtualEndpointTransport(VirtualEndpointSpec {
            id: VirtualEndpointId::from("custom"),
            display_name: "custom".into(),
            render: Some(stereo_stream()),
            capture: None,
            supported_sample_rates: vec![48_000],
            default_sample_rate: 48_000,
        });
        missing_port(
            capture_only,
            ResolverShape::MissingVirtualCapture,
            TO_OS_PORT_ID,
        );
    }

    #[test]
    fn rejects_unregistered_or_ambiguous_provider_kernels() {
        let provider = NodeConfig::Provider(ProviderNodeConfig {
            ports: None,
            type_id: "io.mixrelay.test".into(),
            parameters: serde_json::Value::Null,
        });
        assert_eq!(
            RenderPlan::prepare(&custom_plan(
                provider.clone(),
                ResolverShape::DuplicateProviderPort,
            ))
            .expect_err("duplicate ports are ambiguous"),
            RenderPlanError::DuplicatePort {
                node_id: NodeId::from("custom"),
                port_id: PortId::from(AUDIO_PORT_ID),
            }
        );
        assert_eq!(
            RenderPlan::prepare(&custom_plan(provider, ResolverShape::ValidProvider))
                .expect_err("provider kernel must be registered"),
            RenderPlanError::UnsupportedProvider {
                node_id: NodeId::from("custom"),
            }
        );
    }

    #[test]
    fn unconnected_application_and_hardware_sources_are_drained_and_metered() {
        let execution = compile(
            vec![app_source("app"), device_input("mic"), device_output("out")],
            vec![],
        );
        let plan = Arc::new(RenderPlan::prepare(&execution).unwrap());
        assert_eq!(plan.source_bindings().len(), 2);
        assert!(plan.sink_bindings().is_empty());
        let meters = Arc::new(crate::PeakMeterBank::new(plan.meter_bindings().len(), 64));
        let mut processor = BlockProcessor::with_peak_meters(plan.clone(), meters.clone());
        HEAP_OPERATIONS.with(|count| count.set(Some(0)));
        processor
            .process_block(
                2,
                &[&[0.5, -0.25, 0.1, 0.2], &[0.75, -0.5, 0.2, 0.3]],
                &mut [],
            )
            .unwrap();
        assert_eq!(HEAP_OPERATIONS.with(|count| count.replace(None)), Some(0));
        for (index, binding) in plan.meter_bindings().iter().enumerate() {
            let peaks = meters.take_peaks(1, index, 2);
            let expected = match binding.node_id().as_str() {
                "app" => [0.5, 0.25],
                "mic" => [0.75, 0.5],
                "out" => [0.0, 0.0],
                _ => unreachable!(),
            };
            assert_eq!(peaks, expected);
        }
    }

    #[test]
    fn supports_render_only_virtual_endpoints() {
        let render_only = node(
            "render-only",
            NodeConfig::VirtualEndpointTransport(VirtualEndpointSpec {
                id: VirtualEndpointId::from("render-only"),
                display_name: "render-only".into(),
                render: Some(stereo_stream()),
                capture: None,
                supported_sample_rates: vec![48_000],
                default_sample_rate: 48_000,
            }),
        );
        RenderPlan::prepare(&compile(vec![render_only], vec![]))
            .expect("render-only endpoint should lower");
    }

    #[test]
    fn supports_render_only_contribution_sink_with_no_capture_source() {
        let capture_only = NodeConfig::VirtualEndpointTransport(VirtualEndpointSpec {
            id: VirtualEndpointId::from("capture-only"),
            display_name: "Capture only".into(),
            render: Some(stereo_stream()),
            capture: None,
            supported_sample_rates: vec![48_000],
            default_sample_rate: 48_000,
        });
        let execution = compile(vec![node("capture-only", capture_only)], vec![]);
        let plan = Arc::new(RenderPlan::prepare(&execution).expect("plan should lower"));
        assert!(plan.source_bindings().is_empty());
        assert_eq!(plan.sink_bindings().len(), 1);
        let mut processor = BlockProcessor::new(plan);
        let mut sink = [1.0; 4];

        processor
            .process_block(2, &[], &mut [&mut sink])
            .expect("capture-only endpoint should render silence");
        assert!(sink.iter().all(|sample| sample.abs() < f32::EPSILON));
    }

    #[test]
    fn routes_virtual_endpoint_render_to_capture_inside_one_node() {
        let execution = compile(
            vec![virtual_endpoint("pass-through")],
            vec![edge(
                "internal",
                "pass-through",
                FROM_OS_PORT_ID,
                "pass-through",
                TO_OS_PORT_ID,
            )],
        );
        let plan = Arc::new(RenderPlan::prepare(&execution).expect("pass-through should lower"));
        let input = [1.0, -1.0, 0.5, -0.5];
        let mut output = [0.0; 4];
        let mut processor = BlockProcessor::new(plan);
        processor
            .process_block(2, &[&input], &mut [&mut output])
            .expect("internal route should process");
        for (actual, expected) in output.into_iter().zip(input) {
            assert!((actual - expected).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn rejects_render_arena_size_overflow() {
        let stereo = compile(vec![app_source("stereo")], vec![]);
        let multiply_error =
            allocate_slots(stereo.nodes(), usize::MAX).expect_err("stereo slot size must overflow");
        assert_eq!(multiply_error, RenderPlanError::BufferSizeOverflow);

        let mono = |id| {
            node(
                id,
                NodeConfig::ApplicationCapture(ApplicationCaptureRequest {
                    selector: ApplicationSelector::BundleId {
                        bundle_id: format!("io.mixrelay.{id}"),
                        include_descendants: true,
                    },
                    stream: StreamSpec {
                        channels: 1,
                        channel_layout: ChannelLayout::Mono,
                    },
                    source_playback: SourcePlaybackPolicy::Preserve,
                    forwarding_endpoint: None,
                    gain_db: 0.0,
                    requires_reselection: false,
                }),
            )
        };
        let two_mono = compile(vec![mono("first"), mono("second")], vec![]);
        let add_error = allocate_slots(two_mono.nodes(), usize::MAX)
            .expect_err("second mono slot offset must overflow");
        assert_eq!(add_error, RenderPlanError::BufferSizeOverflow);
    }
}
