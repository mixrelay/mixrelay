use std::{collections::BTreeMap, fmt, sync::Arc};

use mixrelay_audio_graph::{
    AUDIO_PORT_ID, AudioEndpoint, CompiledNode, ExecutionPlan, FROM_OS_PORT_ID, NodeConfig, NodeId,
    PortDirection, PortId, ProviderNodeConfig, RenderFormat, TO_OS_PORT_ID,
};
use mixrelay_audio_io::VirtualEndpointId;
use mixrelay_audio_io::process::{ProcessCycle, ProcessReport, TransferOutcome};
use mixrelay_audio_recorder::{RecorderRegistry, RecorderTap};
use thiserror::Error;

/// Stable transport identity for a recorder's channel geometry. An inactive
/// replacement can coexist with the previous plan until publication or rollback.
#[must_use]
pub fn recording_session_key(node: &NodeId, channels: u16) -> String {
    format!("{}#recorder-channels={channels}", node.as_str())
}

/// A fully prepared third-party processor owned by the audio worker.
///
/// Implementations must reuse storage and must not wait on the UI or control plane from
/// `process`. A failed block is bypassed by the graph so one plug-in cannot mute the route.
pub trait HostedAudioProcessor: Send {
    fn process(&mut self, input: &[f32], output: &mut [f32], frames: usize) -> bool;

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
}

type HostedProcessors = Box<[Box<dyn HostedAudioProcessor>]>;
type PreparedRenderPlan = (RenderPlan, HostedProcessors);

#[cfg(test)]
#[path = "hosted_reuse_tests.rs"]
mod hosted_reuse_tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalBinding {
    node_id: NodeId,
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
}

impl MeterBinding {
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
    source_kernels: Box<[Kernel]>,
    steps: Box<[NodeStep]>,
    sink_routes: Box<[RouteOp]>,
    sink_kernels: Box<[Kernel]>,
    source_bindings: Box<[ExternalBinding]>,
    sink_bindings: Box<[ExternalBinding]>,
    meter_bindings: Box<[MeterBinding]>,
    recorder_bindings: Box<[MeterBinding]>,
    arena_samples: usize,
    dynamics_state_count: usize,
    dynamics_bindings: Box<[DynamicsBinding]>,
    device_bus_samples: usize,
    device_bus_bindings: Box<[DeviceBusBinding]>,
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
        let mut source_kernels = Vec::new();
        let mut sink_kernels = Vec::new();
        let mut steps = Vec::with_capacity(execution.nodes().len());
        let mut dynamics_state_count = 0;
        let mut device_bus_samples = 0;
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
                device_bus_samples: &mut device_bus_samples,
                hosted_processors: &mut hosted_processors,
            };
            let kernels = lower_controlled_kernels(node, &context, &mut state)?;
            source_kernels.extend(kernels.sources);
            sink_kernels.extend(kernels.sinks);
            steps.push(NodeStep {
                incoming_routes: incoming_routes.into_boxed_slice(),
                kernels: kernels.processing.into_boxed_slice(),
            });
        }

        Ok((
            Self {
                revision: execution.revision(),
                format: execution.format(),
                source_kernels: source_kernels.into_boxed_slice(),
                steps: steps.into_boxed_slice(),
                sink_routes: routes.sinks.into_boxed_slice(),
                recorder_bindings: recorder_bindings.into_boxed_slice(),
                sink_kernels: sink_kernels.into_boxed_slice(),
                source_bindings: source_bindings.into_boxed_slice(),
                sink_bindings: sink_bindings.into_boxed_slice(),
                meter_bindings: meter_bindings.into_boxed_slice(),
                arena_samples,
                dynamics_state_count,
                dynamics_bindings: dynamics_bindings(execution),
                device_bus_samples,
                device_bus_bindings: device_bus_bindings(execution, &slots)?,
            },
            hosted_processors.into_boxed_slice(),
        ))
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
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
}

