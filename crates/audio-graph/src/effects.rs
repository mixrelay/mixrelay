//! MixRelay-authored built-in effect contracts. No third-party DSP runtime.
use serde::{Deserialize, Serialize};

pub const SIDECHAIN_PORT_ID: &str = "sidechain";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EffectConfig {
    pub version: u16,
    pub channels: u16,
    #[serde(default)]
    pub bypass: bool,
    pub effect: Effect,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "parameters",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Effect {
    Equalizer(EqualizerParameters),
    HighPass(FilterParameters),
    LowPass(FilterParameters),
    Gate(DynamicsParameters),
    Expander(DynamicsParameters),
    ChannelUtility(ChannelParameters),
    Pan(PositionParameters),
    Balance(PositionParameters),
    Delay(DelayParameters),
    Ducking(DuckingParameters),
    Reverb(ReverbParameters),
    Chorus(ChorusParameters),
}

macro_rules! parameters {
    ($name:ident { $($field:ident : $type:ty),+ $(,)? }) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $name { $(pub $field: $type),+ }
    };
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EqualizerParameters {
    pub low_db: f32,
    pub mid_db: f32,
    pub high_db: f32,
    // Keep the original middle-band keys for saved graph compatibility.
    pub frequency_hz: f32,
    pub q: f32,
    #[serde(default = "default_low_frequency")]
    pub low_frequency_hz: f32,
    #[serde(default = "default_band_q")]
    pub low_q: f32,
    #[serde(default = "default_high_frequency")]
    pub high_frequency_hz: f32,
    #[serde(default = "default_band_q")]
    pub high_q: f32,
}
const fn default_low_frequency() -> f32 {
    150.0
}
const fn default_high_frequency() -> f32 {
    6_000.0
}
const fn default_band_q() -> f32 {
    0.707
}
parameters!(FilterParameters {
    frequency_hz: f32,
    q: f32
});
parameters!(DynamicsParameters {
    threshold_db: f32,
    ratio: f32,
    attack_ms: f32,
    release_ms: f32,
    range_db: f32
});
// Pairs: 0=unchanged, 1=average mono, 2=left to both, 3=right to both, 4=swap, 5=invert polarity.
parameters!(ChannelParameters { mode: u8 });
parameters!(PositionParameters { position: f32 });
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DelayParameters {
    pub time_ms: f32,
    pub feedback: f32,
    pub mix: f32,
    /// 0: independent echoes; 1: alternating stereo-pair feedback (mono stays mono).
    #[serde(default)]
    pub mode: u8,
}
parameters!(DuckingParameters {
    threshold_db: f32,
    reduction_db: f32,
    attack_ms: f32,
    release_ms: f32
});
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReverbParameters {
    pub decay_seconds: f32,
    pub damping: f32,
    pub mix: f32,
    #[serde(default = "default_room_size")]
    pub room_size: f32,
    #[serde(default)]
    pub pre_delay_ms: f32,
    /// 0: classic four-comb/two-allpass; 1: dense eight-comb/four-allpass.
    #[serde(default)]
    pub algorithm: u8,
}
const fn default_room_size() -> f32 {
    1.0
}
parameters!(ChorusParameters {
    rate_hz: f32,
    depth_ms: f32,
    mix: f32
});

impl EffectConfig {
    #[must_use]
    pub const fn output_channels(&self) -> u16 {
        if matches!(self.effect, Effect::Pan(_)) {
            2
        } else {
            self.channels
        }
    }

    /// # Errors
    /// Rejects unsupported versions, channel geometry and non-finite/out-of-range parameters.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 || !(1..=64).contains(&self.channels) {
            return Err("effect version must be 1 and channels must be 1..64".into());
        }
        if matches!(self.effect, Effect::Pan(_)) && self.channels != 1 {
            return Err("pan requires one input channel and produces stereo".into());
        }
        if matches!(self.effect, Effect::Balance(_) | Effect::ChannelUtility(_))
            && !self.channels.is_multiple_of(2)
        {
            return Err("balance/channel utility requires complete stereo pairs".into());
        }
        match &self.effect {
            Effect::Equalizer(p) => {
                for value in [p.low_db, p.mid_db, p.high_db] {
                    range("EQ gain", value, -24.0, 24.0)?;
                }
                frequency(p.low_frequency_hz, p.low_q)?;
                frequency(p.frequency_hz, p.q)?;
                frequency(p.high_frequency_hz, p.high_q)
            }
            Effect::HighPass(p) | Effect::LowPass(p) => frequency(p.frequency_hz, p.q),
            Effect::Gate(p) | Effect::Expander(p) => {
                range("thresholdDb", p.threshold_db, -80.0, 0.0)?;
                range("ratio", p.ratio, 1.0, 20.0)?;
                range("rangeDb", p.range_db, 0.0, 80.0)?;
                times(p.attack_ms, p.release_ms)
            }
            Effect::ChannelUtility(p) => {
                if p.mode <= 5 {
                    Ok(())
                } else {
                    Err("channel mode must be 0..5".into())
                }
            }
            Effect::Pan(p) | Effect::Balance(p) => range("position", p.position, -1.0, 1.0),
            Effect::Delay(p) => {
                if p.mode > 1 {
                    return Err("delay mode must be 0 or 1".into());
                }
                range("timeMs", p.time_ms, 0.0, 2_000.0)?;
                range("feedback", p.feedback, 0.0, 0.9)?;
                range("mix", p.mix, 0.0, 1.0)
            }
            Effect::Ducking(p) => {
                range("thresholdDb", p.threshold_db, -80.0, 0.0)?;
                range("reductionDb", p.reduction_db, 0.0, 60.0)?;
                times(p.attack_ms, p.release_ms)
            }
            Effect::Reverb(p) => {
                if p.algorithm > 1 {
                    return Err("reverb algorithm must be 0 or 1".into());
                }
                range("roomSize", p.room_size, 0.5, 2.0)?;
                range("preDelayMs", p.pre_delay_ms, 0.0, 200.0)?;
                range("decaySeconds", p.decay_seconds, 0.1, 6.0)?;
                range("damping", p.damping, 0.0, 0.95)?;
                range("mix", p.mix, 0.0, 1.0)
            }
            Effect::Chorus(p) => {
                range("rateHz", p.rate_hz, 0.05, 5.0)?;
                range("depthMs", p.depth_ms, 0.0, 10.0)?;
                range("mix", p.mix, 0.0, 1.0)
            }
        }
    }
}

