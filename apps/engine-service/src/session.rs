use std::{
    sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    sync::{Arc, RwLock},
    time::Duration,
};

use mixrelay_audio_graph::RenderFormat;
use mixrelay_audio_io::session::{
    AudioSessionCapabilities, AudioSessionStatus, ClockAuthority, SessionFormat, SessionIssue,
};
use mixrelay_control_protocol::{AppStatus, DriverState, PROTOCOL_VERSION};

use crate::RuntimeStatusBackend;
use crate::persistence::StagedWrite;
use crate::session_preferences::SessionPreferences;

pub mod processing;
use processing::{ProcessingLease, ProcessingLifecycle, WorkerState};

type DeviceRateSetter = fn(u32) -> Result<(), String>;
type DeviceRateReader = fn() -> Option<u32>;
type DeviceFormatSetter = Arc<dyn Fn(RenderFormat) -> Result<(), String> + Send + Sync>;
type DeviceFormatReader = Arc<dyn Fn() -> Option<RenderFormat> + Send + Sync>;

/// Aggregate capture availability, supplied by native capture owners rather than
/// inferred from a silent meter. No configured application is also `Ready`.
#[derive(Clone, Copy)]
pub enum ApplicationRuntimeState {
    Ready,
    Waiting,
    Ambiguous,
}

/// Supplied by the linked OS implementation, never inferred from its OS name.
/// These are implemented capabilities, not everything the native API could do.
pub struct AdapterProfile {
    pub initial_format: SessionFormat,
    pub capabilities: AudioSessionCapabilities,
    pub application_capture_rates: Vec<u32>,
    pub clock_authority: ClockAuthority,
    pub transport_latency_frames: Option<u32>,
    pub requires_observed_device_rate: bool,
}

/// Reports only I/O that the currently linked adapter can actually execute.
/// This is intentionally not a table of everything the operating system supports.
pub struct AdapterRuntimeStatus {
    processing: ProcessingLifecycle,
    application_state: AtomicU32,
    // Pacing authority, synchronization evidence and explicit inactivity are one
    // publication; readers must never combine fields from different clock owners.
    clock_observation: AtomicU32,
    input_starvations: AtomicU64,
    output_overflows: AtomicU64,
    discontinuities: AtomicU64,
    profile: AdapterProfile,
    // Control-plane discovery can narrow capabilities without restarting the
    // engine. Never read/write this lock from the real-time processing path.
    discovered_sample_rates: RwLock<Option<Vec<u32>>>,
    endpoint_observation: AtomicU32,
    observed_device_format: AtomicU64,
    // Independent hardware routing authority must not overwrite the own virtual
    // endpoint observation. Bit 63 records selection even while unavailable.
    routing_clock_observation: AtomicU64,
    active_format: AtomicU64,
    failed: AtomicBool,
    set_device_rate: Option<DeviceRateSetter>,
    set_device_format: Option<DeviceFormatSetter>,
    external_format_reader: Option<DeviceFormatReader>,
    graph_format_control: bool,
    external_rate_reader: Option<DeviceRateReader>,
    preferences: Option<Result<SessionPreferences, String>>,
    preferred_format: AtomicU64,
    preferences_error: AtomicBool,
    restore_failed: AtomicBool,
}

impl AdapterRuntimeStatus {
    #[must_use]
    pub fn new(profile: AdapterProfile) -> Self {
        let initial_format = profile.initial_format;
        let initial_clock = pack_clock(
            profile.clock_authority,
            profile.clock_authority != ClockAuthority::EngineTimer,
        );
        Self {
            processing: ProcessingLifecycle::default(),
            application_state: AtomicU32::new(0),
            clock_observation: AtomicU32::new(initial_clock),
            input_starvations: AtomicU64::new(0),
            output_overflows: AtomicU64::new(0),
            discontinuities: AtomicU64::new(0),
            profile,
            discovered_sample_rates: RwLock::new(None),
            endpoint_observation: AtomicU32::new(0),
            observed_device_format: AtomicU64::new(0),
            routing_clock_observation: AtomicU64::new(0),
            active_format: AtomicU64::new(pack_format(RenderFormat {
                sample_rate: initial_format.sample_rate,
                block_frames: initial_format.block_frames,
            })),
            failed: AtomicBool::new(false),
            set_device_rate: None,
            set_device_format: None,
            external_format_reader: None,
            graph_format_control: false,
            external_rate_reader: None,
            preferences: None,
            preferred_format: AtomicU64::new(0),
            preferences_error: AtomicBool::new(false),
            restore_failed: AtomicBool::new(false),
        }
    }

    /// Called by control/monitor threads, never through an OS API in the audio callback.
    pub fn observe_supported_sample_rates(&self, native_rates: &[u32]) {
        let rates = self
            .profile
            .capabilities
            .supported_sample_rates
            .iter()
            .copied()
            .filter(|rate| native_rates.contains(rate))
            .collect();
        *self
            .discovered_sample_rates
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(rates);
    }

    fn supported_sample_rates(&self) -> Vec<u32> {
        self.discovered_sample_rates
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .unwrap_or(&self.profile.capabilities.supported_sample_rates)
            .clone()
    }

    /// Called by control/monitor threads, never through an OS API in the audio callback.
    pub fn observe_device_rate(&self, rate: Option<u32>) {
        self.observe_device_format(rate.map(|sample_rate| RenderFormat {
            sample_rate,
            block_frames: 0,
        }));
    }

