//! Application producer/consumer clock contract. All mutable fields are atomic;
//! no Rust references to non-atomic interprocess-writable data are exposed.
use crate::shared_audio_buffer::tn_shared_audio_buffer_required_bytes;
use std::{
    ffi::c_void,
    mem::size_of,
    sync::atomic::{AtomicU32, AtomicU64, Ordering},
};

pub const VERSION: u32 = 2;
const MAGIC: u64 = 0x544e_4150_5043_4c4b;
pub const FRAME_MASK: u64 = 0x0000_ffff_ffff_ffff;

#[repr(C)]
pub struct ApplicationClock {
    magic: u64,
    version: u32,
    header_bytes: u32,
    pub format_word: AtomicU64,
    pub acknowledged_format: AtomicU64,
    pub consumer_position: AtomicU64,
    pub consumer_ticks: AtomicU64,
    pub consumer_running: AtomicU32,
    pub native_sample_rate: AtomicU32,
    pub consumer_sequence: AtomicU64,
    pub producer_guard: AtomicU32,
    pub native_frames: AtomicU64,
    pub native_running: AtomicU32,
    pub clock_master: AtomicU32,
    pub producer_lease: AtomicU64,
}

fn offset(capacity: u32, channels: u32) -> Option<usize> {
    let bytes = tn_shared_audio_buffer_required_bytes(capacity, channels);
    (bytes != 0)
        .then_some(bytes)?
        .checked_add(63)
        .map(|value| value & !63)
}

#[unsafe(no_mangle)]
pub extern "C" fn tn_application_clock_required_bytes(capacity: u32, channels: u32) -> usize {
    offset(capacity, channels)
        .and_then(|value| value.checked_add(size_of::<ApplicationClock>()))
        .unwrap_or(0)
}

/// # Safety
/// Memory is a live mapping of `size` bytes; its immutable geometry is not mutated.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tn_application_clock_attach(
    memory: *mut c_void,
    size: usize,
    capacity: u32,
    channels: u32,
) -> *mut ApplicationClock {
    let Some(offset) = offset(capacity, channels) else {
        return std::ptr::null_mut();
    };
    if memory.is_null()
        || size < offset
        || size - offset < size_of::<ApplicationClock>()
        || (memory as usize)
            .checked_add(offset)
            .is_none_or(|address| address % 64 != 0)
    {
        return std::ptr::null_mut();
    }
    // The checks above establish a 64-byte-aligned address inside this mapping.
    let clock = unsafe { memory.byte_add(offset).cast::<ApplicationClock>() };
    if unsafe {
        (*clock).magic != MAGIC
            || (*clock).version != VERSION
            || usize::try_from((*clock).header_bytes).ok() != Some(size_of::<ApplicationClock>())
    } {
        return std::ptr::null_mut();
    }
    clock
}

/// # Safety
/// Memory is exclusively owned, writable, and `size` bytes long until publication.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tn_application_clock_initialize(
    memory: *mut c_void,
    size: usize,
    capacity: u32,
    channels: u32,
) -> *mut ApplicationClock {
    let required = tn_application_clock_required_bytes(capacity, channels);
    let Some(offset) = offset(capacity, channels) else {
        return std::ptr::null_mut();
    };
    if memory.is_null()
        || required == 0
        || size < required
        || (memory as usize)
            .checked_add(offset)
            .is_none_or(|address| address % 64 != 0)
    {
        return std::ptr::null_mut();
    }
    let Ok(header_bytes) = u32::try_from(size_of::<ApplicationClock>()) else {
        return std::ptr::null_mut();
    };
    // The checks above establish aligned, writable storage for the whole clock.
    let clock = unsafe { memory.byte_add(offset).cast::<ApplicationClock>() };
    unsafe {
        clock.write(ApplicationClock {
            magic: MAGIC,
            version: VERSION,
            header_bytes,
            format_word: AtomicU64::new(0),
            acknowledged_format: AtomicU64::new(0),
            consumer_position: AtomicU64::new(0),
            consumer_ticks: AtomicU64::new(0),
            consumer_running: AtomicU32::new(0),
            native_sample_rate: AtomicU32::new(0),
            consumer_sequence: AtomicU64::new(0),
            producer_guard: AtomicU32::new(0),
            native_frames: AtomicU64::new(0),
            native_running: AtomicU32::new(0),
            clock_master: AtomicU32::new(0),
            producer_lease: AtomicU64::new(0),
        });
    }
    clock
}

/// # Safety
/// Clock is null or points to a live initialized application clock.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tn_application_clock_try_producer(clock: *mut ApplicationClock) -> bool {
    unsafe { clock.as_ref() }.is_some_and(|clock| {
        clock
            .producer_guard
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    })
}

/// # Safety
/// Caller owns the producer claim on this live clock.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tn_application_clock_release_producer(clock: *mut ApplicationClock) {
    if let Some(clock) = unsafe { clock.as_ref() } {
        clock.producer_guard.store(0, Ordering::Release);
    }
}

/// # Safety
/// Clock is null or live. The caller is the single claimed consumer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tn_application_clock_request_rate(
    clock: *mut ApplicationClock,
    rate: u32,
) -> u64 {
    let Some(clock) = (unsafe { clock.as_ref() }) else {
        return 0;
    };
    if !(8000..=192_000).contains(&rate) {
        return 0;
    }
    let current = clock.format_word.load(Ordering::Acquire);
    if current & u64::from(u32::MAX) == u64::from(rate) {
        return current;
    }
    let generation = (current >> 32) + 1;
    // A format identity is never reused. The registry replaces this mapping
    // when its producer next attaches after terminal format generation.
    if generation > u64::from(u32::MAX) {
        return 0;
    }
    let next = (generation << 32) | u64::from(rate);
    clock.consumer_running.store(0, Ordering::Release);
    clock.format_word.store(next, Ordering::Release);
    next
}

/// # Safety
/// Clock is null or live. The caller is the single claimed consumer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tn_application_clock_publish_consumer(
    clock: *mut ApplicationClock,
    epoch: u64,
    next_frame: u64,
    frames: u32,
    running: bool,
) {
    let Some(clock) = (unsafe { clock.as_ref() }) else {
        return;
    };
    clock.consumer_sequence.fetch_add(1, Ordering::AcqRel);
    clock
        .consumer_position
        .store((epoch << 48) | (next_frame & FRAME_MASK), Ordering::Release);
    clock
        .consumer_ticks
        .fetch_add(u64::from(frames), Ordering::Release);
    clock
        .consumer_running
        .store(u32::from(running), Ordering::Release);
    clock.consumer_sequence.fetch_add(1, Ordering::Release);
}
