//! Stable executable intent is supervised off the audio thread. Legacy PID
//! intent is pinned to its original kernel process object and never retargeted.
use super::{
    CaptureWorkerConfig, MAX_ENDPOINT_CHANNELS, ProcessLoopbackCapture, run_process_capture,
};
use mixrelay_audio_graph::{ApplicationCaptureRequest, ApplicationSelector, SourcePlaybackPolicy};
use mixrelay_audio_io::process::TransferOutcome;
use mixrelay_audio_io::{
    session::SessionIssue,
    windows::process_identity::{ProcessIdentity, ResolutionError, resolve_executable},
};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        Arc, Weak,
        atomic::{AtomicU32, Ordering},
    },
    thread,
    time::Duration,
};

pub(super) const ACTIVE: u32 = 0;
pub(super) const UNAVAILABLE: u32 = 1;
const AMBIGUOUS: u32 = 2;
pub(super) const fn unknown_transfer() -> TransferOutcome {
    TransferOutcome {
        activity: mixrelay_audio_io::process::InputActivity::Unknown,
        ..TransferOutcome::complete(0)
    }
}
pub(super) type CaptureCache =
    BTreeMap<String, (ApplicationCaptureRequest, Weak<ProcessLoopbackCapture>)>;

#[cfg(test)]
pub(super) fn prepare(
    requests: &[(String, ApplicationCaptureRequest)],
    cache: &mut CaptureCache,
) -> Result<BTreeMap<String, Arc<ProcessLoopbackCapture>>, String> {
    prepare_at_rate(requests, cache, super::ENGINE_SAMPLE_RATE)
}

pub(super) fn prepare_at_rate(
    requests: &[(String, ApplicationCaptureRequest)],
    cache: &mut CaptureCache,
    graph_rate: u32,
) -> Result<BTreeMap<String, Arc<ProcessLoopbackCapture>>, String> {
    let mut result = BTreeMap::new();
    for (id, request) in requests {
        request
            .validate_contract(MAX_ENDPOINT_CHANNELS)
            .map_err(|error| error.to_string())?;
        if request.source_playback != SourcePlaybackPolicy::Preserve {
            return Err("Windows process loopback cannot mute a captured application".into());
        }
        let existing = cache
            .get(id)
            .filter(|(old, _)| old == request)
            .and_then(|(_, owner)| owner.upgrade())
            .filter(|owner| owner.graph_rate == graph_rate);
        result.insert(
            id.clone(),
            match existing {
                Some(owner) => owner,
                None => Arc::new(ProcessLoopbackCapture::open_target_at_rate(
                    if request.requires_reselection {
                        Target::Reselection
                    } else {
                        Target::new(&request.selector)?
                    },
                    request.stream.channels,
                    graph_rate,
                )?),
            },
        );
    }
    cache.clear();
    for (id, request) in requests {
        cache.insert(id.clone(), (request.clone(), Arc::downgrade(&result[id])));
    }
    Ok(result)
}

pub(super) fn issue(cache: &CaptureCache) -> Option<SessionIssue> {
    let states = cache
        .values()
        .filter_map(|(_, owner)| owner.upgrade())
        .map(|owner| owner.state.load(Ordering::Acquire));
    match states.max().unwrap_or(ACTIVE) {
        AMBIGUOUS => Some(SessionIssue::ApplicationAmbiguous),
        UNAVAILABLE => Some(SessionIssue::ApplicationUnavailable),
        _ => None,
    }
}

pub(super) enum Target {
    Executable(String),
    Pinned(Arc<ProcessIdentity>),
    Reselection,
}
impl Target {
    pub(super) fn new(selector: &ApplicationSelector) -> Result<Self, String> {
        match selector {
            ApplicationSelector::ExecutablePath { path } if Path::new(path).is_absolute() => {
                Ok(Self::Executable(path.clone()))
            }
            ApplicationSelector::ProcessId {
                process_id,
                include_descendants: true,
            } => Ok(Self::Pinned(Arc::new(ProcessIdentity::open(*process_id)?))),
            ApplicationSelector::ProcessId { .. } => {
                Err("exact-PID capture without descendants is unsupported".into())
            }
            _ => Err(
                "Windows application capture requires an absolute executable path or PID selector"
                    .into(),
            ),
        }
    }
    fn resolve(&self) -> Result<Arc<ProcessIdentity>, u32> {
        match self {
            Self::Executable(path) => {
                resolve_executable(path)
                    .map(Arc::new)
                    .map_err(|error| match error {
                        ResolutionError::Unavailable => UNAVAILABLE,
                        ResolutionError::Ambiguous => AMBIGUOUS,
                    })
            }
            Self::Pinned(identity) if identity.alive() => Ok(identity.clone()),
            Self::Pinned(_) | Self::Reselection => Err(UNAVAILABLE),
        }
    }
    pub(super) fn verify(&self, active: &ProcessIdentity) -> Result<(), u32> {
        let resolved = self.resolve()?;
        if resolved.pid() == active.pid()
            && resolved.creation_time() == active.creation_time()
            && active.alive()
        {
            Ok(())
        } else {
            Err(UNAVAILABLE)
        }
    }
}