    /// Publishes a coherent observed native pair. Zero block frames means a rate-only
    /// adapter; never infer the native buffer size from the DSP processing capacity.
    pub fn observe_device_format(&self, format: Option<RenderFormat>) {
        self.observed_device_format
            .store(format.map_or(0, pack_format), Ordering::Release);
    }

    /// Selects an independent routing clock without relabeling the own HAL rate.
    /// Missing hardware observation is different from returning authority to HAL.
    pub fn observe_routing_clock(&self, selected: bool, sample_rate: Option<u32>) {
        self.routing_clock_observation.store(
            if selected {
                (1_u64 << 63) | u64::from(sample_rate.unwrap_or(0))
            } else {
                0
            },
            Ordering::Release,
        );
    }

    fn routing_clock_rate(&self) -> Option<u32> {
        let value = self.routing_clock_observation.load(Ordering::Acquire);
        (value != 0).then(|| u32::try_from(value & u64::from(u32::MAX)).expect("routing rate word"))
    }

    /// Cached own-endpoint settings for the device UI. Reading these fields does
    /// not query `CoreAudio` and never substitutes the hardware routing rate.
    #[must_use]
    pub fn endpoint_format_settings(&self) -> (Option<u32>, Vec<u32>, bool) {
        let rate = unpack_format(self.observed_device_format.load(Ordering::Acquire)).sample_rate;
        let rates = self.supported_sample_rates();
        let observed = (rate != 0).then_some(rate);
        let writable = observed.is_some()
            && !rates.is_empty()
            && self.profile.capabilities.can_reconfigure_live
            && self.set_device_rate.is_some()
            && self.external_rate_reader.is_some();
        (observed, rates, writable)
    }

    /// Publishes actual native pacing and synchronization evidence together. A device
    /// callback alone is not proof that another independent clock domain is bridged.
    /// This is lock-free and performs no allocation or OS calls on the DSP boundary.
    pub fn observe_clock(&self, authority: ClockAuthority, synchronized: bool) {
        self.clock_observation
            .store(pack_clock(authority, synchronized), Ordering::Release);
    }

