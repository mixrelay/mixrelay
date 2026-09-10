//! Native workers exclusively own sinc/filter state. Atomic sample FIFOs alone
//! cross the graph/native thread boundary; control edits never borrow the DSP.
use super::SpscSampleRing;
use mixrelay_audio_io::clock_bridge::{
    AsyncClockBridge, BridgeConfig, ClockFeedback, ClockRelation, Discontinuity, FeedbackQueue,
};
use std::sync::atomic::Ordering;

pub(super) const NATIVE_MAX_FRAMES: usize = 8192;
pub(super) const GRAPH_FIFO_FRAMES: usize = 4096;
pub(super) const TARGET_FRAMES: usize = 1024;

fn new_bridge(
    channels: usize,
    input_rate: u32,
    output_rate: u32,
    queue: FeedbackQueue,
) -> Result<AsyncClockBridge, String> {
    AsyncClockBridge::new(BridgeConfig {
        channels,
        input_rate,
        output_rate,
        max_input_frames: NATIVE_MAX_FRAMES,
        max_output_frames: NATIVE_MAX_FRAMES,
        capacity_frames: NATIVE_MAX_FRAMES * 2,
        priming_frames: 128,
        clock_relation: ClockRelation::Independent,
        feedback_queue: queue,
        max_correction_ppm: 1000.0,
    })
    .map_err(|error| error.to_string())
}

pub(super) struct CaptureBridge {
    bridge: AsyncClockBridge,
    selected: usize,
    input: Vec<f32>,
    output: Vec<f32>,
    consumed: u64,
    output_rate: u32,
}

impl CaptureBridge {
    pub(super) fn reset(&mut self, ring: &SpscSampleRing) {
        self.bridge.reset(0, Discontinuity::TransportLoss);
        self.consumed = ring.consumed.load(Ordering::Acquire);
    }
    pub(super) fn new(
        channels: usize,
        native_rate: u32,
        graph_rate: u32,
        ring: &SpscSampleRing,
    ) -> Result<Self, String> {
        #[cfg(test)]
        ring.capture_evidence
            .native_rate
            .store(native_rate, Ordering::Release);
        Ok(Self {
            bridge: new_bridge(channels, native_rate, graph_rate, FeedbackQueue::Output)?,
            selected: channels,
            input: vec![0.0; NATIVE_MAX_FRAMES * channels],
            output: vec![0.0; GRAPH_FIFO_FRAMES * channels],
            consumed: ring.consumed.load(Ordering::Acquire),
            output_rate: graph_rate,
        })
    }

    pub(super) fn push_packet(
        &mut self,
        source: Option<&[f32]>,
        frames: usize,
        native_channels: usize,
        ring: &SpscSampleRing,
    ) -> Result<(), String> {
        if frames > NATIVE_MAX_FRAMES || native_channels < self.selected {
            return Err("native capture packet exceeds prepared rate/channel bounds".into());
        }
        let input = &mut self.input[..frames * self.selected];
        if let Some(source) = source {
            if source.len() != frames * native_channels {
                return Err("native capture frame shape changed".into());
            }
            for (native, selected) in source
                .chunks_exact(native_channels)
                .zip(input.chunks_exact_mut(self.selected))
            {
                selected.copy_from_slice(&native[..self.selected]);
            }
        } else {
            input.fill(0.0);
        }
        let consumed = ring.consumed.load(Ordering::Acquire);
        let advanced = usize::try_from(consumed.saturating_sub(self.consumed))
            .unwrap_or(usize::MAX)
            / self.selected;
        self.consumed = consumed;
        // The graph consumer advances this FIFO at its clock rate. Producer
        // packet count/private staging occupancy are not clock observations.
        if advanced > 0 && advanced <= self.output_rate as usize {
            self.bridge
                .observe_clock(ClockFeedback {
                    queue: FeedbackQueue::Output,
                    epoch: 0,
                    queued_frames: ring.available_frames(self.selected),
                    target_frames: TARGET_FRAMES,
                    elapsed_frames: advanced,
                    clock_running: true,
                })
                .map_err(|error| error.to_string())?;
        }
        self.bridge
            .push_interleaved(input)
            .map_err(|error| error.to_string())?;
        loop {
            let free = GRAPH_FIFO_FRAMES.saturating_sub(ring.available_frames(self.selected));
            if free == 0 {
                break;
            }
            let report = self
                .bridge
                .pull_interleaved(&mut self.output[..free * self.selected])
                .map_err(|error| error.to_string())?;
            let samples = report.transferred_frames * self.selected;
            if samples == 0 {
                break;
            }
            if ring.push(&self.output[..samples], self.selected) != samples {
                return Err("capture clock FIFO overflow".into());
            }
        }
        Ok(())
    }
}

pub(super) struct RenderBridge {
    bridge: AsyncClockBridge,
    channels: usize,
    input: Vec<f32>,
    output: Vec<f32>,
    input_rate: u32,
    produced: u64,
    primed: bool,
    steady: bool,
    master: bool,
    epoch: u64,
}

