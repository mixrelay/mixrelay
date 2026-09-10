//! Application-source mapping ownership and single-consumer clock policy.
//! Control publishes immutable slot pointers; callbacks only claim atomics.
//! Old producer mappings stay isolated after replacement and are reclaimed on
//! control after local readers drain, never on an audio callback.
use crate::{
    application_clock::{
        ApplicationClock, VERSION, tn_application_clock_publish_consumer,
        tn_application_clock_request_rate,
    },
    application_source_mapping::Mapping,
    shared_audio_buffer::{
        SharedAudioBufferStats, tn_shared_audio_buffer_read, tn_shared_audio_buffer_request_epoch,
        tn_shared_audio_buffer_requires_replacement, tn_shared_audio_buffer_stats,
        tn_shared_audio_buffer_try_begin_epoch,
    },
    source_control::SourceRequest,
};
use std::{
    cell::UnsafeCell,
    ffi::c_char,
    io,
    marker::PhantomPinned,
    os::fd::{IntoRawFd, OwnedFd},
    pin::Pin,
    ptr,
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering},
    },
};

const MAX_SOURCES: usize = 64;
const MAX_RETAINED: usize = 128;
const READ_LATENCY: u64 = 512;
const MAX_LEASE: u64 = (1 << 48) - 1;

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct SourceClock {
    /// Opaque lease/stream identity, not a raw shared-buffer epoch.
    pub epoch: u64,
    pub native_frames: u64,
    pub native_rate: u32,
    pub graph_rate: u32,
    pub running: u32,
    pub synchronized: u32,
}
#[derive(Default)]
struct Timeline {
    epoch: u64,
    next: u64,
    started: bool,
}
impl Timeline {
    /// Select a readable range without advancing the single consumer cursor.
    /// The caller publishes an inactive consumer when the range is unavailable.
    fn ready(&mut self, stats: SharedAudioBufferStats, frames: u32, capacity: u32) -> bool {
        if self.epoch != stats.epoch {
            self.epoch = stats.epoch;
            self.started = false;
        }
        let published = stats.published_through_frame;
        if !self.started {
            self.next = 0;
            if published < READ_LATENCY + u64::from(frames) {
                return false;
            }
            self.next = published - READ_LATENCY;
            self.started = true;
        }
        if published > self.next.saturating_add(u64::from(capacity)) {
            self.next = published - READ_LATENCY;
        }
        published >= self.next.saturating_add(u64::from(frames))
    }
}
struct Source {
    id: Vec<u8>,
    token: u64,
    capacity: u32,
    channels: u32,
    mapping: Mapping,
    consumer_busy: AtomicBool,
    timeline: UnsafeCell<Timeline>,
    // Published slot addresses cannot change while any callback retains a pin.
    _pinned: PhantomPinned,
}
// SAFETY: immutable fields and mapping atomics are shared. Only a successful
// consumer_busy claim permits timeline mutation; failed claims never wait.
unsafe impl Send for Source {}
unsafe impl Sync for Source {}
struct Consumer<'a>(&'a Source);
impl Drop for Consumer<'_> {
    fn drop(&mut self) {
        self.0.consumer_busy.store(false, Ordering::Release);
    }
}
struct Producer<'a>(&'a ApplicationClock);
impl Drop for Producer<'_> {
    fn drop(&mut self) {
        self.0.producer_guard.store(0, Ordering::Release);
    }
}
impl Source {
    /// Complete fallible allocation before changing either producer lease.
    fn grant(&self, lease: u64, previous: Option<&Self>) -> io::Result<Attachment> {
        // Duplicate before any lease/epoch mutation: XPC construction and an
        // immediate subsequent replacement cannot invalidate this owned grant.
        let descriptor = self.mapping.descriptor.try_clone()?;
        let epoch = unsafe { tn_shared_audio_buffer_try_begin_epoch(self.mapping.buffer.as_ptr()) };
        if epoch == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "source stream could not rearm",
            ));
        }
        let clock = self.mapping.clock();
        if let Some(old) = previous {
            let old_clock = old.mapping.clock();
            let rate =
                u32::try_from(old_clock.format_word.load(Ordering::Acquire) & u64::from(u32::MAX))
                    .expect("masked rate fits u32");
            if rate != 0 {
                unsafe {
                    tn_application_clock_request_rate(self.mapping.clock.as_ptr(), rate);
                }
            }
            clock.clock_master.store(
                old_clock.clock_master.load(Ordering::Acquire),
                Ordering::Release,
            );
            old_clock.acknowledged_format.store(0, Ordering::Release);
            old_clock.native_running.store(0, Ordering::Release);
            old_clock.producer_lease.store(0, Ordering::Release);
            // SAFETY: attach holds this source's producer claim and its mapping
            // remains pinned. Revoke old PCM as well as the lease before publishing
            // the replacement, including an active (not yet requested) MAX epoch.
            unsafe {
                tn_shared_audio_buffer_request_epoch(old.mapping.buffer.as_ptr());
            }
        }
        clock.acknowledged_format.store(0, Ordering::Release);
        clock.native_running.store(0, Ordering::Release);
        clock.producer_lease.store(lease, Ordering::Release);
        Ok(Attachment {
            descriptor,
            mapped_size: self.mapping.bytes,
            producer_lease: lease,
            source_epoch: epoch,
        })
    }

    fn claim_consumer(&self) -> Option<Consumer<'_>> {
        self.consumer_busy
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| Consumer(self))
    }
    fn claim_producer(&self) -> Option<Producer<'_>> {
        let clock = self.mapping.clock();
        clock
            .producer_guard
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| Producer(clock))
    }
    fn terminal(&self) -> bool {
        (unsafe { tn_shared_audio_buffer_requires_replacement(self.mapping.buffer.as_ptr()) })
            || unsafe { tn_shared_audio_buffer_stats(self.mapping.buffer.as_ptr()) }.epoch
                == u64::from(u16::MAX)
            || self.mapping.clock().format_word.load(Ordering::Acquire) >> 32 == u64::from(u32::MAX)
    }
}
struct Control {
    owners: Vec<Pin<Box<Source>>>,
    next_lease: u64,
}
pub struct SourceRegistry {
    slots: [AtomicPtr<Source>; MAX_SOURCES],
    readers: AtomicUsize,
    control: Mutex<Control>,
}
struct Read<'a> {
    registry: &'a SourceRegistry,
    source: *const Source,
}
impl Read<'_> {
    fn source(&self) -> Option<&Source> {
        unsafe { self.source.as_ref() }
    }
}
impl Drop for Read<'_> {
    fn drop(&mut self) {
        self.registry.readers.fetch_sub(1, Ordering::SeqCst);
    }
}

