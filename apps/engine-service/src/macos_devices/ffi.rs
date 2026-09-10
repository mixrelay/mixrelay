//! `CoreAudio` C ABI. Ownership and thread rules live in native.rs.
use std::ffi::{c_char, c_void};
unsafe extern "C" {
    pub(super) fn mr_device_io_render_hook(
        io: *mut c_void,
        context: *mut c_void,
        hook: unsafe extern "C" fn(*mut c_void, *const c_void, *mut c_void, u32) -> bool,
    );
    pub(super) fn mr_device_io_direct_binding(
        io: *mut c_void,
        device: *mut u32,
        offset: *mut u32,
        total: *mut u32,
    ) -> bool;
    pub(super) fn mr_aggregate_group_create(
        uids: *const *const c_char,
        members: *const super::group_plan::MemberInfo,
        count: u32,
        primary: u32,
        status: *mut i32,
    ) -> *mut c_void;
    pub(super) fn mr_aggregate_member_info(
        uid: *const c_char,
        info: *mut super::group_plan::MemberInfo,
    ) -> bool;
    pub(super) fn mr_aggregate_group_try_destroy(group: *mut c_void) -> bool;
    pub(super) fn mr_aggregate_group_ready(group: *mut c_void) -> bool;
    pub(super) fn mr_device_io_create_group(
        group: *mut c_void,
        uid: *const c_char,
        input: bool,
        channels: u32,
        rate: u32,
    ) -> *mut c_void;
    pub(super) fn mr_device_io_connect_status(io: *mut c_void, expected_device: u32) -> i32;
    pub(super) fn mr_device_io_buffer_info(
        io: *mut c_void,
        info: *mut super::buffer_control::BufferInfo,
    ) -> bool;
    pub(super) fn mr_device_buffer_write(device: u32, frames: u32) -> bool;
    pub(super) fn mr_device_buffer_size(device: u32) -> u32;
    pub(super) fn mr_device_io_disconnect(io: *mut c_void) -> bool;
    pub(super) fn mr_device_io_destroy(io: *mut c_void) -> bool;
    pub(super) fn mr_device_io_alive(io: *mut c_void) -> bool;
    pub(super) fn mr_device_io_ticks(io: *mut c_void) -> u64;
    pub(super) fn mr_device_io_native_frames(io: *mut c_void) -> u64;
    pub(super) fn mr_device_io_native_rate(io: *mut c_void) -> u32;
    pub(super) fn mr_device_io_clock_device(io: *mut c_void) -> u32;
    pub(super) fn mr_device_io_clock_discontinuities(io: *mut c_void) -> u64;
    pub(super) fn mr_device_io_graph_frames(io: *mut c_void) -> u64;
    pub(super) fn mr_device_io_overflows(io: *mut c_void) -> u64;
    pub(super) fn mr_device_io_quantum(io: *mut c_void, graph: bool) -> u32;
    pub(super) fn mr_device_io_queued(io: *mut c_void, graph: bool) -> u32;
    pub(super) fn mr_device_io_worker_discard(io: *mut c_void);
    pub(super) fn mr_device_io_output_epoch(io: *mut c_void, epoch: u64);
    pub(super) fn mr_device_io_worker_read(io: *mut c_void, samples: *mut f32, frames: u32) -> u32;
    pub(super) fn mr_device_io_worker_write(
        io: *mut c_void,
        samples: *const f32,
        frames: u32,
    ) -> u32;
    pub(super) fn mr_device_io_graph_read(
        io: *mut c_void,
        samples: *mut f32,
        frames: u32,
        epoch: u64,
    ) -> u32;
    pub(super) fn mr_device_io_graph_write(
        io: *mut c_void,
        samples: *const f32,
        frames: u32,
    ) -> u32;
}
