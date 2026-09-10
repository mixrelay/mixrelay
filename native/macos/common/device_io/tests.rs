use super::*;
use crate::{
    aggregate_group::NativeBinding,
    audio_buffer::AudioBuffer,
    callback_clock::{CallbackClockSnapshot, mr_callback_clock_snapshot},
    device_io::lifecycle::{Backend, Created},
};
use std::{
    cell::Cell,
    ffi::c_void,
    ptr::{self, NonNull},
    sync::atomic::Ordering::Acquire,
};

#[repr(C)]
struct Buffers {
    count: u32,
    buffers: [AudioBuffer; 64],
}

fn buffers(samples: &mut [f32], channels: u32, frames: u32, planar: bool) -> Buffers {
    let mut result = Buffers {
        count: if planar { channels } else { 1 },
        buffers: std::array::from_fn(|_| AudioBuffer {
            channels: 0,
            byte_size: 0,
            data: ptr::null_mut(),
        }),
    };
    for index in 0..result.count as usize {
        let width = if planar { 1 } else { channels };
        result.buffers[index] = AudioBuffer {
            channels: width,
            byte_size: frames * width * 4,
            // SAFETY: test allocation contains count*frames*width floats.
            data: unsafe {
                samples
                    .as_mut_ptr()
                    .add(if planar { index * frames as usize } else { 0 })
            },
        };
    }
    result
}

fn owner(input: bool, channels: u32) -> DeviceIo {
    DeviceIo::new(c"unit", input, channels, 48000).expect("test owner allocation")
}

fn process(io: &DeviceIo, buffers: &mut Buffers) {
    let native = ptr::from_mut(buffers).cast();
    // SAFETY: tests serialize each role and supply complete metadata/PCM allocations.
    unsafe {
        mr_device_io_process(
            0,
            ptr::null(),
            if io.selection.input {
                native
            } else {
                ptr::null()
            },
            ptr::null(),
            if io.selection.input {
                ptr::null_mut()
            } else {
                native
            },
            ptr::null(),
            ptr::from_ref(io).cast_mut().cast(),
        );
    }
}

fn sample(value: impl TryInto<u16>) -> f32 {
    f32::from(value.try_into().ok().expect("test sample values fit u16"))
}

fn assert_pcm(left: &[f32], right: &[f32]) {
    assert_eq!(left.len(), right.len());
    for (left, right) in left.iter().zip(right) {
        assert_eq!(left.to_bits(), right.to_bits());
    }
}

fn samples(channels: u32, frames: u32) -> Vec<f32> {
    (0..channels * frames)
        .map(|index| sample(index % channels + 1) / 128.0)
        .collect()
}

