//! Explicit LAB acceptance: a copied test executable renders only to the named
//! virtual endpoint. The parent captures that owned process, never another app.
use super::{ProcessLoopbackCapture, application_capture, render};
use mixrelay_audio_graph::{ApplicationSelector, AudioDeviceOutputConfig, DeviceSelector};
use mixrelay_audio_io::{process::CycleBlocks, session::SessionFormat};
use std::{
    io::{Read, Write},
    os::windows::process::CommandExt,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const LAB_NAME: &str = "MixRelay SysVAD Bridge LAB";
const CHILD_TEST: &str = "windows::application_signal_tests::owned_virtual_tone_player";

#[derive(Default, Debug)]
pub(super) struct CaptureEvidence {
    pub(super) native_rate: AtomicU32,
    frames: AtomicU64,
    end_position: AtomicU64,
    gap_frames: AtomicU64,
    zero_positions: AtomicU64,
    rewound_positions: AtomicU64,
    discontinuities: AtomicU64,
    silent_frames: AtomicU64,
}

impl CaptureEvidence {
    pub(super) fn packet(&self, position: u64, frames: u32, flags: u32) {
        let old_frames = self.frames.fetch_add(u64::from(frames), Ordering::Relaxed);
        if flags & 4 == 0 {
            let previous = self
                .end_position
                .swap(position + u64::from(frames), Ordering::Relaxed);
            if position == 0 {
                self.zero_positions.fetch_add(1, Ordering::Relaxed);
            }
            if old_frames != 0 && position > previous {
                self.gap_frames
                    .fetch_add(position - previous, Ordering::Relaxed);
            } else if old_frames != 0 && position < previous {
                // Process-loopback can return zero for every native position.
                // Preserve this evidence as unavailable/non-monotonic, not gaps.
                self.rewound_positions.fetch_add(1, Ordering::Relaxed);
            }
        }
        if flags & 1 != 0 {
            self.discontinuities.fetch_add(1, Ordering::Relaxed);
        }
        if flags & 2 != 0 {
            self.silent_frames
                .fetch_add(u64::from(frames), Ordering::Relaxed);
        }
    }
}

fn dump_pcm(name: &str, samples: &[f32]) {
    let directory =
        std::env::var("MIXRELAY_LAB_PCM_DUMP").expect("explicit evidence directory required");
    let path = PathBuf::from(directory).join(name);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .unwrap();
    for sample in samples {
        file.write_all(&sample.to_le_bytes()).unwrap();
    }
    eprintln!(
        "PCM dump {}: stereo float32le samples={}",
        path.display(),
        samples.len()
    );
}

fn authorized() {
    assert_eq!(
        std::env::var("MIXRELAY_LAB_APPLICATION_TEST").as_deref(),
        Ok("1")
    );
}

fn endpoint() -> String {
    let uid = std::env::var("MIXRELAY_LAB_TONE_ENDPOINT")
        .expect("an explicitly inspected LAB virtual endpoint UID is required");
    let devices = mixrelay_audio_io::windows::devices().unwrap();
    let matches: Vec<_> = devices
        .iter()
        .filter(|device| {
            device.uid == uid
                && device.display_name.contains(LAB_NAME)
                && device.output_channels >= 2
        })
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "exactly one installed LAB virtual render endpoint required: {devices:?}"
    );
    eprintln!(
        "owned tone explicit endpoint: {} / {}",
        matches[0].uid, matches[0].display_name
    );
    matches[0].uid.clone()
}

