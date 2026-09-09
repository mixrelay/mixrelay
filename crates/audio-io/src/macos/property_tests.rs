use std::ffi::{c_char, c_void};
use std::ptr::NonNull;

use super::{OwnedString, UTF8, channel_count, decode_device_ids};

#[test]
fn device_id_decoder_rejects_partial_u32_and_preserves_native_ids() {
    assert!(decode_device_ids(&[]).unwrap().is_empty());
    assert!(decode_device_ids(&[1, 2, 3]).is_err());
    let bytes: Vec<_> = [7_u32, 13, 1024]
        .into_iter()
        .flat_map(u32::to_ne_bytes)
        .collect();
    assert_eq!(decode_device_ids(&bytes).unwrap(), [7, 13, 1024]);
}

#[test]
fn channel_count_checks_variable_list_bounds_and_saturates_overflow() {
    assert_eq!(channel_count(&[]), None);
    assert_eq!(channel_count(&[0; 8]), Some(0));
    let mut bytes = [0_u8; 40];
    bytes[..4].copy_from_slice(&2_u32.to_ne_bytes());
    bytes[8..12].copy_from_slice(&7_u32.to_ne_bytes());
    bytes[24..28].copy_from_slice(&3_u32.to_ne_bytes());
    assert_eq!(channel_count(&bytes), Some(10));
    assert_eq!(channel_count(&bytes[..39]), None);
    bytes[8..12].copy_from_slice(&u32::MAX.to_ne_bytes());
    assert_eq!(channel_count(&bytes), Some(u32::MAX));
    bytes[..4].copy_from_slice(&u32::MAX.to_ne_bytes());
    assert_eq!(channel_count(&bytes), None);
}

#[test]
fn retained_cf_string_conversion_preserves_utf8_and_releases_owned_reference() {
    unsafe extern "C" {
        fn CFStringCreateWithCString(
            allocator: *const c_void,
            bytes: *const c_char,
            encoding: u32,
        ) -> *mut c_void;
    }
    for text in ["Interface", "오디오 장치", ""] {
        let bytes = std::ffi::CString::new(text).unwrap();
        // SAFETY: CString owns NUL-terminated UTF8 for this synchronous copying
        // constructor. Its +1 result transfers directly into our RAII owner.
        let pointer = unsafe { CFStringCreateWithCString(std::ptr::null(), bytes.as_ptr(), UTF8) };
        let owned = OwnedString(NonNull::new(pointer).unwrap());
        assert_eq!(owned.text().as_deref(), Some(text));
        drop(owned);
    }
}
