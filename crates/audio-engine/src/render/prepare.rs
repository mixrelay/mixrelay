//! Control-thread compilation from a validated graph into immutable render operations.
use super::{
    DeviceBusBinding, DeviceBusKernel, DynamicsBinding, DynamicsMeterBinding, ExternalBinding,
    HostedAudioProcessor, HostedProcessorFactory, Kernel, LoweredKernels, LoweredRoutes,
    MeterBinding, NodeStep, PortKey, PortSlot, PreparedRenderPlan, RenderPlan, RenderPlanError,
    RenderSchedule, RouteOp, RuntimeControlBank, allocate_slots, compensation, effects, history,
    sample_count, source_routes,
};
use mixrelay_audio_graph::{
    AUDIO_PORT_ID, AudioEndpoint, CompiledNode, ExecutionPlan, FROM_OS_PORT_ID, NodeConfig, NodeId,
    PortDirection, PortId, RenderFormat, TO_OS_PORT_ID,
};
use std::{collections::BTreeMap, sync::Arc};

mod providers;
use providers::lower_hosted_processor;

#[allow(clippy::too_many_lines)] // Ordered control-thread preparation of independent bounded resources.
pub(super) fn plan(
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
    let source_controls = binding_controls(&controls, source_bindings.iter().map(|b| &b.node_id))?;
    let sink_controls = binding_controls(&controls, sink_bindings.iter().map(|b| &b.node_id))?;
    let recorder_controls =
        binding_controls(&controls, recorder_bindings.iter().map(|b| &b.node_id))?;
    let device_bus_bindings = device_bus_bindings(execution, &slots)?;
    let dynamics_bindings = dynamics_bindings(execution);
    let history = history::bindings(execution, &effects, &dynamics_bindings);
    let source_history =
        history::prepare(execution, &source_bindings, &device_bus_bindings, &history);
    let hosted_scratch_samples = hosted_input_scratch_samples
        .checked_add(hosted_output_scratch_samples)
        .ok_or(RenderPlanError::BufferSizeOverflow)?;

    Ok((
        RenderPlan {
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

fn binding_controls<'a>(
    controls: &RuntimeControlBank,
    ids: impl Iterator<Item = &'a NodeId>,
) -> Result<Box<[usize]>, RenderPlanError> {
    ids.map(|id| {
        controls
            .index(id)
            .ok_or_else(|| RenderPlanError::UncompiledBinding {
                node_id: id.clone(),
            })
    })
    .collect()
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
                routed_source: direction == PortDirection::Output
                    && source_routes::reaches_sink(execution, node.id(), &port_id),
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
