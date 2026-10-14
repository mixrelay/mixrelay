//! Retained atomic evidence; only the reporter acquires the control-side cache lock.
use super::{DeviceSet, MacDevices, ffi};
use mixrelay_audio_engine::BlockProcessor;
use mixrelay_audio_io::{
    process::ProcessReport,
    residence::{Residence, ResidenceSnapshot},
};
use mixrelay_macos_transport::device_io::{
    IoTiming, LatencySnapshot, latency_snapshot, record_direct_latency,
};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

#[derive(Default)]
pub(super) struct BridgeMeasurements {
    pub(super) src_input: Residence,
    pub(super) src_output: Residence,
    pub(super) src_delay: AtomicU64,
    pub(super) faults: AtomicU64,
    revision: AtomicU64,
    graph_epoch: AtomicU64,
    compensation_ready: AtomicBool,
}

#[derive(Clone, Copy)]
pub(crate) struct Observation {
    pub(crate) identity: u64,
    pub(crate) epoch: u64,
    pub(crate) graph_epoch: u64,
    pub(crate) generation: u64,
    pub(crate) clock_breaks: u64,
    pub(crate) native_rate: u32,
    pub(crate) graph_rate: u32,
    pub(crate) direct: bool,
    pub(crate) ready: bool,
    pub(crate) sampled: bool,
    pub(crate) faults: u64,
    pub(crate) native: LatencySnapshot,
    pub(crate) src_input: ResidenceSnapshot,
    pub(crate) src_output: ResidenceSnapshot,
    pub(crate) src_delay: u64,
}
impl Observation {
    pub(crate) fn context(self) -> (u64, u64, u64, u64, u32, u32, bool, u64) {
        (
            self.identity,
            self.epoch,
            self.generation,
            self.clock_breaks,
            self.native_rate,
            self.graph_rate,
            self.direct,
            self.graph_epoch,
        )
    }
    pub(crate) fn unstable(self, previous: Self) -> bool {
        !self.sampled
            || !previous.sampled
            || !self.ready
            || self.faults != previous.faults
            || self.native.faults != previous.native.faults
            || self.native.native_queue.invalid != previous.native.native_queue.invalid
            || self.native.graph_queue.invalid != previous.native.graph_queue.invalid
            || self.src_input.invalid != previous.src_input.invalid
            || self.src_output.invalid != previous.src_output.invalid
    }
    /// Host intervals in ticks; all FIFO means in their own rate domain.
    pub(crate) fn delay_ns(self, previous: Self, input: bool, ns_per_tick: f64) -> Option<f64> {
        let timing = if input {
            self.native
                .acquisition
                .mean_since(previous.native.acquisition)
        } else {
            self.native
                .presentation
                .mean_since(previous.native.presentation)
        }? * ns_per_tick;
        if self.direct {
            return Some(timing);
        }
        let native = self
            .native
            .native_queue
            .mean_since(previous.native.native_queue)?;
        let graph = self
            .native
            .graph_queue
            .mean_since(previous.native.graph_queue)?;
        let src_input = self.src_input.mean_since(previous.src_input)?;
        let src_output = self.src_output.mean_since(previous.src_output)?;
        let scalar = if input {
            self.native
                .input_scalar
                .mean_since(previous.native.input_scalar)
        } else {
            self.native
                .output_scalar
                .mean_since(previous.native.output_scalar)
        }
        .map_or(1.0, |scalar| scalar / 1e9);
        let native_frame_ns = scalar * 1e9 / f64::from(self.native_rate);
        let graph_frame_ns = 1e9 / f64::from(self.graph_rate);
        // SRC queue-time areas advance in nominal native callback time. Both
        // domains need its measured scalar, including graph-domain residence.
        // Algorithmic delay is a frame count at the converter's output rate.
        let (input_frame_ns, output_frame_ns, delay_frame_ns) = if input {
            (native_frame_ns, graph_frame_ns * scalar, graph_frame_ns)
        } else {
            (graph_frame_ns * scalar, native_frame_ns, native_frame_ns)
        };
        #[allow(clippy::cast_precision_loss)]
        let delay = self.src_delay as f64;
        Some(
            timing
                + native * 0.5 * native_frame_ns
                + graph * 0.5 * graph_frame_ns
                + src_input * 0.5 * input_frame_ns
                + src_output * 0.5 * output_frame_ns
                + delay * delay_frame_ns,
        )
    }
}

