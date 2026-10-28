//! Control-owned client PCM queues and capture mixing independent of postmix callbacks.
use crate::{abi::Geometry, clock::Clock, io_outcome::IoOutcome};
use mixrelay_macos_transport::{
    client_pcm::{
        ClientPcmClockOwner, ClientPcmSnapshot, ClientPcmTimeline, ClientPcmWriter as Queue,
        MAX_CLIENTS_PER_ENDPOINT as MAX_CLIENTS, MAX_PCM_BYTES_PER_ENDPOINT as MAX_PCM_BYTES,
        QUEUE_FRAMES,
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

fn client_limit(channels: u32) -> usize {
    if channels == 0 {
        return MAX_CLIENTS;
    }
    let bytes = buffer::mr_shared_audio_buffer_required_bytes(QUEUE_FRAMES, channels);
    MAX_PCM_BYTES
        .checked_div(bytes)
        .map_or(0, |limit| limit.min(MAX_CLIENTS))
}

struct Client {
    id: u32,
    rendered: AtomicBool,
    queue: Option<Queue>,
}

/// One client's queue, allocated on control before the IO freeze that registers it.
pub(super) struct PreparedClient {
    id: u32,
    queue: Option<Queue>,
}
impl PreparedClient {
    pub(super) fn id(&self) -> u32 {
        self.id
    }
}

/// Fresh queues for clients whose ring cannot open another epoch, allocated on control
/// before the freeze in which a clock publication rearms every queue.
#[derive(Default)]
pub(super) struct Rearm(Vec<PreparedClient>);

/// Whether a clock publication opens a new PCM epoch in every client queue.
pub(super) enum Timeline {
    Keep,
    Rearm(Rearm),
}

pub(super) struct ClientMix {
    clients: Vec<Client>,
    geometry: Geometry,
    revision: u64,
    clock: Arc<ClientPcmClockOwner>,
}
impl Drop for ClientMix {
    fn drop(&mut self) {
        self.clock.retire();
    }
}

impl ClientMix {
    pub(super) fn new(geometry: Geometry) -> Option<Self> {
        let mut clients = Vec::new();
        clients.try_reserve_exact(MAX_CLIENTS).ok()?;
        Some(Self {
            clients,
            geometry,
            revision: 1,
            clock: Arc::new(ClientPcmClockOwner::new().ok()?),
        })
    }

    /// Maps and grants a queue without touching the published client set. Shared
    /// access is enough: the caller holds device control, so the set cannot change.
    pub(super) fn prepare(&self, id: u32) -> Option<PreparedClient> {
        let queue = if self.geometry.render_channels == 0 {
            None
        } else {
            Some(Queue::with_clock(self.configuration(), id, Arc::clone(&self.clock)).ok()?)
        };
        Some(PreparedClient { id, queue })
    }

    /// Publishes a prepared queue. The reserved client vector never allocates here.
    pub(super) fn register(&mut self, prepared: PreparedClient) -> bool {
        let PreparedClient { id, queue } = prepared;
        if self.revision == u64::MAX
            || self.clients.len() >= client_limit(self.geometry.render_channels)
            || self.clients.iter().any(|client| client.id == id)
            || queue
                .as_ref()
                .map_or(self.geometry.render_channels != 0, |queue| {
                    !queue.belongs_to(&self.clock)
                })
        {
            return false;
        }
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

    /// Whether both sets hold the same clients in the same order.
    pub(super) fn same_members(&self, other: &Self) -> bool {
        self.clients
            .iter()
            .map(|client| client.id)
            .eq(other.clients.iter().map(|client| client.id))
    }

    fn configuration(&self) -> EndpointTransportConfig {
        EndpointTransportConfig {
            endpoint_id: self.geometry.endpoint_id,
            capacity_frames: QUEUE_FRAMES,
            render_channels: self.geometry.render_channels,
            capture_channels: self.geometry.capture_channels,
        }
    }

    /// Maps a replacement for every queue that cannot open another epoch. Shared access
    /// is enough: the caller holds device control, so neither set nor epochs can change.
    pub(super) fn prepare_rearm(&self) -> Option<Rearm> {
        let mut replacements = Vec::new();
        for client in &self.clients {
            if client
                .queue
                .as_ref()
                .is_some_and(|queue| !queue.can_rearm())
            {
                replacements.push(self.prepare(client.id)?);
            }
        }
        Some(Rearm(replacements))
    }

    pub(super) fn publish_clock(&mut self, clock: &Clock, timeline: Timeline) -> bool {
        self.publish_clock_after(clock, timeline, || true)
    }

    /// Keeps remote readers fenced until client queues and native stream preparation succeed.
    /// Every precondition is checked before the first queue changes, so a refused rearm
    /// leaves all queues and the published clock as they were.
    pub(super) fn publish_clock_after(
        &mut self,
        clock: &Clock,
        timeline: Timeline,
        prepare: impl FnOnce() -> bool,
    ) -> bool {
        let (rearm, mut replacements) = match timeline {
            Timeline::Keep => (false, Vec::new()),
            Timeline::Rearm(Rearm(replacements)) => (true, replacements),
        };
        replacements.retain(|prepared| {
            self.clients.iter().any(|client| {
                client.id == prepared.id
                    && client
                        .queue
                        .as_ref()
                        .is_some_and(|queue| !queue.can_rearm())
            })
        });
        let ready = |client: &Client| {
            client.queue.as_ref().is_none_or(|queue| {
                if rearm {
                    queue.can_rearm()
                        || replacements.iter().any(|prepared| {
                            prepared.id == client.id
                                && prepared.queue.as_ref().is_some_and(|queue| {
                                    queue.belongs_to(&self.clock) && queue.can_rearm()
                                })
                        })
                } else {
                    true
                }
            })
        };
        let Some(revision) = self
            .revision
            .checked_add(u64::try_from(replacements.len()).unwrap_or(u64::MAX))
        else {
            return false;
        };
        if !crate::formats::SAMPLE_RATES.contains(&clock.rate) || !self.clients.iter().all(ready) {
            return false;
        }
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
            if let Some(index) = replacements
                .iter()
                .position(|prepared| prepared.id == client.id)
            {
                // swap_remove keeps the commit allocation-free; the old queue retires here.
                client.queue = replacements.swap_remove(index).queue;
            }
            if let Some(queue) = &mut client.queue
                && !publication.configure(queue, rearm)
            {
                // Only a retired queue refuses here; a rearm excluded that above.
                return false;
            }
            if rearm {
                client.rendered.store(false, Ordering::Relaxed);
            }
        }
        if revision != self.revision {
            self.revision = revision;
            self.clock.publish_membership(self.revision);
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

    /// Prepares the same registrations for another geometry, on control before a freeze.
    pub(super) fn replacement(&self, geometry: Geometry) -> Option<Self> {
        let mut next = Self::new(geometry)?;
        for client in &self.clients {
            let prepared = next.prepare(client.id)?;
            if !next.register(prepared) {
                return None;
            }
        }
        Some(next)
    }

    /// Orders a committed replacement's membership after the set it replaces, so readers
    /// holding the previous grants request the new list.
    pub(super) fn supersede(&mut self, previous: &Self) -> bool {
        let Some(revision) = previous.revision.checked_add(1) else {
            return false;
        };
        self.revision = self.revision.max(revision);
        self.clock.publish_membership(self.revision);
        true
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
