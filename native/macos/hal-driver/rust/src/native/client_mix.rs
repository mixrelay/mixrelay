//! Control-owned client PCM queues and capture mixing independent of postmix callbacks.
use crate::{abi::Geometry, clock::Clock, io_outcome::IoOutcome};
use mixrelay_macos_transport::{
    client_pcm::{
        ClientPcmClockOwner, ClientPcmSnapshot, ClientPcmTimeline, ClientPcmWriter as Queue,
        MAX_CLIENTS_PER_ENDPOINT as MAX_CLIENTS, MAX_PCM_BYTES_PER_ENDPOINT as MAX_PCM_BYTES,
    },
    endpoint_transport::EndpointTransportConfig,
    shared_audio_buffer as buffer,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

// A copy tile bounds stack scratch to 8 KiB at the supported 64-channel width.
// It does not change the device quantum, timeline or queue capacity.
const COPY_FRAMES: usize = 32;
const MAX_CHANNELS: usize = 64;

fn client_limit(capacity: u32, channels: u32) -> usize {
    if channels == 0 {
        return MAX_CLIENTS;
    }
    let bytes = buffer::mr_shared_audio_buffer_required_bytes(capacity, channels);
    MAX_PCM_BYTES
        .checked_div(bytes)
        .map_or(0, |limit| limit.min(MAX_CLIENTS))
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
    revision: u64,
    clock: Arc<ClientPcmClockOwner>,
}
impl Drop for ClientMix {
    fn drop(&mut self) {
        self.clock.retire();
    }
}

impl ClientMix {
    pub(super) fn new(geometry: Geometry, capacity: u32) -> Option<Self> {
        let mut clients = Vec::new();
        clients.try_reserve_exact(MAX_CLIENTS).ok()?;
        Some(Self {
            clients,
            geometry,
            capacity,
            revision: 1,
            clock: Arc::new(ClientPcmClockOwner::new().ok()?),
        })
    }

    pub(super) fn geometry(&self) -> Geometry {
        self.geometry
    }

    pub(super) fn register(&mut self, id: u32) -> bool {
        if self.revision == u64::MAX
            || self.clients.len() >= client_limit(self.capacity, self.geometry.render_channels)
            || self.clients.iter().any(|client| client.id == id)
        {
            return false;
        }
        let queue = if self.geometry.render_channels == 0 {
            None
        } else {
            let Some(queue) =
                Queue::with_clock(self.configuration(), id, Arc::clone(&self.clock)).ok()
            else {
                return false;
            };
            Some(queue)
        };
        self.revision += 1;
        self.clock.publish_membership(self.revision);
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
            self.revision = self.revision.saturating_add(1);
            self.clock.publish_membership(self.revision);
        }
    }

    fn configuration(&self) -> EndpointTransportConfig {
        EndpointTransportConfig {
            endpoint_id: self.geometry.endpoint_id,
            capacity_frames: self.capacity,
            render_channels: self.geometry.render_channels,
            capture_channels: self.geometry.capture_channels,
        }
    }

    pub(super) fn publish_clock(&mut self, clock: &Clock, rearm: bool) -> bool {
        self.publish_clock_after(clock, rearm, || true)
    }

    /// Keeps remote readers fenced until client queues and native stream preparation succeed.
    pub(super) fn publish_clock_after(
        &mut self,
        clock: &Clock,
        rearm: bool,
        prepare: impl FnOnce() -> bool,
    ) -> bool {
        let config = self.configuration();
        let (epoch, anchor) = clock.transport_epoch();
        let Some(publication) = self.clock.begin(ClientPcmTimeline {
            epoch,
            anchor,
            rate: clock.rate,
            running: clock.running,
        }) else {
            return false;
        };
        for client in &mut self.clients {
            if let Some(queue) = &mut client.queue
                && !publication.configure(queue, rearm)
            {
                if !rearm || self.revision == u64::MAX {
                    return false;
                }
                let Ok(mut replacement) =
                    Queue::with_clock(config, client.id, Arc::clone(&self.clock))
                else {
                    return false;
                };
                if !publication.configure(&mut replacement, true) {
                    return false;
                }
                client.queue = Some(replacement);
                self.revision += 1;
                self.clock.publish_membership(self.revision);
            }
            if rearm {
                client.rendered.store(false, Ordering::Relaxed);
            }
        }
        if !prepare() {
            return false;
        }
        publication.commit();
        true
    }

    pub(super) fn revision(&self) -> u64 {
        self.revision
    }
    pub(super) fn append_snapshot(&self, snapshot: &mut ClientPcmSnapshot) -> bool {
        self.clients.iter().all(|client| {
            client
                .queue
                .as_ref()
                .is_none_or(|queue| snapshot.push(queue))
        })
    }

    pub(super) fn replacement(&self, geometry: Geometry, capacity: u32) -> Option<Self> {
        let mut next = Self::new(geometry, capacity)?;
        for client in &self.clients {
            if !next.register(client.id) {
                return None;
            }
        }
        next.revision = self.revision.checked_add(1)?;
        next.clock.publish_membership(next.revision);
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
        let render_channels = self.geometry.render_channels as usize;
        if render_channels == channels && sources.clone().next().is_none() {
            return IoOutcome::completed(first.read(frame, output, frames));
        }
        let mut available = [false; crate::formats::MAX_BUFFER_FRAMES];
        let mut scratch = [0.0; COPY_FRAMES * MAX_CHANNELS];
        let mut coverage = [false; COPY_FRAMES];
        let mut mix = |queue: &Queue| {
            for (tile, destinations) in output.chunks_mut(COPY_FRAMES * channels).enumerate() {
                let offset = tile * COPY_FRAMES;
                let count = destinations.len() / channels;
                queue.read_with_coverage(
                    frame + offset as u64,
                    &mut scratch[..count * render_channels],
                    &mut coverage[..count],
                );
                for (index, (destination, samples)) in destinations
                    .chunks_exact_mut(channels)
                    .zip(scratch.chunks_exact(render_channels))
                    .enumerate()
                {
                    available[offset + index] |= coverage[index];
                    for (destination, sample) in destination.iter_mut().zip(samples) {
                        *destination += sample;
                    }
                }
            }
        };
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