impl DeviceSet {
    pub(crate) fn record_latency_cycle(
        &self,
        processor: &BlockProcessor,
        report: Option<ProcessReport>,
        epoch: u64,
    ) {
        let _ = processor.acknowledge_content_latency();
        for (id, device) in &self.0 {
            let ready = processor.binding_content_latency_ready(id, device.input);
            let taps = &device.native.latency;
            taps.compensation_ready.store(ready, Ordering::Release);
            taps.graph_epoch.store(epoch, Ordering::Release);
            taps.revision
                .store(processor.plan().revision(), Ordering::Release);
            if !ready || report.is_none() {
                taps.faults.fetch_add(1, Ordering::Release);
            }
        }
    }
}

impl super::direct::DirectPlan {
    pub(crate) fn record_latency(
        &self,
        processor: &BlockProcessor,
        report: ProcessReport,
        timing: Option<IoTiming>,
        epoch: u64,
    ) {
        let _ = processor.acknowledge_content_latency();
        for owner in &self.owners {
            let ready = self
                .latency_bindings
                .iter()
                .filter(|(_, device)| std::sync::Arc::ptr_eq(device, owner))
                .all(|(id, device)| processor.binding_content_latency_ready(id, device.input));
            if let Some(timing) = timing {
                // SAFETY: this direct master is the sole writer of each direct tap.
                unsafe { record_direct_latency(owner.native.pointer.as_ptr().cast(), timing) };
            }
            owner
                .native
                .latency
                .compensation_ready
                .store(ready, Ordering::Release);
            owner
                .native
                .latency
                .graph_epoch
                .store(epoch, Ordering::Release);
            owner
                .native
                .latency
                .revision
                .store(processor.plan().revision(), Ordering::Release);
            if !ready || timing.is_none() {
                owner.native.latency.faults.fetch_add(1, Ordering::Release);
            }
        }
        self.bridged
            .record_latency_cycle(processor, Some(report), epoch);
    }
}

impl MacDevices {
    pub(crate) fn latency_observations(
        &self,
        revision: u64,
        timing_pending: bool,
    ) -> BTreeMap<String, Observation> {
        let cache = self
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cache
            .iter()
            .filter_map(|(id, (_, weak))| {
                let device = weak.upgrade()?;
                let taps = &device.native.latency;
                if taps.revision.load(Ordering::Acquire) != revision {
                    return None;
                }
                let direct = device.native.render.enabled();
                let pointer = device.native.pointer.as_ptr();
                // SAFETY: the local Arc retains the initialized owner; only atomics are read.
                let native = unsafe { latency_snapshot(pointer.cast(), direct) };
                let src_input = taps.src_input.snapshot();
                let src_output = taps.src_output.snapshot();
                let sampled = native.is_some() && src_input.is_some() && src_output.is_some();
                Some((
                    id.clone(),
                    Observation {
                        identity: device.identity,
                        epoch: device.epoch.load(Ordering::Acquire),
                        graph_epoch: taps.graph_epoch.load(Ordering::Acquire),
                        generation: device.native.render.generation.load(Ordering::Acquire),
                        clock_breaks: unsafe { ffi::mr_device_io_clock_discontinuities(pointer) },
                        native_rate: device.native_rate.load(Ordering::Acquire),
                        graph_rate: device.graph_rate.load(Ordering::Acquire),
                        direct,
                        ready: !timing_pending
                            && !device.failed.load(Ordering::Acquire)
                            && (direct || device.ready())
                            && device.activity.ready()
                            && taps.compensation_ready.load(Ordering::Acquire),
                        faults: taps
                            .faults
                            .load(Ordering::Acquire)
                            .wrapping_add(unsafe { ffi::mr_device_io_overflows(pointer) }),
                        sampled,
                        native: native.unwrap_or_default(),
                        src_input: src_input.unwrap_or_default(),
                        src_output: src_output.unwrap_or_default(),
                        src_delay: taps.src_delay.load(Ordering::Acquire),
                    },
                ))
            })
            .collect()
    }
}

