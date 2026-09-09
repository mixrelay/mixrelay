use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    collections::BTreeSet,
    ffi::{CStr, CString, c_char, c_void},
    fmt,
    ptr::NonNull,
    sync::{Arc, Mutex},
    thread::JoinHandle,
    time::Duration,
};

use tauri::State;
use tonenari_audio_graph::{ApplicationCaptureRequest, ApplicationSelector, SourcePlaybackPolicy};
use tonenari_audio_io::{
    ApplicationCaptureBackend, ApplicationCaptureCapabilities, ApplicationCaptureHealth,
    ApplicationCaptureSession, ApplicationSelectorKind, AudioDeviceDescriptor,
    CapturableApplication, ChannelLayout, HostAudioBackendError, ResolvedApplicationCapture,
    StreamSpec,
};

#[path = "macos_audio/capture_reader.rs"]
mod capture_reader;
#[path = "macos_audio/capture_transaction.rs"]
mod capture_transaction;
#[path = "macos_audio/clock_pump.rs"]
mod clock_pump;
#[cfg(all(test, tonenari_application_clock_fixture))]
#[path = "macos_audio/live_clock_tests.rs"]
mod live_clock_tests;

const CAPTURE_RING_CAPACITY_FRAMES: usize = 16_384;
const ENGINE_BLOCK_FRAMES: usize = 128;
const MAXIMUM_METER_CHANNELS: usize = 64;
const PUMP_IDLE_INTERVAL: Duration = Duration::from_micros(250);

#[repr(C)]
struct NativeProcessTapSession {
    _private: [u8; 0],
}

#[repr(C)]
struct NativeApplicationSourceTransport {
    _private: [u8; 0],
}

const NATIVE_OK: i32 = 0;
const NATIVE_INVALID_ARGUMENT: i32 = 1;
const NATIVE_RESOURCE_BUSY: i32 = 9;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct NativeError {
    code: i32,
    os_status: i32,
}

