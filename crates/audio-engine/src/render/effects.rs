//! Prepared, bounded, MixRelay-authored DSP. Allocation and destruction belong to control.
mod filters;
use super::{PortSlot, RenderFormat};
use filters::{Shape, coefficients};
use mixrelay_audio_graph::{Effect, EffectConfig, NodeId};

pub(super) const MAX_EFFECT_BYTES: usize = 128 * 1024 * 1024;
// MixRelay tuning, not copied from Freeverb's tuning.h. Eight lowpass feedback
// combs followed by four genuine Schroeder allpasses; see spatial-effect design notes.
const REVERB_DELAYS: [f32; 12] = [
    0.0297, 0.0371, 0.0411, 0.0437, 0.0313, 0.0337, 0.0397, 0.0473, 0.005, 0.0017, 0.0061, 0.0031,
];
const REVERB_LINES: usize = 13;

#[derive(Debug)]
pub(super) struct Binding {
    pub(super) node_id: NodeId,
    pub(super) design: Design,
}

#[derive(Debug, Clone)]
pub(super) struct Design {
    config: EffectConfig,
    target: [f32; 32],
    filter_target: [f64; 15],
    filters: usize,
    line_frames: Vec<usize>,
    smoothing: f32,
}

#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)] // Validated rate and bounded delay calculations.
impl Design {
    #[allow(clippy::too_many_lines)] // One bounded, exhaustive preparation table for the effect contract.
    pub(super) fn new(config: &EffectConfig, format: RenderFormat) -> Result<Self, String> {
        config.validate()?;
        let rate = format.sample_rate as f32;
        if format.sample_rate == 0 {
            return Err("effect sample rate must be positive".into());
        }
        let mut design = Self {
            config: config.clone(),
            target: [0.0; 32],
            filter_target: [0.0; 15],
            filters: 0,
            line_frames: vec![],
            smoothing: 1.0 - (-1.0 / (rate * 0.005)).exp(),
        };
        let t = &mut design.target;
        match &config.effect {
            Effect::Equalizer(p) => {
                design.filters = 3;
                for (index, (shape, hz, q, db)) in [
                    (Shape::Peak, p.low_frequency_hz, p.low_q, p.low_db),
                    (Shape::Peak, p.frequency_hz, p.q, p.mid_db),
                    (Shape::Peak, p.high_frequency_hz, p.high_q, p.high_db),
                ]
                .into_iter()
                .enumerate()
                {
                    design.filter_target[index * 5..index * 5 + 5].copy_from_slice(&coefficients(
                        shape,
                        hz,
                        q,
                        db,
                        format.sample_rate,
                    ));
                }
            }
            Effect::HighPass(p) | Effect::LowPass(p) => {
                design.filters = 1;
                let shape = if matches!(config.effect, Effect::HighPass(_)) {
                    Shape::HighPass
                } else {
                    Shape::LowPass
                };
                design.filter_target[..5].copy_from_slice(&coefficients(
                    shape,
                    p.frequency_hz,
                    p.q,
                    0.0,
                    format.sample_rate,
                ));
            }
            Effect::Gate(p) | Effect::Expander(p) => {
                t[0] = db(p.threshold_db);
                t[1] = p.ratio - 1.0;
                t[2] = smoothing(p.attack_ms, rate);
                t[3] = smoothing(p.release_ms, rate);
                t[4] = db(-p.range_db);
            }
            Effect::ChannelUtility(_) => {}
            Effect::Pan(p) => {
                let angle = (p.position + 1.0) * std::f32::consts::FRAC_PI_4;
                t[0] = angle.cos();
                t[1] = angle.sin();
            }
            Effect::Balance(p) => {
                t[0] = 1.0 - p.position.max(0.0);
                t[1] = 1.0 + p.position.min(0.0);
            }
            Effect::Delay(p) => {
                t[0] = p.time_ms * rate / 1_000.0;
                t[1] = p.feedback;
                t[2] = p.mix;
                t[3] = f32::from(p.mode);
                design.line_frames =
                    vec![(2.0 * rate).ceil() as usize + 2; usize::from(config.channels)];
            }
            Effect::Ducking(p) => {
                t[0] = db(p.threshold_db);
                t[1] = db(-p.reduction_db);
                t[2] = smoothing(p.attack_ms, rate);
                t[3] = smoothing(p.release_ms, rate);
            }
            Effect::Reverb(p) => {
                t[8] = p.damping;
                t[9] = p.mix;
                t[10] = p.pre_delay_ms * rate / 1_000.0;
                t[11] = f32::from(p.algorithm);
                t[24] = (0.0007 * rate * p.room_size).round();
                for (index, seconds) in REVERB_DELAYS.into_iter().enumerate() {
                    let duration = seconds * p.room_size;
                    t[12 + index] = (duration * rate).round().max(1.0);
                    if index < 8 {
                        t[index] = (-6.907_755 * duration / p.decay_seconds).exp();
                    }
                }
                for channel in 0..config.channels {
                    // Reserve maximum room size and pre-delay once, so preset/size
                    // changes can reuse the existing state without callback allocation.
                    design.line_frames.push((0.2 * rate).ceil() as usize + 2);
                    for seconds in REVERB_DELAYS {
                        let duration = seconds + f32::from(channel % 2) * 0.0007;
                        design
                            .line_frames
                            .push((duration * 2.0 * rate).ceil() as usize + 2);
                    }
                }
            }
            Effect::Chorus(p) => {
                t[0] = p.rate_hz / rate;
                t[1] = p.depth_ms * rate / 1_000.0;
                t[2] = p.mix;
                t[3] = 0.015 * rate;
                design.line_frames =
                    vec![(0.05 * rate).ceil() as usize + 2; usize::from(config.channels)];
            }
        }
        t[31] = if config.bypass { 0.0 } else { 1.0 };
        if design.storage_bytes()? > MAX_EFFECT_BYTES {
            return Err("built-in effect memory capacity exceeded".into());
        }
        Ok(design)
    }

