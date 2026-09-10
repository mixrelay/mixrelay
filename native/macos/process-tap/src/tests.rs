use super::native::Native;
use super::*;
mod retirement;
use std::{cell::Cell, ffi::c_void};
use tonenari_macos_transport::{
    audio_buffer::{AudioBuffer, AudioBufferList},
    capture_ring::{tn_audio_ring_create, tn_audio_ring_read_interleaved},
};

fn format() -> TapFormat {
    TapFormat {
        sample_rate: 48_000.0,
        format_id: u32::from_be_bytes(*b"lpcm"),
        flags: 9,
        bytes_per_packet: 8,
        frames_per_packet: 1,
        bytes_per_frame: 8,
        channels: 2,
        bits: 32,
    }
}

fn fixture() -> TapSession {
    let session = TapSession::default();
    session.resources.set(Resources {
        tap: 1,
        aggregate: 2,
        proc: std::ptr::dangling_mut(),
        listener: true,
        resume: false,
    });
    session.format.set(format()).expect("unpublished format");
    let ring = tn_audio_ring_create(256, 2);
    assert!(!ring.is_null());
    session.ring.set(RingOwner(ring)).expect("unpublished ring");
    session
        .sample_rate
        .store(48_000_f64.to_bits(), Ordering::Release);
    session.generation.store(1, Ordering::Release);
    session.running.store(true, Ordering::Release);
    session
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    None,
    Stop,
    Unregister,
    Listener,
    Aggregate,
    Tap,
    Query,
    Start,
    Register,
}

struct Fake<'a> {
    fail: Cell<Failure>,
    format: Cell<TapFormat>,
    change_during_query: Cell<Option<&'a TapSession>>,
    unregister_calls: Cell<u32>,
}

impl Default for Fake<'_> {
    fn default() -> Self {
        Self {
            fail: Cell::new(Failure::None),
            format: Cell::new(format()),
            change_during_query: Cell::new(None),
            unregister_calls: Cell::new(0),
        }
    }
}

impl Fake<'_> {
    fn result(&self, boundary: Failure) -> Result<(), TapError> {
        if self.fail.get() == boundary {
            TapError::status(-1)
        } else {
            Ok(())
        }
    }
}

impl Native for Fake<'_> {
    fn format(&self, _: u32) -> Result<TapFormat, TapError> {
        if let Some(session) = self.change_during_query.take() {
            // SAFETY: fixture session is borrowed through the complete query.
            session.lifecycle.changed();
        }
        self.result(Failure::Query)?;
        Ok(self.format.get())
    }
    fn start(&self, _: u32, _: *mut c_void) -> Result<(), TapError> {
        self.result(Failure::Start)
    }
    fn stop(&self, _: u32, _: *mut c_void) -> Result<(), TapError> {
        self.result(Failure::Stop)
    }
    fn register(&self, session: &TapSession) -> Result<(), TapError> {
        session.adopt_registration(
            std::ptr::dangling_mut(),
            if self.fail.get() == Failure::Register {
                -1
            } else {
                0
            },
        )
    }
    fn unregister(&self, _: u32, _: *mut c_void) -> Result<(), TapError> {
        self.unregister_calls.set(self.unregister_calls.get() + 1);
        self.result(Failure::Unregister)
    }
    fn remove_listener(&self, _: u32, _: &TapSession) -> Result<(), TapError> {
        self.result(Failure::Listener)
    }
    fn destroy_aggregate(&self, _: u32) -> Result<(), TapError> {
        self.result(Failure::Aggregate)
    }
    fn destroy_tap(&self, _: u32) -> Result<(), TapError> {
        self.result(Failure::Tap)
    }
}

#[test]
fn partial_registration_is_adopted_before_error_and_success_requires_an_identifier() {
    let session = fixture();
    let identifier = std::ptr::dangling_mut();
    assert_eq!(
        session.adopt_registration(identifier, -9),
        Err(TapError {
            code: 4,
            os_status: -9
        })
    );
    assert_eq!(session.resources.get().proc, identifier);
    assert_eq!(
        session
            .adopt_registration(ptr::null_mut(), 0)
            .expect_err("null proc")
            .code,
        4
    );
    assert!(session.resources.get().proc.is_null());
    assert!(session.reclaim(&Fake::default()));
}

#[test]
fn native_property_errors_release_owned_outputs_without_inspecting_them() {
    let value = std::ptr::dangling();
    for (status, size) in [(-1, 8), (0, 4), (0, 8)] {
        let compared = Cell::new(0);
        let released = Cell::new(0);
        let matched = native::finish_property(
            (status, size, value),
            |actual| {
                assert_eq!(actual, value);
                compared.set(compared.get() + 1);
                true
            },
            |actual| {
                assert_eq!(actual, value);
                released.set(released.get() + 1);
            },
        );
        assert_eq!(matched, status == 0 && size == 8);
        assert_eq!(compared.get(), u32::from(matched));
        assert_eq!(released.get(), 1);
    }
    assert!(!native::finish_property(
        (0, 8, ptr::null()),
        |_| panic!("null compared"),
        |_| panic!("null released")
    ));
}