fn frequency(hz: f32, q: f32) -> Result<(), String> {
    range("frequencyHz", hz, 20.0, 20_000.0)?;
    range("Q", q, 0.1, 10.0)
}
fn times(attack: f32, release: f32) -> Result<(), String> {
    range("attackMs", attack, 0.1, 200.0)?;
    range("releaseMs", release, 5.0, 2_000.0)
}
fn range(name: &str, value: f32, low: f32, high: f32) -> Result<(), String> {
    if value.is_finite() && (low..=high).contains(&value) {
        Ok(())
    } else {
        Err(format!("{name} must be finite and within {low}..{high}"))
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // Exact serialized parameter/default assertions.
mod tests {
    use super::{Effect, EffectConfig};

    #[test]
    fn legacy_spatial_effects_restore_new_geometry_and_mode_defaults() {
        for (kind, parameters) in [
            ("delay", r#"{"timeMs":100,"feedback":0.3,"mix":0.2}"#),
            ("reverb", r#"{"decaySeconds":1.2,"damping":0.5,"mix":0.2}"#),
        ] {
            let config: EffectConfig = serde_json::from_str(&format!(r#"{{"version":1,"channels":1,"effect":{{"kind":"{kind}","parameters":{parameters}}}}}"#)).unwrap();
            config.validate().unwrap();
            match &config.effect {
                Effect::Delay(p) => assert_eq!(p.mode, 0),
                Effect::Reverb(p) => {
                    assert_eq!(p.room_size, 1.0);
                    assert_eq!(p.pre_delay_ms, 0.0);
                    assert_eq!(p.algorithm, 0);
                }
                _ => unreachable!(),
            }
            assert_eq!(
                serde_json::from_value::<EffectConfig>(serde_json::to_value(&config).unwrap())
                    .unwrap(),
                config
            );
        }
    }

    #[test]
    fn spatial_geometry_and_delay_mode_reject_unsupported_values() {
        let mut delay: EffectConfig = serde_json::from_str(r#"{"version":1,"channels":2,"effect":{"kind":"delay","parameters":{"timeMs":100,"feedback":0.3,"mix":0.2}}}"#).unwrap();
        for mode in [0, 1, 2, 255] {
            let Effect::Delay(p) = &mut delay.effect else {
                unreachable!()
            };
            p.mode = mode;
            assert_eq!(delay.validate().is_ok(), mode < 2);
        }
        let mut reverb: EffectConfig = serde_json::from_str(r#"{"version":1,"channels":2,"effect":{"kind":"reverb","parameters":{"decaySeconds":1.2,"damping":0.5,"mix":0.2}}}"#).unwrap();
        for algorithm in [0, 1, 2, 255] {
            let Effect::Reverb(p) = &mut reverb.effect else {
                unreachable!()
            };
            p.algorithm = algorithm;
            assert_eq!(reverb.validate().is_ok(), algorithm < 2);
        }
        let Effect::Reverb(p) = &mut reverb.effect else {
            unreachable!()
        };
        p.algorithm = 0;
        for (size, pre, valid) in [
            (0.5, 0.0, true),
            (2.0, 200.0, true),
            (0.49, 0.0, false),
            (2.01, 0.0, false),
            (1.0, -0.1, false),
            (1.0, 200.1, false),
            (f32::NAN, 0.0, false),
            (1.0, f32::INFINITY, false),
        ] {
            let Effect::Reverb(p) = &mut reverb.effect else {
                unreachable!()
            };
            p.room_size = size;
            p.pre_delay_ms = pre;
            assert_eq!(reverb.validate().is_ok(), valid);
        }
    }

    #[test]
    fn legacy_equalizer_graphs_restore_middle_band_and_new_band_defaults() {
        let config: EffectConfig = serde_json::from_str(r#"{"version":1,"channels":2,"effect":{"kind":"equalizer","parameters":{"lowDb":2,"midDb":-3,"highDb":4,"frequencyHz":1200,"q":2}}}"#).unwrap();
        config.validate().unwrap();
        let Effect::Equalizer(parameters) = &config.effect else {
            panic!("expected equalizer")
        };
        assert_eq!(parameters.low_frequency_hz, 150.0);
        assert_eq!(parameters.high_frequency_hz, 6_000.0);
        assert_eq!(parameters.low_q, 0.707);
        assert_eq!(parameters.high_q, 0.707);
        assert_eq!(parameters.frequency_hz, 1_200.0);
        assert_eq!(parameters.q, 2.0);
        let saved = serde_json::to_value(&config).unwrap();
        assert_eq!(saved["effect"]["parameters"]["frequencyHz"], 1_200.0);
        assert_eq!(saved["effect"]["parameters"]["lowFrequencyHz"], 150.0);
        assert_eq!(
            serde_json::from_value::<EffectConfig>(saved).unwrap(),
            config
        );
    }

    #[test]
    fn all_equalizer_band_frequency_and_q_ranges_are_validated() {
        let original: EffectConfig = serde_json::from_str(r#"{"version":1,"channels":2,"effect":{"kind":"equalizer","parameters":{"lowDb":0,"midDb":0,"highDb":0,"frequencyHz":1000,"q":0.707}}}"#).unwrap();
        for band in 0..3 {
            for (hz, q, valid) in [
                (20.0, 0.1, true),
                (20_000.0, 10.0, true),
                (19.0, 1.0, false),
                (20_001.0, 1.0, false),
                (100.0, 0.099, false),
                (100.0, 10.001, false),
                (f32::NAN, 1.0, false),
                (100.0, f32::INFINITY, false),
            ] {
                let mut config = original.clone();
                let Effect::Equalizer(p) = &mut config.effect else {
                    unreachable!()
                };
                match band {
                    0 => {
                        p.low_frequency_hz = hz;
                        p.low_q = q;
                    }
                    1 => {
                        p.frequency_hz = hz;
                        p.q = q;
                    }
                    _ => {
                        p.high_frequency_hz = hz;
                        p.high_q = q;
                    }
                }
                assert_eq!(
                    config.validate().is_ok(),
                    valid,
                    "band {band}, {hz} Hz, Q {q}"
                );
            }
        }
    }
}
