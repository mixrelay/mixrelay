//! Host-approved format and mapping transactions with bounded retry state.
use super::{CONFIG_ACK_TIMEOUT_TICKS, Registry};
use crate::{
    abi::{CAPACITY, CONFIG_ACTION, FORMAT, INVALID, NAME, NATIVE_ERROR, NOT_FOUND, OK},
    device::{Control, ConversionScratch, Device, Pending, Prepared, lock},
};
use mixrelay_pcm_ring::SampleFormat;
use std::{os::fd::AsRawFd, sync::atomic::Ordering};

impl Registry {
    pub(crate) fn set_rate(&self, id: u32, rate: u32) -> i32 {
        self.queue_configuration(id, rate, None)
    }

    pub(crate) fn set_format(&self, stream: u32, rate: u32, format: SampleFormat) -> i32 {
        let Some(device) = self.pin(stream) else {
            return NOT_FOUND;
        };
        let index = if stream == device.id + 1
            && device.capture_channels.load(Ordering::Acquire) != 0
        {
            0
        } else if stream == device.id + 2 && device.render_channels.load(Ordering::Acquire) != 0 {
            1
        } else {
            return INVALID;
        };
        self.queue_configuration(device.id, rate, Some((index, format)))
    }

    fn queue_configuration(
        &self,
        id: u32,
        rate: u32,
        requested_format: Option<(usize, SampleFormat)>,
    ) -> i32 {
        if !crate::formats::SAMPLE_RATES.contains(&rate) {
            return INVALID;
        }
        let Some(device) = self.pin(id).filter(|device| device.id == id) else {
            return NOT_FOUND;
        };
        let mut control = lock(&device.control);
        if !device.alive.load(Ordering::Acquire) {
            return NOT_FOUND;
        }
        let current_formats = [
            device.capture_format.format(),
            device.render_format.format(),
        ];
        if control.pending.is_none()
            && device.rate.load(Ordering::Acquire) == rate
            && requested_format.is_none_or(|(index, format)| current_formats[index] == format)
        {
            return OK;
        }
        let generation = control.next_generation;
        let Some(next) = generation.checked_add(1) else {
            return CAPACITY;
        };
        control.next_generation = next;
        let (attachment, name, mut formats) = control.pending.take().map_or_else(
            || (None, control.name.clone(), current_formats),
            |pending| (pending.attachment, pending.name, pending.formats),
        );
        if let Some((index, format)) = requested_format {
            formats[index] = format;
        }
        control.pending = Some(Pending {
            generation,
            requested: false,
            retry_after: 0,
            failures: 0,
            attachment,
            name,
            rate,
            formats,
        });
        drop(control);
        drop(device);
        self.request(id);
        OK
    }

    pub(super) fn request(&self, id: u32) {
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
            self.notify_latency(id);
        }
        self.reclaim();
    }

    /// Maps a pending attachment and prepares queues for every registered client, before
    /// coreaudiod is asked to perform it. Control fixes the client set; a later
    /// membership change prepares again, so Perform only swaps under the IO freeze.
    pub(super) fn prepare_attachment(device: &Device, control: &mut Control) {
        let Some(attachment) = control
            .pending
            .as_mut()
            .and_then(|pending| pending.attachment.as_mut())
        else {
            return;
        };
        // Release the previous preparation before mapping its replacement.
        attachment.prepared = None;
        if !device.alive.load(Ordering::Acquire) {
            return;
        }
        // SAFETY: held device control retains the live runtime and its client set; the
        // attachment owns its descriptor. Preparation runs alongside IO callbacks.
        let change = unsafe {
            (device.native.prepare)(
                device.runtime.load(Ordering::Acquire),
                &raw const attachment.geometry,
                attachment.descriptor.as_raw_fd(),
                attachment.size,
            )
        };
        attachment.prepared = Prepared::new(change, device.native);
    }

    pub(crate) fn perform(&self, id: u32, action: u64, generation: u64) -> i32 {
        self.perform_with_scratch(id, action, generation, Device::prepare_scratch)
    }

    pub(crate) fn perform_with_scratch(
        &self,
        id: u32,
        action: u64,
        generation: u64,
        prepare: impl FnOnce(
            &Device,
            &Pending,
        ) -> Result<ConversionScratch, std::collections::TryReserveError>,
    ) -> i32 {
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
        let Some(mut pending) = control.pending.take() else {
            return OK;
        };
        // Allocate bounded directional storage on control, before draining callbacks.
        let Ok(scratch) = prepare(&device, &pending) else {
            control.attachment_failed = pending.attachment.is_some();
            return CAPACITY;
        };
        let frozen = device.freeze();
        let success = if let Some(attachment) = &mut pending.attachment {
            attachment.prepared.take().is_some_and(|prepared| {
                // SAFETY: control and freeze retain the live runtime; replace consumes
                // the preparation made for exactly this attachment and client set.
                unsafe {
                    (self.native.replace)(
                        device.runtime.load(Ordering::Acquire),
                        prepared.into_raw(),
                        pending.rate,
                    )
                }
            })
        } else {
            device.rate.load(Ordering::Acquire) == pending.rate
                // SAFETY: control and freeze exclude callbacks during this rate transition.
                || unsafe {
                    (self.native.set_rate)(device.runtime.load(Ordering::Acquire), pending.rate)
                }
        };
        if !success {
            control.attachment_failed = pending.attachment.is_some();
            return NATIVE_ERROR;
        }
        // SAFETY: control and freeze are held, all IO pins are drained, and the native
        // rate/mapping transition succeeded. Abort/failure never publishes these buffers.
        unsafe {
            device.publish_configuration(&pending, scratch);
        }
        let renamed = control.name != pending.name;
        control.name = pending.name;
        drop(frozen);
        drop(control);
        drop(device);
        self.host.changed(id, FORMAT);
        self.notify_latency(id);
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
}
