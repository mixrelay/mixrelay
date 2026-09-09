//! Host slider changes must reach the processor, not only its edit controller.
//!
//! The worker serializes control and processing. All COM objects and slot storage
//! are allocated at load; successive edits before one block coalesce to the latest
//! value at offset zero. No queue allocation or lock occurs in `process`.

use std::{
    ptr,
    sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};
use truce_rack_core::error::{Error, Result};
use vst3::Steinberg::Vst::{
    IAudioProcessor, IAudioProcessorTrait, IEditController, IEditControllerTrait, IParamValueQueue,
    IParamValueQueueTrait, IParameterChanges, IParameterChangesTrait, ProcessData,
};
use vst3::Steinberg::{kInvalidArgument, kResultOk};
use vst3::{Class, ComPtr, ComWrapper};

// Matches the containing host's parameter mailbox bound. Reject an unrepresentable
// catalog explicitly rather than silently discarding edits or allocating unboundedly.
const MAX_PARAMETERS: usize = 4096;

struct ValueQueue {
    id: u32,
    value: AtomicU64,
    pending: AtomicBool,
}

impl Class for ValueQueue {
    type Interfaces = (IParamValueQueue,);
}

impl IParamValueQueueTrait for ValueQueue {
    unsafe fn getParameterId(&self) -> u32 {
        self.id
    }
    unsafe fn getPointCount(&self) -> i32 {
        i32::from(self.pending.load(Ordering::Acquire))
    }
    unsafe fn getPoint(&self, index: i32, offset: *mut i32, value: *mut f64) -> i32 {
        if index != 0
            || !self.pending.load(Ordering::Acquire)
            || offset.is_null()
            || value.is_null()
        {
            return kInvalidArgument;
        }
        // SAFETY: the caller supplies writable outputs; null pointers were rejected.
        unsafe {
            *offset = 0;
            *value = f64::from_bits(self.value.load(Ordering::Acquire));
        }
        kResultOk
    }
    unsafe fn addPoint(&self, _: i32, _: f64, _: *mut i32) -> i32 {
        // The processor receives a read-only input queue, not an output queue.
        kInvalidArgument
    }
}

struct Slot {
    queue: ComWrapper<ValueQueue>,
    pointer: ComPtr<IParamValueQueue>,
}

struct Changes {
    slots: Vec<Slot>,
    active: Vec<AtomicUsize>,
    count: AtomicUsize,
}

impl Class for Changes {
    type Interfaces = (IParameterChanges,);
}

impl IParameterChangesTrait for Changes {
    unsafe fn getParameterCount(&self) -> i32 {
        i32::try_from(self.count.load(Ordering::Acquire)).expect("bounded parameter slots")
    }
    unsafe fn getParameterData(&self, index: i32) -> *mut IParamValueQueue {
        usize::try_from(index)
            .ok()
            .filter(|index| *index < self.count.load(Ordering::Acquire))
            .map(|index| self.active[index].load(Ordering::Acquire))
            .map_or(ptr::null_mut(), |index| self.slots[index].pointer.as_ptr())
    }
    unsafe fn addParameterData(&self, _: *const u32, _: *mut i32) -> *mut IParamValueQueue {
        ptr::null_mut()
    }
}

pub(super) struct InputParameters {
    changes: ComWrapper<Changes>,
    pointer: ComPtr<IParameterChanges>,
}

impl InputParameters {
    pub(super) fn from_controller(
        controller: &ComPtr<IEditController>,
        count: usize,
    ) -> Result<Self> {
        if count > MAX_PARAMETERS {
            return Err(Error::Other(
                "VST3 parameter count exceeds host capacity".into(),
            ));
        }
        let mut slots: Vec<Slot> = Vec::with_capacity(count);
        for index in 0..count {
            let mut info = super::empty_parameter_info();
            let index = i32::try_from(index).expect("bounded parameter count");
            if unsafe { controller.getParameterInfo(index, &raw mut info) } != kResultOk {
                return Err(Error::Other(
                    "VST3 parameter catalog could not be read".into(),
                ));
            }
            if slots.iter().any(|slot| slot.queue.id == info.id) {
                return Err(Error::Other(
                    "VST3 parameter catalog contains duplicate IDs".into(),
                ));
            }
            let queue = ComWrapper::new(ValueQueue {
                id: info.id,
                value: AtomicU64::new(0.0_f64.to_bits()),
                pending: AtomicBool::new(false),
            });
            let pointer = queue
                .to_com_ptr::<IParamValueQueue>()
                .expect("ValueQueue interface");
            slots.push(Slot { queue, pointer });
        }
        let changes = ComWrapper::new(Changes {
            slots,
            active: (0..count).map(|_| AtomicUsize::new(0)).collect(),
            count: AtomicUsize::new(0),
        });
        let pointer = changes
            .to_com_ptr::<IParameterChanges>()
            .expect("Changes interface");
        Ok(Self { changes, pointer })
    }

    pub(super) fn set(
        &mut self,
        controller: &ComPtr<IEditController>,
        index: usize,
        value: f64,
    ) -> Result<()> {
        let slot = self
            .changes
            .slots
            .get(index)
            .ok_or(Error::InvalidParameter(index))?;
        if !value.is_finite() {
            return Err(Error::Other("VST3 parameter value must be finite".into()));
        }
        let value = value.clamp(0.0, 1.0);
        if unsafe { controller.setParamNormalized(slot.queue.id, value) } != kResultOk {
            return Err(Error::Other(
                "IEditController::setParamNormalized failed".into(),
            ));
        }
        slot.queue.value.store(value.to_bits(), Ordering::Release);
        if !slot.queue.pending.swap(true, Ordering::AcqRel) {
            // Control and DSP are serialized by the owning worker. Atomics also
            // keep retained COM interfaces safe to read from foreign threads; no
            // unsafe Send/Sync claim or interior borrowed references are needed.
            let count = self.changes.count.load(Ordering::Acquire);
            self.changes.active[count].store(index, Ordering::Release);
            self.changes.count.store(count + 1, Ordering::Release);
        }
        Ok(())
    }

    pub(super) fn process(
        &mut self,
        processor: &ComPtr<IAudioProcessor>,
        data: &mut ProcessData,
    ) -> i32 {
        data.inputParameterChanges = self.pointer.as_ptr();
        let status = unsafe { processor.process(ptr::from_mut(data)) };
        // Do not leave a borrowed COM pointer in caller-owned scratch storage.
        data.inputParameterChanges = ptr::null_mut();
        if status == kResultOk {
            let count = self.changes.count.load(Ordering::Acquire);
            for active in &self.changes.active[..count] {
                let index = active.load(Ordering::Acquire);
                self.changes.slots[index]
                    .queue
                    .pending
                    .store(false, Ordering::Release);
            }
            self.changes.count.store(0, Ordering::Release);
        }
        status
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(super) use tests::count_allocations;