    /// The adapter has observed that no clock is currently pacing processing. This is
    /// not inferred from silent PCM. Retain the last authority for diagnostics, but
    /// do not describe inactivity as synchronization evidence or a clock failure.
    /// The next actual `observe_clock` publication leaves this explicit idle state.
    pub fn observe_clock_idle(&self) {
        // The update closure never returns None, so failure is impossible. A single
        // atomic update preserves whichever authority was latest at its commit point.
        let _ = self
            .clock_observation
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                Some((value & CLOCK_AUTHORITY_MASK) | CLOCK_IDLE)
            });
    }

    /// Off-RT capture supervision publishes its current aggregate availability.
    pub fn observe_application_state(&self, state: ApplicationRuntimeState) {
        self.application_state.store(
            match state {
                ApplicationRuntimeState::Ready => 0,
                ApplicationRuntimeState::Waiting => 1,
                ApplicationRuntimeState::Ambiguous => 2,
            },
            Ordering::Release,
        );
    }

    /// Control/monitor path only. Unknown is not Ready, and device existence is
    /// separate from callback activity or a graph attachment.
    pub fn observe_endpoint(&self, present: bool) {
        self.endpoint_observation
            .store(if present { 1 } else { 2 }, Ordering::Release);
    }

    /// An attachment no longer supplies current device evidence. Unknown is not
    /// absent: dropping a graph must not claim the installed driver was removed.
    pub fn forget_endpoint_observation(&self) {
        self.endpoint_observation.store(0, Ordering::Release);
        self.observe_device_rate(None);
    }

    /// One processing owner per session. Dropping the guard publishes a stopped
    /// state, including unwinding; callback ownership is never made `Send + Sync`.
    ///
    /// # Errors
    /// Fails if a worker is already running. Restart uses a fresh session object.
    pub fn begin_processing(&self) -> Result<ProcessingLease<'_>, &'static str> {
        self.processing.begin()
    }

    pub fn request_stop(&self) {
        self.processing.request_stop();
    }

    #[must_use]
    pub fn stop_requested(&self) -> bool {
        self.processing.stop_requested()
    }

    /// Lock-free reporting from the DSP boundary. Zero samples are not failures.
    pub fn record_process(&self, report: mixrelay_audio_io::process::ProcessReport) {
        self.discontinuities
            .fetch_add(report.discontinuities, Ordering::Relaxed);
        self.input_starvations
            .fetch_add(report.input_starvations, Ordering::Relaxed);
        self.output_overflows
            .fetch_add(report.output_overflows, Ordering::Relaxed);
    }

    #[must_use]
    pub fn device_matches(&self, sample_rate: u32) -> bool {
        unpack_format(self.observed_device_format.load(Ordering::Acquire)).sample_rate
            == sample_rate
    }

    #[must_use]
    pub fn with_format_control(mut self, setter: fn(u32) -> Result<(), String>) -> Self {
        self.set_device_rate = Some(setter);
        self
    }

    /// Full native pair control. The callback confirms both fields before success;
    /// rollback uses the actual pre-write pair from the observed-format reader.
    #[must_use]
    pub fn with_native_format_control(
        mut self,
        setter: impl Fn(RenderFormat) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        self.set_device_format = Some(Arc::new(setter));
        self
    }

    /// Follows this adapter's own endpoint, never an unrelated hardware/server clock.
    /// The callback runs only on control threads and may capture owned/weak backend state.
    #[must_use]
    pub fn with_observed_format_following(
        mut self,
        reader: impl Fn() -> Option<RenderFormat> + Send + Sync + 'static,
    ) -> Self {
        self.external_format_reader = Some(Arc::new(reader));
        self
    }

    /// Explicit engine-format control for adapters whose native streams negotiate their
    /// own formats. Does not imply that the OS device's nominal format can be changed.
    /// The resource backend must stage matching converters and reject fixed-rate devices.
    #[must_use]
    pub fn with_graph_format_control(mut self) -> Self {
        self.graph_format_control = true;
        self
    }

    /// Opt in only when the adapter can rebuild its processing plan after an external
    /// nominal-rate change. The reader queries native state on the control thread; it
    /// is rechecked after suspension to detect another OS/DAW change during preparation.
    /// For rate-only endpoints, startup follows the current device rate instead of
    /// overwriting another application's setting with a saved last-observed rate.
    #[must_use]
    pub fn with_observed_rate_following(mut self, reader: fn() -> Option<u32>) -> Self {
        self.external_rate_reader = Some(reader);
        self
    }

    /// Startup/control thread only, before creating the DSP worker. Bad preferences or
    /// failed native negotiation never prevent the local control service from starting.
    pub fn restore_preferences(&mut self) -> RenderFormat {
        let fallback = unpack_format(self.active_format.load(Ordering::Relaxed));
        let store = SessionPreferences::for_backend(self.profile.capabilities.backend)
            .map_err(|e| e.to_string());
        let loaded = store
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|store| store.load().map_err(|e| e.to_string()));
        self.preferences = Some(store);
        let requested = match loaded {
            Ok(Some(format)) => RenderFormat {
                sample_rate: format.sample_rate,
                block_frames: format.block_frames,
            },
            Ok(None) => return fallback,
            Err(error) => {
                self.preferences_error.store(true, Ordering::Release);
                tracing::warn!(%error, "session preferences not restored");
                return fallback;
            }
        };
        self.restore_saved_format(requested, fallback)
    }

    fn restore_saved_format(
        &self,
        requested: RenderFormat,
        fallback: RenderFormat,
    ) -> RenderFormat {
        self.preferred_format
            .store(pack_format(requested), Ordering::Release);
        if self.external_rate_reader.is_some()
            && self.set_device_format.is_none()
            && self.supports_routing_format(requested)
        {
            return self.restore_followed_rate(requested, fallback);
        }
        // A rate-only own endpoint does not own the DSP buffer preference.
        // Retain that validated buffer when hardware can run without this HAL.
        let independent_buffer = self.set_device_rate.is_some() && self.set_device_format.is_none();
        let fallback = if independent_buffer && self.supports_routing_format(requested) {
            RenderFormat {
                block_frames: requested.block_frames,
                ..fallback
            }
        } else {
            fallback
        };
        self.activate_format(fallback);
        if !self.supports_format(requested) {
            if independent_buffer && self.supports_routing_format(requested) {
                self.restore_failed.store(true, Ordering::Release);
            } else {
                self.preferences_error.store(true, Ordering::Release);
            }
            tracing::warn!(
                ?requested,
                "stored session format is unsupported by this adapter"
            );
            return fallback;
        }
        if let Err(error) = self.restore_native_format(requested, fallback) {
            self.restore_failed.store(true, Ordering::Release);
            tracing::warn!(%error, ?requested, "stored native format could not be restored");
            return fallback;
        }
        self.activate_format(requested);
        requested
    }

    /// Before any worker starts, restore the client buffer preference but let the
    /// device retain its own nominal rate. An unpublished device is unavailable,
    /// not a failed settings write; later observation or hardware routing resolves it.
    fn restore_followed_rate(&self, saved: RenderFormat, fallback: RenderFormat) -> RenderFormat {
        let observed = self.external_sample_rate().filter(|rate| {
            self.profile
                .capabilities
                .supported_sample_rates
                .contains(rate)
        });
        let actual = RenderFormat {
            sample_rate: observed.unwrap_or(fallback.sample_rate),
            block_frames: saved.block_frames,
        };
        if observed.is_some() && actual != saved {
            let persisted = self.stage_preferences(actual).and_then(|staged| {
                staged.map_or(Ok(()), |staged| {
                    staged.commit().map_err(|error| error.to_string())
                })
            });
            self.preferences_error
                .store(persisted.is_err(), Ordering::Release);
            self.preferred_format
                .store(pack_format(actual), Ordering::Release);
        }
        self.activate_format(actual);
        actual
    }

    fn write_native_format(&self, requested: RenderFormat) -> Result<(), String> {
        if let Some(setter) = &self.set_device_format {
            setter(requested)?;
            let actual = self
                .external_format_reader
                .as_ref()
                .and_then(|reader| reader());
            self.observe_device_format(actual);
            if actual != Some(requested) {
                return Err("native format did not confirm the requested pair".into());
            }
            Ok(())
        } else {
            self.set_device_rate
                .map_or(Ok(()), |setter| setter(requested.sample_rate))
        }
    }

    fn native_rollback_format(&self, fallback: RenderFormat) -> Result<RenderFormat, String> {
        if self.set_device_format.is_some() {
            self.external_format_reader
                .as_ref()
                .and_then(|reader| reader())
                .ok_or_else(|| "native format was unavailable before negotiation".into())
        } else {
            Ok(fallback)
        }
    }

    fn restore_native_format(
        &self,
        requested: RenderFormat,
        fallback: RenderFormat,
    ) -> Result<(), String> {
        let previous = self.native_rollback_format(fallback)?;
        if let Err(error) = self.write_native_format(requested) {
            if let Err(rollback) = self.write_native_format(previous) {
                self.failed.store(true, Ordering::Release);
                return Err(format!("{error}; rollback failed: {rollback}"));
            }
            return Err(error);
        }
        Ok(())
    }

    fn supports_format(&self, format: RenderFormat) -> bool {
        self.supported_sample_rates().contains(&format.sample_rate)
            && self
                .profile
                .capabilities
                .supported_block_frames
                .contains(&format.block_frames)
    }

    fn supports_routing_format(&self, format: RenderFormat) -> bool {
        self.profile
            .capabilities
            .supported_sample_rates
            .contains(&format.sample_rate)
            && self
                .profile
                .capabilities
                .supported_block_frames
                .contains(&format.block_frames)
    }

    fn session_issue(
        &self,
        format: RenderFormat,
        observed: RenderFormat,
        worker_state: WorkerState,
        clock_issue: Option<SessionIssue>,
    ) -> Option<SessionIssue> {
        if worker_state == WorkerState::Failed {
            Some(SessionIssue::ProcessingFailed)
        } else if self.failed.load(Ordering::Acquire) {
            Some(SessionIssue::ReconfigurationFailed)
        } else if self.application_state.load(Ordering::Acquire) == 2 {
            Some(SessionIssue::ApplicationAmbiguous)
        } else if self.application_state.load(Ordering::Acquire) == 1 {
            Some(SessionIssue::ApplicationUnavailable)
        } else if self.restore_failed.load(Ordering::Acquire) && self.routing_clock_rate().is_none()
        {
            Some(SessionIssue::StoredFormatUnavailable)
        } else if self.preferences_error.load(Ordering::Acquire) {
            Some(SessionIssue::PreferencesUnavailable)
        } else if self.profile.requires_observed_device_rate {
            if observed.sample_rate == 0 {
                Some(SessionIssue::DeviceUnavailable)
            } else if observed.sample_rate != format.sample_rate {
                Some(SessionIssue::SampleRateMismatch)
            } else if observed.block_frames != 0 && observed.block_frames != format.block_frames {
                Some(SessionIssue::BlockSizeMismatch)
            } else {
                clock_issue
            }
        } else {
            clock_issue
        }
    }

    fn stage_preferences(&self, format: RenderFormat) -> Result<Option<StagedWrite>, String> {
        self.preferences
            .as_ref()
            .map(|store| {
                store
                    .as_ref()
                    .map_err(Clone::clone)?
                    .stage(SessionFormat {
                        sample_rate: format.sample_rate,
                        block_frames: format.block_frames,
                    })
                    .map_err(|error| error.to_string())
            })
            .transpose()
    }

    /// Audio worker only: acknowledge suspension at a block boundary without a lock.
    #[must_use]
    pub fn checkpoint(&self) -> bool {
        self.processing.checkpoint()
    }

    /// Inspect without acknowledging: a callback owner must drain its in-flight
    /// render before the supervisor may acknowledge a format transaction.
    #[must_use]
    pub fn pause_requested(&self) -> bool {
        self.processing.pause_requested()
    }

    /// The outer graph/native transaction could not restore its original state.
    /// Keep the existing suspension latched; a successful inner rollback alone
    /// is not proof that the old DSP plan can safely process the current device.
    pub fn fail_reconfiguration(&self) {
        self.failed.store(true, Ordering::Release);
    }

    /// Follow an already-running routing clock without rewriting an unrelated
    /// virtual endpoint or its saved nominal rate. The caller must publish the
    /// newly prepared DSP plan before resuming the processing owner.
    /// # Errors
    /// Rejects unsupported product geometry or a failed processing suspension.
    pub fn adopt_routing_clock(
        &self,
        requested: RenderFormat,
    ) -> Result<(), mixrelay_audio_io::HostAudioBackendError> {
        if !self.supports_routing_format(requested) {
            return Err(
                mixrelay_audio_io::HostAudioBackendError::CapabilityUnavailable {
                    capability: "routing clock format".into(),
                },
            );
        }
        self.pause_at_boundary()?;
        self.observe_routing_clock(true, Some(requested.sample_rate));
        self.failed.store(false, Ordering::Release);
        Ok(())
    }

    /// Saves a user-selected DSP buffer while retaining the virtual endpoint's
    /// own nominal-rate preference. Native hardware buffer negotiation remains
    /// the prepared device transaction's responsibility; no HAL setter runs here.
    /// # Errors
    /// Rejects unsupported geometry, preference staging/commit failures, or a
    /// processing owner that does not acknowledge suspension.
    pub fn reconfigure_routing_buffer(
        &self,
        requested: RenderFormat,
    ) -> Result<(), mixrelay_audio_io::HostAudioBackendError> {
        use mixrelay_audio_io::HostAudioBackendError;
        if !self.supports_routing_format(requested) {
            return Err(HostAudioBackendError::CapabilityUnavailable {
                capability: "routing buffer format".into(),
            });
        }
        let saved = RenderFormat {
            sample_rate: self.preferred_format().map_or_else(
                || {
                    self.endpoint_format_settings()
                        .0
                        .unwrap_or(self.profile.initial_format.sample_rate)
                },
                |format| format.sample_rate,
            ),
            block_frames: requested.block_frames,
        };
        let staged = self.stage_preferences(saved).map_err(|message| {
            self.preferences_error.store(true, Ordering::Release);
            HostAudioBackendError::BackendFailure { message }
        })?;
        self.pause_at_boundary()?;
        if let Some(staged) = staged {
            staged.commit().map_err(|error| {
                self.preferences_error.store(true, Ordering::Release);
                HostAudioBackendError::BackendFailure {
                    message: error.to_string(),
                }
            })?;
        }
        if self.preferences.is_some() {
            self.preferred_format
                .store(pack_format(saved), Ordering::Release);
        }
        self.observe_routing_clock(true, Some(requested.sample_rate));
        self.preferences_error.store(false, Ordering::Release);
        self.failed.store(false, Ordering::Release);
        Ok(())
    }

    /// Publish an actually activated plan, never a merely requested format.
    pub fn activate_format(&self, format: RenderFormat) {
        self.active_format
            .store(pack_format(format), Ordering::Release);
    }

    /// Requests and waits for callback-boundary suspension on the control thread.
    /// # Errors
    /// Returns an error if the running audio owner does not acknowledge in time.
    pub fn pause_at_boundary(&self) -> Result<(), mixrelay_audio_io::HostAudioBackendError> {
        // Fresh tickets prevent a previous suspension acknowledgement from authorizing
        // native changes while the worker has already resumed.
        let ticket = self.processing.request_pause();
        if !self.processing.await_pause(ticket, Duration::from_secs(1)) {
            self.resume();
            return Err(mixrelay_audio_io::HostAudioBackendError::BackendFailure {
                message: "audio worker did not acknowledge suspension".into(),
            });
        }
        Ok(())
    }

    fn preferred_format(&self) -> Option<SessionFormat> {
        let value = self.preferred_format.load(Ordering::Acquire);
        (value != 0).then(|| {
            let format = unpack_format(value);
            SessionFormat {
                sample_rate: format.sample_rate,
                block_frames: format.block_frames,
            }
        })
    }
}

