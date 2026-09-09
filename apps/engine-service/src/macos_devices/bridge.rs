//! Worker-owned clock conversion and bounded queue policy; no device lifetime management.
use super::{
    ffi::{
        tn_device_io_clock_discontinuities, tn_device_io_graph_frames, tn_device_io_output_epoch,
        tn_device_io_overflows, tn_device_io_quantum, tn_device_io_queued,
        tn_device_io_worker_discard, tn_device_io_worker_read, tn_device_io_worker_write,
    },
    worker::WorkerContext,
};
use std::sync::atomic::{AtomicU64, Ordering};
use tonenari_audio_io::clock_bridge::{
    AsyncClockBridge, BridgeConfig, ClockFeedback, ClockRelation, Discontinuity, FeedbackQueue,
    TransferState,
};
pub(super) const PUMP_FRAMES: usize = 256;
pub(super) const QUEUE_CAPACITY: usize = 16_384;
pub(super) static NEXT_DEVICE_EPOCH: AtomicU64 = AtomicU64::new(1);

/// Only the native resource worker mutates this bridge. Graph revisions share
/// immutable queue handles, never an aliased mutable converter or scratch slab.
pub(super) struct DeviceBridge {
    pub(super) bridge: AsyncClockBridge,
    pub(super) source: Vec<f32>,
    pub(super) destination: Vec<f32>,
    pub(super) graph_rate: u32,
    pub(super) native_rate: u32,
    pub(super) clock_aligned: bool,
    pub(super) epoch: u64,
    pub(super) graph_frames: u64,
    pub(super) streaming: bool,
    pub(super) transport_overflows: u64,
    pub(super) clock_discontinuities: u64,
}
impl DeviceBridge {
    pub(super) fn new(context: &WorkerContext, native_rate: u32) -> Result<Self, String> {
        let graph_rate = context.graph_rate.load(Ordering::Acquire);
        let mut result = Self::prepare(
            context.channels,
            context.input,
            native_rate,
            graph_rate,
            context.clock_aligned.load(Ordering::Acquire),
        )?;
        result.activate(context);
        Ok(result)
    }
    pub(super) fn prepare(
        channels: usize,
        input: bool,
        native_rate: u32,
        graph_rate: u32,
        clock_aligned: bool,
    ) -> Result<Self, String> {
        let (input_rate, output_rate) = if input {
            (native_rate, graph_rate)
        } else {
            (graph_rate, native_rate)
        };
        let mut bridge = AsyncClockBridge::new(BridgeConfig {
            channels,
            input_rate,
            output_rate,
            max_input_frames: PUMP_FRAMES,
            max_output_frames: PUMP_FRAMES,
            capacity_frames: QUEUE_CAPACITY,
            priming_frames: 32,
            // Alignment is supplied by the active device set: the selected
            // pacing owner, or a verified second IOProc of that same device.
            // Matching nominal Hz alone is never enough.
            clock_relation: if clock_aligned {
                ClockRelation::ProvenSameClock
            } else {
                ClockRelation::Independent
            },
            feedback_queue: if input {
                FeedbackQueue::Output
            } else {
                FeedbackQueue::Input
            },
            max_correction_ppm: 1_000.0,
        })
        .map_err(|error| error.to_string())?;
        let epoch = NEXT_DEVICE_EPOCH.fetch_add(1, Ordering::Relaxed);
        bridge.reset(epoch, Discontinuity::StreamRestart);
        Ok(Self {
            bridge,
            source: vec![0.0; PUMP_FRAMES * channels],
            destination: vec![0.0; PUMP_FRAMES * channels],
            graph_rate,
            native_rate,
            clock_aligned,
            epoch,
            graph_frames: 0,
            streaming: false,
            transport_overflows: 0,
            clock_discontinuities: 0,
        })
    }
    pub(super) fn activate(&mut self, context: &WorkerContext) {
        #[cfg(test)]
        eprintln!(
            "hardware bridge activation input={} native_rate={} graph_rate={} aligned={} epoch={}",
            context.input, self.native_rate, self.graph_rate, self.clock_aligned, self.epoch
        );
        context.bridge_ready.store(false, Ordering::Release);
        let pointer = context.native.pointer.as_ptr();
        unsafe {
            tn_device_io_worker_discard(pointer);
            tn_device_io_output_epoch(pointer, self.epoch);
            self.graph_frames = tn_device_io_graph_frames(pointer);
            self.transport_overflows = tn_device_io_overflows(pointer);
            self.clock_discontinuities = tn_device_io_clock_discontinuities(pointer);
        }
    }