#[repr(C)]
struct NativeConfig {
    bundle_id: *const c_char,
    mute_while_captured: bool,
    ring_capacity_frames: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct NativeTransportError {
    code: i32,
    system_error: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NativeClockSnapshot {
    format_word: u64,
    epoch: u64,
    produced_frames: u64,
    consumed_frames: u64,
    consumer_ticks: u64,
    consumer_running: u32,
    clock_master: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NativeStats {
    callback_count: u64,
    captured_frames: u64,
    dropped_frames: u64,
    available_frames: usize,
    channels: u32,
    sample_rate: f64,
    running: bool,
    format_generation: u64,
    clock_discontinuities: u64,
    sample_time: f64,
    host_time: u64,
    clock_valid: bool,
}

unsafe extern "C" {
    fn tn_audio_device_visit(
        visitor: Option<
            unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, u32, u32, bool, bool),
        >,
        context: *mut c_void,
    ) -> NativeError;
    fn tn_process_tap_visit_applications(
        visitor: Option<
            unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, *const c_char, i32),
        >,
        context: *mut c_void,
    ) -> NativeError;
    fn tn_process_tap_create(
        config: *const NativeConfig,
        error: *mut NativeError,
    ) -> *mut NativeProcessTapSession;
    fn tn_process_tap_start(session: *mut NativeProcessTapSession) -> NativeError;
    fn tn_process_tap_refresh_format(session: *mut NativeProcessTapSession) -> NativeError;
    fn tn_process_tap_destroy(session: *mut NativeProcessTapSession);
    fn tn_process_tap_read(
        session: *mut NativeProcessTapSession,
        destination: *mut f32,
        maximum_frames: usize,
    ) -> usize;
    fn tn_process_tap_stats(session: *const NativeProcessTapSession) -> NativeStats;
    fn tn_process_tap_take_peaks(
        session: *mut NativeProcessTapSession,
        destination: *mut f32,
        maximum_channels: usize,
    ) -> usize;
    fn tn_application_source_transport_connect(
        source_id: *const c_char,
        capacity_frames: u32,
        channels: u32,
        error: *mut NativeTransportError,
    ) -> *mut NativeApplicationSourceTransport;
    fn tn_application_source_transport_write(
        transport: *mut NativeApplicationSourceTransport,
        interleaved_samples: *const f32,
        frame_count: u32,
    ) -> bool;
    fn tn_application_source_transport_destroy(transport: *mut NativeApplicationSourceTransport);
    fn tn_application_source_transport_clock(
        transport: *mut NativeApplicationSourceTransport,
        snapshot: *mut NativeClockSnapshot,
    ) -> bool;
    fn tn_application_source_transport_reset(
        transport: *mut NativeApplicationSourceTransport,
        format_word: u64,
        native_sample_rate: u32,
    ) -> bool;
    fn tn_application_source_transport_publish_native(
        transport: *mut NativeApplicationSourceTransport,
        native_frames: u64,
        running: bool,
    );
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CaptureProbeStats {
    callback_count: u64,
    captured_frames: u64,
    dropped_frames: u64,
    available_frames: usize,
    channels: u32,
    sample_rate: f64,
    running: bool,
    format_generation: u64,
    clock_discontinuities: u64,
    sample_time: f64,
    host_time: u64,
    clock_valid: bool,
}

impl From<NativeStats> for CaptureProbeStats {
    fn from(stats: NativeStats) -> Self {
        Self {
            callback_count: stats.callback_count,
            captured_frames: stats.captured_frames,
            dropped_frames: stats.dropped_frames,
            available_frames: stats.available_frames,
            channels: stats.channels,
            sample_rate: stats.sample_rate,
            running: stats.running,
            format_generation: stats.format_generation,
            clock_discontinuities: stats.clock_discontinuities,
            sample_time: stats.sample_time,
            host_time: stats.host_time,
            clock_valid: stats.clock_valid,
        }
    }
}

#[derive(Debug)]
struct CaptureProbeError(NativeError);

impl fmt::Display for CaptureProbeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "application capture failed: {} (OSStatus {})",
            native_error_name(self.0.code),
            self.0.os_status
        )
    }
}

struct CaptureProbeSession {
    native: NonNull<NativeProcessTapSession>,
    reader_claim: AtomicBool,
    resolved: ResolvedApplicationCapture,
    #[cfg(debug_assertions)]
    debug_meter_reported: AtomicBool,
}

// The pointer owns a Core Audio session whose callback state uses C atomics.
// Lifecycle setup happens before publication; the move-only reader serializes
// pump-side format refresh and PCM consumption. Probe commands own a separate mutex.
unsafe impl Send for CaptureProbeSession {}
// SAFETY: shared access exposes atomic telemetry, never a PCM read. A non-Clone
// reader token owns the only SPSC consumer and requires mutable access to read.
unsafe impl Sync for CaptureProbeSession {}

