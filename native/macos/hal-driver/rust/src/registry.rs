use std::ops::Deref;
use std::os::fd::{AsRawFd, OwnedFd};
use std::sync::Mutex;
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

use crate::abi::{
    ALIVE, CAPACITY, CONFIG_ACTION, FORMAT, Geometry, HostOps, INVALID, LIST, MAX_DEVICES, NAME,
    NATIVE_ERROR, NOT_FOUND, NativeOps, OK, RUNNING, STREAM_ACTIVE, Snapshot, Timestamp,
};
use crate::device::{Attachment, Device, Pending, lock};

// The control bridge ticks every 100 ms. A host proxy ACK does not guarantee
// Perform will arrive; retain the owned attachment and retry after five seconds.
pub(crate) const CONFIG_ACK_TIMEOUT_TICKS: u32 = 50;

struct Slot {
    pointer: AtomicPtr<Device>,
    readers: AtomicUsize,
}

enum AttachmentChange {
    Queued,
    Renamed,
    Unchanged,
}

#[derive(Clone, Copy)]
pub(crate) struct IoOperation {
    pub device: u32,
    pub stream: u32,
    pub render: bool,
    pub frame: u64,
    pub samples: *mut f32,
    pub frames: u32,
}

pub(crate) struct Registry {
    pub(crate) native: NativeOps,
    host: HostOps,
    // Object generation is shared across slots. Slots are not reused until a
    // retired device has no clients and all reader pins have been released.
    generation: Mutex<u32>,
    slots: [Slot; MAX_DEVICES],
    #[cfg(target_os = "macos")]
    pub(crate) inventory: Mutex<crate::inventory::InventoryState>,
}

pub(crate) struct Pin<'a> {
    slot: &'a Slot,
    device: *mut Device,
}

impl Deref for Pin<'_> {
    type Target = Device;
    fn deref(&self) -> &Device {
        unsafe { &*self.device }
    }
}