    pub(super) fn observe(
        &mut self,
        context: &WorkerContext,
        native_running: bool,
    ) -> Result<(), String> {
        let pointer = context.native.pointer.as_ptr();
        let frames = unsafe { tn_device_io_graph_frames(pointer) };
        let elapsed = frames.wrapping_sub(self.graph_frames);
        self.graph_frames = frames;
        if elapsed == 0 {
            return Ok(());
        }
        let target = self.queue_target(context, true) / 2;
        self.bridge
            .observe_clock(ClockFeedback {
                queue: if context.input {
                    FeedbackQueue::Output
                } else {
                    FeedbackQueue::Input
                },
                epoch: self.epoch,
                queued_frames: unsafe { tn_device_io_queued(pointer, true) } as usize
                    + if context.input {
                        0
                    } else {
                        self.bridge.buffered_input_frames()
                    },
                target_frames: target,
                elapsed_frames: usize::try_from(elapsed)
                    .unwrap_or(usize::MAX)
                    .min(self.graph_rate as usize),
                clock_running: native_running && self.streaming && !self.clock_aligned,
            })
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    /// Bound buffering by the actual producer/consumer burst, converted to the
    /// destination rate. A 32-sample graph still needs to absorb one complete
    /// hardware callback; it must not silently inherit a fixed 2048-sample FIFO.
    pub(super) fn queue_target(&self, context: &WorkerContext, graph: bool) -> usize {
        let pointer = context.native.pointer.as_ptr();
        let native_quantum = unsafe { tn_device_io_quantum(pointer, false) }.max(32) as usize;
        let graph_quantum = unsafe { tn_device_io_quantum(pointer, true) }.max(32) as usize;
        let burst = if graph {
            graph_quantum.max(
                (native_quantum * self.graph_rate as usize).div_ceil(self.native_rate as usize),
            )
        } else {
            native_quantum
                .max((graph_quantum * self.native_rate as usize).div_ceil(self.graph_rate as usize))
        };
        // Independent clocks retain one extra callback of scheduling headroom.
        (burst * if self.clock_aligned { 1 } else { 2 }).clamp(32, QUEUE_CAPACITY / 2)
    }

    pub(super) fn pump(&mut self, context: &WorkerContext) -> Result<(), String> {
        let pointer = context.native.pointer.as_ptr();
        let destination_graph = context.input;
        let target = self.queue_target(context, destination_graph);
        // Bound each scheduling turn even at extreme rate ratios. Backpressure
        // stays in the real clock-crossing FIFO, not a greedily drained private
        // staging queue. Priming zeros are never enqueued as valid PCM.
        for _ in 0..64 {
            let queued = unsafe { tn_device_io_queued(pointer, destination_graph) } as usize;
            if queued >= target {
                break;
            }
            let source_frames =
                unsafe { tn_device_io_queued(pointer, !destination_graph) } as usize;
            if self.bridge.queued_input_frames() < PUMP_FRAMES * 2 && source_frames != 0 {
                let count = source_frames.min(PUMP_FRAMES);
                let count = unsafe {
                    tn_device_io_worker_read(
                        pointer,
                        self.source.as_mut_ptr(),
                        u32::try_from(count).unwrap_or(0),
                    )
                } as usize;
                if count != 0 {
                    self.bridge
                        .push_interleaved(&self.source[..count * context.channels])
                        .map_err(|error| error.to_string())?;
                }
            }
            let report = self
                .bridge
                .pull_interleaved(
                    &mut self.destination[..(target - queued).min(PUMP_FRAMES) * context.channels],
                )
                .map_err(|error| error.to_string())?;
            self.streaming = report.state != TransferState::Priming;
            if report.transferred_frames == 0 {
                break;
            }
            let count =
                u32::try_from(report.transferred_frames).map_err(|error| error.to_string())?;
            if unsafe { tn_device_io_worker_write(pointer, self.destination.as_ptr(), count) }
                != count
            {
                return Err("CoreAudio SRC destination FIFO overflow".into());
            }
            context.bridge_ready.store(true, Ordering::Release);
        }
        Ok(())
    }
}