impl CaptureProbeSession {
    fn start(bundle_id: &str, mute_while_captured: bool) -> Result<Self, CaptureProbeError> {
        let bundle_id = CString::new(bundle_id)
            .map_err(|_| CaptureProbeError(NativeError::new(NATIVE_INVALID_ARGUMENT)))?;
        let config = NativeConfig {
            bundle_id: bundle_id.as_ptr(),
            mute_while_captured,
            ring_capacity_frames: CAPTURE_RING_CAPACITY_FRAMES,
        };
        let mut error = NativeError::new(NATIVE_OK);
        // SAFETY: `config` and its C string remain alive for the synchronous creation call.
        let native = unsafe { tn_process_tap_create(&raw const config, &raw mut error) };
        let native = NonNull::new(native).ok_or(CaptureProbeError(error))?;
        // SAFETY: `native` was returned as an owned, initialized session.
        let start_error = unsafe { tn_process_tap_start(native.as_ptr()) };
        if start_error.code != NATIVE_OK {
            // SAFETY: this path still owns the session and no Rust alias can observe it.
            unsafe { tn_process_tap_destroy(native.as_ptr()) };
            return Err(CaptureProbeError(start_error));
        }
        let stats: CaptureProbeStats = unsafe { tn_process_tap_stats(native.as_ptr()) }.into();
        let Ok(channels) = u16::try_from(stats.channels) else {
            // SAFETY: no Arc/session escaped; destroy consumes the native owner.
            unsafe { tn_process_tap_destroy(native.as_ptr()) };
            return Err(CaptureProbeError(NativeError::new(NATIVE_INVALID_ARGUMENT)));
        };
        let channel_layout = match channels {
            1 => ChannelLayout::Mono,
            2 => ChannelLayout::Stereo,
            _ => ChannelLayout::Discrete,
        };
        Ok(Self {
            native,
            reader_claim: AtomicBool::new(false),
            resolved: ResolvedApplicationCapture {
                source_id: bundle_id.to_string_lossy().into_owned(),
                stream: StreamSpec {
                    channels,
                    channel_layout,
                },
            },
            #[cfg(debug_assertions)]
            debug_meter_reported: AtomicBool::new(false),
        })
    }

    fn stats(&self) -> CaptureProbeStats {
        // SAFETY: the session remains owned for the duration of this call.
        unsafe { tn_process_tap_stats(self.native.as_ptr()) }.into()
    }

    fn take_native_peaks(&self, channel_count: usize) -> Vec<f32> {
        let mut peaks = [0.0_f32; MAXIMUM_METER_CHANNELS];
        // SAFETY: the destination is writable for the declared capacity and the session is alive.
        let count = unsafe {
            tn_process_tap_take_peaks(
                self.native.as_ptr(),
                peaks.as_mut_ptr(),
                channel_count.min(MAXIMUM_METER_CHANNELS),
            )
        };
        #[cfg(debug_assertions)]
        if peaks[..count].iter().any(|peak| *peak > 0.0)
            && !self.debug_meter_reported.swap(true, Ordering::Relaxed)
        {
            let stats = self.stats();
            eprintln!(
                "application meter started in desktop: running={} callbacks={} captured_frames={} dropped_frames={} available_frames={} channels={} sample_rate={} peaks={:?}",
                stats.running,
                stats.callback_count,
                stats.captured_frames,
                stats.dropped_frames,
                stats.available_frames,
                stats.channels,
                stats.sample_rate,
                &peaks[..count],
            );
        }
        peaks[..count].to_vec()
    }

    fn acquire_reader(self: &Arc<Self>) -> Result<NativeCaptureReader, String> {
        capture_reader::ExclusiveReader::acquire(Arc::clone(self))
            .map(NativeCaptureReader)
            .ok_or_else(|| "application capture already has a PCM reader".to_owned())
    }
}

impl capture_reader::ReaderOwner for CaptureProbeSession {
    fn reader_claim(&self) -> &AtomicBool {
        &self.reader_claim
    }
}

/// Telemetry keeps an Arc session; only this move-only token can consume PCM.
struct NativeCaptureReader(capture_reader::ExclusiveReader<CaptureProbeSession>);

impl NativeCaptureReader {
    fn session(&self) -> &CaptureProbeSession {
        self.0.owner()
    }

    fn read_interleaved(&mut self, destination: &mut [f32]) -> usize {
        let session = self.session();
        let channels = usize::from(session.resolved.stream.channels);
        if channels == 0 {
            return 0;
        }
        // SAFETY: the atomic lease admits one reader; &mut prevents overlapping
        // calls through it. Its owner Arc outlives the bounded native read.
        unsafe {
            tn_process_tap_read(
                session.native.as_ptr(),
                destination.as_mut_ptr(),
                destination.len() / channels,
            )
        }
    }
}

impl ApplicationCaptureSession for CaptureProbeSession {
    fn resolved(&self) -> &ResolvedApplicationCapture {
        &self.resolved
    }

