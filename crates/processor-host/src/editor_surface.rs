//! Bounded control transport. Audio never reads this ring or waits for its publishers.
use crate::{NodeId, ProcessorControlRegistry, WORKER_READY, shared_header_unchecked};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
    time::Duration,
};
use tonenari_control_protocol::{EditorPointerKind, ProcessorEditorFrame, ProcessorEditorInput};

impl ProcessorControlRegistry {
    /// Releases an in-flight gesture before a close/state checkpoint can wait on the broker.
    /// # Errors
    /// Returns an error when the graph no longer owns this worker.
    pub fn cancel_editor_gesture(&self, node_id: &NodeId) -> Result<(), String> {
        let control = self.live_control(node_id)?;
        let header = shared_header_unchecked(&control.shared);
        header.editor_input.cancel_session.store(
            header.editor_sequence.load(Ordering::Acquire),
            Ordering::Release,
        );
        Ok(())
    }
    /// Captures the live worker's native editor without holding graph or DSP locks.
    /// # Errors
    /// Rejects unavailable/replaced/closed workers and bounded busy/timeout responses.
    pub fn editor_frame(&self, node_id: &NodeId) -> Result<ProcessorEditorFrame, String> {
        if !cfg!(target_os = "macos") {
            return Err("inline editor surfaces are unavailable on this backend".to_owned());
        }
        let control = self.live_control(node_id)?;
        let header = shared_header_unchecked(&control.shared);
        let session = header.editor_sequence.load(Ordering::Acquire);
        if !header.editor_observed_open.load(Ordering::Acquire) {
            return Err("editor is not open".to_owned());
        }
        let bytes = control
            .request_blob(EDITOR_FRAME, Duration::from_millis(150))
            .map_err(|error| error.to_string())?;
        let frame: ProcessorEditorFrame =
            serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        if !Arc::ptr_eq(&control, &self.live_control(node_id)?)
            || frame.instance_id != header.instance_id.to_string()
            || frame.session != session
            || header.editor_sequence.load(Ordering::Acquire) != session
        {
            return Err("editor session changed".to_owned());
        }
        Ok(frame)
    }

    /// Enqueues pointer input in a bounded mailbox, independent of pending state/frame reads.
    /// # Errors
    /// Rejects stale sessions, invalid coordinates, unavailable workers, or saturated input.
    pub fn editor_input(
        &self,
        node_id: &NodeId,
        input: &ProcessorEditorInput,
    ) -> Result<(), String> {
        let control = self.live_control(node_id)?;
        let _producer = control
            .input_writer
            .lock()
            .map_err(|_| "editor input lock poisoned")?;
        let header = shared_header_unchecked(&control.shared);
        if header.worker_status.load(Ordering::Acquire) != WORKER_READY
            || !header.editor_observed_open.load(Ordering::Acquire)
            || input.instance_id != header.instance_id.to_string()
            || input.session != header.editor_sequence.load(Ordering::Acquire)
        {
            return Err("editor session is unavailable".to_owned());
        }
        if !input.x.is_finite()
            || !input.y.is_finite()
            || input.modifiers > 15
            || input.x.abs() > 8192.0
            || input.y.abs() > 8192.0
        {
            return Err("invalid editor pointer input".to_owned());
        }
        header.editor_input.push(input)
    }
}

pub(super) const NATIVE_STATE: u32 = 0;
pub(super) const EDITOR_FRAME: u32 = 1;
const CAPACITY: usize = 64;

#[repr(C)]
pub(super) struct InputRing {
    head: AtomicU64,
    tail: AtomicU64,
    cancel_session: AtomicU64,
    slots: [InputSlot; CAPACITY],
}

