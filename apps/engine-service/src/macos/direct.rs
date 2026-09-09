//! Callback-driven hardware rendering. All maps/processor storage are prepared
//! before publication. The supervisor only handles reconfiguration and lifetime;
//! it never schedules PCM or acknowledges pause while a render remains in flight.
use super::AudioContext;
mod ownership;
use super::callback_source::RenderTransport;
use super::callback_transport::{CaptureTransport, NativeCapture};
use super::endpoint_boundaries::EndpointBoundaries;
use crate::macos_devices::direct::{Binding, Descriptor, DirectPlan};
use ownership::PreparedRenderer;
use std::{
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};
use tonenari_audio_engine::{BlockProcessor, ExternalBinding, ExternalIo};
use tonenari_audio_io::{
    process::{ClockDomain, ProcessCycle, TransferOutcome},
    session::ClockAuthority,
};

#[repr(C)]
pub(super) struct AudioBuffer {
    channels: u32,
    bytes: u32,
    data: *mut f32,
}
#[repr(C)]
struct AudioBufferList {
    count: u32,
    first: AudioBuffer,
}

/// Borrowed `CoreAudio` storage, valid only for this synchronous callback. ABL
/// pointers never escape to queues, a worker, or the next callback.
pub(super) struct Buffers<'a> {
    pub(super) input: &'a [AudioBuffer],
    pub(super) output: &'a [AudioBuffer],
}
impl<'a> Buffers<'a> {
    unsafe fn list(pointer: *const c_void, frames: u32) -> Option<&'a [AudioBuffer]> {
        if pointer.is_null() {
            return Some(&[]);
        }
        let list = unsafe { &*pointer.cast::<AudioBufferList>() };
        if list.count > 2048 {
            return None;
        }
        let buffers =
            unsafe { std::slice::from_raw_parts(&raw const list.first, list.count as usize) };
        let mut width = 0;
        for buffer in buffers {
            if buffer.channels == 0
                || buffer.channels > 2048
                || (!buffer.data.is_null()
                    && u64::from(buffer.bytes)
                        != u64::from(frames) * u64::from(buffer.channels) * 4)
            {
                return None;
            }
            width += buffer.channels;
            if width > 2048 {
                return None;
            }
        }
        Some(buffers)
    }
    fn covers(&self, binding: &Binding) -> bool {
        let buffers = if binding.input {
            self.input
        } else {
            self.output
        };
        let end = binding.offset + binding.channels;
        let mut base = 0;
        for buffer in buffers {
            let next = base + buffer.channels as usize;
            if base < end && next > binding.offset && buffer.data.is_null() {
                return false;
            }
            base = next;
        }
        end <= base
    }
}

struct CallbackIo<'a, T> {
    buffers: Buffers<'a>,
    bindings: &'a [Binding],
    offset: usize,
    endpoints: &'a mut EndpointBoundaries<T>,
}
impl<T: CaptureTransport + RenderTransport> ExternalIo for CallbackIo<'_, T> {
    fn read_source(
        &mut self,
        binding: &ExternalBinding,
        destination: &mut [f32],
        cycle: &ProcessCycle,
    ) -> TransferOutcome {
        destination.fill(0.0);
        if let Some(result) = self.endpoints.read(binding, destination) {
            return result;
        }
        let Some(map) = self
            .bindings
            .iter()
            .find(|map| map.input && map.id == binding.node_id().as_str())
        else {
            return TransferOutcome::complete(0);
        };
        transfer(self.buffers.input, map, self.offset, destination);
        TransferOutcome::complete(cycle.frames)
    }
    fn write_sink(
        &mut self,
        binding: &ExternalBinding,
        source: &[f32],
        cycle: &ProcessCycle,
    ) -> TransferOutcome {
        if let Some(result) = self.endpoints.write(binding, source) {
            return result;
        }
        let Some(map) = self
            .bindings
            .iter()
            .find(|map| !map.input && map.id == binding.node_id().as_str())
        else {
            return TransferOutcome::complete(0);
        };
        // Multiple graph sinks may map to the same physical channels: mix their
        // contributions into the callback's initially zeroed output, not overwrite.
        write(self.buffers.output, map, self.offset, source);
        TransferOutcome::complete(cycle.frames)
    }
}