#[test]
fn all_channel_widths_preserve_three_owner_queues_and_recovery_epochs() {
    for channels in 1..=64 {
        let input = owner(true, channels);
        let output = owner(false, channels);
        let mut source = samples(channels, 64);
        let mut destination = vec![0.0; source.len()];
        let mut scratch = destination.clone();
        let mut list = buffers(&mut source, channels, 64, false);
        process(&input, &mut list);
        // SAFETY: every test role runs serially with complete disjoint PCM spans.
        unsafe {
            assert_eq!(input.progress.native_frames.load(Acquire), 64);
            assert_eq!(input.graph_ring.stats().queued, 0);
            assert_eq!(queues::worker_read(&input, scratch.as_mut_ptr(), 64), 64);
            assert_pcm(&scratch, &source);
            assert_eq!(queues::worker_read(&input, scratch.as_mut_ptr(), 64), 0);
            assert_eq!(
                queues::graph_read(&input, destination.as_mut_ptr(), 64, 1),
                0
            );
            for _ in 0..3 {
                assert_eq!(queues::worker_write(&input, source.as_ptr(), 64), 64);
            }
            assert_eq!(
                queues::graph_read(&input, destination.as_mut_ptr(), 64, 1),
                64
            );
            assert_pcm(&destination, &source);
            assert_eq!(
                queues::graph_read(&input, destination.as_mut_ptr(), 64, 2),
                0
            );
            assert_eq!(input.graph_ring.stats().queued, 0);
            assert_eq!(input.progress.graph_frames.load(Acquire), 192);
            assert_eq!(queues::graph_write(&output, source.as_ptr(), 64), 64);
            assert_eq!(output.native_ring.stats().queued, 0);
            assert_eq!(queues::worker_read(&output, scratch.as_mut_ptr(), 64), 64);
            assert_pcm(&scratch, &source);
            assert_eq!(queues::worker_write(&output, scratch.as_ptr(), 64), 64);
        }
        let mut list = buffers(&mut destination, channels, 64, false);
        process(&output, &mut list);
        assert_pcm(&destination, &source);
        output
            .progress
            .output_epoch
            .store(1, std::sync::atomic::Ordering::Release);
        process(&output, &mut list);
        assert!(destination.iter().all(|sample| sample.to_bits() == 0));
        assert_eq!(output.native_ring.stats().queued, 0);
        assert_eq!(output.progress.native_quantum.load(Acquire), 64);
        assert_eq!(output.progress.graph_quantum.load(Acquire), 64);
        assert_eq!(output.progress.native_frames.load(Acquire), 128);
        // SAFETY: serialized worker owns the graph output consumer cursor.
        unsafe {
            queues::graph_write(&output, source.as_ptr(), 64);
            queues::worker_discard(&output);
        }
        assert_eq!(output.graph_ring.stats().queued, 0);
    }
}

fn pcm_case(channels: u32, planar: bool) {
    let input = owner(true, channels);
    let output = owner(false, channels);
    let source = samples(channels, 64);
    let mut hardware = vec![0.0; source.len()];
    for frame in 0..64 {
        for channel in 0..channels as usize {
            hardware[if planar {
                channel * 64 + frame
            } else {
                frame * channels as usize + channel
            }] = source[frame * channels as usize + channel];
        }
    }
    let mut list = buffers(&mut hardware, channels, 64, planar);
    process(&input, &mut list);
    let mut copy = vec![0.0; source.len()];
    // SAFETY: sole worker consumer/producer; full disjoint test PCM allocations.
    unsafe {
        assert_eq!(queues::worker_read(&input, copy.as_mut_ptr(), 64), 64);
        assert_pcm(&copy, &source);
        assert_eq!(queues::worker_write(&output, copy.as_ptr(), 64), 64);
    }
    process(&output, &mut list);
    for frame in 0..64 {
        for channel in 0..channels as usize {
            assert_eq!(
                hardware[if planar {
                    channel * 64 + frame
                } else {
                    frame * channels as usize + channel
                }]
                .to_bits(),
                source[frame * channels as usize + channel].to_bits()
            );
        }
    }
    // SAFETY: no registered callback; fixture runs synchronously.
    unsafe {
        (*output.callback.get()).prime = true;
    }
    process(&output, &mut list);
    assert!(hardware.iter().all(|value| value.to_bits() == 0));
    list.buffers[0].channels = 0;
    process(&output, &mut list);
    assert!(output.progress.invalid.load(Acquire));
    let ticks = output.progress.ticks.load(Acquire);
    list.buffers[0].channels = if planar { 1 } else { channels };
    hardware.fill(1.0);
    process(&output, &mut list);
    assert_eq!(output.progress.ticks.load(Acquire), ticks);
    assert!(hardware.iter().all(|value| value.to_bits() == 0));
}

#[test]
fn callback_layouts_priming_and_terminal_invalid_latch() {
    for _ in 0..32 {
        for channels in [1, 2, 8, 16, 32, 64] {
            pcm_case(channels, false);
            pcm_case(channels, true);
        }
    }
}