#[repr(C)]
struct InputSlot {
    session: AtomicU64,
    kind: AtomicU32,
    x: AtomicU64,
    y: AtomicU64,
    modifiers: AtomicU32,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PointerEvent {
    pub session: u64,
    pub kind: EditorPointerKind,
    pub x: f64,
    pub y: f64,
    pub modifiers: u8,
}

impl InputRing {
    pub(super) fn cancel(&self, session: u64) {
        self.cancel_session.store(session, Ordering::Release);
    }
    pub(super) fn new() -> Self {
        Self {
            head: AtomicU64::new(0),
            tail: AtomicU64::new(0),
            cancel_session: AtomicU64::new(0),
            slots: std::array::from_fn(|_| InputSlot {
                session: AtomicU64::new(0),
                kind: AtomicU32::new(0),
                x: AtomicU64::new(0),
                y: AtomicU64::new(0),
                modifiers: AtomicU32::new(0),
            }),
        }
    }

    /// Caller serializes producers independently of the blob/state publisher.
    pub(super) fn push(&self, event: &ProcessorEditorInput) -> Result<(), String> {
        if event.kind == EditorPointerKind::Cancel {
            self.cancel_session.store(event.session, Ordering::Release);
            return Ok(());
        }
        let head = self.head.load(Ordering::Relaxed);
        let used = head.wrapping_sub(self.tail.load(Ordering::Acquire));
        if used >= CAPACITY as u64 - 2 && event.kind == EditorPointerKind::Move {
            return Ok(()); // Motion may be skipped; gesture termination never is.
        }
        if used >= CAPACITY as u64 - 1 {
            self.cancel_session.store(event.session, Ordering::Release);
            return Err("editor input queue is full; gesture cancelled".to_owned());
        }
        let slot =
            &self.slots[usize::try_from(head % CAPACITY as u64).expect("bounded ring index")];
        slot.session.store(event.session, Ordering::Relaxed);
        slot.kind.store(
            match event.kind {
                EditorPointerKind::Down => 1,
                EditorPointerKind::Move => 2,
                EditorPointerKind::Up => 3,
                EditorPointerKind::Cancel => 4,
            },
            Ordering::Relaxed,
        );
        slot.x.store(event.x.to_bits(), Ordering::Relaxed);
        slot.y.store(event.y.to_bits(), Ordering::Relaxed);
        slot.modifiers
            .store(u32::from(event.modifiers), Ordering::Relaxed);
        self.head.store(head.wrapping_add(1), Ordering::Release);
        Ok(())
    }

    pub(super) fn pop(&self) -> Option<PointerEvent> {
        let tail = self.tail.load(Ordering::Relaxed);
        if tail == self.head.load(Ordering::Acquire) {
            return None;
        }
        let slot =
            &self.slots[usize::try_from(tail % CAPACITY as u64).expect("bounded ring index")];
        let event = PointerEvent {
            session: slot.session.load(Ordering::Relaxed),
            kind: match slot.kind.load(Ordering::Relaxed) {
                1 => EditorPointerKind::Down,
                2 => EditorPointerKind::Move,
                3 => EditorPointerKind::Up,
                _ => EditorPointerKind::Cancel,
            },
            x: f64::from_bits(slot.x.load(Ordering::Relaxed)),
            y: f64::from_bits(slot.y.load(Ordering::Relaxed)),
            modifiers: u8::try_from(slot.modifiers.load(Ordering::Relaxed)).unwrap_or(0),
        };
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        Some(event)
    }

