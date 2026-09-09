use super::*;

fn target(serial: &str, pid: u32, app_id: &str) -> PipeWireTarget {
    PipeWireTarget {
        object_id: 1,
        serial: serial.into(),
        node_name: Some(format!("node-{serial}")),
        process_id: Some(pid),
        application_id: Some(app_id.into()),
        executable: Some("player".into()),
        icon_name: None,
        display_name: "Same display name".into(),
        media_class: "Stream/Output/Audio".into(),
        channels: 2,
    }
}
fn process(pid: u32, start: u64, parent: u32) -> ProcessIdentity {
    ProcessIdentity {
        process_id: pid,
        start_time: start,
        parent_id: parent,
        executable_path: Some("/usr/bin/player".into()),
    }
}
fn selector() -> ApplicationSelector {
    ApplicationSelector::BundleId {
        bundle_id: "org.test.player".into(),
        include_descendants: false,
    }
}
fn request(selector: &ApplicationSelector) -> ApplicationCaptureRequest {
    serde_json::from_value(serde_json::json!({"selector": selector, "stream":{"channels":2,"channelLayout":"stereo"},"sourcePlayback":"preserve"})).unwrap()
}
fn watch() -> WatchEntry {
    WatchEntry {
        format: super::super::render_format(),
        node_id: "app".into(),
        request: request(&selector()),
        pinned_process: None,
        serials: Vec::new(),
        healthy: Arc::new(AtomicBool::new(true)),
    }
}

#[test]
fn exact_application_id_follows_new_streams_without_display_name_aliases() {
    let inventory = [
        target("a", 100, "org.test.player"),
        target("b", 101, "unrelated"),
    ];
    assert_eq!(
        serials(&matching_targets(&inventory, &selector(), None, |_| None)),
        ["a"]
    );
    let mut misleading = target("c", 102, "unrelated");
    misleading.executable = Some("org.test.player".into());
    assert!(matching_targets(&[misleading], &selector(), None, |_| None).is_empty());
    let restarted = [
        target("d", 200, "org.test.player"),
        target("e", 200, "org.test.player"),
    ];
    assert_eq!(
        serials(&matching_targets(&restarted, &selector(), None, |_| None)),
        ["d", "e"]
    );
}

#[test]
fn executable_selector_uses_kernel_path_not_basename_guess() {
    let selector = ApplicationSelector::ExecutablePath {
        path: "/usr/bin/player".into(),
    };
    let inventory = [target("a", 100, "ignored")];
    assert_eq!(
        matching_targets(&inventory, &selector, None, |pid| Some(process(pid, 1, 1))).len(),
        1
    );
    assert!(
        matching_targets(&inventory, &selector, None, |pid| Some(ProcessIdentity {
            executable_path: Some("/other/player".into()),
            ..process(pid, 1, 1)
        }))
        .is_empty()
    );
    let mut unrelated = inventory[0].clone();
    unrelated.executable = Some("other".into());
    assert!(
        matching_targets(&[unrelated], &selector, None, |pid| Some(process(
            pid, 1, 1
        )))
        .is_empty()
    );
}

#[test]
fn legacy_pid_pin_rejects_exit_reuse_and_unpinned_adoption() {
    let selector = ApplicationSelector::ProcessId {
        process_id: 100,
        include_descendants: true,
    };
    let inventory = [target("a", 100, "app"), target("b", 101, "app")];
    let pin = process(100, 10, 1);
    let lookup = |pid| Some(process(pid, 10, if pid == 101 { 100 } else { 1 }));
    assert_eq!(
        matching_targets(&inventory, &selector, Some(&pin), lookup).len(),
        2
    );
    assert!(matching_targets(&inventory, &selector, None, lookup).is_empty());
    assert!(matching_targets(&inventory, &selector, Some(&pin), |_| None).is_empty());
    assert!(
        matching_targets(&inventory, &selector, Some(&pin), |pid| Some(process(
            pid, 99, 100
        )))
        .is_empty()
    );
    let reads = std::cell::Cell::new(0);
    assert!(
        matching_targets(&inventory[..1], &selector, Some(&pin), |pid| {
            reads.set(reads.get() + 1);
            Some(process(pid, if reads.get() == 1 { 10 } else { 99 }, 1))
        })
        .is_empty()
    );
}