const CLOCK_AUTHORITY_MASK: u32 = 3;
const CLOCK_SYNCHRONIZED: u32 = 1 << 2;
const CLOCK_IDLE: u32 = 1 << 3;

const fn pack_clock(authority: ClockAuthority, synchronized: bool) -> u32 {
    let authority = match authority {
        ClockAuthority::EngineTimer => 0,
        ClockAuthority::Device => 1,
        ClockAuthority::AudioServer => 2,
        ClockAuthority::Host => 3,
    };
    authority | if synchronized { CLOCK_SYNCHRONIZED } else { 0 }
}

struct ClockObservation {
    authority: ClockAuthority,
    synchronized: bool,
    idle: bool,
}

impl ClockObservation {
    const fn unpack(value: u32) -> Self {
        Self {
            authority: match value & CLOCK_AUTHORITY_MASK {
                1 => ClockAuthority::Device,
                2 => ClockAuthority::AudioServer,
                3 => ClockAuthority::Host,
                _ => ClockAuthority::EngineTimer,
            },
            synchronized: value & CLOCK_SYNCHRONIZED != 0,
            idle: value & CLOCK_IDLE != 0,
        }
    }
}

const fn pack_format(format: RenderFormat) -> u64 {
    ((format.sample_rate as u64) << 32) | format.block_frames as u64
}