pub(super) fn transfer(
    buffers: &[AudioBuffer],
    map: &Binding,
    offset: usize,
    destination: &mut [f32],
) {
    let mut base = 0;
    for buffer in buffers {
        let width = buffer.channels as usize;
        for channel in 0..width {
            if (map.offset..map.offset + map.channels).contains(&(base + channel)) {
                let selected = base + channel - map.offset;
                for (frame, samples) in destination.chunks_exact_mut(map.channels).enumerate() {
                    // SAFETY: full ABL geometry and all channel maps were checked
                    // before DSP; block offsets remain inside this native callback.
                    samples[selected] =
                        unsafe { *buffer.data.add((offset + frame) * width + channel) };
                }
            }
        }
        base += width;
    }
}
pub(super) fn write(buffers: &[AudioBuffer], map: &Binding, offset: usize, source: &[f32]) {
    let mut base = 0;
    for buffer in buffers {
        let width = buffer.channels as usize;
        for channel in 0..width {
            if (map.offset..map.offset + map.channels).contains(&(base + channel)) {
                let selected = base + channel - map.offset;
                for (frame, samples) in source.chunks_exact(map.channels).enumerate() {
                    // SAFETY: same validated geometry, sole IOProc output writer.
                    unsafe {
                        *buffer.data.add((offset + frame) * width + channel) += samples[selected];
                    }
                }
            }
        }
        base += width;
    }
}

struct Renderer<'a, T = NativeCapture> {
    processor: &'a mut BlockProcessor,
    bindings: &'a [Binding],
    clock_id: u32,
    epoch: u64,
    native_clock: Option<(&'a DirectPlan, u64)>,
    status: &'a tonenari_engine_service::session::AdapterRuntimeStatus,
    frame: u64,
    progress: &'a AtomicU64,
    failed: &'a AtomicBool,
    invalid: &'a AtomicBool,
    endpoints: EndpointBoundaries<T>,
    reconfigure: &'a AtomicBool,
}
unsafe extern "C" fn render(
    context: *mut c_void,
    input: *const c_void,
    output: *mut c_void,
    frames: u32,
) {
    // SAFETY: the descriptor is published to exactly one serialized IOProc;
    // control never accesses this renderer until removal + reader drain.
    let renderer = unsafe { &mut *context.cast::<Renderer<'_>>() };
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        renderer.render(input, output, frames)
    }));
    if !matches!(result, Ok(true)) {
        renderer.failed.store(true, Ordering::Release);
    }
}
impl<T: CaptureTransport + RenderTransport> Renderer<'_, T> {
    fn observe_hardware(&mut self) {
        if let Some((plan, observed)) = &mut self.native_clock {
            let current = plan.discontinuities();
            if current != *observed {
                self.status
                    .record_process(tonenari_audio_io::process::ProcessReport {
                        discontinuities: current.wrapping_sub(*observed),
                        ..Default::default()
                    });
                self.epoch = crate::macos_devices::direct::next_epoch();
                self.frame = 0;
                self.endpoints.discontinuity();
                *observed = current;
            }
        }
    }
    unsafe fn render(&mut self, input: *const c_void, output: *mut c_void, frames: u32) -> bool {
        if self.failed.load(Ordering::Acquire) || frames == 0 || frames > 4096 {
            return false;
        }
        self.observe_hardware();
        let Some(input) = (unsafe { Buffers::list(input, frames) }) else {
            self.invalid.store(true, Ordering::Release);
            return true;
        };
        let Some(output) = (unsafe { Buffers::list(output, frames) }) else {
            self.invalid.store(true, Ordering::Release);
            return true;
        };
        let buffers = Buffers { input, output };
        if !self.bindings.iter().all(|binding| buffers.covers(binding)) {
            self.invalid.store(true, Ordering::Release);
            return true;
        }
        let format = self.processor.plan().format();
        self.endpoints.begin(frames);
        self.reconfigure
            .store(self.endpoints.pending(), Ordering::Release);
        let mut io = CallbackIo {
            buffers,
            bindings: self.bindings,
            offset: 0,
            endpoints: &mut self.endpoints,
        };
        // A larger native quantum is split, never truncated or queued for later.
        while io.offset < frames as usize {
            let count =
                u32::try_from((frames as usize - io.offset).min(format.block_frames as usize))
                    .expect("bounded native quantum");
            let cycle = ProcessCycle {
                clock: ClockDomain {
                    authority: ClockAuthority::Device,
                    id: u64::from(self.clock_id),
                    epoch: self.epoch,
                },
                sample_rate: format.sample_rate,
                frames: count,
                first_frame: self.frame,
                monotonic_ns: None,
                discontinuity: self.frame == 0,
            };
            io.endpoints.prepare_block(count);
            let Ok(report) = self.processor.process_external_cycle(cycle, &mut io) else {
                return false;
            };
            self.status.record_process(report);
            self.status.record_process(io.endpoints.finish_block());
            self.frame += u64::from(count);
            io.offset += count as usize;
        }
        self.status
            .observe_clock(ClockAuthority::Device, io.endpoints.synchronized());
        tonenari_engine_service::endpoint_service::touch_audio();
        self.progress.fetch_add(1, Ordering::Release);
        true
    }
}