/// Only launched by the parent fixture below. The copied executable gives the
/// child a different exact executable identity from the parent test runner.
#[test]
#[ignore = "child of explicit Windows virtual-tone acceptance only"]
fn owned_virtual_tone_player() {
    authorized();
    let uid = std::env::var("MIXRELAY_LAB_TONE_ENDPOINT").unwrap();
    assert_eq!(uid, endpoint(), "no default or non-LAB render fallback");
    let period: u16 = std::env::var("MIXRELAY_LAB_TONE_PERIOD")
        .unwrap()
        .parse()
        .unwrap();
    assert!([32, 48].contains(&period));
    let graph_rate = std::env::var("MIXRELAY_LAB_TONE_GRAPH_RATE")
        .map_or(48_000, |value| value.parse().unwrap());
    assert!([44_100, 48_000, 96_000].contains(&graph_rate));
    let counters = Arc::default();
    let outputs = render::prepare_outputs_at_rate(
        &[(
            "tone".into(),
            AudioDeviceOutputConfig {
                selector: DeviceSelector::PersistentUid { uid },
                input_channels: 2,
            },
        )],
        &mut render::RenderCache::new(),
        &counters,
        graph_rate,
    )
    .unwrap();
    let output = &outputs["tone"];
    let stop = Arc::new(AtomicBool::new(false));
    let stop_reader = Arc::clone(&stop);
    let reader = thread::spawn(move || {
        let _ = std::io::stdin().read(&mut [0]);
        stop_reader.store(true, Ordering::Release);
    });
    let mut demand = mixrelay_engine_service::render_demand::RenderDemandCursor::default();
    let mut pcm = vec![0.0; 8192];
    let mut source_evidence = Vec::with_capacity(graph_rate as usize * 2);
    let start = Instant::now();
    while !stop.load(Ordering::Acquire) && start.elapsed() < Duration::from_mins(1) {
        assert!(!output.failed(), "explicit LAB render failed");
        let Some(cycle) = output.next_cycle(&mut demand) else {
            thread::sleep(Duration::from_millis(5));
            continue;
        };
        for block in CycleBlocks::new(
            cycle,
            SessionFormat {
                sample_rate: graph_rate,
                block_frames: 128,
            },
        )
        .unwrap()
        {
            let samples = block.frames as usize * 2;
            for (offset, frame) in pcm[..samples].chunks_exact_mut(2).enumerate() {
                let position = block.first_frame + offset as u64;
                frame[0] = tone_sample(position, graph_rate, 48_000.0 / f64::from(period)) * 0.004;
                frame[1] = tone_sample(position, graph_rate, 24_000.0 / f64::from(period)) * 0.002;
            }
            assert_eq!(output.write(&pcm[..samples]), block.frames);
            let remaining = (graph_rate as usize * 2).saturating_sub(source_evidence.len());
            source_evidence.extend_from_slice(&pcm[..samples.min(remaining)]);
        }
    }
    eprintln!(
        "owned render actual native rate={:?}, graph rate={graph_rate}, native xruns={:?}",
        output.native_rate(),
        counters.snapshot()
    );
    dump_pcm(
        &format!("source-{}-{graph_rate}-{period}.f32", std::process::id()),
        &source_evidence,
    );
    drop(outputs);
    // The parent writes one byte and closes stdin before joining the process.
    // On the hard time bound, process exit closes the reader instead of hanging.
    if stop.load(Ordering::Acquire) {
        reader.join().unwrap();
    }
}

fn tone_sample(position: u64, rate: u32, frequency: f64) -> f32 {
    let bounded = u32::try_from(position % u64::from(rate)).unwrap();
    let phase = f64::from(bounded) * std::f64::consts::TAU * frequency / f64::from(rate);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "bounded sine explicitly quantized to float PCM"
    )]
    let sample = phase.sin() as f32;
    sample
}

struct Player {
    _directory: mixrelay_test_support::FixtureDir,
    path: PathBuf,
    child: Option<Child>,
}

impl Player {
    fn new() -> Self {
        let directory = mixrelay_test_support::FixtureDir::new("owned-signal").unwrap();
        let path = directory.join("MixRelayOwnedSignal.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &path).unwrap();
        Self {
            _directory: directory,
            path,
            child: None,
        }
    }

    fn spawn(&mut self, uid: &str, period: u16) {
        self.spawn_at_rate(uid, period, 48_000);
    }

    fn spawn_at_rate(&mut self, uid: &str, period: u16, graph_rate: u32) {
        assert!(self.child.is_none());
        self.child = Some(
            Command::new(&self.path)
                .args([
                    CHILD_TEST,
                    "--exact",
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("MIXRELAY_LAB_TONE_ENDPOINT", uid)
                .env("MIXRELAY_LAB_TONE_PERIOD", period.to_string())
                .env("MIXRELAY_LAB_TONE_GRAPH_RATE", graph_rate.to_string())
                .stdin(Stdio::piped())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .creation_flags(0x0800_0000)
                .spawn()
                .unwrap(),
        );
    }

    fn stop(&mut self) {
        let child = self.child.as_mut().unwrap();
        child.stdin.take().unwrap().write_all(b"x").unwrap();
        let started = Instant::now();
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "owned tone player failed: {status}");
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(5),
                "owned tone player did not stop"
            );
            thread::sleep(Duration::from_millis(10));
        }
        self.child = None;
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        // The directory owner removes only this fixture after its child exits.
    }
}

