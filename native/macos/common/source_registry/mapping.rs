use crate::{
    application_clock::{
        ApplicationClock, tn_application_clock_initialize, tn_application_clock_required_bytes,
    },
    create_shared_memory,
    shared_audio_buffer::{SharedAudioBuffer, tn_shared_audio_buffer_initialize},
};
use std::{
    ffi::c_void,
    io,
    os::fd::{AsRawFd, OwnedFd},
    ptr::NonNull,
};

unsafe extern "C" {
    fn mmap(
        address: *mut c_void,
        length: usize,
        protection: i32,
        flags: i32,
        descriptor: i32,
        offset: i64,
    ) -> *mut c_void;
    fn munmap(address: *mut c_void, length: usize) -> i32;
}

pub(super) struct Mapping {
    memory: NonNull<c_void>,
    pub bytes: usize,
    pub descriptor: OwnedFd,
    pub buffer: NonNull<SharedAudioBuffer>,
    pub clock: NonNull<ApplicationClock>,
}
// SAFETY: the mapping's geometry is immutable. Access after publication uses
// only atomic transport APIs; registry readers pin this owner until control reclamation.
unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}
impl Mapping {
    pub(super) fn new(capacity: u32, channels: u32) -> io::Result<Self> {
        let bytes = tn_application_clock_required_bytes(capacity, channels);
        if bytes == 0 {
            return Err(io::Error::other("invalid source geometry"));
        }
        let descriptor = create_shared_memory(bytes, true)?;
        let address = unsafe { mmap(std::ptr::null_mut(), bytes, 3, 1, descriptor.as_raw_fd(), 0) };
        if address == std::ptr::without_provenance_mut(usize::MAX) {
            return Err(io::Error::last_os_error());
        }
        let Some(memory) = NonNull::new(address) else {
            unsafe {
                munmap(address, bytes);
            }
            return Err(io::Error::other("zero address mapping"));
        };
        let buffer = NonNull::new(unsafe {
            tn_shared_audio_buffer_initialize(address, bytes, capacity, channels)
        });
        let clock = NonNull::new(unsafe {
            tn_application_clock_initialize(address, bytes, capacity, channels)
        });
        let (Some(buffer), Some(clock)) = (buffer, clock) else {
            unsafe {
                munmap(address, bytes);
            }
            return Err(io::Error::other("source initialization failed"));
        };
        Ok(Self {
            memory,
            bytes,
            descriptor,
            buffer,
            clock,
        })
    }
    pub(super) fn clock(&self) -> &ApplicationClock {
        // SAFETY: immutable initialized clock geometry and atomic payload live
        // for this mapping owner's lifetime; no mutable Rust reference exists.
        unsafe { self.clock.as_ref() }
    }
}
impl Drop for Mapping {
    fn drop(&mut self) {
        unsafe {
            munmap(self.memory.as_ptr(), self.bytes);
        }
        // The descriptor closes after unmapping. Remote duplicate descriptors
        // and mappings continue owning their independent kernel references.
    }
}
