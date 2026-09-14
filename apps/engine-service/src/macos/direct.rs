//! Callback-driven hardware rendering. All maps/processor storage are prepared
//! before publication. The supervisor only handles reconfiguration and lifetime;
//! it never schedules PCM or acknowledges pause while a render remains in flight.
use super::AudioContext;
#[cfg(test)]
mod continuity_tests;
mod ownership;
#[cfg(test)]
mod virtual_activity_tests;
use super::callback_source::RenderTransport;
use super::callback_transport::{CaptureTransport, NativeCapture};
use super::endpoint_boundaries::EndpointBoundaries;
use crate::macos_devices::direct::{Binding, Descriptor, DirectPlan};
use mixrelay_audio_engine::{BlockProcessor, ExternalBinding, ExternalIo, RenderPlan};
use mixrelay_audio_io::{
    process::{ClockDomain, InputActivity, ProcessCycle, TransferOutcome},
    session::ClockAuthority,
};
use ownership::PreparedRenderer;
use std::{
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

mod buffers;
use buffers::Buffers;

#[cfg(test)]
use mixrelay_macos_transport::audio_buffer::AudioBuffer;

#[cfg(test)]
#[repr(C)]
struct AudioBufferList {
    count: u32,
    first: AudioBuffer,
}

struct CallbackIo<'a, T> {
    buffers: Buffers<'a>,
    bindings: &'a [Binding],
    offset: usize,
    endpoints: &'a mut EndpointBoundaries<T>,
    bridged: Option<&'a crate::macos_devices::DeviceSet>,
}
impl<T: CaptureTransport + RenderTransport> ExternalIo for CallbackIo<'_, T> {
    fn begin_cycle(
        &mut self,
        cycle: &ProcessCycle,
        controls: mixrelay_audio_engine::ExternalControlState<'_>,
    ) -> mixrelay_audio_engine::CycleAdmission {
        if let Some(devices) = self.bridged {
            for (binding, enabled) in controls.sources().chain(controls.sinks()) {
                devices.set_active(binding.node_id().as_str(), enabled);
            }
        }
        self.endpoints.prepare_block(cycle.frames, controls);
        mixrelay_audio_engine::CycleAdmission::Process
    }
    fn read_source(
        &mut self,
        binding: &ExternalBinding,
        destination: &mut [f32],
        cycle: &ProcessCycle,
    ) -> TransferOutcome {
        destination.fill(0.0);
        if let Some(result) = self
            .bridged
            .and_then(|devices| devices.read(binding.node_id().as_str(), destination))
        {
            return result;
        }
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
        if map.offset.is_none() {
            return TransferOutcome {
                activity: InputActivity::Inactive,
                ..TransferOutcome::complete(cycle.frames)
            };
        }
        self.buffers.transfer(map, self.offset, destination);
        TransferOutcome::complete(cycle.frames)
    }
    fn write_sink(
        &mut self,
        binding: &ExternalBinding,
        source: &[f32],
        cycle: &ProcessCycle,
    ) -> TransferOutcome {
        if let Some(result) = self
            .bridged
            .and_then(|devices| devices.write(binding.node_id().as_str(), source))
        {
            return result;
        }
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
        self.buffers.write(map, self.offset, source);
        TransferOutcome::complete(cycle.frames)
    }
}

struct Renderer<'a, T = NativeCapture> {
    processor: &'a mut Box<BlockProcessor>,
    activation: Option<GraphActivation<'a>>,
    bindings: &'a [Binding],
    clock_id: u32,
    epoch: u64,
    native_clock: Option<(&'a DirectPlan, u64)>,
    status: &'a mixrelay_engine_service::session::AdapterRuntimeStatus,
    frame: u64,
    progress: &'a AtomicU64,
    failed: &'a AtomicBool,
    invalid: &'a AtomicBool,
    endpoints: EndpointBoundaries<T>,
    reconfigure: &'a AtomicBool,
}

/// The installed native geometry is immutable throughout a direct lifetime.
/// A compatible DSP replacement can use it without withdrawing the callback or
/// rebuilding independent-clock converters. Other plans remain queued for rebind.
struct GraphActivation<'a> {
    installed: &'a RenderPlan,
    context: AudioContext<'a>,
    rebind: &'a AtomicBool,
}

impl GraphActivation<'_> {
    fn activate(&self, processor: &mut Box<BlockProcessor>) -> bool {
        if !self.context.exchange.has_pending() {
            return true;
        }
        self.context
            .exchange
            .activate_pending_if_configured(
                processor,
                |next| {
                    let compatible = !self.context.devices.has_pending()
                        && same_external_geometry(self.installed, next);
                    if !compatible {
                        self.rebind.store(true, Ordering::Release);
                    }
                    compatible
                },
                |_| self.context.endpoint_backend.activate_pending().map(|_| ()),
            )
            .is_ok()
    }
}