pub struct Attachment {
    pub descriptor: OwnedFd,
    pub mapped_size: usize,
    pub producer_lease: u64,
    pub source_epoch: u64,
}
impl Default for SourceRegistry {
    fn default() -> Self {
        Self::new()
    }
}
impl SourceRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            slots: std::array::from_fn(|_| AtomicPtr::new(ptr::null_mut())),
            readers: AtomicUsize::new(0),
            control: Mutex::new(Control {
                owners: Vec::new(),
                next_lease: 1,
            }),
        }
    }
    fn control(&self) -> MutexGuard<'_, Control> {
        self.control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn collect(&self, control: &mut Control) {
        if self.readers.load(Ordering::SeqCst) == 0 {
            control.owners.retain(|owner| {
                self.slots
                    .iter()
                    .any(|slot| ptr::eq(slot.load(Ordering::SeqCst), owner.as_ref().get_ref()))
            });
        }
    }
    fn read(&self, token: u64) -> Read<'_> {
        self.readers.fetch_add(1, Ordering::SeqCst);
        let source = self
            .slots
            .iter()
            .find_map(|slot| {
                let source = slot.load(Ordering::SeqCst);
                // The reader increment pins every pointer before inspection.
                unsafe { source.as_ref() }
                    .filter(|source| source.token == token)
                    .map(ptr::from_ref)
            })
            .unwrap_or(ptr::null());
        Read {
            registry: self,
            source,
        }
    }
    /// # Errors
    /// Invalid geometry/identity, exhausted leases, busy producers, allocation
    /// failure, or bounded retirement pressure reject without publishing state.
    pub fn attach(&self, request: &SourceRequest) -> io::Result<Attachment> {
        let id = validated_id(request)?;
        let token = token(&id);
        let mut control = self.control();
        self.collect(&mut control);
        if control.next_lease > MAX_LEASE {
            return Err(io::Error::other("source lease space exhausted"));
        }
        let existing = self.slots.iter().enumerate().find_map(|(index, slot)| {
            let pointer = slot.load(Ordering::SeqCst);
            unsafe { pointer.as_ref() }
                .filter(|source| source.token == token)
                .map(|source| (index, source))
        });
        if existing.is_some_and(|(_, source)| {
            source.id != id
                || source.capacity != request.capacity_frames
                || source.channels != request.channels
        }) {
            return Err(io::Error::other("source identity/geometry conflict"));
        }
        let index = existing
            .map(|(index, _)| index)
            .or_else(|| {
                self.slots
                    .iter()
                    .position(|slot| slot.load(Ordering::SeqCst).is_null())
            })
            .ok_or_else(|| io::Error::other("source inventory full"))?;
        let old_producer = match existing {
            Some((_, source)) => Some(source.claim_producer().ok_or_else(|| {
                io::Error::new(io::ErrorKind::WouldBlock, "source producer busy")
            })?),
            None => None,
        };
        let replace = existing.is_none_or(|(_, source)| source.terminal());
        if replace && control.owners.len() >= MAX_RETAINED {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "source readers must drain",
            ));
        }
        let mut candidate = if replace {
            Some(Box::pin(Source {
                id,
                token,
                capacity: request.capacity_frames,
                channels: request.channels,
                mapping: Mapping::new(request.capacity_frames, request.channels)?,
                consumer_busy: AtomicBool::new(false),
                timeline: UnsafeCell::new(Timeline::default()),
                _pinned: PhantomPinned,
            }))
        } else {
            None
        };
        let source = candidate
            .as_deref()
            .or_else(|| existing.map(|(_, source)| source))
            .ok_or_else(|| io::Error::other("source absent"))?;
        let attachment = source.grant(
            control.next_lease,
            existing.filter(|_| replace).map(|(_, old)| old),
        )?;
        control.next_lease += 1;
        if let Some(candidate) = candidate.take() {
            // The pin remains control-owned. Readers access only immutable
            // fields, atomics and the independently claimed timeline cell.
            let pointer = ptr::from_ref(candidate.as_ref().get_ref()).cast_mut();
            control.owners.push(candidate);
            self.slots[index].store(pointer, Ordering::SeqCst);
        }
        drop(old_producer);
        self.collect(&mut control);
        Ok(attachment)
    }
}