fn unpack_format(value: u64) -> RenderFormat {
    RenderFormat {
        sample_rate: u32::try_from(value >> 32).expect("upper word fits"),
        block_frames: u32::try_from(value & u64::from(u32::MAX)).expect("lower word fits"),
    }
}

impl RuntimeStatusBackend for AdapterRuntimeStatus {
    fn capabilities_for_format(&self, format: RenderFormat) -> Option<AudioSessionCapabilities> {
        let mut capabilities = self.profile.capabilities.clone();
        capabilities.supported_sample_rates = self.supported_sample_rates();
        if let Some(rate) = self.routing_clock_rate() {
            // This API reports the selected hardware clock. Changing that
            // device's native rate is a device operation, not an own-HAL write.
            capabilities.supported_sample_rates = self
                .profile
                .capabilities
                .supported_sample_rates
                .iter()
                .copied()
                .filter(|supported| *supported == rate)
                .collect();
        }
        capabilities.application_capture &= self
            .profile
            .application_capture_rates
            .contains(&format.sample_rate);
        capabilities.can_reconfigure_live &= self.set_device_rate.is_some()
            || self.set_device_format.is_some()
            || self.graph_format_control;
        Some(capabilities)
    }
    fn external_sample_rate(&self) -> Option<u32> {
        if let Some(reader) = &self.external_format_reader {
            let format = reader();
            self.observe_device_format(format);
            return format.map(|format| format.sample_rate);
        }
        let rate = self.external_rate_reader.and_then(|reader| reader());
        if self.external_rate_reader.is_some() {
            self.observe_device_rate(rate);
        }
        rate
    }

