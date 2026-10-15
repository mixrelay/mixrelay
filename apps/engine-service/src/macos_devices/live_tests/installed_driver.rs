//! Installed Driver loopback, using ordinary `CoreAudio` admission and native codecs.
//! Explicit UID/width/format select an already configured device; no setting is written.
use crate::macos_devices::{device::Device, ffi, native::Native, spec::Spec, worker_retirement};
use mixrelay_audio_graph::DeviceSelector;
use mixrelay_audio_io::{NativeSampleFormat, NativeStreamFormat};
use mixrelay_driver_control::DriverClient;
use std::{
    cell::UnsafeCell,
    ffi::c_void,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const RATE: u32 = 48_000;
const PRIMING: u32 = RATE;
const PHASE_FRAMES: u64 = 240_000;
const FRAME_LIMIT: u32 = 1 << 20;
const MAX_FRAMES: usize = 4096;

// Two 10-bit words identify every frame before FRAME_LIMIT. Mixing the low word
// into every lane makes each lane change every frame. Disjoint channel bands
// preserve lane identity in every frame. Levels are exact PCM16 binary fractions
// in [-1, 1), also exact in PCM24/32 and Float32.
fn sample_word(word: u16, lane: u16) -> f32 {
    f32::from(lane) / 32.0 - 1.0 + f32::from(word) / 32768.0
}

fn marker(frame: u32, lane: u16) -> f32 {
    let [a, b, c, _] = frame.to_le_bytes();
    let low = u32::from(u16::from_le_bytes([a, b]) & 1023);
    let high = u32::from(u16::from_le_bytes([b, c]) >> 2) & 1023;
    let word = match lane {
        0 => low,
        1 => high + low * 17,
        _ => high + low * (u32::from(lane) * 2 + 1) + u32::from(lane) * 37,
    };
    let [a, b, _, _] = word.to_le_bytes();
    sample_word(u16::from_le_bytes([a, b]) & 1023, lane)
}

fn decode_word(sample: f32, lane: u16) -> Option<u16> {
    if !sample.is_finite() {
        return None;
    }
    let (mut first, mut end) = (0_u16, 1024_u16);
    while first < end {
        let middle = first + (end - first) / 2;
        let expected = sample_word(middle, lane);
        if sample.to_bits() == expected.to_bits() {
            return Some(middle);
        }
        if sample < expected {
            end = middle;
        } else {
            first = middle + 1;
        }
    }
    None
}

#[derive(Debug, PartialEq, Eq)]
struct Fault {
    frame: u32,
    lane: u16,
    expected: u32,
    actual: u32,
}

#[derive(Default)]
struct Sequence {
    seen: u32,
    next: Option<u32>,
    looping: bool,
}

impl Sequence {
    fn check(&mut self, samples: &[f32], channels: u16, priming: u32) -> Result<u32, Fault> {
        let mut checked = 0;
        for frame in samples.chunks_exact(usize::from(channels)) {
            self.seen += 1;
            if self.seen <= priming {
                continue;
            }
            let position = if let Some(next) = self.next {
                next
            } else if let (Some(low), Some(mixed)) =
                (decode_word(frame[0], 0), decode_word(frame[1], 1))
            {
                u32::from(low) | (u32::from(mixed.wrapping_sub(low.wrapping_mul(17)) & 1023) << 10)
            } else {
                return Err(Fault {
                    frame: self.seen,
                    lane: 0,
                    expected: 0,
                    actual: frame[0].to_bits(),
                });
            };
            for (lane, sample) in (0..channels).zip(frame) {
                let expected = marker(position, lane).to_bits();
                if (!self.looping && position >= FRAME_LIMIT) || sample.to_bits() != expected {
                    return Err(Fault {
                        frame: position,
                        lane,
                        expected,
                        actual: sample.to_bits(),
                    });
                }
            }
            self.next = Some(if self.looping {
                (position + 1) % FRAME_LIMIT
            } else {
                position + 1
            });
            checked += 1;
        }
        Ok(checked)
    }
}

#[derive(Default)]
struct Statistics {
    produced: AtomicU64,
    checked: AtomicU64,
    faults: AtomicU32,
    bad_frame: AtomicU32,
    bad_lane: AtomicU32,
    expected: AtomicU32,
    actual: AtomicU32,
}

struct Scratch {
    samples: Box<[f32]>,
    sent: u32,
    previous_frames: u32,
    sequence: Sequence,
    clock_breaks: Option<u64>,
}

struct Probe {
    pointer: *mut c_void,
    channels: u16,
    input: bool,
    scratch: UnsafeCell<Scratch>,
    statistics: Arc<Statistics>,
}

impl Probe {
    fn output(&self, scratch: &mut Scratch, frames: u32) -> bool {
        if !scratch.sequence.looping && scratch.sent + frames >= FRAME_LIMIT {
            self.statistics.faults.fetch_or(1, Ordering::Release);
            return false;
        }
        for (position, frame) in (scratch.sent..scratch.sent + frames)
            .zip(scratch.samples.chunks_exact_mut(usize::from(self.channels)))
        {
            for (lane, sample) in (0..self.channels).zip(frame) {
                *sample = marker(position, lane);
            }
        }
        // SAFETY: this callback is the native output ring's sole producer; the
        // admitted current quantum owns frames*channels initialized samples.
        if unsafe { ffi::mr_device_io_bridge_write(self.pointer, scratch.samples.as_ptr(), frames) }
            != frames
        {
            self.statistics.faults.fetch_or(2, Ordering::Release);
            return false;
        }
        scratch.sent += frames;
        self.statistics
            .produced
            .store(u64::from(scratch.sent), Ordering::Release);
        true
    }

    fn input(&self, scratch: &mut Scratch, frames: u32) -> bool {
        let previous = scratch.previous_frames;
        scratch.previous_frames = frames;
        // The hook precedes native transfer. Only the first callback has no
        // preceding admitted block; later reads must consume that entire block.
        if previous == 0 {
            return true;
        }
        // SAFETY: this callback is the input native ring's sole consumer. The
        // previous native transfer converted precisely this admitted quantum.
        let read = unsafe {
            ffi::mr_device_io_bridge_read(self.pointer, scratch.samples.as_mut_ptr(), previous)
        };
        if read != previous {
            self.statistics.faults.fetch_or(4, Ordering::Release);
            return false;
        }
        let samples = &scratch.samples[..previous as usize * usize::from(self.channels)];
        match scratch.sequence.check(samples, self.channels, PRIMING) {
            Ok(checked) => {
                if scratch.sequence.next.is_some_and(|next| {
                    u64::from(next) > self.statistics.produced.load(Ordering::Acquire)
                }) {
                    self.statistics.faults.fetch_or(32, Ordering::Release);
                    return false;
                }
                self.statistics
                    .checked
                    .fetch_add(u64::from(checked), Ordering::Release);
                true
            }
            Err(fault) => {
                self.statistics
                    .bad_frame
                    .store(fault.frame, Ordering::Relaxed);
                self.statistics
                    .bad_lane
                    .store(u32::from(fault.lane), Ordering::Relaxed);
                self.statistics
                    .expected
                    .store(fault.expected, Ordering::Relaxed);
                self.statistics
                    .actual
                    .store(fault.actual, Ordering::Relaxed);
                self.statistics.faults.fetch_or(8, Ordering::Release);
                false
            }
        }
    }
}

unsafe extern "C" fn callback(context: *mut c_void) -> bool {
    // SAFETY: Client retains this stable Box until Native destroys and drains
    // the registration, including unwind. CoreAudio serializes this IOProc;
    // only it borrows scratch, while control reads separate atomic statistics.
    let probe = unsafe { &*context.cast::<Probe>() };
    if probe.statistics.faults.load(Ordering::Acquire) != 0 {
        return false;
    }
    // SAFETY: retained Native; quantum is an atomic set after actual ASBD buffer
    // validation and before invoking this hook in the same callback.
    let frames = unsafe { ffi::mr_device_io_quantum(probe.pointer, false) };
    if frames == 0 || frames as usize > MAX_FRAMES {
        probe.statistics.faults.fetch_or(16, Ordering::Release);
        return false;
    }
    // SAFETY: the registered native callback exclusively owns this Scratch.
    let scratch = unsafe { &mut *probe.scratch.get() };
    let primed = if probe.input {
        scratch.sequence.seen >= PRIMING
    } else {
        scratch.sent >= PRIMING
    };
    if primed {
        // SAFETY: atomic-only observation of this retained native clock. Its
        // writer recorded the actual callback timestamp before invoking us.
        let breaks = unsafe { ffi::mr_device_io_clock_discontinuities(probe.pointer) };
        if scratch
            .clock_breaks
            .is_some_and(|previous| previous != breaks)
        {
            probe.statistics.faults.fetch_or(64, Ordering::Release);
            return false;
        }
        scratch.clock_breaks = Some(breaks);
    }
    if probe.input {
        probe.input(scratch, frames)
    } else {
        probe.output(scratch, frames)
    }
}

struct Client {
    // Declaration order is the unwind contract: Native unregisters and drains
    // before Rust drops the hook context or its preallocated PCM scratch.
    native: Native,
    probe: Box<Probe>,
}

impl Client {
    fn connect(spec: &Spec, statistics: &Arc<Statistics>) -> Self {
        Self::connect_with_sequence(spec, statistics, false)
    }

    fn connect_with_sequence(spec: &Spec, statistics: &Arc<Statistics>, looping: bool) -> Self {
        let native = Native::create(spec, RATE, None, 0).expect("prepare installed Driver client");
        assert_eq!(
            native.requested_buffer(),
            0,
            "fixture must not write native settings"
        );
        let probe = Box::new(Probe {
            pointer: native.pointer.as_ptr(),
            channels: spec.channels,
            input: spec.input,
            statistics: Arc::clone(statistics),
            scratch: UnsafeCell::new(Scratch {
                samples: vec![0.0; MAX_FRAMES * usize::from(spec.channels)].into_boxed_slice(),
                sent: 0,
                previous_frames: 0,
                sequence: Sequence {
                    looping,
                    ..Sequence::default()
                },
                clock_breaks: None,
            }),
        });
        let client = Self { native, probe };
        // SAFETY: unregistered sole control owner; Client already owns both
        // Native and the hook Box before connect can start or fail partially.
        unsafe {
            ffi::mr_device_io_pcm_hook(
                client.native.pointer.as_ptr(),
                std::ptr::from_ref(client.probe.as_ref()).cast_mut().cast(),
                callback,
            );
        }
        client
            .native
            .connect(&AtomicBool::new(false))
            .expect("connect real CoreAudio client");
        // SAFETY: sole control-side query of the retained connected owner.
        assert_eq!(
            unsafe { ffi::mr_device_io_native_rate(client.native.pointer.as_ptr()) },
            RATE
        );
        client
    }
}

/// Repeats the exact frame/lane words through wrap. A three-second progress
/// watchdog prevents an entire marker period being silently lost or replayed.
#[test]
#[ignore = "explicit installed Driver, exclusive PCM path and soak duration required"]
fn installed_driver_pcm_soak() {
    let seconds: u64 = std::env::var("MIXRELAY_TEST_SOAK_SECONDS")
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        (30..=86_400).contains(&seconds),
        "soak duration must be 30..86400 seconds"
    );
    let output_spec = selection();
    let input_spec = Spec {
        input: true,
        ..output_spec.clone()
    };
    let statistics = Arc::new(Statistics::default());
    let output = Client::connect_with_sequence(&output_spec, &statistics, true);
    let input = Client::connect_with_sequence(&input_spec, &statistics, true);
    let other = Device::open(&input_spec, RATE).expect("ordinary graph capture client");
    let mut other = Some(other);
    let mut detached_at = None;
    let started = Instant::now();
    let deadline = started + Duration::from_secs(seconds + 10);
    let mut last_progress = started;
    let mut previous = 0;
    let mut last_report = started;
    let target = u64::from(RATE) * seconds;
    while statistics.checked.load(Ordering::Acquire) < target {
        assert_no_pcm_faults(&statistics);
        let now = Instant::now();
        let checked = statistics.checked.load(Ordering::Acquire);
        if checked != previous {
            previous = checked;
            last_progress = now;
        }
        assert!(
            now < deadline && now.duration_since(last_progress) < Duration::from_secs(3),
            "soak deadline/progress failed: checked={checked} target={target}"
        );
        if checked >= target / 2 && other.is_some() {
            let retired = Arc::downgrade(&other.as_ref().unwrap().native);
            drop(other.take());
            while retired.upgrade().is_some() {
                worker_retirement::WORKERS.reclaim();
                assert!(Instant::now() < deadline, "soak client did not drain");
                thread::park_timeout(Duration::from_millis(10));
            }
            detached_at = Some(checked);
        }
        if now.duration_since(last_report) >= Duration::from_mins(1) {
            eprintln!(
                "installed_pcm_soak elapsed_seconds={} checked={checked} target={target} detached_at={detached_at:?} faults=0",
                started.elapsed().as_secs()
            );
            last_report = now;
        }
        thread::park_timeout(Duration::from_millis(10));
    }
    assert!(
        started.elapsed() >= Duration::from_secs(seconds - 2),
        "native frame clock ran faster than elapsed time"
    );
    assert_no_client_overflows(&input, &output);
    drop(input);
    drop(output);
    assert_eq!(statistics.faults.load(Ordering::Acquire), 0);
    assert_eq!(
        selection().channels,
        output_spec.channels,
        "native format changed during soak"
    );
    eprintln!(
        "installed_pcm_soak PASS elapsed_seconds={} checked={} channels={} rate={RATE} detached_at={detached_at:?} all_lanes_bit_exact=true faults=0",
        started.elapsed().as_secs(),
        statistics.checked.load(Ordering::Acquire),
        output_spec.channels
    );
}

