use super::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct CountingAllocator;
thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static OPERATIONS: Cell<usize> = const { Cell::new(0) };
}

fn count() {
    if COUNTING.try_with(Cell::get).unwrap_or(false) {
        let _ = OPERATIONS.try_with(|value| value.set(value.get() + 1));
    }
}

// SAFETY: all allocations/deallocations are forwarded unchanged to System.
// Thread-local counters isolate the measured audio path from parallel tests.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        count();
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count();
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn prepared_audio_paths_including_feedback_and_reset_do_not_allocate_or_free() {
    for channels in [1, 2, 8, 16, 32, 64] {
        let mut bridge = AsyncClockBridge::new(config(channels, 44_100, 48_000)).unwrap();
        let input = vec![0.25; 128 * channels];
        let mut output = vec![0.0; 256 * channels];
        let feedback = ClockFeedback {
            queue: FeedbackQueue::Output,
            epoch: 0,
            queued_frames: 1200,
            target_frames: 1024,
            elapsed_frames: 128,
            clock_running: true,
        };
        OPERATIONS.set(0);
        COUNTING.set(true);
        for iteration in 0..100 {
            bridge.push_interleaved(&input).unwrap();
            bridge.pull_interleaved(&mut output).unwrap();
            bridge.observe_clock(feedback).unwrap();
            if iteration % 20 == 0 {
                bridge.reset(0, Discontinuity::TransportLoss);
            }
        }
        COUNTING.set(false);
        let operations = OPERATIONS.get();
        assert_eq!(operations, 0, "{channels} channels RT allocator operations");
        eprintln!(
            "RT {channels}ch 44.1->48k: 100 push/pull/feedback cycles +5 resets = {operations} heap operations"
        );
    }
}

#[test]
fn extreme_supported_ratios_and_channel_counts_preserve_bounded_shapes() {
    for channels in [1, 64] {
        for (from, to) in [(8_000, 384_000), (384_000, 8_000)] {
            let mut bridge = AsyncClockBridge::new(config(channels, from, to)).unwrap();
            let input = vec![0.0; 128 * channels];
            let mut output = vec![1.0; 4096 * channels];
            for _ in 0..8 {
                bridge.push_interleaved(&input).unwrap();
                for _ in 0..3 {
                    let report = bridge.pull_interleaved(&mut output).unwrap();
                    assert!(report.transferred_frames <= 4096);
                    assert!(output.iter().all(|sample| *sample == 0.0));
                    assert!(report.queued_input_frames <= 8192);
                }
            }
        }
    }
}