pub struct BlockProcessor {
    recorders: Box<[RecorderTap]>,
    cycle_tracker: mixrelay_audio_io::process::CycleTracker,
    plan: Arc<RenderPlan>,
    arena: Box<[f32]>,
    source_buffers: Box<[Box<[f32]>]>,
    sink_buffers: Box<[Box<[f32]>]>,
    meters: Option<Arc<crate::PeakMeterBank>>,
    dynamics_envelopes: Box<[f32]>,
    device_bus_state: Box<[f32]>,
    hosted_processors: Box<[Box<dyn HostedAudioProcessor>]>,
    hosted_identities: Box<[(u64, usize)]>,
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
            .field("has_meters", &self.meters.is_some())
            .field("dynamics_envelopes", &self.dynamics_envelopes.len())
            .field("device_bus_state", &self.device_bus_state.len())
            .field("hosted_processors", &self.hosted_processors.len())
            .field("hosted_identities", &self.hosted_identities)
            .finish()
    }
}

pub trait ExternalIo {
    /// Native I/O must consume this cycle's timeline, not infer a second clock
    /// from the destination capacity. Never block or allocate at this boundary.
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
    pub fn new(plan: Arc<RenderPlan>) -> Self {
        let arena = vec![0.0; plan.arena_samples].into_boxed_slice();
        let source_buffers = allocate_external_buffers(&plan.source_bindings, plan.format);
        let sink_buffers = allocate_external_buffers(&plan.sink_bindings, plan.format);
        let dynamics_envelopes = vec![0.0; plan.dynamics_state_count].into_boxed_slice();
        let device_bus_state = vec![0.0; plan.device_bus_samples].into_boxed_slice();
        Self {
            recorders: Box::new([]),
            cycle_tracker: mixrelay_audio_io::process::CycleTracker::default(),
            plan,
            arena,
            source_buffers,
            sink_buffers,
            meters: None,
            dynamics_envelopes,
            device_bus_state,
            hosted_processors: Box::new([]),
            hosted_identities: Box::new([]),
        }
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
        let mut processor = Self::with_peak_meters(Arc::new(plan), meters);
        processor.hosted_processors = hosted_processors;
        let mut identities: Vec<_> = processor
            .hosted_processors
            .iter()
            .enumerate()
            .filter_map(|(index, hosted)| hosted.reuse_identity().map(|id| (id, index)))
            .collect();
        identities.sort_unstable();
        processor.hosted_identities = identities.into_boxed_slice();
        Ok(processor)
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

    pub(crate) fn retain_dynamics_state(&mut self, previous: &Self) {
        if self.plan.format != previous.plan.format {
            return;
        }
        for binding in &self.plan.dynamics_bindings {
            let Ok(index) = previous
                .plan
                .dynamics_bindings
                .binary_search_by(|candidate| candidate.node_id.cmp(&binding.node_id))
            else {
                continue;
            };
            let source = &previous.plan.dynamics_bindings[index];
            if source.config == binding.config {
                self.dynamics_envelopes[binding.state_index] =
                    previous.dynamics_envelopes[source.state_index];
            }
        }
    }

    pub(crate) fn retain_device_bus_state(&mut self, previous: &Self) {
        if self.plan.format != previous.plan.format {
            return;
        }
        for binding in &self.plan.device_bus_bindings {
            let Ok(index) = previous
                .plan
                .device_bus_bindings
                .binary_search_by(|candidate| candidate.node_id.cmp(&binding.node_id))
            else {
                continue;
            };
            let source = &previous.plan.device_bus_bindings[index];
            if source.samples.len() == binding.samples.len() {
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
    ) -> Result<ProcessReport, ProcessBlockError> {
        let frames = cycle.frames;
        if frames > self.plan.format.block_frames {
            return Err(ProcessBlockError::FrameCountExceeded {
                maximum: self.plan.format.block_frames,
                found: frames,
            });
        }
        self.cycle_tracker
            .accept(
                cycle,
                mixrelay_audio_io::session::SessionFormat {
                    sample_rate: self.plan.format.sample_rate,
                    block_frames: self.plan.format.block_frames,
                },
            )
            .map_err(ProcessBlockError::InvalidCycle)?;
        let frames_usize = frames as usize;
        let mut report = ProcessReport::default();
        for (binding, destination) in self
            .plan
            .source_bindings
            .iter()
            .zip(&mut self.source_buffers)
        {
            let samples = sample_count(frames_usize, binding.channels);
            destination[..samples].fill(0.0);
            let transfer = external.read_source(binding, &mut destination[..samples], &cycle);
            if transfer.frames > frames {
                return Err(ProcessBlockError::InvalidTransfer);
            }
            // An adapter may fill only a prefix. Never process stale data from
            // another cycle, even if it wrote beyond the reported prefix.
            destination[sample_count(transfer.frames as usize, binding.channels)..samples]
                .fill(0.0);
            report.input_starvations += u64::from(
                transfer.activity == mixrelay_audio_io::process::InputActivity::Active
                    && transfer.frames < frames,
            );
            report.discontinuities += u64::from(transfer.discontinuity);
        }
        // Snapshots belong to the accepted input timeline. A restart must not
        // replay a completed quantum from the previous device/clock generation.
        if cycle.discontinuity || report.discontinuities != 0 {
            self.device_bus_state.fill(0.0);
        }
        self.process_loaded_block(frames_usize);
        for (binding, source) in self.plan.sink_bindings.iter().zip(&self.sink_buffers) {
            let samples = sample_count(frames_usize, binding.channels);
            let transfer = external.write_sink(binding, &source[..samples], &cycle);
            if transfer.frames > frames {
                return Err(ProcessBlockError::InvalidTransfer);
            }
            report.output_overflows += u64::from(transfer.frames < frames);
            report.discontinuities += u64::from(transfer.discontinuity);
        }
        Ok(report)
    }

    fn process_loaded_block(&mut self, frames: usize) {
        self.arena.fill(0.0);

        for kernel in &self.plan.source_kernels {
            apply_kernel(
                KernelResources {
                    arena: &mut self.arena,
                    sources: &self.source_buffers,
                    sinks: &mut self.sink_buffers,
                    dynamics_envelopes: &mut self.dynamics_envelopes,
                    device_bus_state: &mut self.device_bus_state,
                    hosted_processors: &mut self.hosted_processors,
                },
                kernel,
                frames,
            );
        }
        for step in &self.plan.steps {
            apply_routes(&mut self.arena, &step.incoming_routes, frames);
            for kernel in &step.kernels {
                apply_kernel(
                    KernelResources {
                        arena: &mut self.arena,
                        sources: &self.source_buffers,
                        sinks: &mut self.sink_buffers,
                        dynamics_envelopes: &mut self.dynamics_envelopes,
                        device_bus_state: &mut self.device_bus_state,
                        hosted_processors: &mut self.hosted_processors,
                    },
                    kernel,
                    frames,
                );
            }
        }
        apply_routes(&mut self.arena, &self.plan.sink_routes, frames);
        for kernel in &self.plan.sink_kernels {
            apply_kernel(
                KernelResources {
                    arena: &mut self.arena,
                    sources: &self.source_buffers,
                    sinks: &mut self.sink_buffers,
                    dynamics_envelopes: &mut self.dynamics_envelopes,
                    device_bus_state: &mut self.device_bus_state,
                    hosted_processors: &mut self.hosted_processors,
                },
                kernel,
                frames,
            );
        }
        for (binding, recorder) in self.plan.recorder_bindings.iter().zip(&self.recorders) {
            let samples = sample_count(frames, binding.channels);
            recorder.capture(
                &self.arena[binding.slot.offset..binding.slot.offset + samples],
                self.plan.format.sample_rate,
            );
        }
        if let Some(meters) = &self.meters {
            for (index, binding) in self.plan.meter_bindings.iter().enumerate() {
                let samples = sample_count(frames, binding.channels);
                meters.record_interleaved(
                    self.plan.revision,
                    index,
                    binding.channels,
                    &self.arena[binding.slot.offset..binding.slot.offset + samples],
                );
            }
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

fn allocate_external_buffers(
    bindings: &[ExternalBinding],
    format: RenderFormat,
) -> Box<[Box<[f32]>]> {
    bindings
        .iter()
        .map(|binding| {
            vec![0.0; sample_count(format.block_frames as usize, binding.channels)]
                .into_boxed_slice()
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RenderPlanError {
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
}

fn device_bus_bindings(
    execution: &ExecutionPlan,
    slots: &BTreeMap<PortKey, PortSlot>,
) -> Result<Box<[DeviceBusBinding]>, RenderPlanError> {
    let mut offset = 0;
    let mut bindings = Vec::new();
    for node in execution
        .nodes()
        .iter()
        .filter(|node| node.control().enabled)
    {
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
        });
        offset = end;
    }
    bindings.sort_unstable_by(|left, right| left.node_id.cmp(&right.node_id));
    Ok(bindings.into_boxed_slice())
}

fn dynamics_bindings(execution: &ExecutionPlan) -> Box<[DynamicsBinding]> {
    let mut bindings: Vec<_> = execution
        .nodes()
        .iter()
        .filter(|node| {
            node.control().enabled
                && matches!(
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
    bindings.sort_unstable_by(|left, right| left.node_id.cmp(&right.node_id));
    bindings.into_boxed_slice()
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct PortKey {
    endpoint: AudioEndpoint,
    direction: PortDirection,
}

#[derive(Debug)]
struct NodeStep {
    incoming_routes: Box<[RouteOp]>,
    kernels: Box<[Kernel]>,
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
struct RouteOp {
    source: PortSlot,
    destination: PortSlot,
    channels: Box<[(u16, u16)]>,
}

#[derive(Debug)]
enum Kernel {
    Silence(PortSlot),
    DeviceBus(DeviceBusKernel),
    Source {
        binding: usize,
        output: PortSlot,
        linear_gain: f32,
    },
    Sink {
        binding: usize,
        input: PortSlot,
    },
    Gain {
        input: PortSlot,
        output: PortSlot,
        linear_gain: f32,
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
    },
    Limiter {
        input: PortSlot,
        output: PortSlot,
        state_index: usize,
        ceiling_linear: f32,
        release_coefficient: f32,
    },
    Mixer {
        inputs: Box<[PortSlot]>,
        output: PortSlot,
    },
    Hosted {
        input: PortSlot,
        output: PortSlot,
        processor: usize,
    },
}

#[derive(Debug)]
enum DeviceBusKernel {
    Read {
        snapshot: PortSlot,
        output: PortSlot,
    },
    Publish {
        snapshot: PortSlot,
        input: PortSlot,
        capacity: usize,
    },
}

impl DeviceBusKernel {
    fn apply(&self, arena: &mut [f32], state: &mut [f32], frames: usize) {
        match self {
            Self::Read { snapshot, output } => {
                let samples = sample_count(frames, output.channels);
                arena[output.offset..output.offset + samples]
                    .copy_from_slice(&state[snapshot.offset..snapshot.offset + samples]);
            }
            Self::Publish {
                snapshot,
                input,
                capacity,
            } => {
                let samples = sample_count(frames, input.channels);
                let destination = &mut state[snapshot.offset..snapshot.offset + capacity];
                // Short I/O quanta cannot expose the previous longer quantum's tail.
                destination.fill(0.0);
                destination[..samples]
                    .copy_from_slice(&arena[input.offset..input.offset + samples]);
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
    let disabled: std::collections::BTreeSet<_> = execution
        .nodes()
        .iter()
        .filter(|node| !node.control().enabled)
        .map(CompiledNode::id)
        .collect();
    for edge in execution.edges() {
        if disabled.contains(&edge.source().node_id)
            || disabled.contains(&edge.destination().node_id)
        {
            continue;
        }
        let source = compiled_edge_slot(slots, edge.source(), PortDirection::Output);
        let destination = compiled_edge_slot(slots, edge.destination(), PortDirection::Input);
        let channels = edge
            .routes()
            .iter()
            .map(|route| (route.source_channel, route.destination_channel))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        if is_external_sink(execution, edge.destination()) {
            lowered.sinks.push(RouteOp {
                source,
                destination,
                channels,
            });
            continue;
        }
        lowered
            .incoming
            .entry(edge.destination().node_id.clone())
            .or_default()
            .push(RouteOp {
                source,
                destination,
                channels,
            });
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
        if !node.control().enabled {
            continue;
        }
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
                if config.render.is_some() =>
            {
                Some(FROM_OS_PORT_ID)
            }
            (NodeConfig::VirtualEndpointTransport(config), PortDirection::Input)
                if config.capture.is_some() =>
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
    for node in execution.nodes() {
        for port in node.outputs() {
            bindings.push(meter_binding(
                node,
                port.id(),
                PortDirection::Output,
                slots,
            )?);
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
        .filter(|node| {
            node.control().enabled && matches!(node.config(), NodeConfig::Recorder { .. })
        })
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
    device_bus_samples: &'a mut usize,
    hosted_processors: &'a mut Vec<Box<dyn HostedAudioProcessor>>,
}

fn lower_kernels(
    node: &CompiledNode,
    context: &LoweringContext<'_>,
    state: &mut LoweringState<'_>,
) -> Result<LoweredKernels, RenderPlanError> {
    match node.config() {
        NodeConfig::ApplicationCapture(config) => {
            lower_source_node(node, config.gain_db, context.slots, context.source_bindings)
        }
        NodeConfig::AudioDeviceInput(config) => {
            lower_source_node(node, config.gain_db, context.slots, context.source_bindings)
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

fn lower_controlled_kernels(
    node: &CompiledNode,
    context: &LoweringContext<'_>,
    state: &mut LoweringState<'_>,
) -> Result<LoweredKernels, RenderPlanError> {
    if !node.control().enabled {
        // The arena is cleared each cycle. No kernel, binding, hosted instance or
        // recorder tap is created for an OFF node; its output cannot become bypass.
        return Ok(LoweredKernels::default());
    }
    let mut kernels = lower_kernels(node, context, state)?;
    if !node.control().muted {
        return Ok(kernels);
    }
    if node.outputs().is_empty() {
        let silence = node
            .inputs()
            .iter()
            .map(|port| {
                node_slot(node, port.id(), PortDirection::Input, context.slots).map(Kernel::Silence)
            })
            .collect::<Result<Vec<_>, _>>()?;
        kernels.sinks.splice(0..0, silence);
    } else {
        let boundary = if node.inputs().is_empty() {
            &mut kernels.sources
        } else {
            &mut kernels.processing
        };
        for port in node.outputs() {
            boundary.push(Kernel::Silence(node_slot(
                node,
                port.id(),
                PortDirection::Output,
                context.slots,
            )?));
        }
    }
    Ok(kernels)
}

fn lower_builtin_processor(
    node: &CompiledNode,
    context: &LoweringContext<'_>,
    state: &mut LoweringState<'_>,
) -> Result<LoweredKernels, RenderPlanError> {
    match node.config() {
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
        NodeConfig::Gain(config) => Ok(LoweredKernels {
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
                linear_gain: 10.0_f32.powf(config.gain_db / 20.0),
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
        }],
        sinks: vec![],
    })
}

/// Device snapshots cross the ordinary I/O cycle boundary, regardless of topology.
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
    if config.render.is_some() {
        kernels.sources.extend(source_kernel(
            node,
            FROM_OS_PORT_ID,
            0.0,
            context.slots,
            context.source_bindings,
        )?);
    }
    if config.capture.is_some() {
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
        return Err(RenderPlanError::UnsupportedProvider {
            node_id: node.id().clone(),
        });
    };
    let (input, output) = processor_slots(node, context.slots)?;
    let processor = factory
        .prepare(HostedProcessorPrepareRequest {
            node_id: node.id(),
            config,
            format: context.format,
            input_channels: input.channels,
            output_channels: output.channels,
        })
        .map_err(|reason| RenderPlanError::ProviderPreparation {
            node_id: node.id().clone(),
            reason,
        })?;
    let processor_index = state.hosted_processors.len();
    state.hosted_processors.push(processor);
    Ok(LoweredKernels {
        sources: vec![],
        processing: vec![Kernel::Hosted {
            input,
            output,
            processor: processor_index,
        }],
        sinks: vec![],
    })
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
    gain_db: f32,
    slots: &BTreeMap<PortKey, PortSlot>,
    source_bindings: &[ExternalBinding],
) -> Result<LoweredKernels, RenderPlanError> {
    Ok(LoweredKernels {
        sources: source_kernel(node, AUDIO_PORT_ID, gain_db, slots, source_bindings)?
            .into_iter()
            .collect(),
        processing: vec![],
        sinks: vec![],
    })
}

fn source_kernel(
    node: &CompiledNode,
    port_id: &str,
    gain_db: f32,
    slots: &BTreeMap<PortKey, PortSlot>,
    bindings: &[ExternalBinding],
) -> Result<Option<Kernel>, RenderPlanError> {
    let port_id = PortId::from(port_id);
    let output = node_slot(node, &port_id, PortDirection::Output, slots)?;
    let Some(binding) = binding_index(bindings, node.id(), &port_id) else {
        return Ok(None);
    };
    Ok(Some(Kernel::Source {
        binding,
        output,
        linear_gain: 10.0_f32.powf(gain_db / 20.0),
    }))
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

fn apply_routes(arena: &mut [f32], routes: &[RouteOp], frames: usize) {
    for route in routes {
        let source_channels = usize::from(route.source.channels);
        let destination_channels = usize::from(route.destination.channels);
        for frame in 0..frames {
            let source_frame = route.source.offset + frame * source_channels;
            let destination_frame = route.destination.offset + frame * destination_channels;
            for &(source_channel, destination_channel) in &route.channels {
                arena[destination_frame + usize::from(destination_channel)] +=
                    arena[source_frame + usize::from(source_channel)];
            }
        }
    }
}

struct KernelResources<'a> {
    arena: &'a mut [f32],
    sources: &'a [Box<[f32]>],
    sinks: &'a mut [Box<[f32]>],
    dynamics_envelopes: &'a mut [f32],
    device_bus_state: &'a mut [f32],
    hosted_processors: &'a mut [Box<dyn HostedAudioProcessor>],
}

fn apply_kernel(resources: KernelResources<'_>, kernel: &Kernel, frames: usize) {
    match kernel {
        Kernel::Silence(slot) => {
            resources.arena[slot.offset..slot.offset + sample_count(frames, slot.channels)]
                .fill(0.0);
        }
        Kernel::DeviceBus(bus) => bus.apply(resources.arena, resources.device_bus_state, frames),
        Kernel::Source {
            binding,
            output,
            linear_gain,
        } => apply_source(resources, *binding, *output, *linear_gain, frames),
        Kernel::Sink { binding, input } => apply_sink(resources, *binding, *input, frames),
        Kernel::Gain {
            input,
            output,
            linear_gain,
        } => apply_gain(resources.arena, *input, *output, *linear_gain, frames),
        Kernel::Compressor {
            input,
            output,
            state_index,
            threshold_linear,
            exponent,
            attack_coefficient,
            release_coefficient,
            makeup_gain,
        } => apply_compressor(
            DynamicKernelContext {
                arena: resources.arena,
                input: *input,
                output: *output,
                frames,
                envelope: &mut resources.dynamics_envelopes[*state_index],
            },
            CompressorKernelParameters {
                threshold_linear: *threshold_linear,
                exponent: *exponent,
                attack_coefficient: *attack_coefficient,
                release_coefficient: *release_coefficient,
                makeup_gain: *makeup_gain,
            },
        ),
        Kernel::Limiter {
            input,
            output,
            state_index,
            ceiling_linear,
            release_coefficient,
        } => apply_limiter(
            DynamicKernelContext {
                arena: resources.arena,
                input: *input,
                output: *output,
                frames,
                envelope: &mut resources.dynamics_envelopes[*state_index],
            },
            LimiterKernelParameters {
                ceiling_linear: *ceiling_linear,
                release_coefficient: *release_coefficient,
            },
        ),
        Kernel::Mixer { inputs, output } => {
            apply_mixer(resources.arena, inputs, *output, frames);
        }
        Kernel::Hosted {
            input,
            output,
            processor,
        } => apply_hosted_processor(
            resources.arena,
            *input,
            *output,
            frames,
            resources.hosted_processors[*processor].as_mut(),
        ),
    }
}

fn apply_source(
    resources: KernelResources<'_>,
    binding: usize,
    output: PortSlot,
    linear_gain: f32,
    frames: usize,
) {
    let KernelResources { arena, sources, .. } = resources;
    let samples = frames * usize::from(output.channels);
    let destination = &mut arena[output.offset..output.offset + samples];
    if (linear_gain - 1.0).abs() <= f32::EPSILON {
        destination.copy_from_slice(&sources[binding][..samples]);
    } else {
        for (output, input) in destination.iter_mut().zip(&sources[binding][..samples]) {
            *output = *input * linear_gain;
        }
    }
}

fn apply_sink(resources: KernelResources<'_>, binding: usize, input: PortSlot, frames: usize) {
    let KernelResources { arena, sinks, .. } = resources;
    let samples = frames * usize::from(input.channels);
    sinks[binding][..samples].copy_from_slice(&arena[input.offset..input.offset + samples]);
}

fn apply_gain(
    arena: &mut [f32],
    input: PortSlot,
    output: PortSlot,
    linear_gain: f32,
    frames: usize,
) {
    let samples = frames * usize::from(input.channels);
    for index in 0..samples {
        arena[output.offset + index] = arena[input.offset + index] * linear_gain;
    }
}

fn apply_mixer(arena: &mut [f32], inputs: &[PortSlot], output: PortSlot, frames: usize) {
    let samples = frames * usize::from(output.channels);
    for index in 0..samples {
        arena[output.offset + index] = inputs.iter().map(|input| arena[input.offset + index]).sum();
    }
}

fn apply_hosted_processor(
    arena: &mut [f32],
    input: PortSlot,
    output: PortSlot,
    frames: usize,
    processor: &mut dyn HostedAudioProcessor,
) {
    let input_samples = sample_count(frames, input.channels);
    let output_samples = sample_count(frames, output.channels);
    let processed = if input.offset < output.offset {
        let (before_output, from_output) = arena.split_at_mut(output.offset);
        processor.process(
            &before_output[input.offset..input.offset + input_samples],
            &mut from_output[..output_samples],
            frames,
        )
    } else {
        let (before_input, from_input) = arena.split_at_mut(input.offset);
        processor.process(
            &from_input[..input_samples],
            &mut before_input[output.offset..output.offset + output_samples],
            frames,
        )
    };
    if processed {
        return;
    }
    let shared_channels = usize::from(input.channels.min(output.channels));
    for frame in 0..frames {
        for channel in 0..usize::from(output.channels) {
            arena[output.offset + frame * usize::from(output.channels) + channel] =
                if channel < shared_channels {
                    arena[input.offset + frame * usize::from(input.channels) + channel]
                } else {
                    0.0
                };
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct CompressorKernelParameters {
    threshold_linear: f32,
    exponent: f32,
    attack_coefficient: f32,
    release_coefficient: f32,
    makeup_gain: f32,
}

struct DynamicKernelContext<'a> {
    arena: &'a mut [f32],
    input: PortSlot,
    output: PortSlot,
    frames: usize,
    envelope: &'a mut f32,
}

fn apply_compressor(context: DynamicKernelContext<'_>, parameters: CompressorKernelParameters) {
    let DynamicKernelContext {
        arena,
        input,
        output,
        frames,
        envelope,
    } = context;
    let channels = usize::from(input.channels);
    for frame in 0..frames {
        let input_frame = input.offset + frame * channels;
        let output_frame = output.offset + frame * channels;
        let peak = arena[input_frame..input_frame + channels]
            .iter()
            .fold(0.0_f32, |current, sample| current.max(sample.abs()));
        let coefficient = if peak > *envelope {
            parameters.attack_coefficient
        } else {
            parameters.release_coefficient
        };
        *envelope = coefficient.mul_add(*envelope, (1.0 - coefficient) * peak);
        let gain = if *envelope > parameters.threshold_linear {
            (parameters.threshold_linear / *envelope).powf(parameters.exponent)
                * parameters.makeup_gain
        } else {
            parameters.makeup_gain
        };
        for channel in 0..channels {
            arena[output_frame + channel] = arena[input_frame + channel] * gain;
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct LimiterKernelParameters {
    ceiling_linear: f32,
    release_coefficient: f32,
}

fn apply_limiter(context: DynamicKernelContext<'_>, parameters: LimiterKernelParameters) {
    let DynamicKernelContext {
        arena,
        input,
        output,
        frames,
        envelope,
    } = context;
    let channels = usize::from(input.channels);
    for frame in 0..frames {
        let input_frame = input.offset + frame * channels;
        let output_frame = output.offset + frame * channels;
        let peak = arena[input_frame..input_frame + channels]
            .iter()
            .fold(0.0_f32, |current, sample| current.max(sample.abs()));
        *envelope = if peak >= *envelope {
            peak
        } else {
            parameters
                .release_coefficient
                .mul_add(*envelope, (1.0 - parameters.release_coefficient) * peak)
        };
        let gain = if *envelope > parameters.ceiling_linear {
            parameters.ceiling_linear / *envelope
        } else {
            1.0
        };
        for channel in 0..channels {
            arena[output_frame + channel] = arena[input_frame + channel] * gain;
        }
    }
}

#[cfg(test)]
mod tests {
    mod controls;
    mod recorder;
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
            unsafe { std::alloc::System.alloc(layout) }
        }
        unsafe fn dealloc(&self, pointer: *mut u8, layout: std::alloc::Layout) {
            count_heap_operation();
            unsafe { std::alloc::System.dealloc(pointer, layout) };
        }
        unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
            count_heap_operation();
            unsafe { std::alloc::System.alloc_zeroed(layout) }
        }
        unsafe fn realloc(
            &self,
            pointer: *mut u8,
            layout: std::alloc::Layout,
            size: usize,
        ) -> *mut u8 {
            count_heap_operation();
            unsafe { std::alloc::System.realloc(pointer, layout, size) }
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
                    .unwrap();
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
            ProcessReport::default()
        );
        assert_eq!(io.output.map(f32::to_bits), [0.0_f32; 8].map(f32::to_bits));
        io.read_frames = 2;
        io.write_frames = 3;
        io.value = 1.0;
        let next = ProcessCycle {
            first_frame: 4,
            ..cycle
        };
        let report = processor.process_external_cycle(next, &mut io).unwrap();
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
        assert_eq!(plan.source_bindings()[0].channels(), 2);
        assert_eq!(plan.sink_bindings()[0].channels(), 16);
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
    fn device_snapshot_has_one_io_quantum_latency_without_delaying_direct_hardware() {
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
        let first = [0.2, 0.4, 0.6, 0.8];
        let second = [1.0, 1.0, 1.0, 1.0];
        let mut output = [0.0; 4];
        HEAP_OPERATIONS.with(|count| count.set(Some(0)));
        processor
            .process_block(2, &[&first], &mut [&mut output])
            .unwrap();
        assert_eq!(output.map(f32::to_bits), first.map(f32::to_bits));
        processor
            .process_block(1, &[&second], &mut [&mut output])
            .unwrap();
        assert!((output[0] - 1.1).abs() < 0.000_001);
        assert!((output[1] - 1.2).abs() < 0.000_001);
        processor
            .process_block(2, &[&second], &mut [&mut output])
            .unwrap();
        assert_eq!(
            output.map(f32::to_bits),
            [1.5_f32, 1.5, 1.0, 1.0].map(f32::to_bits)
        );
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
            render: None,
            capture: Some(stereo_stream()),
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
    fn supports_capture_only_virtual_endpoints() {
        let capture_only = NodeConfig::VirtualEndpointTransport(VirtualEndpointSpec {
            id: VirtualEndpointId::from("capture-only"),
            display_name: "Capture only".into(),
            render: None,
            capture: Some(stereo_stream()),
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
