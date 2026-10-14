//! Single-writer, cumulative measurements. Readers never reset callback counters.
use std::sync::atomic::{AtomicU64, Ordering};

/// A coherent cumulative sum and weight, with invalid observations counted separately.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ResidenceSnapshot {
    pub sum: u64,
    pub weight: u64,
    pub invalid: u64,
}
impl ResidenceSnapshot {
    /// Mean over the interval between two snapshots. Counter wrap is intentional.
    #[must_use]
    pub fn mean_since(self, previous: Self) -> Option<f64> {
        let weight = self.weight.wrapping_sub(previous.weight);
        if self.invalid != previous.invalid || weight == 0 {
            return None;
        }
        #[allow(clippy::cast_precision_loss)]
        Some(self.sum.wrapping_sub(previous.sum) as f64 / weight as f64)
    }
}

/// One serialized consumer writes a tap; a control reader gets a bounded snapshot.
/// All callback operations are atomics only, with no allocation, locks or native calls.
#[derive(Debug, Default)]
pub struct Residence {
    sequence: AtomicU64,
    sum: AtomicU64,
    weight: AtomicU64,
    invalid: AtomicU64,
}
impl Residence {
    /// Mirror an exclusively owned converter's cumulative tap to its retained adapter.
    pub fn publish(&self, value: ResidenceSnapshot) {
        self.sequence.fetch_add(1, Ordering::SeqCst);
        self.sum.store(value.sum, Ordering::SeqCst);
        self.weight.store(value.weight, Ordering::SeqCst);
        self.invalid.store(value.invalid, Ordering::SeqCst);
        self.sequence.fetch_add(1, Ordering::SeqCst);
    }
    pub fn record(&self, value: Option<u64>, weight: u64) {
        self.sequence.fetch_add(1, Ordering::SeqCst);
        if let Some(value) = value {
            self.sum
                .fetch_add(value.wrapping_mul(weight), Ordering::SeqCst);
            self.weight.fetch_add(weight, Ordering::SeqCst);
        } else {
            self.invalid.fetch_add(1, Ordering::SeqCst);
        }
        self.sequence.fetch_add(1, Ordering::SeqCst);
    }

    /// Twice the mean fill seen by the frames read: `2 * fill - read`.
    /// Frame weighting keeps partial reads and different callback quantums comparable.
    pub fn read(&self, fill: u64, read: u64) {
        if read != 0 {
            self.record(
                fill.checked_mul(2).and_then(|fill| fill.checked_sub(read)),
                read,
            );
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> Option<ResidenceSnapshot> {
        let before = self.sequence.load(Ordering::SeqCst);
        let value = ResidenceSnapshot {
            sum: self.sum.load(Ordering::SeqCst),
            weight: self.weight.load(Ordering::SeqCst),
            invalid: self.invalid.load(Ordering::SeqCst),
        };
        (before & 1 == 0 && self.sequence.load(Ordering::SeqCst) == before).then_some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_weight_partial_reads_and_invalid_evidence_without_resetting_writer() {
        let tap = Residence::default();
        let empty = tap.snapshot().unwrap();
        assert_eq!(empty.mean_since(empty), None);
        let measured = allocation_counter::measure(|| {
            tap.read(100, 20);
            tap.read(80, 40);
            tap.read(0, 0);
        });
        assert_eq!(measured.count_total, 0);
        let first = tap.snapshot().unwrap();
        assert_eq!(first.mean_since(empty), Some(140.0));
        let mirror = Residence::default();
        mirror.publish(first);
        assert_eq!(mirror.snapshot(), Some(first));
        tap.record(Some(6), 1);
        assert_eq!(tap.snapshot().unwrap().mean_since(first), Some(6.0));
        tap.read(0, 1);
        assert_eq!(tap.snapshot().unwrap().mean_since(first), None);
        tap.sequence.store(1, Ordering::SeqCst);
        assert_eq!(tap.snapshot(), None);
        let wrapped = ResidenceSnapshot {
            sum: 3,
            weight: 1,
            invalid: 0,
        };
        assert_eq!(
            wrapped.mean_since(ResidenceSnapshot {
                sum: u64::MAX,
                weight: u64::MAX,
                invalid: 0
            }),
            Some(2.0)
        );
    }
}
