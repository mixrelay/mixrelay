//! Native bus facts, checked activation, and bounded per-block bus views.
//! A channel count is not a speaker mask: unknown/discrete layouts preserve the
//! arrangement reported by the plugin instead of silently forcing stereo.

use std::ptr;
use truce_rack_core::{
    buffer::AudioBuffer,
    bus::{Bus, BusKind, BusLayout, ChannelConfig},
    error::{Error, Result},
};
use vst3::{
    ComPtr,
    Steinberg::{
        Vst::{
            AudioBusBuffers, AudioBusBuffers__type0, BusInfo, IAudioProcessor,
            IAudioProcessorTrait, IComponent, IComponentTrait, SpeakerArr,
        },
        kResultOk,
    },
};

const MAX_CHANNELS: usize = 64;
const MAX_BUSES: usize = 64;
const AUDIO: i32 = 0;
const INPUT: i32 = 0;
const OUTPUT: i32 = 1;

fn invalid(message: &str) -> Error {
    Error::Other(format!("VST3 bus layout: {message}"))
}

fn bus_count(component: &ComPtr<IComponent>, direction: i32) -> Result<usize> {
    let count = usize::try_from(unsafe { component.getBusCount(AUDIO, direction) })
        .map_err(|_| invalid("negative bus count"))?;
    if count > MAX_BUSES {
        return Err(invalid("bus count exceeds host capacity"));
    }
    Ok(count)
}

fn bus_info(component: &ComPtr<IComponent>, direction: i32, index: usize) -> Result<BusInfo> {
    let mut info = BusInfo {
        mediaType: AUDIO,
        direction,
        channelCount: 0,
        name: [0; 128],
        busType: 0,
        flags: 0,
    };
    let index = i32::try_from(index).map_err(|_| invalid("bus index overflow"))?;
    if unsafe { component.getBusInfo(AUDIO, direction, index, &raw mut info) } != kResultOk {
        return Err(invalid("getBusInfo failed"));
    }
    if info.mediaType != AUDIO
        || info.direction != direction
        || !(0..=64).contains(&info.channelCount)
    {
        return Err(invalid("invalid native bus geometry"));
    }
    Ok(info)
}

fn arrangement(processor: &ComPtr<IAudioProcessor>, direction: i32, index: usize) -> Result<u64> {
    let mut mask = 0;
    let index = i32::try_from(index).map_err(|_| invalid("bus index overflow"))?;
    if unsafe { processor.getBusArrangement(direction, index, &raw mut mask) } != kResultOk {
        return Err(invalid("getBusArrangement failed"));
    }
    Ok(mask)
}

fn discover_buses(
    component: &ComPtr<IComponent>,
    processor: &ComPtr<IAudioProcessor>,
    direction: i32,
) -> Result<Vec<Bus>> {
    let mut buses = Vec::new();
    let mut total = 0;
    for index in 0..bus_count(component, direction)? {
        let info = bus_info(component, direction, index)?;
        let mask = arrangement(processor, direction, index)?;
        let count =
            u32::try_from(info.channelCount).map_err(|_| invalid("negative channel count"))?;
        if mask.count_ones() != count {
            return Err(invalid("channel count and speaker arrangement differ"));
        }
        total += count;
        if total > 64 {
            return Err(invalid("total channel count exceeds host capacity"));
        }
        let channels = match mask {
            SpeakerArr::kMono => ChannelConfig::Mono,
            SpeakerArr::kStereo => ChannelConfig::Stereo,
            SpeakerArr::k51 => ChannelConfig::Surround5_1,
            SpeakerArr::k71Music => ChannelConfig::Surround7_1,
            _ => ChannelConfig::Discrete(count),
        };
        let kind = if info.busType == 0 {
            BusKind::Main
        } else if direction == INPUT {
            BusKind::Sidechain
        } else {
            BusKind::Auxiliary
        };
        buses.push(Bus {
            name: super::string128_to_string(&info.name),
            kind,
            channels,
        });
    }
    Ok(buses)
}

pub(super) fn discover(
    component: &ComPtr<IComponent>,
    processor: &ComPtr<IAudioProcessor>,
) -> Result<BusLayout> {
    Ok(BusLayout {
        inputs: discover_buses(component, processor, INPUT)?
            .into_iter()
            .collect(),
        outputs: discover_buses(component, processor, OUTPUT)?
            .into_iter()
            .collect(),
    })
}

fn requested_masks(
    component: &ComPtr<IComponent>,
    processor: &ComPtr<IAudioProcessor>,
    direction: i32,
    buses: &[Bus],
) -> Result<Vec<u64>> {
    if buses.len() != bus_count(component, direction)? {
        return Err(invalid("requested bus count differs from native topology"));
    }
    let mut total = 0;
    buses
        .iter()
        .enumerate()
        .map(|(index, bus)| {
            let count = bus.channels.count();
            total += u64::from(count);
            if total > 64 {
                return Err(invalid("requested channels exceed host capacity"));
            }
            let mask = match bus.channels {
                ChannelConfig::Mono => SpeakerArr::kMono,
                ChannelConfig::Stereo => SpeakerArr::kStereo,
                ChannelConfig::Surround5_1 => SpeakerArr::k51,
                ChannelConfig::Surround7_1 => SpeakerArr::k71Music,
                ChannelConfig::Discrete(_) => arrangement(processor, direction, index)?,
            };
            if mask.count_ones() != count {
                return Err(invalid(
                    "discrete arrangement does not match requested channels",
                ));
            }
            Ok(mask)
        })
        .collect()
}