#[test]
fn mixed_stream_subset_preserves_selected_channels_and_silences_neighbors() {
    let input = owner(true, 4);
    let output = owner(false, 4);
    let mut hardware = [0.0; 64 * 6];
    let mut list = buffers(&mut hardware, 6, 64, false);
    list.count = 3;
    let mut offset = 0;
    for (index, width) in [1, 2, 3].into_iter().enumerate() {
        for frame in 0..64 {
            for channel in 0..width {
                hardware[offset * 64 + frame * width + channel] =
                    sample(offset + channel + 1) / 128.0;
            }
        }
        // SAFETY: each stream owns its disjoint 64*width subspan.
        list.buffers[index] = AudioBuffer {
            channels: u32::try_from(width).unwrap(),
            byte_size: u32::try_from(64 * width * 4).unwrap(),
            data: unsafe { hardware.as_mut_ptr().add(offset * 64) },
        };
        offset += width;
    }
    process(&input, &mut list);
    let mut graph = [0.0; 64 * 4];
    // SAFETY: worker role exclusively consumes/produces complete disjoint spans.
    unsafe {
        assert_eq!(queues::worker_read(&input, graph.as_mut_ptr(), 64), 64);
        assert_eq!(queues::worker_write(&output, graph.as_ptr(), 64), 64);
    }
    assert_pcm(&graph, &samples(4, 64));
    process(&output, &mut list);
    offset = 0;
    for width in [1, 2, 3] {
        for frame in 0..64 {
            for channel in 0..width {
                let expected = if offset + channel < 4 {
                    sample(offset + channel + 1) / 128.0
                } else {
                    0.0
                };
                assert_eq!(
                    hardware[offset * 64 + frame * width + channel].to_bits(),
                    expected.to_bits()
                );
            }
        }
        offset += width;
    }
}

fn aggregate_slice(offset: u32, channels: u32, input: bool) {
    let io = owner(input, channels);
    // SAFETY: fixture has no native registration; direct callback selection is the
    // resolved aggregate projection, without inventing a dangling aggregate owner.
    unsafe {
        (*io.callback.get()).selection = BufferSelection {
            channels,
            offset,
            total: 128,
            limit: 2048,
        };
    }
    let mut hardware: Vec<f32> = (0..512).map(sample).collect();
    let mut selected = Vec::new();
    for frame in 0..4 {
        for channel in 0..channels {
            selected.push(sample(frame * 128 + offset + channel));
        }
    }
    let mut list = buffers(&mut hardware, 64, 4, false);
    list.count = 2;
    // Pack two streams with the same global frame/channel values.
    for frame in 0..4 {
        for channel in 0..64 {
            hardware[frame * 64 + channel] = sample(frame * 128 + channel);
            hardware[256 + frame * 64 + channel] = sample(frame * 128 + channel + 64);
        }
    }
    // SAFETY: second stream is the disjoint second half of 512 initialized samples.
    list.buffers[1] = AudioBuffer {
        channels: 64,
        byte_size: 1024,
        data: unsafe { hardware.as_mut_ptr().add(256) },
    };
    // SAFETY: serialized output worker owns this ring's producer cursor.
    if !input {
        unsafe {
            assert_eq!(queues::worker_write(&io, selected.as_ptr(), 4), 4);
        }
    }
    process(&io, &mut list);
    assert!(!io.progress.invalid.load(Acquire));
    if input {
        let mut read = vec![0.0; selected.len()];
        // SAFETY: sole input worker consumer and exact-sized writable output.
        unsafe {
            assert_eq!(queues::worker_read(&io, read.as_mut_ptr(), 4), 4);
        }
        assert_pcm(&read, &selected);
    } else {
        for frame in 0..4 {
            for channel in 0..128 {
                let position = if channel < 64 {
                    frame * 64 + channel
                } else {
                    256 + frame * 64 + channel - 64
                };
                let expected = if (offset as usize..(offset + channels) as usize).contains(&channel)
                {
                    sample(frame * 128 + channel)
                } else {
                    0.0
                };
                assert_eq!(hardware[position].to_bits(), expected.to_bits());
            }
        }
    }
    list.count = 1;
    process(&io, &mut list);
    assert!(io.progress.invalid.load(Acquire));
}