fn assert_no_client_overflows(input: &Client, output: &Client) {
    for client in [input, output] {
        // SAFETY: connected Client retains the native owner; query is atomic only.
        assert_eq!(
            unsafe { ffi::mr_device_io_overflows(client.native.pointer.as_ptr()) },
            0
        );
    }
}

fn assert_no_pcm_faults(statistics: &Statistics) {
    assert_eq!(
        statistics.faults.load(Ordering::Acquire),
        0,
        "soak PCM fault: frame={} lane={} expected={:#x} actual={:#x}",
        statistics.bad_frame.load(Ordering::Relaxed),
        statistics.bad_lane.load(Ordering::Relaxed),
        statistics.expected.load(Ordering::Relaxed),
        statistics.actual.load(Ordering::Relaxed)
    );
}

#[test]
fn looping_words_validate_wrap_and_reject_a_gap_or_repeat_at_the_boundary() {
    let samples: Vec<_> = (FRAME_LIMIT - 3..FRAME_LIMIT + 3)
        .flat_map(|frame| (0..2).map(move |lane| marker(frame, lane)))
        .collect();
    let mut sequence = Sequence {
        looping: true,
        ..Sequence::default()
    };
    assert_eq!(sequence.check(&samples, 2, 0), Ok(6));
    assert_eq!(sequence.next, Some(3));
    for defect in [0, 1] {
        let mut changed = samples.clone();
        if defect == 0 {
            changed.drain(6..8);
        } else {
            changed.splice(6..6, samples[4..6].iter().copied());
        }
        assert!(
            Sequence {
                looping: true,
                ..Sequence::default()
            }
            .check(&changed, 2, 0)
            .is_err()
        );
    }
}

