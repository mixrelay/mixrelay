use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

/// Control-thread sampling requests and the last completed transport measurement.
#[derive(Default)]
pub(crate) struct TimingMetrics {
    requested: AtomicU64,
    completed: AtomicU64,
    nanos: AtomicU64,
}

impl TimingMetrics {
    pub(crate) fn request(&self) {
        self.requested.fetch_add(1, Ordering::Release);
    }

    pub(crate) fn latest(&self) -> Option<u64> {
        let completed = self.completed.load(Ordering::Acquire);
        let requested = self.requested.load(Ordering::Acquire);
        let nanos = self.nanos.load(Ordering::Relaxed);
        (completed != 0 && requested.saturating_sub(completed) <= 1 && nanos != 0).then_some(nanos)
    }
}

/// Audio-owner state: clock reads occur only for the one requested block each second.
#[derive(Default)]
pub(crate) struct TimingProbe {
    generation: u64,
    pending: Option<(u64, u64, Instant)>,
}

impl TimingProbe {
    pub(crate) fn submitted(&mut self, sequence: u64, metrics: &TimingMetrics) {
        let generation = metrics.requested.load(Ordering::Acquire);
        if generation == self.generation {
            return;
        }
        self.generation = generation;
        self.pending = Some((sequence, generation, Instant::now()));
    }

    pub(crate) fn received(&mut self, sequence: u64, metrics: &TimingMetrics) {
        let Some((expected, generation, start)) = self.pending else {
            return;
        };
        if sequence != expected {
            return;
        }
        self.pending = None;
        let nanos = u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX);
        metrics.nanos.store(nanos, Ordering::Relaxed);
        metrics.completed.store(generation, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn samples_only_requested_sequences_and_expires_stalled_measurements() {
        let metrics = TimingMetrics::default();
        let mut probe = TimingProbe::default();
        probe.submitted(1, &metrics);
        assert!(probe.pending.is_none());
        assert_eq!(metrics.latest(), None);
        metrics.request();
        probe.submitted(2, &metrics);
        probe.submitted(3, &metrics);
        assert_eq!(probe.pending.as_ref().unwrap().0, 2);
        probe.received(1, &metrics);
        assert_eq!(metrics.latest(), None);
        probe.pending.as_mut().unwrap().2 = Instant::now()
            .checked_sub(Duration::from_millis(1))
            .unwrap();
        probe.received(2, &metrics);
        assert!(metrics.latest().unwrap() >= 1_000_000);
        let measured = metrics.latest();
        probe.received(2, &metrics);
        assert_eq!(metrics.latest(), measured);
        metrics.request();
        assert_eq!(metrics.latest(), measured);
        metrics.request();
        assert_eq!(metrics.latest(), None);
    }

    #[test]
    fn a_new_sampling_request_cannot_accept_the_old_pending_output() {
        let metrics = TimingMetrics::default();
        let mut probe = TimingProbe::default();
        metrics.request();
        probe.submitted(10, &metrics);
        metrics.request();
        probe.submitted(11, &metrics);
        probe.received(10, &metrics);
        assert_eq!(metrics.latest(), None);
        assert_eq!(probe.pending.as_ref().unwrap().0, 11);
        probe.received(11, &metrics);
        assert!(probe.pending.is_none());
        assert_eq!(metrics.completed.load(Ordering::Acquire), 2);
    }
}
