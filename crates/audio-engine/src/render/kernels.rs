//! Realtime node kernels borrow prepared buffers and state; no graph preparation or control publication.
use super::{DryBusMap, HostedAudioProcessor, HostedIo, Kernel, PortSlot, port_pair, sample_count};

pub(super) struct KernelResources<'a> {
    pub(super) arena: &'a mut [f32],
    pub(super) sources: &'a [Box<[f32]>],
    pub(super) sinks: &'a mut [Box<[f32]>],
    pub(super) dynamics_envelopes: &'a mut [f32],
    pub(super) dynamics_minimum_gains: &'a mut [f32],
    pub(super) dynamics_wet: &'a mut [f32],
    pub(super) effect_states: &'a mut [super::effects::State],
    pub(super) effects: &'a [super::effects::Binding],
    pub(super) device_bus_state: &'a mut [f32],
    pub(super) device_bus_cursor: usize,
    pub(super) hosted_scratch: &'a mut [f32],
    pub(super) hosted_input_scratch_samples: usize,
    pub(super) hosted_processors: &'a mut [Box<dyn HostedAudioProcessor>],
}

#[allow(clippy::too_many_lines)] // Exhaustive dispatch borrows each prepared resource without allocating.
pub(super) fn apply_kernel(
    resources: KernelResources<'_>,
    kernel: &Kernel,
    linear_gain: f32,
    frames: usize,
) {
    match kernel {
        Kernel::Effect {
            input,
            output,
            sidechain,
            state,
        } => {
            resources.effect_states[*state].process(
                &resources.effects[*state].design,
                resources.arena,
                *input,
                *output,
                *sidechain,
                frames,
            );
        }
        Kernel::DeviceBus(bus) => bus.apply(
            resources.arena,
            resources.device_bus_state,
            frames,
            resources.device_bus_cursor,
        ),
        Kernel::Source { binding, output } => {
            apply_source(resources, *binding, *output, linear_gain, frames);
        }
        Kernel::Sink { binding, input } => apply_sink(resources, *binding, *input, frames),
        Kernel::Gain { input, output } => {
            apply_gain(resources.arena, *input, *output, linear_gain, frames);
        }
        Kernel::Compressor {
            input,
            output,
            state_index,
            threshold_linear,
            exponent,
            attack_coefficient,
            release_coefficient,
            makeup_gain,
            bypass,
            bypass_coefficient,
        } => apply_compressor(
            DynamicKernelContext {
                arena: resources.arena,
                input: *input,
                output: *output,
                frames,
                envelope: &mut resources.dynamics_envelopes[*state_index],
                minimum_gain: &mut resources.dynamics_minimum_gains[*state_index],
                wet: &mut resources.dynamics_wet[*state_index],
                wet_target: f32::from(!*bypass),
                bypass_coefficient: *bypass_coefficient,
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
            bypass,
            bypass_coefficient,
        } => apply_limiter(
            DynamicKernelContext {
                arena: resources.arena,
                input: *input,
                output: *output,
                frames,
                envelope: &mut resources.dynamics_envelopes[*state_index],
                minimum_gain: &mut resources.dynamics_minimum_gains[*state_index],
                wet: &mut resources.dynamics_wet[*state_index],
                wet_target: f32::from(!*bypass),
                bypass_coefficient: *bypass_coefficient,
            },
            LimiterKernelParameters {
                ceiling_linear: *ceiling_linear,
                release_coefficient: *release_coefficient,
            },
        ),
        Kernel::Mixer { inputs, output } => {
            apply_mixer(resources.arena, inputs, *output, frames);
        }
        Kernel::Hosted { io, processor } => {
            apply_hosted_kernel(resources, io, *processor, frames);
        }
    }
}

fn apply_hosted_kernel(
    resources: KernelResources<'_>,
    io: &HostedIo,
    processor_index: usize,
    frames: usize,
) {
    let KernelResources {
        arena,
        hosted_scratch,
        hosted_input_scratch_samples,
        hosted_processors,
        ..
    } = resources;
    match io {
        HostedIo::Single { input, output } => apply_hosted_processor(
            arena,
            *input,
            *output,
            frames,
            hosted_processors[processor_index].as_mut(),
        ),
        HostedIo::Multiple {
            inputs,
            outputs,
            input_channels,
            output_channels,
            dry_maps,
        } => apply_hosted_processor_multiple(
            arena,
            MultipleHostedIo {
                inputs,
                outputs,
                input_channels: *input_channels,
                output_channels: *output_channels,
                frames,
                dry_maps,
            },
            HostedScratch {
                samples: hosted_scratch,
                input_samples: hosted_input_scratch_samples,
            },
            hosted_processors[processor_index].as_mut(),
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
    let (source, destination) = port_pair(arena, input, output, frames);
    for (destination, source) in destination.iter_mut().zip(source) {
        *destination = source * linear_gain;
    }
}

fn apply_mixer(arena: &mut [f32], inputs: &[PortSlot], output: PortSlot, frames: usize) {
    // Match f32 iterator summation's identity and input order, including -0.0.
    arena[output.offset..output.offset + sample_count(frames, output.channels)].fill(-0.0);
    for &input in inputs {
        let (source, destination) = port_pair(arena, input, output, frames);
        for (destination, source) in destination.iter_mut().zip(source) {
            *destination += source;
        }
    }
}

fn apply_hosted_processor(
    arena: &mut [f32],
    input: PortSlot,
    output: PortSlot,
    frames: usize,
    processor: &mut dyn HostedAudioProcessor,
) {
    let (source, destination) = port_pair(arena, input, output, frames);
    let processed = processor.process(source, destination, frames);
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

#[derive(Clone, Copy)]
struct MultipleHostedIo<'a> {
    inputs: &'a [PortSlot],
    outputs: &'a [PortSlot],
    input_channels: u16,
    output_channels: u16,
    frames: usize,
    dry_maps: &'a [DryBusMap],
}

struct HostedScratch<'a> {
    samples: &'a mut [f32],
    input_samples: usize,
}

fn apply_hosted_processor_multiple(
    arena: &mut [f32],
    io: MultipleHostedIo<'_>,
    scratch: HostedScratch<'_>,
    processor: &mut dyn HostedAudioProcessor,
) {
    let input_samples = sample_count(io.frames, io.input_channels);
    let output_samples = sample_count(io.frames, io.output_channels);
    let HostedScratch {
        samples,
        input_samples: scratch_input_samples,
    } = scratch;
    let (input_scratch, output_scratch) = samples.split_at_mut(scratch_input_samples);
    let input = &mut input_scratch[..input_samples];
    let output = &mut output_scratch[..output_samples];
    gather_buses(arena, io.inputs, input, io.frames, io.input_channels);
    if !processor.process(input, output, io.frames) {
        preserve_main_buses(input, output, &io);
    }
    scatter_buses(arena, io.outputs, output, io.frames, io.output_channels);
}

fn gather_buses(
    arena: &[f32],
    buses: &[PortSlot],
    destination: &mut [f32],
    frames: usize,
    total_channels: u16,
) {
    let mut channels_before = 0;
    for frame in 0..frames {
        for bus in buses {
            let channels = usize::from(bus.channels);
            let source_start = bus.offset + frame * channels;
            let destination_start = frame * usize::from(total_channels) + channels_before;
            destination[destination_start..destination_start + channels]
                .copy_from_slice(&arena[source_start..source_start + channels]);
            channels_before += channels;
        }
        channels_before = 0;
    }
}

fn scatter_buses(
    arena: &mut [f32],
    buses: &[PortSlot],
    source: &[f32],
    frames: usize,
    total_channels: u16,
) {
    let mut channels_before = 0;
    for frame in 0..frames {
        for bus in buses {
            let channels = usize::from(bus.channels);
            let source_start = frame * usize::from(total_channels) + channels_before;
            let destination_start = bus.offset + frame * channels;
            arena[destination_start..destination_start + channels]
                .copy_from_slice(&source[source_start..source_start + channels]);
            channels_before += channels;
        }
        channels_before = 0;
    }
}

fn preserve_main_buses(input: &[f32], output: &mut [f32], io: &MultipleHostedIo<'_>) {
    output.fill(0.0);
    let input_channels = usize::from(io.input_channels);
    let output_channels = usize::from(io.output_channels);
    for mapping in io.dry_maps {
        for frame in 0..io.frames {
            let source = frame * input_channels + mapping.source_channel_offset;
            let destination = frame * output_channels + mapping.destination_channel_offset;
            let channels = usize::from(mapping.channels);
            output[destination..destination + channels]
                .copy_from_slice(&input[source..source + channels]);
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
    minimum_gain: &'a mut f32,
    wet: &'a mut f32,
    wet_target: f32,
    bypass_coefficient: f32,
}

fn smooth_dynamics_wet(wet: &mut f32, target: f32, coefficient: f32) -> f32 {
    let next = coefficient.mul_add(target - *wet, *wet);
    *wet = if (target - next).abs() < 0.000_001 || next.to_bits() == wet.to_bits() {
        target
    } else {
        next
    };
    *wet
}

fn apply_compressor(context: DynamicKernelContext<'_>, parameters: CompressorKernelParameters) {
    let DynamicKernelContext {
        arena,
        input,
        output,
        frames,
        envelope,
        minimum_gain,
        wet,
        wet_target,
        bypass_coefficient,
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
        let reduction_gain = if *envelope > parameters.threshold_linear {
            (parameters.threshold_linear / *envelope).powf(parameters.exponent)
        } else {
            1.0
        };
        let wet = smooth_dynamics_wet(wet, wet_target, bypass_coefficient);
        *minimum_gain = minimum_gain.min(wet.mul_add(reduction_gain - 1.0, 1.0));
        let gain = wet.mul_add(reduction_gain * parameters.makeup_gain - 1.0, 1.0);
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
        minimum_gain,
        wet,
        wet_target,
        bypass_coefficient,
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
        let gain =
            smooth_dynamics_wet(wet, wet_target, bypass_coefficient).mul_add(gain - 1.0, 1.0);
        *minimum_gain = minimum_gain.min(gain);
        for channel in 0..channels {
            arena[output_frame + channel] = arena[input_frame + channel] * gain;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contiguous_mixer_keeps_scalar_sum_order_and_signed_zero() {
        for values in [
            [-0.0_f32, -0.0, -0.0],
            [1e20, -1e20, 3.25],
            [3.25, 1e20, -1e20],
        ] {
            let mut arena = [values[0], values[1], values[2], 99.0];
            let inputs = [0, 1, 2].map(|offset| PortSlot {
                offset,
                channels: 1,
            });
            apply_mixer(
                &mut arena,
                &inputs,
                PortSlot {
                    offset: 3,
                    channels: 1,
                },
                1,
            );
            assert_eq!(
                arena[3].to_bits(),
                values.into_iter().sum::<f32>().to_bits()
            );
        }
    }
}