    pub(super) fn storage_bytes(&self) -> Result<usize, String> {
        self.line_frames.iter().try_fold(0_usize, |sum, frames| {
            frames
                .checked_mul(size_of::<f32>())
                .and_then(|bytes| sum.checked_add(bytes))
                .ok_or_else(|| "effect buffer size overflow".into())
        })
    }

    pub(super) fn compatible(&self, other: &Self) -> bool {
        self.config.channels == other.config.channels
            && self.line_frames == other.line_frames
            && std::mem::discriminant(&self.config.effect)
                == std::mem::discriminant(&other.config.effect)
    }
}

#[derive(Debug)]
struct Line {
    samples: Box<[f32]>,
    cursor: usize,
    valid: usize,
    damped: f32,
}
impl Line {
    fn new(frames: usize) -> Result<Self, String> {
        let mut samples = Vec::new();
        samples
            .try_reserve_exact(frames)
            .map_err(|e| e.to_string())?;
        samples.resize(frames, 0.0);
        Ok(Self {
            samples: samples.into_boxed_slice(),
            cursor: 0,
            valid: 0,
            damped: 0.0,
        })
    }
    fn push(&mut self, value: f32) {
        self.samples[self.cursor] = history_sample(value);
        self.cursor = (self.cursor + 1) % self.samples.len();
        self.valid = (self.valid + 1).min(self.samples.len());
    }
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )] // Delay clamped to a prepared line's capacity.
    fn read(&self, delay: f32) -> f32 {
        let delay = delay.clamp(0.0, (self.samples.len() - 2) as f32);
        let whole = delay.floor() as usize;
        let fraction = delay - whole as f32;
        let at = |age: usize| {
            if age < self.valid {
                self.samples[(self.cursor + self.samples.len() - 1 - age) % self.samples.len()]
            } else {
                0.0
            }
        };
        at(whole).mul_add(1.0 - fraction, at(whole + 1) * fraction)
    }
    fn reset(&mut self) {
        self.cursor = 0;
        self.valid = 0;
        self.damped = 0.0;
    }
}

