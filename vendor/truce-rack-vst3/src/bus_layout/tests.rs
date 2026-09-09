use super::*;
use std::cell::{Cell, RefCell};
use truce_rack_core::buffer::BusRange;
use vst3::{
    Class, ComWrapper,
    Steinberg::{
        FUnknown, IBStream, IPluginBaseTrait, TUID,
        Vst::{ProcessData, ProcessSetup, RoutingInfo},
        kInvalidArgument,
    },
};

struct Native {
    inputs: RefCell<Vec<u64>>,
    outputs: RefCell<Vec<u64>>,
    reject: Cell<bool>,
    wrong_readback: Cell<bool>,
    negotiated: Cell<bool>,
    reject_activation: Cell<Option<(i32, i32)>>,
    activation: RefCell<Vec<(i32, i32, u8)>>,
}
impl Native {
    fn new(inputs: &[u64], outputs: &[u64]) -> Self {
        Self {
            inputs: RefCell::new(inputs.to_vec()),
            outputs: RefCell::new(outputs.to_vec()),
            reject: Cell::new(false),
            wrong_readback: Cell::new(false),
            negotiated: Cell::new(false),
            reject_activation: Cell::new(None),
            activation: RefCell::new(Vec::new()),
        }
    }
    fn masks(&self, direction: i32) -> &RefCell<Vec<u64>> {
        if direction == INPUT {
            &self.inputs
        } else {
            &self.outputs
        }
    }
}
impl Class for Native {
    type Interfaces = (IComponent, IAudioProcessor);
}
impl IPluginBaseTrait for Native {
    unsafe fn initialize(&self, _: *mut FUnknown) -> i32 {
        kResultOk
    }
    unsafe fn terminate(&self) -> i32 {
        kResultOk
    }
}
impl IComponentTrait for Native {
    unsafe fn getControllerClassId(&self, _: *mut TUID) -> i32 {
        kInvalidArgument
    }
    unsafe fn setIoMode(&self, _: i32) -> i32 {
        kResultOk
    }
    unsafe fn getBusCount(&self, _: i32, direction: i32) -> i32 {
        i32::try_from(self.masks(direction).borrow().len()).unwrap()
    }
    unsafe fn getBusInfo(&self, _: i32, direction: i32, index: i32, info: *mut BusInfo) -> i32 {
        let Ok(index) = usize::try_from(index) else {
            return kInvalidArgument;
        };
        let masks = self.masks(direction).borrow();
        let Some(mask) = masks.get(index) else {
            return kInvalidArgument;
        };
        unsafe {
            *info = BusInfo {
                mediaType: AUDIO,
                direction,
                channelCount: i32::try_from(mask.count_ones()).unwrap(),
                name: [0; 128],
                busType: i32::from(index > 0),
                flags: 0,
            };
        }
        kResultOk
    }
    unsafe fn getRoutingInfo(&self, _: *mut RoutingInfo, _: *mut RoutingInfo) -> i32 {
        kInvalidArgument
    }
    unsafe fn activateBus(&self, _: i32, direction: i32, index: i32, state: u8) -> i32 {
        self.activation.borrow_mut().push((direction, index, state));
        if state != 0 && self.reject_activation.get() == Some((direction, index)) {
            kInvalidArgument
        } else {
            kResultOk
        }
    }
    unsafe fn setActive(&self, _: u8) -> i32 {
        kResultOk
    }
    unsafe fn setState(&self, _: *mut IBStream) -> i32 {
        kResultOk
    }
    unsafe fn getState(&self, _: *mut IBStream) -> i32 {
        kResultOk
    }
}
impl IAudioProcessorTrait for Native {
    unsafe fn setBusArrangements(
        &self,
        inputs: *mut u64,
        ins: i32,
        outputs: *mut u64,
        outs: i32,
    ) -> i32 {
        if self.reject.get() {
            return kInvalidArgument;
        }
        for (pointer, count, direction) in [(inputs, ins, INPUT), (outputs, outs, OUTPUT)] {
            let Ok(count) = usize::try_from(count) else {
                return kInvalidArgument;
            };
            if count > 0 && pointer.is_null() {
                return kInvalidArgument;
            }
            *self.masks(direction).borrow_mut() = if count == 0 {
                vec![]
            } else {
                unsafe { std::slice::from_raw_parts(pointer, count) }.to_vec()
            };
        }
        self.negotiated.set(true);
        kResultOk
    }
    unsafe fn getBusArrangement(&self, direction: i32, index: i32, mask: *mut u64) -> i32 {
        let Ok(index) = usize::try_from(index) else {
            return kInvalidArgument;
        };
        let masks = self.masks(direction).borrow();
        let Some(value) = masks.get(index) else {
            return kInvalidArgument;
        };
        unsafe {
            *mask = *value ^ u64::from(self.wrong_readback.get() && self.negotiated.get());
        }
        kResultOk
    }
    unsafe fn canProcessSampleSize(&self, _: i32) -> i32 {
        kResultOk
    }
    unsafe fn getLatencySamples(&self) -> u32 {
        0
    }
    unsafe fn setupProcessing(&self, _: *mut ProcessSetup) -> i32 {
        kResultOk
    }
    unsafe fn setProcessing(&self, _: u8) -> i32 {
        kResultOk
    }
    unsafe fn getTailSamples(&self) -> u32 {
        0
    }
    unsafe fn process(&self, data: *mut ProcessData) -> i32 {
        let data = unsafe { &mut *data };
        let inputs = self.inputs.borrow();
        let outputs = self.outputs.borrow();
        if usize::try_from(data.numInputs).ok() != Some(inputs.len())
            || usize::try_from(data.numOutputs).ok() != Some(outputs.len())
        {
            return kInvalidArgument;
        }
        for (index, expected) in inputs.iter().enumerate() {
            let input = unsafe { &*data.inputs.add(index) };
            if u32::try_from(input.numChannels).ok() != Some(expected.count_ones()) {
                return kInvalidArgument;
            }
        }
        for (index, expected) in outputs.iter().enumerate() {
            let output = unsafe { &mut *data.outputs.add(index) };
            if u32::try_from(output.numChannels).ok() != Some(expected.count_ones()) {
                return kInvalidArgument;
            }
            for channel in 0..usize::try_from(output.numChannels).unwrap() {
                let source = inputs
                    .get(index)
                    .filter(|mask| channel < mask.count_ones() as usize)
                    .map(|_| unsafe {
                        *(*data.inputs.add(index))
                            .__field0
                            .channelBuffers32
                            .add(channel)
                    });
                let destination = unsafe { *output.__field0.channelBuffers32.add(channel) };
                for frame in 0..usize::try_from(data.numSamples).unwrap() {
                    unsafe {
                        *destination.add(frame) = source.map_or(0.0, |source| *source.add(frame));
                    }
                }
            }
        }
        kResultOk
    }
}

