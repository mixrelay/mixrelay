//! Process-tap capture ABI declarations for the desktop adapter.

use super::capture_error::NativeError;
use mixrelay_process_tap::TapStats as NativeStats;
use std::ffi::{c_char, c_void};

#[repr(C)]
pub(super) struct NativeProcessTapSession {
    _private: [u8; 0],
}

#[repr(C)]
pub(super) struct NativeConfig {
    pub(super) bundle_id: *const c_char,
    pub(super) mute_while_captured: bool,
    pub(super) ring_capacity_frames: usize,
    pub(super) target_device_uid: *const c_char,
    pub(super) system_audio: bool,
    pub(super) excluded_bundle_ids: *const *const c_char,
    pub(super) excluded_bundle_count: usize,
}

unsafe extern "C" {
    pub(super) fn mr_process_tap_visit_applications(
        visitor: Option<
            unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, *const c_char, i32),
        >,
        context: *mut c_void,
    ) -> NativeError;
    pub(super) fn mr_process_tap_create(
        config: *const NativeConfig,
        error: *mut NativeError,
    ) -> *mut NativeProcessTapSession;
    pub(super) fn mr_process_tap_start(session: *mut NativeProcessTapSession) -> NativeError;
    pub(super) fn mr_process_tap_stop(session: *mut NativeProcessTapSession) -> NativeError;
    pub(super) fn mr_process_tap_refresh_format(
        session: *mut NativeProcessTapSession,
    ) -> NativeError;
    pub(super) fn mr_process_tap_wakeup(session: *const NativeProcessTapSession) -> *const c_void;
    pub(super) fn mr_process_tap_destroy(session: *mut NativeProcessTapSession);
    pub(super) fn mr_process_tap_read(
        session: *mut NativeProcessTapSession,
        destination: *mut f32,
        maximum_frames: usize,
    ) -> usize;
    pub(super) fn mr_process_tap_read_timed(
        session: *mut NativeProcessTapSession,
        destination: *mut f32,
        times: *mut u64,
        maximum_frames: usize,
        generation: mixrelay_process_tap::TapReadGeneration,
    ) -> usize;
    pub(super) fn mr_process_tap_stats(session: *const NativeProcessTapSession) -> NativeStats;
    pub(super) fn mr_process_tap_take_peaks(
        session: *mut NativeProcessTapSession,
        destination: *mut f32,
        maximum_channels: usize,
    ) -> usize;

}
