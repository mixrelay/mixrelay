//! Whole-inventory publication: control owns immutable pins, readers select an exact ID.
use crate::endpoint_registry::{
    CallbackEndpoint, EndpointId, EndpointRegistry, EndpointSpec, OwnedInventory, PreparedInventory,
};
use parking_lot::Mutex;
use std::{
    collections::BTreeMap,
    io,
    pin::Pin,
    ptr,
    sync::atomic::{AtomicPtr, AtomicU8, AtomicUsize, Ordering},
};
use tonenari_macos_transport::{
    endpoint_transport::tn_endpoint_transport_requires_replacement,
    shared_audio_buffer::{
        tn_shared_audio_buffer_requires_replacement, tn_shared_audio_buffer_try_begin_epoch,
    },
};

const IDLE: u8 = 0;
const GRAPH_PREPARED: u8 = 1;
const RECOVERY_PREPARED: u8 = 2;
const ACTIVATING: u8 = 3;
const ACTIVATED: u8 = 4;
const MAX_PINS: usize = 64;
struct Inventory<'a> {
    endpoints: BTreeMap<EndpointId, CallbackEndpoint<'a>>,
}
struct Control<'a> {
    pending: Option<PreparedInventory<'a>>,
    pins: Vec<Pin<Box<Inventory<'a>>>>,
}
pub(super) struct Runtime<'a> {
    registry: &'a EndpointRegistry,
    control: Mutex<Control<'a>>,
    active: AtomicPtr<Inventory<'a>>,
    prepared: AtomicPtr<Inventory<'a>>,
    readers: AtomicUsize,
    phase: AtomicU8,
}
impl<'a> Runtime<'a> {
    pub(super) fn new(registry: &'a EndpointRegistry) -> Self {
        let empty = Box::pin(Inventory {
            endpoints: BTreeMap::new(),
        });
        let pointer = ptr::from_ref(empty.as_ref().get_ref()).cast_mut();
        Self {
            registry,
            control: Mutex::new(Control {
                pending: None,
                pins: vec![empty],
            }),
            active: AtomicPtr::new(pointer),
            prepared: AtomicPtr::new(ptr::null_mut()),
            readers: AtomicUsize::new(0),
            phase: AtomicU8::new(IDLE),
        }
    }
    pub(super) fn prepare_inventory(&self, specs: &[EndpointSpec]) -> io::Result<()> {
        let mut control = self.control.lock();
        self.require_idle()?;
        self.collect(&mut control);
        self.stage(&mut control, specs, None)
    }
    fn stage(
        &self,
        control: &mut Control<'a>,
        specs: &[EndpointSpec],
        replacement: Option<EndpointId>,
    ) -> io::Result<()> {
        if control.pins.len() >= MAX_PINS {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "endpoint readers must drain before replacement",
            ));
        }
        let pending = self.registry.prepare_inventory_change(specs, replacement)?;
        let pins = pending
            .callback_pins()
            .into_iter()
            .map(|pin| (pin.spec().id, pin))
            .collect();
        let inventory = Box::pin(Inventory { endpoints: pins });
        let pointer = ptr::from_ref(inventory.as_ref().get_ref()).cast_mut();
        control.pins.push(inventory);
        control.pending = Some(pending);
        self.prepared.store(pointer, Ordering::Relaxed);
        self.phase.store(
            if replacement.is_some() {
                RECOVERY_PREPARED
            } else {
                GRAPH_PREPARED
            },
            Ordering::Release,
        );
        Ok(())
    }
    /// Sole audio-boundary publisher: bounded atomics, no allocation or reclamation.
    pub(super) fn activate(&self) -> bool {
        self.activate_kind(GRAPH_PREPARED)
    }
    /// Recovery never publishes a graph's provisional native inventory.
    pub(super) fn activate_recovery(&self) -> bool {
        self.activate_kind(RECOVERY_PREPARED)
    }
    fn activate_kind(&self, expected: u8) -> bool {
        match self
            .phase
            .compare_exchange(expected, ACTIVATING, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => {
                self.active
                    .store(self.prepared.load(Ordering::Relaxed), Ordering::SeqCst);
                self.phase.store(ACTIVATED, Ordering::Release);
                true
            }
            Err(_) => false,
        }
    }
    pub(super) fn cancel(&self) {
        let mut control = self.control.lock();
        if self
            .phase
            .compare_exchange(GRAPH_PREPARED, IDLE, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            let unpublished = self.prepared.swap(ptr::null_mut(), Ordering::Relaxed);
            drop(control.pending.take());
            control
                .pins
                .retain(|pin| !ptr::eq(pin.as_ref().get_ref(), unpublished));
            self.collect(&mut control);
        }
    }
    pub(super) fn tick(&self) -> io::Result<bool> {
        let mut control = self.control.lock();
        let changed = self.phase.load(Ordering::Acquire) == ACTIVATED;
        if changed {
            control
                .pending
                .take()
                .ok_or_else(|| io::Error::other("missing acknowledged inventory transaction"))?
                .commit()?;
            self.prepared.store(ptr::null_mut(), Ordering::Relaxed);
            self.phase.store(IDLE, Ordering::Release);
        }
        if self.phase.load(Ordering::Acquire) == IDLE {
            self.collect(&mut control);
        }
        Ok(changed)
    }
    fn require_idle(&self) -> io::Result<()> {
        if self.phase.load(Ordering::Acquire) == IDLE {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "endpoint inventory transaction is pending",
            ))
        }
    }
    fn collect(&self, control: &mut Control<'a>) {
        // SeqCst reader increment/publication excludes all older-pointer readers before
        // reclaim. Once zero is observed, new readers can only select the active inventory.
        if self.readers.load(Ordering::SeqCst) == 0 {
            let active = self.active.load(Ordering::SeqCst);
            control
                .pins
                .retain(|pin| ptr::eq(pin.as_ref().get_ref(), active));
            self.registry.collect();
        }
    }
    pub(super) fn read(&self) -> Read<'_, 'a> {
        self.readers.fetch_add(1, Ordering::SeqCst);
        Read {
            runtime: self,
            pointer: self.active.load(Ordering::SeqCst),
        }
    }
    pub(super) fn snapshot(&self) -> io::Result<OwnedInventory> {
        self.registry.snapshot()
    }
    pub(super) fn replace(&self, id: EndpointId, generation: u64) -> io::Result<bool> {
        let mut control = self.control.lock();
        self.require_idle()?;
        self.collect(&mut control);
        let read = self.read();
        if read
            .endpoint(id)
            .is_none_or(|current| current.generation() != generation)
        {
            return Ok(false);
        }
        self.stage(&mut control, &read.specs(), Some(id))?;
        Ok(true)
    }
    /// Control-only: caller has drained the capture producer before epoch acknowledgment.
    pub(super) fn recover_capture(&self, id: EndpointId) -> io::Result<bool> {
        let read = self.read();
        let current = read
            .endpoint(id)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "endpoint absent"))?;
        // SAFETY: this read guard pins the mappings and the caller drained the sole writer.
        let exhausted = unsafe {
            tn_endpoint_transport_requires_replacement(current.transport())
                || tn_shared_audio_buffer_requires_replacement(current.capture_buffer())
        };
        if exhausted {
            return self.replace(id, current.generation());
        }
        // SAFETY: the same producer-drained mapping is pinned until this guard drops.
        Ok(unsafe { tn_shared_audio_buffer_try_begin_epoch(current.capture_buffer()) } != 0)
    }
}
pub(super) struct Read<'guard, 'registry> {
    runtime: &'guard Runtime<'registry>,
    pointer: *const Inventory<'registry>,
}
impl<'a> Read<'_, 'a> {
    pub(super) fn endpoints(&self) -> impl Iterator<Item = &CallbackEndpoint<'a>> {
        // SAFETY: this guard retains the immutable pinned inventory until iteration ends.
        unsafe { self.pointer.as_ref() }
            .into_iter()
            .flat_map(|inventory| inventory.endpoints.values())
    }
    pub(super) fn endpoint(&self, id: EndpointId) -> Option<&CallbackEndpoint<'a>> {
        // SAFETY: Runtime begins with a pinned empty inventory. The guard's reader count
        // excludes reclamation; the returned borrow cannot outlive the guard.
        unsafe { self.pointer.as_ref() }?.endpoints.get(&id)
    }
    pub(super) fn specs(&self) -> Vec<EndpointSpec> {
        // SAFETY: same guard-pinned immutable inventory; this allocation is control-only.
        unsafe { self.pointer.as_ref() }.map_or_else(Vec::new, |inventory| {
            inventory
                .endpoints
                .values()
                .map(|pin| pin.spec().clone())
                .collect()
        })
    }
}
impl Drop for Read<'_, '_> {
    fn drop(&mut self) {
        self.runtime.readers.fetch_sub(1, Ordering::SeqCst);
    }
}
