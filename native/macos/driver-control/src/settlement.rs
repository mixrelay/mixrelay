//! HAL reports of how rate, format and mapping requests ended, and their attribution
//! to the request the service issued.
use std::collections::VecDeque;

/// HAL ↔ Driver service report version. Both sides ship in one package; a report with
/// another version means a partial install and is surfaced instead of being ignored.
pub(crate) const PROTOCOL: u64 = 2;
/// Reports kept per HAL connection. Older entries predate every request still waiting:
/// a request waits at most `settle::ACK_TIMEOUT + settle::SETTLE_TIMEOUT`, and the HAL
/// settles far fewer requests than this in that time.
const RETAINED: usize = 256;

/// What one service request asked the HAL for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Wanted {
    Rate(u32),
    /// A directional sample format, as a HAL `SampleFormat` code, at the device rate.
    Format {
        render: bool,
        rate: u32,
        format: u32,
    },
}

/// How the HAL ended one rate, format or mapping request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Configured {
    pub(crate) endpoint: [u8; 16],
    /// HAL-wide settlement order.
    pub(crate) sequence: u64,
    pub(crate) rate: u32,
    /// Requested capture and render `SampleFormat` codes.
    pub(crate) formats: [u32; 2],
    pub(crate) applied: bool,
}

impl Configured {
    /// Whether this settlement ended a request for exactly `wanted` on `endpoint`.
    fn answers(&self, endpoint: [u8; 16], wanted: Wanted) -> bool {
        self.endpoint == endpoint
            && match wanted {
                Wanted::Rate(rate) => self.rate == rate,
                Wanted::Format {
                    render,
                    rate,
                    format,
                } => self.rate == rate && self.formats[usize::from(render)] == format,
            }
    }
}

/// One HAL connection's settlements, oldest first, bounded by `RETAINED`.
#[derive(Debug, Default)]
pub(crate) struct Log(VecDeque<Configured>);

impl Log {
    pub(crate) fn record(&mut self, configured: Configured) {
        // The HAL resends retained settlements after a reconnect; keep each one once.
        if self.newest() >= configured.sequence {
            return;
        }
        if self.0.len() == RETAINED {
            self.0.pop_front();
        }
        self.0.push_back(configured);
    }

    /// The newest sequence received; a request issued now settles after it.
    pub(crate) fn newest(&self) -> u64 {
        self.0.back().map_or(0, |configured| configured.sequence)
    }

    /// The first settlement after `since` that ended a request for exactly `wanted`.
    pub(crate) fn find(
        &self,
        endpoint: [u8; 16],
        since: u64,
        wanted: Wanted,
    ) -> Option<Configured> {
        self.0
            .iter()
            .find(|configured| configured.sequence > since && configured.answers(endpoint, wanted))
            .copied()
    }
}

#[cfg(test)]
mod tests;