impl RenderBridge {
    pub(super) fn new(
        channels: usize,
        graph_rate: u32,
        native_rate: u32,
        ring: &SpscSampleRing,
    ) -> Result<Self, String> {
        Ok(Self {
            bridge: new_bridge(channels, graph_rate, native_rate, FeedbackQueue::Input)?,
            channels,
            input: vec![0.0; GRAPH_FIFO_FRAMES * channels],
            output: vec![0.0; NATIVE_MAX_FRAMES * channels],
            input_rate: graph_rate,
            produced: ring.write.load(Ordering::Acquire),
            primed: false,
            steady: false,
            master: false,
            epoch: 0,
        })
    }

    pub(super) fn fill(
        &mut self,
        destination: &mut [f32],
        native_channels: usize,
        ring: &SpscSampleRing,
        master: bool,
    ) -> Result<usize, String> {
        destination.fill(0.0);
        let frames = destination.len() / native_channels;
        if frames > NATIVE_MAX_FRAMES || self.channels > native_channels {
            return Err("native render quantum exceeds prepared rate/channel bounds".into());
        }
        if master != self.master {
            // Clock-authority failover resets only bridge history, not COM or
            // the endpoint. Rubato reset also restores the nominal SRC ratio.
            self.epoch += 1;
            self.bridge.reset(self.epoch, Discontinuity::StreamRestart);
            self.primed = false;
            self.steady = false;
            self.master = master;
        }
        let available = ring.available_frames(self.channels);
        if !self.primed {
            if available < TARGET_FRAMES {
                return Ok(0);
            }
            self.primed = true;
        }
        let produced = ring.write.load(Ordering::Acquire);
        let advanced = usize::try_from(produced.saturating_sub(self.produced))
            .unwrap_or(usize::MAX)
            / self.channels;
        self.produced = produced;
        if !master && advanced > 0 && advanced <= self.input_rate as usize {
            self.bridge
                .observe_clock(ClockFeedback {
                    queue: FeedbackQueue::Input,
                    epoch: self.epoch,
                    queued_frames: available + self.bridge.buffered_input_frames(),
                    target_frames: TARGET_FRAMES,
                    elapsed_frames: advanced,
                    clock_running: true,
                })
                .map_err(|error| error.to_string())?;
        }
        let samples = ring.pop(&mut self.input[..available * self.channels], self.channels);
        self.bridge
            .push_interleaved(&self.input[..samples])
            .map_err(|error| error.to_string())?;
        let report = self
            .bridge
            .pull_interleaved(&mut self.output[..frames * self.channels])
            .map_err(|error| error.to_string())?;
        for (source, target) in self.output[..report.transferred_frames * self.channels]
            .chunks_exact(self.channels)
            .zip(destination.chunks_exact_mut(native_channels))
        {
            target[..self.channels].copy_from_slice(source);
        }
        self.steady |= report.transferred_frames == frames;
        Ok(report.transferred_frames)
    }

