//! Control-owned client PCM queues and capture mixing independent of postmix callbacks.
use mixrelay_macos_transport::shared_audio_buffer::{self as buffer, SharedAudioBuffer};
use std::{
    alloc::{Layout, alloc_zeroed, dealloc},
    ptr::NonNull,
    sync::atomic::{AtomicBool, Ordering},
};

use crate::{abi::Geometry, io_outcome::IoOutcome};

const MAX_CLIENTS: usize = 256;
// Match the driver registry's existing mapping admission bound for private PCM storage.
const MAX_PCM_BYTES: usize = 1 << 30;

struct Queue {
    pointer: NonNull<SharedAudioBuffer>,
    layout: Layout,
}

// All shared cells are atomic. One serialized MixOutput producer owns each client queue;
// capture readers borrow it concurrently. Control drains device IO before dropping it.
unsafe impl Send for Queue {}
unsafe impl Sync for Queue {}

impl Queue {
    fn new(capacity: u32, channels: u32) -> Option<Self> {
        let bytes = buffer::mr_shared_audio_buffer_required_bytes(capacity, channels);
        let layout = Layout::from_size_align(bytes, align_of::<SharedAudioBuffer>()).ok()?;
        if bytes == 0 {
            return None;
        }
        // SAFETY: control owns the complete allocation before any callback sees it.
        let memory = NonNull::new(unsafe { alloc_zeroed(layout) })?;
        let pointer = unsafe {
            buffer::mr_shared_audio_buffer_initialize(
                memory.as_ptr().cast(),
                bytes,
                capacity,
                channels,
            )
        };
        let Some(pointer) = NonNull::new(pointer) else {
            // SAFETY: initialization did not transfer allocation ownership.
            unsafe { dealloc(memory.as_ptr(), layout) };
            return None;
        };
        Some(Self { pointer, layout })
    }

    fn begin(&self) -> bool {
        // SAFETY: the device control owner excludes IO during this epoch transition.
        unsafe { buffer::mr_shared_audio_buffer_try_begin_epoch(self.pointer.as_ptr()) != 0 }
    }

    fn write(&self, frame: u64, samples: &[f32], frames: u32) -> bool {
        // SAFETY: the retained queue and complete interleaved input belong to this callback.
        unsafe {
            let epoch = buffer::mr_shared_audio_buffer_stats(self.pointer.as_ptr()).epoch;
            buffer::mr_shared_audio_buffer_write(
                self.pointer.as_ptr(),
                epoch,
                frame,
                samples.as_ptr(),
                frames,
            )
        }
    }

    fn read(&self, frame: u64, samples: &mut [f32], frames: u32) -> u32 {
        // SAFETY: the retained queue and exclusive complete output belong to this callback.
        unsafe {
            buffer::mr_shared_audio_buffer_read(
                self.pointer.as_ptr(),
                frame,
                samples.as_mut_ptr(),
                frames,
            )
        }
    }
}

impl Drop for Queue {
    fn drop(&mut self) {
        // SAFETY: control drains all callback pins before retiring this allocation.
        unsafe { dealloc(self.pointer.as_ptr().cast(), self.layout) };
    }
}

struct Client {
    id: u32,
    rendered: AtomicBool,
    queue: Option<Queue>,
}

pub(super) struct ClientMix {
    clients: Vec<Client>,
    geometry: Geometry,
    capacity: u32,
}

impl ClientMix {
    pub(super) fn new(geometry: Geometry, capacity: u32) -> Option<Self> {
        let mut clients = Vec::new();
        clients.try_reserve_exact(MAX_CLIENTS).ok()?;
        Some(Self {
            clients,
            geometry,
            capacity,
        })
    }

    pub(super) fn geometry(&self) -> Geometry {
        self.geometry
    }

