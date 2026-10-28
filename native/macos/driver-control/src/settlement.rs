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
    /// The HAL load that settled it; sequences restart with each one.
    pub(crate) incarnation: u64,
    pub(crate) endpoint: [u8; 16],
    /// Settlement order within the incarnation.
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

/// The last settlement known when a request was issued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Since {
    pub(crate) incarnation: u64,
    pub(crate) sequence: u64,
}

/// One HAL connection's settlements, oldest first, bounded by `RETAINED`. A connection
/// first replays what the HAL retained; only after its marker is the newest sequence a
/// valid bound for a new request.
#[derive(Debug, Default)]
pub(crate) struct Log {
    incarnation: u64,
    replayed: bool,
    entries: VecDeque<Configured>,
}

impl Log {
    /// Starts over when the HAL reports from another incarnation.
    fn adopt(&mut self, incarnation: u64) {
        if self.incarnation != incarnation {
            *self = Self {
                incarnation,
                ..Self::default()
            };
        }
    }

    pub(crate) fn record(&mut self, configured: Configured) {
        self.adopt(configured.incarnation);
        // A reconnect replays retained settlements; keep each one once.
        if self.newest() >= configured.sequence {
            return;
        }
        if self.entries.len() == RETAINED {
            self.entries.pop_front();
        }
        self.entries.push_back(configured);
    }

    /// The HAL has resent every settlement it retained.
    pub(crate) fn replayed(&mut self, incarnation: u64) {
        self.adopt(incarnation);
        self.replayed = true;
    }

    fn newest(&self) -> u64 {
        self.entries
            .back()
            .map_or(0, |configured| configured.sequence)
    }

    /// The bound for a request issued now, once the replay has completed.
    pub(crate) fn since(&self) -> Option<Since> {
        self.replayed.then(|| Since {
            incarnation: self.incarnation,
            sequence: self.newest(),
        })
    }

    /// The first settlement after `since` that ended a request for exactly `wanted`.
    /// Every settlement of a newer incarnation follows `since`: the HAL restarted.
    pub(crate) fn find(
        &self,
        endpoint: [u8; 16],
        since: Since,
        wanted: Wanted,
    ) -> Option<Configured> {
        let restarted = self.incarnation != since.incarnation;
        self.entries
            .iter()
            .find(|configured| {
                (restarted || configured.sequence > since.sequence)
                    && configured.answers(endpoint, wanted)
            })
            .copied()
    }
}

#[cfg(test)]
mod tests;
