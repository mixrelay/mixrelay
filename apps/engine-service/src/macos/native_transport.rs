//! Checked transfers preserve the exact endpoint identity, incarnation and channel geometry.
use super::NativeClockSnapshot;
pub(super) use endpoint_service::EndpointSnapshot;
use tonenari_engine_service::{endpoint_registry::EndpointId, endpoint_service};

pub(super) fn snapshot(id: EndpointId) -> Option<EndpointSnapshot> {
    endpoint_service::snapshot(id)
}
pub(super) fn clock(snapshot: &EndpointSnapshot) -> NativeClockSnapshot {
    NativeClockSnapshot {
        incarnation: snapshot.incarnation,
        epoch: snapshot.clock.epoch,
        render_through: snapshot.clock.render_through,
        capture_through: snapshot.clock.capture_through,
        sample_rate: snapshot.clock.sample_rate,
        running: snapshot.clock.running,
    }
}
fn channels(samples: usize, frames: u32) -> Option<u32> {
    let frames = usize::try_from(frames).ok().filter(|value| *value != 0)?;
    if !samples.is_multiple_of(frames) {
        return None;
    }
    u32::try_from(samples / frames)
        .ok()
        .filter(|value| (1..=64).contains(value))
}
pub(super) fn read(
    expected: &EndpointSnapshot,
    frame: u64,
    samples: &mut [f32],
    frames: u32,
) -> u32 {
    let Some(channels) = channels(samples.len(), frames) else {
        samples.fill(0.0);
        return 0;
    };
    endpoint_service::read_render(expected, frame, samples, channels)
}
pub(super) fn write(expected: &EndpointSnapshot, frame: u64, samples: &[f32], frames: u32) -> bool {
    let Some(channels) = channels(samples.len(), frames) else {
        return false;
    };
    endpoint_service::write_capture(expected, frame, samples, channels)
}
pub(super) fn pending(expected: &EndpointSnapshot) -> bool {
    endpoint_service::capture_pending(expected)
}
pub(super) fn rearm(expected: &EndpointSnapshot) -> Option<EndpointSnapshot> {
    endpoint_service::rearm_capture(expected).filter(|current| {
        current.endpoint_id == expected.endpoint_id
            && current.incarnation == expected.incarnation
            && current.clock.epoch == expected.clock.epoch
            && current.clock.sample_rate == expected.clock.sample_rate
            && current.clock.running == expected.clock.running
            && current.capture_channels == expected.capture_channels
            && current.capture_epoch != 0
    })
}
