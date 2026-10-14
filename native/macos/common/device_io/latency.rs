//! Callback host intervals and consumer-side FIFO residence, never nominal targets.
use super::{DeviceIo, IoTiming};
use mixrelay_audio_io::residence::{Residence, ResidenceSnapshot};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
pub(super) struct Measurements {
    pub(super) faults: AtomicU64,
    pub(super) acquisition: Residence,
    pub(super) presentation: Residence,
    pub(super) direct_acquisition: Residence,
    pub(super) direct_presentation: Residence,
    pub(super) input_scalar: Residence,
    pub(super) output_scalar: Residence,
    pub(super) direct_input_scalar: Residence,
    pub(super) direct_output_scalar: Residence,
    pub(super) native_queue: Residence,
    pub(super) graph_queue: Residence,
}

impl Measurements {
    pub(super) fn callback(&self, timing: IoTiming, direct: bool) {
        let (input, output) = if direct {
            (&self.direct_acquisition, &self.direct_presentation)
        } else {
            (&self.acquisition, &self.presentation)
        };
        input.record(interval(timing.now, timing.input), 1);
        output.record(interval(timing.output, timing.now), 1);
        let (input, output) = if direct {
            (&self.direct_input_scalar, &self.direct_output_scalar)
        } else {
            (&self.input_scalar, &self.output_scalar)
        };
        scalar(input, timing.input);
        scalar(output, timing.output);
    }
}

// Fixed-point scalars preserve a coherent sum/count using the existing atomic tap.
// Absent or invalid scalars are skipped, so a window with none falls back to nominal.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "validated finite positive fixed-point scalar"
)]
fn scalar(tap: &Residence, timestamp: super::IoTimestamp) {
    let value = timestamp.rate_scalar * 1e9;
    if timestamp.flags & 4 != 0 && value.is_finite() && (1.0..1e12).contains(&value) {
        tap.record(Some(value.round() as u64), 1);
    }
}

fn interval(later: super::IoTimestamp, earlier: super::IoTimestamp) -> Option<u64> {
    (later.flags & earlier.flags & 2 != 0)
        .then(|| later.host_time.checked_sub(earlier.host_time))
        .flatten()
}

/// Coherent individual tap snapshots. Host intervals are ticks; queues are twice frames.
#[derive(Debug, Clone, Copy, Default)]
pub struct LatencySnapshot {
    pub faults: u64,
    pub acquisition: ResidenceSnapshot,
    pub presentation: ResidenceSnapshot,
    /// Window scalar means are fixed-point (1e9 = nominal); no valid sample means nominal.
    pub input_scalar: ResidenceSnapshot,
    pub output_scalar: ResidenceSnapshot,
    pub native_queue: ResidenceSnapshot,
    pub graph_queue: ResidenceSnapshot,
}

/// Reads atomics only; a busy writer returns no snapshot instead of blocking.
/// # Safety
/// The native owner is retained throughout this call.
pub unsafe fn latency_snapshot(io: *const DeviceIo, direct: bool) -> Option<LatencySnapshot> {
    let io = unsafe { io.as_ref() }?;
    let taps = &io.measurements;
    let taps = [
        if direct {
            &taps.direct_acquisition
        } else {
            &taps.acquisition
        },
        if direct {
            &taps.direct_presentation
        } else {
            &taps.presentation
        },
        if direct {
            &taps.direct_input_scalar
        } else {
            &taps.input_scalar
        },
        if direct {
            &taps.direct_output_scalar
        } else {
            &taps.output_scalar
        },
        &taps.native_queue,
        &taps.graph_queue,
    ]
    .map(Residence::snapshot);
    coherent_snapshot(io.measurements.faults.load(Ordering::Acquire), taps)
}

fn coherent_snapshot(faults: u64, taps: [Option<ResidenceSnapshot>; 6]) -> Option<LatencySnapshot> {
    let [
        Some(acquisition),
        Some(presentation),
        Some(input_scalar),
        Some(output_scalar),
        Some(native_queue),
        Some(graph_queue),
    ] = taps
    else {
        return None;
    };
    Some(LatencySnapshot {
        faults,
        acquisition,
        presentation,
        input_scalar,
        output_scalar,
        native_queue,
        graph_queue,
    })
}

