//! One initialized transport mapping. Geometry never changes in-place; every
//! replacement gets its own descriptor and shared-memory lifetime.
use super::EndpointSpec;
use std::{
    ffi::{c_char, c_int, c_void},
    io,
    os::fd::{AsRawFd, OwnedFd},
    ptr::{self, NonNull},
};
use tonenari_macos_transport::{
    control_inventory::InventoryEntry,
    endpoint_transport::{
        EndpointTransport, EndpointTransportConfig, tn_endpoint_transport_capture_buffer,
        tn_endpoint_transport_initialize, tn_endpoint_transport_render_buffer,
        tn_endpoint_transport_required_bytes,
    },
    shared_audio_buffer::SharedAudioBuffer,
};

unsafe extern "C" {
    fn mmap(
        address: *mut c_void,
        length: usize,
        protection: c_int,
        flags: c_int,
        descriptor: c_int,
        offset: i64,
    ) -> *mut c_void;
    fn munmap(address: *mut c_void, length: usize) -> c_int;
}

/// Never exposes a Rust reference into interprocess-writable storage. Only the
/// transport's atomic ABI may access it after initialization and publication.
struct Region {
    address: NonNull<c_void>,
    length: usize,
    descriptor: OwnedFd,
}

impl Region {
    fn new(length: usize) -> io::Result<Self> {
        let descriptor = tonenari_macos_transport::create_shared_memory(length, true)?;
        let address = unsafe { mmap(ptr::null_mut(), length, 3, 1, descriptor.as_raw_fd(), 0) };
        if address == ptr::without_provenance_mut(usize::MAX) {
            return Err(io::Error::last_os_error());
        }
        let Some(address) = NonNull::new(address) else {
            // A zero-address mapping cannot be represented by the native ABI.
            unsafe { munmap(address, length) };
            return Err(io::Error::other("native mapping returned address zero"));
        };
        Ok(Self {
            address,
            length,
            descriptor,
        })
    }
}

impl Drop for Region {
    fn drop(&mut self) {
        // SAFETY: only this owner unmaps, after all callback pins are released.
        unsafe { munmap(self.address.as_ptr(), self.length) };
        // OwnedFd closes after unmapping. Independently duplicated HAL/snapshot
        // FDs continue to own the object even when the engine retires its map.
    }
}

pub(super) struct Mapping {
    spec: EndpointSpec,
    generation: u64,
    region: Region,
    transport: NonNull<EndpointTransport>,
    render: *mut SharedAudioBuffer,
    capture: *mut SharedAudioBuffer,
}

// SAFETY: geometry/addresses/spec are immutable after initialization. Mutable
// shared storage is accessed solely through the atomic native transport ABI;
// this type never hands out mutable Rust references. Registry retirement pins
// ensure unmapping cannot race either a local callback or a snapshot FD copy.
unsafe impl Send for Mapping {}
unsafe impl Sync for Mapping {}

impl Mapping {
    pub(super) fn new(spec: EndpointSpec, generation: u64) -> io::Result<Self> {
        let config = EndpointTransportConfig {
            endpoint_id: spec.id,
            capacity_frames: spec.capacity_frames,
            render_channels: spec.render_channels,
            capture_channels: spec.capture_channels,
        };
        let length = unsafe { tn_endpoint_transport_required_bytes(&raw const config) };
        if length == 0 || u32::try_from(length).is_err() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid native endpoint layout",
            ));
        }
        let region = Region::new(length)?;
        let transport = NonNull::new(unsafe {
            tn_endpoint_transport_initialize(region.address.as_ptr(), length, &raw const config)
        })
        .ok_or_else(|| io::Error::other("endpoint transport initialization failed"))?;
        let render = unsafe { tn_endpoint_transport_render_buffer(transport.as_ptr()) };
        let capture = unsafe { tn_endpoint_transport_capture_buffer(transport.as_ptr()) };
        if (spec.render_channels != 0 && render.is_null())
            || (spec.capture_channels != 0 && capture.is_null())
        {
            return Err(io::Error::other("endpoint transport buffers are invalid"));
        }
        Ok(Self {
            spec,
            generation,
            region,
            transport,
            render,
            capture,
        })
    }
    pub(super) fn spec(&self) -> &EndpointSpec {
        &self.spec
    }
    pub(super) fn generation(&self) -> u64 {
        self.generation
    }
    pub(super) fn transport(&self) -> *mut EndpointTransport {
        self.transport.as_ptr()
    }
    pub(super) fn render_buffer(&self) -> *mut SharedAudioBuffer {
        self.render
    }
    pub(super) fn capture_buffer(&self) -> *mut SharedAudioBuffer {
        self.capture
    }
    pub(super) fn inventory_entry(&self) -> io::Result<(InventoryEntry, OwnedFd)> {
        let descriptor = self.region.descriptor.try_clone()?;
        let mut entry = InventoryEntry {
            endpoint_id: self.spec.id,
            sample_rate: self.spec.sample_rate,
            descriptor: descriptor.as_raw_fd(),
            mapped_size: self.region.length,
            generation: self.generation,
            ..InventoryEntry::default()
        };
        for (slot, byte) in entry
            .display_name
            .iter_mut()
            .zip(self.spec.display_name.bytes())
        {
            *slot = c_char::from_ne_bytes([byte]);
        }
        Ok((entry, descriptor))
    }
}