#[test]
fn aggregate_slices_cover_offsets_and_member_disappearance() {
    for offset in [1, 13, 33, 64] {
        for channels in [1, 2, 7, 32, 64] {
            aggregate_slice(offset, channels, true);
            aggregate_slice(offset, channels, false);
        }
    }
}

#[derive(Default)]
struct RegistrationStub {
    create_status: i32,
    start_status: i32,
    destroy_status: i32,
    prepares: usize,
    starts: usize,
    stops: usize,
    destroys: usize,
}

struct StubBackend<'a> {
    io: &'a DeviceIo,
    native: &'a mut RegistrationStub,
}

impl Backend for StubBackend<'_> {
    fn prepare(&mut self, _: u32) -> Result<u32, i32> {
        self.native.prepares += 1;
        Ok(123)
    }
    fn create(&mut self, _: u32) -> Created {
        Created {
            procedure: Some(NonNull::dangling()),
            status: self.native.create_status,
        }
    }
    fn start(&mut self, _: u32, _: NonNull<c_void>) -> i32 {
        self.native.starts += 1;
        self.native.start_status
    }
    fn silence(&mut self) {
        self.io
            .progress
            .invalid
            .store(true, std::sync::atomic::Ordering::Release);
    }
    fn stop(&mut self, _: u32, _: NonNull<c_void>) -> i32 {
        self.native.stops += 1;
        23
    }
    fn destroy(&mut self, _: u32, _: NonNull<c_void>) -> i32 {
        self.native.destroys += 1;
        self.native.destroy_status
    }
}

fn register(io: &DeviceIo, native: &mut RegistrationStub) -> i32 {
    // SAFETY: unit fixture owns Control exclusively; stub callback does not access it.
    unsafe {
        (*io.control.get())
            .lifecycle
            .connect(0, &mut StubBackend { io, native })
    }
}

fn retire(io: *mut DeviceIo, native: &mut RegistrationStub) -> bool {
    // SAFETY: exact production allocation release path with a synchronous mock
    // native acknowledgement. No worker/graph survives this test-owned allocation.
    unsafe {
        ffi::destroy_with(io, |io| {
            (*io.control.get())
                .lifecycle
                .disconnect(&mut StubBackend { io, native })
        })
    }
}

#[test]
fn failed_teardown_and_partial_create_keep_complete_owner_until_acknowledged() {
    for (create_status, start_status) in [(0, 0), (17, 0), (0, 19)] {
        let pointer = owner(false, 2).publish();
        assert!(!pointer.is_null());
        // SAFETY: test retains published allocation until its successful retirement below.
        let io = unsafe { &*pointer };
        let mut native = RegistrationStub {
            create_status,
            start_status,
            destroy_status: 23,
            ..RegistrationStub::default()
        };
        let uid = io.selection.uid.as_ptr();
        // SAFETY: no callback registered; scratch allocation identity read only here.
        let scratch = unsafe { (*io.callback.get()).scratch.as_ptr() };
        let pcm = [0.0; 128];
        // SAFETY: sole graph/worker producers and complete immutable block.
        unsafe {
            assert_eq!(queues::graph_write(io, pcm.as_ptr(), 64), 64);
            assert_eq!(queues::worker_write(io, pcm.as_ptr(), 64), 64);
        }
        assert_eq!(register(io, &mut native), create_status.max(start_status));
        assert!(!retire(pointer, &mut native));
        assert_eq!(io.selection.uid.as_ptr(), uid);
        // SAFETY: failed retirement leaves complete allocation and scratch untouched.
        assert_eq!(unsafe { (*io.callback.get()).scratch.as_ptr() }, scratch);
        assert_eq!(io.native_ring.stats().queued, 64);
        assert_eq!(io.graph_ring.stats().queued, 64);
        let prepares = native.prepares;
        assert_eq!(register(io, &mut native), -1);
        assert_eq!(native.prepares, prepares);
        let mut hardware = [1.0; 128];
        process(io, &mut buffers(&mut hardware, 2, 64, false));
        assert!(hardware.iter().all(|sample| sample.to_bits() == 0));
        assert_eq!(io.progress.ticks.load(Acquire), 0);
        native.destroy_status = if create_status != 0 {
            i32::from_be_bytes(*b"!obj")
        } else {
            0
        };
        assert!(retire(pointer, &mut native));
        assert_eq!(native.stops, native.destroys);
    }
}

