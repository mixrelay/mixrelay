use super::{SourceClockSnapshot, SourceTransport};
use crate::{
    application_source_mapping::Mapping,
    control_inventory::{Message, Xpc},
    shared_audio_buffer::{mr_shared_audio_buffer_capacity, mr_shared_audio_buffer_channels},
    source_control::{
        SourceAttachment, mr_engine_control_create_clocked_source_request,
        mr_engine_control_parse_source_attachment,
    },
};
use std::{
    alloc::{Layout, alloc},
    ffi::c_char,
    io,
    os::fd::{FromRawFd, OwnedFd},
    ptr, slice,
};

/// Native attachment failure category; values match the C control ABI.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum TransportErrorCode {
    /// Attachment completed successfully.
    #[default]
    Ok = 0,
    /// Source identity or geometry was not valid.
    InvalidArgument = 1,
    /// The engine did not return a usable XPC response.
    EngineUnavailable = 2,
    /// Descriptor, geometry, headers or lease did not match the grant.
    InvalidAttachment = 3,
    /// The owned mapping or local owner could not be allocated.
    MappingFailed = 4,
}

/// Error returned on the control worker, with an optional POSIX error number.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct TransportError {
    /// Stable native error category.
    pub code: TransportErrorCode,
    /// POSIX error, or zero when no system call failed.
    pub system_error: i32,
}

unsafe extern "C" {
    // This single platform glue operation owns the XPC blocks-based connection.
    fn mr_application_source_request(request: Xpc) -> Xpc;
}

// SAFETY: attachment contains one uniquely owned descriptor from the source
// codec/registry. The registry's published backing object is never truncated;
// its headers remain immutable while producer and consumer use atomic payloads.
unsafe fn attach(
    attachment: &mut SourceAttachment,
    capacity: u32,
    channels: u32,
) -> Result<SourceTransport, TransportError> {
    let SourceAttachment {
        descriptor,
        mapped_size,
        producer_lease,
        source_epoch,
    } = std::mem::take(attachment);
    let invalid = TransportError {
        code: TransportErrorCode::InvalidAttachment,
        system_error: 0,
    };
    if descriptor < 0 {
        return Err(invalid);
    }
    // SAFETY: the C attachment facade takes and clears the unique parsed FD;
    // the connect codec also returns a fresh duplicate owned by this function.
    let descriptor = unsafe { OwnedFd::from_raw_fd(descriptor) };
    if producer_lease == 0 || source_epoch == 0 {
        return Err(invalid);
    }
    let mapping = unsafe { Mapping::attach(descriptor, mapped_size, capacity, channels) }.map_err(
        |error| TransportError {
            code: if error.kind() == io::ErrorKind::InvalidData {
                TransportErrorCode::InvalidAttachment
            } else {
                TransportErrorCode::MappingFailed
            },
            system_error: error.raw_os_error().unwrap_or(0),
        },
    )?;
    let transport = SourceTransport {
        mapping,
        next_frame: 0,
        owned_epoch: source_epoch,
        owned_lease: producer_lease,
    };
    if !transport.owns_stream() {
        return Err(invalid);
    }
    Ok(transport)
}

/// Allocates only after all mapping/lease validation. Failed allocation drops
/// the acquired mapping and descriptor before returning the control-plane error.
pub(super) unsafe fn publish_owner(
    result: Result<SourceTransport, TransportError>,
    error: *mut TransportError,
    allocate: impl FnOnce(Layout) -> *mut u8,
) -> *mut SourceTransport {
    let result = result.and_then(|transport| {
        // SAFETY: global allocation matches Box's layout/deallocator. Initialization
        // happens before publication and allocation failure retains the local owner.
        let pointer = allocate(Layout::new::<SourceTransport>())
            .cast::<std::ffi::c_void>()
            .cast::<SourceTransport>();
        if pointer.is_null() {
            return Err(TransportError {
                code: TransportErrorCode::MappingFailed,
                system_error: 12,
            });
        }
        unsafe { pointer.write(transport) };
        Ok(pointer)
    });
    let (pointer, failure) = match result {
        Ok(pointer) => (pointer, TransportError::default()),
        Err(error) => (ptr::null_mut(), error),
    };
    // SAFETY: the facade's optional output is exclusive initialized writable storage.
    if let Some(error) = unsafe { error.as_mut() } {
        *error = failure;
    }
    pointer
}

/// Adopts one registry attachment, consuming its descriptor even on failure.
/// # Safety
/// Attachment is exclusive writable storage containing one owned parsed FD (or
/// -1). Its backing object has immutable headers and cannot be truncated while
/// published. Error is null or exclusive writable storage. Both are disjoint.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_application_source_transport_attach(
    attachment: *mut SourceAttachment,
    capacity: u32,
    channels: u32,
    error: *mut TransportError,
) -> *mut SourceTransport {
    // SAFETY: descriptor ownership crosses once and caller storage is cleared first.
    let result = unsafe { attachment.as_mut() }.map_or_else(
        || {
            Err(TransportError {
                code: TransportErrorCode::InvalidAttachment,
                system_error: 0,
            })
        },
        |attachment| unsafe { attach(attachment, capacity, channels) },
    );
    unsafe { publish_owner(result, error, |layout| alloc(layout)) }
}

