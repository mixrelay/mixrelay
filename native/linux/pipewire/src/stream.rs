//! Owning wrappers around the native `PipeWire` callback transport. Destruction is
//! control-thread only: native disconnect drains callbacks before freeing data.
use crate::{Connection, ConnectionHandles};
use mixrelay_audio_io::process::ProcessReport;
use std::{
    cell::Cell,
    ffi::{CString, c_char, c_void},
    marker::PhantomData,
    ptr::NonNull,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

static ACTIVE_STREAMS: AtomicUsize = AtomicUsize::new(0);
#[cfg(any(test, feature = "test-observation"))]
static NEXT_OBSERVED_OWNER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

struct StreamOwner;
impl StreamOwner {
    fn acquire() -> Self {
        ACTIVE_STREAMS.fetch_add(1, Ordering::AcqRel);
        Self
    }
}
impl Drop for StreamOwner {
    fn drop(&mut self) {
        ACTIVE_STREAMS.fetch_sub(1, Ordering::AcqRel);
    }
}

mod capture_read;
#[path = "stream/control.rs"]
mod control;
pub use control::{DescriptionChange, StreamProperties};

#[derive(Default)]
#[repr(C)]
struct NativeCounters {
    overflows: u64,
    starvations: u64,
}

/// One stream's lifetime diagnostic failures, in the C stage order.
/// Capture stage zero includes malformed ingress and ring admission failures;
/// unknown geometry increases calls with zero known frames. Ordinary dequeue-null
/// and declared empty capture chunks are not rejected writes.
#[cfg(any(test, feature = "test-observation"))]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[repr(C)]
pub struct RejectedTransfer {
    pub rejected_calls: u64,
    pub rejected_frames: u64,
    pub first_position: u64,
    pub last_position: u64,
    pub first_clock: u64,
    pub last_clock: u64,
}

/// Independent atomic observations; cursors and failures are not a transaction.
#[cfg(any(test, feature = "test-observation"))]
#[derive(Default, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[repr(C)]
pub struct NativeTransferSnapshot {
    pub read: u64,
    pub written: u64,
    pub queued: u64,
    /// Native capture write, native output read, engine capture read, engine output write.
    pub failures: [RejectedTransfer; 4],
    /// Successful engine delivery, distinct from the consumer cursor above.
    pub engine_read_frames: u64,
    /// Explicit consumer discard invocations, including empty queues.
    pub consumer_discard_calls: u64,
    /// Cursor advance inside explicit consumer discards.
    pub consumer_discard_frames: u64,
    /// Native state/format callbacks that requested a ring epoch restart.
    pub native_restart_calls: u64,
    /// Failed output release operations, separate from PCM failures and clock transitions.
    pub native_output_trigger_rejections: u64,
}

#[cfg(any(test, feature = "test-observation"))]
impl std::fmt::Debug for NativeTransferSnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeTransferSnapshot")
            .field("read", &self.read)
            .field("written", &self.written)
            .field("queued", &self.queued)
            .field("native_capture_write", &self.failures[0])
            .field("native_output_read", &self.failures[1])
            .field("engine_capture_read", &self.failures[2])
            .field("engine_output_write", &self.failures[3])
            .field("engine_read_frames", &self.engine_read_frames)
            .field("consumer_discard_calls", &self.consumer_discard_calls)
            .field("consumer_discard_frames", &self.consumer_discard_frames)
            .field("native_restart_calls", &self.native_restart_calls)
            .field(
                "native_output_trigger_rejections",
                &self.native_output_trigger_rejections,
            )
            .finish()
    }
}

/// Fixed owned-set totals. Positions/clock labels describe the first enumerated
/// stream only and can be joined to PCM tags only when `streams == 1`.
/// Lifetime counts and cursor sums wrap unsigned; compare with wrapping subtraction.
#[cfg(any(test, feature = "test-observation"))]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct TransferSnapshot {
    pub streams: u64,
    pub owner_sequence: u64,
    /// First owner's current atomic clock: ID high32, rate low32; zero unavailable.
    pub native_clock: u64,
    /// First owner's current graph frames high32, native PCM frames low32; zero unavailable.
    /// Independent of the clock and failure observations, not failure-time geometry.
    pub native_quantum: u64,
    pub counters: NativeTransferSnapshot,
}

#[cfg(any(test, feature = "test-observation"))]
/// Fixed test-owner publication size, including separately observed native clock geometry.
pub const TRANSFER_SNAPSHOT_WORDS: usize = 36;

#[cfg(any(test, feature = "test-observation"))]
impl TransferSnapshot {
    /// Folds a borrowed live stream; never changes its normal report baseline.
    pub fn include(&mut self, stream: &NativeStream) {
        if self.streams == 0 {
            self.owner_sequence = stream.allocation.observation_owner;
            // SAFETY: this borrowed owner keeps the stream live. Both getters
            // copy atomic observations and never borrow native callback IO.
            self.native_clock = unsafe { mr_pw_stream_clock(stream.allocation.pointer.as_ptr()) };
            self.native_quantum = stream.callback_quantum();
        }
        // SAFETY: the current set borrows its owning NativeStream. Its allocation
        // and native ring remain live; C reads atomics, never borrowed IO memory.
        self.include_counters(unsafe {
            mr_pw_stream_transfer_snapshot(stream.allocation.pointer.as_ptr())
        });
    }