#[test]
fn cleanup_queue_failure_is_rejected_before_session_publication() {
    assert_eq!(
        native::checked_queue(ptr::null_mut)
            .expect_err("allocation failure")
            .code,
        6
    );
    let value = ptr::dangling_mut::<c_void>();
    assert_eq!(native::checked_queue(|| value), Ok(value as usize));
}

#[test]
fn public_capture_boundary_rejects_null_and_invalid_startup_without_native_objects() {
    let mut error = TapError::default();
    // SAFETY: null and invalid startup are explicit API inputs; no dangling pointer
    // is fabricated. Error/result buffers remain exclusive and correctly sized.
    unsafe {
        assert!(tn_process_tap_create(ptr::null(), &raw mut error).is_null());
        assert_eq!(error.code, 1);
        assert_eq!(tn_process_tap_start(ptr::null_mut()).code, 1);
        assert_eq!(tn_process_tap_stop(ptr::null_mut()).code, 1);
        assert_eq!(tn_process_tap_refresh_format(ptr::null_mut()).code, 1);
        assert_eq!(tn_process_tap_read(ptr::null_mut(), ptr::null_mut(), 3), 0);
        assert_eq!(
            tn_process_tap_take_peaks(ptr::null_mut(), ptr::null_mut(), 3),
            0
        );
        assert_eq!(tn_process_tap_stats(ptr::null()).channels, 0);
        tn_process_tap_destroy(ptr::null_mut());
        for capacity in [0, 255, 257, (1 << 20) + 1] {
            let config = TapConfig {
                bundle_id: c"io.tonenari.fixture".as_ptr(),
                mute_while_captured: false,
                ring_capacity_frames: capacity,
            };
            assert!(tn_process_tap_create(&raw const config, &raw mut error).is_null());
            assert_eq!(error.code, 1);
        }
    }
}

#[repr(C)]
struct Buffer {
    count: u32,
    part: AudioBuffer,
}

fn callback(session: &TapSession, samples: &mut [f32], time: Option<&native::Timestamp>) {
    let buffer = Buffer {
        count: 1,
        part: AudioBuffer {
            channels: 2,
            byte_size: u32::try_from(size_of_val(samples)).expect("small fixture"),
            data: samples.as_mut_ptr(),
        },
    };
    // SAFETY: fixture supplies one complete stereo buffer and retains the session/time;
    // callbacks and control are serialized except the separately tested atomic guards.
    unsafe {
        ffi::io_proc(
            2,
            ptr::null(),
            ptr::from_ref(&buffer).cast::<AudioBufferList>(),
            time.map_or(ptr::null(), ptr::from_ref),
            ptr::null_mut(),
            ptr::null(),
            ptr::from_ref(session).cast_mut().cast(),
        )
    };
}

#[test]
fn each_failed_native_teardown_retains_exact_resources_for_one_thousand_cycles() {
    let native = Fake::default();
    for _ in 0..1000 {
        let session = fixture();
        let ring = session.ring();
        native.fail.set(Failure::Stop);
        assert_eq!(session.stop(&native).expect_err("stop failure").code, 4);
        assert!(session.running.load(Ordering::Acquire));
        native.fail.set(Failure::Unregister);
        assert!(!session.reclaim(&native));
        assert!(!session.resources.get().proc.is_null());
        native.fail.set(Failure::Listener);
        assert!(!session.reclaim(&native));
        assert!(session.resources.get().proc.is_null());
        assert!(session.resources.get().listener);
        native.fail.set(Failure::Aggregate);
        assert!(!session.reclaim(&native));
        assert_eq!(session.resources.get().aggregate, 2);
        native.fail.set(Failure::Tap);
        assert!(!session.reclaim(&native));
        assert_eq!(session.resources.get().tap, 1);
        assert_eq!(session.resources.get().aggregate, 0);
        assert_eq!(session.ring(), ring);
        native.fail.set(Failure::None);
        assert!(session.reclaim(&native));
        assert_eq!(session.resources.get().tap, 0);
    }
}

#[test]
fn callback_and_listener_drain_are_independent_reclamation_requirements() {
    let native = Fake::default();
    let session = fixture();
    session.active_callbacks.store(1, Ordering::Release);
    assert!(!session.reclaim(&native));
    assert_eq!(session.resources.get().aggregate, 2);
    session.active_callbacks.store(0, Ordering::Release);
    session.active_listeners.store(1, Ordering::Release);
    assert!(!session.reclaim(&native));
    assert!(!session.resources.get().listener);
    assert_eq!(session.resources.get().tap, 1);
    session.active_listeners.store(0, Ordering::Release);
    assert!(session.reclaim(&native));
}