    pub(super) fn steady(&self) -> bool {
        self.steady
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_discard_does_not_advance_capture_clock_feedback() {
        let ring = SpscSampleRing::new(GRAPH_FIFO_FRAMES);
        let mut capture = CaptureBridge::new(1, 48_000, 48_000, &ring).unwrap();
        capture
            .push_packet(Some(&[0.1; 2048]), 2048, 1, &ring)
            .unwrap();
        capture.reset(&ring);
        ring.discard();
        capture
            .push_packet(Some(&[0.1; 256]), 256, 1, &ring)
            .unwrap();
        assert_eq!(ring.consumed.load(Ordering::Acquire), 0);
        assert!(capture.bridge.correction_ppm().abs() < f64::EPSILON);
    }

    #[test]
    fn render_startup_is_distinct_from_later_underrun() {
        let ring = SpscSampleRing::new(GRAPH_FIFO_FRAMES);
        let mut render = RenderBridge::new(1, 48_000, 48_000, &ring).unwrap();
        let mut output = [0.0; 480];
        assert_eq!(render.fill(&mut output, 1, &ring, true).unwrap(), 0);
        assert!(!render.steady());
        ring.push(&[0.1; TARGET_FRAMES], 1);
        for _ in 0..5 {
            ring.push(&[0.1; 480], 1);
            render.fill(&mut output, 1, &ring, true).unwrap();
        }
        assert!(render.steady());
        for _ in 0..8 {
            render.fill(&mut output, 1, &ring, true).unwrap();
        }
        assert_eq!(render.fill(&mut output, 1, &ring, true).unwrap(), 0);
        assert!(
            render.steady(),
            "an actual later shortage remains countable"
        );
    }

    #[test]
    fn windows_worker_bridges_preserve_pcm_across_native_rates() {
        for native_rate in [44_100, 48_000, 96_000, 192_000] {
            let ring = SpscSampleRing::new(GRAPH_FIFO_FRAMES * 2);
            let mut capture = CaptureBridge::new(2, native_rate, 48_000, &ring).unwrap();
            let native_frames = native_rate as usize / 100;
            let source = vec![0.25_f32; native_frames * 2];
            let mut received = [0.0; 960];
            let mut frames = 0;
            for _ in 0..100 {
                capture
                    .push_packet(Some(&source), native_frames, 2, &ring)
                    .unwrap();
                if ring.available_frames(2) >= TARGET_FRAMES {
                    frames += ring.pop(&mut received, 2) / 2;
                }
            }
            assert!(
                frames > 45_000,
                "native capture rate={native_rate}, frames={frames}"
            );
            assert!(received.iter().all(|sample| (*sample - 0.25).abs() < 0.001));

            let ring = SpscSampleRing::new(GRAPH_FIFO_FRAMES * 2);
            let mut render = RenderBridge::new(2, 48_000, native_rate, &ring).unwrap();
            let graph = [0.25_f32; 960];
            let mut destination = vec![0.0; native_frames * 2];
            let mut rendered = 0;
            for _ in 0..100 {
                assert_eq!(ring.push(&graph, 2), graph.len());
                rendered += render.fill(&mut destination, 2, &ring, true).unwrap();
            }
            assert!(rendered > native_rate as usize * 9 / 10);
            assert!(
                destination
                    .iter()
                    .all(|sample| (*sample - 0.25).abs() < 0.001)
            );
            assert!(
                render.bridge.correction_ppm().abs() < f64::EPSILON,
                "pacing master stays nominal"
            );
        }
    }

    #[test]
    #[ignore = "bounded offline 120-second independent-clock simulation, no native audio"]
    fn windows_capture_fifo_drift_before_after_same_clock_conditions() {
        let run = |feedback: bool| {
            let ring = SpscSampleRing::new(GRAPH_FIFO_FRAMES);
            let mut capture = CaptureBridge::new(1, 48_000, 48_000, &ring).unwrap();
            let source = [0.01_f32; 257];
            let mut output = [0.0; 256];
            let mut remainder = 0_u64;
            let mut primed = false;
            let mut tail = (usize::MAX, 0);
            for block in 0..22_500 {
                // Identical actual source +300 ppm and graph clock quanta in
                // both runs. Only external-FIFO feedback is disabled in baseline.
                remainder += 256 * 1_000_300;
                let frames = usize::try_from(remainder / 1_000_000).unwrap();
                remainder %= 1_000_000;
                if !feedback {
                    capture.consumed = ring.consumed.load(Ordering::Acquire);
                }
                capture
                    .push_packet(Some(&source[..frames]), frames, 1, &ring)
                    .unwrap();
                primed |= ring.available_frames(1) >= TARGET_FRAMES;
                if primed {
                    assert_eq!(ring.pop(&mut output, 1), 256);
                }
                if block > 18_000 {
                    let queued = ring.available_frames(1) + capture.bridge.queued_input_frames();
                    tail.0 = tail.0.min(queued);
                    tail.1 = tail.1.max(queued);
                }
            }
            (tail, capture.bridge.correction_ppm())
        };
        let baseline = run(false);
        let corrected = run(true);
        eprintln!(
            "Windows actual FIFO +300ppm/120s: disabled feedback tail={baseline:?}; enabled tail={corrected:?}"
        );
        assert!(baseline.0.1 > corrected.0.1 + 800);
        assert!(corrected.0.1 - corrected.0.0 < 256);
        assert!(corrected.1 < -100.0 && corrected.1 > -600.0);
    }

    #[test]
    #[ignore = "bounded offline 120-second secondary-render clock simulation, no native audio"]
    fn windows_render_fifo_drift_before_after_same_clock_conditions() {
        let run = |feedback: bool| {
            let ring = SpscSampleRing::new(GRAPH_FIFO_FRAMES);
            let mut render = RenderBridge::new(1, 48_000, 48_000, &ring).unwrap();
            let source = [0.01_f32; 257];
            let mut destination = [0.0; 256];
            assert_eq!(ring.push(&[0.01; TARGET_FRAMES], 1), TARGET_FRAMES);
            let mut remainder = 0_u64;
            let mut tail = (usize::MAX, 0);
            for block in 0..22_500 {
                remainder += 256 * 1_000_300;
                let frames = usize::try_from(remainder / 1_000_000).unwrap();
                remainder %= 1_000_000;
                assert_eq!(ring.push(&source[..frames], 1), frames);
                if !feedback {
                    render.produced = ring.write.load(Ordering::Acquire);
                }
                assert_eq!(render.fill(&mut destination, 1, &ring, false).unwrap(), 256);
                if block > 18_000 {
                    let queued = ring.available_frames(1) + render.bridge.buffered_input_frames();
                    tail.0 = tail.0.min(queued);
                    tail.1 = tail.1.max(queued);
                }
            }
            (tail, render.bridge.correction_ppm())
        };
        let baseline = run(false);
        let corrected = run(true);
        eprintln!(
            "Windows render FIFO +300ppm/120s: disabled feedback tail={baseline:?}; enabled tail={corrected:?}"
        );
        assert!(baseline.0.1 > corrected.0.1 + 800);
        assert!(corrected.0.1 - corrected.0.0 < 256);
        assert!(corrected.1 < -100.0 && corrected.1 > -600.0);
    }
}
