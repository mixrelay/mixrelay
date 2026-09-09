use super::*;
use std::alloc::{Layout, alloc_zeroed, dealloc, handle_alloc_error};
use std::ptr::NonNull;
use std::time::{Duration, Instant};

unsafe extern "C" {
    fn mmap(
        address: *mut c_void,
        size: usize,
        protection: i32,
        flags: i32,
        descriptor: i32,
        offset: i64,
    ) -> *mut c_void;
    fn munmap(address: *mut c_void, size: usize) -> i32;
    fn fork() -> i32;
    fn waitpid(pid: i32, status: *mut i32, options: i32) -> i32;
    fn kill(pid: i32, signal: i32) -> i32;
    fn _exit(status: i32) -> !;
}

/// A real inherited `MAP_SHARED` region; no filesystem name or process-local
/// allocation is substituted for the interprocess producer claim under test.
struct DuplexMapping {
    memory: NonNull<c_void>,
    size: usize,
}
impl DuplexMapping {
    fn new() -> Self {
        use crate::endpoint_transport::{
            EndpointTransportConfig, tn_endpoint_transport_initialize,
            tn_endpoint_transport_required_bytes,
        };
        let config = EndpointTransportConfig {
            endpoint_id: [9; 16],
            capacity_frames: 8,
            render_channels: 1,
            capture_channels: 1,
        };
        let size = unsafe { tn_endpoint_transport_required_bytes(&raw const config) };
        // Darwin MAP_ANON|MAP_SHARED; prepare every allocation before fork.
        let memory = unsafe { mmap(ptr::null_mut(), size, 3, 0x1001, -1, 0) };
        assert_ne!(
            memory as isize,
            -1,
            "mmap failed: {}",
            std::io::Error::last_os_error()
        );
        let result = Self {
            memory: NonNull::new(memory).unwrap(),
            size,
        };
        assert!(
            !unsafe { tn_endpoint_transport_initialize(memory, size, &raw const config) }.is_null()
        );
        result
    }
    fn transport(&self) -> *mut crate::endpoint_transport::EndpointTransport {
        self.memory.as_ptr().cast()
    }
    fn render(&self) -> *mut SharedAudioBuffer {
        unsafe { crate::endpoint_transport::tn_endpoint_transport_render_buffer(self.transport()) }
    }
    fn capture(&self) -> *mut SharedAudioBuffer {
        unsafe { crate::endpoint_transport::tn_endpoint_transport_capture_buffer(self.transport()) }
    }
}
impl Drop for DuplexMapping {
    fn drop(&mut self) {
        // SAFETY: child is reaped and every borrowed HAL context has unbound.
        assert_eq!(unsafe { munmap(self.memory.as_ptr(), self.size) }, 0);
    }
}