    fn include_counters(&mut self, next: NativeTransferSnapshot) {
        if self.streams == 0 {
            self.counters = next;
        } else {
            self.counters.read = self.counters.read.wrapping_add(next.read);
            self.counters.written = self.counters.written.wrapping_add(next.written);
            self.counters.queued = self.counters.queued.wrapping_add(next.queued);
            self.counters.engine_read_frames = self
                .counters
                .engine_read_frames
                .wrapping_add(next.engine_read_frames);
            self.counters.consumer_discard_calls = self
                .counters
                .consumer_discard_calls
                .wrapping_add(next.consumer_discard_calls);
            self.counters.consumer_discard_frames = self
                .counters
                .consumer_discard_frames
                .wrapping_add(next.consumer_discard_frames);
            self.counters.native_restart_calls = self
                .counters
                .native_restart_calls
                .wrapping_add(next.native_restart_calls);
            self.counters.native_output_trigger_rejections = self
                .counters
                .native_output_trigger_rejections
                .wrapping_add(next.native_output_trigger_rejections);
            for (total, next) in self.counters.failures.iter_mut().zip(next.failures) {
                total.rejected_calls = total.rejected_calls.wrapping_add(next.rejected_calls);
                total.rejected_frames = total.rejected_frames.wrapping_add(next.rejected_frames);
            }
        }
        self.streams = self.streams.wrapping_add(1);
    }

    /// Packs only fixed integers for the test owner's control-readable atomics.
    #[must_use]
    pub fn words(self) -> [u64; TRANSFER_SNAPSHOT_WORDS] {
        std::array::from_fn(|index| match index {
            0 => self.streams,
            1 => self.counters.read,
            2 => self.counters.written,
            3 => self.counters.queued,
            28 => self.owner_sequence,
            29 => self.native_clock,
            30 => self.native_quantum,
            31 => self.counters.engine_read_frames,
            32 => self.counters.consumer_discard_calls,
            33 => self.counters.consumer_discard_frames,
            34 => self.counters.native_restart_calls,
            35 => self.counters.native_output_trigger_rejections,
            _ => {
                let failure = self.counters.failures[(index - 4) / 6];
                [
                    failure.rejected_calls,
                    failure.rejected_frames,
                    failure.first_position,
                    failure.last_position,
                    failure.first_clock,
                    failure.last_clock,
                ][(index - 4) % 6]
            }
        })
    }

    /// Reconstructs an independently loaded observation, not a coherent queue transaction.
    #[must_use]
    pub fn from_words(words: &[u64; TRANSFER_SNAPSHOT_WORDS]) -> Self {
        Self {
            streams: words[0],
            owner_sequence: words[28],
            native_clock: words[29],
            native_quantum: words[30],
            counters: NativeTransferSnapshot {
                read: words[1],
                written: words[2],
                queued: words[3],
                engine_read_frames: words[31],
                consumer_discard_calls: words[32],
                consumer_discard_frames: words[33],
                native_restart_calls: words[34],
                native_output_trigger_rejections: words[35],
                failures: std::array::from_fn(|index| {
                    let offset = 4 + index * 6;
                    RejectedTransfer {
                        rejected_calls: words[offset],
                        rejected_frames: words[offset + 1],
                        first_position: words[offset + 2],
                        last_position: words[offset + 3],
                        first_clock: words[offset + 4],
                        last_clock: words[offset + 5],
                    }
                }),
            },
        }
    }
}

unsafe extern "C" {
    fn mr_pw_stream_open_pcm(
        connection: *const ConnectionHandles<'_>,
        name: *const c_char,
        description: *const c_char,
        target: *const c_char,
        mode: u32,
        channels: u32,
        sample_rate: u32,
        block_frames: u32,
        manual_output: bool,
        sample_format: u32,
    ) -> *mut c_void;
    fn mr_pw_stream_close(stream: *mut c_void);
    fn mr_pw_stream_set_description(stream: *mut c_void, description: *const c_char) -> i32;
    fn mr_pw_stream_node_id(stream: *mut c_void) -> u32;
    fn mr_pw_stream_activate_capture(stream: *mut c_void) -> i32;
    fn mr_pw_stream_ready(stream: *mut c_void) -> i32;
    fn mr_pw_stream_failed(stream: *mut c_void) -> i32;
    fn mr_pw_stream_connected(stream: *mut c_void) -> i32;
    fn mr_pw_stream_input_activity(stream: *mut c_void) -> i32;
    fn mr_pw_stream_clock(stream: *mut c_void) -> u64;
    fn mr_pw_stream_format(stream: *mut c_void) -> u64;
    fn mr_pw_stream_native_format(stream: *mut c_void) -> u64;
    fn mr_pw_stream_supported_formats(stream: *mut c_void) -> u32;
    fn mr_pw_stream_quantum(stream: *mut c_void) -> u64;
    fn mr_pw_stream_discard_input(stream: *mut c_void);
    fn mr_pw_stream_counters(stream: *mut c_void) -> NativeCounters;
    #[cfg(any(test, feature = "test-observation"))]
    fn mr_pw_stream_transfer_snapshot(stream: *mut c_void) -> NativeTransferSnapshot;
    fn mr_pw_stream_read(stream: *mut c_void, samples: *mut f32, frames: u32) -> u32;
    fn mr_pw_stream_capture_failures(stream: *mut c_void) -> u64;
    fn mr_pw_stream_write(stream: *mut c_void, samples: *const f32, frames: u32) -> u32;
    fn mr_pw_stream_trigger_output(stream: *mut c_void) -> i32;
}