/// Connects synchronously on the control worker; no audio callback invokes XPC.
/// # Safety
/// Source ID is null or readable through its first NUL, at most 128 bytes.
/// Error is null or exclusive writable storage disjoint from that ID.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_application_source_transport_connect(
    source: *const c_char,
    capacity: u32,
    channels: u32,
    error: *mut TransportError,
) -> *mut SourceTransport {
    // SAFETY: the source string meets the codec contract. Message owns each XPC ref.
    let request = Message::new(unsafe {
        mr_engine_control_create_clocked_source_request(source, capacity, channels)
    });
    let result = request
        .ok_or(TransportError {
            code: TransportErrorCode::InvalidArgument,
            system_error: 0,
        })
        .and_then(|request| {
            // SAFETY: adapter borrows this retained request and returns an owned response.
            let response = Message::new(unsafe { mr_application_source_request(request.0) })
                .ok_or(TransportError {
                    code: TransportErrorCode::EngineUnavailable,
                    system_error: 0,
                })?;
            let mut attachment = SourceAttachment::default();
            // SAFETY: codec borrows the response and returns a fresh FD into empty storage.
            if !unsafe {
                mr_engine_control_parse_source_attachment(response.0, &raw mut attachment)
            } {
                return Err(TransportError {
                    code: TransportErrorCode::InvalidAttachment,
                    system_error: 0,
                });
            }
            // SAFETY: source codec just transferred a registry-owned duplicate.
            unsafe { attach(&mut attachment, capacity, channels) }
        });
    unsafe { publish_owner(result, error, |layout| alloc(layout)) }
}

/// Reads one bounded consumer snapshot; contention returns false without spinning.
/// # Safety
/// Handle is null or retained by its sole pump. Snapshot is exclusive writable
/// storage disjoint from the handle/mapping; destroy cannot overlap this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_application_source_transport_clock(
    transport: *const SourceTransport,
    snapshot: *mut SourceClockSnapshot,
) -> bool {
    let (Some(transport), Some(snapshot)) =
        (unsafe { transport.as_ref() }, unsafe { snapshot.as_mut() })
    else {
        return false;
    };
    let Some(observed) = transport.clock() else {
        return false;
    };
    *snapshot = observed;
    true
}

/// Begins a producer discontinuity only for the current lease and requested format.
/// # Safety
/// Handle is null or exclusively borrowed by the sole pump; no local operation or
/// destruction overlaps. Remote users access only the mapping's atomic protocol.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_application_source_transport_reset(
    transport: *mut SourceTransport,
    format: u64,
    native_rate: u32,
) -> bool {
    unsafe { transport.as_mut() }.is_some_and(|transport| transport.reset(format, native_rate))
}

/// Publishes native progress only while the source grant is current.
/// # Safety
/// Handle is null or retained by its sole pump, without concurrent destruction.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_application_source_transport_publish_native(
    transport: *const SourceTransport,
    frames: u64,
    running: bool,
) {
    if let Some(transport) = unsafe { transport.as_ref() } {
        transport.publish_native(frames, running);
    }
}

/// Publishes one complete PCM block, advancing the local cursor only on success.
/// # Safety
/// Handle is null or exclusively owned by the pump. Non-null input contains
/// frames*channels readable floats, disjoint from the handle and mapped storage.
/// No other local operation or destruction overlaps this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_application_source_transport_write(
    transport: *mut SourceTransport,
    input: *const f32,
    frames: u32,
) -> bool {
    let Some(transport) = (unsafe { transport.as_mut() }) else {
        return false;
    };
    // SAFETY: validated immutable geometry belongs to the retained mapping.
    let buffer = transport.mapping.buffer.as_ptr();
    let capacity = unsafe { mr_shared_audio_buffer_capacity(buffer) };
    let channels = unsafe { mr_shared_audio_buffer_channels(buffer) };
    if input.is_null() || frames > capacity {
        return false;
    }
    // SAFETY: bounded geometry and the pump's readable input establish this extent.
    let samples = unsafe { slice::from_raw_parts(input, frames as usize * channels as usize) };
    transport.write(samples, frames)
}

/// Stops native publication and releases the mapping, descriptor and local owner.
/// # Safety
/// Handle is null or the unique attach/connect result after the pump has stopped;
/// no references or pending calls remain. It is destroyed exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_application_source_transport_destroy(transport: *mut SourceTransport) {
    if !transport.is_null() {
        // SAFETY: publish_owner allocated this initialized object using Box's layout.
        drop(unsafe { Box::from_raw(transport) });
    }
}