/// Owns only the exact child PID returned by fork. Cleanup is bounded and never
/// signals a user process, a process group, or an already-reaped/reused PID.
struct ClaimChild(Option<i32>);
impl ClaimChild {
    fn spawn(buffer: *mut SharedAudioBuffer) -> Self {
        let pid = unsafe { fork() };
        assert!(pid >= 0, "fork failed: {}", std::io::Error::last_os_error());
        if pid == 0 {
            // SAFETY: inherited live MAP_SHARED bytes were initialized before
            // fork. This branch performs only lock-free atomic claim and _exit:
            // no allocation, XPC, printing, unwinding or Rust destructors run.
            let claim = unsafe { &*buffer }.claim_producer();
            if let Some(claim) = claim {
                std::mem::forget(claim);
                unsafe { _exit(0) }
            }
            unsafe { _exit(2) }
        }
        Self(Some(pid))
    }
    fn poll(&mut self) -> Option<i32> {
        let pid = self.0?;
        let mut status = 0;
        let result = unsafe { waitpid(pid, &raw mut status, 1) }; // WNOHANG.
        if result == pid {
            self.0 = None;
            return Some(status);
        }
        if result < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                // ECHILD means no longer ours to signal; other errors also must
                // not turn an unverified PID into a cleanup target.
                self.0 = None;
                panic!("waitpid failed: {error}");
            }
        }
        None
    }
    fn wait(&mut self) -> i32 {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Some(status) = self.poll() {
                return status;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("owned claim child did not exit within five seconds");
    }
}
impl Drop for ClaimChild {
    fn drop(&mut self) {
        if let Some(pid) = self.0 {
            // This unreaped exact child cannot have its PID reassigned yet.
            unsafe {
                kill(pid, 9);
            }
            let deadline = Instant::now() + Duration::from_secs(5);
            while self.0.is_some() && Instant::now() < deadline {
                if self.poll().is_some() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(
                self.0.is_none(),
                "failed to reap owned claim child after SIGKILL"
            );
        }
    }
}

/// Owns raw aligned storage, never a non-atomic Rust slice alias to live PCM.
struct Buffer {
    memory: NonNull<u8>,
    layout: Layout,
    pointer: *mut SharedAudioBuffer,
}

// SAFETY: immutable geometry and atomic shared cells are the only accesses after
// construction. Tests use exactly one PCM writer; scoped threads join before Drop.
unsafe impl Sync for Buffer {}

impl Buffer {
    fn new(capacity: u32, channels: u32) -> Self {
        let size = tn_shared_audio_buffer_required_bytes(capacity, channels);
        let layout = Layout::from_size_align(size, align_of::<SharedAudioBuffer>()).unwrap();
        let memory = NonNull::new(unsafe { alloc_zeroed(layout) })
            .unwrap_or_else(|| handle_alloc_error(layout));
        // SAFETY: this allocation is exclusive, aligned and exactly large enough.
        let pointer = unsafe {
            tn_shared_audio_buffer_initialize(memory.as_ptr().cast(), size, capacity, channels)
        };
        assert!(!pointer.is_null());
        Self {
            memory,
            layout,
            pointer,
        }
    }
    fn header(&self) -> &SharedAudioBuffer {
        // SAFETY: header lives until this owner drops, geometry is immutable.
        unsafe { &*self.pointer }
    }
    fn write(&self, start: u64, samples: &[f32]) -> bool {
        let channels = self.header().channels as usize;
        assert_eq!(samples.len() % channels, 0);
        unsafe {
            tn_shared_audio_buffer_write(
                self.pointer,
                self.stats().epoch,
                start,
                samples.as_ptr(),
                u32::try_from(samples.len() / channels).unwrap(),
            )
        }
    }
    fn read(&self, start: u64, samples: &mut [f32]) -> u32 {
        let channels = self.header().channels as usize;
        assert_eq!(samples.len() % channels, 0);
        unsafe {
            tn_shared_audio_buffer_read(
                self.pointer,
                start,
                samples.as_mut_ptr(),
                u32::try_from(samples.len() / channels).unwrap(),
            )
        }
    }
    fn stats(&self) -> SharedAudioBufferStats {
        unsafe { tn_shared_audio_buffer_stats(self.pointer) }
    }
}
impl Drop for Buffer {
    fn drop(&mut self) {
        // SAFETY: every user/thread borrows this owner and has finished. No live
        // atomic references survive; allocation and deallocation layouts match.
        unsafe {
            dealloc(self.memory.as_ptr(), self.layout);
        }
    }
}

#[test]
fn abi_layout_and_geometry_are_bounded() {
    assert_eq!(size_of::<SharedAudioBuffer>(), 80);
    assert_eq!(size_of::<SharedAudioBufferStats>(), 40);
    assert_eq!(
        std::mem::offset_of!(SharedAudioBuffer, published_through_frame),
        64
    );
    assert_eq!(tn_shared_audio_buffer_required_bytes(4, 2), 144);
    assert_ne!(
        tn_shared_audio_buffer_required_bytes(MAX_CAPACITY, MAX_CHANNELS),
        0
    );
    for (capacity, channels) in [(0, 2), (3, 2), (MAX_CAPACITY * 2, 2), (4, 0), (4, 65)] {
        assert_eq!(tn_shared_audio_buffer_required_bytes(capacity, channels), 0);
    }
}

#[test]
fn initialization_and_attach_reject_invalid_or_old_geometry() {
    let buffer = Buffer::new(8, 2);
    let memory = buffer.memory.as_ptr().cast();
    let size = buffer.layout.size();
    // SAFETY: all tested pointers are either null or inside the owned allocation.
    // Corruptions are exclusive and restored before any PCM/atomic user resumes.
    unsafe {
        assert!(tn_shared_audio_buffer_initialize(ptr::null_mut(), size, 8, 2).is_null());
        assert!(tn_shared_audio_buffer_initialize(memory, size - 1, 8, 2).is_null());
        assert!(
            tn_shared_audio_buffer_initialize(buffer.memory.as_ptr().add(1).cast(), size - 1, 8, 2)
                .is_null()
        );
        assert_eq!(tn_shared_audio_buffer_attach(memory, size), buffer.pointer);
        assert!(tn_shared_audio_buffer_attach(memory, size - 1).is_null());
        assert!(tn_shared_audio_buffer_attach(memory, 71).is_null());
        assert!(tn_shared_audio_buffer_attach(ptr::null_mut(), size).is_null());
        (*buffer.pointer).version = 2;
        assert!(tn_shared_audio_buffer_attach(memory, size).is_null());
        (*buffer.pointer).version = VERSION;
        (*buffer.pointer).capacity_mask = 0;
        assert!(tn_shared_audio_buffer_attach(memory, size).is_null());
        (*buffer.pointer).capacity_mask = 7;
        (*buffer.pointer).magic = 0;
        assert!(tn_shared_audio_buffer_attach(memory, size).is_null());
        (*buffer.pointer).magic = MAGIC;
        assert_eq!(tn_shared_audio_buffer_attach(memory, size), buffer.pointer);
    }
}

#[test]
fn pcm_bits_activity_and_counters_roundtrip() {
    let buffer = Buffer::new(8, 2);
    let bits = [0x8000_0000, 0x7fc0_1234, 0x3f00_0000, 0xbf80_0000];
    let input = bits.map(f32::from_bits);
    assert!(buffer.write(100, &input));
    let mut output = [0.0; 4];
    assert_eq!(buffer.read(100, &mut output), 2);
    assert_eq!(output.map(f32::to_bits), bits);
    assert_eq!(
        buffer.stats(),
        SharedAudioBufferStats {
            written_frames: 2,
            read_frames: 2,
            missing_frames: 0,
            epoch: 1,
            published_through_frame: 102
        }
    );
    unsafe {
        assert!(tn_shared_audio_buffer_is_lock_free(buffer.pointer));
        assert_eq!(tn_shared_audio_buffer_capacity(buffer.pointer), 8);
        assert_eq!(tn_shared_audio_buffer_channels(buffer.pointer), 2);
        for activity in 0..=3 {
            tn_shared_audio_buffer_set_producer_activity(buffer.pointer, activity);
            assert_eq!(
                tn_shared_audio_buffer_producer_activity(buffer.pointer),
                activity
            );
        }
        tn_shared_audio_buffer_set_producer_activity(buffer.pointer, 99);
        assert_eq!(tn_shared_audio_buffer_producer_activity(buffer.pointer), 0);
        assert_eq!(
            tn_shared_audio_buffer_stats(ptr::null()),
            SharedAudioBufferStats::default()
        );
    }
}

#[test]
fn ring_overwrite_silences_only_missing_absolute_frames() {
    let buffer = Buffer::new(4, 1);
    assert!(buffer.write(0, &[1.0, 2.0, 3.0, 4.0]));
    assert!(buffer.write(4, &[5.0, 6.0]));
    let mut output = [9.0; 4];
    assert_eq!(buffer.read(0, &mut output), 2);
    assert_eq!(
        output.map(f32::to_bits),
        [0.0_f32, 0.0, 3.0, 4.0].map(f32::to_bits)
    );
    assert_eq!(buffer.stats().missing_frames, 2);
}

#[test]
fn pending_request_is_idempotent_and_blocks_old_pcm_until_writer_rearms() {
    let buffer = Buffer::new(8, 1);
    assert!(buffer.write(100, &[0.5; 4]));
    unsafe {
        assert_eq!(tn_shared_audio_buffer_request_epoch(buffer.pointer), 1);
        assert_eq!(tn_shared_audio_buffer_request_epoch(buffer.pointer), 1);
        assert!(tn_shared_audio_buffer_epoch_pending(buffer.pointer));
        assert!(!tn_shared_audio_buffer_requires_replacement(buffer.pointer));
        assert_eq!(tn_shared_audio_buffer_published_through(buffer.pointer), 0);
    }
    assert_eq!(buffer.stats().epoch, 0);
    assert!(!buffer.write(104, &[1.0]));
    let mut output = [9.0; 4];
    assert_eq!(buffer.read(100, &mut output), 0);
    assert!(output.iter().all(|sample| sample.to_bits() == 0));
    unsafe {
        assert_eq!(tn_shared_audio_buffer_try_begin_epoch(buffer.pointer), 2);
        assert!(!tn_shared_audio_buffer_epoch_pending(buffer.pointer));
    }
    assert_eq!(buffer.read(100, &mut output), 0);
    assert!(buffer.write(0, &[0.25; 4]));
    assert_eq!(buffer.read(0, &mut output), 4);
    assert!(
        output
            .iter()
            .all(|sample| sample.to_bits() == 0.25_f32.to_bits())
    );
}

#[test]
fn exhausted_mapping_never_reuses_a_remote_readers_generation() {
    let old = Buffer::new(4, 1);
    assert!(old.write(0, &[0.75]));
    for expected in 2..=MAX_EPOCH {
        assert_eq!(
            unsafe { tn_shared_audio_buffer_try_begin_epoch(old.pointer) },
            expected
        );
    }
    assert_eq!(
        unsafe { tn_shared_audio_buffer_try_begin_epoch(old.pointer) },
        0
    );
    assert_eq!(
        unsafe { tn_shared_audio_buffer_try_begin_epoch(old.pointer) },
        0
    );
    assert!(unsafe { tn_shared_audio_buffer_epoch_pending(old.pointer) });
    assert!(unsafe { tn_shared_audio_buffer_requires_replacement(old.pointer) });
    // The generation-one stamp is not cleared or reused in this live mapping.
    assert_eq!(
        unsafe { old.header().stamp_cell(0) }.load(Ordering::Acquire),
        stamp(1, 0)
    );
    let fresh = Buffer::new(4, 1);
    let mut output = [1.0];
    assert_eq!(fresh.read(0, &mut output), 0);
    assert_eq!(output[0].to_bits(), 0);
    assert_eq!(fresh.stats().epoch, 1);
    assert!(!unsafe { tn_shared_audio_buffer_requires_replacement(fresh.pointer) });
    assert_eq!(old.read(0, &mut output), 0);
}

#[test]
fn replacement_signal_distinguishes_pending_rearm_from_exhaustion() {
    let buffer = Buffer::new(4, 1);
    assert!(!unsafe { tn_shared_audio_buffer_requires_replacement(ptr::null()) });
    assert!(!unsafe { tn_shared_audio_buffer_requires_replacement(buffer.pointer) });
    // Deterministic boundary setup uses only an atomic store; no PCM writer or
    // reader is active while this test moves the generation near exhaustion.
    buffer
        .header()
        .epoch
        .store(MAX_EPOCH - 1, Ordering::Release);
    unsafe {
        assert_eq!(
            tn_shared_audio_buffer_request_epoch(buffer.pointer),
            MAX_EPOCH - 1
        );
        assert!(tn_shared_audio_buffer_epoch_pending(buffer.pointer));
        assert!(!tn_shared_audio_buffer_requires_replacement(buffer.pointer));
        assert_eq!(
            tn_shared_audio_buffer_try_begin_epoch(buffer.pointer),
            MAX_EPOCH
        );
        assert!(!tn_shared_audio_buffer_epoch_pending(buffer.pointer));
        assert!(!tn_shared_audio_buffer_requires_replacement(buffer.pointer));
        assert_eq!(
            tn_shared_audio_buffer_request_epoch(buffer.pointer),
            MAX_EPOCH
        );
        assert!(tn_shared_audio_buffer_requires_replacement(buffer.pointer));
        assert_eq!(tn_shared_audio_buffer_try_begin_epoch(buffer.pointer), 0);
        assert_eq!(tn_shared_audio_buffer_try_begin_epoch(buffer.pointer), 0);
        assert!(tn_shared_audio_buffer_requires_replacement(buffer.pointer));
    }
    assert_eq!(
        buffer.header().epoch.load(Ordering::Acquire),
        REQUESTED | MAX_EPOCH
    );
}

#[test]
fn stale_writer_watermark_cannot_become_a_new_epoch_timeline() {
    let buffer = Buffer::new(4, 1);
    assert!(buffer.write(100, &[1.0]));
    assert_eq!(
        unsafe { tn_shared_audio_buffer_try_begin_epoch(buffer.pointer) },
        2
    );
    // Deterministic interleaving: an old writer reaches publication after rearm.
    buffer
        .header()
        .published_through_frame
        .store(stamp(1, 101), Ordering::Release);
    assert_eq!(buffer.stats().epoch, 2);
    assert_eq!(buffer.stats().published_through_frame, 0);
    assert_eq!(
        unsafe { tn_shared_audio_buffer_published_through(buffer.pointer) },
        0
    );
    assert!(buffer.write(0, &[2.0]));
    assert_eq!(buffer.stats().published_through_frame, 1);
}

#[test]
fn rearm_after_observation_rejects_old_format_pcm_before_publication() {
    let buffer = Buffer::new(4, 1);
    let observed_epoch = buffer.stats().epoch;
    assert_eq!(
        unsafe { tn_shared_audio_buffer_try_begin_epoch(buffer.pointer) },
        2
    );
    let old_format_samples = [0.75; 2];
    assert!(!unsafe {
        tn_shared_audio_buffer_write(
            buffer.pointer,
            observed_epoch,
            0,
            old_format_samples.as_ptr(),
            2,
        )
    });
    assert_eq!(buffer.stats().written_frames, 0);
    assert_eq!(buffer.stats().published_through_frame, 0);
    let mut output = [1.0; 2];
    assert_eq!(buffer.read(0, &mut output), 0);
    assert!(output.iter().all(|sample| sample.to_bits() == 0));
    assert!(
        buffer.write(0, &[0.25; 2]),
        "failed expected-epoch check releases its producer claim"
    );
    assert_eq!(buffer.read(0, &mut output), 2);
    assert!(
        output
            .iter()
            .all(|sample| sample.to_bits() == 0.25_f32.to_bits())
    );
}

#[test]
fn active_producer_defers_control_rearm_without_exposing_a_new_epoch() {
    let buffer = Buffer::new(4, 1);
    let observed_epoch = buffer.stats().epoch;
    // Deterministically hold the same RAII claim used inside production writes.
    // Control must invalidate, not advance, while that producer is still active.
    let producer = buffer.header().claim_producer().unwrap();
    assert_eq!(
        unsafe { tn_shared_audio_buffer_try_begin_epoch(buffer.pointer) },
        0
    );
    assert_eq!(
        buffer.header().epoch.load(Ordering::Acquire),
        REQUESTED | observed_epoch
    );
    assert_eq!(buffer.stats().epoch, 0);
    assert!(!unsafe {
        tn_shared_audio_buffer_write(buffer.pointer, observed_epoch, 0, [1.0].as_ptr(), 1)
    });
    drop(producer);
    assert_eq!(
        unsafe { tn_shared_audio_buffer_try_begin_epoch(buffer.pointer) },
        2
    );
    assert!(!unsafe {
        tn_shared_audio_buffer_write(buffer.pointer, observed_epoch, 0, [1.0].as_ptr(), 1)
    });
    assert!(buffer.write(0, &[0.5]));
    assert_eq!(buffer.stats().written_frames, 1);
}

#[test]
fn rejected_write_paths_release_the_claim_and_never_accept_inactive_epoch() {
    let buffer = Buffer::new(4, 1);
    for expected_epoch in [0, REQUESTED | 1, MAX_EPOCH + 1, 2] {
        assert!(!unsafe {
            tn_shared_audio_buffer_write(buffer.pointer, expected_epoch, 0, [1.0].as_ptr(), 1)
        });
        assert_eq!(buffer.header().producer_busy.load(Ordering::Acquire), 0);
    }
    assert!(buffer.write(0, &[0.5]));
    assert!(!buffer.write(0, &[0.75]));
    assert_eq!(buffer.header().producer_busy.load(Ordering::Acquire), 0);
    assert_eq!(
        unsafe { tn_shared_audio_buffer_try_begin_epoch(buffer.pointer) },
        2
    );
}

#[test]
fn exited_producer_is_retired_by_drained_hal_start_and_only_new_mapping_recovers() {
    use crate::hal_io::{
        HalIoContext, tn_hal_io_begin_stream, tn_hal_io_bind, tn_hal_io_requires_replacement,
        tn_hal_io_unbind, tn_hal_io_write_render,
    };
    let old = DuplexMapping::new();
    let render = old.render();
    let mut child = ClaimChild::spawn(render);
    assert_eq!(
        child.wait(),
        0,
        "child must exit after acquiring its real claim"
    );
    assert_eq!(unsafe { &*render }.producer_busy.load(Ordering::Acquire), 1);
    let mut restarted_hal = HalIoContext::new(1, 1).unwrap();
    unsafe {
        assert!(tn_hal_io_bind(
            &raw mut restarted_hal,
            old.transport().cast()
        ));
        // Actual native caller authority: the old producer process has exited,
        // and this replacement context has never admitted any IO callbacks.
        assert!(!tn_hal_io_begin_stream(&raw mut restarted_hal));
        assert!(tn_hal_io_requires_replacement(&raw mut restarted_hal));
        assert!(tn_shared_audio_buffer_requires_replacement(render));
        assert_eq!(tn_shared_audio_buffer_try_begin_epoch(render), 0);
        assert!(!tn_shared_audio_buffer_write(
            render,
            1,
            0,
            [1.0].as_ptr(),
            1
        ));
        assert_eq!(
            (&*render).producer_busy.load(Ordering::Acquire),
            1,
            "retirement must never clear an abandoned claim"
        );
        tn_hal_io_unbind(&raw mut restarted_hal);
        let fresh = DuplexMapping::new();
        assert!(tn_hal_io_bind(
            &raw mut restarted_hal,
            fresh.transport().cast()
        ));
        assert!(tn_hal_io_begin_stream(&raw mut restarted_hal));
        assert!(!tn_hal_io_requires_replacement(&raw mut restarted_hal));
        assert!(tn_hal_io_write_render(
            &raw mut restarted_hal,
            0,
            [0.25].as_ptr(),
            1
        ));
        tn_hal_io_unbind(&raw mut restarted_hal);
    }
}

#[test]
fn hal_start_never_retires_the_remote_capture_writer_on_contention() {
    use crate::hal_io::{
        HalIoContext, tn_hal_io_begin_stream, tn_hal_io_bind, tn_hal_io_requires_replacement,
        tn_hal_io_unbind,
    };
    let mapping = DuplexMapping::new();
    let capture = unsafe { &*mapping.capture() };
    let remote_writer = capture.claim_producer().unwrap();
    let mut hal = HalIoContext::new(1, 1).unwrap();
    unsafe {
        assert!(tn_hal_io_bind(&raw mut hal, mapping.transport().cast()));
        assert!(tn_hal_io_begin_stream(&raw mut hal));
        assert!(!tn_hal_io_requires_replacement(&raw mut hal));
        assert!(!tn_shared_audio_buffer_requires_replacement(
            mapping.capture()
        ));
        assert!(tn_shared_audio_buffer_epoch_pending(mapping.capture()));
        tn_hal_io_unbind(&raw mut hal);
    }
    drop(remote_writer);
    assert_eq!(
        unsafe { tn_shared_audio_buffer_try_begin_epoch(mapping.capture()) },
        2
    );
    assert!(!unsafe { tn_shared_audio_buffer_retire_busy_producer_after_drain(mapping.capture()) });
    assert!(!unsafe { tn_shared_audio_buffer_retire_busy_producer_after_drain(ptr::null_mut()) });
}

#[test]
fn packed_end_overflow_and_same_epoch_stamp_aba_are_rejected() {
    let buffer = Buffer::new(4, 1);
    assert!(buffer.write(10, &[1.0, 2.0]));
    assert!(!buffer.write(10, &[9.0]));
    assert!(!buffer.write(11, &[9.0, 9.0]));
    let mut output = [0.0; 2];
    assert_eq!(buffer.read(10, &mut output), 2);
    assert_eq!(output.map(f32::to_bits), [1.0_f32, 2.0].map(f32::to_bits));
    assert!(buffer.write(FRAME_MASK - 1, &[3.0]));
    assert_eq!(buffer.stats().published_through_frame, FRAME_MASK);
    assert!(!buffer.write(FRAME_MASK, &[4.0]));
    assert!(!buffer.write(u64::MAX, &[4.0, 5.0]));
    assert_eq!(buffer.read(FRAME_MASK, &mut output[..1]), 0);
    unsafe {
        assert!(tn_shared_audio_buffer_write(
            buffer.pointer,
            buffer.stats().epoch,
            u64::MAX,
            ptr::null(),
            0
        ));
        assert_eq!(
            tn_shared_audio_buffer_read(buffer.pointer, u64::MAX, ptr::null_mut(), 0),
            0
        );
    }
}

#[test]
fn concurrent_ring_replacement_never_returns_torn_multichannel_pcm() {
    const CHANNELS: usize = 8;
    const FRAMES: u32 = 20_000;
    let buffer = Buffer::new(64, u32::try_from(CHANNELS).unwrap());
    std::thread::scope(|scope| {
        scope.spawn(|| {
            for frame in 0..FRAMES {
                // Exact bit patterns avoid float rounding ambiguity in this test.
                let samples = std::array::from_fn::<_, CHANNELS, _>(|channel| {
                    f32::from_bits(0x3f00_0000 + frame * 16 + u32::try_from(channel).unwrap())
                });
                assert!(buffer.write(u64::from(frame), &samples));
            }
        });
        for _ in 0..FRAMES {
            let end = unsafe { tn_shared_audio_buffer_published_through(buffer.pointer) };
            if end == 0 {
                continue;
            }
            let frame = end - 1;
            let mut samples = [0.0; CHANNELS];
            let available = buffer.read(frame, &mut samples);
            for (channel, sample) in samples.iter().enumerate() {
                let expected = if available == 0 {
                    0
                } else {
                    0x3f00_0000
                        + u32::try_from(frame).unwrap() * 16
                        + u32::try_from(channel).unwrap()
                };
                assert_eq!(sample.to_bits(), expected);
            }
        }
    });
    assert_eq!(buffer.stats().written_frames, u64::from(FRAMES));
}
