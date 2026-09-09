use super::*;
use crate::endpoint_transport::{self as endpoint, EndpointClockSnapshot};
use std::{
    alloc::{Layout, alloc_zeroed, dealloc, handle_alloc_error},
    mem::MaybeUninit,
    ptr::NonNull,
};

struct Storage {
    allocation: NonNull<u8>,
    layout: Layout,
    transport: *mut EndpointTransport,
}
impl Storage {
    fn new(render_channels: u32, capture_channels: u32) -> Self {
        let config = EndpointConfig {
            endpoint_id: [1; 16],
            capacity_frames: 16,
            render_channels,
            capture_channels,
        };
        let bytes = unsafe { endpoint::tn_endpoint_transport_required_bytes(&raw const config) };
        let layout = Layout::from_size_align(bytes, 16).unwrap();
        let allocation = NonNull::new(unsafe { alloc_zeroed(layout) })
            .unwrap_or_else(|| handle_alloc_error(layout));
        let transport = unsafe {
            endpoint::tn_endpoint_transport_initialize(
                allocation.as_ptr().cast(),
                bytes,
                &raw const config,
            )
        };
        assert!(!transport.is_null());
        Self {
            allocation,
            layout,
            transport,
        }
    }
}
impl Drop for Storage {
    fn drop(&mut self) {
        unsafe { dealloc(self.allocation.as_ptr(), self.layout) };
    }
}
struct Fixture {
    context: HalIoContext,
    endpoint: Storage,
    render: *mut SharedAudioBuffer,
    capture: *mut SharedAudioBuffer,
}
impl Fixture {
    fn new() -> Self {
        let endpoint = Storage::new(2, 2);
        let render = unsafe { tn_endpoint_transport_render_buffer(endpoint.transport) };
        let capture = unsafe { tn_endpoint_transport_capture_buffer(endpoint.transport) };
        let mut context = MaybeUninit::uninit();
        assert!(unsafe { tn_hal_io_initialize(context.as_mut_ptr(), 2, 2) });
        let mut context = unsafe { context.assume_init() };
        assert!(unsafe { tn_hal_io_bind(&raw mut context, endpoint.transport.cast()) });
        unsafe {
            tn_hal_io_clock_state(&raw mut context, 48_000, false);
            assert_ne!(buffer::tn_shared_audio_buffer_try_begin_epoch(capture), 0);
        }
        Self {
            context,
            endpoint,
            render,
            capture,
        }
    }
    fn clock(&self) -> EndpointClockSnapshot {
        let mut snapshot = EndpointClockSnapshot::default();
        assert!(unsafe {
            endpoint::tn_endpoint_transport_clock_snapshot(
                self.endpoint.transport,
                &raw mut snapshot,
            )
        });
        snapshot
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        unsafe { tn_hal_io_unbind(&raw mut self.context) };
    }
}

#[test]
fn context_abi_and_integral_sample_domain_are_exact() {
    assert_eq!(std::mem::size_of::<HalIoContext>(), 56);
    assert_eq!(std::mem::offset_of!(HalIoContext, capture_channels), 52);
    let mut context = MaybeUninit::uninit();
    assert!(!unsafe { tn_hal_io_initialize(ptr::null_mut(), 2, 2) });
    for (render, capture) in [(0, 0), (65, 2), (2, 65)] {
        assert!(!unsafe { tn_hal_io_initialize(context.as_mut_ptr(), render, capture) });
    }
    for (render, capture) in [(2, 0), (0, 2), (64, 64)] {
        assert!(unsafe { tn_hal_io_initialize(context.as_mut_ptr(), render, capture) });
        assert!(unsafe { tn_hal_io_is_lock_free(context.as_ptr()) });
    }
    let mut frame = 99;
    for time in [-1.0, f64::NAN, f64::INFINITY, 0.5, MAX_EXACT_FRAME + 1.0] {
        assert!(!unsafe { tn_hal_io_sample_frame(time, &raw mut frame) });
    }
    for (time, expected) in [
        (0.0, 0),
        (-0.0, 0),
        (1.0, 1),
        (12345.0, 12345),
        (MAX_EXACT_FRAME, MAX_FRAME),
    ] {
        assert!(unsafe { tn_hal_io_sample_frame(time, &raw mut frame) });
        assert_eq!(frame, expected);
    }
    assert!(!unsafe { tn_hal_io_sample_frame(0.0, ptr::null_mut()) });
}