fn selection() -> Spec {
    let uid = std::env::var("MIXRELAY_TEST_DRIVER_UID")
        .expect("explicit canonical installed Driver UID required");
    let channels: u16 = std::env::var("MIXRELAY_TEST_DRIVER_CHANNELS")
        .expect("explicit expected native width required")
        .parse()
        .expect("native channel count");
    assert!(matches!(channels, 2 | 32 | 64));
    let sample_format = match std::env::var("MIXRELAY_TEST_DRIVER_FORMAT").as_deref() {
        Ok("float32") => NativeSampleFormat::Float32,
        Ok("signed16") => NativeSampleFormat::Signed16,
        Ok("signed24") => NativeSampleFormat::Signed24,
        Ok("signed32") => NativeSampleFormat::Signed32,
        _ => panic!("explicit native float32/signed16/signed24/signed32 required"),
    };
    let snapshot = DriverClient::new()
        .expect("installed Driver control client")
        .snapshot_blocking()
        .expect("read-only actual Driver snapshot");
    let device = snapshot
        .settings
        .devices
        .iter()
        .find(|device| device.native_device_ids.contains(&uid))
        .expect("exact installed UID absent");
    let id =
        mixrelay_macos_transport::endpoint_identity::parse_endpoint_id(device.spec.id.as_str())
            .expect("canonical Driver device identity");
    assert_eq!(
        uid,
        mixrelay_macos_transport::endpoint_identity::endpoint_uid(id)
    );
    let expected = Some(NativeStreamFormat {
        sample_rate: RATE,
        channels,
        sample_format,
    });
    assert_eq!(device.observed_sample_rate, Some(RATE));
    assert_eq!(
        device.native_formats.render.current, expected,
        "actual playback ASBD"
    );
    assert_eq!(
        device.native_formats.capture.current, expected,
        "actual capture ASBD"
    );
    Spec {
        selector: DeviceSelector::PersistentUid { uid },
        input: false,
        channels,
    }
}