    pub(super) fn register(&mut self, id: u32) -> bool {
        if self.clients.len() == MAX_CLIENTS || self.clients.iter().any(|client| client.id == id) {
            return false;
        }
        let bytes = buffer::mr_shared_audio_buffer_required_bytes(
            self.capacity,
            self.geometry.render_channels,
        );
        if self
            .clients
            .len()
            .checked_add(1)
            .and_then(|count| count.checked_mul(bytes))
            .is_none_or(|bytes| bytes > MAX_PCM_BYTES)
        {
            return false;
        }
        let queue = if self.geometry.render_channels == 0 {
            None
        } else {
            let Some(queue) = Queue::new(self.capacity, self.geometry.render_channels) else {
                return false;
            };
            Some(queue)
        };
        self.clients.push(Client {
            id,
            rendered: AtomicBool::new(false),
            queue,
        });
        true
    }

    pub(super) fn remove(&mut self, id: u32) {
        if let Some(index) = self.clients.iter().position(|client| client.id == id) {
            self.clients.remove(index);
        }
    }

    pub(super) fn begin_device_io(&mut self) -> bool {
        for client in &mut self.clients {
            if let Some(queue) = &client.queue
                && !queue.begin()
            {
                let Some(queue) = Queue::new(self.capacity, self.geometry.render_channels) else {
                    return false;
                };
                client.queue = Some(queue);
            }
            // Device control excludes callbacks while opening the new timeline.
            client.rendered.store(false, Ordering::Relaxed);
        }
        true
    }

    pub(super) fn replacement(&self, geometry: Geometry, capacity: u32) -> Option<Self> {
        let mut next = Self::new(geometry, capacity)?;
        for client in &self.clients {
            if !next.register(client.id) {
                return None;
            }
        }
        Some(next)
    }

    pub(super) fn render(
        &self,
        client: u32,
        frame: u64,
        samples: &[f32],
        frames: u32,
    ) -> IoOutcome {
        if frame
            .checked_add(u64::from(frames))
            .is_none_or(|end| end > (1 << 48) - 1)
        {
            return IoOutcome::default();
        }
        let Some(client) = self.clients.iter().find(|candidate| candidate.id == client) else {
            return IoOutcome::default();
        };
        if samples.len() != frames as usize * self.geometry.render_channels as usize
            || !client
                .queue
                .as_ref()
                .is_some_and(|queue| queue.write(frame, samples, frames))
        {
            return IoOutcome::default();
        }
        // Release publishes that this client has PCM, after the queue committed its frames.
        client.rendered.store(true, Ordering::Release);
        IoOutcome::completed(frames)
    }

    pub(super) fn capture(&self, frame: u64, output: &mut [f32], frames: u32) -> IoOutcome {
        output.fill(0.0);
        let channels = self.geometry.capture_channels as usize;
        if channels == 0
            || frames as usize > crate::formats::MAX_BUFFER_FRAMES
            || frame
                .checked_add(u64::from(frames))
                .is_none_or(|end| end > (1 << 48) - 1)
            || output.len() != frames as usize * channels
        {
            return IoOutcome::default();
        }
        // Acquire observes each client's first completed PCM publication. Lifecycle is frozen
        // by the device IO pin; no client or epoch can be retired during this mix.
        let mut sources = self
            .clients
            .iter()
            .filter(|client| client.rendered.load(Ordering::Acquire));
        let Some(first) = sources.next().and_then(|client| client.queue.as_ref()) else {
            return IoOutcome::completed(0);
        };
        let mut available = [false; crate::formats::MAX_BUFFER_FRAMES];
        let mut scratch = [0.0; 64];
        let render_channels = self.geometry.render_channels as usize;
        let mut mix = |queue: &Queue| {
            for (index, destination) in output.chunks_exact_mut(channels).enumerate() {
                let supplied =
                    queue.read(frame + index as u64, &mut scratch[..render_channels], 1) == 1;
                available[index] |= supplied;
                for (destination, sample) in destination.iter_mut().zip(&scratch[..render_channels])
                {
                    *destination += sample;
                }
            }
        };
        if render_channels == channels && sources.clone().next().is_none() {
            return IoOutcome::completed(first.read(frame, output, frames));
        }
        mix(first);
        for client in sources {
            if let Some(queue) = &client.queue {
                mix(queue);
            }
        }
        IoOutcome::completed(
            available[..frames as usize]
                .iter()
                .fold(0_u32, |count, valid| count + u32::from(*valid)),
        )
    }
}

#[cfg(test)]
mod tests;