#[test]
fn duplex_pcm_demand_and_clock_lifecycle_are_separate() {
    let mut fixture = Fixture::new();
    let previous_epoch = fixture.clock().epoch;
    let pcm = [0.25_f32, -0.25, 0.75, -0.75];
    let mut output = [0.0_f32; 4];
    unsafe {
        tn_hal_io_clock_state(&raw mut fixture.context, 96000, true);
        assert!(buffer::tn_shared_audio_buffer_epoch_pending(
            fixture.capture
        ));
        assert_ne!(
            buffer::tn_shared_audio_buffer_try_begin_epoch(fixture.capture),
            0
        );
        assert!(tn_hal_io_write_render(
            &raw mut fixture.context,
            4,
            pcm.as_ptr(),
            2
        ));
        assert_eq!(
            buffer::tn_shared_audio_buffer_read(fixture.render, 4, output.as_mut_ptr(), 2),
            2
        );
        assert_eq!(pcm.map(f32::to_bits), output.map(f32::to_bits));
        assert!(buffer::tn_shared_audio_buffer_write(
            fixture.capture,
            buffer::tn_shared_audio_buffer_stats(fixture.capture).epoch,
            8,
            pcm.as_ptr(),
            2
        ));
        assert_eq!(
            tn_hal_io_read_capture(&raw mut fixture.context, 8, output.as_mut_ptr(), 2),
            2
        );
    }
    assert_eq!(pcm.map(f32::to_bits), output.map(f32::to_bits));
    assert_ne!(fixture.clock().epoch, previous_epoch);
    assert_eq!(fixture.clock().sample_rate, 96000);
    assert_eq!(fixture.clock().render_through, 6);
    assert_eq!(fixture.clock().capture_through, 10);
    assert_eq!(
        unsafe { endpoint::tn_endpoint_transport_stats(fixture.endpoint.transport) }
            .driver_heartbeat,
        2
    );
}

#[test]
fn capture_restart_requests_writer_rearm_and_reads_silence_until_acknowledged() {
    let mut fixture = Fixture::new();
    let pcm = [0.75_f32; 4];
    let mut output = [1.0_f32; 4];
    unsafe {
        assert!(buffer::tn_shared_audio_buffer_write(
            fixture.capture,
            buffer::tn_shared_audio_buffer_stats(fixture.capture).epoch,
            8,
            pcm.as_ptr(),
            2
        ));
        let previous = buffer::tn_shared_audio_buffer_stats(fixture.render).epoch;
        assert!(tn_hal_io_begin_stream(&raw mut fixture.context));
        assert!(!tn_hal_io_requires_replacement(&raw const fixture.context));
        assert!(buffer::tn_shared_audio_buffer_epoch_pending(
            fixture.capture
        ));
        assert_eq!(
            buffer::tn_shared_audio_buffer_stats(fixture.render).epoch,
            previous + 1
        );
        assert_eq!(
            tn_hal_io_read_capture(&raw mut fixture.context, 8, output.as_mut_ptr(), 2),
            0
        );
        assert!(output.iter().all(|sample| sample.abs() < f32::EPSILON));
        assert!(!buffer::tn_shared_audio_buffer_write(
            fixture.capture,
            buffer::tn_shared_audio_buffer_stats(fixture.capture).epoch,
            8,
            pcm.as_ptr(),
            2
        ));
        assert_ne!(
            buffer::tn_shared_audio_buffer_try_begin_epoch(fixture.capture),
            0
        );
        assert_eq!(
            tn_hal_io_read_capture(&raw mut fixture.context, 8, output.as_mut_ptr(), 2),
            0
        );
        assert!(buffer::tn_shared_audio_buffer_write(
            fixture.capture,
            buffer::tn_shared_audio_buffer_stats(fixture.capture).epoch,
            8,
            pcm.as_ptr(),
            2
        ));
        assert_eq!(
            tn_hal_io_read_capture(&raw mut fixture.context, 8, output.as_mut_ptr(), 2),
            2
        );
    }
    assert_eq!(pcm.map(f32::to_bits), output.map(f32::to_bits));
}

