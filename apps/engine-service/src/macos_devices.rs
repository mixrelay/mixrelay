//! `CoreAudio` resource coordination. Graph publication, recovery and format
//! transactions have separate activation contracts; no native work runs on DSP.
mod admission;
mod aggregate;
mod application;
mod bridge;
mod buffer_control;
mod buffer_transaction;
mod device;
pub(super) mod direct;
mod ffi;
mod format;
mod group_plan;
mod handoff;
mod native;
mod retirement;
mod spec;
mod worker;
use aggregate::Aggregate;
#[cfg(test)]
use bridge::DeviceBridge;
use device::Device;
use ffi::tn_device_io_native_frames;
#[cfg(test)]
use ffi::*;
use format::{PreparedDeviceFormat, PublishedFormat};
use handoff::{Publication, ResourceHandoff};
use spec::{Spec, aggregate_members, specs};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tonenari_audio_graph::{DeviceSelector, GraphDocument, RenderFormat};
use tonenari_audio_io::{HostAudioBackendError, process::TransferOutcome};
use tonenari_engine_service::{
    GraphResourceBackend, PreparedGraphFormat, RuntimeStatusBackend, session::AdapterRuntimeStatus,
};
#[cfg(test)]
use {
    bridge::NEXT_DEVICE_EPOCH,
    native::Native,
    std::{
        sync::atomic::{AtomicU32, AtomicU64},
        thread,
    },
    tonenari_audio_graph::NodeConfig,
    worker::WorkerContext,
};
#[derive(Default)]
pub(super) struct DeviceSet(BTreeMap<String, Arc<Device>>);
pub(super) struct HardwareClockProgress {
    pub(super) owner_id: u64,
    pub(super) epoch: u64,
    pub(super) native_frames: u64,
    pub(super) native_rate: u32,
}
impl DeviceSet {
    /// Explicit graph pacing identity, never inferred from nominal sample rate.
    pub(super) fn set_clock_master(&self, owner_id: Option<u64>) {
        let master = self.0.values().find(|device| {
            owner_id == Some(device.identity) && !device.failed.load(Ordering::Acquire)
        });
        let master_clock = master.map_or(0, |device| device.clock_identity.load(Ordering::Acquire));
        let master_rate = master.map_or(0, |device| device.native_rate.load(Ordering::Acquire));
        for device in self.0.values() {
            // Two IOProcs of the same non-aggregate device share its timeline.
            // A shared nominal rate or unknown clock domain is never proof.
            // User aggregates stay independent until their subdevice policy is
            // verified; merely sharing an Aggregate object ID is insufficient.
            let aligned = master.is_some()
                && !device.failed.load(Ordering::Acquire)
                && (owner_id == Some(device.identity)
                    || (master_clock != 0
                        && device.clock_identity.load(Ordering::Acquire) == master_clock
                        && master_rate != 0
                        && device.native_rate.load(Ordering::Acquire) == master_rate));
            device.clock_aligned.store(aligned, Ordering::Release);
        }
    }
    pub(super) fn clock_synchronized(&self, id: &str) -> Option<bool> {
        self.0
            .get(id)
            .map(|device| device.ready() && device.bridge_ready.load(Ordering::Acquire))
    }
    /// A pacing candidate is not proof that unrelated clocks are synchronized.
    /// Prefer output demand; capture-only graphs can use a healthy input clock.
    pub(super) fn hardware_clock_progress(&self) -> Option<HardwareClockProgress> {
        self.0
            .values()
            .filter(|device| {
                !device.failed.load(Ordering::Acquire)
                    && device.native_rate.load(Ordering::Acquire) != 0
            })
            .min_by_key(|device| device.input)
            .map(|device| HardwareClockProgress {
                owner_id: device.identity,
                epoch: device.epoch.load(Ordering::Acquire),
                native_frames: unsafe {
                    tn_device_io_native_frames(device.native.pointer.as_ptr())
                },
                native_rate: device.native_rate.load(Ordering::Acquire),
            })
    }
    pub(super) fn read(&self, id: &str, samples: &mut [f32]) -> Option<TransferOutcome> {
        self.0.get(id).map(|device| device.read(samples))
    }
    pub(super) fn write(&self, id: &str, samples: &[f32]) -> Option<TransferOutcome> {
        self.0.get(id).map(|device| device.write(samples))
    }
}
type Cache = BTreeMap<String, (Spec, Weak<Device>)>;
pub(super) struct MacDevices {
    cache: Mutex<Cache>,
    handoff: ResourceHandoff<DeviceSet>,
    status: Arc<AdapterRuntimeStatus>,
    format: Arc<PublishedFormat>,
    next_group_check: Mutex<Instant>,
    prepared_hardware_clock: AtomicBool,
    hardware_clock: AtomicBool,
}
impl MacDevices {
    pub(super) fn has_pending(&self) -> bool {
        self.handoff.has_pending()
    }
    pub(super) fn new(status: Arc<AdapterRuntimeStatus>, rate: u32, buffer_frames: u32) -> Self {
        Self {
            cache: Mutex::default(),
            handoff: ResourceHandoff::default(),
            status,
            format: Arc::new(PublishedFormat::new(rate, buffer_frames)),
            next_group_check: Mutex::new(Instant::now()),
            prepared_hardware_clock: AtomicBool::new(false),
            hardware_clock: AtomicBool::new(false),
        }
    }
    pub(super) fn reclaim(&self) {
        let Ok(mut cache) = self.cache.try_lock() else {
            return;
        };
        self.handoff.reclaim();
        // Membership, rate changes and hotplug are reconciled off the DSP
        // thread. Preparation is transactional: old owners remain published
        // until an entire replacement set is ready, and invalid callbacks mute.
        let Ok(mut next) = self.next_group_check.try_lock() else {
            return;
        };
        if Instant::now() < *next {
            return;
        }
        *next = Instant::now() + Duration::from_secs(2);
        Aggregate::reclaim_retired();
        if !self.handoff.idle() {
            return;
        }
        let desired = cache
            .iter()
            .map(|(id, (spec, _))| (id.clone(), spec.clone()))
            .collect();
        if let Err(error) = self.prepare_devices(&mut cache, desired, Publication::Recovery) {
            eprintln!("CoreAudio private-group recovery deferred: {error}");
        }
    }
    /// Only the serialized graph activation callback may swap these owners.
    pub(super) unsafe fn activate(&self, active: &mut DeviceSet) {
        unsafe {
            self.handoff.activate(Publication::Graph, active);
        }
        self.hardware_clock.store(
            self.prepared_hardware_clock.load(Ordering::Acquire),
            Ordering::Release,
        );
    }