struct PropertiesStub {
    rate: f64,
    default: u32,
    transport: Option<u32>,
    stream_size: u32,
    default_queries: Cell<u32>,
}

impl Default for PropertiesStub {
    fn default() -> Self {
        Self {
            rate: 48000.0,
            default: 234,
            transport: Some(u32::from_be_bytes(*b"usb ")),
            stream_size: 4,
            default_queries: Cell::new(0),
        }
    }
}

fn scalar_copy<T: Copy + Default, U: Copy>(value: U) -> Option<T> {
    if size_of::<T>() != size_of::<U>() {
        return None;
    }
    let mut result = T::default();
    // SAFETY: property test supplies the exact private query type and checked ABI size.
    unsafe {
        ptr::copy_nonoverlapping(
            ptr::from_ref(&value).cast::<u8>(),
            ptr::from_mut(&mut result).cast(),
            size_of::<T>(),
        );
    }
    Some(result)
}

impl native::Properties for PropertiesStub {
    fn read<T: Copy + Default>(&self, _: u32, address: native::Address) -> Option<T> {
        match address.selector.to_be_bytes() {
            [b'd', b'I', b'n', b' '] | [b'd', b'O', b'u', b't'] => {
                self.default_queries.set(self.default_queries.get() + 1);
                scalar_copy(self.default)
            }
            [b't', b'r', b'a', b'n'] => self.transport.and_then(scalar_copy),
            [b'l', b'i', b'v', b'n'] => scalar_copy(1_u32),
            [b's', b'f', b'm', b't'] => scalar_copy(native::Description {
                rate: self.rate,
                format: u32::from_be_bytes(*b"lpcm"),
                flags: 1,
                bits: 32,
                channels: 2,
                frame_bytes: 8,
                ..native::Description::default()
            }),
            _ => None,
        }
    }
    fn streams(&self, _: u32, _: u32, _: u32) -> Option<native::StreamIds> {
        native::valid_stream_extent(self.stream_size, 4).then_some(native::StreamIds {
            values: [234; 2048],
            count: 1,
        })
    }
}

#[test]
fn property_admission_observes_rates_and_rejects_malformed_stream_readback() {
    let io = owner(true, 2);
    let binding = NativeBinding {
        device: 123,
        offset: 0,
        total: 0,
    };
    for rate in [44100_u32, 96000] {
        assert_eq!(
            native::format(
                &io.selection,
                binding,
                &PropertiesStub {
                    rate: f64::from(rate),
                    ..PropertiesStub::default()
                }
            ),
            Some(rate)
        );
    }
    for rate in [f64::NAN, 48000.5, 0.0] {
        assert_eq!(
            native::format(
                &io.selection,
                binding,
                &PropertiesStub {
                    rate,
                    ..PropertiesStub::default()
                }
            ),
            None
        );
    }
    for stream_size in [0, 3, 8] {
        assert_eq!(
            native::format(
                &io.selection,
                binding,
                &PropertiesStub {
                    stream_size,
                    ..PropertiesStub::default()
                }
            ),
            None
        );
    }
}