fn same_external_geometry(installed: &RenderPlan, next: &RenderPlan) -> bool {
    installed.format() == next.format()
        && installed.source_bindings() == next.source_bindings()
        && installed.sink_bindings() == next.sink_bindings()
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
                    .record_process(mixrelay_audio_io::process::ProcessReport {
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
        if self
            .activation
            .as_ref()
            .is_some_and(|activation| !activation.activate(self.processor))
        {
            return false;
        }
        self.observe_hardware();
        // SAFETY: the serialized native callback retains both ABLs and their PCM;
        // DSP scratch belongs to the independent BlockProcessor allocation. The
        // closure cannot return a view or retain access past this synchronous call.
        let result = unsafe {
            Buffers::with_native(input, output, frames, |buffers| {
                self.render_buffers(buffers, frames)
            })
        };
        result.unwrap_or_else(|| {
            self.invalid.store(true, Ordering::Release);
            true
        })
    }
    fn render_buffers(&mut self, buffers: Buffers<'_>, frames: u32) -> bool {
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
            bridged: self.native_clock.as_ref().map(|(plan, _)| &plan.bridged),
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
            let Ok(Some(report)) = self.processor.process_external_cycle(cycle, &mut io) else {
                return false;
            };
            self.status.record_process(report);
            self.status.record_process(io.endpoints.finish_block());
            self.frame += u64::from(count);
            io.offset += count as usize;
        }
        self.status
            .observe_clock(ClockAuthority::Device, io.endpoints.synchronized());
        mixrelay_engine_service::endpoint_service::touch_audio();
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
    tracing::info!(
        clock_id = plan.clock_id,
        processing_rate = rate,
        buffer_frames = buffer,
        bindings = plan.bindings.len(),
        "CoreAudio direct renderer starting"
    );
}

#[derive(Default)]
struct RenderSignals {
    progress: AtomicU64,
    failed: AtomicBool,
    invalid: AtomicBool,
    reconfigure: AtomicBool,
    rebind: AtomicBool,
}

pub(super) fn run(
    plan: &DirectPlan,
    processor: &mut Box<BlockProcessor>,
    context: AudioContext<'_>,
) -> Result<(), String> {
    let signals = RenderSignals::default();
    let installed_plan = std::sync::Arc::clone(processor.plan());
    let graph_rate = processor.plan().format().sample_rate;
    let endpoints = EndpointBoundaries::prepare(processor.plan())?;
    let capture_ids = endpoints.capture_ids();
    signals
        .reconfigure
        .store(endpoints.pending(), Ordering::Release);
    let renderer = Renderer {
        processor,
        activation: Some(GraphActivation {
            installed: &installed_plan,
            context,
            rebind: &signals.rebind,
        }),
        bindings: &plan.bindings,
        clock_id: plan.clock_id,
        epoch: super::super::macos_devices::direct::next_epoch(),
        native_clock: Some((plan, plan.discontinuities())),
        status: context.runtime_status,
        frame: 0,
        progress: &signals.progress,
        failed: &signals.failed,
        invalid: &signals.invalid,
        endpoints,
        reconfigure: &signals.reconfigure,
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
    let mut last_execution_report = Instant::now();
    let result = signals.supervise(plan, context, &mut installed, &mut last_execution_report);
    drop(installed);
    plan.report_execution(last_execution_report.elapsed());
    drop(capture_retirement);
    finish_render(
        plan,
        context,
        signals.invalid.load(Ordering::Acquire),
        signals.failed.load(Ordering::Acquire),
    )?;
    result
}

impl RenderSignals {
    fn supervise(
        &self,
        plan: &DirectPlan,
        context: AudioContext<'_>,
        installed: &mut ownership::InstalledRenderer<'_, '_>,
        last_execution_report: &mut Instant,
    ) -> Result<(), String> {
        let mut ticks = 0;
        let mut last = Instant::now();
        let mut last_boundary_check = Instant::now();
        while !context.runtime_status.stop_requested()
            && !context.runtime_status.pause_requested()
            && !self.rebind.load(Ordering::Acquire)
            && !context.devices.has_pending()
            && plan.healthy()
        {
            if self.failed.load(Ordering::Acquire) || self.invalid.load(Ordering::Acquire) {
                break;
            }
            if last_execution_report.elapsed() >= Duration::from_secs(5) {
                plan.report_execution(last_execution_report.elapsed());
                *last_execution_report = Instant::now();
            }
            if self.reconfigure.load(Ordering::Acquire)
                && last_boundary_check.elapsed() >= Duration::from_millis(100)
            {
                last_boundary_check = Instant::now();
                installed.replace_boundaries().inspect_err(|error| {
                tracing::warn!(clock_id = plan.clock_id, %error, "CoreAudio boundary preparation failed");
            })?;
            }
            let next = self.progress.load(Ordering::Acquire);
            if next != ticks {
                ticks = next;
                last = Instant::now();
            } else if last.elapsed() > Duration::from_secs(2) {
                tracing::warn!(
                    clock_id = plan.clock_id,
                    "CoreAudio direct callback progress stalled"
                );
                break;
            }
            // Supervisory responsiveness only. This thread never moves PCM; every
            // block has already been rendered synchronously inside the IOProc.
            thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    }
}

fn finish_render(
    plan: &DirectPlan,
    context: AudioContext<'_>,
    invalid: bool,
    failed: bool,
) -> Result<(), String> {
    tracing::info!(
        clock_id = plan.clock_id,
        invalid_layout = invalid,
        render_failed = failed,
        stop = context.runtime_status.stop_requested(),
        pause = context.runtime_status.pause_requested(),
        graph_pending = context.exchange.has_pending(),
        devices_pending = context.devices.has_pending(),
        healthy = plan.healthy(),
        "CoreAudio direct renderer stopped"
    );
    if invalid {
        tracing::warn!(
            clock_id = plan.clock_id,
            "CoreAudio callback rejected native buffer layout"
        );
        plan.request_reconnect();
    }
    if !plan.healthy() {
        context.devices.request_recovery();
    }
    if failed {
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
    use mixrelay_audio_graph::{
        CompileContext, CompileLimits, GraphCompiler, GraphDocument, RenderFormat,
    };
    use mixrelay_engine_service::RuntimeStatusBackend;
    use std::sync::Arc;

    pub(super) fn stereo_bindings() -> [Binding; 2] {
        [
            Binding {
                id: "input".into(),
                input: true,
                offset: Some(0),
                channels: 2,
            },
            Binding {
                id: "output".into(),
                input: false,
                offset: Some(0),
                channels: 2,
            },
        ]
    }

    pub(super) fn stereo_list(pcm: &mut [f32]) -> AudioBufferList {
        AudioBufferList {
            count: 1,
            first: AudioBuffer {
                channels: 2,
                byte_size: u32::try_from(std::mem::size_of_val(pcm)).unwrap(),
                data: pcm.as_mut_ptr(),
            },
        }
    }

    pub(super) fn processor_at(block_frames: u32, sample_rate: u32) -> Box<BlockProcessor> {
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
        Box::new(BlockProcessor::new(Arc::new(
            RenderPlan::prepare(&execution).unwrap(),
        )))
    }

    fn offline_branch_processor() -> Box<BlockProcessor> {
        let document: GraphDocument = serde_json::from_value(serde_json::json!({
            "schemaVersion":2,"revision":1,"nodes":[
                {"id":"input","type":"audio_device_input","config":{"selector":{"match":"default_input"},"outputChannels":2,"gainDb":0}},
                {"id":"missing-input","type":"audio_device_input","config":{"selector":{"match":"default_input"},"outputChannels":2,"gainDb":0}},
                {"id":"output","type":"audio_device_output","config":{"selector":{"match":"default_output"},"inputChannels":2}},
                {"id":"missing-output","type":"audio_device_output","config":{"selector":{"match":"default_output"},"inputChannels":2}}
            ],"edges":[
                {"id":"live","source":{"nodeId":"input","portId":"audio"},"destination":{"nodeId":"output","portId":"audio"}},
                {"id":"silent","source":{"nodeId":"missing-input","portId":"audio"},"destination":{"nodeId":"output","portId":"audio"}},
                {"id":"discard","source":{"nodeId":"input","portId":"audio"},"destination":{"nodeId":"missing-output","portId":"audio"}}
            ]
        })).unwrap();
        let execution = GraphCompiler::default()
            .compile(
                &document,
                CompileContext {
                    format: RenderFormat {
                        sample_rate: 48_000,
                        block_frames: 32,
                    },
                    limits: CompileLimits::default(),
                },
            )
            .unwrap();
        Box::new(BlockProcessor::new(Arc::new(
            RenderPlan::prepare(&execution).unwrap(),
        )))
    }

    #[test]
    fn offline_transfers_are_inactive_silence_and_discard_with_complete_frame_counts() {
        let processor = processor_at(32, 48_000);
        let mut bindings = stereo_bindings();
        for binding in &mut bindings {
            binding.offset = None;
        }
        let mut endpoints = EndpointBoundaries::<NativeCapture>::empty();
        let mut io = CallbackIo {
            buffers: Buffers::empty(),
            bindings: &bindings,
            offset: 0,
            endpoints: &mut endpoints,
            bridged: None,
        };
        assert!(bindings.iter().all(|binding| io.buffers.covers(binding)));
        let cycle = ProcessCycle {
            clock: ClockDomain {
                authority: ClockAuthority::Device,
                id: 7,
                epoch: 3,
            },
            sample_rate: 48_000,
            frames: 32,
            first_frame: 0,
            monotonic_ns: None,
            discontinuity: true,
        };
        let mut pcm = [9.0; 64];
        let input = io.read_source(&processor.plan().source_bindings()[0], &mut pcm, &cycle);
        assert_eq!(
            input,
            TransferOutcome {
                activity: InputActivity::Inactive,
                ..TransferOutcome::complete(32)
            }
        );
        assert_eq!(pcm.map(f32::to_bits), [0.0_f32; 64].map(f32::to_bits));
        assert_eq!(
            io.write_sink(&processor.plan().sink_bindings()[0], &[0.5; 64], &cycle),
            TransferOutcome::complete(32)
        );
    }

    #[test]
    fn missing_device_branches_do_not_interrupt_live_callback_pcm_or_create_xruns() {
        let mut processor = offline_branch_processor();
        let mut bindings = Vec::from(stereo_bindings());
        for (id, input) in [("missing-input", true), ("missing-output", false)] {
            bindings.push(Binding {
                id: id.into(),
                input,
                offset: None,
                channels: 2,
            });
        }
        let status = mixrelay_engine_service::session::AdapterRuntimeStatus::new(
            crate::macos::adapter_profile(),
        );
        let progress = AtomicU64::new(0);
        let failed = AtomicBool::new(false);
        let invalid = AtomicBool::new(false);
        let mut renderer = Renderer {
            processor: &mut processor,
            activation: None,
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
        exercise_live_callback(&mut renderer);
        assert_eq!(progress.load(Ordering::Acquire), 10);
        assert!(!invalid.load(Ordering::Acquire));
        assert!(!failed.load(Ordering::Acquire));
        let measured = status.status(RenderFormat {
            sample_rate: 48_000,
            block_frames: 32,
        });
        assert_eq!((measured.underruns, measured.overruns), (0, 0));
    }

    fn exercise_live_callback(renderer: &mut Renderer<'_>) {
        let mut input = [0.25; 98];
        let mut output = [0.0; 98];
        let input_list = stereo_list(&mut input);
        let mut output_list = stereo_list(&mut output);
        for _ in 0..10 {
            output.fill(0.0);
            // SAFETY: both ABLs own 49 stereo frames for this synchronous call;
            // the two offline maps have no native storage to dereference.
            assert!(unsafe {
                renderer.render(
                    (&raw const input_list).cast(),
                    (&raw mut output_list).cast(),
                    49,
                )
            });
            assert_eq!(output.map(f32::to_bits), input.map(f32::to_bits));
        }
    }

    #[test]
    fn synchronous_callback_preserves_every_sample_at_all_product_buffers() {
        for &block_frames in mixrelay_audio_io::formats::BUFFER_SIZES {
            let mut processor = processor_at(block_frames, 48000);
            let status = mixrelay_engine_service::session::AdapterRuntimeStatus::new(
                crate::macos::adapter_profile(),
            );
            let progress = AtomicU64::new(0);
            let failed = AtomicBool::new(false);
            let invalid = AtomicBool::new(false);
            let bindings = stereo_bindings();
            let mut renderer = Renderer {
                processor: &mut processor,
                activation: None,
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
                        u16::try_from((cycle * input_list.first.byte_size as usize + index) % 8192)
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
                    byte_size: 0,
                    data: std::ptr::null_mut(),
                },
                AudioBuffer {
                    channels: 2,
                    byte_size: 256,
                    data: input.as_mut_ptr(),
                },
            ],
        };
        let map = Binding {
            id: "input".into(),
            input: true,
            offset: Some(3),
            channels: 2,
        };
        let mut destination = [0.0; 64];
        // SAFETY: sized list and live PCM are retained across this operation;
        // inactive metadata has no PCM and destination is separate scratch.
        unsafe {
            Buffers::with_native((&raw const list).cast(), std::ptr::null_mut(), 32, |view| {
                assert!(view.covers(&map));
                assert!(!view.covers(&Binding {
                    id: "missing".into(),
                    input: true,
                    offset: Some(2),
                    channels: 2,
                }));
                view.transfer(&map, 0, &mut destination);
            })
        }
        .unwrap();
        assert_eq!(destination.map(f32::to_bits), input.map(f32::to_bits));
        // SAFETY: metadata is live; the advertised PCM byte count is deliberately
        // inconsistent with 64 frames and rejected before a PCM reference exists.
        assert!(
            unsafe {
                Buffers::with_native((&raw const list).cast(), std::ptr::null_mut(), 64, |_| ())
            }
            .is_none()
        );
    }
}

#[cfg(test)]
#[path = "direct_mixed_tests.rs"]
mod mixed_tests;

#[cfg(test)]
#[path = "direct/measurement.rs"]
mod measurement;
