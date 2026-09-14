use super::*;
use crate::macos_devices::tests::synthetic_device;
use mixrelay_macos_transport::{audio_buffer::AudioBuffer, device_io::mr_device_io_process};

#[repr(C)]
struct Buffer {
    count: u32,
    buffers: [AudioBuffer; 1],
}

fn tick(context: &WorkerContext, samples: &mut [f32; 32]) {
    let mut buffer = Buffer {
        count: 1,
        buffers: [AudioBuffer {
            channels: 1,
            byte_size: 128,
            data: samples.as_mut_ptr(),
        }],
    };
    // SAFETY: no OS registration exists; this fixture is the sole native caller
    // with a complete mono ABL, disjoint PCM, and a retained callback owner.
    unsafe {
        mr_device_io_process(
            0,
            ptr::null(),
            if context.input {
                ptr::from_ref(&buffer).cast()
            } else {
                ptr::null()
            },
            ptr::null(),
            if context.input {
                ptr::null_mut()
            } else {
                ptr::from_mut(&mut buffer).cast()
            },
            ptr::null(),
            context.native.pointer.as_ptr(),
        );
    }
}

#[test]
fn native_callback_alone_converts_mixed_rates_without_polling_or_rt_allocation() {
    for input in [true, false] {
        let (device, context) = synthetic_device(1, 48_000, 96_000, input);
        let owner = CallbackBridge::new(&context, 96_000).unwrap();
        let mut native = [0.25; 32];
        let mut graph = [0.0; 32];
        device.read(&mut graph);
        let mut missing = 0;
        let allocation = allocation_counter::measure(|| {
            for cycle in 0..30_000 {
                if !input && cycle % 2 == 0 {
                    assert_eq!(device.write(&[0.25; 32]).frames, 32);
                }
                tick(&context, &mut native);
                if input && cycle % 2 == 0 {
                    device.read(&mut graph);
                    if cycle > 300 {
                        missing += graph.iter().filter(|x| (**x - 0.25).abs() > 0.001).count();
                    }
                } else if !input && cycle > 300 {
                    missing += native.iter().filter(|x| (**x - 0.25).abs() > 0.001).count();
                }
            }
        });
        assert_eq!(missing, 0, "native callback continuity input={input}");
        assert_eq!(allocation.count_total, 0);
        assert!(context.native.pcm.failure().is_none());
        if !input {
            // SAFETY: the fixture retains native storage; counter access is atomic.
            assert_eq!(
                unsafe { ffi::mr_device_io_starvations(context.native.pointer.as_ptr(), false) },
                0
            );
        }
        drop(owner);
        assert!(context.native.pcm.state.load(Ordering::Acquire).is_null());
    }
}

#[test]
fn off_on_is_acknowledged_by_callback_and_dropped_owner_cannot_run_again() {
    let (device, context) = synthetic_device(1, 48_000, 48_000, false);
    context.clock_aligned.store(true, Ordering::Release);
    let owner = CallbackBridge::new(&context, 48_000).unwrap();
    let mut native = [0.0; 32];
    tick(&context, &mut native);
    device.write(&[0.25; 32]);
    tick(&context, &mut native);
    assert_eq!(native.map(f32::to_bits), [0.25_f32.to_bits(); 32]);
    device.set_active(false);
    tick(&context, &mut native);
    assert_eq!(native.map(f32::to_bits), [0.0_f32.to_bits(); 32]);
    device.set_active(true);
    tick(&context, &mut native);
    device.write(&[0.75; 32]);
    tick(&context, &mut native);
    assert_eq!(native.map(f32::to_bits), [0.75_f32.to_bits(); 32]);
    drop(owner);
    tick(&context, &mut native);
    assert_eq!(native.map(f32::to_bits), [0.0_f32.to_bits(); 32]);
}

#[test]
fn reader_drain_precedes_replacement_or_freeing_callback_state() {
    let (_device, context) = synthetic_device(1, 48_000, 48_000, false);
    let owner = CallbackBridge::new(&context, 48_000).unwrap();
    let slot = &context.native.pcm;
    slot.readers.store(1, Ordering::SeqCst);
    let drained = AtomicBool::new(false);
    thread::scope(|scope| {
        scope.spawn(|| {
            drop(owner);
            drained.store(true, Ordering::Release);
        });
        thread::sleep(Duration::from_millis(5));
        assert!(!drained.load(Ordering::Acquire));
        slot.readers.store(0, Ordering::SeqCst);
    });
    assert!(slot.state.load(Ordering::SeqCst).is_null());
}

#[test]
fn resumed_output_primes_real_converted_pcm_before_admitting_native_reads() {
    let (device, context) = synthetic_device(1, 48_000, 96_000, false);
    let _owner = CallbackBridge::new(&context, 96_000).unwrap();
    let mut native = [0.0; 32];
    let allocation = allocation_counter::measure(|| {
        for cycle in 0..3000 {
            if cycle % 300 == 100 {
                device.set_active(false);
            }
            if cycle % 300 == 200 {
                device.set_active(true);
            }
            if cycle % 2 == 0 {
                device.write(&[0.25; 32]);
            }
            tick(&context, &mut native);
        }
    });
    assert_eq!(allocation.count_total, 0);
    assert!(native.iter().all(|sample| (*sample - 0.25).abs() < 0.001));
    // SAFETY: retained synthetic owner, atomic snapshots of the native consumer.
    assert_eq!(
        unsafe { ffi::mr_device_io_starvations(context.native.pointer.as_ptr(), false) },
        0
    );
    assert_eq!(
        unsafe { ffi::mr_device_io_overflows(context.native.pointer.as_ptr()) },
        0
    );
}

#[test]
fn control_lock_cannot_block_pcm_and_format_replacement_preserves_owner() {
    let (_device, context) = synthetic_device(1, 48_000, 48_000, true);
    let mut owner = CallbackBridge::new(&context, 48_000).unwrap();
    thread::scope(|scope| {
        let guard = context.pending_bridge.lock().unwrap();
        let (sent, received) = std::sync::mpsc::sync_channel(1);
        let callback_context = &context;
        scope.spawn(move || {
            tick(callback_context, &mut [0.25; 32]);
            sent.send(()).unwrap();
        });
        let completed = received.recv_timeout(Duration::from_secs(1)).is_ok();
        drop(guard);
        assert!(completed, "control lock blocked native PCM");
    });
    let pointer = context.native.pointer;
    context.graph_rate.store(96_000, Ordering::Release);
    tick(&context, &mut [0.25; 32]);
    assert_eq!(context.ready_rate.load(Ordering::Acquire), 0);
    owner.replace(DeviceBridge::prepare(1, true, 48_000, 96_000, false).unwrap());
    tick(&context, &mut [0.25; 32]);
    assert_eq!(context.ready_rate.load(Ordering::Acquire), 96_000);
    assert_eq!(context.native.pointer, pointer);
}
