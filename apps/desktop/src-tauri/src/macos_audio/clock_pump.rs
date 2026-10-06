//! Non-real-time owner for process-tap SRC. Feedback comes from the engine's
//! consumed shared-ring cursor, never capture amplitude or the private staging FIFO.
use super::{
    ApplicationSourceTransport, CAPTURE_RING_CAPACITY_FRAMES, ENGINE_BLOCK_FRAMES,
    NativeCaptureReader, NativeClockSnapshot, PUMP_IDLE_INTERVAL,
};
use mixrelay_audio_io::clock_bridge::{
    AsyncClockBridge, BridgeConfig, ClockFeedback, ClockRelation, Discontinuity, FeedbackQueue,
};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

pub(super) fn native_rate(rate: f64) -> Result<u32, String> {
    if !rate.is_finite() || !(8_000.0..=192_000.0).contains(&rate) || rate.fract().abs() > 0.001 {
        return Err(format!(
            "unsupported application capture sample rate: {rate}"
        ));
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "finite integral validated sample rate"
    )]
    Ok(rate as u32)
}

struct PumpClock {
    bridge: AsyncClockBridge,
    format: u64,
    native_rate: u32,
    ticks: u64,
    epoch: u64,
    dropped_frames: u64,
    queued_input: usize,
    native_generation: u64,
    native_discontinuities: u64,
    clock_master: bool,
}

impl PumpClock {
    fn needs_reset(
        &self,
        clock: NativeClockSnapshot,
        health: &super::CaptureProbeStats,
        rate: u32,
    ) -> bool {
        self.format != clock.format_word
            || self.native_rate != rate
            || self.dropped_frames != health.dropped_frames
            || self.native_generation != health.format_generation
            || self.native_discontinuities != health.clock_discontinuities
            || self.clock_master != (clock.clock_master != 0)
    }

    fn new(
        channels: usize,
        rate: u32,
        clock: NativeClockSnapshot,
        dropped_frames: u64,
    ) -> Result<Self, String> {
        let graph_rate =
            u32::try_from(clock.format_word & u64::from(u32::MAX)).map_err(|e| e.to_string())?;
        let bridge = AsyncClockBridge::new(BridgeConfig {
            channels,
            input_rate: rate,
            output_rate: graph_rate,
            max_input_frames: ENGINE_BLOCK_FRAMES,
            max_output_frames: ENGINE_BLOCK_FRAMES,
            capacity_frames: CAPTURE_RING_CAPACITY_FRAMES,
            priming_frames: 256,
            clock_relation: if clock.clock_master != 0 {
                ClockRelation::ProvenSameClock
            } else {
                ClockRelation::Independent
            },
            feedback_queue: FeedbackQueue::Output,
            max_correction_ppm: 2_000.0,
        })
        .map_err(|e| e.to_string())?;
        Ok(Self {
            bridge,
            format: clock.format_word,
            native_rate: rate,
            ticks: clock.consumer_ticks,
            epoch: 0,
            dropped_frames,
            queued_input: 0,
            native_generation: 0,
            native_discontinuities: 0,
            clock_master: clock.clock_master != 0,
        })
    }

    fn observe(&mut self, clock: NativeClockSnapshot) -> Result<usize, String> {
        let queued = clock.produced_frames.saturating_sub(clock.consumed_frames);
        let queued = usize::try_from(queued).map_err(|e| e.to_string())?;
        let elapsed = clock.consumer_ticks.saturating_sub(self.ticks);
        self.ticks = clock.consumer_ticks;
        // A stopped consumer does not turn clock-running merely because its last
        // published flag was true. No position advance means no PI update.
        let elapsed = usize::try_from(elapsed).map_err(|e| e.to_string())?;
        let output_rate =
            usize::try_from(self.format & u64::from(u32::MAX)).map_err(|e| e.to_string())?;
        self.bridge
            .observe_clock(ClockFeedback {
                queue: FeedbackQueue::Output,
                epoch: self.epoch,
                queued_frames: queued,
                target_frames: 512,
                elapsed_frames: elapsed.min(output_rate),
                clock_running: clock.consumer_running != 0 && elapsed > 0 && elapsed <= output_rate,
            })
            .map_err(|e| e.to_string())?;
        Ok(queued)
    }

