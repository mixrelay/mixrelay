//! Low-rate, control-thread snapshots. No logging or property IPC on callbacks.
use super::{MacDevices, ffi};
use mixrelay_engine_service::endpoint_service;
use std::sync::atomic::Ordering;

impl MacDevices {
    /// Reads atomic FIFO/progress counters while retaining each native owner.
    /// Called only by the format monitor; does not lock or pause a PCM producer.
    pub(crate) fn trace_audio(&self) {
        let owners: Vec<_> = {
            let Ok(cache) = self.cache.try_lock() else {
                return;
            };
            cache
                .values()
                .filter_map(|(_, owner)| owner.upgrade())
                .collect()
        };
        for device in owners {
            device.native.pcm_metrics.report(device.native.log_id);
            let pointer = device.native.pointer.as_ptr();
            let mut event = ffi::ClockBreak::default();
            // SAFETY: local Arc retains the atomic evidence; output is exclusively owned.
            if unsafe { ffi::mr_device_io_clock_break(pointer, std::ptr::from_mut(&mut event)) } {
                tracing::info!(target: "audio_trace", owner = device.native.log_id,
                    count = event.count, expected_sample = event.expected_sample,
                    actual_sample = event.actual_sample,
                    delta_frames = event.actual_sample - event.expected_sample,
                    previous_host = event.previous_host, actual_host = event.actual_host,
                    flags = event.flags, "native clock break evidence");
            }
            // SAFETY: the local Arc retains initialized Native storage. These
            // accessors read atomics only, never the worker-owned control cell.
            let (native_frames, graph_frames, native_queued, graph_queued, overflows, clock_gaps) = unsafe {
                (
                    ffi::mr_device_io_native_frames(pointer),
                    ffi::mr_device_io_graph_frames(pointer),
                    ffi::mr_device_io_queued(pointer, false),
                    ffi::mr_device_io_queued(pointer, true),
                    ffi::mr_device_io_overflows(pointer),
                    ffi::mr_device_io_clock_discontinuities(pointer),
                )
            };
            // SAFETY: same retained owner and atomic-only observation. Select
            // the deadline-facing consumer, not the worker's empty polls.
            let pcm_starvations = unsafe { ffi::mr_device_io_starvations(pointer, device.input) };
            tracing::info!(target: "audio_trace", owner = device.native.log_id,
                input = device.input, enabled = device.activity.enabled(),
                ready = device.ready(), epoch = device.epoch.load(Ordering::Acquire),
                clock_id = device.clock_identity.load(Ordering::Acquire),
                native_rate = device.native_rate.load(Ordering::Acquire),
                graph_rate = device.graph_rate.load(Ordering::Acquire),
                native_frames, graph_frames, native_queued, graph_queued, overflows, clock_gaps,
                pcm_starvations,
                "native PCM queues");
        }
        for spec in endpoint_service::endpoint_specs() {
            match endpoint_service::snapshot(spec.id) {
                Ok(value) => tracing::info!(target: "audio_trace", endpoint = ?spec.id,
                    clock = ?value.clock, render_activity = value.render_activity,
                    render_epoch = value.render_epoch, capture_epoch = value.capture_epoch,
                    published_through = value.published_through, "HAL callback credit"),
                Err(error) => tracing::info!(target: "audio_trace", endpoint = ?spec.id,
                    %error, "HAL observation unavailable"),
            }
        }
    }
}