fn ranges(buses: &[Bus]) -> Vec<BusRange> {
    let mut offset = 0;
    buses
        .iter()
        .map(|bus| {
            let count = bus.channels.count() as usize;
            let range = BusRange::new(offset, count);
            offset += count;
            range
        })
        .collect()
}

fn process(
    processor: &ComPtr<IAudioProcessor>,
    buffer: &mut AudioBuffer<'_, f32>,
    layout: &BusLayout,
) -> Result<i32> {
    with_buffers(buffer, layout, |inputs, outputs| {
        let mut data = ProcessData {
            processMode: 0,
            symbolicSampleSize: 0,
            numSamples: 2,
            numInputs: i32::try_from(inputs.len()).unwrap(),
            numOutputs: i32::try_from(outputs.len()).unwrap(),
            inputs: inputs.as_mut_ptr(),
            outputs: outputs.as_mut_ptr(),
            inputParameterChanges: ptr::null_mut(),
            outputParameterChanges: ptr::null_mut(),
            inputEvents: ptr::null_mut(),
            outputEvents: ptr::null_mut(),
            processContext: ptr::null_mut(),
        };
        unsafe { processor.process(&raw mut data) }
    })
}

#[test]
fn actual_mono_stereo_discrete_seven_and_sixty_four_arrangements_reach_all_pcm_channels() {
    for mask in [SpeakerArr::kMono, SpeakerArr::kStereo, 0x7f, u64::MAX] {
        let native = ComWrapper::new(Native::new(&[mask], &[mask]));
        let component = native.to_com_ptr::<IComponent>().unwrap();
        let processor = native.to_com_ptr::<IAudioProcessor>().unwrap();
        let layout = discover(&component, &processor).unwrap();
        assert_eq!(layout.total_input_channels(), mask.count_ones());
        negotiate(&component, &processor, &layout).unwrap();
        activate_buses(&component, &layout, true).unwrap();
        let channels = mask.count_ones() as usize;
        let input = vec![[0.375_f32, -0.625]; channels];
        let mut output = vec![[0.0_f32; 2]; channels];
        let input_refs: Vec<_> = input.iter().map(<[f32; 2]>::as_slice).collect();
        let mut output_refs: Vec<_> = output.iter_mut().map(<[f32; 2]>::as_mut_slice).collect();
        let bus_range = [BusRange::new(0, channels)];
        let mut buffer = AudioBuffer::new(&input_refs, &mut output_refs, 2, &bus_range, &bus_range);
        assert_eq!(
            process(&processor, &mut buffer, &layout).unwrap(),
            kResultOk
        );
        assert!(
            input
                .iter()
                .zip(output.iter())
                .all(|(a, b)| a.map(f32::to_bits) == b.map(f32::to_bits))
        );
        activate_buses(&component, &layout, false).unwrap();
    }
}