pub(super) fn negotiate(
    component: &ComPtr<IComponent>,
    processor: &ComPtr<IAudioProcessor>,
    layout: &BusLayout,
) -> Result<()> {
    let mut inputs = requested_masks(component, processor, INPUT, &layout.inputs)?;
    let mut outputs = requested_masks(component, processor, OUTPUT, &layout.outputs)?;
    let in_count = i32::try_from(inputs.len()).expect("bounded bus count");
    let out_count = i32::try_from(outputs.len()).expect("bounded bus count");
    let in_ptr = if inputs.is_empty() {
        ptr::null_mut()
    } else {
        inputs.as_mut_ptr()
    };
    let out_ptr = if outputs.is_empty() {
        ptr::null_mut()
    } else {
        outputs.as_mut_ptr()
    };
    if unsafe { processor.setBusArrangements(in_ptr, in_count, out_ptr, out_count) } != kResultOk {
        return Err(invalid("setBusArrangements rejected the requested layout"));
    }
    for (direction, masks) in [(INPUT, inputs), (OUTPUT, outputs)] {
        for (index, expected) in masks.into_iter().enumerate() {
            let info = bus_info(component, direction, index)?;
            let actual = arrangement(processor, direction, index)?;
            if actual != expected
                || u32::try_from(info.channelCount).ok() != Some(expected.count_ones())
            {
                return Err(invalid("negotiated layout readback differs from request"));
            }
        }
    }
    Ok(())
}

pub(super) fn activate_buses(
    component: &ComPtr<IComponent>,
    layout: &BusLayout,
    active: bool,
) -> Result<()> {
    for (direction, count) in [(INPUT, layout.inputs.len()), (OUTPUT, layout.outputs.len())] {
        for index in 0..count {
            let index = i32::try_from(index).map_err(|_| invalid("bus index overflow"))?;
            if unsafe { component.activateBus(AUDIO, direction, index, u8::from(active)) }
                != kResultOk
                && active
            {
                // Include the failed bus in cleanup: a plugin may mutate before returning failure.
                let _ = activate_buses(component, layout, false);
                return Err(invalid("activateBus failed"));
            }
        }
    }
    Ok(())
}

fn empty_bus() -> AudioBusBuffers {
    AudioBusBuffers {
        numChannels: 0,
        silenceFlags: 0,
        __field0: AudioBusBuffers__type0 {
            channelBuffers32: ptr::null_mut(),
        },
    }
}

/// Borrowed sample pointers live only for the synchronous closure. Stack arrays
/// cover the host's channel/bus limits and replace per-block channel Vec allocation.
pub(super) fn with_buffers<T>(
    buffer: &mut AudioBuffer<'_, f32>,
    layout: &BusLayout,
    process: impl FnOnce(&mut [AudioBusBuffers], &mut [AudioBusBuffers]) -> T,
) -> Result<T> {
    if layout.inputs.len() > MAX_BUSES
        || layout.outputs.len() > MAX_BUSES
        || buffer.num_input_buses() != layout.inputs.len()
        || buffer.num_output_buses() != layout.outputs.len()
        || buffer.total_input_channels() > MAX_CHANNELS
        || buffer.total_output_channels() > MAX_CHANNELS
    {
        return Err(invalid(
            "process buffer topology differs from active layout",
        ));
    }
    let mut input_ptrs = [ptr::null_mut(); MAX_CHANNELS];
    let mut output_ptrs = [ptr::null_mut(); MAX_CHANNELS];
    let mut inputs = std::array::from_fn::<_, MAX_BUSES, _>(|_| empty_bus());
    let mut outputs = std::array::from_fn::<_, MAX_BUSES, _>(|_| empty_bus());
    let frames = buffer.num_frames();
    let mut cursor = 0;
    for (index, expected) in layout.inputs.iter().enumerate() {
        let channels = buffer.bus_inputs(index);
        if channels.len() != expected.channels.count() as usize
            || channels.iter().any(|channel| channel.len() < frames)
        {
            return Err(invalid("input bus dimensions differ"));
        }
        for (slot, channel) in input_ptrs[cursor..].iter_mut().zip(channels) {
            *slot = channel.as_ptr().cast_mut();
        }
        inputs[index].numChannels = i32::try_from(channels.len()).expect("bounded channel count");
        inputs[index].__field0.channelBuffers32 = input_ptrs[cursor..].as_mut_ptr();
        cursor += channels.len();
    }
    cursor = 0;
    for (index, expected) in layout.outputs.iter().enumerate() {
        let channels = buffer.bus_outputs(index);
        if channels.len() != expected.channels.count() as usize
            || channels.iter().any(|channel| channel.len() < frames)
        {
            return Err(invalid("output bus dimensions differ"));
        }
        for (slot, channel) in output_ptrs[cursor..].iter_mut().zip(channels.iter_mut()) {
            *slot = channel.as_mut_ptr();
        }
        outputs[index].numChannels = i32::try_from(channels.len()).expect("bounded channel count");
        outputs[index].__field0.channelBuffers32 = output_ptrs[cursor..].as_mut_ptr();
        cursor += channels.len();
    }
    Ok(process(
        &mut inputs[..layout.inputs.len()],
        &mut outputs[..layout.outputs.len()],
    ))
}

#[cfg(test)]
mod tests;