    fn start_epoch(
        mut self,
        capture: &mut NativeCaptureReader,
        transport: &mut ApplicationSourceTransport,
        input: &mut [f32],
        health: &super::CaptureProbeStats,
    ) -> Option<Self> {
        if !transport.reset(self.format, self.native_rate) {
            return None;
        }
        // Discard only the prefix present at the discontinuity, not an unbounded
        // drain while the capture callback continues producing new frames.
        let mut remaining = health.available_frames;
        while remaining > 0 {
            let frames = capture.read_interleaved(input);
            if frames == 0 {
                break;
            }
            remaining = remaining.saturating_sub(frames);
        }
        self.epoch = transport.clock()?.epoch;
        self.native_generation = health.format_generation;
        self.native_discontinuities = health.clock_discontinuities;
        self.bridge.reset(self.epoch, Discontinuity::FormatChanged);
        Some(self)
    }

    fn transfer(
        &mut self,
        capture: &mut NativeCaptureReader,
        input: &mut [f32],
        output: &mut [f32],
    ) -> Result<usize, String> {
        let channels = usize::from(capture.session().resolved.stream.channels);
        let frames = if self.queued_input < 256 {
            capture.read_interleaved(input)
        } else {
            0
        };
        if frames > 0 {
            self.bridge
                .push_interleaved(&input[..frames * channels])
                .map_err(|e| e.to_string())?;
        }
        let report = self
            .bridge
            .pull_interleaved(output)
            .map_err(|e| e.to_string())?;
        self.queued_input = report.queued_input_frames;
        Ok(report.transferred_frames)
    }
}

pub(super) fn run_with_handoff(
    mut capture: NativeCaptureReader,
    mut transport: ApplicationSourceTransport,
    channels: usize,
    control: super::PumpControl<'_>,
) -> Result<(), String> {
    let mut input = vec![0.0; ENGINE_BLOCK_FRAMES * channels];
    let mut output = vec![0.0; ENGINE_BLOCK_FRAMES * channels];
    let mut state: Option<PumpClock> = None;
    let mut last_refresh = Instant::now();
    let mut format_pending = false;
    let mut scoped = super::ActivePlayback(None);
    while !control.stop.load(Ordering::Acquire) {
        if let Some(replacement) = control.playback.take() {
            scoped = super::ActivePlayback(replacement);
        }
        if let Some(replacement) = control.capture.take() {
            // This pump is the only ring reader. The old tap is released on
            // this non-real-time worker, never on its Core Audio callback.
            capture = replacement;
            state = None;
            format_pending = false;
            last_refresh = Instant::now();
        }
        if last_refresh.elapsed() >= Duration::from_millis(100) {
            if let Some(playback) = &scoped.0 {
                // A changed exact process/device lease revokes suppression;
                // ordinary shared capture continues, without reacquiring mute.
                if let Err(error) = playback.refresh() {
                    playback.stop()?;
                    tracing::warn!(%error, "scoped playback lease revoked; capture preserved");
                    scoped = super::ActivePlayback(None);
                }
            }
            format_pending = !refresh_capture(&mut capture)?;
            last_refresh = Instant::now();
        }
        if format_pending {
            transport.publish_native(0, false);
            if let Some(previous) = state.take() {
                transport.reset(previous.format, previous.native_rate);
            }
            std::thread::sleep(PUMP_IDLE_INTERVAL);
            continue;
        }
        let Some(clock) = transport.clock() else {
            std::thread::sleep(PUMP_IDLE_INTERVAL);
            continue;
        };
        if clock.format_word == 0 {
            std::thread::sleep(PUMP_IDLE_INTERVAL);
            continue;
        }
        let health = capture.session().stats();
        // Ring acceptance is not the native clock: overflowed callback frames
        // still elapsed. A drop also forces the epoch reset below.
        transport.publish_native(
            health.captured_frames.saturating_add(health.dropped_frames),
            health.running,
        );
        let rate = native_rate(health.sample_rate)?;
        if !health.running {
            return Err("native process tap stopped".to_owned());
        }
        if state
            .as_ref()
            .is_none_or(|s| s.needs_reset(clock, &health, rate))
        {
            let fresh = PumpClock::new(channels, rate, clock, health.dropped_frames)?;
            state = fresh.start_epoch(&mut capture, &mut transport, &mut input, &health);
            continue;
        }
        let Some(state) = state.as_mut() else {
            continue;
        };
        if state.observe(clock)? >= CAPTURE_RING_CAPACITY_FRAMES - ENGINE_BLOCK_FRAMES {
            std::thread::sleep(PUMP_IDLE_INTERVAL);
            continue;
        }
        let transferred = state.transfer(&mut capture, &mut input, &mut output)?;
        if transferred > 0 && !transport.write(&output[..transferred * channels], transferred) {
            // A format change races the worker safely: no old-rate PCM is accepted;
            // next tick observes the new token and reconstructs off the RT thread.
            continue;
        }
        if transferred == 0 {
            std::thread::sleep(PUMP_IDLE_INTERVAL);
        }
    }
    Ok(())
}