    fn external_format(&self, current: RenderFormat) -> Option<RenderFormat> {
        if let Some(reader) = &self.external_format_reader {
            let format = reader();
            self.observe_device_format(format);
            format
        } else {
            self.external_sample_rate().map(|sample_rate| RenderFormat {
                sample_rate,
                ..current
            })
        }
    }

    fn adopt_external_format(
        &self,
        requested: RenderFormat,
    ) -> Result<(), mixrelay_audio_io::HostAudioBackendError> {
        use mixrelay_audio_io::HostAudioBackendError;
        if !self.supports_format(requested)
            || (self.external_rate_reader.is_none() && self.external_format_reader.is_none())
        {
            return Err(HostAudioBackendError::CapabilityUnavailable {
                capability: "external format following".into(),
            });
        }
        if self.external_format(requested) != Some(requested) {
            return Err(HostAudioBackendError::BackendFailure {
                message: "external native format changed during preparation".into(),
            });
        }
        // The device has already changed. Storage failures are warnings, not a reason
        // to keep processing PCM at the wrong rate. Preserve the last good file.
        let staged = self.stage_preferences(requested);
        self.pause_at_boundary()?;
        if self.external_format(requested) != Some(requested) {
            return Err(HostAudioBackendError::BackendFailure {
                message: "external native format changed during suspension".into(),
            });
        }
        let saved = staged.and_then(|staged| {
            staged.map_or(Ok(()), |staged| {
                staged.commit().map_err(|error| error.to_string())
            })
        });
        self.preferences_error
            .store(saved.is_err(), Ordering::Release);
        if let Err(error) = saved {
            tracing::warn!(%error, ?requested, "externally accepted format was not saved");
        }
        if self.preferences.is_some() {
            self.preferred_format
                .store(pack_format(requested), Ordering::Release);
        }
        self.restore_failed.store(false, Ordering::Release);
        // Never call the native setter here. The caller publishes the prepared DSP plan
        // before resuming; the saved last-accepted format prevents restart undoing the DAW.
        self.failed.store(false, Ordering::Release);
        Ok(())
    }

    fn status(&self, format: RenderFormat) -> AppStatus {
        let format = if self.set_device_rate.is_some()
            || self.set_device_format.is_some()
            || self.external_rate_reader.is_some()
            || self.external_format_reader.is_some()
            || self.graph_format_control
        {
            unpack_format(self.active_format.load(Ordering::Acquire))
        } else {
            format
        };
        let observed = self.routing_clock_rate().map_or_else(
            || unpack_format(self.observed_device_format.load(Ordering::Acquire)),
            |sample_rate| RenderFormat {
                sample_rate,
                block_frames: 0,
            },
        );
        let worker_state = self.processing.state();
        let clock = ClockObservation::unpack(self.clock_observation.load(Ordering::Acquire));
        let clock_issue =
            (!clock.idle && !clock.synchronized).then_some(SessionIssue::ClockNotSynchronized);
        let observed_device_sample_rate =
            (observed.sample_rate != 0).then_some(observed.sample_rate);
        let observed_device_buffer_frames =
            (observed.block_frames != 0).then_some(observed.block_frames);
        let issue = self.session_issue(format, observed, worker_state, clock_issue);
        let capabilities = self
            .capabilities_for_format(format)
            .expect("adapter always supplies capabilities");
        AppStatus {
            processor_timings: Vec::new(),
            recorder_error: None,
            protocol_version: PROTOCOL_VERSION,
            driver_state: match (
                capabilities.virtual_endpoint,
                self.endpoint_observation.load(Ordering::Acquire),
            ) {
                (true, 1) => DriverState::Ready,
                (true, 0) => DriverState::InstalledNotLoaded,
                _ => DriverState::NotInstalled,
            },
            // An operational DSP engine is independent from virtual-driver installation.
            sample_rate: Some(format.sample_rate),
            buffer_frames: Some(format.block_frames),
            underruns: self.input_starvations.load(Ordering::Relaxed),
            overruns: self.output_overflows.load(Ordering::Relaxed),
            audio_session: Some(AudioSessionStatus {
                endpoint_preferences_unavailable: false,
                virtual_endpoint_settings: None,
                processing_state: match worker_state {
                    WorkerState::Running if !clock.idle => {
                        mixrelay_audio_io::process::ProcessingState::Running
                    }
                    WorkerState::Stopped | WorkerState::Failed => {
                        mixrelay_audio_io::process::ProcessingState::Stopped
                    }
                    WorkerState::NotStarted | WorkerState::Running => {
                        mixrelay_audio_io::process::ProcessingState::Idle
                    }
                },
                discontinuities: self.discontinuities.load(Ordering::Relaxed),
                format: SessionFormat {
                    sample_rate: format.sample_rate,
                    block_frames: format.block_frames,
                },
                preferred_format: self.preferred_format(),
                clock_authority: clock.authority,
                observed_device_sample_rate,
                observed_device_buffer_frames,
                issue,
                transport_latency_frames: self.profile.transport_latency_frames,
                capabilities,
            }),
        }
    }