#[test]
fn absent_new_removed_and_recreated_streams_drive_updates() {
    let mut entry = watch();
    assert!(entry.changed_targets(&[], |_| None).is_none());
    let active = [target("first", 100, "org.test.player")];
    assert_eq!(
        serials(&entry.changed_targets(&active, |_| None).unwrap()),
        ["first"]
    );
    entry.serials = vec!["first".into()];
    assert!(entry.changed_targets(&active, |_| None).is_none());
    assert!(entry.changed_targets(&[], |_| None).unwrap().is_empty());
    let replacement = [target("replacement", 200, "org.test.player")];
    assert_eq!(
        serials(&entry.changed_targets(&replacement, |_| None).unwrap()),
        ["replacement"]
    );
    entry.healthy.store(false, Ordering::Release);
    assert!(entry.changed_targets(&active, |_| None).is_some());
}

#[test]
fn absent_stable_app_prepares_silence_without_native_stream_or_server() {
    let (mut captures, watch) =
        CaptureSet::open_inventory(&[("app".into(), request(&selector()))], 5, &[]).unwrap();
    assert_eq!(captures.generation, 5);
    assert_eq!(watch.entries.len(), 1);
    let mut samples = [1.0; 8];
    let outcome = captures.read("app", &mut samples);
    assert_eq!(outcome.frames, 0);
    assert_eq!(outcome.activity, InputActivity::Unknown);
    assert_eq!(samples.map(f32::to_bits), [0; 8]);
}

#[test]
fn stopped_or_empty_watch_never_inspects_registry() {
    let stopped = AtomicBool::new(true);
    let watcher = CaptureWatch {
        generation: 1,
        entries: vec![watch()],
    };
    assert!(watcher.recover(&stopped).unwrap().is_none());
    let watcher = CaptureWatch {
        generation: 1,
        entries: Vec::new(),
    };
    assert!(watcher.recover(&AtomicBool::new(false)).unwrap().is_none());
}

#[test]
fn restored_pid_requires_reselection_and_never_resolves_or_recaptures() {
    let selector = ApplicationSelector::ProcessId {
        process_id: std::process::id(),
        include_descendants: true,
    };
    let mut request = request(&selector);
    request.requires_reselection = true;
    let (mut capture, watch) = CaptureSet::open(&[("restored".into(), request)], 10).unwrap();
    assert!(matches!(
        capture.state(),
        tonenari_engine_service::session::ApplicationRuntimeState::Waiting
    ));
    assert!(watch.recover(&AtomicBool::new(false)).unwrap().is_none());
    assert!(
        watch.entries[0]
            .changed_targets(&[target("a", std::process::id(), "old")], |_| panic!(
                "must not read PID"
            ))
            .is_none()
    );
    let mut samples = [1.0; 4];
    assert_eq!(capture.read("restored", &mut samples).frames, 0);
    assert_eq!(samples.map(f32::to_bits), [0; 4]);
}