fn refresh_capture(capture: &mut NativeCaptureReader) -> Result<bool, String> {
    // SAFETY: sole format-control/ring-read owner. RESOURCE_BUSY retains the
    // native context and requires retry, not destruction or silent raw playback.
    let error = unsafe { super::mr_process_tap_refresh_format(capture.session().native.as_ptr()) };
    match error.code {
        super::NATIVE_OK => Ok(true),
        super::NATIVE_RESOURCE_BUSY => Ok(false),
        _ => Err(format!(
            "application format refresh failed: {} ({})",
            error.code, error.os_status
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timestamp_break_resets_even_without_a_rate_change_or_ring_overflow() {
        let snapshot = NativeClockSnapshot {
            format_word: (1_u64 << 32) | 0xbb80,
            ..NativeClockSnapshot::default()
        };
        let clock = PumpClock::new(2, 48_000, snapshot, 0).unwrap();
        let mut health = super::super::CaptureProbeStats {
            callback_count: 1,
            captured_frames: 128,
            dropped_frames: 0,
            available_frames: 128,
            channels: 2,
            sample_rate: 48_000.0,
            running: true,
            format_generation: 0,
            clock_discontinuities: 0,
            sample_time: 0.0,
            host_time: 100,
            clock_valid: true,
        };
        assert!(!clock.needs_reset(snapshot, &health, 48_000));
        health.host_time = 100_000; // observation time is not a rate-change signal
        assert!(!clock.needs_reset(snapshot, &health, 48_000));
        health.clock_discontinuities = 1;
        assert!(clock.needs_reset(snapshot, &health, 48_000));
    }
    #[test]
    fn native_rates_are_validated_without_assuming_48k() {
        assert_eq!(native_rate(44_100.0).unwrap(), 44_100);
        assert_eq!(native_rate(96_000.0).unwrap(), 96_000);
        for rate in [f64::NAN, f64::INFINITY, -1.0, 0.0, 48_000.5, 384_000.0] {
            assert!(native_rate(rate).is_err());
        }
    }

    #[test]
    fn only_engine_consumption_drives_feedback_and_master_never_corrects_itself() {
        for master in [0, 1] {
            let mut snapshot = NativeClockSnapshot {
                format_word: (1_u64 << 32) | 0xbb80,
                produced_frames: 1_024,
                consumer_running: 1,
                clock_master: master,
                ..NativeClockSnapshot::default()
            };
            let mut clock = PumpClock::new(2, 48_000, snapshot, 0).unwrap();
            for _ in 0..1_000 {
                clock.observe(snapshot).unwrap();
            }
            assert!(
                clock.bridge.correction_ppm().abs() < f64::EPSILON,
                "worker ticks are not clock progress"
            );
            for _ in 0..1_000 {
                snapshot.consumer_ticks += 128;
                clock.observe(snapshot).unwrap();
            }
            if master == 0 {
                assert!(clock.bridge.correction_ppm() < 0.0);
            } else {
                assert!(clock.bridge.correction_ppm().abs() < f64::EPSILON);
            }
        }
    }

    #[test]
    fn source_converter_transfers_rate_scaled_prefix_only() {
        for (native, graph) in [(44_100_u32, 48_000_u32), (96_000, 44_100), (48_000, 48_000)] {
            let snapshot = NativeClockSnapshot {
                format_word: (1_u64 << 32) | u64::from(graph),
                ..NativeClockSnapshot::default()
            };
            let mut clock = PumpClock::new(2, native, snapshot, 0).unwrap();
            let input = [0.25_f32; 256];
            let mut output = [0.0; 256];
            let mut valid = 0_usize;
            for _ in 0..1_000 {
                clock.bridge.push_interleaved(&input).unwrap();
                loop {
                    let report = clock.bridge.pull_interleaved(&mut output).unwrap();
                    valid += report.transferred_frames;
                    assert!(
                        output[report.transferred_frames * 2..]
                            .iter()
                            .all(|sample| *sample == 0.0)
                    );
                    if report.transferred_frames == 0 {
                        break;
                    }
                }
            }
            let expected = 128_000_u64 * u64::from(graph) / u64::from(native);
            let valid = u64::try_from(valid).unwrap();
            assert!(
                valid <= expected && expected - valid < 512,
                "{native} → {graph}: {valid}/{expected}"
            );
            println!(
                "app SRC {native} → {graph}: raw baseline 128000 frames, converted {valid}, ideal {expected} (unflushed sinc tail)"
            );
        }
    }
}
