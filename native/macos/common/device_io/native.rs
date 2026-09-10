use std::{
    ffi::{c_char, c_void},
    ptr,
};

use super::Selection;
use crate::{
    aggregate_group::{NativeBinding, mr_aggregate_group_binding},
    device_buffers::BufferSelection,
    device_io::stream_format::{StreamFormat, StreamFormats},
};

const GLOBAL: u32 = u32::from_be_bytes(*b"glob");
const STREAMS: u32 = u32::from_be_bytes(*b"stm#");
const BUFFER_SIZE: u32 = u32::from_be_bytes(*b"fsiz");

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Address {
    pub(super) selector: u32,
    scope: u32,
    element: u32,
}

impl Address {
    fn new(selector: u32, scope: u32) -> Self {
        Self {
            selector,
            scope,
            element: 0,
        }
    }
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
pub(super) struct Description {
    pub(super) rate: f64,
    pub(super) format: u32,
    pub(super) flags: u32,
    pub(super) packet_bytes: u32,
    pub(super) packet_frames: u32,
    pub(super) frame_bytes: u32,
    pub(super) channels: u32,
    pub(super) bits: u32,
    pub(super) reserved: u32,
}

pub(super) struct StreamIds {
    pub(super) values: [u32; 2048],
    pub(super) count: usize,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct ValueRange {
    minimum: f64,
    maximum: f64,
}

/// Control-side device buffer facts. Missing optional bounds/settable facts stay zero.
#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct BufferInfo {
    /// Resolved native object identity.
    pub device: u32,
    /// Current frame size; zero means unavailable.
    pub frames: u32,
    /// Native lower bound, when available.
    pub minimum: f64,
    /// Native upper bound, when available.
    pub maximum: f64,
    /// One when `CoreAudio` reports the frame-size property writable.
    pub settable: u32,
}

type IoProc = unsafe extern "C" fn(
    u32,
    *const c_void,
    *const c_void,
    *const c_void,
    *mut c_void,
    *const c_void,
    *mut c_void,
) -> i32;

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    #[link_name = "AudioObjectGetPropertyData"]
    fn property_read(
        id: u32,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
        output: *mut c_void,
    ) -> i32;
    #[link_name = "AudioObjectGetPropertyDataSize"]
    fn property_size(
        id: u32,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
    ) -> i32;
    #[link_name = "AudioObjectSetPropertyData"]
    fn property_write(
        id: u32,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: u32,
        input: *const c_void,
    ) -> i32;
    #[link_name = "AudioObjectIsPropertySettable"]
    fn property_settable(id: u32, address: *const Address, result: *mut u8) -> i32;
    #[link_name = "AudioDeviceCreateIOProcID"]
    pub(super) fn create(
        device: u32,
        callback: IoProc,
        context: *mut c_void,
        procedure: *mut *mut c_void,
    ) -> i32;
    #[link_name = "AudioDeviceStart"]
    pub(super) fn start(device: u32, procedure: *mut c_void) -> i32;
    #[link_name = "AudioDeviceStop"]
    pub(super) fn stop(device: u32, procedure: *mut c_void) -> i32;
    #[link_name = "AudioDeviceDestroyIOProcID"]
    pub(super) fn destroy(device: u32, procedure: *mut c_void) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    #[link_name = "CFStringCreateWithCString"]
    fn string_create(allocator: *const c_void, text: *const c_char, encoding: u32)
    -> *const c_void;
    #[link_name = "CFRelease"]
    fn release(value: *const c_void);
}

/// Private synchronous property boundary. Tests supply fixed scalar facts; no
/// callback or resource owner retains this adapter or its query-local output.
pub(super) trait Properties {
    fn read<T: Copy + Default>(&self, id: u32, address: Address) -> Option<T>;
    fn streams(&self, id: u32, scope: u32, limit: u32) -> Option<StreamIds>;
}

pub(super) struct System;

impl Properties for System {
    fn read<T: Copy + Default>(&self, id: u32, address: Address) -> Option<T> {
        let mut result = T::default();
        let capacity = u32::try_from(size_of::<T>()).ok()?;
        let mut size = capacity;
        // SAFETY: all private call sites pair a native scalar ABI type with its
        // matching selector. Output owns exactly the advertised capacity.
        let status = unsafe {
            property_read(
                id,
                &raw const address,
                0,
                ptr::null(),
                &raw mut size,
                ptr::from_mut(&mut result).cast(),
            )
        };
        (status == 0 && size == capacity).then_some(result)
    }

    fn streams(&self, id: u32, scope: u32, limit: u32) -> Option<StreamIds> {
        let address = Address::new(STREAMS, scope);
        let mut size = 0;
        // SAFETY: scalar size output belongs to this control-side query.
        if unsafe { property_size(id, &raw const address, 0, ptr::null(), &raw mut size) } != 0
            || !valid_stream_extent(size, limit * 4)
        {
            return None;
        }
        let capacity = size;
        let mut streams = StreamIds {
            values: [0; 2048],
            count: 0,
        };
        // SAFETY: initialized u32 buffer has the queried capacity; CoreAudio may
        // shorten the list, but returned size must never expand that capacity.
        let status = unsafe {
            property_read(
                id,
                &raw const address,
                0,
                ptr::null(),
                &raw mut size,
                streams.values.as_mut_ptr().cast(),
            )
        };
        if status != 0 || !valid_stream_extent(size, capacity) {
            return None;
        }
        streams.count = size as usize / 4;
        Some(streams)
    }
}

pub(super) fn valid_stream_extent(size: u32, capacity: u32) -> bool {
    size != 0 && size <= capacity && size.is_multiple_of(4)
}

