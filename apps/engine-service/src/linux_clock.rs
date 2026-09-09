//! `PipeWire` adapters deliver the selected PCM rate even when the graph clock uses
//! another rate. Convert callback duration, not samples: doing SRC here again
//! would resample data that `PipeWire` already converted at the stream boundary.
use super::{ENGINE_SAMPLE_RATE, native::NativeCycle};
use tonenari_audio_io::process::{ClockDomain, ProcessCycle};
use tonenari_audio_io::session::ClockAuthority;

pub(super) struct GraphTimeline(tonenari_audio_io::frame_budget::FrameBudgetMapper);

impl Default for GraphTimeline {
    fn default() -> Self {
        Self(
            tonenari_audio_io::frame_budget::FrameBudgetMapper::new(ENGINE_SAMPLE_RATE)
                .expect("nonzero prepared engine rate"),
        )
    }
}

impl GraphTimeline {
    pub(super) fn new(rate: u32) -> Self {
        Self(
            tonenari_audio_io::frame_budget::FrameBudgetMapper::new(rate)
                .expect("validated graph rate"),
        )
    }
    pub(super) fn map(&mut self, native: &NativeCycle) -> Result<ProcessCycle, ()> {
        // Bound callback work, reject malformed clocks before touching state.
        if !(8_000..=384_000).contains(&native.rate) || native.frames == 0 || native.frames > 32_768
        {
            return Err(());
        }
        self.0
            .map(ProcessCycle {
                clock: ClockDomain {
                    authority: ClockAuthority::AudioServer,
                    id: u64::from(native.clock_id),
                    epoch: 1,
                },
                sample_rate: native.rate,
                frames: native.frames,
                first_frame: native.position,
                monotonic_ns: Some(native.monotonic_ns),
                discontinuity: false,
            })
            .map_err(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_mapping_preserves_exact_duration_with_variable_quantums() {
        for rate in [44_100, 48_000, 96_000] {
            let mut timeline = GraphTimeline::default();
            let mut input = 0;
            let mut output = 0;
            for quantum in [128, 256, 1024, 127].into_iter().cycle().take(10_000) {
                let cycle = timeline
                    .map(&NativeCycle {
                        position: input,
                        monotonic_ns: 0,
                        clock_id: 1,
                        rate,
                        frames: quantum,
                    })
                    .unwrap();
                assert_eq!(cycle.first_frame, output);
                assert_eq!(cycle.discontinuity, input == 0);
                input += u64::from(quantum);
                output += u64::from(cycle.frames);
            }
            assert_eq!(
                output,
                input * u64::from(ENGINE_SAMPLE_RATE) / u64::from(rate)
            );
        }
    }

    #[test]
    fn clock_change_reanchors_remainder_and_invalid_callback_preserves_state() {
        let mut timeline = GraphTimeline::default();
        let native = NativeCycle {
            position: 0,
            monotonic_ns: 0,
            clock_id: 1,
            rate: 44_100,
            frames: 128,
        };
        let first = timeline.map(&native).unwrap();
        assert!(timeline.map(&NativeCycle { rate: 0, ..native }).is_err());
        let changed = timeline
            .map(&NativeCycle {
                clock_id: 2,
                ..native
            })
            .unwrap();
        assert_eq!(first.frames, changed.frames);
        assert!(changed.discontinuity);
        assert_eq!(changed.clock.epoch, first.clock.epoch + 1);
        assert_eq!(changed.first_frame, u64::from(first.frames));
    }
}