struct CaptureRetirement<F: FnMut()>(F);
impl<F: FnMut()> Drop for CaptureRetirement<F> {
    fn drop(&mut self) {
        (self.0)();
    }
}

fn log_route(plan: &DirectPlan, rate: u32, buffer: u32) {
    eprintln!(
        "CoreAudio callback render: clock={} rate={} buffer={} bindings={}",
        plan.clock_id,
        rate,
        buffer,
        plan.bindings.len()
    );
}

pub(super) fn run(
    plan: &DirectPlan,
    processor: &mut BlockProcessor,
    context: AudioContext<'_>,
) -> Result<(), String> {
    let progress = AtomicU64::new(0);
    let failed = AtomicBool::new(false);
    let invalid = AtomicBool::new(false);
    let reconfigure = AtomicBool::new(false);
    let graph_rate = processor.plan().format().sample_rate;
    let endpoints = EndpointBoundaries::prepare(processor.plan())?;
    let capture_ids = endpoints.capture_ids();
    reconfigure.store(endpoints.pending(), Ordering::Release);
    let renderer = Renderer {
        processor,
        bindings: &plan.bindings,
        clock_id: plan.clock_id,
        epoch: super::super::macos_devices::direct::next_epoch(),
        native_clock: Some((plan, plan.discontinuities())),
        status: context.runtime_status,
        frame: 0,
        progress: &progress,
        failed: &failed,
        invalid: &invalid,
        endpoints,
        reconfigure: &reconfigure,
    };
    log_route(
        plan,
        graph_rate,
        renderer.processor.plan().format().block_frames,
    );
    let mut prepared = PreparedRenderer::new(renderer);
    // Declare before registration: all exits, including preparation errors,
    // first drain the sole writer and only then invalidate its old capture tail.
    let capture_retirement = CaptureRetirement(|| {
        for &id in &capture_ids {
            super::callback_transport::recover_capture_epoch(id);
        }
    });
    let mut installed = prepared.install(plan);
    let mut ticks = 0;
    let mut last = Instant::now();
    let mut last_boundary_check = Instant::now();
    while !context.runtime_status.stop_requested()
        && !context.runtime_status.pause_requested()
        && !context.exchange.has_pending()
        && !context.devices.has_pending()
        && plan.healthy()
    {
        if failed.load(Ordering::Acquire) || invalid.load(Ordering::Acquire) {
            break;
        }
        if reconfigure.load(Ordering::Acquire)
            && last_boundary_check.elapsed() >= Duration::from_millis(100)
        {
            last_boundary_check = Instant::now();
            installed.replace_boundaries()?;
        }
        let next = progress.load(Ordering::Acquire);
        if next != ticks {
            ticks = next;
            last = Instant::now();
        } else if last.elapsed() > Duration::from_secs(2) {
            break;
        }
        // Supervisory responsiveness only. This thread never moves PCM; every
        // block has already been rendered synchronously inside the IOProc.
        thread::sleep(Duration::from_millis(1));
    }
    drop(installed);
    drop(capture_retirement);
    if invalid.load(Ordering::Acquire) {
        plan.request_reconnect();
    }
    if failed.load(Ordering::Acquire) {
        return Err("CoreAudio direct callback rejected buffer geometry or DSP processing".into());
    }
    Ok(())
}

