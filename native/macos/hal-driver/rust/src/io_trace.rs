//! Opt-in, bounded HAL callback evidence without retaining audio samples.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::io_outcome::IoOutcome;

// A complete 48 kHz installed probe retained up to 3,886 observations across
// 128/512-frame clients. Longer sessions still report overwritten rows explicitly.
const CAPACITY: usize = 4096;
const FIELDS: usize = 8;

pub(crate) struct IoTrace {
    enabled: AtomicBool,
    next: AtomicU64,
    dropped: AtomicU64,
    rows: Box<[Row]>,
}

struct Row {
    sequence: AtomicU64,
    fields: [AtomicU64; FIELDS],
}

#[derive(Clone, Copy)]
pub(crate) struct Observation<'a> {
    pub render: bool,
    pub client: u32,
    pub frame: u64,
    pub channels: usize,
    pub samples: &'a [f32],
    pub outcome: IoOutcome,
}

impl IoTrace {
    pub(crate) fn new() -> Result<Self, std::collections::TryReserveError> {
        let mut rows = Vec::new();
        rows.try_reserve_exact(CAPACITY)?;
        rows.extend((0..CAPACITY).map(|_| Row {
            sequence: AtomicU64::new(0),
            fields: std::array::from_fn(|_| AtomicU64::new(0)),
        }));
        Ok(Self {
            enabled: AtomicBool::new(false),
            next: AtomicU64::new(0),
            dropped: AtomicU64::new(0),
            rows: rows.into_boxed_slice(),
        })
    }

    /// Control holds the device freeze, excluding callbacks during reset.
    pub(crate) fn set_enabled(&self, enabled: bool) {
        self.next.store(0, Ordering::Relaxed);
        self.dropped.store(0, Ordering::Relaxed);
        for row in &self.rows {
            row.sequence.store(0, Ordering::Relaxed);
        }
        // Release makes the reset visible to callbacks admitted after the freeze.
        self.enabled.store(enabled, Ordering::Release);
    }

    pub(crate) fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    pub(crate) fn record(&self, observation: Observation<'_>) {
        if !self.enabled() || observation.channels == 0 {
            return;
        }
        let id = self.next.fetch_add(1, Ordering::Relaxed).saturating_add(1);
        if id > u64::MAX / 2 {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            self.enabled.store(false, Ordering::Release);
            return;
        }
        let Ok(slot) = usize::try_from((id - 1) % CAPACITY as u64) else {
            return;
        };
        let Some(row) = self.rows.get(slot) else {
            return;
        };
        let previous = row.sequence.load(Ordering::SeqCst);
        // One bounded claim; concurrent readers/writers never wait for evidence.
        if previous & 1 != 0
            || previous >= id * 2
            || row
                .sequence
                .compare_exchange(previous, id * 2 - 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
        {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let mut zero_frames = 0;
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for frame in observation.samples.chunks_exact(observation.channels) {
            zero_frames += u64::from(frame.iter().all(|sample| *sample == 0.0));
            for sample in frame {
                hash = (hash ^ u64::from(sample.to_bits())).wrapping_mul(0x100_0000_01b3);
            }
        }
        let fields = [
            u64::from(observation.render),
            observation.frame,
            (observation.samples.len() / observation.channels) as u64,
            u64::from(observation.outcome.accepted),
            u64::from(observation.outcome.available),
            zero_frames,
            hash,
            u64::from(observation.client),
        ];
        // SeqCst cells make a snapshot's two sequence reads bracket every field;
        // an overwritten or still-publishing row is refused instead of mixed.
        for (cell, value) in row.fields.iter().zip(fields) {
            cell.store(value, Ordering::SeqCst);
        }
        row.sequence.store(id * 2, Ordering::SeqCst);
    }

    /// Fixed-size, native-endian `UInt64` payload: version, fields, capacity, issued,
    /// dropped, then (sequence, render, frame, frames, accepted, available, zeros, hash, client).
    pub(crate) fn snapshot(&self) -> Vec<u8> {
        let mut words = vec![
            2,
            FIELDS as u64,
            CAPACITY as u64,
            self.next.load(Ordering::Acquire),
            self.dropped.load(Ordering::Acquire),
        ];
        for row in &self.rows {
            let before = row.sequence.load(Ordering::SeqCst);
            let fields = row
                .fields
                .each_ref()
                .map(|cell| cell.load(Ordering::SeqCst));
            let after = row.sequence.load(Ordering::SeqCst);
            if before != 0 && before & 1 == 0 && before == after {
                words.push(before / 2);
                words.extend(fields);
            } else {
                words.extend([0; FIELDS + 1]);
            }
        }
        words.into_iter().flat_map(u64::to_ne_bytes).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(trace: &IoTrace) -> Vec<u64> {
        trace
            .snapshot()
            .chunks_exact(size_of::<u64>())
            .map(|value| u64::from_ne_bytes(value.try_into().unwrap()))
            .collect()
    }

    #[test]
    fn disabled_and_enabled_callback_evidence_never_allocates() {
        let trace = IoTrace::new().unwrap();
        let pcm = [0.25, -0.25, 0.0, 0.0];
        for enabled in [false, true] {
            trace.set_enabled(enabled);
            let measured = allocation_counter::measure(|| {
                trace.record(Observation {
                    render: false,
                    client: 0,
                    frame: 64,
                    channels: 2,
                    samples: &pcm,
                    outcome: IoOutcome::completed(1),
                });
            });
            assert_eq!(measured.count_total, 0);
            let actual = words(&trace);
            assert_eq!(actual[3], u64::from(enabled));
            if enabled {
                assert_eq!(&actual[5..12], &[1, 0, 64, 2, 1, 1, 1]);
            }
        }
    }

    #[test]
    fn bounded_history_marks_overwrites_and_reset_removes_old_observations() {
        let trace = IoTrace::new().unwrap();
        trace.set_enabled(true);
        for frame in 0..CAPACITY as u64 + 3 {
            trace.record(Observation {
                render: true,
                client: 42,
                frame,
                channels: 1,
                samples: &[0.5],
                outcome: IoOutcome::completed(1),
            });
        }
        let actual = words(&trace);
        assert_eq!(actual[3], CAPACITY as u64 + 3);
        assert_eq!(actual[4], 0);
        let mut sequences: Vec<_> = actual[5..]
            .chunks_exact(FIELDS + 1)
            .map(|row| row[0])
            .collect();
        sequences.sort_unstable();
        assert_eq!(sequences, (4..=CAPACITY as u64 + 3).collect::<Vec<_>>());
        trace.set_enabled(false);
        assert!(words(&trace)[3..].iter().all(|word| *word == 0));
    }
}
