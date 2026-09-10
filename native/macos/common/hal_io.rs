//! HAL callback routing, with separate writer authority for each direction.
//!
//! Control binds/unbinds only after callback readers drain. HAL is the render
//! writer, but only requests capture invalidation: the engine owns that writer.
//! Exhausted generations stay pending until control replaces the mapping.
use crate::endpoint_transport::{
    EndpointTransport, EndpointTransportConfig as EndpointConfig,
    mr_endpoint_transport_begin_clock_transition, mr_endpoint_transport_capture_buffer,
    mr_endpoint_transport_clock_demand, mr_endpoint_transport_clock_state,
    mr_endpoint_transport_config, mr_endpoint_transport_render_buffer,
    mr_endpoint_transport_requires_replacement, mr_endpoint_transport_touch_driver,
};
use crate::shared_audio_buffer::{self as buffer, SharedAudioBuffer};
use std::{
    ffi::c_void,
    ptr,
    sync::atomic::{AtomicPtr, AtomicU32, Ordering},
};

const MAX_FRAME: u64 = (1 << 48) - 1;
const MAX_EXACT_FRAME: f64 = 281_474_976_710_655.0;

#[repr(C)]
pub struct HalIoContext {
    render_activity: AtomicU32,
    clock_epoch: u64,
    clock_sample_rate: u32,
    clock_running: bool,
    transport: AtomicPtr<EndpointTransport>,
    render_buffer: AtomicPtr<SharedAudioBuffer>,
    capture_buffer: AtomicPtr<SharedAudioBuffer>,
    pub render_channels: u32,
    pub capture_channels: u32,
}
const _: () = assert!(std::mem::size_of::<HalIoContext>() == 56);
const _: () = assert!(std::mem::align_of::<HalIoContext>() == 8);
const _: () = assert!(std::mem::offset_of!(HalIoContext, transport) == 24);
const _: () = assert!(std::mem::offset_of!(HalIoContext, render_channels) == 48);
const _: () = assert!(cfg!(target_has_atomic = "ptr") && cfg!(target_has_atomic = "32"));

impl HalIoContext {
    /// Constructs an unattached control-owned context without allocation.
    #[must_use]
    pub fn new(render: u32, capture: u32) -> Option<Self> {
        valid_channels(render, capture).then(|| Self {
            render_activity: AtomicU32::new(1),
            clock_epoch: 0,
            clock_sample_rate: 0,
            clock_running: false,
            transport: AtomicPtr::new(ptr::null_mut()),
            render_buffer: AtomicPtr::new(ptr::null_mut()),
            capture_buffer: AtomicPtr::new(ptr::null_mut()),
            render_channels: render,
            capture_channels: capture,
        })
    }

    /// Prepares a replacement before binding. Does not alias its mapping or
    /// publish lifecycle changes into the currently attached transport.
    pub fn inherit_lifecycle(&mut self, previous: &Self) {
        self.clock_epoch = previous.clock_epoch;
        self.clock_sample_rate = previous.clock_sample_rate;
        self.clock_running = previous.clock_running;
        self.render_activity.store(
            previous.render_activity.load(Ordering::Acquire),
            Ordering::Relaxed,
        );
    }
}

fn valid_channels(render: u32, capture: u32) -> bool {
    (render != 0 || capture != 0) && render <= 64 && capture <= 64
}
fn valid_range(start: u64, count: u32) -> bool {
    start <= MAX_FRAME
        && start
            .checked_add(u64::from(count))
            .is_some_and(|end| end <= MAX_FRAME)
}
unsafe fn silence(samples: *mut f32, count: u32, channels: u32) {
    if !samples.is_null() {
        // SAFETY: ABI caller provides count * configured channels writable samples.
        unsafe { ptr::write_bytes(samples, 0, count as usize * channels as usize) };
    }
}

/// # Safety
/// Non-null context is aligned, caller-owned, and exclusive until publication.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_hal_io_initialize(
    context: *mut HalIoContext,
    render: u32,
    capture: u32,
) -> bool {
    if context.is_null() {
        return false;
    }
    let Some(initialized) = HalIoContext::new(render, capture) else {
        return false;
    };
    unsafe {
        context.write(initialized);
    }
    true
}