    fn reconfigure(
        &self,
        current: RenderFormat,
        requested: RenderFormat,
    ) -> Result<(), mixrelay_audio_io::HostAudioBackendError> {
        use mixrelay_audio_io::HostAudioBackendError;
        if !self.supports_format(requested) || !self.profile.capabilities.can_reconfigure_live {
            return Err(HostAudioBackendError::CapabilityUnavailable {
                capability: "requested native session format".into(),
            });
        }
        if self.set_device_rate.is_none()
            && self.set_device_format.is_none()
            && !self.graph_format_control
        {
            return Err(HostAudioBackendError::CapabilityUnavailable {
                capability: "live format reconfiguration".into(),
            });
        }
        // Serialize and sync the new preference before suspending audio. The rename is
        // committed only after the native device has confirmed the requested format.
        let staged = self.stage_preferences(requested).map_err(|message| {
            self.preferences_error.store(true, Ordering::Release);
            HostAudioBackendError::BackendFailure { message }
        })?;
        self.pause_at_boundary()?;
        let previous_native = self
            .native_rollback_format(current)
            .map_err(|message| HostAudioBackendError::BackendFailure { message })?;
        let negotiated = self.write_native_format(requested).and_then(|()| {
            if let Some(staged) = staged {
                staged.commit().map_err(|error| {
                    self.preferences_error.store(true, Ordering::Release);
                    format!("session preferences could not be committed: {error}")
                })?;
            }
            Ok(())
        });
        if let Err(error) = negotiated {
            if let Err(rollback) = self.write_native_format(previous_native) {
                self.failed.store(true, Ordering::Release);
                return Err(HostAudioBackendError::BackendFailure {
                    message: format!(
                        "{error}; rollback failed: {rollback}; audio remains suspended"
                    ),
                });
            }
            return Err(HostAudioBackendError::BackendFailure { message: error });
        }
        if self.set_device_format.is_some() {
            self.observe_device_format(Some(requested));
        } else if self.set_device_rate.is_some() {
            self.observe_device_rate(Some(requested.sample_rate));
        }
        if self.preferences.is_some() {
            self.preferred_format
                .store(pack_format(requested), Ordering::Release);
        }
        self.preferences_error.store(false, Ordering::Release);
        self.restore_failed.store(false, Ordering::Release);
        self.failed.store(false, Ordering::Release);
        Ok(())
    }

    fn resume(&self) {
        if !self.failed.load(Ordering::Acquire) {
            self.processing.resume();
        }
    }
}

/// Control-plane ownership of the audio thread. Bind/startup failures and normal
/// server shutdown request cancellation and join before platform resources die.
/// Never construct or destroy this owner on an audio callback. A stuck native
/// driver call still requires process isolation; Rust cannot safely kill a thread.
pub struct AudioWorker {
    status: Arc<AdapterRuntimeStatus>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl AudioWorker {
    #[must_use]
    pub fn new(status: Arc<AdapterRuntimeStatus>, thread: std::thread::JoinHandle<()>) -> Self {
        Self {
            status,
            thread: Some(thread),
        }
    }
}

impl Drop for AudioWorker {
    fn drop(&mut self) {
        self.status.request_stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Wait for an OS shutdown request on the control runtime, never the audio
/// thread. Server teardown then drops `AudioWorker` and joins native resources.
///
/// # Errors
/// Returns signal registration failures rather than silently detaching a worker.
pub async fn shutdown_signal() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result,
            _ = terminate.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await
}

#[cfg(test)]
#[path = "session_clock_tests.rs"]
mod clock_tests;

#[cfg(test)]
#[path = "session_rate_tests.rs"]
mod rate_tests;

#[cfg(test)]
#[path = "session_native_format_tests.rs"]
mod native_format_tests;

#[cfg(test)]
#[path = "session_catalog_tests.rs"]
mod catalog_tests;

#[cfg(test)]
#[path = "session_routing_tests.rs"]
mod routing_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use mixrelay_audio_io::session::AudioBackendKind;

    pub(super) fn profile(backend: AudioBackendKind) -> AdapterProfile {
        AdapterProfile {
            initial_format: SessionFormat {
                sample_rate: 44_100,
                block_frames: 128,
            },
            capabilities: AudioSessionCapabilities {
                backend,
                supported_sample_rates: vec![44_100, 48_000, 96_000],
                supported_block_frames: vec![64, 128, 256],
                can_reconfigure_live: false,
                hardware_input: true,
                hardware_output: false,
                virtual_endpoint: true,
                application_capture: true,
                sample_rate_conversion: false,
                drift_correction: false,
            },
            application_capture_rates: vec![44_100, 96_000],
            clock_authority: ClockAuthority::AudioServer,
            transport_latency_frames: None,
            requires_observed_device_rate: false,
        }
    }

    #[test]
    fn os_label_does_not_invent_device_health_or_supported_io() {
        for backend in [
            AudioBackendKind::CoreAudio,
            AudioBackendKind::PipeWire,
            AudioBackendKind::Wasapi,
        ] {
            let status = AdapterRuntimeStatus::new(profile(backend));
            let format = RenderFormat {
                sample_rate: 44_100,
                block_frames: 128,
            };
            let initial = status.status(format);
            assert_eq!(initial.driver_state, DriverState::InstalledNotLoaded);
            let session = initial.audio_session.unwrap();
            assert_eq!(session.clock_authority, ClockAuthority::AudioServer);
            assert!(session.capabilities.hardware_input);
            assert!(!session.capabilities.hardware_output);
            assert!(session.capabilities.application_capture);
            status.observe_endpoint(true);
            assert_eq!(status.status(format).driver_state, DriverState::Ready);
            status.observe_device_rate(Some(44_100));
            status.forget_endpoint_observation();
            let detached = status.status(format);
            assert_eq!(detached.driver_state, DriverState::InstalledNotLoaded);
            assert_eq!(
                detached.audio_session.unwrap().observed_device_sample_rate,
                None
            );
            status.observe_endpoint(false);
            assert_eq!(
                status.status(format).driver_state,
                DriverState::NotInstalled
            );
            assert!(
                !status
                    .status(RenderFormat {
                        sample_rate: 48_000,
                        ..format
                    })
                    .audio_session
                    .unwrap()
                    .capabilities
                    .application_capture
            );
        }
    }