fn replace_boundaries(
    renderer: &mut Renderer<'_>,
    replacement: EndpointBoundaries<NativeCapture>,
) -> EndpointBoundaries<NativeCapture> {
    renderer.endpoints.recover_pending();
    super::endpoint_backend::MacVirtualEndpointBackend::activate_recovery();
    let retired = renderer.endpoints.replace_pending(replacement);
    renderer
        .reconfigure
        .store(renderer.endpoints.pending(), Ordering::Release);
    retired
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tonenari_audio_engine::RenderPlan;
    use tonenari_audio_graph::{
        CompileContext, CompileLimits, GraphCompiler, GraphDocument, RenderFormat,
    };
    use tonenari_engine_service::RuntimeStatusBackend;

    pub(super) fn stereo_bindings() -> [Binding; 2] {
        [
            Binding {
                id: "input".into(),
                input: true,
                offset: 0,
                channels: 2,
            },
            Binding {
                id: "output".into(),
                input: false,
                offset: 0,
                channels: 2,
            },
        ]
    }

    pub(super) fn stereo_list(pcm: &mut [f32]) -> AudioBufferList {
        AudioBufferList {
            count: 1,
            first: AudioBuffer {
                channels: 2,
                bytes: u32::try_from(std::mem::size_of_val(pcm)).unwrap(),
                data: pcm.as_mut_ptr(),
            },
        }
    }

    pub(super) fn processor_at(block_frames: u32, sample_rate: u32) -> BlockProcessor {
        let document: GraphDocument = serde_json::from_value(serde_json::json!({
            "schemaVersion":2,"revision":1,"nodes":[
                {"id":"input","type":"audio_device_input","config":{"selector":{"match":"default_input"},"outputChannels":2,"gainDb":0}},
                {"id":"output","type":"audio_device_output","config":{"selector":{"match":"default_output"},"inputChannels":2}}
            ],"edges":[{"id":"wire","source":{"nodeId":"input","portId":"audio"},"destination":{"nodeId":"output","portId":"audio"}}]
        })).unwrap();
        let execution = GraphCompiler::default()
            .compile(
                &document,
                CompileContext {
                    format: RenderFormat {
                        sample_rate,
                        block_frames,
                    },
                    limits: CompileLimits::default(),
                },
            )
            .unwrap();
        BlockProcessor::new(Arc::new(RenderPlan::prepare(&execution).unwrap()))
    }

    #[test]
    fn synchronous_callback_preserves_every_sample_at_all_product_buffers() {
        for &block_frames in tonenari_audio_io::formats::BUFFER_SIZES {
            let mut processor = processor_at(block_frames, 48000);
            let status = tonenari_engine_service::session::AdapterRuntimeStatus::new(
                crate::macos::adapter_profile(),
            );
            let progress = AtomicU64::new(0);
            let failed = AtomicBool::new(false);
            let invalid = AtomicBool::new(false);
            let bindings = stereo_bindings();
            let mut renderer = Renderer {
                processor: &mut processor,
                bindings: &bindings,
                clock_id: 7,
                epoch: 3,
                native_clock: None,
                status: &status,
                frame: 0,
                progress: &progress,
                failed: &failed,
                invalid: &invalid,
                endpoints: EndpointBoundaries::<NativeCapture>::empty(),
                reconfigure: &invalid,
            };
            // Native callback larger than the DSP quantum proves split offsets,
            // including non-divisible final blocks. No queued next-call output.
            let frames = (block_frames + 17).min(4096);
            let mut input = vec![0.0; frames as usize * 2];
            let mut output = vec![0.0; input.len()];
            let input_list = stereo_list(&mut input);
            let mut output_list = stereo_list(&mut output);
            for cycle in 0..100 {
                for (index, sample) in input.iter_mut().enumerate() {
                    *sample = f32::from(
                        u16::try_from((cycle * input_list.first.bytes as usize + index) % 8192)
                            .unwrap(),
                    ) / 8192.0;
                }
                output.fill(0.0);
                assert!(unsafe {
                    renderer.render(
                        (&raw const input_list).cast(),
                        (&raw mut output_list).cast(),
                        frames,
                    )
                });
                assert_eq!(input, output, "buffer={block_frames} cycle={cycle}");
            }
            assert_eq!(progress.load(Ordering::Acquire), 100);
            assert_eq!(renderer.frame, u64::from(frames) * 100);
            let measured = status.status(RenderFormat {
                sample_rate: 48000,
                block_frames,
            });
            assert_eq!(measured.underruns, 0);
            assert_eq!(measured.overruns, 0);
        }
    }

    #[test]
    fn disabled_unused_stream_keeps_offsets_but_selected_missing_pcm_is_rejected() {
        #[repr(C)]
        struct List {
            count: u32,
            buffers: [AudioBuffer; 2],
        }
        let mut input = [0.25_f32; 64];
        let list = List {
            count: 2,
            buffers: [
                AudioBuffer {
                    channels: 3,
                    bytes: 0,
                    data: std::ptr::null_mut(),
                },
                AudioBuffer {
                    channels: 2,
                    bytes: 256,
                    data: input.as_mut_ptr(),
                },
            ],
        };
        let buffers = unsafe { Buffers::list((&raw const list).cast(), 32) }.unwrap();
        let map = Binding {
            id: "input".into(),
            input: true,
            offset: 3,
            channels: 2,
        };
        let view = Buffers {
            input: buffers,
            output: &[],
        };
        assert!(view.covers(&map));
        assert!(!view.covers(&Binding { offset: 2, ..map }));
        let mut destination = [0.0; 64];
        transfer(
            buffers,
            &Binding {
                id: "input".into(),
                input: true,
                offset: 3,
                channels: 2,
            },
            0,
            &mut destination,
        );
        assert_eq!(destination.map(f32::to_bits), input.map(f32::to_bits));
        assert!(unsafe { Buffers::list((&raw const list).cast(), 64) }.is_none());
    }
}

#[cfg(test)]
#[path = "direct_mixed_tests.rs"]
mod mixed_tests;