#[test]
fn default_endpoint_changes_and_unproven_transport_identity_are_rejected() {
    for input in [false, true] {
        let io = DeviceIo::new(c"", input, 2, 48000).expect("owner");
        let properties = PropertiesStub::default();
        // SAFETY: sole test control owner accesses connection state before registration.
        unsafe {
            (*io.control.get()).connection.binding.device = 123;
            assert!(!control::alive(&io, &properties));
        }
        assert_eq!(properties.default_queries.get(), 1);
        assert_eq!(native::clock_device(123, &properties), 123);
        for transport in [Some(0), Some(u32::from_be_bytes(*b"grup")), None] {
            assert_eq!(
                native::clock_device(
                    123,
                    &PropertiesStub {
                        transport,
                        ..PropertiesStub::default()
                    }
                ),
                0
            );
        }
        // SAFETY: no registration means no proven clock regardless of native transport.
        assert_eq!(unsafe { control::clock_device(&io, &properties) }, 0);
        let mut registration = RegistrationStub::default();
        assert_eq!(register(&io, &mut registration), 0);
        // SAFETY: test control queries are serialized; mock registration retains this owner.
        assert_eq!(unsafe { control::clock_device(&io, &properties) }, 123);
        for transport in [Some(0), Some(u32::from_be_bytes(*b"grup")), None] {
            let observed = PropertiesStub {
                transport,
                ..PropertiesStub::default()
            };
            // SAFETY: only the retained control cell and mock properties are observed.
            assert_eq!(unsafe { control::clock_device(&io, &observed) }, 0);
        }
        // SAFETY: mock native drain precedes releasing the test owner.
        assert!(unsafe {
            (*io.control.get()).lifecycle.disconnect(&mut StubBackend {
                io: &io,
                native: &mut registration,
            })
        });
    }
}

#[test]
fn callback_uses_direction_timestamp_and_keeps_discontinuities() {
    for input in [false, true] {
        let io = owner(input, 2);
        let mut samples = [0.0; 128];
        let mut list = buffers(&mut samples, 2, 64, false);
        let mut capture = callback::Timestamp {
            sample: 100.0,
            host: 1000,
            flags: 3,
            ..callback::Timestamp::default()
        };
        let mut render = callback::Timestamp {
            sample: 200.0,
            host: 2000,
            flags: 3,
            ..callback::Timestamp::default()
        };
        for pass in 0..2 {
            // SAFETY: complete synchronous fixture lists/timestamps and one callback owner.
            unsafe {
                mr_device_io_process(
                    123,
                    ptr::null(),
                    ptr::from_ref(&list).cast(),
                    ptr::from_ref(&capture).cast(),
                    ptr::from_mut(&mut list).cast(),
                    ptr::from_ref(&render).cast(),
                    ptr::from_ref(&io).cast_mut().cast(),
                );
            }
            let mut clock = CallbackClockSnapshot::default();
            // SAFETY: atomic clock snapshot and exclusive disjoint local output.
            assert!(unsafe { mr_callback_clock_snapshot(&raw const io.clock, &raw mut clock) });
            assert_eq!(clock.valid, 1);
            assert_eq!(
                clock.sample_time.to_bits(),
                if input { capture.sample } else { render.sample }.to_bits()
            );
            assert_eq!(
                clock.host_time,
                if input { capture.host } else { render.host }
            );
            assert_eq!(clock.discontinuities, pass);
            capture.sample += 128.0;
            render.sample += 128.0;
            capture.host += 1000;
            render.host += 1000;
        }
    }
}

#[test]
fn callback_queue_pair_is_allocation_free_and_invalid_construction_is_rejected() {
    let io = owner(false, 2);
    let mut hardware = [0.0; 128];
    let source = [0.25; 128];
    let mut list = buffers(&mut hardware, 2, 64, false);
    let allocation = allocation_counter::measure(|| {
        for _ in 0..1000 {
            // SAFETY: serialized producer and exact-sized source.
            unsafe {
                assert_eq!(queues::worker_write(&io, source.as_ptr(), 64), 64);
            }
            process(&io, &mut list);
        }
    });
    assert_eq!(allocation.count_total, 0);
    assert_pcm(&hardware, &source);
    for channels in [0, 65] {
        assert!(DeviceIo::new(c"unit", true, channels, 48000).is_none());
    }
    assert!(DeviceIo::new(c"unit", true, 2, 0).is_none());
}