pub(super) fn supervise(
    config: &CaptureWorkerConfig,
    ready: &std::sync::mpsc::SyncSender<Result<(), String>>,
) {
    let _state_lease = CaptureStateLease(&config.state);
    let recoverable_start = matches!(config.target, Target::Executable(_) | Target::Reselection);
    let mut announced = false;
    while !config.stop.load(Ordering::Acquire) {
        let result = match config.target.resolve() {
            Ok(identity) => {
                config.state.store(UNAVAILABLE, Ordering::Release);
                run_process_capture(config, &identity, &mut || {
                    config.epoch.fetch_add(1, Ordering::AcqRel);
                    config.state.store(ACTIVE, Ordering::Release);
                    if !announced {
                        let _ = ready.try_send(Ok(()));
                        announced = true;
                    }
                })
            }
            Err(state) => {
                config.state.store(state, Ordering::Release);
                Err("application unavailable or ambiguous".into())
            }
        };
        let _ =
            config
                .state
                .compare_exchange(ACTIVE, UNAVAILABLE, Ordering::AcqRel, Ordering::Acquire);
        if !announced {
            if recoverable_start {
                let _ = ready.try_send(Ok(()));
                announced = true;
            } else {
                let _ = ready.try_send(Err(result
                    .err()
                    .unwrap_or_else(|| "application capture cancelled".into())));
                return;
            }
        }
        if !config.stop.load(Ordering::Acquire) {
            thread::park_timeout(Duration::from_millis(500));
        }
    }
}