    fn read_interleaved(
        &mut self,
        destination: &mut [f32],
    ) -> Result<usize, HostAudioBackendError> {
        let channels = usize::from(self.resolved.stream.channels);
        if channels == 0 {
            return Ok(0);
        }
        // SAFETY: &mut session excludes shared Arc readers (Arc::get_mut fails
        // while a reader owns an Arc). The destination covers maximum_frames.
        Ok(unsafe {
            tn_process_tap_read(
                self.native.as_ptr(),
                destination.as_mut_ptr(),
                destination.len() / channels,
            )
        })
    }

    fn take_peaks(&self) -> Result<Vec<f32>, HostAudioBackendError> {
        Ok(self.take_native_peaks(usize::from(self.resolved.stream.channels)))
    }

    fn health(&self) -> Result<ApplicationCaptureHealth, HostAudioBackendError> {
        let stats = self.stats();
        Ok(ApplicationCaptureHealth {
            callback_count: stats.callback_count,
            captured_frames: stats.captured_frames,
            dropped_frames: stats.dropped_frames,
            available_frames: stats.available_frames,
            running: stats.running,
        })
    }
}

impl Drop for CaptureProbeSession {
    fn drop(&mut self) {
        // SAFETY: `CaptureProbeSession` has unique ownership and the C destructor is null-safe.
        unsafe { tn_process_tap_destroy(self.native.as_ptr()) };
    }
}

struct MacApplicationCaptureBackend {
    capabilities: ApplicationCaptureCapabilities,
}

impl Default for MacApplicationCaptureBackend {
    fn default() -> Self {
        Self {
            capabilities: ApplicationCaptureCapabilities {
                max_channels: 2,
                supported_selectors: BTreeSet::from([ApplicationSelectorKind::BundleId]),
                supports_mute_while_captured: true,
            },
        }
    }
}

impl ApplicationCaptureBackend for MacApplicationCaptureBackend {
    type Session = CaptureProbeSession;

    fn capabilities(&self) -> &ApplicationCaptureCapabilities {
        &self.capabilities
    }

    fn open(
        &self,
        request: &ApplicationCaptureRequest,
    ) -> Result<Self::Session, HostAudioBackendError> {
        request.validate(&self.capabilities).map_err(|error| {
            HostAudioBackendError::CapabilityUnavailable {
                capability: error.to_string(),
            }
        })?;
        let ApplicationSelector::BundleId {
            bundle_id,
            include_descendants: false,
        } = &request.selector
        else {
            return Err(HostAudioBackendError::CapabilityUnavailable {
                capability: "macOS requires an exact bundle selector without descendant expansion"
                    .to_owned(),
            });
        };
        CaptureProbeSession::start(
            bundle_id,
            request.source_playback == SourcePlaybackPolicy::MuteWhileCaptured,
        )
        .map_err(|error| HostAudioBackendError::BackendFailure {
            message: error.to_string(),
        })
    }
}

struct ApplicationSourceTransport {
    native: NonNull<NativeApplicationSourceTransport>,
}

// The mapped transport is exclusively owned by one pump thread and uses lock-free shared atoms.
unsafe impl Send for ApplicationSourceTransport {}

impl ApplicationSourceTransport {
    fn publish_native(&mut self, frames: u64, running: bool) {
        // SAFETY: sole pump owner; bounded atomic publication into validated trailer.
        unsafe {
            tn_application_source_transport_publish_native(self.native.as_ptr(), frames, running);
        };
    }
    fn clock(&mut self) -> Option<NativeClockSnapshot> {
        let mut snapshot = NativeClockSnapshot::default();
        // SAFETY: sole worker owns the transport; C validates the versioned mapping.
        unsafe { tn_application_source_transport_clock(self.native.as_ptr(), &raw mut snapshot) }
            .then_some(snapshot)
    }

    fn reset(&mut self, format: u64, native_rate: u32) -> bool {
        // SAFETY: only the producer worker can reset this transport's epoch.
        unsafe { tn_application_source_transport_reset(self.native.as_ptr(), format, native_rate) }
    }