fn wait_for_frames(statistics: &Statistics, target: u64, deadline: Instant) {
    while statistics.checked.load(Ordering::Acquire) < target {
        assert_eq!(
            statistics.faults.load(Ordering::Acquire),
            0,
            "PCM fault: frame={} lane={} expected={:#x} actual={:#x}",
            statistics.bad_frame.load(Ordering::Relaxed),
            statistics.bad_lane.load(Ordering::Relaxed),
            statistics.expected.load(Ordering::Relaxed),
            statistics.actual.load(Ordering::Relaxed)
        );
        assert!(
            Instant::now() < deadline,
            "installed PCM timeout: checked={} target={target}",
            statistics.checked.load(Ordering::Acquire)
        );
        // Control waits for observed completion only; callbacks generate and
        // consume all PCM on their actual native clock, without external pacing.
        thread::park_timeout(Duration::from_millis(10));
    }
}

#[test]
fn installed_driver_pcm_continuity_survives_other_client_detach() {
    let output_spec = selection();
    let input_spec = Spec {
        input: true,
        ..output_spec.clone()
    };
    let statistics = Arc::new(Statistics::default());
    let deadline = Instant::now() + Duration::from_secs(20);
    let output = Client::connect(&output_spec, &statistics);
    let input = Client::connect(&input_spec, &statistics);
    // This additional client uses the ordinary graph Device/worker lifecycle.
    let other = Device::open(&input_spec, RATE).expect("ordinary graph capture client");
    let retired = Arc::downgrade(&other.native);
    wait_for_frames(&statistics, PHASE_FRAMES, deadline);
    drop(other);
    while retired.upgrade().is_some() {
        worker_retirement::WORKERS.reclaim();
        assert!(Instant::now() < deadline, "ordinary client did not drain");
        thread::park_timeout(Duration::from_millis(10));
    }
    let detached_at = statistics.checked.load(Ordering::Acquire);
    wait_for_frames(&statistics, detached_at + PHASE_FRAMES, deadline);
    for client in [&input, &output] {
        // SAFETY: Client retains both initialized queues; this query only reads
        // atomic overflow counters and does not consume or alter PCM.
        assert_eq!(
            unsafe { ffi::mr_device_io_overflows(client.native.pointer.as_ptr()) },
            0
        );
    }
    drop(input);
    drop(output);
    assert_eq!(statistics.faults.load(Ordering::Acquire), 0);
    let checked = statistics.checked.load(Ordering::Acquire);
    assert!(checked >= PHASE_FRAMES * 2);
    eprintln!(
        "installed Driver PCM: {:?} channels={} rate={RATE} fresh_frames={checked} detached_at={detached_at} post_detach_frames={} priming={PRIMING} exact_words=true faults=0",
        output_spec.selector,
        output_spec.channels,
        checked - detached_at
    );
}