unsafe extern "C" fn render_hook(
    context: *mut c_void,
    _: *const c_void,
    output: *mut c_void,
    frames: u32,
) -> bool {
    // SAFETY: hook test retains the Cell and full two-channel packed output.
    let calls = unsafe { &*context.cast::<Cell<u32>>() };
    calls.set(calls.get() + 1);
    // SAFETY: this fixture supplies a complete 64-entry metadata container.
    let list = unsafe { &*output.cast::<Buffers>() };
    // SAFETY: this callback exclusively owns frames*2 initialized output floats.
    unsafe { std::slice::from_raw_parts_mut(list.buffers[0].data, frames as usize * 2) }.fill(0.5);
    true
}

#[test]
fn direct_hook_advances_clock_without_consuming_queued_output() {
    let io = owner(false, 2);
    let calls = Cell::new(0_u32);
    let mut hardware = [0.0; 128];
    let source = [0.25; 128];
    let mut list = buffers(&mut hardware, 2, 64, false);
    // SAFETY: no native registration exists, hook context outlives all test callbacks.
    unsafe {
        mr_device_io_render_hook(
            ptr::from_ref(&io).cast_mut(),
            ptr::from_ref(&calls).cast_mut().cast(),
            Some(render_hook),
        );
        assert_eq!(queues::worker_write(&io, source.as_ptr(), 64), 64);
    }
    process(&io, &mut list);
    assert_eq!(calls.get(), 1);
    assert!(
        hardware
            .iter()
            .all(|sample| sample.to_bits() == 0.5_f32.to_bits())
    );
    assert_eq!(io.native_ring.stats().queued, 64);
    assert_eq!(io.progress.native_frames.load(Acquire), 64);
    assert_eq!(io.progress.ticks.load(Acquire), 1);
}

#[test]
fn absent_native_uid_fails_before_registration_and_releases_owner() {
    // This is the native read-only boundary probe formerly in MRDeviceIOTests.c,
    // not a substitute for mocked property/registration failure tests above.
    let uid = c"io.mixrelay.test.absent.32cdcbba-f015-4ef5-9d5c-01dc1f1c690e";
    // SAFETY: valid immutable UID, then sole control ownership. An unresolvable UID
    // fails preparation before IOProc creation/start; no real device is opened.
    unsafe {
        let io = mr_device_io_create(uid.as_ptr(), true, 2, 48000);
        assert!(!io.is_null());
        assert!(!mr_device_io_connect(io));
        assert_eq!(mr_device_io_connect_status(io, 0), -2);
        assert_eq!(mr_device_io_ticks(io), 0);
        assert!(mr_device_io_destroy(io));
    }
}

#[test]
fn unavailable_buffer_info_initializes_an_uninitialized_output_slot() {
    let uid = c"io.mixrelay.test.absent.32cdcbba-f015-4ef5-9d5c-01dc1f1c690e";
    let io = DeviceIo::new(uid, false, 2, 48000).expect("test owner");
    let mut result = std::mem::MaybeUninit::<BufferInfo>::uninit();
    // SAFETY: stable unregistered owner and an aligned, exclusive output-only
    // slot. The absent UID lookup is read-only and cannot start any device.
    let found =
        unsafe { mr_device_io_buffer_info(ptr::from_ref(&io).cast_mut(), result.as_mut_ptr()) };
    assert!(!found);
    // SAFETY: a non-null owner/output call always initialized the full output.
    let result = unsafe { result.assume_init() };
    assert_eq!((result.device, result.frames, result.settable), (0, 0, 0));
    assert_eq!(result.minimum.to_bits(), 0_f64.to_bits());
    assert_eq!(result.maximum.to_bits(), 0_f64.to_bits());
}