    pub(super) fn follows_hardware(&self) -> bool {
        self.hardware_clock.load(Ordering::Acquire)
    }

    pub(super) fn routing_rate(&self) -> Option<u32> {
        if !self.follows_hardware() {
            return None;
        }
        self.cache
            .lock()
            .ok()?
            .values()
            .filter_map(|(_, owner)| owner.upgrade())
            .filter(|device| !device.failed.load(Ordering::Acquire))
            .min_by_key(|device| device.input)
            .map(|device| device.native_rate.load(Ordering::Acquire))
            .filter(|rate| *rate != 0)
    }
    /// Same-spec recovery is independent of graph revisions, at a block boundary.
    pub(super) unsafe fn activate_recovered(&self, active: &mut DeviceSet) {
        unsafe {
            self.handoff.activate(Publication::Recovery, active);
        }
    }
    fn prepare_devices(
        &self,
        cache: &mut Cache,
        desired: BTreeMap<String, Spec>,
        publication: Publication,
    ) -> Result<(), HostAudioBackendError> {
        let members = aggregate_members(&desired);
        let existing_group = cache
            .values()
            .filter_map(|(_, weak)| weak.upgrade())
            .find_map(|device| {
                device
                    .native
                    .group
                    .clone()
                    .filter(|group| group.members == members && group.ready())
            });
        let group = match existing_group {
            Some(group) => Some(group),
            None => Aggregate::create(members).map_err(backend_error)?,
        };
        let mut devices = BTreeMap::new();
        let mut changed = desired.len() != cache.len();
        for (id, spec) in &desired {
            let selected_group = if matches!(spec.selector, DeviceSelector::PersistentUid { .. }) {
                group.clone()
            } else {
                None
            };
            let existing = cache
                .get(id)
                .filter(|(old, _)| old == spec)
                .and_then(|(_, owner)| owner.upgrade())
                .filter(|device| match (&device.native.group, &selected_group) {
                    (Some(old), Some(new)) => Arc::ptr_eq(old, new),
                    (None, None) => true,
                    _ => false,
                });
            let device = if let Some(device) = existing {
                device
            } else {
                changed = true;
                Arc::new(
                    Device::open_with_group(
                        spec,
                        self.format.current().sample_rate,
                        selected_group,
                        self.format.current().block_frames,
                    )
                    .map_err(backend_error)?,
                )
            };
            devices.insert(id.clone(), device);
        }
        if !changed {
            return Ok(());
        }
        let next_cache = desired
            .into_iter()
            .map(|(id, spec)| {
                let weak = Arc::downgrade(&devices[&id]);
                (id, (spec, weak))
            })
            .collect();
        self.handoff
            .publish(publication, DeviceSet(devices))
            .map_err(|_| backend_error("CoreAudio resource publication failed"))?;
        *cache = next_cache;
        Ok(())
    }
}
fn backend_error(message: impl Into<String>) -> HostAudioBackendError {
    HostAudioBackendError::BackendFailure {
        message: message.into(),
    }
}
impl GraphResourceBackend for MacDevices {
    fn device_inventory(
        &self,
    ) -> Result<Vec<tonenari_audio_io::AudioDeviceDescriptor>, HostAudioBackendError> {
        tonenari_audio_io::macos::devices()
    }