/// # Safety
/// Context and transport are live. Control has drained all context callbacks;
/// transport mappings remain owned until a later drained unbind.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_hal_io_bind(
    context: *mut HalIoContext,
    transport: *mut c_void,
) -> bool {
    let Some(context) = (unsafe { context.as_ref() }) else {
        return false;
    };
    let transport = transport.cast::<EndpointTransport>();
    let mut config = EndpointConfig::default();
    if transport.is_null()
        || !unsafe { mr_endpoint_transport_config(transport, &raw mut config) }
        || config.render_channels != context.render_channels
        || config.capture_channels != context.capture_channels
    {
        return false;
    }
    let render = unsafe { mr_endpoint_transport_render_buffer(transport) };
    let capture = unsafe { mr_endpoint_transport_capture_buffer(transport) };
    if (context.render_channels != 0 && render.is_null())
        || (context.capture_channels != 0 && capture.is_null())
    {
        return false;
    }
    if !unsafe { mr_endpoint_transport_begin_clock_transition(transport) } {
        return false;
    }
    context.render_buffer.store(render, Ordering::Relaxed);
    context.capture_buffer.store(capture, Ordering::Relaxed);
    context.transport.store(transport, Ordering::Release);
    unsafe {
        // Binding is preparation, not a ready lifecycle publication. Inherited
        // rate/running fields become visible only after the caller resets PCM
        // epochs and commits through clock_state.
        buffer::mr_shared_audio_buffer_set_producer_activity(
            render,
            context.render_activity.load(Ordering::Acquire),
        );
    }
    true
}

/// # Safety
/// Non-null context is live and all callbacks have drained. Its mapping owner
/// may retire the transport only after this call returns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_hal_io_unbind(context: *mut HalIoContext) {
    let Some(context) = (unsafe { context.as_ref() }) else {
        return;
    };
    unsafe {
        mr_endpoint_transport_begin_clock_transition(context.transport.load(Ordering::Acquire));
        buffer::mr_shared_audio_buffer_request_epoch(
            context.capture_buffer.load(Ordering::Acquire),
        );
        mr_endpoint_transport_clock_state(
            context.transport.load(Ordering::Acquire),
            context.clock_epoch,
            context.clock_sample_rate,
            false,
        );
        buffer::mr_shared_audio_buffer_set_producer_activity(
            context.render_buffer.load(Ordering::Acquire),
            0,
        );
    }
    context.transport.store(ptr::null_mut(), Ordering::Release);
    context
        .render_buffer
        .store(ptr::null_mut(), Ordering::Relaxed);
    context
        .capture_buffer
        .store(ptr::null_mut(), Ordering::Relaxed);
}

/// # Safety
/// A non-null context must be initialized, aligned and live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_hal_io_is_lock_free(context: *const HalIoContext) -> bool {
    !context.is_null()
}

/// Distinguishes terminal mapping state from an ordinary capture rearm request.
/// # Safety
/// Non-null context and its attached mapping remain live throughout this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_hal_io_requires_replacement(context: *const HalIoContext) -> bool {
    let Some(context) = (unsafe { context.as_ref() }) else {
        return false;
    };
    unsafe {
        mr_endpoint_transport_requires_replacement(context.transport.load(Ordering::Acquire))
            || buffer::mr_shared_audio_buffer_requires_replacement(
                context.render_buffer.load(Ordering::Acquire),
            )
            || buffer::mr_shared_audio_buffer_requires_replacement(
                context.capture_buffer.load(Ordering::Acquire),
            )
    }
}

/// # Safety
/// Context and its attached transport remain live through this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_hal_io_set_render_activity(context: *mut HalIoContext, activity: u32) {
    let Some(context) = (unsafe { context.as_ref() }) else {
        return;
    };
    context.render_activity.store(activity, Ordering::Release);
    unsafe {
        buffer::mr_shared_audio_buffer_set_producer_activity(
            context.render_buffer.load(Ordering::Acquire),
            activity,
        );
    };
}

/// # Safety
/// Control owns context exclusively: clock lifecycle mutations cannot race bind
/// or unbind. PCM callbacks do not read these control-only plain fields.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_hal_io_clock_state(
    context: *mut HalIoContext,
    sample_rate: u32,
    running: bool,
) {
    let Some(context) = (unsafe { context.as_mut() }) else {
        return;
    };
    let transport = context.transport.load(Ordering::Acquire);
    if !transport.is_null() && !unsafe { mr_endpoint_transport_begin_clock_transition(transport) } {
        return;
    }
    context.clock_epoch = context.clock_epoch.wrapping_add(1);
    context.clock_sample_rate = sample_rate;
    context.clock_running = running;
    unsafe {
        // No observer can combine the previous clock with a newly reset ring:
        // begin_stream hid the clock before resetting any PCM epoch. In-flight
        // old writers retain their expected epoch and fail after this rearm;
        // writer claims prevent consuming rearm until the preceding writer drains.
        buffer::mr_shared_audio_buffer_request_epoch(
            context.capture_buffer.load(Ordering::Acquire),
        );
        mr_endpoint_transport_clock_state(transport, context.clock_epoch, sample_rate, running);
    };
}

