//! Prepared channel operations; common full-width routes use contiguous PCM spans.
use super::{PortSlot, buffers::port_pair};
use crate::runtime_control::ControlValue;

#[derive(Debug)]
enum ChannelTransfer {
    Contiguous,
    Mapped(Box<[(u16, u16)]>),
}

#[derive(Debug)]
pub(super) struct RouteOp {
    source: PortSlot,
    destination: PortSlot,
    channels: ChannelTransfer,
    controls: [usize; 2],
}

impl RouteOp {
    pub(super) fn new(
        source: PortSlot,
        destination: PortSlot,
        controls: [usize; 2],
        routes: impl ExactSizeIterator<Item = (u16, u16)> + Clone,
    ) -> Self {
        let contiguous = source.channels == destination.channels
            && routes.len() == usize::from(source.channels)
            && routes
                .clone()
                .enumerate()
                .all(|(i, (from, to))| usize::from(from) == i && from == to);
        Self {
            source,
            destination,
            controls,
            channels: if contiguous {
                ChannelTransfer::Contiguous
            } else {
                ChannelTransfer::Mapped(routes.collect())
            },
        }
    }

    fn apply(&self, arena: &mut [f32], frames: usize) {
        let (source, destination) = port_pair(arena, self.source, self.destination, frames);
        match &self.channels {
            ChannelTransfer::Contiguous => {
                for (destination, source) in destination.iter_mut().zip(source) {
                    *destination += source;
                }
            }
            ChannelTransfer::Mapped(routes) => {
                for (source, destination) in source
                    .chunks_exact(usize::from(self.source.channels))
                    .zip(destination.chunks_exact_mut(usize::from(self.destination.channels)))
                {
                    for &(from, to) in routes {
                        destination[usize::from(to)] += source[usize::from(from)];
                    }
                }
            }
        }
    }
}

pub(super) fn apply_routes(
    arena: &mut [f32],
    routes: &[RouteOp],
    controls: &[ControlValue],
    frames: usize,
) {
    for route in routes {
        if route
            .controls
            .iter()
            .all(|&index| controls[index].control.enabled)
        {
            route.apply(arena, frames);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compiled_routes_preserve_channel_maps_accumulation_and_both_span_orders() {
        for channels in [2_u16, 32, 64] {
            for reversed in [false, true] {
                for mapped in [false, true] {
                    let samples = usize::from(channels) * 3;
                    let source = PortSlot {
                        offset: if reversed { samples } else { 0 },
                        channels,
                    };
                    let destination = PortSlot {
                        offset: if reversed { 0 } else { samples },
                        channels,
                    };
                    let pairs: Vec<_> = (0..channels)
                        .map(|from| (from, if mapped { 0 } else { from }))
                        .collect();
                    let route = RouteOp::new(source, destination, [0, 1], pairs.iter().copied());
                    assert_eq!(matches!(route.channels, ChannelTransfer::Mapped(_)), mapped);
                    let mut arena = vec![0.0_f32; samples * 2];
                    for (i, value) in arena[source.offset..source.offset + samples]
                        .iter_mut()
                        .enumerate()
                    {
                        *value = f32::from(u16::try_from(i + 1).unwrap()) / 1024.0;
                    }
                    let input = arena[source.offset..source.offset + samples].to_vec();
                    let mut expected = vec![0.0_f32; samples];
                    for frame in 0..3 {
                        for &(from, to) in &pairs {
                            expected[frame * usize::from(channels) + usize::from(to)] +=
                                input[frame * usize::from(channels) + usize::from(from)];
                        }
                    }
                    route.apply(&mut arena, 3);
                    assert!(
                        arena[destination.offset..destination.offset + samples]
                            .iter()
                            .zip(expected)
                            .all(|(a, b)| a.to_bits() == b.to_bits())
                    );
                    assert!(
                        arena[source.offset..source.offset + samples]
                            .iter()
                            .zip(input)
                            .all(|(a, b)| a.to_bits() == b.to_bits())
                    );
                }
            }
        }
    }
}