    pub(super) fn take_cancel(&self) -> u64 {
        self.cancel_session.swap(0, Ordering::AcqRel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        EDITOR_COMMAND_OPEN, EDITOR_STATE_OPEN, HostedProcessorControl, ProcessorStateError,
        create_shared_file, map_worker_regions, publish_state_response,
    };
    use tonenari_test_support::FixtureDir;

    struct Fixture {
        control: Arc<HostedProcessorControl>,
        _directory: FixtureDir,
    }
    impl Fixture {
        fn new() -> Self {
            let directory = FixtureDir::new("editor-surface").unwrap();
            let file = create_shared_file(&directory.join("audio"), 2, 2, 32).unwrap();
            let (audio, control) = map_worker_regions(&file).unwrap();
            drop(audio);
            let header = shared_header_unchecked(&control.shared);
            header.worker_status.store(WORKER_READY, Ordering::Release);
            header.editor_sequence.store(1, Ordering::Release);
            header
                .editor_command
                .store(EDITOR_COMMAND_OPEN, Ordering::Release);
            header.editor_observed_open.store(true, Ordering::Release);
            header
                .editor_state
                .store(EDITOR_STATE_OPEN, Ordering::Release);
            Self {
                control,
                _directory: directory,
            }
        }
        fn input(&self, kind: EditorPointerKind) -> ProcessorEditorInput {
            ProcessorEditorInput {
                instance_id: shared_header_unchecked(&self.control.shared)
                    .instance_id
                    .to_string(),
                session: 1,
                kind,
                x: 12.0,
                y: 20.0,
                modifiers: 0,
            }
        }
    }

    #[test]
    fn full_motion_ring_preserves_release_and_out_of_band_cancel() {
        let fixture = Fixture::new();
        let ring = &shared_header_unchecked(&fixture.control.shared).editor_input;
        ring.push(&fixture.input(EditorPointerKind::Down)).unwrap();
        for _ in 0..1000 {
            ring.push(&fixture.input(EditorPointerKind::Move)).unwrap();
        }
        ring.push(&fixture.input(EditorPointerKind::Up)).unwrap();
        ring.push(&fixture.input(EditorPointerKind::Cancel))
            .unwrap();
        assert_eq!(ring.take_cancel(), 1);
        assert_eq!(ring.take_cancel(), 0);
        let mut kinds = Vec::new();
        while let Some(event) = ring.pop() {
            kinds.push(event.kind);
        }
        assert_eq!(kinds.first(), Some(&EditorPointerKind::Down));
        assert_eq!(kinds.last(), Some(&EditorPointerKind::Up));
        assert!(kinds.len() <= CAPACITY);
    }

    #[test]
    fn blob_writer_does_not_block_mouse_up_and_stale_sessions_are_rejected() {
        let fixture = Fixture::new();
        let registry = ProcessorControlRegistry::default();
        let id = NodeId::from("effect");
        let _registration = registry.register(&id, &fixture.control);
        let _blob_writer = fixture.control.writer.lock().unwrap();
        registry
            .editor_input(&id, &fixture.input(EditorPointerKind::Up))
            .unwrap();
        let header = shared_header_unchecked(&fixture.control.shared);
        assert_eq!(
            header.editor_input.pop().unwrap().kind,
            EditorPointerKind::Up
        );
        header.editor_sequence.store(2, Ordering::Release);
        assert!(
            registry
                .editor_input(&id, &fixture.input(EditorPointerKind::Down))
                .is_err()
        );
        assert!(header.editor_input.pop().is_none());
    }

    #[test]
    fn duplicate_open_keeps_the_live_surface_session() {
        let fixture = Fixture::new();
        fixture.control.set_editor_open(true);
        assert_eq!(
            shared_header_unchecked(&fixture.control.shared)
                .editor_sequence
                .load(Ordering::Acquire),
            1
        );
    }

    #[test]
    fn retired_frame_is_acknowledged_before_a_native_state_request() {
        let fixture = Fixture::new();
        let control = &fixture.control;
        assert!(matches!(
            control.request_blob(EDITOR_FRAME, Duration::ZERO),
            Err(ProcessorStateError::Timeout)
        ));
        let header = shared_header_unchecked(&control.shared);
        let request = header.state_request.load(Ordering::Acquire);
        // Same terminal response used by the owning main-thread timer on retirement.
        publish_state_response(
            &control.shared,
            request,
            Err(ProcessorStateError::Unavailable),
        );
        assert!(matches!(
            control.snapshot_state(Duration::ZERO),
            Err(ProcessorStateError::Timeout)
        ));
        assert_eq!(header.state_request.load(Ordering::Acquire), request + 1);
        assert_eq!(header.state_kind.load(Ordering::Acquire), NATIVE_STATE);
        assert_eq!(header.editor_input.take_cancel(), 1);
    }

    #[test]
    fn native_checkpoint_cancels_tracking_before_waiting_for_a_pending_frame() {
        let fixture = Fixture::new();
        let control = &fixture.control;
        assert!(matches!(
            control.request_blob(EDITOR_FRAME, Duration::ZERO),
            Err(ProcessorStateError::Timeout)
        ));
        let worker_control = Arc::clone(control);
        let worker = std::thread::spawn(move || {
            let header = shared_header_unchecked(&worker_control.shared);
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while header.editor_input.take_cancel() == 0 {
                assert!(std::time::Instant::now() < deadline);
                std::thread::yield_now();
            }
            assert_eq!(header.state_kind.load(Ordering::Acquire), EDITOR_FRAME);
            let frame_request = header.state_request.load(Ordering::Acquire);
            publish_state_response(&worker_control.shared, frame_request, Ok(vec![1]));
            while header.state_request.load(Ordering::Acquire) == frame_request {
                assert!(std::time::Instant::now() < deadline);
                std::thread::yield_now();
            }
            assert_eq!(header.state_kind.load(Ordering::Acquire), NATIVE_STATE);
            publish_state_response(&worker_control.shared, frame_request + 1, Ok(vec![2]));
        });
        let state = control.snapshot_state(Duration::from_secs(2));
        worker.join().unwrap();
        assert_eq!(state.unwrap(), serde_json::json!({ "stateBase64": "Ag==" }));
    }

    #[test]
    fn checkpoint_deadline_preserves_the_unacknowledged_frame() {
        let fixture = Fixture::new();
        let control = &fixture.control;
        assert!(matches!(
            control.request_blob(EDITOR_FRAME, Duration::ZERO),
            Err(ProcessorStateError::Timeout)
        ));
        let header = shared_header_unchecked(&control.shared);
        let request = header.state_request.load(Ordering::Acquire);
        assert!(matches!(
            control.snapshot_state(Duration::from_millis(5)),
            Err(ProcessorStateError::Pending)
        ));
        assert_eq!(header.state_request.load(Ordering::Acquire), request);
        assert_eq!(header.state_kind.load(Ordering::Acquire), EDITOR_FRAME);
        assert_eq!(header.editor_input.take_cancel(), 1);
    }

    #[test]
    fn retired_but_live_worker_is_not_reused_by_immediate_undo() {
        let fixture = Fixture::new();
        let registry = ProcessorControlRegistry::default();
        let id = NodeId::from("effect");
        let config = tonenari_audio_graph::ProviderNodeConfig {
            type_id: "au:aufx:bpas:appl".into(),
            parameters: serde_json::json!({}),
        };
        let request = tonenari_audio_engine::HostedProcessorPrepareRequest {
            node_id: &id,
            config: &config,
            format: tonenari_audio_graph::RenderFormat {
                sample_rate: 48000,
                block_frames: 32,
            },
            input_channels: 2,
            output_channels: 2,
        };
        *fixture.control.reuse.lock().unwrap() = Some(crate::reuse::ReuseSignature::new(&request));
        let _registration = registry.register(&id, &fixture.control);
        assert!(registry.reuse_request(&request).is_none());
        fixture.control.active_owner.store(true, Ordering::Release);
        assert!(registry.reuse_request(&request).unwrap().requests_reuse());
        // Deletion publishes retirement at activation, before any destructor/reclamation.
        fixture.control.active_owner.store(false, Ordering::Release);
        assert!(registry.live_control(&id).is_ok());
        assert!(registry.reuse_request(&request).is_none());
    }
}