    #[test]
    fn timer_capability_cannot_claim_synchronized_audio() {
        let mut descriptor = profile(AudioBackendKind::PipeWire);
        descriptor.clock_authority = ClockAuthority::EngineTimer;
        let status = AdapterRuntimeStatus::new(descriptor);
        assert_eq!(
            status
                .status(RenderFormat {
                    sample_rate: 48_000,
                    block_frames: 128
                })
                .audio_session
                .unwrap()
                .issue,
            Some(SessionIssue::ClockNotSynchronized)
        );
    }

    #[test]
    fn processing_owner_and_real_transfer_counters_have_a_lifetime() {
        let status = AdapterRuntimeStatus::new(profile(AudioBackendKind::PipeWire));
        let lease = status.begin_processing().unwrap();
        assert!(status.begin_processing().is_err());
        status.record_process(mixrelay_audio_io::process::ProcessReport {
            input_starvations: 3,
            output_overflows: 2,
            discontinuities: 4,
        });
        let format = RenderFormat {
            sample_rate: 48_000,
            block_frames: 128,
        };
        let running = status.status(format);
        assert_eq!((running.underruns, running.overruns), (3, 2));
        assert_eq!(running.audio_session.as_ref().unwrap().discontinuities, 4);
        assert_eq!(
            running.audio_session.unwrap().processing_state,
            mixrelay_audio_io::process::ProcessingState::Running
        );
        drop(lease);
        assert_eq!(
            status.status(format).audio_session.unwrap().issue,
            Some(SessionIssue::ProcessingFailed)
        );
        assert!(status.begin_processing().is_err());
    }

    #[test]
    fn dropping_worker_cancels_and_joins_native_resource_ownership() {
        struct NativeResource(Arc<AtomicBool>);
        impl Drop for NativeResource {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }
        let status = Arc::new(AdapterRuntimeStatus::new(profile(
            AudioBackendKind::PipeWire,
        )));
        let released = Arc::new(AtomicBool::new(false));
        let thread_status = status.clone();
        let thread_released = released.clone();
        let (ready, receive) = std::sync::mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || {
            let _resource = NativeResource(thread_released);
            let _lease = thread_status.begin_processing().unwrap();
            ready.send(()).unwrap();
            while !thread_status.stop_requested() {
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        let owner = AudioWorker::new(status.clone(), thread);
        receive.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(!released.load(Ordering::Acquire));
        drop(owner);
        assert!(released.load(Ordering::Acquire));
        assert_eq!(
            status
                .status(RenderFormat {
                    sample_rate: 48_000,
                    block_frames: 128
                })
                .audio_session
                .unwrap()
                .processing_state,
            mixrelay_audio_io::process::ProcessingState::Stopped
        );
        assert_eq!(
            status
                .status(RenderFormat {
                    sample_rate: 48_000,
                    block_frames: 128
                })
                .audio_session
                .unwrap()
                .issue,
            None
        );
    }

    #[test]
    fn unwinding_is_processing_failure_even_if_cancellation_was_requested() {
        let status = AdapterRuntimeStatus::new(profile(AudioBackendKind::PipeWire));
        status.observe_endpoint(true);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _lease = status.begin_processing().unwrap();
            status.request_stop();
            panic!("native owner failed during shutdown");
        }));
        assert!(result.is_err());
        let report = status.status(RenderFormat {
            sample_rate: 48_000,
            block_frames: 128,
        });
        // A failed DSP owner must not claim that the installed virtual driver vanished.
        assert_eq!(report.driver_state, DriverState::Ready);
        assert_eq!(
            report.audio_session.unwrap().issue,
            Some(SessionIssue::ProcessingFailed)
        );
    }

    #[test]
    fn application_availability_recovers_without_hiding_processing_failure() {
        let status = AdapterRuntimeStatus::new(profile(AudioBackendKind::PipeWire));
        let format = RenderFormat {
            sample_rate: 48_000,
            block_frames: 128,
        };
        let lease = status.begin_processing().unwrap();
        for (state, issue) in [
            (
                ApplicationRuntimeState::Waiting,
                Some(SessionIssue::ApplicationUnavailable),
            ),
            (
                ApplicationRuntimeState::Ambiguous,
                Some(SessionIssue::ApplicationAmbiguous),
            ),
            (ApplicationRuntimeState::Ready, None),
        ] {
            status.observe_application_state(state);
            assert_eq!(status.status(format).audio_session.unwrap().issue, issue);
        }
        drop(lease);
        status.observe_application_state(ApplicationRuntimeState::Waiting);
        assert_eq!(
            status.status(format).audio_session.unwrap().issue,
            Some(SessionIssue::ProcessingFailed)
        );
    }
}
