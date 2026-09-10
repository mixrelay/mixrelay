//! Windows v2 wire contract, deliberately independent of Win32 handles and DSP.
//! This module is host-testable on every OS; it does NOT advertise device support.
//! The installed `SysVAD` LAB endpoint still accepts only [`LAB_LAYOUT`].

const PAGE_BYTES: usize = 4096;
const SAMPLE_BYTES: usize = size_of::<f32>();
pub const IOCTL_MAP: u32 = (0x1d << 16) | (3 << 14) | (0x800 << 2);
pub const STARTUP_SILENCE_FRAMES: usize = 256;
pub const LAB_LAYOUT: Layout = Layout {
    render_channels: 2,
    capture_channels: 1,
    sample_rate: 48_000,
    capacity: 4096,
};

/// Private validated dimensions, never values reread from a writable mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    render_channels: u32,
    capture_channels: u32,
    sample_rate: u32,
    capacity: u32,
}

impl Layout {
    /// Matches `MrTransportMap` bounds, including one-way and non-power-of-two rings.
    /// # Errors
    /// Rejects unsupported ABI dimensions before any size arithmetic or mapping.
    pub fn new(render: u32, capture: u32, rate: u32, capacity: u32) -> Result<Self, &'static str> {
        if render > 64
            || capture > 64
            || (render == 0 && capture == 0)
            || !(256..=16_384).contains(&capacity)
            || ![44_100, 48_000, 96_000].contains(&rate)
        {
            return Err("unsupported Windows transport dimensions");
        }
        Ok(Self {
            render_channels: render,
            capture_channels: capture,
            sample_rate: rate,
            capacity,
        })
    }

    #[must_use]
    pub const fn request(self) -> [u32; 6] {
        [
            2,
            24,
            self.render_channels,
            self.capture_channels,
            self.sample_rate,
            self.capacity,
        ]
    }

    #[must_use]
    pub const fn render_channels(self) -> usize {
        self.render_channels as usize
    }
    #[must_use]
    pub const fn capture_channels(self) -> usize {
        self.capture_channels as usize
    }
    #[must_use]
    pub const fn capacity(self) -> u64 {
        self.capacity as u64
    }
    #[must_use]
    pub const fn render_offset(self) -> usize {
        PAGE_BYTES
    }
    #[must_use]
    pub const fn capture_offset(self) -> usize {
        PAGE_BYTES + self.capacity as usize * self.render_channels() * SAMPLE_BYTES
    }
    #[must_use]
    pub const fn mapping_bytes(self) -> usize {
        let end =
            self.capture_offset() + self.capacity as usize * self.capture_channels() * SAMPLE_BYTES;
        (end + PAGE_BYTES - 1) & !(PAGE_BYTES - 1)
    }

    /// Verifies the fixed header once; audio copies subsequently use private dimensions.
    #[must_use]
    pub fn matches_header(self, prefix: [u32; 7], offsets: [u64; 3]) -> bool {
        prefix
            == [
                0x4f49_524d,
                2,
                112,
                self.capacity,
                self.render_channels,
                self.capture_channels,
                self.sample_rate,
            ]
            && offsets
                == [
                    self.render_offset() as u64,
                    self.capture_offset() as u64,
                    self.mapping_bytes() as u64,
                ]
    }

    /// No offsets or subtraction may be derived from peer counters before this check.
    #[must_use]
    pub fn valid_counters(
        self,
        read: u64,
        written: u64,
        previous: (u64, u64),
        peer: (u64, u64),
    ) -> bool {
        let (produced, consumed) = peer;
        produced >= previous.0
            && consumed >= previous.1
            && produced >= read
            && produced - read <= self.capacity()
            && consumed <= written
            && written - consumed <= self.capacity()
            && produced <= i64::MAX as u64 - self.capacity()
            && written <= i64::MAX as u64 - self.capacity()
    }
}

#[repr(C)]
#[derive(Default)]
pub struct Descriptor {
    pub version: u32,
    pub size: u32,
    pub bytes: u64,
    pub address: u64,
    pub render_event: u64,
    pub capture_event: u64,
}

impl Descriptor {
    /// Checks the complete mapping range in the caller's address width. Handle
    /// adoption/validation remains in the native owner, including on rejection.
    /// # Errors
    /// Rejects a mismatched ABI, extent, alignment or overflowing address range.
    pub fn mapping_address(&self, returned: u32, layout: Layout) -> Result<usize, &'static str> {
        let address =
            usize::try_from(self.address).map_err(|_| "mapping address exceeds process width")?;
        if returned != 40
            || self.version != 2
            || self.size != 40
            || self.bytes != layout.mapping_bytes() as u64
            || address == 0
            || !address.is_multiple_of(PAGE_BYTES)
            || address.checked_add(layout.mapping_bytes()).is_none()
        {
            return Err("invalid shared transport descriptor");
        }
        Ok(address)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lab_wire_layout_is_unchanged() {
        assert_eq!(size_of::<Descriptor>(), 40);
        assert_eq!(std::mem::offset_of!(Descriptor, address), 16);
        assert_eq!(std::mem::offset_of!(Descriptor, capture_event), 32);
        assert_eq!(IOCTL_MAP, 0x001d_e000);
        assert_eq!(LAB_LAYOUT.request(), [2, 24, 2, 1, 48000, 4096]);
        assert_eq!(LAB_LAYOUT.capture_offset(), 36864);
        assert_eq!(LAB_LAYOUT.mapping_bytes(), 53248);
        assert!(LAB_LAYOUT.matches_header(
            [0x4f49_524d, 2, 112, 4096, 2, 1, 48000],
            [4096, 36864, 53248]
        ));
    }