    fn connect(source_id: &str, channels: u16) -> Result<Self, String> {
        let source_id = CString::new(source_id)
            .map_err(|_| "application source ID contains a null byte".to_owned())?;
        let mut error = NativeTransportError {
            code: 0,
            system_error: 0,
        };
        // SAFETY: the source ID remains valid for the synchronous XPC attachment request.
        let native = unsafe {
            tn_application_source_transport_connect(
                source_id.as_ptr(),
                u32::try_from(CAPTURE_RING_CAPACITY_FRAMES)
                    .map_err(|_| "application source capacity is invalid".to_owned())?,
                u32::from(channels),
                &raw mut error,
            )
        };
        let native = NonNull::new(native).ok_or_else(|| {
            format!(
                "application source transport failed: code={} system_error={}",
                error.code, error.system_error
            )
        })?;
        Ok(Self { native })
    }

    fn write(&mut self, samples: &[f32], frames: usize) -> bool {
        let Ok(frames) = u32::try_from(frames) else {
            return false;
        };
        // SAFETY: this pump exclusively owns the transport and samples cover the frame count.
        unsafe {
            tn_application_source_transport_write(self.native.as_ptr(), samples.as_ptr(), frames)
        }
    }
}

impl Drop for ApplicationSourceTransport {
    fn drop(&mut self) {
        // SAFETY: the pump has unique ownership of this mapped transport.
        unsafe { tn_application_source_transport_destroy(self.native.as_ptr()) };
    }
}

struct ApplicationAudioSession {
    capture: Arc<CaptureProbeSession>,
    pump: Arc<AudioPump>,
    prepared_reader: Mutex<Option<NativeCaptureReader>>,
}

struct AudioPump {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    handoff: Arc<capture_transaction::CaptureHandoff<NativeCaptureReader>>,
}

impl AudioPump {
    fn start(source_id: &str, capture: NativeCaptureReader, channels: u16) -> Result<Self, String> {
        let transport = ApplicationSourceTransport::connect(source_id, channels)?;
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let channel_count = usize::from(channels);
        let handoff = Arc::new(capture_transaction::CaptureHandoff::default());
        let worker_handoff = Arc::clone(&handoff);
        let worker = std::thread::Builder::new()
            .name(format!("tonenari-source-{source_id}"))
            .spawn(move || {
                run_audio_pump(
                    capture,
                    transport,
                    channel_count,
                    &worker_stop,
                    &worker_handoff,
                );
            })
            .map_err(|error| format!("could not start application source pump: {error}"))?;
        Ok(Self {
            stop,
            worker: Some(worker),
            handoff,
        })
    }
}

impl Drop for AudioPump {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn run_audio_pump(
    capture: NativeCaptureReader,
    transport: ApplicationSourceTransport,
    channels: usize,
    stop: &AtomicBool,
    handoff: &capture_transaction::CaptureHandoff<NativeCaptureReader>,
) {
    if let Err(error) = clock_pump::run_with_handoff(capture, transport, channels, stop, handoff) {
        eprintln!("application source clock bridge stopped: {error}");
    }
}

impl NativeError {
    const fn new(code: i32) -> Self {
        Self { code, os_status: 0 }
    }
}

fn native_error_name(code: i32) -> &'static str {
    match code {
        0 => "ok",
        1 => "invalid_argument",
        2 => "unsupported_system",
        3 => "application_not_found",
        4 => "core_audio_failure",
        5 => "unsupported_format",
        6 => "out_of_memory",
        7 => "already_running",
        8 => "not_running",
        9 => "resource_busy",
        _ => "unknown_native_error",
    }
}

