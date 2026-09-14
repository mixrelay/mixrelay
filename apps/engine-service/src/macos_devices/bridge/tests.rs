use super::*;
use mixrelay_macos_transport::{audio_buffer::AudioBuffer, device_io::mr_device_io_process};
use std::ptr;

#[test]
fn transport_restart_reuses_storage_and_invalidates_old_pcm() {
    let (_device, context) = crate::macos_devices::tests::synthetic_device(2, 48_000, 96_000, true);
    let mut bridge = DeviceBridge::new(&context, 96_000).unwrap();
    bridge.bridge.push_interleaved(&[0.5; 512]).unwrap();
    let epoch = bridge.epoch;
    let storage = (bridge.source.as_ptr(), bridge.destination.as_ptr());
    let allocation = allocation_counter::measure(|| bridge.restart(&context));
    assert_eq!(allocation.count_total, 0);
    assert_eq!(
        storage,
        (bridge.source.as_ptr(), bridge.destination.as_ptr())
    );
    assert_ne!(bridge.epoch, epoch);
    assert!(!bridge.streaming);
    assert!(!context.bridge_ready.load(Ordering::Acquire));
    assert_eq!(bridge.bridge.queued_input_frames(), 0);
    let report = bridge.bridge.pull_interleaved(&mut [0.0; 64]).unwrap();
    assert_eq!(report.transferred_frames, 0);
}

#[test]
#[ignore = "serial before/after timing; run explicitly without other benchmarks"]
fn compare_transport_recreation_and_restart() {
    use std::{hint::black_box, time::Instant};
    let (_device, context) = crate::macos_devices::tests::synthetic_device(2, 48_000, 96_000, true);
    let mut bridge = DeviceBridge::new(&context, 96_000).unwrap();
    // Same geometry, native owner, activation/epoch reset and iteration count.
    // Alternate order each round; the old path includes replacing/dropping its converter.
    for round in 0..6 {
        for recreate in if round % 2 == 0 {
            [true, false]
        } else {
            [false, true]
        } {
            let started = Instant::now();
            let allocations = allocation_counter::measure(|| {
                for _ in 0..100 {
                    if recreate {
                        bridge = DeviceBridge::new(&context, 96_000).unwrap();
                    } else {
                        bridge.restart(&context);
                    }
                    black_box(&bridge);
                }
            });
            eprintln!(
                "transport recovery round={round} recreate={recreate} iterations=100 elapsed_us={} allocations={} bytes={}",
                started.elapsed().as_micros(),
                allocations.count_total,
                allocations.bytes_total
            );
        }
    }
}

#[repr(C)]
struct InputBuffer {
    count: u32,
    buffers: [AudioBuffer; 1],
}

fn native_callback(context: &WorkerContext, native: &mut InputBuffer) {
    unsafe extern "C" fn manually_pumped(_: *mut std::ffi::c_void) -> bool {
        true
    }
    // SAFETY: this synthetic owner has no registered OS callback. The test
    // serializes native/worker/graph roles with complete mono metadata/PCM.
    unsafe {
        // This converter unit fixture explicitly advances its PCM owner itself.
        // No OS callback is registered; callback-owned integration is tested separately.
        crate::macos_devices::ffi::mr_device_io_pcm_hook(
            context.native.pointer.as_ptr(),
            ptr::null_mut(),
            manually_pumped,
        );
        mr_device_io_process(
            0,
            ptr::null(),
            if context.input {
                ptr::from_mut(native).cast()
            } else {
                ptr::null()
            },
            ptr::null(),
            if context.input {
                ptr::null_mut()
            } else {
                ptr::from_mut(native).cast()
            },
            ptr::null(),
            context.native.pointer.as_ptr(),
        );
    }
}

