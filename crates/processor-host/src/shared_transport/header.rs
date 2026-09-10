use crate::{
    AudioLayout, EDITOR_COMMAND_NONE, EDITOR_FAILURE_NONE, EDITOR_STATE_UNAVAILABLE,
    PARAMETER_CAPACITY, SHARED_MAGIC, SHARED_VERSION, STATE_OK, WORKER_STARTING, editor_surface,
};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
static NEXT_CONTROL_INSTANCE: AtomicU64 = AtomicU64::new(1);
/// Header shared by the engine and one VST worker process.
///
/// The PCM regions following the state payload are protected by the sequence
/// atomics: the producer never overwrites a region until the consumer publishes the matching
/// consumed sequence. This keeps the engine side lock-free without reading memory while the
/// worker writes it.
#[repr(C, align(64))]
pub(crate) struct SharedHeader {
    pub(super) magic: u64,
    pub(super) version: u32,
    pub(super) input_channels: u32,
    pub(super) output_channels: u32,
    pub(super) block_frames: u32,
    pub(crate) worker_status: AtomicU32,
    pub(crate) shutdown: AtomicU32,
    pub(super) input_sequence: AtomicU64,
    pub(super) input_consumed: AtomicU64,
    pub(super) output_sequence: AtomicU64,
    pub(super) output_consumed: AtomicU64,
    pub(crate) process_failures: AtomicU64,
    pub(crate) parameter_count: AtomicU32,
    pub(crate) parameters_pending: AtomicBool,
    pub(crate) parameters: [ParameterSlot; PARAMETER_CAPACITY],
    pub(super) state_request: AtomicU64,
    pub(super) state_kind: AtomicU32,
    pub(super) state_response: AtomicU64,
    pub(super) state_length: AtomicU32,
    pub(super) state_result: AtomicU32,
    pub(crate) editor_command: AtomicU32,
    pub(crate) editor_sequence: AtomicU64,
    pub(crate) editor_consumed: AtomicU64,
    pub(crate) editor_state: AtomicU32,
    pub(crate) editor_failure: AtomicU32,
    pub(crate) editor_width: AtomicU32,
    pub(crate) editor_height: AtomicU32,
    pub(crate) editor_resizable: AtomicU32,
    pub(crate) instance_id: u64,
    /// Broker-owned lifecycle, separate from the parent's optimistic Opening status.
    pub(crate) editor_observed_open: AtomicBool,
    pub(crate) editor_close_generation: AtomicU64,
    pub(crate) editor_input: editor_surface::InputRing,
}

/// Slots are assigned once per worker lifetime by serialized control publishers. Stable IDs
/// let the consumer load each atomic value independently, without a torn ID/value pair.
#[repr(C)]
pub(crate) struct ParameterSlot {
    pub(crate) id: AtomicU32,
    pub(crate) value_bits: AtomicU64,
    pub(crate) pending: AtomicBool,
}

impl ParameterSlot {
    fn empty() -> Self {
        Self {
            id: AtomicU32::new(0),
            value_bits: AtomicU64::new(0),
            pending: AtomicBool::new(false),
        }
    }
}

impl SharedHeader {
    pub(super) fn new(layout: AudioLayout) -> Self {
        Self {
            magic: SHARED_MAGIC,
            version: SHARED_VERSION,
            input_channels: u32::try_from(layout.inputs).expect("creation validated channels"),
            output_channels: u32::try_from(layout.outputs).expect("creation validated channels"),
            block_frames: u32::try_from(layout.block_frames).expect("creation validated frames"),
            worker_status: AtomicU32::new(WORKER_STARTING),
            shutdown: AtomicU32::new(0),
            input_sequence: AtomicU64::new(0),
            input_consumed: AtomicU64::new(0),
            output_sequence: AtomicU64::new(0),
            output_consumed: AtomicU64::new(0),
            process_failures: AtomicU64::new(0),
            parameter_count: AtomicU32::new(0),
            parameters_pending: AtomicBool::new(false),
            parameters: std::array::from_fn(|_| ParameterSlot::empty()),
            state_request: AtomicU64::new(0),
            state_kind: AtomicU32::new(editor_surface::NATIVE_STATE),
            state_response: AtomicU64::new(0),
            state_length: AtomicU32::new(0),
            state_result: AtomicU32::new(STATE_OK),
            editor_command: AtomicU32::new(EDITOR_COMMAND_NONE),
            editor_sequence: AtomicU64::new(0),
            editor_consumed: AtomicU64::new(0),
            editor_state: AtomicU32::new(EDITOR_STATE_UNAVAILABLE),
            editor_failure: AtomicU32::new(EDITOR_FAILURE_NONE),
            editor_width: AtomicU32::new(0),
            editor_height: AtomicU32::new(0),
            editor_resizable: AtomicU32::new(0),
            instance_id: NEXT_CONTROL_INSTANCE.fetch_add(1, Ordering::Relaxed),
            editor_observed_open: AtomicBool::new(false),
            editor_close_generation: AtomicU64::new(0),
            editor_input: editor_surface::InputRing::new(),
        }
    }
}
