//! Retained last clock-break evidence; callback publishes, control formats it.
use super::DeviceIo;
use std::sync::atomic::{AtomicU64, Ordering::SeqCst};

/// Native sample and host positions at the most recent detected timeline break.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct ClockBreak {
    /// Cumulative native continuity counter at publication.
    pub count: u64,
    /// Previous packet's sample position plus its actual frame count.
    pub expected_sample: f64,
    /// Sample position supplied by the discontinuous packet.
    pub actual_sample: f64,
    /// Prior packet's native host-clock ticks, not callback arrival time.
    pub previous_host: u64,
    /// Discontinuous packet's native host-clock ticks.
    pub actual_host: u64,
    /// Unmodified `CoreAudio` timestamp-validity flags.
    pub flags: u64,
}

#[derive(Default)]
pub(super) struct ClockEvidence {
    sequence: AtomicU64,
    fields: [AtomicU64; 6],
}

impl ClockEvidence {
    pub(super) fn publish(&self, event: ClockBreak) {
        self.sequence.fetch_add(1, SeqCst);
        let values = [
            event.count,
            event.expected_sample.to_bits(),
            event.actual_sample.to_bits(),
            event.previous_host,
            event.actual_host,
            event.flags,
        ];
        for (field, value) in self.fields.iter().zip(values) {
            field.store(value, SeqCst);
        }
        self.sequence.fetch_add(1, SeqCst);
    }

    fn read(&self) -> Option<ClockBreak> {
        let sequence = self.sequence.load(SeqCst);
        if sequence & 1 != 0 {
            return None;
        }
        let values = self.fields.each_ref().map(|field| field.load(SeqCst));
        (sequence == self.sequence.load(SeqCst) && values[0] != 0).then_some(ClockBreak {
            count: values[0],
            expected_sample: f64::from_bits(values[1]),
            actual_sample: f64::from_bits(values[2]),
            previous_host: values[3],
            actual_host: values[4],
            flags: values[5],
        })
    }
}

/// Tries one atomic snapshot. False leaves output untouched, including before any break.
/// # Safety
/// `io` is null or initialized and retained for this read. Non-null `event` owns
/// writable aligned storage disjoint from the device. No callback state is borrowed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_device_io_clock_break(
    io: *const DeviceIo,
    event: *mut ClockBreak,
) -> bool {
    // SAFETY: the native owner retains its initialized atomic evidence.
    let Some(io) = (unsafe { io.as_ref() }) else {
        return false;
    };
    if event.is_null() {
        return false;
    }
    let Some(snapshot) = io.clock_evidence.read() else {
        return false;
    };
    // SAFETY: caller owns a full, aligned output span separate from the retained owner.
    unsafe { event.write(snapshot) };
    true
}

const _: () = assert!(size_of::<ClockBreak>() == 48 && align_of::<ClockBreak>() == 8);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn evidence_preserves_fractional_positions_and_rejects_pending_publication() {
        let evidence = ClockEvidence::default();
        assert!(evidence.read().is_none());
        evidence.publish(ClockBreak {
            count: 2,
            expected_sample: 320.5,
            actual_sample: 384.5,
            previous_host: 10,
            actual_host: 12,
            flags: 3,
        });
        let event = evidence.read().unwrap();
        assert_eq!(event.expected_sample.to_bits(), 320.5_f64.to_bits());
        assert_eq!(event.actual_sample.to_bits(), 384.5_f64.to_bits());
        assert_eq!(
            (
                event.count,
                event.previous_host,
                event.actual_host,
                event.flags
            ),
            (2, 10, 12, 3)
        );
        evidence.sequence.fetch_add(1, SeqCst);
        assert!(evidence.read().is_none());
    }
}