#[test]
fn exhausted_render_generation_refuses_start_and_preserves_pending_mapping() {
    let mut fixture = Fixture::new();
    unsafe {
        for _ in 1..u16::MAX {
            assert_ne!(
                buffer::tn_shared_audio_buffer_try_begin_epoch(fixture.render),
                0
            );
        }
        assert!(!tn_hal_io_begin_stream(&raw mut fixture.context));
        assert!(tn_hal_io_requires_replacement(&raw const fixture.context));
        assert!(buffer::tn_shared_audio_buffer_epoch_pending(fixture.render));
        assert!(!tn_hal_io_begin_stream(&raw mut fixture.context));
        assert_eq!(
            buffer::tn_shared_audio_buffer_try_begin_epoch(fixture.render),
            0
        );
        assert!(!tn_hal_io_write_render(
            &raw mut fixture.context,
            0,
            [0.5_f32; 2].as_ptr(),
            1
        ));
    }
    let mut snapshot = EndpointClockSnapshot::default();
    assert!(!unsafe {
        endpoint::tn_endpoint_transport_clock_snapshot(
            fixture.endpoint.transport,
            &raw mut snapshot,
        )
    });
}

#[test]
fn invalid_bind_preserves_old_route_and_unbind_silences_capture() {
    let mut fixture = Fixture::new();
    let previous = fixture.context.transport.load(Ordering::Acquire);
    let mut output = [1.0_f32; 4];
    unsafe {
        let wrong = Storage::new(1, 2);
        assert!(!tn_hal_io_bind(
            &raw mut fixture.context,
            wrong.transport.cast()
        ));
        assert_eq!(fixture.context.transport.load(Ordering::Acquire), previous);
        assert_eq!(
            tn_hal_io_read_capture(&raw mut fixture.context, MAX_FRAME, output.as_mut_ptr(), 2),
            0
        );
        assert!(output.iter().all(|sample| sample.abs() < f32::EPSILON));
        tn_hal_io_unbind(&raw mut fixture.context);
        output.fill(1.0);
        assert_eq!(
            tn_hal_io_read_capture(&raw mut fixture.context, 0, output.as_mut_ptr(), 2),
            0
        );
        assert!(!tn_hal_io_begin_stream(&raw mut fixture.context));
    }
    assert!(output.iter().all(|sample| sample.abs() < f32::EPSILON));
    assert!(fixture.context.transport.load(Ordering::Acquire).is_null());
    assert_eq!(fixture.clock().running, 0);
}

#[test]
fn unattached_start_preserves_lifecycle_for_later_binding() {
    let mut previous = HalIoContext::new(2, 2).unwrap();
    unsafe {
        assert!(!tn_hal_io_begin_stream(&raw mut previous));
        tn_hal_io_clock_state(&raw mut previous, 96000, true);
    }
    assert_eq!(previous.render_activity.load(Ordering::Acquire), 2);
    let mut replacement = HalIoContext::new(2, 2).unwrap();
    replacement.inherit_lifecycle(&previous);
    assert!(replacement.transport.load(Ordering::Acquire).is_null());
    let transport = Storage::new(2, 2);
    unsafe {
        assert!(tn_hal_io_bind(
            &raw mut replacement,
            transport.transport.cast()
        ));
        let render = tn_endpoint_transport_render_buffer(transport.transport);
        assert_eq!(buffer::tn_shared_audio_buffer_producer_activity(render), 2);
        let mut clock = EndpointClockSnapshot::default();
        assert!(!endpoint::tn_endpoint_transport_clock_snapshot(
            transport.transport,
            &raw mut clock
        ));
        assert!(tn_hal_io_begin_stream(&raw mut replacement));
        tn_hal_io_clock_state(&raw mut replacement, 96_000, true);
        assert!(endpoint::tn_endpoint_transport_clock_snapshot(
            transport.transport,
            &raw mut clock
        ));
        assert_eq!(clock.sample_rate, 96000);
        assert_eq!(clock.running, 1);
        tn_hal_io_unbind(&raw mut replacement);
    }
}

