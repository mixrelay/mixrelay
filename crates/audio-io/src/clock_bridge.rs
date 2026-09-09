//! Owned streaming sample-rate and independent-clock boundary.
//!
//! Construct off the audio thread. Push and pull on one owning worker; transport
//! across threads with the adapter's bounded FIFO. No mutex, allocation, sleep or
//! native call occurs here after construction. Equal nominal rates are NOT clock
//! synchronization. A caller may bypass sinc only with explicit same-clock proof.
//!
//! Feedback is supplied by the owner from the actual clock-crossing FIFO, NOT
//! from a greedily drained private staging queue. There is exactly one feedback
//! authority per bridge; no amplitude/silence-based clock inference is performed.

use audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Adjustable, Async, FixedAsync, Resampler, SincInterpolationParameters};
use thiserror::Error;

mod controller;
mod fifo;
use controller::DriftController;
pub use controller::{ClockFeedback, FeedbackQueue};
use fifo::FrameFifo;

const CHUNK: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockRelation {
    Independent,
    /// Caller has verified one actual clock domain, not just matching Hz.
    ProvenSameClock,
}

#[derive(Clone, Copy, Debug)]
pub struct BridgeConfig {
    pub channels: usize,
    pub input_rate: u32,
    pub output_rate: u32,
    pub max_input_frames: usize,
    pub max_output_frames: usize,
    /// Capacity in source/input frames; includes room for native callback bursts.
    pub capacity_frames: usize,
    /// Input frames to accumulate after construction/reset before processing.
    pub priming_frames: usize,
    pub clock_relation: ClockRelation,
    /// Which clock-crossing FIFO is authoritative. Never mix two controllers.
    pub feedback_queue: FeedbackQueue,
    /// Hard correction limit, 1..=2000 ppm. Defaults are deliberately caller-owned.
    pub max_correction_ppm: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum BridgeError {
    #[error("invalid sample-rate bridge configuration")]
    InvalidConfig,
    #[error("interleaved buffer shape or sample value is invalid")]
    InvalidBuffer,
    #[error("bounded sample-rate bridge input FIFO overflow")]
    Overflow,
    #[error("clock feedback is invalid or belongs to another epoch/authority")]
    InvalidFeedback,
    #[error("sample-rate converter failed")]
    Resampler,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Discontinuity {
    StreamRestart,
    FormatChanged,
    TransportLoss,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransferState {
    Priming,
    Running,
    Underflow,
}

#[derive(Clone, Copy, Debug)]
pub struct PullReport {
    /// Only these prefix frames are valid transferred PCM. Remaining output is zero.
    pub transferred_frames: usize,
    /// Real input frames removed from the staging FIFO during this pull.
    pub consumed_input_frames: usize,
    pub state: TransferState,
    pub epoch: u64,
    pub queued_input_frames: usize,
    pub queued_output_frames: usize,
    /// Sinc algorithmic delay in output frames; excludes external transport.
    pub algorithmic_delay_frames: usize,
    pub correction_ppm: f64,
}

/// One worker owns both directions of this object. It is not an SPSC queue.
pub struct AsyncClockBridge {
    config: BridgeConfig,
    resampler: Option<Async<f32>>,
    input: FrameFifo,
    input_scratch: Vec<f32>,
    output_scratch: Vec<f32>,
    output_read: usize,
    output_frames: usize,
    controller: DriftController,
    primed: bool,
    started: bool,
    epoch: u64,
}

impl AsyncClockBridge {
    /// Allocates all storage and constructs the sinc filter off the audio thread.
    /// # Errors
    /// Rejects unbounded/invalid configuration or converter construction failure.
    pub fn new(config: BridgeConfig) -> Result<Self, BridgeError> {
        validate_config(config)?;
        let bypass = config.clock_relation == ClockRelation::ProvenSameClock
            && config.input_rate == config.output_rate;
        let resampler = if bypass {
            None
        } else {
            Some(
                Async::new_sinc(
                    f64::from(config.output_rate) / f64::from(config.input_rate),
                    1.003,
                    &SincInterpolationParameters::default(),
                    CHUNK,
                    config.channels,
                    FixedAsync::Input,
                )
                .map_err(|_| BridgeError::Resampler)?,
            )
        };
        let output_max = resampler
            .as_ref()
            .map_or(CHUNK, Resampler::output_frames_max);
        Ok(Self {
            input: FrameFifo::new(config.capacity_frames, config.channels),
            input_scratch: vec![0.0; CHUNK * config.channels],
            output_scratch: vec![0.0; output_max * config.channels],
            output_read: 0,
            output_frames: 0,
            controller: DriftController::new(config.max_correction_ppm),
            primed: false,
            started: false,
            epoch: 0,
            config,
            resampler,
        })
    }

    /// Queues complete interleaved frames, accepting all or none.
    /// # Errors
    /// Malformed/nonfinite input and overflow leave queued PCM unchanged. The
    /// adapter must explicitly reset after dropping a native packet.
    pub fn push_interleaved(&mut self, samples: &[f32]) -> Result<usize, BridgeError> {
        let frames = validate_buffer(
            samples.len(),
            self.config.channels,
            self.config.max_input_frames,
        )?;
        if samples.iter().any(|sample| !sample.is_finite()) {
            return Err(BridgeError::InvalidBuffer);
        }
        if frames > self.input.available() {
            return Err(BridgeError::Overflow);
        }
        self.input.push(samples);
        Ok(frames)
    }

    /// Pulls available converted PCM; never fabricates valid frames from silence.
    /// A worker pumping a producer MUST enqueue only `transferred_frames`.
    /// Shortage retains filter history for arbitrary quantum adaptation. Native
    /// discontinuities/stalls require an explicit `reset`, not hidden padding.
    /// # Errors
    /// Invalid output shape/quantum or a converter error is reported explicitly.
    pub fn pull_interleaved(&mut self, output: &mut [f32]) -> Result<PullReport, BridgeError> {
        let wanted = validate_buffer(
            output.len(),
            self.config.channels,
            self.config.max_output_frames,
        )?;
        output.fill(0.0);
        let input_before = self.input.frames();
        let mut transferred = 0;
        if !self.primed && self.input.frames() >= self.config.priming_frames {
            self.primed = true;
        }
        while self.primed && transferred < wanted {
            if self.output_read == self.output_frames && !self.convert_chunk()? {
                break;
            }
            let count = (wanted - transferred).min(self.output_frames - self.output_read);
            let channels = self.config.channels;
            output[transferred * channels..(transferred + count) * channels].copy_from_slice(
                &self.output_scratch
                    [self.output_read * channels..(self.output_read + count) * channels],
            );
            self.output_read += count;
            transferred += count;
        }
        let mut report = self.report(transferred, wanted);
        report.consumed_input_frames = input_before - self.input.frames();
        Ok(report)
    }

    /// Feeds exactly one actual clock-crossing FIFO observation to the drift PI.
    /// # Errors
    /// Wrong authority/epoch, invalid duration or queue geometry is rejected.
    pub fn observe_clock(&mut self, feedback: ClockFeedback) -> Result<f64, BridgeError> {
        if feedback.epoch != self.epoch || feedback.queue != self.config.feedback_queue {
            return Err(BridgeError::InvalidFeedback);
        }
        let rate = match feedback.queue {
            FeedbackQueue::Input => self.config.input_rate,
            FeedbackQueue::Output => self.config.output_rate,
        };
        let correction = self.controller.observe(feedback, rate)?;
        if self.config.clock_relation == ClockRelation::Independent
            && let Some(resampler) = &mut self.resampler
        {
            resampler
                .set_resample_ratio_relative(1.0 + correction * 1e-6, true)
                .map_err(|_| BridgeError::Resampler)?;
        }
        Ok(self.correction_ppm())
    }

    /// Clears queued PCM, filter tail and feedback integral on an explicit epoch
    /// boundary. Caller also flushes its external FIFO, otherwise old PCM remains.
    pub fn reset(&mut self, epoch: u64, _reason: Discontinuity) {
        self.epoch = epoch;
        self.input.clear();
        self.output_read = 0;
        self.output_frames = 0;
        self.primed = false;
        self.started = false;
        self.controller.reset();
        if let Some(resampler) = &mut self.resampler {
            resampler.reset();
        }
    }

    #[must_use]
    pub fn queued_input_frames(&self) -> usize {
        self.input.frames()
    }

    #[must_use]
    pub const fn queued_output_frames(&self) -> usize {
        self.output_frames - self.output_read
    }

    /// Private FIFO backlog in input-rate frames, including converted frames not
    /// yet pulled. Add this to an upstream FIFO observation after greedy moves.
    /// Filter history/algorithmic delay is not queued transport and is excluded.
    #[must_use]
    #[expect(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "bounded buffers and validated positive rate ratio"
    )]
    pub fn buffered_input_frames(&self) -> usize {
        let ratio = f64::from(self.config.output_rate) / f64::from(self.config.input_rate)
            * (1.0 + self.correction_ppm() * 1e-6);
        self.input.frames() + (self.queued_output_frames() as f64 / ratio).ceil() as usize
    }

    #[must_use]
    pub fn correction_ppm(&self) -> f64 {
        if self.config.clock_relation == ClockRelation::Independent {
            self.controller.correction()
        } else {
            0.0
        }
    }

    fn convert_chunk(&mut self) -> Result<bool, BridgeError> {
        let needed = self
            .resampler
            .as_ref()
            .map_or(1, Resampler::input_frames_next);
        if self.input.frames() < needed {
            return Ok(false);
        }
        if let Some(resampler) = &mut self.resampler {
            self.input
                .peek(&mut self.input_scratch[..needed * self.config.channels]);
            let input = InterleavedSlice::new(&self.input_scratch, self.config.channels, needed)
                .map_err(|_| BridgeError::Resampler)?;
            let capacity = self.output_scratch.len() / self.config.channels;
            let mut output =
                InterleavedSlice::new_mut(&mut self.output_scratch, self.config.channels, capacity)
                    .map_err(|_| BridgeError::Resampler)?;
            let (consumed, produced) = resampler
                .process_into_buffer(&input, &mut output, None)
                .map_err(|_| BridgeError::Resampler)?;
            if consumed == 0 && produced == 0 {
                return Err(BridgeError::Resampler);
            }
            self.input.consume(consumed);
            self.output_frames = produced;
        } else {
            let frames = self.input.frames().min(CHUNK);
            self.input
                .peek(&mut self.output_scratch[..frames * self.config.channels]);
            self.input.consume(frames);
            self.output_frames = frames;
        }
        self.output_read = 0;
        self.started |= self.output_frames != 0;
        // Startup can consume input without producing a frame. Continue while
        // input remains: each iteration advances the bounded input FIFO, so this
        // cannot spin and does not misreport queued startup PCM as starvation.
        Ok(true)
    }

    fn report(&self, transferred: usize, wanted: usize) -> PullReport {
        PullReport {
            transferred_frames: transferred,
            consumed_input_frames: 0,
            state: if !self.primed || !self.started {
                TransferState::Priming
            } else if transferred < wanted {
                TransferState::Underflow
            } else {
                TransferState::Running
            },
            epoch: self.epoch,
            queued_input_frames: self.input.frames(),
            queued_output_frames: self.output_frames - self.output_read,
            algorithmic_delay_frames: self.resampler.as_ref().map_or(0, Resampler::output_delay),
            correction_ppm: self.correction_ppm(),
        }
    }
}

fn validate_buffer(samples: usize, channels: usize, maximum: usize) -> Result<usize, BridgeError> {
    if !samples.is_multiple_of(channels) || samples / channels > maximum {
        return Err(BridgeError::InvalidBuffer);
    }
    Ok(samples / channels)
}

fn validate_config(c: BridgeConfig) -> Result<(), BridgeError> {
    if !(1..=64).contains(&c.channels)
        || !(8_000..=384_000).contains(&c.input_rate)
        || !(8_000..=384_000).contains(&c.output_rate)
        || c.max_input_frames == 0
        || c.max_output_frames == 0
        || c.max_input_frames > 65_536
        || c.max_output_frames > 65_536
        || c.capacity_frames < CHUNK.max(c.max_input_frames)
        || c.capacity_frames > 262_144
        || c.priming_frames > c.capacity_frames
        || !c.max_correction_ppm.is_finite()
        || !(1.0..=2_000.0).contains(&c.max_correction_ppm)
    {
        return Err(BridgeError::InvalidConfig);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
