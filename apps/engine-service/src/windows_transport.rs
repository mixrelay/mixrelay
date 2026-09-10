//! Owner-bound v2 shared PCM mapping. Open/close belong to control/reclaimer
//! threads; one audio worker holds mutable access for a complete graph cycle.
pub(super) use mixrelay_engine_service::windows_transport_contract::STARTUP_SILENCE_FRAMES;
use mixrelay_engine_service::windows_transport_contract::{
    Descriptor, IOCTL_MAP, LAB_LAYOUT, Layout,
};
use std::{
    ffi::c_void,
    ptr::NonNull,
    sync::{
        Arc,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
};
use windows::{
    Win32::{
        Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE},
        Storage::FileSystem::{
            CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        },
        System::{IO::DeviceIoControl, Threading::WaitForSingleObject},
    },
    core::w,
};

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        if self.0.is_invalid() {
            return;
        }
        // SAFETY: each successful OS handle is adopted exactly once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// A wait owns its event independently, so it need not hold the PCM mutex.
pub(super) struct WakeEvent(Handle);
// SAFETY: Windows synchronization handles support concurrent cross-thread waits.
// Arc ownership keeps this event open; no handle or mapped pointer escapes.
unsafe impl Send for WakeEvent {}
unsafe impl Sync for WakeEvent {}
impl WakeEvent {
    pub(super) fn wait(&self) {
        // SAFETY: this owner retains the event through the entire bounded wait.
        unsafe {
            let _ = WaitForSingleObject(self.0.0, 5);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TransportFault {
    Offline,
    InvalidCounters,
    InvalidQuantum,
}

pub(super) struct Transport {
    // Field order matters: close event references before file CLEANUP unmaps pages.
    render_event: Arc<WakeEvent>,
    _capture_event: Handle,
    _file: Handle,
    base: NonNull<u64>,
    layout: Layout,
    read: u64,
    written: u64,
    last_produced: u64,
    last_consumed: u64,
    // A repaired-looking shared header cannot restore trust in this mapping.
    // Recovery requires a new owner/handshake, after control-plane removal.
    fault: Option<TransportFault>,
    #[cfg(test)]
    _test_memory: Option<Vec<u64>>,
}
// SAFETY: mapping is process-local, not thread-local. Mutable access is serialized
// by the owning engine resource mutex; no raw pointer or borrowed slice escapes.
unsafe impl Send for Transport {}

impl Transport {
    pub(super) fn open_lab() -> Result<Self, String> {
        // LAB is an explicit opt-in. Never fall back from this identity to a
        // physical/default endpoint or the old experimental MixRelay driver.
        let file = Handle(
            unsafe {
                CreateFileW(
                    w!("\\\\.\\MixRelaySysvadBridge"),
                    GENERIC_READ.0 | GENERIC_WRITE.0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    None,
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    None,
                )
            }
            .map_err(|e| format!("open shared transport: {e}"))?,
        );
        let layout = LAB_LAYOUT;
        let request = layout.request();
        let mut descriptor = Descriptor::default();
        let mut returned = 0;
        // SAFETY: METHOD_BUFFERED receives exact initialized request/output sizes.
        unsafe {
            DeviceIoControl(
                file.0,
                IOCTL_MAP,
                Some(request.as_ptr().cast()),
                24,
                Some((&raw mut descriptor).cast()),
                40,
                Some(&raw mut returned),
                None,
            )
        }
        .map_err(|e| format!("map shared transport: {e}"))?;
        let render_event = Handle(HANDLE(descriptor.render_event as *mut c_void));
        let capture_event = Handle(HANDLE(descriptor.capture_event as *mut c_void));
        if render_event.0.is_invalid() || capture_event.0.is_invalid() {
            return Err("invalid shared transport event handles".into());
        }
        let address = descriptor.mapping_address(returned, layout)?;
        let base = NonNull::new(address as *mut u64).ok_or("null mapping")?;
        let mut value = Self {
            render_event: Arc::new(WakeEvent(render_event)),
            _capture_event: capture_event,
            _file: file,
            base,
            layout,
            read: 0,
            written: 0,
            last_produced: 0,
            last_consumed: 0,
            fault: None,
            #[cfg(test)]
            _test_memory: None,
        };
        // SAFETY: trusted kernel descriptor pins the validated readable/writable extent
        // until this file closes. Check the immutable ABI before using any PCM.
        let header: [u32; 7] = unsafe { std::ptr::read_volatile(value.base.as_ptr().cast()) };
        if !layout.matches_header(
            header,
            [value.load64(64), value.load64(72), value.load64(80)],
        ) {
            return Err("shared header disagrees with negotiated layout".into());
        }
        // Capture may request frames before the first complete render quantum.
        // Publish a bounded silent lead once per mapping, not on every graph
        // revision. This trades 256/48000 seconds of queue slack for startup safety.
        value.write(&[0.0; STARTUP_SILENCE_FRAMES * LAB_LAYOUT.capture_channels()]);
        Ok(value)
    }
    fn load64(&self, offset: usize) -> u64 {
        // SAFETY: constant 8-byte aligned header offset, inside the owned mapping;
        // kernel publishes counters with Interlocked operations of the same width.
        unsafe { &*self.base.as_ptr().add(offset / 8).cast::<AtomicU64>() }.load(Ordering::Acquire)
    }
    fn store64(&self, offset: usize, value: u64) {
        // SAFETY: only the single graph worker publishes these consumer/producer counters.
        unsafe { &*self.base.as_ptr().add(offset / 8).cast::<AtomicU64>() }
            .store(value, Ordering::Release);
    }
    pub(super) fn online(&self) -> bool {
        // SAFETY: aligned State field remains mapped even when PnP goes offline.
        unsafe { &*self.base.as_ptr().cast::<AtomicU32>().add(7) }.load(Ordering::Acquire) == 1
    }
    pub(super) fn counters(&self) -> (u64, u64) {
        (self.load64(88), self.load64(96))
    }
    pub(super) fn fault(&self) -> Option<TransportFault> {
        self.fault
    }
    fn fail(&mut self, fault: TransportFault) -> Result<bool, TransportFault> {
        self.fault = Some(fault);
        Err(fault)
    }
    pub(super) fn ready(&mut self, frames: u32) -> Result<bool, TransportFault> {
        if let Some(fault) = self.fault {
            return Err(fault);
        }
        if frames == 0 || u64::from(frames) > self.layout.capacity() {
            return self.fail(TransportFault::InvalidQuantum);
        }
        if !self.online() {
            return self.fail(TransportFault::Offline);
        }
        let produced = self.load64(32);
        let consumed = self.load64(56);
        if !self.layout.valid_counters(
            self.read,
            self.written,
            (self.last_produced, self.last_consumed),
            (produced, consumed),
        ) {
            return self.fail(TransportFault::InvalidCounters);
        }
        self.last_produced = produced;
        self.last_consumed = consumed;
        Ok(produced - self.read >= u64::from(frames)
            && self.layout.capacity() - (self.written - consumed) >= u64::from(frames))
    }
    pub(super) fn waiter(&self) -> Arc<WakeEvent> {
        // Clone is allocation-free. Caller releases it before changing the
        // active mapping; an event wakeup never authorizes DSP without ready().
        self.render_event.clone()
    }
    pub(super) fn read(&mut self, destination: &mut [f32]) -> u32 {
        destination.fill(0.0);
        if self.fault.is_some() {
            return 0;
        }
        let channels = self.layout.render_channels();
        if channels == 0 || !destination.len().is_multiple_of(channels) {
            return 0;
        }
        let count = (destination.len() / channels)
            .min(usize::try_from(self.last_produced - self.read).expect("bounded ring"));
        for frame in 0..count {
            let slot = usize::try_from((self.read + frame as u64) % self.layout.capacity())
                .expect("bounded ring slot");
            for channel in 0..channels {
                // SAFETY: producer release/acquire owns these complete frames;
                // bounded private layout, never live header offsets, forms addresses.
                destination[frame * channels + channel] = unsafe {
                    self.base
                        .as_ptr()
                        .cast::<f32>()
                        .add(self.layout.render_offset() / size_of::<f32>())
                        .add(slot * channels + channel)
                        .read_volatile()
                };
            }
        }
        self.read += count as u64;
        self.store64(40, self.read);
        u32::try_from(count).expect("bounded ring")
    }
    pub(super) fn write(&mut self, source: &[f32]) -> u32 {
        if self.fault.is_some() {
            return 0;
        }
        let channels = self.layout.capture_channels();
        if channels == 0 || !source.len().is_multiple_of(channels) {
            return 0;
        }
        let count = (source.len() / channels).min(
            usize::try_from(self.layout.capacity() - (self.written - self.last_consumed))
                .expect("bounded ring"),
        );
        for (sample_index, sample) in source.iter().take(count * channels).enumerate() {
            let frame = sample_index / channels;
            let channel = sample_index % channels;
            let slot = usize::try_from((self.written + frame as u64) % self.layout.capacity())
                .expect("bounded ring slot");
            // SAFETY: this producer owns the free frame until the release below.
            unsafe {
                self.base
                    .as_ptr()
                    .cast::<f32>()
                    .add(self.layout.capture_offset() / size_of::<f32>())
                    .add(slot * channels + channel)
                    .write_volatile(*sample);
            }
        }
        self.written += count as u64;
        self.store64(48, self.written);
        u32::try_from(count).expect("bounded ring")
    }
}

/// Owns ordinary memory and real event handles, but never opens or mutates a driver.
/// Only Windows unit tests can construct or fault this mapping.
#[cfg(test)]
impl Transport {
    pub(super) fn test_fixture(layout: Layout) -> Self {
        use windows::Win32::System::Threading::CreateEventW;
        let mut memory = vec![0; layout.mapping_bytes() / size_of::<u64>()];
        let event = || Handle(unsafe { CreateEventW(None, false, false, None) }.unwrap());
        let value = Self {
            render_event: Arc::new(WakeEvent(event())),
            _capture_event: event(),
            _file: Handle(HANDLE::default()),
            base: NonNull::new(memory.as_mut_ptr()).unwrap(),
            layout,
            read: 0,
            written: 0,
            last_produced: 0,
            last_consumed: 0,
            fault: None,
            _test_memory: Some(memory),
        };
        value.test_set_online(true);
        value
    }

    pub(super) fn test_set_online(&self, online: bool) {
        // SAFETY: fixture owns the aligned State field; publication mirrors the kernel.
        unsafe { &*self.base.as_ptr().cast::<AtomicU32>().add(7) }
            .store(u32::from(online), Ordering::Release);
    }

    pub(super) fn test_set_produced(&self, frames: u64) {
        self.store64(32, frames);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;
    const CAPACITY: u64 = LAB_LAYOUT.capacity();
    fn valid_counters(read: u64, written: u64, previous: (u64, u64), peer: (u64, u64)) -> bool {
        LAB_LAYOUT.valid_counters(read, written, previous, peer)
    }
    #[test]
    fn abi_matches_c_and_malicious_counters_fail_before_copy() {
        assert_eq!(size_of::<Descriptor>(), 40);
        assert_eq!(std::mem::offset_of!(Descriptor, address), 16);
        assert_eq!(IOCTL_MAP, 0x001d_e000);
        assert!(valid_counters(0, 0, (0, 0), (0, 0)));
        assert!(valid_counters(0, 4096, (0, 0), (4096, 0)));
        assert!(!valid_counters(0, 0, (0, 0), (4097, 0)));
        assert!(!valid_counters(0, 0, (0, 0), (0, 1)));
        assert!(!valid_counters(8, 0, (8, 0), (7, 0)));
        assert!(!valid_counters(0, 8, (0, 8), (0, 7)));
        assert!(!valid_counters(0, 0, (0, 0), (u64::MAX, 0)));
        assert!(!valid_counters(0, 4097, (0, 0), (0, 0)));
    }
    #[test]
    fn wrapped_counter_windows_preserve_capacity() {
        for wrap in 0..128 {
            let base = wrap * CAPACITY;
            for count in [0, 1, 255, 256, 4095, 4096] {
                assert!(valid_counters(
                    base,
                    base + count,
                    (base, base),
                    (base + count, base)
                ));
                assert!(!valid_counters(
                    base,
                    base + count,
                    (base, base),
                    (base + CAPACITY + 1, base)
                ));
            }
        }
    }

    #[test]
    fn interleaved_copies_preserve_channels_across_wrap_and_odd_byte_offsets() {
        for (render, capture, capacity) in [(2, 1, 4096), (64, 16, 4096), (1, 3, 257)] {
            let layout = Layout::new(render, capture, 48000, capacity).unwrap();
            let mut memory = vec![0u64; layout.mapping_bytes() / 8];
            // The local Vec owns the complete aligned mapping. Dummy invalid
            // handles are inert; normal Drop still releases the event Arc.
            let mut io = Transport {
                render_event: Arc::new(WakeEvent(Handle(HANDLE::default()))),
                _capture_event: Handle(HANDLE::default()),
                _file: Handle(HANDLE::default()),
                base: NonNull::new(memory.as_mut_ptr()).unwrap(),
                layout,
                read: u64::from(capacity) - 1,
                written: u64::from(capacity) - 1,
                last_produced: u64::from(capacity) + 1,
                last_consumed: u64::from(capacity) - 1,
                fault: None,
                _test_memory: None,
            };
            // Distinct exact binary fractions detect channel permutations, not
            // merely the presence of some signal on each side of the wrap.
            let render_samples: Vec<f32> = (1..=2 * render)
                .map(|index| f32::from(u16::try_from(index).unwrap()) / 128.0)
                .collect();
            let capture_samples: Vec<f32> = (1..=2 * capture)
                .map(|index| -f32::from(u16::try_from(index).unwrap()) / 128.0)
                .collect();
            for (index, sample) in render_samples.iter().enumerate() {
                let slot = (capacity as usize - 1 + index / render as usize) % capacity as usize;
                // SAFETY: bounded owned fixture, no concurrent producer.
                unsafe {
                    memory
                        .as_mut_ptr()
                        .cast::<f32>()
                        .add(layout.render_offset() / size_of::<f32>())
                        .add(slot * render as usize + index % render as usize)
                        .write(*sample);
                }
            }
            let mut received = vec![0.0; render_samples.len()];
            assert_eq!(io.read(&mut received), 2);
            assert_eq!(received, render_samples);
            assert_eq!(io.write(&capture_samples), 2);
            for (index, expected) in capture_samples.iter().enumerate() {
                let slot = (capacity as usize - 1 + index / capture as usize) % capacity as usize;
                // SAFETY: inspect only the frames published into the owned fixture.
                let actual = unsafe {
                    memory
                        .as_ptr()
                        .cast::<f32>()
                        .add(layout.capture_offset() / size_of::<f32>())
                        .add(slot * capture as usize + index % capture as usize)
                        .read()
                };
                assert_eq!(actual.to_bits(), expected.to_bits());
            }
            assert_eq!(io.load64(40), u64::from(capacity) + 1);
            assert_eq!(io.load64(48), u64::from(capacity) + 1);
            assert_eq!(io.read(&mut received), 0);
            assert!(received.iter().all(|sample| *sample == 0.0));
        }
    }

    #[test]
    fn faults_are_sticky_and_never_publish_or_copy_more_pcm() {
        for fault in [
            TransportFault::Offline,
            TransportFault::InvalidCounters,
            TransportFault::InvalidQuantum,
        ] {
            let mut io = Transport::test_fixture(LAB_LAYOUT);
            match fault {
                TransportFault::Offline => io.test_set_online(false),
                TransportFault::InvalidCounters => io.test_set_produced(LAB_LAYOUT.capacity() + 1),
                TransportFault::InvalidQuantum => {}
            }
            assert_eq!(
                io.ready(if fault == TransportFault::InvalidQuantum {
                    0
                } else {
                    128
                }),
                Err(fault)
            );
            io.test_set_online(true);
            io.test_set_produced(128);
            assert_eq!(io.ready(128), Err(fault));
            let mut destination = [1.0; 256];
            assert_eq!(io.read(&mut destination), 0);
            assert!(destination.iter().all(|sample| sample.to_bits() == 0));
            assert_eq!(io.write(&[1.0; 128]), 0);
            assert_eq!((io.load64(40), io.load64(48)), (0, 0));
        }
        let mut fresh = Transport::test_fixture(LAB_LAYOUT);
        assert_eq!(fresh.ready(128), Ok(false)); // Stalled is not failed.
        fresh.test_set_produced(128);
        assert_eq!(fresh.ready(128), Ok(true));
    }
}