/// Decodes the fixed native PCM ABI encoding, including packed three-byte S24.
#[must_use]
pub fn native_sample_format(code: u32) -> Option<mixrelay_audio_io::NativeSampleFormat> {
    use mixrelay_audio_io::NativeSampleFormat;
    match code {
        1 => Some(NativeSampleFormat::Float32),
        2 => Some(NativeSampleFormat::Signed16),
        3 => Some(NativeSampleFormat::Signed24),
        4 => Some(NativeSampleFormat::Signed32),
        _ => None,
    }
}

fn decode_native_format(snapshot: u64) -> Option<mixrelay_audio_io::NativeStreamFormat> {
    let bytes = snapshot.to_le_bytes();
    Some(mixrelay_audio_io::NativeStreamFormat {
        sample_rate: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        channels: u16::from_le_bytes([bytes[0], bytes[1]]),
        sample_format: native_sample_format(u32::from(bytes[2]))?,
    })
}

/// Unique serialized PCM owner; drop on control after its processing callback drains.
pub struct NativeStream {
    // Retired on control before the allocation closes its native capture stream.
    input_binding: Option<crate::GraphInputBinding>,
    allocation: Arc<NativeAllocation>,
    channels: usize,
    counters: NativeCounters,
    capture_read: capture_read::CaptureRead,
    native_output: bool,
    sample_rate: u32,
    // PCM cursors/counter baselines still have exactly one movable Rust owner.
    not_sync: PhantomData<Cell<()>>,
}

struct NativeAllocation {
    pointer: NonNull<c_void>,
    // Drop body drains the stream before this connection releases core/context.
    connection: Connection,
    // Fields drop in declaration order: retain ownership until stream close
    // AND the connection's control thread/context teardown have completed.
    _owner: StreamOwner,
    #[cfg(any(test, feature = "test-observation"))]
    observation_owner: u64,
}

// SAFETY: this private token owns a pinned native allocation. Only NativeStream
// can read/write its Rust-side PCM cursor; it is not Clone or Sync. Weak property
// handles can temporarily retain the allocation but expose only loop-locked
// metadata updates, never PCM/counters. Native callbacks own the opposite cursor.
unsafe impl Send for NativeAllocation {}
// SAFETY: concurrent property access uses the native control-loop lock. PCM
// methods are exclusive NativeStream operations; other observations are atomic.
// The final strong owner closes/drains native callbacks on a control thread.
unsafe impl Sync for NativeAllocation {}