#[derive(Debug)]
pub(super) struct State {
    current: [f32; 32],
    filter_current: [f64; 15],
    histories: Box<[[f64; 2]]>,
    lines: Vec<Line>,
    envelope: f32,
    gain: f32,
    phase: f32,
    suspended: bool,
    mode: u8,
    previous_mode: u8,
    mode_mix: f32,
}
#[allow(clippy::too_many_arguments)] // Explicit prepared port geometry and arena; no runtime wrapper allocation.
impl State {
    pub(super) fn new(design: &Design) -> Result<Self, String> {
        let lines = design
            .line_frames
            .iter()
            .map(|&frames| Line::new(frames))
            .collect::<Result<_, _>>()?;
        let mode = if let Effect::ChannelUtility(p) = &design.config.effect {
            p.mode
        } else {
            0
        };
        Ok(Self {
            current: design.target,
            filter_current: design.filter_target,
            histories: vec![[0.0; 2]; design.filters * usize::from(design.config.channels)]
                .into_boxed_slice(),
            lines,
            envelope: 0.0,
            gain: 1.0,
            phase: 0.0,
            suspended: false,
            mode,
            previous_mode: mode,
            mode_mix: 1.0,
        })
    }
    pub(super) fn suspend(&mut self) {
        if self.suspended {
            return;
        }
        self.histories.fill([0.0; 2]);
        for line in &mut self.lines {
            line.reset();
        }
        self.envelope = 0.0;
        self.gain = 1.0;
        self.suspended = true;
    }
    pub(super) fn process(
        &mut self,
        design: &Design,
        arena: &mut [f32],
        input: PortSlot,
        output: PortSlot,
        sidechain: Option<PortSlot>,
        frames: usize,
    ) {
        self.suspended = false;
        let channels = usize::from(input.channels);
        let outputs = usize::from(output.channels);
        if let Effect::ChannelUtility(p) = &design.config.effect
            && p.mode != self.mode
        {
            self.previous_mode = self.mode;
            self.mode = p.mode;
            self.mode_mix = 0.0;
        }
        for frame in 0..frames {
            for (current, target) in self.current.iter_mut().zip(design.target) {
                let next = *current + design.smoothing * (target - *current);
                *current = if next.to_bits() == current.to_bits() || (next - target).abs() < 1e-7 {
                    target
                } else {
                    next
                };
            }
            for (current, target) in self.filter_current.iter_mut().zip(design.filter_target) {
                let next = *current + f64::from(design.smoothing) * (target - *current);
                *current = if next.to_bits() == current.to_bits() {
                    target
                } else {
                    next
                };
            }
            let start = input.offset + frame * channels;
            let end = output.offset + frame * outputs;
            self.update_gain(
                design,
                arena,
                start,
                channels,
                sidechain.map(|port| arena[port.offset + frame]),
            );
            self.mode_mix = (self.mode_mix + design.smoothing).min(1.0);
            if matches!(design.config.effect, Effect::Delay(_)) {
                self.delay_frame(arena, start, end, outputs);
                continue;
            }
            for channel in 0..outputs {
                let source = finite(arena[start + channel.min(channels - 1)]);
                let processed = self.sample(design, arena, start, channel, source);
                arena[end + channel] = if self.current[31] == 0.0 {
                    source
                } else {
                    finite(source + self.current[31] * (processed - source))
                };
            }
            if matches!(design.config.effect, Effect::Chorus(_)) {
                self.phase = (self.phase + self.current[0]).fract();
            }
        }
    }

    fn delay_frame(&mut self, arena: &mut [f32], start: usize, end: usize, channels: usize) {
        let c = &self.current;
        let mut echoes = [0.0_f32; 64];
        for (channel, echo) in echoes[..channels].iter_mut().enumerate() {
            *echo = self.lines[channel].read((c[0] - 1.0).max(0.0));
        }
        for channel in 0..channels {
            let source = finite(arena[start + channel]);
            let partner = channel ^ 1;
            let cross = if partner < channels { c[3] } else { 0.0 };
            let drive = if channel.is_multiple_of(2) && partner < channels {
                (source + finite(arena[start + partner])) * 0.5
            } else {
                0.0
            };
            let injected = source + cross * (drive - source);
            let feedback =
                echoes[channel] + cross * (echoes[partner.min(channels - 1)] - echoes[channel]);
            let line = &mut self.lines[channel];
            // Zero/sub-sample time never creates an instantaneous feedback loop.
            line.push(if c[0] >= 1.0 {
                injected + feedback * c[1]
            } else {
                source
            });
            let wet = line.read(c[0]);
            let processed = source + c[2] * (wet - source);
            arena[end + channel] = if c[31] == 0.0 {
                source
            } else {
                finite(source + c[31] * (processed - source))
            };
        }
    }

    fn update_gain(
        &mut self,
        design: &Design,
        arena: &[f32],
        start: usize,
        channels: usize,
        control: Option<f32>,
    ) {
        let c = &self.current;
        let target = match &design.config.effect {
            Effect::Gate(_) | Effect::Expander(_) => {
                let peak = arena[start..start + channels]
                    .iter()
                    .fold(0.0_f32, |p, x| p.max(finite(*x).abs()));
                let smoothing = if peak > self.envelope { c[2] } else { c[3] };
                self.envelope = smoothing.mul_add(self.envelope, (1.0 - smoothing) * peak);
                if self.envelope >= c[0] {
                    1.0
                } else if matches!(design.config.effect, Effect::Gate(_)) {
                    c[4]
                } else {
                    (self.envelope / c[0]).powf(c[1]).max(c[4])
                }
            }
            Effect::Ducking(_) => {
                if finite(control.unwrap_or(0.0)).abs() >= c[0] {
                    c[1]
                } else {
                    1.0
                }
            }
            _ => return,
        };
        // Gate opens quickly; ducking closes quickly. Both linked channels use one gain.
        let attack = matches!(design.config.effect, Effect::Ducking(_)) == (target < self.gain);
        let smoothing = if attack { c[2] } else { c[3] };
        self.gain = smoothing.mul_add(self.gain, (1.0 - smoothing) * target);
    }

