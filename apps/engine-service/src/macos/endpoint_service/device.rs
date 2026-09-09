//! Exact own-device Core Audio property access; no default selection or graph clock policy.
use crate::endpoint_registry::EndpointId;
use std::{
    ffi::{CString, c_void},
    ptr,
};
use tonenari_macos_transport::endpoint_identity::endpoint_uid;

#[repr(C)]
struct Address {
    selector: u32,
    scope: u32,
    element: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Range {
    minimum: f64,
    maximum: f64,
}
unsafe extern "C" {
    fn CFStringCreateWithCString(
        allocator: *const c_void,
        value: *const std::ffi::c_char,
        encoding: u32,
    ) -> *const c_void;
    fn CFRelease(value: *const c_void);
    fn AudioObjectGetPropertyData(
        object: u32,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: *mut u32,
        data: *mut c_void,
    ) -> i32;
    fn AudioObjectSetPropertyData(
        object: u32,
        address: *const Address,
        qualifier_size: u32,
        qualifier: *const c_void,
        size: u32,
        data: *const c_void,
    ) -> i32;
}
fn address(selector: [u8; 4]) -> Address {
    Address {
        selector: u32::from_be_bytes(selector),
        scope: u32::from_be_bytes(*b"glob"),
        element: 0,
    }
}
fn device(id: EndpointId) -> Option<u32> {
    let name = CString::new(endpoint_uid(id)).ok()?;
    // SAFETY: name is a terminated owned UTF-8 buffer retained through creation;
    // the create rule returns a separately owned CFString, released below.
    let uid = unsafe { CFStringCreateWithCString(ptr::null(), name.as_ptr(), 0x0800_0100) };
    if uid.is_null() {
        return None;
    }
    let mut object = 0_u32;
    let mut size = u32::try_from(size_of_val(&object)).ok()?;
    let property = address(*b"uidd");
    // SAFETY: the qualifier is one live CFStringRef pointer. Core Audio writes only
    // the declared u32 output capacity; all locals remain aligned and disjoint.
    let result = unsafe {
        AudioObjectGetPropertyData(
            1,
            &raw const property,
            u32::try_from(size_of_val(&uid)).ok()?,
            (&raw const uid).cast(),
            &raw mut size,
            (&raw mut object).cast(),
        )
    };
    // SAFETY: release exactly our +1 create ownership after the synchronous lookup.
    unsafe { CFRelease(uid) };
    (result == 0 && size == 4 && object != 0).then_some(object)
}
pub(super) fn rate(id: EndpointId) -> Option<u32> {
    let object = device(id)?;
    let mut rate = 0.0_f64;
    let mut size = 8;
    let property = address(*b"nsrt");
    // SAFETY: the property value is one f64; Core Audio receives its exact initialized
    // writable extent and does not retain this stack storage.
    let result = unsafe {
        AudioObjectGetPropertyData(
            object,
            &raw const property,
            0,
            ptr::null(),
            &raw mut size,
            (&raw mut rate).cast(),
        )
    };
    if result != 0 || size != 8 || !rate.is_finite() {
        return None;
    }
    tonenari_audio_io::formats::SAMPLE_RATES
        .iter()
        .copied()
        .find(|value| f64::from(*value).to_bits() == rate.to_bits())
}
pub(super) fn supports(id: EndpointId, rate: u32) -> bool {
    if !tonenari_audio_io::formats::SAMPLE_RATES.contains(&rate) {
        return false;
    }
    let Some(object) = device(id) else {
        return false;
    };
    let mut ranges = [Range::default(); 32];
    let mut size = u32::try_from(size_of_val(&ranges)).unwrap_or(0);
    let property = address(*b"nsr#");
    // SAFETY: the declared buffer capacity is the complete aligned Range array.
    // Returned byte length is checked before slicing; failed calls expose no values.
    let result = unsafe {
        AudioObjectGetPropertyData(
            object,
            &raw const property,
            0,
            ptr::null(),
            &raw mut size,
            ranges.as_mut_ptr().cast(),
        )
    };
    if result != 0
        || size as usize > size_of_val(&ranges)
        || !(size as usize).is_multiple_of(size_of::<Range>())
    {
        return false;
    }
    ranges[..size as usize / size_of::<Range>()]
        .iter()
        .any(|range| (range.minimum..=range.maximum).contains(&f64::from(rate)))
}
pub(super) fn set_rate(id: EndpointId, rate: u32) -> bool {
    if !supports(id, rate) {
        return false;
    }
    let Some(object) = device(id) else {
        return false;
    };
    let rate = f64::from(rate);
    let property = address(*b"nsrt");
    // SAFETY: Core Audio synchronously copies this finite supported f64 value;
    // no pointer is retained and no device outside the ID-derived UID is selected.
    unsafe {
        AudioObjectSetPropertyData(
            object,
            &raw const property,
            0,
            ptr::null(),
            8,
            (&raw const rate).cast(),
        ) == 0
    }
}