#[test]
fn words_survive_native_codecs_and_reject_lost_repeated_or_wrong_lanes() {
    use mixrelay_pcm_ring::SampleFormat;
    for channels in [2_u16, 32, 64] {
        let original: Vec<_> = (960..1100)
            .flat_map(|frame| (0..channels).map(move |lane| marker(frame, lane)))
            .collect();
        for format in [
            SampleFormat::Float32,
            SampleFormat::Signed16,
            SampleFormat::Signed24,
            SampleFormat::Signed32,
        ] {
            let mut bytes = vec![0; original.len() * format.bytes_per_sample()];
            let mut restored = vec![0.0; original.len()];
            assert!(format.encode(&original, &mut bytes));
            assert!(format.decode(&bytes, &mut restored));
            assert_eq!(Sequence::default().check(&restored, channels, 0), Ok(140));
        }
        for defect in 0..3 {
            let mut changed = original.clone();
            let at = 40 * usize::from(channels);
            let width = usize::from(channels);
            match defect {
                0 => {
                    changed.drain(at..at + width);
                }
                1 => {
                    changed.splice(at..at, original[at - width..at].iter().copied());
                }
                _ => changed.swap(at, at + 1),
            }
            assert!(Sequence::default().check(&changed, channels, 0).is_err());
        }
    }
    let mut sequence = Sequence::default();
    assert_eq!(sequence.check(&[0.0; 4], 2, 2), Ok(0));
    assert!(sequence.check(&[0.0; 2], 2, 2).is_err());
}