    fn resolve_application_capture(
        &self,
        request: &tonenari_audio_io::ApplicationCaptureRequest,
    ) -> Result<tonenari_engine_service::graph_composition::SharedSource, HostAudioBackendError>
    {
        application::resolve(request)
    }

    fn prepare_format_change(
        &self,
        _document: &GraphDocument,
        current: RenderFormat,
        requested: RenderFormat,
    ) -> Result<Box<dyn PreparedGraphFormat>, HostAudioBackendError> {
        let cache = self
            .cache
            .lock()
            .map_err(|_| backend_error("CoreAudio device cache poisoned"))?;
        self.handoff.reclaim();
        let reservation = self.handoff.reserve_format().ok_or_else(|| {
            backend_error("CoreAudio resource or format update is already pending")
        })?;
        PreparedDeviceFormat::prepare(
            cache.values().filter_map(|(_, owner)| owner.upgrade()),
            self.format.clone(),
            current,
            requested,
            reservation,
        )
        .map(|prepared| Box::new(prepared) as Box<dyn PreparedGraphFormat>)
    }
    fn reconcile(&self, document: &GraphDocument) -> Result<(), HostAudioBackendError> {
        let desired = specs(document);
        // Adding a virtual endpoint does not transfer hardware's clock authority
        // to that endpoint. Cross-clock transport is an explicit bridge boundary.
        let follow_hardware = !desired.is_empty();
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| backend_error("CoreAudio device cache poisoned"))?;
        if desired.len() == cache.len()
            && desired.iter().all(|(id, spec)| {
                cache
                    .get(id)
                    .is_some_and(|(old, device)| old == spec && device.strong_count() != 0)
            })
        {
            self.prepared_hardware_clock
                .store(follow_hardware, Ordering::Release);
            return Ok(());
        }
        self.handoff.reclaim();
        if !self.handoff.idle() {
            return Err(backend_error("CoreAudio device update is already pending"));
        }
        self.prepare_devices(&mut cache, desired, Publication::Graph)?;
        self.prepared_hardware_clock
            .store(follow_hardware, Ordering::Release);
        Ok(())
    }
}
impl RuntimeStatusBackend for MacDevices {
    fn fail_reconfiguration(&self) {
        self.status.fail_reconfiguration();
    }
    fn suspend(&self) -> Result<(), HostAudioBackendError> {
        self.status.pause_at_boundary()
    }
    fn capabilities_for_format(
        &self,
        format: RenderFormat,
    ) -> Option<tonenari_audio_io::session::AudioSessionCapabilities> {
        self.status
            .observe_routing_clock(self.follows_hardware(), self.routing_rate());
        self.status.capabilities_for_format(format)
    }
    fn external_sample_rate(&self) -> Option<u32> {
        self.status.external_sample_rate()
    }
    fn external_format(&self, current: RenderFormat) -> Option<RenderFormat> {
        if self.follows_hardware() {
            return self.routing_rate().map(|sample_rate| RenderFormat {
                sample_rate,
                ..current
            });
        }
        self.status.external_format(current)
    }
    fn adopt_external_format(&self, requested: RenderFormat) -> Result<(), HostAudioBackendError> {
        if self.follows_hardware() {
            if self.routing_rate() != Some(requested.sample_rate) {
                return Err(backend_error("routing clock changed during preparation"));
            }
            self.status.adopt_routing_clock(requested)?;
            if self.routing_rate() != Some(requested.sample_rate) {
                return Err(backend_error("routing clock changed during suspension"));
            }
            return Ok(());
        }
        self.status.adopt_external_format(requested)
    }
    fn status(&self, format: RenderFormat) -> tonenari_control_protocol::AppStatus {
        self.status
            .observe_routing_clock(self.follows_hardware(), self.routing_rate());
        let mut status = self.status.status(format);
        let failed = self.cache.lock().map_or(true, |cache| {
            cache.values().any(|(_, weak)| {
                weak.upgrade()
                    .is_some_and(|device| device.failed.load(Ordering::Acquire))
            })
        });
        if failed
            && let Some(session) = &mut status.audio_session
            && !matches!(
                session.issue,
                Some(
                    tonenari_audio_io::session::SessionIssue::ProcessingFailed
                        | tonenari_audio_io::session::SessionIssue::ReconfigurationFailed
                )
            )
        {
            session.issue = Some(tonenari_audio_io::session::SessionIssue::DeviceUnavailable);
        }
        status
    }
    fn reconfigure(
        &self,
        current: RenderFormat,
        requested: RenderFormat,
    ) -> Result<(), HostAudioBackendError> {
        if self.follows_hardware() {
            if self.routing_rate() != Some(requested.sample_rate) {
                return Err(backend_error(
                    "hardware routing follows its device sample rate; change the selected device format, not the unrelated virtual endpoint",
                ));
            }
            return self.status.reconfigure_routing_buffer(requested);
        }
        self.status.reconfigure(current, requested)
    }
    fn resume(&self) {
        self.status.resume();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_owner_allocation_retirement_stress() {
        if !tonenari_macos_test_support::memory_child(
            "macos_devices::tests::native_owner_allocation_retirement_stress",
        ) {
            return;
        }
        let baseline = Native::active_owners();
        for channels in [1, 2, 8, 64] {
            for _ in 0..128 {
                let (device, context) = synthetic_output(channels, 48_000, 48_000);
                assert_eq!(Native::active_owners(), baseline + 1);
                drop(device);
                assert_eq!(
                    Native::active_owners(),
                    baseline + 1,
                    "worker still owns native queues"
                );
                drop(context);
                assert_eq!(
                    Native::active_owners(),
                    baseline,
                    "last owner releases both queues and admission"
                );
            }
        }
        eprintln!(
            "512 native create/retire cycles across 1/2/8/64 channels; owner count returned to {baseline}"
        );
    }
    #[test]
    fn aggregate_members_are_unique_fixed_uids_with_output_clock_first() {
        let mut desired = BTreeMap::new();
        for (name, uid, input) in [("a", "mic", true), ("b", "out", false), ("c", "out", true)] {
            desired.insert(
                name.to_owned(),
                Spec {
                    selector: DeviceSelector::PersistentUid {
                        uid: uid.to_owned(),
                    },
                    input,
                    channels: 2,
                },
            );
        }
        desired.insert(
            "default".into(),
            Spec {
                selector: DeviceSelector::DefaultOutput,
                input: false,
                channels: 2,
            },
        );
        assert_eq!(aggregate_members(&desired), ["out", "mic"]);
    }
    #[test]
    fn low_latency_queue_keeps_one_burst_for_shared_and_two_for_independent_clocks() {
        let (device, context) = synthetic_output(2, 48000, 48000);
        let pcm = [0.0; 64];
        unsafe {
            tn_device_io_graph_write(device.native.pointer.as_ptr(), pcm.as_ptr(), 32);
        }
        let mut bridge = DeviceBridge::new(&context, 48000).unwrap();
        bridge.clock_aligned = true;
        assert_eq!(bridge.queue_target(&context, false), 32);
        bridge.clock_aligned = false;
        assert_eq!(bridge.queue_target(&context, false), 64);
    }
    #[test]
    fn output_queue_latency_budget() {
        let (device, context) = synthetic_output(2, 48000, 48000);
        context.clock_aligned.store(true, Ordering::Release);
        let mut bridge = DeviceBridge::new(&context, 48000).unwrap();
        let source = vec![0.125_f32; 256 * 2];
        for _ in 0..16 {
            assert_eq!(
                unsafe {
                    tn_device_io_graph_write(device.native.pointer.as_ptr(), source.as_ptr(), 256)
                },
                256
            );
        }
        for _ in 0..32 {
            bridge.pump(&context).unwrap();
        }
        let queued = unsafe { tn_device_io_queued(device.native.pointer.as_ptr(), false) };
        eprintln!(
            "same-clock 48kHz stereo native output queued={queued} frames ({:.3} ms)",
            f64::from(queued) / 48.0
        );
        assert_eq!(queued, 256);
    }
    fn synthetic_output(
        channels: u16,
        graph_rate: u32,
        native_rate: u32,
    ) -> (Arc<Device>, WorkerContext) {
        let native = Arc::new(
            Native::create(
                &Spec {
                    selector: DeviceSelector::PersistentUid {
                        uid: "synthetic-no-device".into(),
                    },
                    input: false,
                    channels,
                },
                native_rate,
                None,
                0,
            )
            .unwrap(),
        );
        let context = WorkerContext {
            native: native.clone(),
            stop: Arc::new(AtomicBool::new(false)),
            failed: Arc::new(AtomicBool::new(false)),
            epoch: Arc::new(AtomicU64::new(1)),
            graph_rate: Arc::new(AtomicU32::new(graph_rate)),
            ready_rate: Arc::new(AtomicU32::new(graph_rate)),
            native_rate: Arc::new(AtomicU32::new(native_rate)),
            clock_identity: Arc::new(AtomicU32::new(0)),
            bridge_ready: Arc::new(AtomicBool::new(false)),
            pending_bridge: Arc::new(Mutex::new(None)),
            clock_aligned: Arc::new(AtomicBool::new(false)),
            channels: usize::from(channels),
            input: false,
        };
        let device = Arc::new(Device {
            identity: NEXT_DEVICE_EPOCH.fetch_add(1, Ordering::Relaxed),
            native,
            channels: context.channels,
            input: false,
            stop: context.stop.clone(),
            failed: context.failed.clone(),
            epoch: context.epoch.clone(),
            graph_rate: context.graph_rate.clone(),
            ready_rate: context.ready_rate.clone(),
            native_rate: context.native_rate.clone(),
            clock_identity: context.clock_identity.clone(),
            bridge_ready: context.bridge_ready.clone(),
            pending_bridge: context.pending_bridge.clone(),
            clock_aligned: context.clock_aligned.clone(),
            worker: None,
        });
        (device, context)
    }
    #[test]
    fn worker_bridge_has_no_priming_output_and_uses_actual_native_rate() {
        for channels in [1, 2, 16, 64] {
            let mut counts = Vec::new();
            for native_rate in [44_100, 96_000] {
                let (device, context) = synthetic_output(channels, 48_000, native_rate);
                let mut bridge = DeviceBridge::new(&context, native_rate).unwrap();
                bridge.pump(&context).unwrap();
                assert_eq!(
                    unsafe { tn_device_io_queued(device.native.pointer.as_ptr(), false) },
                    0
                );
                let source = vec![0.25; 512 * usize::from(channels)];
                assert_eq!(device.write(&source).frames, 512);
                for _ in 0..4 {
                    bridge.pump(&context).unwrap();
                }
                let transferred =
                    unsafe { tn_device_io_queued(device.native.pointer.as_ptr(), false) };
                assert!(transferred > 0);
                assert!(device.bridge_ready.load(Ordering::Acquire));
                assert_eq!(
                    unsafe { tn_device_io_graph_frames(device.native.pointer.as_ptr()) },
                    512
                );
                counts.push(transferred);
            }
            assert!(
                counts[1] > counts[0] * 3 / 2,
                "48k to 96k must produce more native frames than 44.1k: {counts:?}"
            );
        }
    }
    #[test]
    fn format_staging_is_cancel_safe_and_mismatched_graph_rate_is_silent() {
        let (device, context) = synthetic_output(2, 48_000, 44_100);
        let status = Arc::new(AdapterRuntimeStatus::new(crate::macos::adapter_profile()));
        let resources = MacDevices::new(status, 48_000, 256);
        resources.cache.lock().unwrap().insert(
            "output".into(),
            (
                Spec {
                    selector: DeviceSelector::DefaultOutput,
                    input: false,
                    channels: 2,
                },
                Arc::downgrade(&device),
            ),
        );
        let graph = GraphDocument::new(1, vec![], vec![]);
        let old = RenderFormat {
            sample_rate: 48_000,
            block_frames: 256,
        };
        let new = RenderFormat {
            sample_rate: 96_000,
            block_frames: 256,
        };
        assert_eq!(device.write(&[0.25; 512]).frames, 256);
        let cancelled = resources.prepare_format_change(&graph, old, new).unwrap();
        assert!(!resources.handoff.idle());
        assert!(resources.prepare_format_change(&graph, old, new).is_err());
        resources.reclaim();
        assert!(
            !resources.handoff.idle(),
            "recovery must not take a format reservation"
        );
        assert_eq!(device.graph_rate.load(Ordering::Acquire), 48_000);
        assert_eq!(
            unsafe { tn_device_io_queued(device.native.pointer.as_ptr(), true) },
            256
        );
        drop(cancelled);
        assert!(
            resources.handoff.idle(),
            "cancellation releases resource preparation"
        );
        assert!(device.pending_bridge.lock().unwrap().is_none());
        resources
            .prepare_format_change(&graph, old, new)
            .unwrap()
            .commit();
        assert!(
            resources.handoff.idle(),
            "commit releases resource preparation"
        );
        assert_eq!(
            (
                device.graph_rate.load(Ordering::Acquire),
                device.native_rate.load(Ordering::Acquire),
            ),
            (96_000, 44_100),
            "publishing the processing rate must preserve the physical device rate"
        );
        assert_eq!(
            device.write(&[0.25; 512]).frames,
            0,
            "old converter must not accept new-rate PCM"
        );
        assert!(
            resources.prepare_format_change(&graph, new, old).is_err(),
            "pending publication slot cannot be replaced"
        );
        let mut prepared = device.pending_bridge.lock().unwrap().take().unwrap();
        prepared.activate(&context);
        assert_eq!(
            unsafe { tn_device_io_queued(device.native.pointer.as_ptr(), true) },
            0
        );
        assert_eq!(
            (prepared.graph_rate, prepared.native_rate),
            (96_000, 44_100)
        );
    }
    #[test]
    fn same_native_device_bypasses_src_but_unknown_and_other_clocks_do_not() {
        let (master, _) = synthetic_output(2, 48_000, 48_000);
        let (peer, peer_context) = synthetic_output(2, 48_000, 48_000);
        let (other, other_context) = synthetic_output(2, 48_000, 48_000);
        master.clock_identity.store(41, Ordering::Release);
        peer.clock_identity.store(41, Ordering::Release);
        other.clock_identity.store(42, Ordering::Release);
        let devices = DeviceSet(BTreeMap::from([
            ("master".into(), master.clone()),
            ("peer".into(), peer.clone()),
            ("other".into(), other),
        ]));
        // Reproduce the previous blanket-independent policy under exactly the
        // same channel/rate/input conditions as the new relationship decision.
        let mut before = DeviceBridge::new(&peer_context, 48_000).unwrap();
        let source = [0.25; 1024];
        let mut output = [0.0; 512];
        before.bridge.push_interleaved(&source[..512]).unwrap();
        before.bridge.push_interleaved(&source[..512]).unwrap();
        let previous = before.bridge.pull_interleaved(&mut output).unwrap();
        devices.set_clock_master(Some(master.identity));
        assert!(peer_context.clock_aligned.load(Ordering::Acquire));
        assert!(!other_context.clock_aligned.load(Ordering::Acquire));
        let mut after = DeviceBridge::new(&peer_context, 48_000).unwrap();
        after.bridge.push_interleaved(&source[..512]).unwrap();
        after.bridge.push_interleaved(&source[..512]).unwrap();
        let current = after.bridge.pull_interleaved(&mut output).unwrap();
        assert!(previous.algorithmic_delay_frames > 0);
        assert_eq!(current.algorithmic_delay_frames, 0);
        assert_eq!(current.transferred_frames, 256);
        assert!(
            output
                .iter()
                .all(|sample| sample.to_bits() == 0.25_f32.to_bits())
        );
        eprintln!(
            "same-device 2ch/48000Hz/256-frame pull: SRC delay {} -> {} frames, bypass PCM bit-exact",
            previous.algorithmic_delay_frames, current.algorithmic_delay_frames
        );

        // Zero means unverified (including user aggregates), not shared clock.
        master.clock_identity.store(0, Ordering::Release);
        peer.clock_identity.store(0, Ordering::Release);
        devices.set_clock_master(Some(master.identity));
        assert!(!peer_context.clock_aligned.load(Ordering::Acquire));
        master.clock_identity.store(41, Ordering::Release);
        peer.clock_identity.store(41, Ordering::Release);
        peer.native_rate.store(44_100, Ordering::Release);
        devices.set_clock_master(Some(master.identity));
        assert!(!peer_context.clock_aligned.load(Ordering::Acquire));
        peer.native_rate.store(48_000, Ordering::Release);
        master.failed.store(true, Ordering::Release);
        devices.set_clock_master(Some(master.identity));
        assert!(!peer_context.clock_aligned.load(Ordering::Acquire));
        devices.set_clock_master(None);
        assert!(!master.clock_aligned.load(Ordering::Acquire));
    }

    #[test]
    fn hardware_master_identity_survives_bridge_priming_and_epoch_changes() {
        let (device, context) = synthetic_output(2, 48_000, 48_000);
        let devices = DeviceSet(BTreeMap::from([("output".into(), device.clone())]));
        let owner = devices.hardware_clock_progress().unwrap().owner_id;
        devices.set_clock_master(Some(owner));
        assert!(context.clock_aligned.load(Ordering::Acquire));
        device.ready_rate.store(0, Ordering::Release);
        device.epoch.fetch_add(1, Ordering::Release);
        assert_eq!(devices.hardware_clock_progress().unwrap().owner_id, owner);
        devices.set_clock_master(Some(owner));
        assert!(context.clock_aligned.load(Ordering::Acquire));
        let mut bridge = DeviceBridge::new(&context, 48_000).unwrap();
        assert!(bridge.clock_aligned);
        device.ready_rate.store(48_000, Ordering::Release);
        let source = [0.25; 1024];
        assert_eq!(device.write(&source).frames, 512);
        for _ in 0..4 {
            bridge.pump(&context).unwrap();
        }
        bridge.observe(&context, true).unwrap();
        assert_eq!(
            bridge.bridge.correction_ppm().to_bits(),
            0.0_f64.to_bits(),
            "native master cannot fight its own pacing PI"
        );
        devices.set_clock_master(None);
        assert!(!context.clock_aligned.load(Ordering::Acquire));
        device.failed.store(true, Ordering::Release);
        assert!(devices.hardware_clock_progress().is_none());
    }
    #[test]
    fn prepared_bridge_and_graph_rate_publish_as_one_worker_observation() {
        let (device, _) = synthetic_output(2, 48_000, 44_100);
        let other = device.clone();
        let worker = thread::spawn(move || {
            loop {
                let mut pending = other.pending_bridge.lock().unwrap();
                if let Some(bridge) = pending.take() {
                    assert_eq!(other.graph_rate.load(Ordering::Acquire), bridge.graph_rate);
                    return;
                }
                drop(pending);
                thread::yield_now();
            }
        });
        Box::new(PreparedDeviceFormat {
            _reservation: ResourceHandoff::<DeviceSet>::default()
                .reserve_format()
                .unwrap(),
            devices: vec![(
                device,
                DeviceBridge::prepare(2, false, 44_100, 96_000, false).unwrap(),
            )],
            published: Arc::new(PublishedFormat::new(48_000, 256)),
            requested_rate: 96_000,
            requested_buffer: 256,
            previous_buffer: 256,
        })
        .commit();
        worker.join().unwrap();
    }
    #[test]
    fn wrong_direction_and_invalid_width_fail_before_callbacks() {
        for spec in [
            Spec {
                selector: DeviceSelector::DefaultOutput,
                input: true,
                channels: 2,
            },
            Spec {
                selector: DeviceSelector::DefaultInput,
                input: false,
                channels: 2,
            },
            Spec {
                selector: DeviceSelector::DefaultInput,
                input: true,
                channels: 65,
            },
        ] {
            assert!(Device::open(&spec, 48_000).is_err());
        }
    }
    #[test]
    fn empty_graph_needs_no_native_devices() {
        let document = GraphDocument::new(1, vec![], vec![]);
        assert!(specs(&document).is_empty());
    }
}

#[cfg(test)]
#[path = "macos_devices_tests.rs"]
mod live_tests;
