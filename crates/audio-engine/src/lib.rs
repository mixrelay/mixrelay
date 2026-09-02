use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LatencyProfile {
    Stable,
    LowLatency,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineConfig {
    pub sample_rate: u32,
    pub buffer_frames: u32,
    pub latency_profile: LatencyProfile,
}

impl EngineConfig {
    pub const fn stable() -> Self {
        Self {
            sample_rate: 48_000,
            buffer_frames: 128,
            latency_profile: LatencyProfile::Stable,
        }
    }

    pub const fn low_latency() -> Self {
        Self {
            sample_rate: 48_000,
            buffer_frames: 32,
            latency_profile: LatencyProfile::LowLatency,
        }
    }

    pub fn validate(self) -> Result<Self, ConfigError> {
        if !matches!(self.sample_rate, 44_100 | 48_000) {
            return Err(ConfigError::UnsupportedSampleRate(self.sample_rate));
        }

        if !(16..=16_384).contains(&self.buffer_frames) || !self.buffer_frames.is_power_of_two() {
            return Err(ConfigError::InvalidBufferFrames(self.buffer_frames));
        }

        Ok(self)
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("unsupported sample rate: {0}")]
    UnsupportedSampleRate(u32),
    #[error("buffer frames must be a power of two between 16 and 16384: {0}")]
    InvalidBufferFrames(u32),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct XrunCounters {
    pub underruns: u64,
    pub overruns: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_profile_is_valid() {
        assert_eq!(
            EngineConfig::stable().validate(),
            Ok(EngineConfig::stable())
        );
    }

    #[test]
    fn rejects_non_power_of_two_buffer() {
        let config = EngineConfig {
            buffer_frames: 96,
            ..EngineConfig::stable()
        };
        assert_eq!(config.validate(), Err(ConfigError::InvalidBufferFrames(96)));
    }
}