/// Match the same directional suffix contract as graph device lookup.
pub(crate) fn binding<'a>(
    observations: &'a BTreeMap<String, Observation>,
    id: &str,
    input: bool,
) -> Option<&'a Observation> {
    observations.get(id).or_else(|| {
        observations.get(&format!(
            "{id}/{}",
            if input { "capture" } else { "render" }
        ))
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn observation(direct: bool) -> Observation {
        Observation {
            identity: 1,
            epoch: 2,
            graph_epoch: 5,
            generation: 3,
            clock_breaks: 0,
            native_rate: 44_100,
            graph_rate: 48_000,
            direct,
            ready: true,
            sampled: true,
            faults: 0,
            native: LatencySnapshot::default(),
            src_input: ResidenceSnapshot::default(),
            src_output: ResidenceSnapshot::default(),
            src_delay: 0,
        }
    }
    use mixrelay_audio_engine::{
        ExternalBinding, ExternalIo, HostedAudioProcessor, HostedProcessorFactory,
        HostedProcessorPrepareRequest,
    };
    use mixrelay_audio_io::process::{ProcessCycle, TransferOutcome};
    pub(crate) struct Factory;
    impl HostedProcessorFactory for Factory {
        fn prepare(
            &self,
            _: HostedProcessorPrepareRequest<'_>,
        ) -> Result<Box<dyn HostedAudioProcessor>, String> {
            unreachable!()
        }
    }
    struct Io;
    impl ExternalIo for Io {
        fn read_source(
            &mut self,
            _: &ExternalBinding,
            _: &mut [f32],
            cycle: &ProcessCycle,
        ) -> TransferOutcome {
            TransferOutcome::complete(cycle.frames)
        }
        fn write_sink(
            &mut self,
            _: &ExternalBinding,
            _: &[f32],
            cycle: &ProcessCycle,
        ) -> TransferOutcome {
            TransferOutcome::complete(cycle.frames)
        }
    }
    #[test]
    fn unrelated_process_report_faults_do_not_taint_bridged_or_direct_bindings() {
        use mixrelay_audio_engine::PeakMeterBank;
        use mixrelay_audio_io::process::ClockDomain;
        use std::sync::Arc;
        let mut processor =
            crate::macos::test_feedback_processor(&Factory, Arc::new(PeakMeterBank::new(8, 64)));
        processor
            .process_external_cycle(
                ProcessCycle {
                    clock: ClockDomain {
                        authority: mixrelay_audio_io::session::ClockAuthority::Device,
                        id: 1,
                        epoch: 1,
                    },
                    sample_rate: 48_000,
                    frames: 32,
                    first_frame: 0,
                    monotonic_ns: None,
                    discontinuity: false,
                },
                &mut Io,
            )
            .unwrap()
            .unwrap();
        let (device, _) = super::super::tests::synthetic_device(2, 48_000, 48_000, false);
        let set = DeviceSet(
            BTreeMap::from([("virtual/render".into(), Arc::clone(&device))]),
            BTreeMap::new(),
        );
        let unrelated = ProcessReport {
            input_starvations: 1,
            output_overflows: 1,
            discontinuities: 1,
            ..Default::default()
        };
        set.record_latency_cycle(&processor, Some(unrelated), 1);
        assert_eq!(device.native.latency.faults.load(Ordering::Acquire), 0);
        assert!(
            device
                .native
                .latency
                .compensation_ready
                .load(Ordering::Acquire)
        );
        let direct = super::super::direct::DirectPlan {
            master: Arc::clone(&device),
            owners: vec![Arc::clone(&device)],
            latency_bindings: vec![("virtual/render".into(), Arc::clone(&device))],
            bindings: vec![],
            bridged: set,
            clock_id: 1,
        };
        direct.record_latency(&processor, unrelated, Some(IoTiming::default()), 1);
        assert_eq!(device.native.latency.faults.load(Ordering::Acquire), 0);
    }

    #[test]
    fn native_scalar_window_mean_scales_both_src_residence_domains() {
        let mut old = observation(false);
        old.native_rate = 48_000;
        let mut next = old;
        let sample = |sum| ResidenceSnapshot {
            sum,
            weight: 1,
            invalid: 0,
        };
        next.native.acquisition = sample(0);
        next.native.presentation = sample(0);
        next.native.native_queue = sample(24_000);
        next.native.graph_queue = sample(96);
        next.src_input = sample(0);
        next.src_output = sample(0);
        // No scalar captured: 12000 native frames at 48 kHz = 250 ms.
        assert_eq!(next.delay_ns(old, true, 1.0), Some(251_000_000.0));
        next.native.input_scalar = ResidenceSnapshot {
            sum: 2_002_000_000,
            weight: 2,
            invalid: 0,
        };
        next.native.output_scalar = sample(1_002_000_000);
        assert!((next.delay_ns(old, true, 1.0).unwrap() - 251_250_000.0).abs() < 0.001);
        next.src_input = sample(96);
        next.src_output = sample(96);
        next.src_delay = 48;
        let input = 251_250_000.0 + 1_001_000.0 + 1_001_000.0 + 1_000_000.0;
        let output = 251_500_000.0 + 1_002_000.0 + 1_002_000.0 + 1_002_000.0;
        assert!((next.delay_ns(old, true, 1.0).unwrap() - input).abs() < 0.001);
        assert!((next.delay_ns(old, false, 1.0).unwrap() - output).abs() < 0.001);
    }

    #[test]
    fn residence_uses_each_converter_output_rate_and_direct_has_no_queues() {
        let old = observation(false);
        let mut next = old;
        let sample = |sum| ResidenceSnapshot {
            sum,
            weight: 1,
            invalid: 0,
        };
        next.native.acquisition = sample(100);
        next.native.presentation = sample(50);
        next.native.native_queue = sample(88);
        next.native.graph_queue = sample(96);
        next.src_input = sample(44);
        next.src_output = sample(48);
        next.src_delay = 10;
        let input = 100.0
            + 88.0 * 0.5e9 / 44_100.0
            + 96.0 * 0.5e9 / 48_000.0
            + 44.0 * 0.5e9 / 44_100.0
            + 48.0 * 0.5e9 / 48_000.0
            + 10.0 * 1e9 / 48_000.0;
        assert_eq!(next.delay_ns(old, true, 1.0), Some(input));
        let output = 50.0
            + 88.0 * 0.5e9 / 44_100.0
            + 96.0 * 0.5e9 / 48_000.0
            + 44.0 * 0.5e9 / 48_000.0
            + 48.0 * 0.5e9 / 44_100.0
            + 10.0 * 1e9 / 44_100.0;
        assert_eq!(next.delay_ns(old, false, 1.0), Some(output));
        assert!(!next.unstable(old));
        next.faults += 1;
        assert!(next.unstable(old));
        next.direct = true;
        assert_eq!(next.delay_ns(old, true, 1.0), Some(100.0));
        assert_eq!(next.delay_ns(old, false, 1.0), Some(50.0));
        next.native.acquisition.invalid = 1;
        assert_eq!(next.delay_ns(old, true, 1.0), None);
        let values = BTreeMap::from([("source/capture".into(), old), ("sink/render".into(), next)]);
        assert!(binding(&values, "source", true).is_some());
        assert!(binding(&values, "sink", false).is_some());
        assert!(binding(&values, "source", false).is_none());
    }
}