/// Stream purpose also determines whether an existing target is required.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamRole<'a> {
    /// OS playback into a published virtual sink.
    VirtualRender,
    /// Graph PCM exposed as a published virtual source.
    VirtualCapture,
    /// Capture one application's pinned `PipeWire` stream serial.
    ApplicationCapture(&'a str),
    /// Capture a pinned physical source serial.
    DeviceInput(&'a str),
    /// Play into a pinned physical sink serial.
    DeviceOutput(&'a str),
    /// Graph-only contribution into a Driver sink, delivered one native cycle later.
    /// This asynchronous boundary permits reading that same device's mixed capture.
    GraphContribution(&'a str),
    /// Independent test playback publisher without a graph-cycle callback.
    #[cfg(any(test, feature = "test-observation"))]
    AutomaticDeviceOutput(&'a str),
}

impl<'a> StreamRole<'a> {
    const fn native_output(self) -> bool {
        match self {
            Self::VirtualCapture | Self::DeviceOutput(_) | Self::GraphContribution(_) => true,
            #[cfg(any(test, feature = "test-observation"))]
            Self::AutomaticDeviceOutput(_) => true,
            Self::VirtualRender | Self::ApplicationCapture(_) | Self::DeviceInput(_) => false,
        }
    }

    const fn manual_output(self) -> bool {
        matches!(
            self,
            Self::VirtualCapture | Self::DeviceOutput(_) | Self::GraphContribution(_)
        )
    }

    // The numeric mode exists only at the C ABI, not at graph/native callers.
    const fn native(self) -> (u32, Option<&'a str>) {
        match self {
            Self::VirtualRender => (0, None),
            Self::VirtualCapture => (1, None),
            Self::ApplicationCapture(target) => (2, Some(target)),
            Self::DeviceInput(target) => (3, Some(target)),
            Self::DeviceOutput(target) => (4, Some(target)),
            Self::GraphContribution(target) => (5, Some(target)),
            #[cfg(any(test, feature = "test-observation"))]
            Self::AutomaticDeviceOutput(target) => (4, Some(target)),
        }
    }
}

/// Requested RAW PCM rate and processing block bound, independent of any DSP graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamFormat {
    /// RAW stream samples per second.
    pub sample_rate: u32,
    /// Largest serialized read/write block.
    pub block_frames: u32,
}

#[derive(Clone, Copy)]
struct StreamSetup {
    channels: u16,
    format: StreamFormat,
    sample_format: u32,
}

impl NativeStream {
    /// Includes prepared, active and retiring native streams until final teardown.
    #[must_use]
    pub fn active_owners() -> usize {
        ACTIVE_STREAMS.load(Ordering::Acquire)
    }

    /// Test-only opaque identity of the still-owned allocation, including after
    /// `PipeWire` removes its registry node on target disconnect. Never dereferenced.
    #[cfg(any(test, feature = "test-observation"))]
    #[must_use]
    pub fn allocation_id(&self) -> usize {
        self.allocation.pointer.as_ptr() as usize
    }

    /// Prepare one owned native stream off RT; the native factory copies identity strings.
    /// # Errors
    /// Returns an error for invalid C strings or failed native creation.
    pub fn open_format(
        name: &str,
        description: &str,
        role: StreamRole<'_>,
        channels: u16,
        format: StreamFormat,
    ) -> Result<Self, String> {
        Self::open(
            name,
            description,
            role,
            StreamSetup {
                channels,
                format,
                sample_format: 0,
            },
        )
    }

    /// Opens one exact native encoding tuple, with no alternative negotiation.
    /// # Errors
    /// Rejects invalid geometry, identity or failed native preparation.
    pub fn open_pcm_format(
        name: &str,
        description: &str,
        role: StreamRole<'_>,
        format: mixrelay_audio_io::NativeStreamFormat,
        block_frames: u32,
    ) -> Result<Self, String> {
        use mixrelay_audio_io::NativeSampleFormat;
        let sample_format = match format.sample_format {
            NativeSampleFormat::Float32 => 1,
            NativeSampleFormat::Signed16 => 2,
            NativeSampleFormat::Signed24 => 3,
            NativeSampleFormat::Signed32 => 4,
        };
        Self::open(
            name,
            description,
            role,
            StreamSetup {
                channels: format.channels,
                format: StreamFormat {
                    sample_rate: format.sample_rate,
                    block_frames,
                },
                sample_format,
            },
        )
    }

    fn open(
        name: &str,
        description: &str,
        role: StreamRole<'_>,
        setup: StreamSetup,
    ) -> Result<Self, String> {
        let StreamSetup {
            channels,
            format,
            sample_format,
        } = setup;
        if !(1..=64).contains(&channels) {
            return Err("invalid PipeWire channel count".into());
        }
        let name = CString::new(name).map_err(|_| "invalid PipeWire node name")?;
        let description = CString::new(description).map_err(|_| "invalid PipeWire description")?;
        let (mode, target) = role.native();
        let target = target
            .map(CString::new)
            .transpose()
            .map_err(|_| "invalid PipeWire target")?;
        let connection = Connection::new(c"mixrelay-stream").map_err(|error| error.to_string())?;
        // SAFETY: input strings remain live until copied. The connection is
        // retained beside the stream; NativeAllocation drains the child first.
        // It is unstarted, so preparation cannot race its control-loop callbacks.
        let pointer = unsafe {
            mr_pw_stream_open_pcm(
                &connection.handles(),
                name.as_ptr(),
                description.as_ptr(),
                target.as_ref().map_or(std::ptr::null(), |s| s.as_ptr()),
                mode,
                u32::from(channels),
                format.sample_rate,
                format.block_frames,
                role.manual_output(),
                sample_format,
            )
        };
        let pointer = NonNull::new(pointer).ok_or("could not open native PipeWire stream")?;
        let mut allocation = NativeAllocation {
            pointer,
            connection,
            _owner: StreamOwner::acquire(),
            #[cfg(any(test, feature = "test-observation"))]
            observation_owner: NEXT_OBSERVED_OWNER.fetch_add(1, Ordering::Relaxed),
        };
        // Put the child under its drain guard before starting: a start error
        // closes it before reclaiming the context instead of leaking either.
        allocation
            .connection
            .start()
            .map_err(|error| error.to_string())?;
        if !role.native_output() {
            prepare_capture(&mut allocation, format.sample_rate)?;
        }
        Ok(Self {
            input_binding: None,
            allocation: Arc::new(allocation),
            channels: usize::from(channels),
            counters: NativeCounters::default(),
            capture_read: capture_read::CaptureRead::default(),
            native_output: role.native_output(),
            sample_rate: format.sample_rate,
            not_sync: PhantomData,
        })
    }

    /// Non-owning metadata handle. It cannot outlive/revive this acquisition or
    /// borrow PCM; only an in-flight control operation temporarily pins storage.
    #[must_use]
    pub fn properties(&self) -> StreamProperties {
        StreamProperties::new(&self.allocation)
    }

    /// Prepare this capture owner's adapter-completion dependency off RT.
    /// Retained canonical owners keep their existing link across graph edits.
    /// # Errors
    /// Returns an error if capture negotiation times out, the capture adapter
    /// has no exported registry node, or `GraphInputs::bind` fails.
    pub fn bind_inputs(&mut self, inputs: &crate::GraphInputs) -> Result<(), String> {
        if self.native_output || self.input_binding.is_some() {
            return Ok(());
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !self.ready() {
            if std::time::Instant::now() >= deadline {
                return Err("capture format did not negotiate before dependency setup".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let node_id = self
            .properties()
            .node_id()
            .ok_or("capture adapter is not exported")?;
        let monitor = match self.channels {
            1 => "monitor_MONO",
            2 => "monitor_FL",
            _ => "monitor_AUX0",
        };
        self.input_binding = Some(
            inputs
                .bind(node_id, monitor)
                .map_err(|error| error.to_string())?,
        );
        Ok(())
    }

    /// True only after the exact opened rate/channel geometry is negotiated.
    #[must_use]
    pub fn ready(&self) -> bool {
        // SAFETY: this is an atomic observation of our live owned stream.
        let ready = unsafe { mr_pw_stream_ready(self.allocation.pointer.as_ptr()) != 0 };
        ready
            && self.negotiated_format()
                == (u64::from(self.sample_rate) << 32 | self.channels as u64)
    }

    /// Actual streaming state; paused negotiated streams are not active PCM.
    #[must_use]
    pub fn connected(&self) -> bool {
        // SAFETY: atomic state observation; no native control calls on the DSP thread.
        unsafe { mr_pw_stream_connected(self.allocation.pointer.as_ptr()) != 0 }
    }

    /// Compares actual callback clock identity and rate, not requested rates.
    #[must_use]
    pub fn shares_clock(&self, clock_id: u32, rate: u32) -> bool {
        // SAFETY: atomic value copy; never dereferences native IO memory here.
        let observed = unsafe { mr_pw_stream_clock(self.allocation.pointer.as_ptr()) };
        observed != 0 && observed == (u64::from(clock_id) << 32 | u64::from(rate))
    }

    /// Atomic negotiated rate high32/channel low32 snapshot; zero is unavailable.
    #[must_use]
    pub fn negotiated_format(&self) -> u64 {
        // SAFETY: atomic negotiated format snapshot from our owned stream.
        unsafe { mr_pw_stream_format(self.allocation.pointer.as_ptr()) }
    }

    /// Graph frames high32 and native PCM frames low32, observed independently.
    #[must_use]
    pub fn callback_quantum(&self) -> u64 {
        // SAFETY: atomic value snapshot; graph and PCM units remain distinct.
        unsafe { mr_pw_stream_quantum(self.allocation.pointer.as_ptr()) }
    }

    /// Native streaming/inactive evidence without inferring demand from PCM.
    #[must_use]
    pub fn input_activity(&self) -> mixrelay_audio_io::process::InputActivity {
        use mixrelay_audio_io::process::InputActivity;
        // SAFETY: one atomic native state read; ownership keeps the stream live.
        match unsafe { mr_pw_stream_input_activity(self.allocation.pointer.as_ptr()) } {
            1 => InputActivity::Inactive,
            2 => InputActivity::Active,
            _ => InputActivity::Unknown,
        }
    }

    /// Consumer-only discard; never rewinds the concurrent native producer.
    pub fn discard_input(&mut self) {
        // SAFETY: exclusive engine-side consumer. Native rejects output modes;
        // only the consumer cursor changes, never the concurrent producer's.
        unsafe {
            mr_pw_stream_discard_input(self.allocation.pointer.as_ptr());
        }
    }

    /// Advances this owner's diagnostic baseline without changing PCM cursors.
    #[must_use]
    pub fn take_report(&mut self) -> ProcessReport {
        // SAFETY: native only reads atomic counters from the owned live stream.
        let counters = unsafe { mr_pw_stream_counters(self.allocation.pointer.as_ptr()) };
        let report = ProcessReport {
            // The opposite (DSP-facing) side already reports through ExternalIo.
            // Include only native-side failures here to avoid double counting.
            input_starvations: if self.native_output {
                counters
                    .starvations
                    .saturating_sub(self.counters.starvations)
            } else {
                0
            },
            output_overflows: if self.native_output {
                0
            } else {
                counters.overflows.saturating_sub(self.counters.overflows)
            },
            discontinuities: 0,
            sample_faults: mixrelay_audio_io::process::SampleFaults::default(),
        };
        self.counters = counters;
        report
    }

    /// Reads complete interleaved frames; any unavailable block is silenced.
    pub fn read(&mut self, destination: &mut [f32]) -> u32 {
        read_native_capture(&self.allocation, self.channels, destination)
    }

    /// Reads capture PCM and invalidates this source after newly observed native
    /// ingress loss, even when queued valid PCM fills the entire request. This
    /// conservative fault observation does not locate the missing packet inside
    /// the queue, discard valid PCM, or advance the independent report baseline.
    /// Non-finite samples are replaced before Rust source mixing and graph state;
    /// external `PipeWire` conversion may already have happened before this boundary.
    pub fn read_capture(
        &mut self,
        destination: &mut [f32],
    ) -> mixrelay_audio_io::process::TransferOutcome {
        let allocation = &self.allocation;
        let channels = self.channels;
        self.capture_read.prepare_read(|| {
            // SAFETY: this exclusive input consumer retains the live owner.
            // Discard advances only its cursor; the native producer is untouched.
            unsafe { mr_pw_stream_discard_input(allocation.pointer.as_ptr()) };
        });
        self.capture_read.read(
            destination,
            channels,
            || {
                // SAFETY: this owner pins the stream. C reads the native input
                // failure counter atomically, without borrowing callback IO.
                unsafe { mr_pw_stream_capture_failures(allocation.pointer.as_ptr()) }
            },
            |samples| read_native_capture(allocation, channels, samples),
        )
    }

    /// Completes one graph block for a canonical capture owner. Any read keeps
    /// its queue intact for shared users; unused inputs discard OFF-period PCM.
    /// The first resumed read also fences PCM queued after this block boundary.
    /// Returns whether this canonical owner was read in the completed block.
    pub fn finish_capture_block(&mut self) -> bool {
        if self.native_output {
            return false;
        }
        let allocation = &self.allocation;
        self.capture_read.finish_block(|| {
            // SAFETY: the graph owns the only input consumer. Native discard is
            // bounded and changes only that consumer's cursor, without control IO.
            unsafe { mr_pw_stream_discard_input(allocation.pointer.as_ptr()) };
        })
    }

    /// Writes one complete interleaved block, retaining unread PCM on overflow.
    pub fn write(&mut self, source: &[f32]) -> u32 {
        let Ok(frames) = u32::try_from(source.len() / self.channels) else {
            return 0;
        };
        // SAFETY: source holds frames * channels readable samples; native copies them.
        unsafe { mr_pw_stream_write(self.allocation.pointer.as_ptr(), source.as_ptr(), frames) }
    }

    /// Releases one output after the complete native cycle, never after each DSP slice.
    /// A late release is retained as a separate native rejection count; queued PCM
    /// remains intact and the next cycle retries without stopping the session.
    pub fn trigger_output(&mut self) {
        // SAFETY: this serialized DSP publisher retains the output allocation;
        // PipeWire's RT-safe trigger only releases its scheduling dependency.
        unsafe { mr_pw_stream_trigger_output(self.allocation.pointer.as_ptr()) };
    }
}

fn read_native_capture(
    allocation: &NativeAllocation,
    channels: usize,
    destination: &mut [f32],
) -> u32 {
    let frames = destination.len() / channels;
    destination.fill(0.0);
    let Ok(frames) = u32::try_from(frames) else {
        return 0;
    };
    // SAFETY: callers retain the stream's exclusive PCM consumer. The mutable
    // slice holds at least frames * channels samples and the native copy is bounded.
    unsafe {
        mr_pw_stream_read(
            allocation.pointer.as_ptr(),
            destination.as_mut_ptr(),
            frames,
        )
    }
}

fn prepare_capture(allocation: &mut NativeAllocation, rate: u32) -> Result<(), String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    let node_id = loop {
        // SAFETY: this unique allocation pins the inactive stream and parent
        // connection; the native query takes its control-loop lock.
        let id = unsafe { mr_pw_stream_node_id(allocation.pointer.as_ptr()) };
        if id != u32::MAX {
            break id;
        }
        if std::time::Instant::now() >= deadline {
            return Err("capture adapter export timed out".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    };
    allocation
        .connection
        .prepare_capture(node_id, rate)
        .map_err(|error| error.to_string())?;
    // SAFETY: successful PortConfig readback precedes activation; this unique
    // control owner retains the native stream through the loop-locked call.
    let result = unsafe { mr_pw_stream_activate_capture(allocation.pointer.as_ptr()) };
    if result < 0 {
        return Err(format!("capture activation failed: {result}"));
    }
    Ok(())
}

impl Drop for NativeAllocation {
    fn drop(&mut self) {
        // SAFETY: final strong owner, retired off DSP. No property transaction or
        // PCM owner remains. Native close disconnects/drains before freeing data.
        unsafe {
            mr_pw_stream_close(self.pointer.as_ptr());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_format_readback_decodes_all_encodings_from_one_tuple() {
        use mixrelay_audio_io::NativeSampleFormat;
        for (code, sample_format) in [
            (1, NativeSampleFormat::Float32),
            (2, NativeSampleFormat::Signed16),
            (3, NativeSampleFormat::Signed24),
            (4, NativeSampleFormat::Signed32),
        ] {
            let tuple = (96_000_u64 << 32) | (code << 16) | 64;
            let actual = decode_native_format(tuple).unwrap();
            assert_eq!(actual.sample_rate, 96_000);
            assert_eq!(actual.channels, 64);
            assert_eq!(actual.sample_format, sample_format);
        }
        assert_eq!(decode_native_format(0), None);
        assert_eq!(
            decode_native_format((48_000_u64 << 32) | (5 << 16) | 2),
            None
        );
    }

    #[test]
    fn output_roles_share_pcm_mode_but_select_their_actual_publisher_timing() {
        let manual = StreamRole::DeviceOutput("pinned-sink");
        let automatic = StreamRole::AutomaticDeviceOutput("pinned-sink");
        assert_eq!(manual.native(), automatic.native());
        assert!(manual.native_output() && automatic.native_output());
        assert!(manual.manual_output());
        assert!(!automatic.manual_output());
        assert!(StreamRole::VirtualCapture.manual_output());
        let contribution = StreamRole::GraphContribution("driver-sink");
        assert_eq!(contribution.native(), (5, Some("driver-sink")));
        assert!(contribution.native_output() && contribution.manual_output());
        assert!(!StreamRole::DeviceInput("pinned-source").manual_output());
    }

    #[test]
    fn transfer_snapshot_abi_and_all_stage_words_match_c_layout() {
        assert_eq!(std::mem::size_of::<NativeCounters>(), 16);
        assert_eq!(std::mem::size_of::<RejectedTransfer>(), 48);
        assert_eq!(std::mem::align_of::<RejectedTransfer>(), 8);
        assert_eq!(std::mem::offset_of!(RejectedTransfer, first_clock), 32);
        assert_eq!(std::mem::size_of::<NativeTransferSnapshot>(), 256);
        assert_eq!(std::mem::align_of::<NativeTransferSnapshot>(), 8);
        assert_eq!(std::mem::offset_of!(NativeTransferSnapshot, failures), 24);
        assert_eq!(
            std::mem::offset_of!(NativeTransferSnapshot, engine_read_frames),
            216
        );
        assert_eq!(
            std::mem::offset_of!(NativeTransferSnapshot, native_restart_calls),
            240
        );
        assert_eq!(
            std::mem::offset_of!(NativeTransferSnapshot, native_output_trigger_rejections),
            248
        );
        let words = std::array::from_fn(|index| index as u64 + 1);
        let snapshot = TransferSnapshot::from_words(&words);
        assert_eq!(snapshot.words(), words);
        assert_eq!(snapshot.native_clock, words[29]);
        assert_eq!(snapshot.native_quantum, words[30]);
        assert_eq!(snapshot.counters.engine_read_frames, words[31]);
        assert_eq!(snapshot.counters.consumer_discard_calls, words[32]);
        assert_eq!(snapshot.counters.consumer_discard_frames, words[33]);
        assert_eq!(snapshot.counters.native_restart_calls, words[34]);
        assert_eq!(
            snapshot.counters.native_output_trigger_rejections,
            words[35]
        );
        for (stage, failure) in snapshot.counters.failures.iter().enumerate() {
            assert_eq!(failure.rejected_calls, words[4 + stage * 6]);
            assert_eq!(failure.last_clock, words[9 + stage * 6]);
        }
    }

    #[test]
    fn transfer_debug_labels_read_and_write_failures_without_changing_words() {
        let words = std::array::from_fn(|index| index as u64 + 1);
        let snapshot = TransferSnapshot::from_words(&words);
        let diagnostic = format!("{:?}", snapshot.counters);
        for (stage, label) in [
            "native_capture_write",
            "native_output_read",
            "engine_capture_read",
            "engine_output_write",
        ]
        .into_iter()
        .enumerate()
        {
            assert!(diagnostic.contains(&format!(
                "{label}: RejectedTransfer {{ rejected_calls: {}",
                words[4 + stage * 6]
            )));
        }
        assert!(!diagnostic.contains("failures: ["));
        assert_eq!(snapshot.words(), words);
    }

    #[test]
    fn transfer_totals_wrap_without_joining_multiple_stream_clocks() {
        let first = NativeTransferSnapshot {
            read: u64::MAX,
            engine_read_frames: u64::MAX,
            consumer_discard_calls: u64::MAX,
            consumer_discard_frames: u64::MAX,
            native_restart_calls: u64::MAX,
            native_output_trigger_rejections: u64::MAX,
            failures: [RejectedTransfer {
                rejected_calls: u64::MAX,
                rejected_frames: u64::MAX,
                first_clock: 7_u64 << 32 | 0xbb80,
                last_clock: 7_u64 << 32 | 0xbb80,
                first_position: 128,
                last_position: 256,
            }; 4],
            ..NativeTransferSnapshot::default()
        };
        let mut snapshot = TransferSnapshot::default();
        assert_eq!(snapshot.words(), [0; TRANSFER_SNAPSHOT_WORDS]);
        snapshot.include_counters(first);
        assert_eq!(snapshot.streams, 1);
        snapshot.include_counters(NativeTransferSnapshot {
            read: 2,
            engine_read_frames: 2,
            consumer_discard_calls: 2,
            consumer_discard_frames: 2,
            native_restart_calls: 2,
            native_output_trigger_rejections: 2,
            failures: [RejectedTransfer {
                rejected_calls: 2,
                rejected_frames: 4,
                first_clock: 8_u64 << 32 | 0xbb80,
                last_clock: 8_u64 << 32 | 0xbb80,
                first_position: 1,
                last_position: 10,
            }; 4],
            ..NativeTransferSnapshot::default()
        });
        assert_eq!(snapshot.streams, 2);
        assert_eq!(snapshot.counters.read, 1);
        assert_eq!(snapshot.counters.engine_read_frames, 1);
        assert_eq!(snapshot.counters.consumer_discard_calls, 1);
        assert_eq!(snapshot.counters.consumer_discard_frames, 1);
        assert_eq!(snapshot.counters.native_restart_calls, 1);
        assert_eq!(snapshot.counters.native_output_trigger_rejections, 1);
        for failure in snapshot.counters.failures {
            assert_eq!(failure.rejected_calls, 1);
            assert_eq!(failure.rejected_frames, 3);
            assert_eq!(failure.first_clock, first.failures[0].first_clock);
            assert_eq!(failure.last_position, 256);
        }
        assert_eq!(1_u64.wrapping_sub(u64::MAX), 2);
        assert_eq!(
            TransferSnapshot::default().words(),
            [0; TRANSFER_SNAPSHOT_WORDS]
        );
    }

    #[test]
    #[cfg(feature = "native-lab")]
    fn native_property_transaction_pins_stream_until_final_control_release() {
        use std::time::{Duration, Instant};

        let fixture = std::env::var("MIXRELAY_LAB_APPLICATION_FIXTURE").unwrap();
        assert!(std::path::Path::new(&fixture).is_file());
        assert_eq!(
            NativeStream::active_owners(),
            0,
            "run native tests serially"
        );
        for (role, suffix) in [
            (StreamRole::VirtualRender, "render"),
            (StreamRole::VirtualCapture, "capture"),
        ] {
            let name = format!(
                "io.mixrelay.lab-property-pin-{}-{suffix}",
                std::process::id()
            );
            let stream = NativeStream::open_format(
                &name,
                "Pinned stream",
                role,
                64,
                StreamFormat {
                    sample_rate: 48_000,
                    block_frames: 128,
                },
            )
            .unwrap();
            let properties = stream.properties();
            let weak_copy = properties.clone();
            let deadline = Instant::now() + Duration::from_secs(3);
            let node_id = loop {
                if let Some(id) = properties.node_id() {
                    break id;
                }
                assert!(Instant::now() < deadline, "native node was not published");
                std::thread::sleep(Duration::from_millis(5));
            };
            let change = properties
                .description_change("Pinned stream", "Renamed stream")
                .unwrap();
            drop(stream);
            assert_eq!(NativeStream::active_owners(), 1);
            assert_eq!(weak_copy.node_id(), Some(node_id));
            change.apply().unwrap();
            change.rollback().unwrap();
            drop(change);
            assert_eq!(NativeStream::active_owners(), 0);
            assert_eq!(properties.node_id(), None);
            assert_eq!(weak_copy.node_id(), None);
            assert!(weak_copy.description_change("old", "new").is_err());

            let deadline = Instant::now() + Duration::from_secs(3);
            loop {
                let dump = std::process::Command::new("pw-dump").output().unwrap();
                assert!(dump.status.success());
                let nodes: serde_json::Value = serde_json::from_slice(&dump.stdout).unwrap();
                let present = nodes.as_array().unwrap().iter().any(|node| {
                    node.pointer("/info/props/node.name")
                        .and_then(serde_json::Value::as_str)
                        == Some(name.as_str())
                });
                if !present {
                    break;
                }
                assert!(Instant::now() < deadline, "retired native node leaked");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    #[test]
    fn invalid_identity_fails_before_opening_a_native_stream() {
        let format = StreamFormat {
            sample_rate: 48_000,
            block_frames: 128,
        };
        for (name, description, role) in [
            ("bad\0name", "description", StreamRole::VirtualRender),
            ("name", "bad\0description", StreamRole::VirtualCapture),
            (
                "name",
                "description",
                StreamRole::DeviceInput("bad\0serial"),
            ),
        ] {
            assert!(NativeStream::open_format(name, description, role, 2, format).is_err());
        }
    }

    #[test]
    fn invalid_channel_width_fails_before_native_context_creation() {
        for channels in [0, 65, u16::MAX] {
            let result = NativeStream::open_format(
                "name",
                "description",
                StreamRole::VirtualRender,
                channels,
                StreamFormat {
                    sample_rate: 48_000,
                    block_frames: 128,
                },
            );
            assert_eq!(
                result.err().as_deref(),
                Some("invalid PipeWire channel count")
            );
        }
    }
}