#[test]
fn replacement_identity_preserves_native_lifecycle_without_replaying_old_pcm() {
    let mut previous = Fixture::new();
    let replacement = Storage::new(2, 2);
    let mut next = HalIoContext::new(2, 2).unwrap();
    let mut old_id = [0; 16];
    let mut new_id = [0; 16];
    let mut pcm = [0.0_f32; 2];
    unsafe {
        tn_hal_io_clock_state(&raw mut previous.context, 96000, true);
        assert!(tn_hal_io_write_render(
            &raw mut previous.context,
            4,
            [0.75_f32; 2].as_ptr(),
            1
        ));
        next.inherit_lifecycle(&previous.context);
        assert!(tn_hal_io_bind(&raw mut next, replacement.transport.cast()));
        assert!(tn_hal_io_begin_stream(&raw mut next));
        tn_hal_io_clock_state(&raw mut next, 96_000, true);
        assert!(endpoint::tn_endpoint_transport_incarnation(
            previous.endpoint.transport,
            old_id.as_mut_ptr()
        ));
        assert!(endpoint::tn_endpoint_transport_incarnation(
            replacement.transport,
            new_id.as_mut_ptr()
        ));
        assert_ne!(old_id, new_id);
        let mut clock = EndpointClockSnapshot::default();
        assert!(endpoint::tn_endpoint_transport_clock_snapshot(
            replacement.transport,
            &raw mut clock
        ));
        assert_eq!(
            (clock.sample_rate, clock.running, clock.render_through),
            (96000, 1, 0)
        );
        let render = tn_endpoint_transport_render_buffer(replacement.transport);
        assert_eq!(
            buffer::tn_shared_audio_buffer_read(render, 4, pcm.as_mut_ptr(), 1),
            0
        );
        assert!(pcm.iter().all(|sample| sample.abs() < f32::EPSILON));
        assert!(tn_hal_io_write_render(
            &raw mut next,
            4,
            [0.25_f32; 2].as_ptr(),
            1
        ));
        assert_eq!(
            buffer::tn_shared_audio_buffer_read(render, 4, pcm.as_mut_ptr(), 1),
            1
        );
        assert_eq!(pcm.map(f32::to_bits), [0.25_f32; 2].map(f32::to_bits));
        // An old remote reader retains its old mapping, never sees new identity's PCM.
        assert_eq!(
            buffer::tn_shared_audio_buffer_read(previous.render, 4, pcm.as_mut_ptr(), 1),
            1
        );
        assert_eq!(pcm.map(f32::to_bits), [0.75_f32; 2].map(f32::to_bits));
        tn_hal_io_unbind(&raw mut previous.context);
        assert!(endpoint::tn_endpoint_transport_clock_snapshot(
            replacement.transport,
            &raw mut clock
        ));
        assert_eq!((clock.running, clock.render_through), (1, 5));
        tn_hal_io_unbind(&raw mut next);
    }
}