/// Same `IOProc` timings for the bindings actually rendered by the direct master.
/// # Safety
/// Direct rendering serializes this tap's sole writer; IO is retained until drain.
pub unsafe fn record_direct_latency(io: *const DeviceIo, timing: IoTiming) {
    if let Some(io) = unsafe { io.as_ref() } {
        io.measurements.callback(timing, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_io::IoTimestamp;
    #[test]
    fn any_contended_tap_returns_no_new_evidence() {
        let taps = [Some(ResidenceSnapshot::default()); 6];
        assert!(coherent_snapshot(0, taps).is_some());
        for index in 0..taps.len() {
            let mut contended = taps;
            contended[index] = None;
            assert!(coherent_snapshot(0, contended).is_none());
        }
    }

    #[test]
    fn scalar_capture_skips_invalid_values_and_keeps_directional_window_means() {
        let io = DeviceIo::new(c"unit", true, 1, 48_000).unwrap();
        let before = unsafe { latency_snapshot(&raw const io, false) }.unwrap();
        let timing = |input, output| IoTiming {
            input: IoTimestamp {
                rate_scalar: input,
                flags: 4,
                ..Default::default()
            },
            output: IoTimestamp {
                rate_scalar: output,
                flags: 4,
                ..Default::default()
            },
            ..Default::default()
        };
        let allocations = allocation_counter::measure(|| {
            for invalid in [f64::NAN, f64::INFINITY, 0.0, -1.0, 1000.0] {
                io.measurements.callback(timing(invalid, invalid), false);
            }
            io.measurements.callback(timing(1.000, 0.998), false);
            io.measurements.callback(timing(1.002, 1.000), false);
            unsafe { record_direct_latency(&raw const io, timing(1.001, 1.002)) };
        });
        assert_eq!(allocations.count_total, 0);
        let next = unsafe { latency_snapshot(&raw const io, false) }.unwrap();
        assert_eq!(
            next.input_scalar.mean_since(before.input_scalar),
            Some(1_001_000_000.0)
        );
        assert_eq!(
            next.output_scalar.mean_since(before.output_scalar),
            Some(999_000_000.0)
        );
        let direct = unsafe { latency_snapshot(&raw const io, true) }.unwrap();
        assert_eq!(
            direct.input_scalar.mean_since(before.input_scalar),
            Some(1_001_000_000.0)
        );
        assert_eq!(
            direct.output_scalar.mean_since(before.output_scalar),
            Some(1_002_000_000.0)
        );
    }

    #[test]
    fn synthetic_host_intervals_validate_flags_direction_and_direct_taps() {
        let io = DeviceIo::new(c"unit", true, 1, 48_000).unwrap();
        let time = |host_time| IoTimestamp {
            host_time,
            flags: 2,
            ..Default::default()
        };
        let timing = IoTiming {
            now: time(100),
            input: time(40),
            output: time(130),
        };
        let before = unsafe { latency_snapshot(&raw const io, false) }.unwrap();
        let allocations = allocation_counter::measure(|| {
            io.measurements.callback(timing, false);
            unsafe { record_direct_latency(&raw const io, timing) };
        });
        assert_eq!(allocations.count_total, 0);
        let after = unsafe { latency_snapshot(&raw const io, false) }.unwrap();
        assert_eq!(after.acquisition.mean_since(before.acquisition), Some(60.0));
        assert_eq!(
            after.presentation.mean_since(before.presentation),
            Some(30.0)
        );
        assert_eq!(
            unsafe { latency_snapshot(&raw const io, true) }
                .unwrap()
                .acquisition
                .sum,
            60
        );
        for invalid in [
            IoTiming::default(),
            IoTiming {
                now: time(150),
                ..timing
            },
        ] {
            io.measurements.callback(invalid, false);
        }
        assert_eq!(
            unsafe { latency_snapshot(&raw const io, false) }
                .unwrap()
                .presentation
                .mean_since(after.presentation),
            None
        );
        assert!(unsafe { latency_snapshot(std::ptr::null(), false) }.is_none());
        unsafe { record_direct_latency(std::ptr::null(), timing) };
    }
}