pub(super) fn default_device(input: bool, properties: &impl Properties) -> Option<u32> {
    let selector = if input { *b"dIn " } else { *b"dOut" };
    properties.read(1, Address::new(u32::from_be_bytes(selector), GLOBAL))
}

pub(super) fn resolve(
    selection: &Selection,
    properties: &impl Properties,
) -> Option<NativeBinding> {
    if !selection.group.is_null() {
        let mut binding = NativeBinding::default();
        // SAFETY: enclosing engine Native owns the aggregate Arc, retained until
        // this device context is destroyed. UID is immutable owned NUL-terminated storage.
        return unsafe {
            mr_aggregate_group_binding(
                selection.group.cast(),
                selection.uid.as_ptr().cast(),
                selection.input,
                selection.channels,
                &raw mut binding,
            )
        }
        .then_some(binding);
    }
    let device = if selection.uid.len() == 1 {
        default_device(selection.input, properties)?
    } else {
        resolve_uid(selection)?
    };
    (device != 0).then_some(NativeBinding {
        device,
        offset: 0,
        total: 0,
    })
}

fn resolve_uid(selection: &Selection) -> Option<u32> {
    // SAFETY: owned bytes were copied including the terminator from a complete CStr.
    let uid = unsafe { string_create(ptr::null(), selection.uid.as_ptr().cast(), 0x0800_0100) };
    if uid.is_null() {
        return None;
    }
    let address = Address::new(u32::from_be_bytes(*b"uidd"), GLOBAL);
    let mut device = 0;
    let mut size = 4;
    // SAFETY: native qualifier is one retained CFStringRef; output is one u32.
    let status = unsafe {
        property_read(
            1,
            &raw const address,
            8,
            ptr::from_ref(&uid).cast(),
            &raw mut size,
            ptr::from_mut(&mut device).cast(),
        )
    };
    // SAFETY: this function owns exactly the create-rule CF reference.
    unsafe { release(uid) };
    (status == 0 && size == 4).then_some(device)
}

pub(super) fn format(
    selection: &Selection,
    binding: NativeBinding,
    properties: &impl Properties,
) -> Option<u32> {
    let slice = buffer_selection(selection, binding);
    let scope = u32::from_be_bytes(if selection.input { *b"inpt" } else { *b"outp" });
    let streams = properties.streams(binding.device, scope, slice.limit)?;
    let mut formats = StreamFormats::default();
    for &stream in &streams.values[..streams.count] {
        let format: Description =
            properties.read(stream, Address::new(u32::from_be_bytes(*b"sfmt"), GLOBAL))?;
        if !formats.accept(
            &StreamFormat {
                rate: format.rate,
                format: format.format,
                flags: format.flags,
                bits: format.bits,
                channels: format.channels,
                frame_bytes: format.frame_bytes,
            },
            slice.limit,
        ) {
            return None;
        }
    }
    formats.selected_rate(slice).map(admitted_rate)
}

// Called only with StreamFormats' admitted finite integral 8000..384000 Hz value.
// Safe numeric conversion avoids introducing unsafe solely to satisfy cast lints.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "StreamFormats admitted finite integral 8000..384000 Hz"
)]
fn admitted_rate(rate: f64) -> u32 {
    rate as u32
}

pub(super) fn buffer_selection(selection: &Selection, binding: NativeBinding) -> BufferSelection {
    BufferSelection {
        channels: selection.channels,
        offset: binding.offset,
        total: if selection.group.is_null() {
            0
        } else {
            binding.total
        },
        limit: if selection.group.is_null() { 64 } else { 2048 },
    }
}

pub(super) fn buffer_size(device: u32) -> u32 {
    System
        .read(device, Address::new(BUFFER_SIZE, GLOBAL))
        .unwrap_or(0)
}

pub(super) fn buffer_info(selection: &Selection) -> Option<BufferInfo> {
    let device = resolve(selection, &System)?.device;
    let frames = buffer_size(device);
    if frames == 0 {
        return None;
    }
    let range: ValueRange = System
        .read(device, Address::new(u32::from_be_bytes(*b"fsz#"), GLOBAL))
        .unwrap_or_default();
    let mut settable = 0;
    let address = Address::new(BUFFER_SIZE, GLOBAL);
    // SAFETY: CoreAudio Boolean is one byte, exclusively borrowed until return.
    let status = unsafe { property_settable(device, &raw const address, &raw mut settable) };
    Some(BufferInfo {
        device,
        frames,
        minimum: range.minimum,
        maximum: range.maximum,
        settable: u32::from(status == 0 && settable != 0),
    })
}

pub(super) fn buffer_write(device: u32, frames: u32) -> bool {
    let address = Address::new(BUFFER_SIZE, GLOBAL);
    // SAFETY: synchronous frame-size write borrows one immutable u32.
    unsafe {
        property_write(
            device,
            &raw const address,
            0,
            ptr::null(),
            4,
            ptr::from_ref(&frames).cast(),
        ) == 0
    }
}

pub(super) fn alive(device: u32, properties: &impl Properties) -> bool {
    properties
        .read::<u32>(device, Address::new(u32::from_be_bytes(*b"livn"), GLOBAL))
        .is_some_and(|alive| alive != 0)
}

pub(super) fn clock_device(device: u32, properties: &impl Properties) -> u32 {
    properties
        .read::<u32>(device, Address::new(u32::from_be_bytes(*b"tran"), GLOBAL))
        .filter(|transport| *transport != 0 && *transport != u32::from_be_bytes(*b"grup"))
        .map_or(0, |_| device)
}

const _: () = assert!(size_of::<BufferInfo>() == 32 && size_of::<Description>() == 40);