#[test]
fn committed_clock_rearms_capture_after_any_early_engine_acknowledgment() {
    let mut fixture = Fixture::new();
    let mut output = [1.0_f32; 2];
    unsafe {
        tn_hal_io_clock_state(&raw mut fixture.context, 48_000, true);
        let previous = fixture.clock();
        let mut observed = previous;
        assert!(tn_hal_io_begin_stream(&raw mut fixture.context));
        assert!(!endpoint::tn_endpoint_transport_clock_snapshot(
            fixture.endpoint.transport,
            &raw mut observed
        ));
        assert_eq!(
            observed, previous,
            "unready snapshots leave output untouched"
        );
        // Deliberately emulate an already-in-flight producer acknowledging an
        // early request. New checked service operations cannot enter: the real
        // transport snapshot remains unavailable throughout this interval.
        assert_ne!(
            buffer::tn_shared_audio_buffer_try_begin_epoch(fixture.capture),
            0
        );
        assert!(buffer::tn_shared_audio_buffer_write(
            fixture.capture,
            buffer::tn_shared_audio_buffer_stats(fixture.capture).epoch,
            4,
            [0.75_f32; 2].as_ptr(),
            1
        ));
        assert!(!endpoint::tn_endpoint_transport_clock_snapshot(
            fixture.endpoint.transport,
            &raw mut observed
        ));
        let transitional_epoch = buffer::tn_shared_audio_buffer_stats(fixture.capture).epoch;
        tn_hal_io_clock_state(&raw mut fixture.context, 96000, true);
        let committed = fixture.clock();
        assert_eq!((committed.sample_rate, committed.running), (96_000, 1));
        assert!(committed.epoch > previous.epoch);
        assert!(buffer::tn_shared_audio_buffer_epoch_pending(
            fixture.capture
        ));
        assert_eq!(
            tn_hal_io_read_capture(&raw mut fixture.context, 4, output.as_mut_ptr(), 1),
            0
        );
        assert!(output.iter().all(|sample| sample.abs() < f32::EPSILON));
        assert_ne!(
            buffer::tn_shared_audio_buffer_try_begin_epoch(fixture.capture),
            0
        );
        assert!(!buffer::tn_shared_audio_buffer_write(
            fixture.capture,
            transitional_epoch,
            4,
            [0.75_f32; 2].as_ptr(),
            1
        ));
        assert_eq!(
            tn_hal_io_read_capture(&raw mut fixture.context, 4, output.as_mut_ptr(), 1),
            0
        );
        assert!(buffer::tn_shared_audio_buffer_write(
            fixture.capture,
            buffer::tn_shared_audio_buffer_stats(fixture.capture).epoch,
            4,
            [0.25_f32; 2].as_ptr(),
            1
        ));
        assert_eq!(
            tn_hal_io_read_capture(&raw mut fixture.context, 4, output.as_mut_ptr(), 1),
            1
        );
        assert_eq!(output.map(f32::to_bits), [0.25_f32; 2].map(f32::to_bits));
    }
}

#[test]
fn remote_snapshot_thread_cannot_observe_old_clock_with_reset_pcm_epochs() {
    let mut fixture = Fixture::new();
    unsafe { tn_hal_io_clock_state(&raw mut fixture.context, 48_000, true) };
    let previous = fixture.clock();
    let transport = unsafe { &*fixture.endpoint.transport };
    let capture = unsafe { &*fixture.capture };
    assert!(unsafe { tn_hal_io_begin_stream(&raw mut fixture.context) });
    let (ready, wait) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        scope.spawn(move || {
            let mut snapshot = previous;
            // Use the actual shared clock and ring on a remote observer thread.
            for _ in 0..1000 {
                assert!(!unsafe {
                    endpoint::tn_endpoint_transport_clock_snapshot(transport, &raw mut snapshot)
                });
                assert_eq!(snapshot, previous);
                assert!(unsafe { buffer::tn_shared_audio_buffer_epoch_pending(capture) });
            }
            ready.send(()).unwrap();
        });
        wait.recv().unwrap();
        unsafe { tn_hal_io_clock_state(&raw mut fixture.context, 96_000, true) };
    });
    let committed = fixture.clock();
    assert_eq!(committed.sample_rate, 96_000);
    assert!(committed.epoch > previous.epoch);
}