/// # Safety
/// Control has serialized lifecycle transitions and drained this HAL writer.
/// Remote engine writers/readers need not be drained: no shared storage clears.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_hal_io_begin_stream(context: *mut HalIoContext) -> bool {
    let Some(context) = (unsafe { context.as_ref() }) else {
        return false;
    };
    let transport = context.transport.load(Ordering::Acquire);
    let capture = context.capture_buffer.load(Ordering::Relaxed);
    let render = context.render_buffer.load(Ordering::Relaxed);
    // A client start is lifecycle evidence even if the mapping has not arrived.
    // Retain it in the context so a later bind publishes AwaitingRender instead
    // of falsely reporting inactivity throughout the client's running lifetime.
    context.render_activity.store(2, Ordering::Release);
    unsafe { buffer::mr_shared_audio_buffer_set_producer_activity(render, 2) };
    if transport.is_null()
        || (context.capture_channels != 0 && capture.is_null())
        || (context.render_channels != 0 && render.is_null())
    {
        return false;
    }
    if !unsafe { mr_endpoint_transport_begin_clock_transition(transport) } {
        return false;
    }
    unsafe {
        // Capture is reader-owned here: mark pending, never consume its epoch.
        if !capture.is_null() {
            buffer::mr_shared_audio_buffer_request_epoch(capture);
        }
        // Render is HAL-owned. Exhaustion marks pending and fails the start;
        // engine control must replace this endpoint, including failed-start cases.
        if render.is_null() || buffer::mr_shared_audio_buffer_try_begin_epoch(render) != 0 {
            return true;
        }
        // The native registry holds control + Device::freeze around every
        // start/replace/rate lifecycle, so this sole HAL writer has drained.
        // A remaining render claim may belong to a crashed predecessor HAL;
        // retire that mapping rather than clearing busy or retrying forever.
        // Capture is deliberately excluded: its remote engine writer may live.
        buffer::mr_shared_audio_buffer_retire_busy_producer_after_drain(render);
        false
    }
}

/// # Safety
/// Non-null output points to one writable u64.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_hal_io_sample_frame(time: f64, frame: *mut u64) -> bool {
    if frame.is_null()
        || !time.is_finite()
        || !(0.0..=MAX_EXACT_FRAME).contains(&time)
        || time.fract() != 0.0
    {
        return false;
    }
    // Decode the integral IEEE-754 value instead of an unchecked float cast.
    let bits = time.to_bits();
    let converted = if time < 1.0 {
        0
    } else {
        let exponent = (bits >> 52) & 0x7ff;
        let significand = (bits & ((1_u64 << 52) - 1)) | (1_u64 << 52);
        significand >> (1075 - exponent)
    };
    unsafe {
        frame.write(converted);
    }
    true
}

/// # Safety
/// Context/mapping remain live and HAL is the single serialized render writer.
/// Samples reference count * configured render channels readable Float32 values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_hal_io_write_render(
    context: *mut HalIoContext,
    start: u64,
    samples: *const f32,
    count: u32,
) -> bool {
    let Some(context) = (unsafe { context.as_ref() }) else {
        return false;
    };
    if context.render_channels == 0
        || (count != 0 && samples.is_null())
        || !valid_range(start, count)
    {
        return false;
    }
    let transport = context.transport.load(Ordering::Acquire);
    let render = context.render_buffer.load(Ordering::Relaxed);
    if transport.is_null()
        || render.is_null()
        || !unsafe {
            // Lifecycle changes drain this sole HAL writer. A reader request
            // may still race us; the expected generation makes that write fail
            // rather than publishing into the subsequently acknowledged epoch.
            let epoch = buffer::mr_shared_audio_buffer_stats(render).epoch;
            buffer::mr_shared_audio_buffer_write(render, epoch, start, samples, count)
        }
    {
        return false;
    }
    unsafe {
        if count != 0 {
            context.render_activity.store(3, Ordering::Release);
            buffer::mr_shared_audio_buffer_set_producer_activity(render, 3);
            mr_endpoint_transport_clock_demand(transport, true, start + u64::from(count), count);
        }
        mr_endpoint_transport_touch_driver(transport);
    }
    true
}

/// # Safety
/// Context/mapping remain live. Samples reference count * configured capture
/// channels writable Float32 values. Pending generations always read silence.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mr_hal_io_read_capture(
    context: *mut HalIoContext,
    start: u64,
    samples: *mut f32,
    count: u32,
) -> u32 {
    let Some(context) = (unsafe { context.as_ref() }) else {
        return 0;
    };
    if context.capture_channels == 0 || (count != 0 && samples.is_null()) {
        return 0;
    }
    let transport = context.transport.load(Ordering::Acquire);
    let capture = context.capture_buffer.load(Ordering::Relaxed);
    if !valid_range(start, count) || transport.is_null() || capture.is_null() {
        unsafe { silence(samples, count, context.capture_channels) };
        return 0;
    }
    let available = unsafe { buffer::mr_shared_audio_buffer_read(capture, start, samples, count) };
    unsafe {
        // Real callback demand exists even while a pending mapping returns silence.
        if count != 0 {
            mr_endpoint_transport_clock_demand(transport, false, start + u64::from(count), count);
        }
        mr_endpoint_transport_touch_driver(transport);
    }
    available
}

#[cfg(test)]
#[path = "unit_tests/hal_io.rs"]
mod tests;