unsafe extern "C" fn collect_application(
    context: *mut c_void,
    bundle_id: *const c_char,
    display_name: *const c_char,
    icon_data_url: *const c_char,
    process_id: i32,
) {
    if context.is_null() || bundle_id.is_null() || display_name.is_null() {
        return;
    }
    // SAFETY: the native enumerator keeps both pointers valid for this synchronous callback.
    let applications = unsafe { &mut *context.cast::<Vec<CapturableApplication>>() };
    // SAFETY: Core Audio bundle IDs are passed as null-terminated UTF-8 for this callback.
    let bundle_id = unsafe { CStr::from_ptr(bundle_id) };
    let display_name = unsafe { CStr::from_ptr(display_name) };
    let icon_data_url = if icon_data_url.is_null() {
        None
    } else {
        // SAFETY: the native enumerator keeps this optional pointer valid for the callback.
        unsafe { CStr::from_ptr(icon_data_url) }
            .to_str()
            .ok()
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    if let (Ok(bundle_id), Ok(display_name), Ok(process_id)) = (
        bundle_id.to_str(),
        display_name.to_str(),
        u32::try_from(process_id),
    ) && !bundle_id.is_empty()
        && !display_name.is_empty()
    {
        applications.push(CapturableApplication {
            bundle_id: bundle_id.to_owned(),
            display_name: display_name.to_owned(),
            icon_data_url,
            process_id,
            selector: ApplicationSelector::BundleId {
                bundle_id: bundle_id.to_owned(),
                include_descendants: false,
            },
        });
    }
}

unsafe extern "C" fn collect_audio_device(
    context: *mut c_void,
    uid: *const c_char,
    display_name: *const c_char,
    input_channels: u32,
    output_channels: u32,
    is_default_input: bool,
    is_default_output: bool,
) {
    if context.is_null() || uid.is_null() || display_name.is_null() {
        return;
    }
    // SAFETY: the native enumerator invokes this callback synchronously with a matching vector.
    let devices = unsafe { &mut *context.cast::<Vec<AudioDeviceDescriptor>>() };
    // SAFETY: the native enumerator keeps both null-terminated strings alive for the callback.
    let uid = unsafe { CStr::from_ptr(uid) };
    let display_name = unsafe { CStr::from_ptr(display_name) };
    if let (Ok(uid), Ok(display_name)) = (uid.to_str(), display_name.to_str())
        && !uid.is_empty()
        && !display_name.is_empty()
    {
        devices.push(AudioDeviceDescriptor {
            uid: uid.to_owned(),
            display_name: display_name.to_owned(),
            input_channels,
            output_channels,
            is_default_input,
            is_default_output,
        });
    }
}

#[derive(Default)]
pub(crate) struct CaptureProbeState {
    session: Mutex<Option<CaptureProbeSession>>,
}

pub(crate) struct PreparedApplicationCaptures {
    set: capture_transaction::CaptureSet<ApplicationAudioSession>,
}

#[derive(Default)]
pub(crate) struct ApplicationCaptureState {
    owners: Mutex<capture_transaction::CaptureOwners<ApplicationAudioSession>>,
    backend: MacApplicationCaptureBackend,
    pub(crate) monitor_active: AtomicBool,
}

impl ApplicationCaptureState {
    pub(crate) fn reserve(
        &self,
        manifest: &tonenari_control_protocol::ApplicationCaptureManifest,
    ) -> Result<(), String> {
        let mut owners = self
            .owners
            .lock()
            .map_err(|_| "application capture state is poisoned".to_owned())?;
        owners.ensure_reservable()?;
        let retained = owners.retained()?;
        let set = capture_transaction::prepare(
            manifest,
            owners.latest(),
            &retained,
            |id, request, previous| prepare_application_audio(&self.backend, id, request, previous),
        )?;
        owners.reserve(set)
    }

    pub(crate) fn accept_reserved(&self) -> Result<(), String> {
        let mut owners = self
            .owners
            .lock()
            .map_err(|_| "application capture state is poisoned".to_owned())?;
        let retired = owners.accept_reserved()?;
        drop(owners);
        drop(retired);
        Ok(())
    }

    pub(crate) fn reject_reserved(&self) -> Result<(), String> {
        let mut owners = self
            .owners
            .lock()
            .map_err(|_| "application capture state is poisoned".to_owned())?;
        let retired = owners.reject_reserved();
        drop(owners);
        drop(retired);
        Ok(())
    }

    pub(crate) fn prepare(
        &self,
        manifest: &tonenari_control_protocol::ApplicationCaptureManifest,
    ) -> Result<PreparedApplicationCaptures, String> {
        let owners = self
            .owners
            .lock()
            .map_err(|_| "application capture state is poisoned".to_owned())?;
        let retained = owners.retained()?;
        let set = capture_transaction::prepare(
            manifest,
            owners.latest(),
            &retained,
            |id, request, previous| prepare_application_audio(&self.backend, id, request, previous),
        )?;
        Ok(PreparedApplicationCaptures { set })
    }

    pub(crate) fn stage(&self, prepared: PreparedApplicationCaptures) -> Result<(), String> {
        let mut owners = self
            .owners
            .lock()
            .map_err(|_| "application capture state is poisoned".to_owned())?;
        let retired = owners.stage(prepared.set)?;
        drop(owners);
        drop(retired);
        Ok(())
    }

    pub(crate) fn acknowledge(&self, revision: u64) -> Result<(), String> {
        let mut owners = self
            .owners
            .lock()
            .map_err(|_| "application capture state is poisoned".to_owned())?;
        let retired = owners.acknowledge(revision, |session| {
            let reader = session
                .prepared_reader
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some(reader) = reader {
                session.pump.handoff.publish(reader);
            }
        });
        drop(owners);
        // Destruction joins native pump workers only on this blocking control thread.
        drop(retired);
        Ok(())
    }

    pub(crate) fn has_pending(&self) -> Result<bool, String> {
        self.owners
            .lock()
            .map(|owners| owners.has_unacknowledged())
            .map_err(|_| "application capture state is poisoned".to_owned())
    }

    pub(crate) fn requires_manifest(&self, submitted_revision: u64) -> Result<bool, String> {
        self.owners
            .lock()
            .map(|owners| owners.requires_manifest(submitted_revision))
            .map_err(|_| "application capture state is poisoned".to_owned())
    }
}

fn same_capture_session(
    current: &ApplicationCaptureRequest,
    requested: &ApplicationCaptureRequest,
) -> bool {
    current.selector == requested.selector
        && current.requires_reselection == requested.requires_reselection
        && current.stream == requested.stream
        && current.source_playback == requested.source_playback
}

#[cfg(test)]
mod restored_pid_tests {
    use super::*;
    #[test]
    fn restored_pid_manifest_is_rejected_without_attempting_a_native_tap() {
        let manifest = serde_json::from_value(serde_json::json!({
            "protocolVersion":1, "revision":7, "sources":[{
                "nodeId":"saved-pid", "request":{
                    "selector":{"match":"process_id","process_id":4_294_967_295_u32,"include_descendants":true},
                    "stream":{"channels":2,"channelLayout":"stereo"},"sourcePlayback":"preserve","requiresReselection":true
                }
            }]
        })).unwrap();
        let state = ApplicationCaptureState::default();
        assert!(state.prepare(&manifest).is_err());
        assert!(!state.has_pending().unwrap());
    }
}

fn prepare_application_audio(
    backend: &MacApplicationCaptureBackend,
    node_id: &tonenari_audio_graph::NodeId,
    request: &ApplicationCaptureRequest,
    previous: Option<&Arc<ApplicationAudioSession>>,
) -> Result<Arc<ApplicationAudioSession>, String> {
    if previous
        .is_some_and(|session| session.capture.resolved.stream.channels != request.stream.channels)
    {
        return Err("live application transport channel replacement is unsupported".to_owned());
    }
    let session = backend.open(request).map_err(|error| error.to_string())?;
    let stats = session.stats();
    if stats.channels != u32::from(request.stream.channels) {
        return Err("application capture channel count does not match the graph".to_owned());
    }
    clock_pump::native_rate(stats.sample_rate)?;
    let capture = Arc::new(session);
    // A replacement must reserve its sole reader before remote acceptance;
    // committing only moves an existing token and cannot fail acquisition.
    let reader = capture.acquire_reader()?;
    // A second connection to the same source would steal its producer lease
    // before HTTP acceptance. Keep the transport and its single reader alive;
    // only commit publishes the prepared tap to that pump's control mailbox.
    let (pump, prepared_reader) = if let Some(previous) = previous {
        (Arc::clone(&previous.pump), Some(reader))
    } else {
        (
            Arc::new(AudioPump::start(
                node_id.as_str(),
                reader,
                request.stream.channels,
            )?),
            None,
        )
    };
    Ok(Arc::new(ApplicationAudioSession {
        capture,
        pump,
        prepared_reader: Mutex::new(prepared_reader),
    }))
}

#[tauri::command]
pub(crate) fn list_capturable_applications() -> Result<Vec<CapturableApplication>, String> {
    let mut applications: Vec<CapturableApplication> = Vec::new();
    // SAFETY: enumeration and callbacks are synchronous and `applications` outlives the call.
    let error = unsafe {
        tn_process_tap_visit_applications(Some(collect_application), (&raw mut applications).cast())
    };
    if error.code != NATIVE_OK {
        return Err(CaptureProbeError(error).to_string());
    }
    applications.sort_unstable_by(|left, right| {
        left.bundle_id
            .cmp(&right.bundle_id)
            .then(left.process_id.cmp(&right.process_id))
    });
    applications.dedup_by(|right, left| right.bundle_id == left.bundle_id);
    applications.sort_unstable_by(|left, right| {
        left.display_name
            .cmp(&right.display_name)
            .then(left.bundle_id.cmp(&right.bundle_id))
    });
    Ok(applications)
}

#[tauri::command]
pub(crate) fn list_audio_devices() -> Result<Vec<AudioDeviceDescriptor>, String> {
    let mut devices: Vec<AudioDeviceDescriptor> = Vec::new();
    // SAFETY: enumeration and callbacks are synchronous and `devices` outlives the call.
    let error =
        unsafe { tn_audio_device_visit(Some(collect_audio_device), (&raw mut devices).cast()) };
    if error.code != NATIVE_OK {
        return Err(CaptureProbeError(error).to_string());
    }
    devices.sort_unstable_by(|left, right| {
        left.display_name
            .cmp(&right.display_name)
            .then(left.uid.cmp(&right.uid))
    });
    Ok(devices)
}

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri deserializes owned command arguments and State extractors"
)]
pub(crate) fn start_capture_probe(
    bundle_id: String,
    mute_while_captured: bool,
    state: State<'_, CaptureProbeState>,
) -> Result<CaptureProbeStats, String> {
    let mut session = state
        .session
        .lock()
        .map_err(|_| "application capture state is poisoned".to_owned())?;
    if session.is_some() {
        return Err("an application capture probe is already running".to_owned());
    }
    let capture = CaptureProbeSession::start(&bundle_id, mute_while_captured)
        .map_err(|error| error.to_string())?;
    let snapshot = capture.stats();
    *session = Some(capture);
    Ok(snapshot)
}

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri command State is an owned request extractor"
)]
pub(crate) fn capture_probe_stats(
    state: State<'_, CaptureProbeState>,
) -> Result<CaptureProbeStats, String> {
    let session = state
        .session
        .lock()
        .map_err(|_| "application capture state is poisoned".to_owned())?;
    session
        .as_ref()
        .map(CaptureProbeSession::stats)
        .ok_or_else(|| "no application capture probe is running".to_owned())
}

#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri command State is an owned request extractor"
)]
pub(crate) fn stop_capture_probe(state: State<'_, CaptureProbeState>) -> Result<(), String> {
    let mut session = state
        .session
        .lock()
        .map_err(|_| "application capture state is poisoned".to_owned())?;
    session
        .take()
        .map(drop)
        .ok_or_else(|| "no application capture probe is running".to_owned())
}
