//! Native callback completion and capture availability at the HAL boundary.

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub(crate) struct IoOutcome {
    pub accepted: bool,
    pub available: u32,
}

impl IoOutcome {
    pub(crate) fn completed(available: u32) -> Self {
        Self {
            accepted: true,
            available,
        }
    }
}