#[test]
fn rate_refresh_preserves_storage_and_counters_but_drains_stale_pcm() {
    let session = fixture();
    let native = Fake::default();
    let ring = session.ring();
    let mut samples = [0.25, -0.5, 0.75, -1.0];
    callback(&session, &mut samples, None);
    assert_eq!(session.stats().available_frames, 2);
    native.format.set(TapFormat {
        sample_rate: 44_100.0,
        ..format()
    });
    assert!(session.refresh(&native).is_ok());
    assert_eq!(session.ring(), ring);
    let stats = session.stats();
    assert_eq!(stats.available_frames, 0);
    assert_eq!(stats.captured_frames, 2);
    assert_eq!(stats.sample_rate.to_bits(), 44_100_f64.to_bits());
    assert_eq!(stats.format_generation, 2);
    assert!(stats.running);
    assert!(session.reclaim(&native));
}

#[test]
fn invalid_geometry_query_and_failed_restart_never_reopen_delivery() {
    let session = fixture();
    let native = Fake::default();
    native.format.set(TapFormat {
        channels: 1,
        bytes_per_frame: 4,
        bytes_per_packet: 4,
        ..format()
    });
    assert_eq!(
        session
            .refresh(&native)
            .expect_err("incompatible geometry")
            .code,
        5
    );
    assert!(!session.allowed());
    assert_eq!(session.start(&native).expect_err("gated").code, 9);
    assert_eq!(session.format.get().expect("immutable format").channels, 2);
    native.format.set(format());
    native.fail.set(Failure::Query);
    assert_eq!(session.refresh(&native).expect_err("query failure").code, 4);
    native.fail.set(Failure::Register);
    assert_eq!(
        session.refresh(&native).expect_err("register failure").code,
        4
    );
    assert!(!session.allowed());
    native.fail.set(Failure::Start);
    assert_eq!(
        session.refresh(&native).expect_err("restart failure").code,
        4
    );
    assert!(!session.allowed());
    native.fail.set(Failure::None);
    assert!(session.refresh(&native).is_ok());
    assert!(session.allowed() && session.stats().running);
    assert!(session.reclaim(&native));
}

#[test]
fn notification_during_query_rejects_stale_commit_and_retries_fresh_discovery() {
    let session = fixture();
    let native = Fake::default();
    native.format.set(TapFormat {
        sample_rate: 44_100.0,
        ..format()
    });
    native.change_during_query.set(Some(&session));
    assert_eq!(
        session.refresh(&native).expect_err("stale discovery").code,
        9
    );
    assert!(!session.allowed());
    assert!(session.refresh(&native).is_ok());
    assert!(session.allowed());
    assert!(session.reclaim(&native));
}

#[test]
fn whole_callback_retains_clock_credit_for_overflow_and_allocates_nothing() {
    let session = fixture();
    let mut samples = [0.25; 512];
    let measured = allocation_counter::measure(|| {
        callback(&session, &mut samples, None);
        callback(&session, &mut samples, None);
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(session.stats().captured_frames, 256);
    assert_eq!(session.stats().dropped_frames, 256);
    // SAFETY: fixture retains gate and invalidates before the next serialized callback.
    session.lifecycle.changed();
    callback(&session, &mut samples, None);
    assert_eq!(
        session.stats().captured_frames + session.stats().dropped_frames,
        512
    );
    let native = Fake::default();
    assert!(session.refresh(&native).is_ok());
    callback(&session, &mut samples, None);
    assert_eq!(session.stats().captured_frames, 512);
    assert_eq!(session.stats().dropped_frames, 256);
    let mut output = [0.0; 512];
    // SAFETY: sole consumer uses sufficiently sized exclusive output after callback.
    assert_eq!(
        unsafe { tn_audio_ring_read_interleaved(session.ring(), output.as_mut_ptr(), 256) },
        256
    );
    assert_eq!(output.map(f32::to_bits), samples.map(f32::to_bits));
    assert!(session.reclaim(&native));
}

#[test]
fn native_timestamp_continuity_counts_buffer_positions_not_callback_arrival() {
    let session = fixture();
    let mut samples = [0.0; 128];
    let mut time = native::Timestamp::default();
    time.flags = 3;
    time.host = 100;
    callback(&session, &mut samples, Some(&time));
    assert!(session.stats().clock_valid);
    assert_eq!(session.stats().clock_discontinuities, 0);
    time.sample = 128.0;
    time.host = 200;
    callback(&session, &mut samples, Some(&time));
    assert_eq!(session.stats().clock_discontinuities, 1);
    callback(&session, &mut samples, None);
    assert!(!session.stats().clock_valid);
    assert_eq!(session.stats().clock_discontinuities, 2);
    assert!(session.reclaim(&Fake::default()));
}