    #[test]
    fn all_channel_pairs_and_rates_have_disjoint_bounded_pcm_regions() {
        for render in 0..=64 {
            for capture in 0..=64 {
                if render == 0 && capture == 0 {
                    continue;
                }
                for capacity in [256, 257, 4096, 16384] {
                    for rate in [44_100, 48_000, 96_000] {
                        let layout = Layout::new(render, capture, rate, capacity).unwrap();
                        let render_end = layout.render_offset()
                            + capacity as usize * render as usize * SAMPLE_BYTES;
                        let capture_end = layout.capture_offset()
                            + capacity as usize * capture as usize * SAMPLE_BYTES;
                        assert_eq!(render_end, layout.capture_offset());
                        assert!(capture_end <= layout.mapping_bytes());
                        assert!(layout.mapping_bytes() - capture_end < PAGE_BYTES);
                        assert!(layout.mapping_bytes().is_multiple_of(PAGE_BYTES));
                        assert!(layout.mapping_bytes() <= 8_392_704);
                    }
                }
            }
        }
    }

    #[test]
    fn invalid_dimensions_and_mutated_headers_are_rejected() {
        for args in [
            (0, 0, 48000, 4096),
            (65, 1, 48000, 4096),
            (1, 65, 48000, 4096),
            (2, 1, 192_000, 4096),
            (2, 1, 48000, 255),
            (2, 1, 48000, 16385),
            (u32::MAX, 1, 48000, 4096),
        ] {
            assert!(Layout::new(args.0, args.1, args.2, args.3).is_err());
        }
        let prefix = [0x4f49_524d, 2, 112, 4096, 2, 1, 48000];
        let offsets = [4096, 36864, 53248];
        for field in 0..prefix.len() {
            let mut changed = prefix;
            changed[field] += 1;
            assert!(!LAB_LAYOUT.matches_header(changed, offsets));
        }
        for field in 0..offsets.len() {
            let mut changed = offsets;
            changed[field] = u64::MAX;
            assert!(!LAB_LAYOUT.matches_header(prefix, changed));
        }
    }

    #[test]
    fn descriptor_rejects_bad_extent_abi_and_address_before_dereference() {
        let good = || Descriptor {
            version: 2,
            size: 40,
            bytes: 53248,
            address: 4096,
            ..Descriptor::default()
        };
        assert_eq!(good().mapping_address(40, LAB_LAYOUT), Ok(4096));
        for address in [0, 1, 4097, u64::MAX, u64::MAX - 4095] {
            assert!(
                Descriptor { address, ..good() }
                    .mapping_address(40, LAB_LAYOUT)
                    .is_err()
            );
        }
        assert!(
            Descriptor {
                version: 1,
                ..good()
            }
            .mapping_address(40, LAB_LAYOUT)
            .is_err()
        );
        assert!(
            Descriptor { size: 39, ..good() }
                .mapping_address(40, LAB_LAYOUT)
                .is_err()
        );
        assert!(
            Descriptor {
                bytes: 53247,
                ..good()
            }
            .mapping_address(40, LAB_LAYOUT)
            .is_err()
        );
        assert!(good().mapping_address(39, LAB_LAYOUT).is_err());
    }

    #[test]
    fn wrapped_counters_reject_backward_overfull_and_signed_overflow_windows() {
        for capacity in [256, 257, 4096, 16384] {
            let layout = Layout::new(64, 16, 48000, capacity).unwrap();
            let capacity = u64::from(capacity);
            for wrap in 0..128 {
                let base = wrap * capacity;
                for count in [0, 1, capacity - 1, capacity] {
                    assert!(layout.valid_counters(
                        base,
                        base + count,
                        (base, base),
                        (base + count, base)
                    ));
                    assert!(!layout.valid_counters(
                        base,
                        base + count,
                        (base, base),
                        (base + capacity + 1, base)
                    ));
                }
            }
            for (read, written, previous, peer) in [
                (0, 0, (0, 0), (0, 1)),
                (8, 0, (8, 0), (7, 0)),
                (0, 8, (0, 8), (0, 7)),
                (0, 0, (0, 0), (u64::MAX, 0)),
                (0, capacity + 1, (0, 0), (0, 0)),
                (i64::MAX as u64, 0, (0, 0), (i64::MAX as u64, 0)),
            ] {
                assert!(!layout.valid_counters(read, written, previous, peer));
            }
        }
    }
}