#[test]
fn independent_converter_preserves_pcm_across_three_native_callback_bursts() {
    for input in [true, false] {
        let (device, context) =
            crate::macos_devices::tests::synthetic_device(1, 48_000, 96_000, input);
        let mut bridge = DeviceBridge::new(&context, 96_000).unwrap();
        let mut native_pcm = [0.25_f32; 32];
        let mut native = InputBuffer {
            count: 1,
            buffers: [AudioBuffer {
                channels: 1,
                byte_size: 128,
                data: native_pcm.as_mut_ptr(),
            }],
        };
        let mut graph_pcm = [0.0; 32];
        device.read(&mut graph_pcm);
        native_callback(&context, &mut native);
        if input {
            prime(&context, &device, &mut bridge, &mut native);
        } else {
            for _ in 0..4 {
                device.write(&[0.25; 256]);
                bridge.pump(&context).unwrap();
            }
        }
        let mut missing = 0;
        for tick in 0..30_000 {
            if !input && tick % 2 == 0 {
                assert_eq!(device.write(&[0.25; 32]).frames, 32);
            }
            if tick % 3 == 0 {
                bridge.observe(&context, true).unwrap();
                bridge.pump(&context).unwrap();
            }
            native_callback(&context, &mut native);
            if input && tick % 2 == 0 {
                device.read(&mut graph_pcm);
                if tick > 300 {
                    missing += graph_pcm
                        .iter()
                        .filter(|sample| (**sample - 0.25).abs() > 0.001)
                        .count();
                }
            } else if !input && tick > 300 {
                missing += native_pcm
                    .iter()
                    .filter(|sample| (**sample - 0.25).abs() > 0.001)
                    .count();
            }
        }
        eprintln!(
            "10s native=96k/32 graph=48k/32 worker=1ms input={input}: invalid PCM frames={missing}"
        );
        assert_eq!(
            missing, 0,
            "a complete converter batch must survive normal worker batching"
        );
    }
}

fn prime(
    context: &WorkerContext,
    device: &crate::macos_devices::device::Device,
    bridge: &mut DeviceBridge,
    native: &mut InputBuffer,
) {
    // Real source PCM primes the filter and FIFO; discard only the initial filter
    // tail before measurement, never a short read in the measured stream.
    for pass in 0..2 {
        for _ in 0..64 {
            native_callback(context, native);
            bridge.pump(context).unwrap();
        }
        if pass == 0 {
            for _ in 0..16 {
                device.read(&mut [0.0; 32]);
            }
        }
    }
}

#[test]
fn capture_clock_feedback_bounds_backlog_across_batched_graph_consumption() {
    let mut outcomes = Vec::new();
    for drift_ppm in [-300_i32, 0, 300] {
        let (device, context) =
            crate::macos_devices::tests::synthetic_device(1, 48_000, 96_000, true);
        context.graph_quantum.store(256, Ordering::Release);
        let mut bridge = DeviceBridge::new(&context, 96_000).unwrap();
        let mut input = [0.25_f32; 32];
        let mut native = InputBuffer {
            count: 1,
            buffers: [AudioBuffer {
                channels: 1,
                byte_size: 128,
                data: input.as_mut_ptr(),
            }],
        };
        let mut output = [0.0; 32];
        device.read(&mut output);
        prime(&context, &device, &mut bridge, &mut native);
        let mut credit = 0.0_f64;
        let mut missing = 0;
        let mut peak_backlog = 0;
        for _ in 0..180_000 {
            native_callback(&context, &mut native);
            bridge.observe(&context, true).unwrap();
            bridge.pump(&context).unwrap();
            credit += 16.0 / (1.0 + f64::from(drift_ppm) * 1e-6);
            if credit >= 256.0 {
                credit -= 256.0;
                for _ in 0..8 {
                    let frames = device.read(&mut output).frames;
                    missing += 32 - frames;
                    assert!(output.iter().all(|sample| (*sample - 0.25).abs() < 0.001));
                }
            }
            // SAFETY: the retained native owner provides an atomic queue snapshot.
            let queued = unsafe { mr_device_io_queued(context.native.pointer.as_ptr(), false) };
            peak_backlog = peak_backlog.max(queued);
        }
        let queued = unsafe { mr_device_io_queued(context.native.pointer.as_ptr(), false) };
        let overflow = unsafe { mr_device_io_overflows(context.native.pointer.as_ptr()) };
        eprintln!(
            "60s 96k->48k {drift_ppm:+}ppm 32-native/256-graph: peak native backlog={peak_backlog}, final={queued}, lost={missing}, overflow={overflow}, correction={:.3}ppm",
            bridge.bridge.correction_ppm()
        );
        outcomes.push((
            missing,
            overflow,
            peak_backlog,
            bridge.bridge.correction_ppm() + f64::from(drift_ppm),
        ));
    }
    assert!(
        outcomes
            .iter()
            .all(|&(missing, overflow, peak, error)| missing == 0
                && overflow == 0
                && peak < 4096
                && error.abs() < 50.0),
        "clock drift must not drop PCM or accumulate until a reset: {outcomes:?}"
    );
}
