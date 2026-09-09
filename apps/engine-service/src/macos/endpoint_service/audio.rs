//! Checked callback access. One pin fixes mapping identity; paired ring stats
//! and pre/post epoch checks also reject a restart within the same mapping.
use super::EndpointSnapshot;
use crate::endpoint_registry::CallbackEndpoint;
use tonenari_macos_transport::{
    endpoint_transport::{
        EndpointClockSnapshot, tn_endpoint_transport_clock_snapshot,
        tn_endpoint_transport_incarnation,
    },
    shared_audio_buffer::{
        tn_shared_audio_buffer_producer_activity, tn_shared_audio_buffer_read,
        tn_shared_audio_buffer_stats, tn_shared_audio_buffer_try_begin_epoch,
        tn_shared_audio_buffer_write,
    },
};

pub(super) fn snapshot(endpoint: &CallbackEndpoint<'_>) -> Option<EndpointSnapshot> {
    let mut result = EndpointSnapshot {
        endpoint_id: endpoint.spec().id,
        ..EndpointSnapshot::default()
    };
    // SAFETY: the registry pin owns the complete initialized transport; both output
    // locals have the exact ABI extent and remain disjoint through these calls.
    if !unsafe { tn_endpoint_transport_clock_snapshot(endpoint.transport(), &raw mut result.clock) }
        || !unsafe {
            tn_endpoint_transport_incarnation(endpoint.transport(), result.incarnation.as_mut_ptr())
        }
    {
        return None;
    }
    // SAFETY: optional disabled rings are null, which these read-only shared-buffer
    // accessors explicitly accept. Enabled rings remain owned by the endpoint pin.
    let render = unsafe { tn_shared_audio_buffer_stats(endpoint.render_buffer()) };
    result.render_epoch = render.epoch;
    result.capture_epoch = unsafe { tn_shared_audio_buffer_stats(endpoint.capture_buffer()) }.epoch;
    result.published_through = render.published_through_frame;
    result.render_activity =
        unsafe { tn_shared_audio_buffer_producer_activity(endpoint.render_buffer()) };
    result.render_channels = endpoint.spec().render_channels;
    result.capture_channels = endpoint.spec().capture_channels;
    let mut after = EndpointClockSnapshot::default();
    // SAFETY: the same transport remains pinned; after is separate writable ABI storage.
    if !unsafe { tn_endpoint_transport_clock_snapshot(endpoint.transport(), &raw mut after) }
        || after.epoch != result.clock.epoch
        || after.sample_rate != result.clock.sample_rate
        || after.running != result.clock.running
    {
        return None;
    }
    Some(result)
}

pub(super) fn matches(endpoint: &CallbackEndpoint<'_>, expected: &EndpointSnapshot) -> bool {
    let Some(actual) = snapshot(endpoint) else {
        return false;
    };
    actual.endpoint_id == expected.endpoint_id
        && actual.incarnation == expected.incarnation
        && actual.clock.epoch == expected.clock.epoch
        && actual.clock.sample_rate == expected.clock.sample_rate
        && actual.clock.running == expected.clock.running
}

pub(super) fn read_for(
    endpoint: Option<&CallbackEndpoint<'_>>,
    expected: &EndpointSnapshot,
    frame: u64,
    samples: &mut [f32],
    channels: u32,
) -> u32 {
    let Some(frames) = frame_count(samples.len(), channels) else {
        return 0;
    };
    let count = endpoint.map_or(0, |endpoint| {
        if endpoint.spec().render_channels != channels || !matches(endpoint, expected) {
            return 0;
        }
        let buffer = endpoint.render_buffer();
        // SAFETY: the checked channel width selects this live, pin-owned render ring.
        if unsafe { tn_shared_audio_buffer_stats(buffer) }.epoch != expected.render_epoch {
            return 0;
        }
        // SAFETY: the endpoint pin retains the live mapping; channel geometry
        // matches this slice and frame_count validated its exact writable extent.
        let count =
            unsafe { tn_shared_audio_buffer_read(buffer, frame, samples.as_mut_ptr(), frames) };
        // SAFETY: the ring remains pinned across the transfer and this follow-up read.
        if unsafe { tn_shared_audio_buffer_stats(buffer) }.epoch != expected.render_epoch
            || !matches(endpoint, expected)
        {
            return 0;
        }
        count
    });
    if count == 0 {
        samples.fill(0.0);
    }
    count
}

pub(super) fn write_for(
    endpoint: &CallbackEndpoint<'_>,
    expected: &EndpointSnapshot,
    frame: u64,
    samples: &[f32],
    channels: u32,
) -> bool {
    let Some(frames) = frame_count(samples.len(), channels) else {
        return false;
    };
    // SAFETY: short-circuit validation selects the live capture ring and exact sample
    // extent before FFI. The pin owns storage, while the ring's producer claim rejects
    // concurrent writers/rearm; immutable samples remain borrowed for this call only.
    endpoint.spec().capture_channels == channels
        && matches(endpoint, expected)
        && unsafe { tn_shared_audio_buffer_stats(endpoint.capture_buffer()) }.epoch
            == expected.capture_epoch
        && unsafe {
            tn_shared_audio_buffer_write(
                endpoint.capture_buffer(),
                expected.capture_epoch,
                frame,
                samples.as_ptr(),
                frames,
            )
        }
        && matches(endpoint, expected)
}

fn frame_count(samples: usize, channels: u32) -> Option<u32> {
    if !(1..=64).contains(&channels) || !samples.is_multiple_of(channels as usize) {
        return None;
    }
    u32::try_from(samples / channels as usize)
        .ok()
        .filter(|frames| (1..=1 << 20).contains(frames))
}

pub(super) fn begin_for(endpoint: &CallbackEndpoint<'_>, expected: &EndpointSnapshot) -> bool {
    // SAFETY: the endpoint pin retains the initialized writable capture mapping;
    // the buffer's producer claim arbitrates rearm against active PCM publication.
    matches(endpoint, expected)
        && unsafe { tn_shared_audio_buffer_try_begin_epoch(endpoint.capture_buffer()) } != 0
}