    fn sample(
        &mut self,
        design: &Design,
        arena: &[f32],
        start: usize,
        channel: usize,
        source: f32,
    ) -> f32 {
        let c = &self.current;
        match &design.config.effect {
            Effect::Equalizer(_) | Effect::HighPass(_) | Effect::LowPass(_) => {
                let mut result = source;
                for stage in 0..design.filters {
                    result = filters::sample(
                        result,
                        &self.filter_current[stage * 5..stage * 5 + 5],
                        &mut self.histories[channel * design.filters + stage],
                    );
                }
                result
            }
            Effect::Gate(_) | Effect::Expander(_) | Effect::Ducking(_) => source * self.gain,
            Effect::Pan(_) | Effect::Balance(_) => source * c[channel % 2],
            Effect::ChannelUtility(_) => {
                let pair = start + channel / 2 * 2;
                let left = finite(arena[pair]);
                let right = finite(arena[pair + 1]);
                let old = channel_sample(self.previous_mode, channel, left, right);
                let new = channel_sample(self.mode, channel, left, right);
                old + self.mode_mix * (new - old)
            }
            Effect::Delay(_) => unreachable!("delay pairs are processed atomically per frame"),
            Effect::Chorus(_) => {
                let line = &mut self.lines[channel];
                line.push(source);
                #[allow(clippy::cast_precision_loss)]
                let phase = self.phase + (channel % 2) as f32 * 0.25;
                let modulation = (phase * std::f32::consts::TAU).sin();
                let wet = line.read(c[3] + c[1] * modulation);
                source + c[2] * (wet - source)
            }
            Effect::Reverb(_) => {
                let lines = &mut self.lines[channel * REVERB_LINES..(channel + 1) * REVERB_LINES];
                let partner = channel ^ 1;
                let excitation_source = if partner < usize::from(design.config.channels) {
                    (source + finite(arena[start + partner])) * 0.5
                } else {
                    source
                };
                lines[0].push(source + c[11] * (excitation_source - source));
                let excitation = lines[0].read(c[10]);
                #[allow(clippy::cast_precision_loss)]
                let spread = (channel % 2) as f32 * c[24];
                let mut classic = 0.0;
                let mut dense = 0.0;
                for (index, line) in lines[1..9].iter_mut().enumerate() {
                    let delayed = line.read(c[12 + index] + spread - 1.0);
                    line.damped = history_sample(delayed * (1.0 - c[8]) + line.damped * c[8]);
                    line.push(excitation + line.damped * c[index]);
                    if index < 4 {
                        classic += delayed * 0.25;
                    }
                    dense += delayed * 0.125;
                }
                let mut wet = classic + c[11] * (dense - classic);
                let mut classic_diffusion = 0.0;
                for (index, line) in lines[9..].iter_mut().enumerate() {
                    let delayed = line.read(c[20 + index] + spread - 1.0);
                    let result = delayed - 0.5 * wet;
                    line.push(wet + 0.5 * result);
                    wet = result;
                    if index == 1 {
                        classic_diffusion = wet;
                    }
                }
                wet = classic_diffusion + c[11] * (wet - classic_diffusion);
                source + c[9] * (wet - source)
            }
        }
    }
}

fn channel_sample(mode: u8, channel: usize, left: f32, right: f32) -> f32 {
    match mode {
        1 => (left + right) * 0.5,
        2 => left,
        3 => right,
        4 => {
            if channel.is_multiple_of(2) {
                right
            } else {
                left
            }
        }
        5 => {
            -if channel.is_multiple_of(2) {
                left
            } else {
                right
            }
        }
        _ => {
            if channel.is_multiple_of(2) {
                left
            } else {
                right
            }
        }
    }
}
fn db(value: f32) -> f32 {
    10.0_f32.powf(value / 20.0)
}
fn smoothing(ms: f32, rate: f32) -> f32 {
    (-1.0 / (ms * rate / 1_000.0)).exp()
}
fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

// Flush inaudible recursive history before denormals increase callback CPU cost.
// Dry/bypassed input retains its finite sample values unchanged.
fn history_sample(value: f32) -> f32 {
    if value.abs() < 1e-20 {
        0.0
    } else {
        finite(value)
    }
}