struct CaptureStateLease<'a>(&'a AtomicU32);
impl Drop for CaptureStateLease<'_> {
    fn drop(&mut self) {
        // A native worker's unexpected unwind/return must not leave stale Ready.
        self.0.store(UNAVAILABLE, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::super::SpscSampleRing;
    use super::*;
    use std::{
        os::windows::process::CommandExt,
        process::{Child, Command, Stdio},
        sync::atomic::{AtomicBool, AtomicU64},
        time::Instant,
    };

    #[test]
    fn unexpected_capture_owner_exit_never_retains_ready_state() {
        let state = AtomicU32::new(ACTIVE);
        let panic = std::panic::catch_unwind(|| {
            let _lease = CaptureStateLease(&state);
            panic!("fixture native worker unwind");
        });
        assert!(panic.is_err());
        assert_eq!(state.load(Ordering::Acquire), UNAVAILABLE);
    }

    struct Fixture {
        path: std::path::PathBuf,
        _directory: mixrelay_test_support::FixtureDir,
        children: Vec<Child>,
    }
    impl Fixture {
        fn new() -> Self {
            let directory = mixrelay_test_support::FixtureDir::new("app-identity").unwrap();
            let path = directory.join("MixRelayProcessFixture.exe");
            std::fs::copy(
                Path::new(&std::env::var("SystemRoot").unwrap()).join("System32\\cmd.exe"),
                &path,
            )
            .unwrap();
            Self {
                path,
                _directory: directory,
                children: vec![],
            }
        }
        fn selector(&self) -> ApplicationSelector {
            ApplicationSelector::ExecutablePath {
                path: self.path.to_str().unwrap().into(),
            }
        }
        fn spawn(&mut self) -> u32 {
            // Owned silent process, waiting on our piped stdin. No render/default
            // device is opened, and no unrelated application is captured or stopped.
            let child = Command::new(&self.path)
                .args(["/d", "/q", "/k"])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(0x0800_0000)
                .spawn()
                .unwrap();
            let pid = child.id();
            self.children.push(child);
            pid
        }
        fn stop(&mut self, index: usize) {
            self.children[index].kill().unwrap();
            self.children[index].wait().unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            for child in &mut self.children {
                let _ = child.kill();
                let _ = child.wait();
            }
            // The directory owner drops after these copied-executable children.
        }
    }

    fn wait_for(mut condition: impl FnMut() -> bool) {
        let start = Instant::now();
        while !condition() {
            assert!(
                start.elapsed() < Duration::from_secs(15),
                "application transition timed out"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn real_process_lifetime_unavailable_unique_ambiguous_relaunch_and_pid_pin() {
        let mut fixture = Fixture::new();
        let target = Target::new(&fixture.selector()).unwrap();
        assert!(matches!(target.resolve(), Err(UNAVAILABLE)));
        let first_pid = fixture.spawn();
        wait_for(|| target.resolve().is_ok());
        let first = target.resolve().unwrap();
        assert_eq!(first.pid(), first_pid);
        let pinned = Target::new(&ApplicationSelector::ProcessId {
            process_id: first_pid,
            include_descendants: true,
        })
        .unwrap();
        let second_pid = fixture.spawn();
        wait_for(|| matches!(target.resolve(), Err(AMBIGUOUS)));
        assert_eq!(pinned.resolve().unwrap().pid(), first_pid);
        fixture.stop(0);
        wait_for(|| {
            target
                .resolve()
                .is_ok_and(|identity| identity.pid() == second_pid)
        });
        assert!(!first.alive());
        assert!(matches!(pinned.resolve(), Err(UNAVAILABLE)));
        fixture.stop(1);
        assert!(matches!(target.resolve(), Err(UNAVAILABLE)));
        let third_pid = fixture.spawn();
        wait_for(|| {
            target
                .resolve()
                .is_ok_and(|identity| identity.pid() == third_pid)
        });
        assert!(matches!(pinned.resolve(), Err(UNAVAILABLE)));
    }

    #[test]
    fn missing_app_is_recoverable_and_removal_joins_owner_without_stale_pcm() {
        let fixture = Fixture::new();
        let capture = ProcessLoopbackCapture::open_selector(&fixture.selector(), 2).unwrap();
        assert_eq!(capture.state.load(Ordering::Acquire), UNAVAILABLE);
        capture.ring.push(&[0.5; 8], 2);
        let mut pcm = [f32::NAN; 8];
        assert_eq!(capture.read(&mut pcm), unknown_transfer());
        assert!(pcm.iter().all(|value| *value == 0.0));
        let start = Instant::now();
        drop(capture);
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn restored_pid_requires_reselection_without_opening_or_retrying_that_pid() {
        let request: ApplicationCaptureRequest = serde_json::from_value(serde_json::json!({
            "selector": {"match":"process_id", "process_id": u32::MAX, "include_descendants":true},
            "stream": {"channels":2,"channelLayout":"stereo"},
            "sourcePlayback":"preserve", "requiresReselection":true
        }))
        .unwrap();
        let mut cache = CaptureCache::new();
        let requests = [("restored".into(), request)];
        let first = prepare(&requests, &mut cache).unwrap();
        assert!(first["restored"].worker.is_none());
        assert_eq!(issue(&cache), Some(SessionIssue::ApplicationUnavailable));
        let next = prepare(&requests, &mut cache).unwrap();
        assert!(Arc::ptr_eq(&first["restored"], &next["restored"]));
        assert!(prepare(&[], &mut cache).unwrap().is_empty());
        assert_eq!(issue(&cache), None);
        drop(first);
        drop(next);
    }

    #[test]
    fn recovered_capture_never_replays_previous_epoch_pcm() {
        let capture = ProcessLoopbackCapture {
            ring: Arc::new(SpscSampleRing::new(16)),
            stop: Arc::new(AtomicBool::new(false)),
            state: Arc::new(AtomicU32::new(ACTIVE)),
            epoch: Arc::new(AtomicU64::new(1)),
            consumed_epoch: AtomicU64::new(0),
            primed: AtomicBool::new(true),
            channels: 2,
            graph_rate: super::super::ENGINE_SAMPLE_RATE,
            worker: None,
        };
        let mut pcm = [f32::NAN; 4];
        capture.ring.push(&[0.5; 4], 2);
        assert_eq!(capture.read(&mut pcm), unknown_transfer());
        // This fixture isolates epoch retirement, not startup FIFO priming.
        capture.primed.store(true, Ordering::Relaxed);
        capture.ring.push(&[0.25; 4], 2);
        assert_eq!(capture.read(&mut pcm), TransferOutcome::complete(2));
        assert!(
            pcm.iter()
                .all(|value| value.to_bits() == 0.25_f32.to_bits())
        );
        capture.ring.push(&[0.75; 4], 2);
        capture.epoch.store(2, Ordering::Release);
        assert_eq!(capture.read(&mut pcm), unknown_transfer());
        assert!(pcm.iter().all(|value| *value == 0.0));
    }

    #[test]
    #[ignore = "explicit Windows process-loopback acceptance; captures only silent test-owned cmd copies"]
    fn native_application_capture_relaunch_and_ambiguity_without_graph_reapply() {
        assert_eq!(
            std::env::var("MIXRELAY_LAB_APPLICATION_TEST").as_deref(),
            Ok("1")
        );
        let mut fixture = Fixture::new();
        let capture = ProcessLoopbackCapture::open_selector(&fixture.selector(), 2).unwrap();
        assert_eq!(capture.state.load(Ordering::Acquire), UNAVAILABLE);
        fixture.spawn();
        wait_for(|| capture.state.load(Ordering::Acquire) == ACTIVE);
        let first_epoch = capture.epoch.load(Ordering::Acquire);
        fixture.spawn();
        wait_for(|| capture.state.load(Ordering::Acquire) == AMBIGUOUS);
        fixture.stop(0);
        wait_for(|| {
            capture.state.load(Ordering::Acquire) == ACTIVE
                && capture.epoch.load(Ordering::Acquire) > first_epoch
        });
        let second_epoch = capture.epoch.load(Ordering::Acquire);
        fixture.stop(1);
        wait_for(|| capture.state.load(Ordering::Acquire) == UNAVAILABLE);
        fixture.spawn();
        wait_for(|| {
            capture.state.load(Ordering::Acquire) == ACTIVE
                && capture.epoch.load(Ordering::Acquire) > second_epoch
        });
        eprintln!(
            "native owned silent process: unavailable -> active epoch{first_epoch} -> ambiguous -> active epoch{second_epoch} -> unavailable -> active epoch{}",
            capture.epoch.load(Ordering::Acquire)
        );
        drop(capture);
    }
}