fn probe(capture: &ProcessLoopbackCapture, frequency: f64) {
    eprintln!(
        "capture before probe graph={} native evidence={:?}",
        capture.graph_rate, capture.ring.capture_evidence
    );
    let mut pcm = vec![0.0; 8192];
    let mut energy = [0.0_f64; 2];
    let mut peak = [0.0_f32; 2];
    let mut crossings = [0_u32; 2];
    let mut raw_crossings = [0_u32; 2];
    let mut armed = [false; 2];
    let mut previous = [0.0_f32; 2];
    let mut frames = 0_u32;
    let mut evidence = Vec::with_capacity(capture.graph_rate as usize * 2);
    let start = Instant::now();
    while frames < capture.graph_rate / 2 {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "no sustained captured tone: frames={frames}, peak={peak:?}"
        );
        let count = capture.read(&mut pcm).frames as usize * 2;
        // Activation can start with silent packets; begin measurement only once
        // the owned tone arrives. No fabricated data or direct ring writes.
        if pcm[..count].iter().all(|sample| sample.abs() < 0.000_01) {
            thread::sleep(Duration::from_millis(5));
            continue;
        }
        for frame in pcm[..count].chunks_exact(2) {
            for channel in 0..2 {
                let sample = frame[channel];
                assert!(sample.is_finite());
                energy[channel] += f64::from(sample).powi(2);
                peak[channel] = peak[channel].max(sample.abs());
                if previous[channel] <= 0.0 && sample > 0.0 {
                    raw_crossings[channel] += 1;
                }
                // Sinc reconstruction may ring around zero during startup or
                // packet silence. Count full excursions, not numerical chatter.
                if sample < -0.0001 {
                    armed[channel] = true;
                } else if sample > 0.0001 && armed[channel] {
                    crossings[channel] += 1;
                    armed[channel] = false;
                }
                previous[channel] = sample;
            }
            frames += 1;
        }
        evidence.extend_from_slice(&pcm[..count]);
    }
    let rms = energy.map(|value| (value / f64::from(frames)).sqrt());
    let measured_hz =
        crossings.map(|value| f64::from(value) * f64::from(capture.graph_rate) / f64::from(frames));
    eprintln!(
        "owned actual PCM: epoch={} frames={frames} peak={peak:?} rms={rms:?} measured_hz={measured_hz:?} raw_zero_crossings={raw_crossings:?}",
        capture.epoch.load(Ordering::Acquire)
    );
    eprintln!(
        "capture after probe graph={} native evidence={:?}, valid graph produced={}, consumed={}",
        capture.graph_rate,
        capture.ring.capture_evidence,
        capture.ring.write.load(Ordering::Acquire) / 2,
        capture.ring.read.load(Ordering::Acquire) / 2
    );
    dump_pcm(
        &format!(
            "capture-{}-{frequency}-{}.f32",
            capture.graph_rate,
            capture.epoch.load(Ordering::Acquire)
        ),
        &evidence,
    );
    assert!((0.001..0.006).contains(&peak[0]));
    assert!((0.001..0.004).contains(&rms[0]));
    assert!(
        (rms[1] / rms[0] - 0.5).abs() < 0.08,
        "stereo channel ratio changed"
    );
    assert!(
        (measured_hz[0] / frequency - 1.0).abs() < 0.1,
        "left tone corrupted or stale"
    );
    assert!(
        (measured_hz[1] / (frequency / 2.0) - 1.0).abs() < 0.1,
        "right tone corrupted or stale"
    );
}

#[test]
#[ignore = "explicit Windows LAB: owned quiet PCM through graph/native SRC, no endpoint format changes"]
fn native_owned_pcm_crosses_44100_and_96000_graph_formats() {
    authorized();
    let uid = endpoint();
    let mut player = Player::new();
    for (render_rate, capture_rate) in [(44_100, 96_000), (96_000, 44_100)] {
        let target = application_capture::Target::new(&ApplicationSelector::ExecutablePath {
            path: player.path.to_str().unwrap().into(),
        })
        .unwrap();
        let capture = ProcessLoopbackCapture::open_target_at_rate(target, 2, capture_rate).unwrap();
        player.spawn_at_rate(&uid, 48, render_rate);
        eprintln!(
            "actual WASAPI SRC: graph render={render_rate}, LAB native=48000, graph capture={capture_rate}"
        );
        probe(&capture, 1000.0);
        player.stop();
        drop(capture);
    }
}

#[test]
#[ignore = "explicit Windows LAB: owned child renders quiet tone only to the MixRelay virtual endpoint"]
fn native_owned_application_nonzero_pcm_survives_relaunch() {
    authorized();
    let uid = endpoint();
    let mut player = Player::new();
    let capture = ProcessLoopbackCapture::open_selector(
        &ApplicationSelector::ExecutablePath {
            path: player.path.to_str().unwrap().into(),
        },
        2,
    )
    .unwrap();
    assert_eq!(
        capture.state.load(Ordering::Acquire),
        application_capture::UNAVAILABLE
    );
    player.spawn(&uid, 48);
    probe(&capture, 1000.0);
    let first_epoch = capture.epoch.load(Ordering::Acquire);
    player.stop();
    let start = Instant::now();
    while capture.state.load(Ordering::Acquire) != application_capture::UNAVAILABLE {
        assert!(start.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(10));
    }
    let mut silent = [f32::NAN; 64];
    assert_eq!(capture.read(&mut silent).frames, 0);
    assert!(silent.iter().all(|sample| *sample == 0.0));
    player.spawn(&uid, 32);
    probe(&capture, 1500.0);
    assert!(capture.epoch.load(Ordering::Acquire) > first_epoch);
    player.stop();
    drop(capture);
    eprintln!(
        "same capture owner: actual tone -> exited/silent -> relaunched/different actual tone; no graph reapply"
    );
}