fn validated_id(request: &SourceRequest) -> io::Result<Vec<u8>> {
    let id = request
        .source_id
        .iter()
        .position(|byte| *byte == 0)
        .filter(|length| *length != 0)
        .map(|length| {
            request.source_id[..length]
                .iter()
                .map(|byte| byte.to_ne_bytes()[0])
                .collect::<Vec<_>>()
        })
        .ok_or_else(|| io::Error::other("invalid source identity"))?;
    if request.transport_version != VERSION
        || !(1024..=1_048_576).contains(&request.capacity_frames)
        || !request.capacity_frames.is_power_of_two()
        || !(1..=64).contains(&request.channels)
    {
        return Err(io::Error::other("invalid source format"));
    }
    Ok(id)
}

fn token(id: &[u8]) -> u64 {
    let hash = id
        .iter()
        .fold(14_695_981_039_346_656_037_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(1_099_511_628_211)
        });
    if hash == 0 { 1 } else { hash }
}

impl SourceRegistry {
    fn clock(&self, token: u64) -> Option<SourceClock> {
        let read = self.read(token);
        let source = read.source()?;
        let clock = source.mapping.clock();
        let format = clock.format_word.load(Ordering::Acquire);
        let acknowledged = clock.acknowledged_format.load(Ordering::Acquire);
        let lease = clock.producer_lease.load(Ordering::Acquire);
        let epoch = unsafe { tn_shared_audio_buffer_stats(source.mapping.buffer.as_ptr()) }.epoch;
        let frames = clock.native_frames.load(Ordering::Acquire);
        let rate = clock.native_sample_rate.load(Ordering::Acquire);
        let running = clock.native_running.load(Ordering::Acquire);
        let consumer = clock.consumer_position.load(Ordering::Acquire);
        let consumer_running = clock.consumer_running.load(Ordering::Acquire) != 0;
        if format == 0
            || format != acknowledged
            || epoch == 0
            || lease == 0
            || lease > MAX_LEASE
            || format != clock.format_word.load(Ordering::Acquire)
            || acknowledged != clock.acknowledged_format.load(Ordering::Acquire)
            || lease != clock.producer_lease.load(Ordering::Acquire)
            || epoch
                != unsafe { tn_shared_audio_buffer_stats(source.mapping.buffer.as_ptr()) }.epoch
        {
            return None;
        }
        Some(SourceClock {
            epoch: (lease << 16) | epoch,
            native_frames: frames,
            native_rate: rate,
            graph_rate: u32::try_from(format & u64::from(u32::MAX)).expect("masked rate fits u32"),
            running,
            synchronized: u32::from(running != 0 && consumer_running && consumer >> 48 == epoch),
        })
    }
    fn read_audio(&self, token: u64, samples: &mut [f32], frames: u32, rate: u32) -> u32 {
        samples.fill(0.0);
        let read = self.read(token);
        let Some(source) = read.source().filter(|source| {
            frames != 0 && samples.len() == frames as usize * source.channels as usize
        }) else {
            return 0;
        };
        let Some(_claim) = source.claim_consumer() else {
            return 0;
        };
        // SAFETY: this nonblocking consumer claim is the only access to the
        // timeline; no control method modifies it, including replacement.
        let timeline = unsafe { &mut *source.timeline.get() };
        let buffer = source.mapping.buffer.as_ptr();
        let clock = source.mapping.clock.as_ptr();
        let stats = unsafe { tn_shared_audio_buffer_stats(buffer) };
        let format = unsafe { tn_application_clock_request_rate(clock, rate) };
        let valid = format != 0
            && stats.epoch != 0
            && source
                .mapping
                .clock()
                .acknowledged_format
                .load(Ordering::Acquire)
                == format;
        if !valid {
            timeline.started = false;
            unsafe {
                tn_application_clock_publish_consumer(clock, stats.epoch, 0, frames, false);
            }
            return 0;
        }
        if !timeline.ready(stats, frames, source.capacity) {
            // SAFETY: the consumer claim owns this timeline and the read guard
            // retains its mapping through this bounded atomic publication.
            unsafe {
                tn_application_clock_publish_consumer(
                    clock,
                    stats.epoch,
                    timeline.next,
                    frames,
                    false,
                );
            }
            return 0;
        }
        let available = unsafe {
            tn_shared_audio_buffer_read(buffer, timeline.next, samples.as_mut_ptr(), frames)
        };
        timeline.next += u64::from(frames);
        let valid = source
            .mapping
            .clock()
            .acknowledged_format
            .load(Ordering::Acquire)
            == format
            && stats.epoch == unsafe { tn_shared_audio_buffer_stats(buffer) }.epoch;
        unsafe {
            tn_application_clock_publish_consumer(
                clock,
                stats.epoch,
                timeline.next,
                frames,
                valid && available == frames,
            );
        }
        if valid {
            available
        } else {
            samples.fill(0.0);
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn tn_engine_source_registry_create() -> *mut SourceRegistry {
    Box::into_raw(Box::new(SourceRegistry::new()))
}
/// # Safety
/// Registry was created by this module; all callers/readers have stopped.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tn_engine_source_registry_destroy(registry: *mut SourceRegistry) {
    if !registry.is_null() {
        drop(unsafe { Box::from_raw(registry) });
    }
}
/// # Safety
/// Registry/request and all output pointers are live. A successful descriptor
/// is owned by the caller and must be closed after the attachment is consumed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tn_engine_source_registry_attach_clocked(
    registry: *mut SourceRegistry,
    request: *const SourceRequest,
    descriptor: *mut i32,
    bytes: *mut usize,
    lease: *mut u64,
    epoch: *mut u64,
) -> bool {
    let (Some(registry), Some(request)) =
        (unsafe { registry.as_ref() }, unsafe { request.as_ref() })
    else {
        return false;
    };
    if descriptor.is_null() || bytes.is_null() || lease.is_null() || epoch.is_null() {
        return false;
    }
    let Ok(attachment) = registry.attach(request) else {
        return false;
    };
    unsafe {
        descriptor.write(attachment.descriptor.into_raw_fd());
        bytes.write(attachment.mapped_size);
        lease.write(attachment.producer_lease);
        epoch.write(attachment.source_epoch);
    }
    true
}
/// # Safety
/// Source ID is a NUL-terminated string of at most 127 bytes, or a readable
/// 128-byte unterminated array (which is rejected).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tn_engine_source_id_token(id: *const c_char) -> u64 {
    if id.is_null() {
        return 0;
    }
    let mut bytes = [0_u8; 128];
    for index in 0..128 {
        let byte = unsafe { id.add(index).read() }.to_ne_bytes()[0];
        if byte == 0 {
            return if index == 0 {
                0
            } else {
                token(&bytes[..index])
            };
        }
        bytes[index] = byte;
    }
    0
}
/// # Safety
/// Registry is live and output is writable and disjoint from shared memory.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tn_engine_source_registry_clock(
    registry: *mut SourceRegistry,
    token: u64,
    output: *mut SourceClock,
) -> bool {
    let Some(registry) = (unsafe { registry.as_ref() }) else {
        return false;
    };
    if output.is_null() {
        return false;
    }
    let Some(clock) = registry.clock(token) else {
        return false;
    };
    unsafe {
        output.write(clock);
    }
    true
}
/// # Safety
/// Registry remains live throughout this control call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tn_engine_source_registry_set_clock_master(
    registry: *mut SourceRegistry,
    token: u64,
    master: bool,
) -> bool {
    let Some(registry) = (unsafe { registry.as_ref() }) else {
        return false;
    };
    let read = registry.read(token);
    let Some(source) = read.source() else {
        return false;
    };
    source
        .mapping
        .clock()
        .clock_master
        .store(u32::from(master), Ordering::Release);
    true
}
/// # Safety
/// Registry is live; samples has frames*channels writable floats and does not
/// alias the transport or another output buffer during this nonblocking call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tn_engine_source_registry_read_clocked(
    registry: *mut SourceRegistry,
    token: u64,
    samples: *mut f32,
    frames: u32,
    channels: u32,
    rate: u32,
) -> u32 {
    if samples.is_null() || frames > 1_048_576 || channels == 0 || channels > 64 {
        return 0;
    }
    let output =
        unsafe { std::slice::from_raw_parts_mut(samples, frames as usize * channels as usize) };
    let Some(registry) = (unsafe { registry.as_ref() }) else {
        output.fill(0.0);
        return 0;
    };
    registry.read_audio(token, output, frames, rate)
}

#[cfg(test)]
mod tests;
