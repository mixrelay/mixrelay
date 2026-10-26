//! Admission and dispatch of bounded host PCM requests, independent of inventory control.
use crate::{
    device::Device,
    io_admission::Stage,
    registry::{IoOperation, Registry},
};
use std::sync::atomic::Ordering;

impl Registry {
    pub(crate) unsafe fn io(&self, operation: IoOperation) -> bool {
        let _audio = crate::transport::AudioScope::enter();
        let IoOperation {
            device: id,
            stream,
            render,
            client,
            frame,
            samples,
            frames,
        } = operation;
        let Some(device) = self.pin(id).filter(|device| device.id == id) else {
            return false;
        };
        device
            .io_trace
            .note(if render { Stage::Write } else { Stage::Read });
        if samples.is_null() || frames as usize > crate::formats::MAX_BUFFER_FRAMES {
            device.io_trace.note(Stage::Extent);
            return false;
        }
        let expected = device.id + if render { 2 } else { 1 };
        if stream != expected {
            device.io_trace.note(Stage::Stream);
            return false;
        }
        let Some(_io) = device.io_pin() else {
            device.io_trace.note(Stage::Frozen);
            return false;
        };
        // SAFETY: the IO pin fixes native geometry and the host owns the full buffer.
        if unsafe { idle_capture(&device, operation) } {
            device.io_trace.note(Stage::Inactive);
            return true;
        }
        let accepted = unsafe {
            device.process_io(crate::io_outcome::Transfer {
                direction: if render {
                    crate::io_outcome::Direction::ClientOutput(client)
                } else {
                    crate::io_outcome::Direction::Capture
                },
                frame,
                samples,
                frames,
            })
        };
        if !accepted {
            device.io_trace.note(Stage::Native);
        }
        accepted
    }
}

// SAFETY: the caller holds an IO pin and validated the operation extent. The SDK
// owns frames * native_channels writable samples, in the pinned native format.
unsafe fn idle_capture(device: &Device, operation: IoOperation) -> bool {
    let active = if operation.render {
        &device.output_active
    } else {
        &device.input_active
    };
    let idle = !device.alive.load(Ordering::Acquire)
        || !active.load(Ordering::Acquire)
        || device.running.load(Ordering::Acquire) == 0;
    if idle && !operation.render {
        let channels = device.capture_channels.load(Ordering::Acquire) as usize;
        let bytes = operation.frames as usize
            * channels
            * device.capture_format.format().bytes_per_sample();
        // SAFETY: the IO pin fixes this validated writable native extent.
        unsafe { std::ptr::write_bytes(operation.samples.cast::<u8>(), 0, bytes) };
    }
    idle
}