impl Drop for Pin<'_> {
    fn drop(&mut self) {
        self.slot.readers.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Registry {
    pub(crate) fn new(native: NativeOps, host: HostOps) -> Self {
        Self {
            native,
            host,
            generation: Mutex::new(0),
            slots: std::array::from_fn(|_| Slot {
                pointer: AtomicPtr::default(),
                readers: AtomicUsize::new(0),
            }),
            #[cfg(target_os = "macos")]
            inventory: Mutex::new(crate::inventory::InventoryState::default()),
        }
    }

    fn pin_slot(&self, index: usize) -> Option<Pin<'_>> {
        let slot = &self.slots[index];
        slot.readers.fetch_add(1, Ordering::SeqCst);
        let device = slot.pointer.load(Ordering::SeqCst);
        if device.is_null() {
            slot.readers.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(Pin { slot, device })
    }

    pub(crate) fn pin(&self, object: u32) -> Option<Pin<'_>> {
        let offset = object.checked_sub(16)?;
        let index = (offset / 16) as usize % MAX_DEVICES;
        let pin = self.pin_slot(index)?;
        (object >= pin.id && object <= pin.id + 2).then_some(pin)
    }

    fn endpoint(&self, endpoint: [u8; 16]) -> Option<Pin<'_>> {
        (0..MAX_DEVICES)
            .filter_map(|index| self.pin_slot(index))
            .find(|device| device.endpoint == endpoint && device.alive.load(Ordering::Acquire))
    }

    pub(crate) fn upsert(
        &self,
        endpoint: [u8; 16],
        name: &str,
        rate: u32,
        transport: (OwnedFd, usize),
    ) -> Result<u32, i32> {
        let (descriptor, size) = transport;
        if name.is_empty()
            || name.len() >= 128
            || name.contains('\0')
            || !crate::formats::SAMPLE_RATES.contains(&rate)
        {
            return Err(INVALID);
        }
        let mut geometry = Geometry::default();
        if !unsafe { (self.native.inspect)(descriptor.as_raw_fd(), size, &raw mut geometry) }
            || !geometry.valid()
            || geometry.endpoint_id != endpoint
        {
            return Err(INVALID);
        }
        let attachment = Attachment {
            descriptor,
            size,
            geometry,
        };
        let mut generation = lock(&self.generation);
        if let Some(device) = self.endpoint(endpoint) {
            let id = device.id;
            let change = Self::queue_attachment(&device, name, attachment)?;
            drop(device);
            drop(generation);
            match change {
                AttachmentChange::Queued => self.request(id),
                AttachmentChange::Renamed => self.host.changed(id, NAME),
                AttachmentChange::Unchanged => {}
            }
            return Ok(id);
        }
        let index = self
            .slots
            .iter()
            .position(|slot| slot.pointer.load(Ordering::SeqCst).is_null())
            .ok_or(CAPACITY)?;
        let serial = generation
            .checked_mul(u32::try_from(MAX_DEVICES).map_err(|_| CAPACITY)?)
            .ok_or(CAPACITY)?;
        let id = serial
            .checked_add(u32::try_from(index).map_err(|_| CAPACITY)?)
            .and_then(|id| id.checked_mul(16))
            .and_then(|id| id.checked_add(16))
            .ok_or(CAPACITY)?;
        let next_generation = generation.checked_add(1).ok_or(CAPACITY)?;
        let runtime = unsafe {
            (self.native.create)(
                &raw const geometry,
                rate,
                attachment.descriptor.as_raw_fd(),
                size,
            )
        };
        if runtime.is_null() {
            return Err(NATIVE_ERROR);
        }
        let device = Box::new(Device::new(
            id,
            geometry,
            name.into(),
            rate,
            (self.native, runtime),
        ));
        self.slots[index]
            .pointer
            .store(Box::into_raw(device), Ordering::SeqCst);
        *generation = next_generation;
        drop(generation);
        self.host.changed(1, LIST);
        Ok(id)
    }

    fn queue_attachment(
        device: &Device,
        name: &str,
        attachment: Attachment,
    ) -> Result<AttachmentChange, i32> {
        let mut control = lock(&device.control);
        if control.pending.is_none()
            && unsafe {
                (device.native.same_transport)(
                    device.runtime,
                    attachment.descriptor.as_raw_fd(),
                    attachment.size,
                )
            }
        {
            control.attachment_failed = false;
            if control.name == name {
                return Ok(AttachmentChange::Unchanged);
            }
            control.name = name.into();
            return Ok(AttachmentChange::Renamed);
        }
        let generation = control.next_generation;
        control.next_generation = generation.checked_add(1).ok_or(CAPACITY)?;
        // Reconnection must not overwrite a DAW-owned nominal rate with stale
        // engine metadata. A pending explicit rate request is preserved too.
        let rate = control.pending.as_ref().map_or_else(
            || device.rate.load(Ordering::Acquire),
            |pending| pending.rate,
        );
        control.pending = Some(Pending {
            generation,
            requested: false,
            retry_after: 0,
            failures: 0,
            attachment: Some(attachment),
            name: name.into(),
            rate,
        });
        control.attachment_failed = false;
        Ok(AttachmentChange::Queued)
    }

    pub(crate) fn remove(&self, endpoint: [u8; 16]) -> i32 {
        let guard = lock(&self.generation);
        let Some(device) = self.endpoint(endpoint) else {
            return NOT_FOUND;
        };
        let id = device.id;
        let mut control = lock(&device.control);
        device.alive.store(false, Ordering::Release);
        control.pending = None;
        drop(control);
        drop(device);
        drop(guard);
        self.host.changed(id, ALIVE);
        self.host.changed(1, LIST);
        self.reclaim();
        OK
    }

    pub(crate) fn devices(&self) -> Vec<u32> {
        (0..MAX_DEVICES)
            .filter_map(|index| self.pin_slot(index))
            .filter(|device| device.alive.load(Ordering::Acquire))
            .map(|device| device.id)
            .collect()
    }

    pub(crate) fn lookup_uid(&self, uid: &str) -> Option<u32> {
        (0..MAX_DEVICES)
            .filter_map(|index| self.pin_slot(index))
            .find(|device| device.alive.load(Ordering::Acquire) && device.uid == uid)
            .map(|device| device.id)
    }

    pub(crate) fn snapshot(&self, object: u32) -> Option<Snapshot> {
        Some(self.pin(object)?.snapshot())
    }

    /// I/O-adjacent identity checks must not enter the metadata control mutex.
    /// The fixed-slot pin keeps the device alive until its atomic state is read.
    pub(crate) fn device_alive(&self, object: u32) -> bool {
        self.pin(object)
            .is_some_and(|device| device.id == object && device.alive.load(Ordering::Acquire))
    }

    pub(crate) fn replacement_endpoint(&self) -> Option<[u8; 16]> {
        for id in self.devices() {
            let Some(device) = self.pin(id) else { continue };
            let control = lock(&device.control);
            if control.pending.is_some() {
                continue;
            }
            if unsafe { (self.native.requires_replacement)(device.runtime) } {
                return Some(device.endpoint);
            }
        }
        None
    }

    pub(crate) fn attachment_failed(&self, endpoint: [u8; 16]) -> bool {
        self.endpoint(endpoint)
            .is_some_and(|device| lock(&device.control).attachment_failed)
    }

    pub(crate) fn client(&self, id: u32, client: u32, add: bool) -> i32 {
        let Some(device) = self.pin(id).filter(|device| device.id == id) else {
            return NOT_FOUND;
        };
        let mut control = lock(&device.control);
        let mut stopped = false;
        if add {
            if !device.alive.load(Ordering::Acquire) {
                return NOT_FOUND;
            }
            if control.clients.contains_key(&client) {
                return INVALID;
            }
            control.clients.insert(client, false);
        } else {
            match control.clients.get(&client) {
                Some(false) => {
                    control.clients.remove(&client);
                }
                Some(true) => {
                    // A client process can disappear without a balanced StopIO.
                    // Its removal releases only that client's runtime claim.
                    let frozen = device.freeze();
                    let count = device.running.load(Ordering::Acquire) - 1;
                    if count == 0 {
                        unsafe { (self.native.stop)(device.runtime) };
                    } else {
                        unsafe { (self.native.remaining_client)(device.runtime) };
                    }
                    device.running.store(count, Ordering::Release);
                    control.clients.remove(&client);
                    stopped = count == 0;
                    drop(frozen);
                }
                None => return INVALID,
            }
        }
        drop(control);
        drop(device);
        if stopped {
            self.host.changed(id, RUNNING);
        }
        OK
    }

    pub(crate) fn start(&self, id: u32, client: u32, now: u64) -> i32 {
        let Some(device) = self.pin(id).filter(|device| device.id == id) else {
            return NOT_FOUND;
        };
        let mut control = lock(&device.control);
        if !device.alive.load(Ordering::Acquire) {
            return NOT_FOUND;
        }
        let Some(running) = control.clients.get_mut(&client) else {
            return INVALID;
        };
        if *running {
            return INVALID;
        }
        let count = device.running.load(Ordering::Acquire);
        if count == u32::MAX {
            return CAPACITY;
        }
        let frozen = device.freeze();
        if count == 0 && !unsafe { (self.native.start)(device.runtime, now) } {
            return NATIVE_ERROR;
        }
        *running = true;
        device.running.store(count + 1, Ordering::Release);
        drop(frozen);
        drop(control);
        drop(device);
        if count == 0 {
            self.host.changed(id, RUNNING);
        }
        OK
    }

    pub(crate) fn stop(&self, id: u32, client: u32) -> i32 {
        let Some(device) = self.pin(id).filter(|device| device.id == id) else {
            return NOT_FOUND;
        };
        let mut control = lock(&device.control);
        let Some(running) = control.clients.get_mut(&client) else {
            return INVALID;
        };
        if !*running {
            return INVALID;
        }
        let frozen = device.freeze();
        let count = device.running.load(Ordering::Acquire) - 1;
        if count == 0 {
            unsafe { (self.native.stop)(device.runtime) };
        } else {
            unsafe { (self.native.remaining_client)(device.runtime) };
        }
        *running = false;
        device.running.store(count, Ordering::Release);
        drop(frozen);
        drop(control);
        drop(device);
        if count == 0 {
            self.host.changed(id, RUNNING);
        }
        OK
    }

    pub(crate) fn set_rate(&self, id: u32, rate: u32) -> i32 {
        if !crate::formats::SAMPLE_RATES.contains(&rate) {
            return INVALID;
        }
        let Some(device) = self.pin(id).filter(|device| device.id == id) else {
            return NOT_FOUND;
        };
        if !device.alive.load(Ordering::Acquire) {
            return NOT_FOUND;
        }
        let mut control = lock(&device.control);
        if control.pending.is_none() && device.rate.load(Ordering::Acquire) == rate {
            return OK;
        }
        let generation = control.next_generation;
        let Some(next) = generation.checked_add(1) else {
            return CAPACITY;
        };
        control.next_generation = next;
        let (attachment, name) = control.pending.take().map_or_else(
            || (None, control.name.clone()),
            |pending| (pending.attachment, pending.name),
        );
        control.pending = Some(Pending {
            generation,
            requested: false,
            retry_after: 0,
            failures: 0,
            attachment,
            name,
            rate,
        });
        drop(control);
        drop(device);
        self.request(id);
        OK
    }

    pub(crate) fn set_stream_active(&self, stream: u32, active: bool) -> i32 {
        let Some(device) = self.pin(stream) else {
            return NOT_FOUND;
        };
        if !device.alive.load(Ordering::Acquire) {
            return NOT_FOUND;
        }
        if stream == device.id + 1 {
            device.input_active.store(active, Ordering::Release);
        } else if stream == device.id + 2 {
            device.output_active.store(active, Ordering::Release);
        } else {
            return INVALID;
        }
        drop(device);
        self.host.changed(stream, STREAM_ACTIVE);
        OK
    }

    fn request(&self, id: u32) {
        let Some(device) = self.pin(id) else { return };
        let mut control = lock(&device.control);
        let Some(pending) = control.pending.as_mut() else {
            return;
        };
        if pending.requested || pending.retry_after > 0 {
            return;
        }
        pending.requested = true;
        pending.retry_after = CONFIG_ACK_TIMEOUT_TICKS;
        let generation = pending.generation;
        drop(control);
        drop(device);
        if self.host.request(id, generation) {
            return;
        }
        let Some(device) = self.pin(id) else { return };
        let mut control = lock(&device.control);
        if let Some(pending) = control
            .pending
            .as_mut()
            .filter(|pending| pending.generation == generation)
        {
            pending.requested = false;
            pending.failures = (pending.failures + 1).min(6);
            pending.retry_after = 1 << pending.failures;
        }
    }

    pub(crate) fn tick(&self) {
        for id in self.devices() {
            if let Some(device) = self.pin(id)
                && let Some(pending) = lock(&device.control).pending.as_mut()
            {
                pending.retry_after = pending.retry_after.saturating_sub(1);
                if pending.retry_after == 0 {
                    pending.requested = false;
                }
            }
            self.request(id);
        }
        self.reclaim();
    }

    pub(crate) fn perform(&self, id: u32, action: u64, generation: u64) -> i32 {
        if action != CONFIG_ACTION {
            return INVALID;
        }
        let Some(device) = self.pin(id).filter(|device| device.id == id) else {
            return NOT_FOUND;
        };
        let mut control = lock(&device.control);
        if !device.alive.load(Ordering::Acquire) {
            return NOT_FOUND;
        }
        if control
            .pending
            .as_ref()
            .is_none_or(|pending| pending.generation != generation)
        {
            return OK;
        }
        let pending = control.pending.take().expect("matched pending generation");
        let frozen = device.freeze();
        let success = if let Some(attachment) = &pending.attachment {
            unsafe {
                (self.native.replace)(
                    device.runtime,
                    &raw const attachment.geometry,
                    pending.rate,
                    attachment.descriptor.as_raw_fd(),
                    attachment.size,
                )
            }
        } else {
            unsafe { (self.native.set_rate)(device.runtime, pending.rate) }
        };
        if !success {
            control.attachment_failed = pending.attachment.is_some();
            return NATIVE_ERROR;
        }
        if let Some(attachment) = &pending.attachment {
            device
                .render_channels
                .store(attachment.geometry.render_channels, Ordering::Release);
            device
                .capture_channels
                .store(attachment.geometry.capture_channels, Ordering::Release);
        }
        device.rate.store(pending.rate, Ordering::Release);
        let renamed = control.name != pending.name;
        control.name = pending.name;
        drop(frozen);
        drop(control);
        drop(device);
        self.host.changed(id, FORMAT);
        if renamed {
            self.host.changed(id, NAME);
        }
        OK
    }

    pub(crate) fn abort(&self, id: u32, action: u64, generation: u64) -> i32 {
        if action != CONFIG_ACTION {
            return INVALID;
        }
        let Some(device) = self.pin(id).filter(|device| device.id == id) else {
            return NOT_FOUND;
        };
        let mut control = lock(&device.control);
        if control
            .pending
            .as_ref()
            .is_some_and(|pending| pending.generation == generation)
        {
            control.attachment_failed = control
                .pending
                .take()
                .is_some_and(|pending| pending.attachment.is_some());
        }
        OK
    }

    fn reclaim(&self) {
        let _guard = lock(&self.generation);
        for slot in &self.slots {
            let pointer = slot.pointer.load(Ordering::SeqCst);
            if pointer.is_null() {
                continue;
            }
            let device = unsafe { &*pointer };
            if device.alive.load(Ordering::Acquire) || device.running.load(Ordering::Acquire) != 0 {
                continue;
            }
            if !lock(&device.control).clients.is_empty() {
                continue;
            }
            // First unpublish the pointer, then inspect readers. New readers can
            // only observe null. If old readers remain, defer rather than free.
            slot.pointer.store(std::ptr::null_mut(), Ordering::SeqCst);
            while slot.readers.load(Ordering::SeqCst) != 0 {
                std::thread::yield_now();
            }
            drop(unsafe { Box::from_raw(pointer) });
        }
    }

    pub(crate) fn timestamp(&self, id: u32, now: u64, output: &mut Timestamp) -> bool {
        let Some(device) = self.pin(id).filter(|device| device.id == id) else {
            return false;
        };
        let Some(_io) = device.io_pin() else {
            return false;
        };
        device.alive.load(Ordering::Acquire)
            && unsafe { (self.native.timestamp)(device.runtime, now, output) }
    }

    pub(crate) unsafe fn io(&self, operation: IoOperation) -> bool {
        let IoOperation {
            device: id,
            stream,
            render,
            frame,
            samples,
            frames,
        } = operation;
        let Some(device) = self.pin(id).filter(|device| device.id == id) else {
            return false;
        };
        let expected = device.id + if render { 2 } else { 1 };
        if stream != expected {
            return false;
        }
        let Some(_io) = device.io_pin() else {
            return false;
        };
        let channels = if render {
            device.render_channels.load(Ordering::Acquire)
        } else {
            device.capture_channels.load(Ordering::Acquire)
        };
        let active = if render {
            &device.output_active
        } else {
            &device.input_active
        };
        if !device.alive.load(Ordering::Acquire)
            || !active.load(Ordering::Acquire)
            || device.running.load(Ordering::Acquire) == 0
        {
            if !render {
                unsafe { std::ptr::write_bytes(samples, 0, frames as usize * channels as usize) };
            }
            return true;
        }
        unsafe { (self.native.io)(device.runtime, render, frame, samples, frames) }
    }
}

impl Drop for Registry {
    fn drop(&mut self) {
        for slot in &mut self.slots {
            let pointer = *slot.pointer.get_mut();
            if !pointer.is_null() {
                drop(unsafe { Box::from_raw(pointer) });
            }
        }
    }
}
