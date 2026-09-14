//! Safe, disjoint PCM borrows over the immutable plan's allocated port spans.
use super::{PortSlot, sample_count};

pub(super) fn port_pair(
    arena: &mut [f32],
    input: PortSlot,
    output: PortSlot,
    frames: usize,
) -> (&[f32], &mut [f32]) {
    let input_samples = sample_count(frames, input.channels);
    let output_samples = sample_count(frames, output.channels);
    // allocate_slots assigns disjoint spans, including a node's own input/output.
    // split_at_mut enforces that contract without raw pointers or aliasing promises.
    if input.offset < output.offset {
        let (before, after) = arena.split_at_mut(output.offset);
        (
            &before[input.offset..input.offset + input_samples],
            &mut after[..output_samples],
        )
    } else {
        let (before, after) = arena.split_at_mut(input.offset);
        (
            &after[..input_samples],
            &mut before[output.offset..output.offset + output_samples],
        )
    }
}