#[test]
fn sidechain_auxiliary_and_asymmetric_buses_are_not_silently_dropped() {
    let native = ComWrapper::new(Native::new(
        &[SpeakerArr::kStereo, SpeakerArr::kMono],
        &[SpeakerArr::kStereo, SpeakerArr::kMono],
    ));
    let component = native.to_com_ptr::<IComponent>().unwrap();
    let processor = native.to_com_ptr::<IAudioProcessor>().unwrap();
    let layout = discover(&component, &processor).unwrap();
    assert_eq!(layout.inputs[1].kind, BusKind::Sidechain);
    assert_eq!(layout.outputs[1].kind, BusKind::Auxiliary);
    negotiate(&component, &processor, &layout).unwrap();
    let input = [[0.125_f32; 2], [0.25; 2], [0.75; 2]];
    let mut output = [[0.0_f32; 2]; 3];
    let refs: Vec<_> = input.iter().map(<[f32; 2]>::as_slice).collect();
    let mut out_refs: Vec<_> = output.iter_mut().map(<[f32; 2]>::as_mut_slice).collect();
    let ins = ranges(&layout.inputs);
    let outs = ranges(&layout.outputs);
    let mut buffer = AudioBuffer::new(&refs, &mut out_refs, 2, &ins, &outs);
    assert_eq!(
        process(&processor, &mut buffer, &layout).unwrap(),
        kResultOk
    );
    assert_eq!(
        input.map(|v| v.map(f32::to_bits)),
        output.map(|v| v.map(f32::to_bits))
    );
    let mut requested = layout.clone();
    requested.outputs[0].channels = ChannelConfig::Mono;
    negotiate(&component, &processor, &requested).unwrap();
    assert_eq!(native.outputs.borrow()[0], SpeakerArr::kMono);
}

#[test]
fn negotiation_rejection_readback_mismatch_and_activation_failure_are_errors() {
    let native = ComWrapper::new(Native::new(&[SpeakerArr::kStereo], &[SpeakerArr::kStereo]));
    let component = native.to_com_ptr::<IComponent>().unwrap();
    let processor = native.to_com_ptr::<IAudioProcessor>().unwrap();
    native.reject.set(true);
    assert!(negotiate(&component, &processor, &BusLayout::mono()).is_err());
    native.reject.set(false);
    native.wrong_readback.set(true);
    assert!(negotiate(&component, &processor, &BusLayout::mono()).is_err());
    native.wrong_readback.set(false);
    native.reject_activation.set(Some((OUTPUT, 0)));
    assert!(activate_buses(&component, &BusLayout::mono(), true).is_err());
    assert!(native.activation.borrow().contains(&(INPUT, 0, 0)));
    assert!(native.activation.borrow().contains(&(OUTPUT, 0, 0)));
    let mut invalid = BusLayout::mono();
    invalid.inputs[0].channels = ChannelConfig::Discrete(65);
    assert!(negotiate(&component, &processor, &invalid).is_err());
    assert!(negotiate(&component, &processor, &BusLayout::new()).is_err());
}

#[test]
fn mismatched_process_geometry_is_rejected_before_the_native_callback() {
    let input: [&[f32]; 1] = [&[0.0_f32; 2]];
    let mut output = [0.0_f32; 2];
    let mut outputs: [&mut [f32]; 1] = [&mut output];
    let ranges = [BusRange::new(0, 1)];
    let mut buffer = AudioBuffer::new(&input, &mut outputs, 2, &ranges, &ranges);
    assert!(
        with_buffers(&mut buffer, &BusLayout::stereo(), |_, _| panic!(
            "must not call native processor"
        ))
        .is_err()
    );
}

#[test]
fn stack_bus_binding_removes_twenty_thousand_allocations_over_ten_thousand_stereo_blocks() {
    let input: [&[f32]; 2] = [&[0.0_f32; 2], &[0.0_f32; 2]];
    let mut left = [0.0_f32; 2];
    let mut right = [0.0_f32; 2];
    let mut output: [&mut [f32]; 2] = [&mut left, &mut right];
    let ranges = [BusRange::new(0, 2)];
    let mut buffer = AudioBuffer::new(&input, &mut output, 2, &ranges, &ranges);
    let layout = BusLayout::stereo();
    let before = crate::parameter_delivery::count_allocations(|| {
        for _ in 0..10_000 {
            let ins: Vec<*mut f32> = buffer
                .main_inputs()
                .iter()
                .map(|channel| channel.as_ptr().cast_mut())
                .collect();
            let outs: Vec<*mut f32> = buffer
                .main_outputs()
                .iter_mut()
                .map(|channel| channel.as_mut_ptr())
                .collect();
            std::hint::black_box((ins, outs));
        }
    });
    let after = crate::parameter_delivery::count_allocations(|| {
        for _ in 0..10_000 {
            with_buffers(&mut buffer, &layout, |inputs, outputs| {
                std::hint::black_box((inputs, outputs));
            })
            .unwrap();
        }
    });
    assert_eq!(before, 20_000);
    assert_eq!(after, 0);
}