pub(crate) struct FixtureProcess(std::process::Child);
impl FixtureProcess {
    pub(crate) fn start(path: &std::path::Path) -> Self {
        Self(
            std::process::Command::new(path)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::inherit())
                .spawn()
                .expect("owned application fixture"),
        )
    }
}
impl Drop for FixtureProcess {
    fn drop(&mut self) {
        if let Ok(pid) = i32::try_from(self.0.id()) {
            // SAFETY: child is still owned/unreaped, so its PID cannot be reused.
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }
        }
        for _ in 0..50 {
            if self.0.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn recover_until(captures: &mut CaptureSet, watch: &mut CaptureWatch, present: bool) {
    let started = std::time::Instant::now();
    loop {
        if let Some((mut next, next_watch)) = watch.recover(&AtomicBool::new(false)).unwrap() {
            captures.replace(&mut next);
            *watch = next_watch;
        }
        let mut samples = [0.0; 256];
        let outcome = captures.read("fixture", &mut samples);
        let frames = outcome.frames;
        let peak = samples.iter().copied().map(f32::abs).fold(0.0, f32::max);
        let empty = captures.nodes["fixture"].taps.is_empty();
        if (present && frames > 0 && peak > 0.05)
            || (!present && empty && frames == 0 && peak == 0.0)
        {
            assert_eq!(
                outcome.activity,
                if present {
                    InputActivity::Active
                } else {
                    InputActivity::Unknown
                }
            );
            println!(
                "present={present}; native_frames={frames}; peak={peak}; taps={}",
                captures.nodes["fixture"].taps.len()
            );
            return;
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "native application recovery timed out: present={present}, frames={frames}, peak={peak}, empty={empty}, formats={:?}, ready={:?}",
            captures.nodes["fixture"]
                .taps
                .iter()
                .map(NativeStream::negotiated_format)
                .collect::<Vec<_>>(),
            captures.nodes["fixture"]
                .taps
                .iter()
                .map(NativeStream::connected)
                .collect::<Vec<_>>()
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[test]
#[ignore = "requires opt-in owned non-autoconnecting PipeWire application fixture"]
fn native_application_stream_relaunch_recovers_pcm_without_graph_reapply() {
    let path = std::path::PathBuf::from(
        std::env::var("TONENARI_LAB_APPLICATION_FIXTURE").expect("explicit fixture executable"),
    );
    assert!(path.is_absolute() && path.is_file());
    let selector = ApplicationSelector::BundleId {
        bundle_id: "org.tonenari.fixture.application".into(),
        include_descendants: false,
    };
    let (mut captures, mut watch) =
        CaptureSet::open(&[("fixture".into(), request(&selector))], 9).unwrap();
    assert!(
        captures.nodes["fixture"].taps.is_empty(),
        "fixture must not already be running"
    );
    let first = FixtureProcess::start(&path);
    let first_pid = first.0.id();
    recover_until(&mut captures, &mut watch, true);
    let apps = tonenari_audio_io::linux::applications().unwrap();
    assert_eq!(
        apps.iter()
            .find(|app| app.process_id == first_pid)
            .unwrap()
            .selector,
        selector
    );
    drop(first);
    recover_until(&mut captures, &mut watch, false);
    let second = FixtureProcess::start(&path);
    assert_ne!(first_pid, second.0.id());
    recover_until(&mut captures, &mut watch, true);
    let apps = tonenari_audio_io::linux::applications().unwrap();
    assert_eq!(
        apps.iter()
            .find(|app| app.process_id == second.0.id())
            .unwrap()
            .selector,
        selector
    );
    assert_eq!(captures.generation, 9);
    assert_eq!(captures.nodes["fixture"].taps.len(), 1);
    drop(second);
    recover_until(&mut captures, &mut watch, false);
}

#[test]
#[ignore = "requires isolated PipeWire graph and owned rate-selectable application fixture"]
fn native_pipewire_rate_conversion_and_observed_clock() {
    use super::super::native::{NativeCycle, NativeDriver};
    use std::sync::atomic::AtomicU64;
    unsafe extern "C" fn observe(data: *mut std::ffi::c_void, cycle: *const NativeCycle) {
        // SAFETY: test pins this atomic until the owning driver drains on Drop.
        let (value, cycle) = unsafe { (&*data.cast::<AtomicU64>(), &*cycle) };
        value.store(
            u64::from(cycle.clock_id) << 32 | u64::from(cycle.rate),
            Ordering::Release,
        );
    }
    let clock = Box::new(AtomicU64::new(0));
    // SAFETY: pinned atomic outlives driver and callback only accesses it atomically.
    let driver = unsafe {
        NativeDriver::open(
            observe,
            std::ptr::from_ref(clock.as_ref()).cast_mut().cast(),
        )
    }
    .unwrap();
    let path = std::path::PathBuf::from(std::env::var("TONENARI_LAB_APPLICATION_FIXTURE").unwrap());
    let source = FixtureProcess::start(&path);
    let selector = ApplicationSelector::BundleId {
        bundle_id: "org.tonenari.fixture.application".into(),
        include_descendants: false,
    };
    let (mut captures, mut watch) =
        CaptureSet::open(&[("fixture".into(), request(&selector))], 1).unwrap();
    recover_until(&mut captures, &mut watch, true);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut samples = Vec::with_capacity(53_000);
    while samples.len() < 52_800 {
        let mut block = [0.0; 256];
        let frames = captures.read("fixture", &mut block).frames as usize;
        samples.extend(block[..frames * 2].chunks_exact(2).map(|frame| frame[0]));
        assert!(std::time::Instant::now() < deadline);
        if frames == 0 {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    // Exclude startup converter history; count a full second of valid PCM.
    let samples = &samples[4800..52_800];
    let crossings = samples
        .windows(2)
        .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
        .count();
    let frequency = f64::from(u32::try_from(crossings).unwrap()) * 48_000.0
        / f64::from(u32::try_from(samples.len()).unwrap());
    assert!(
        (frequency - 1000.0).abs() <= 3.0,
        "converted tone frequency={frequency}"
    );
    let observed = clock.load(Ordering::Acquire);
    let rate = u32::try_from(observed & u64::from(u32::MAX)).unwrap();
    let id = u32::try_from(observed >> 32).unwrap();
    let expected: u32 = std::env::var("TONENARI_LAB_GRAPH_RATE")
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(rate, expected);
    assert!(
        captures.shares_clock(id, rate),
        "stream and filter must observe the same actual driver"
    );
    println!(
        "graph_rate={rate}; source_rate={}; stream_pcm_rate=48000; measured_tone_hz={frequency}; samples={}; clock_id={id}; shared_driver=true",
        std::env::var("TONENARI_LAB_SOURCE_RATE").unwrap(),
        samples.len()
    );
    drop(captures);
    drop(source);
    drop(driver);
}
