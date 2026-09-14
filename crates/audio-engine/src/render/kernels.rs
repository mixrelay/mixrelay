//! Realtime node kernels borrow prepared buffers and state; no graph preparation or control publication.
use super::{HostedAudioProcessor, Kernel, PortSlot, port_pair, sample_count};

pub(super) struct KernelResources<'a> {
    pub(super) arena: &'a mut [f32],
    pub(super) sources: &'a [Box<[f32]>],
    pub(super) sinks: &'a mut [Box<[f32]>],
    pub(super) dynamics_envelopes: &'a mut [f32],
    pub(super) device_bus_state: &'a mut [f32],
    pub(super) hosted_processors: &'a mut [Box<dyn HostedAudioProcessor>],
}

pub(super) fn apply_kernel(
    resources: KernelResources<'_>,
    kernel: &Kernel,
    linear_gain: f32,
    frames: usize,
) {
    match kernel {
        Kernel::DeviceBus(bus) => bus.apply(resources.arena, resources.device_bus_state, frames),
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
