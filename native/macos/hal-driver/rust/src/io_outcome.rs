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

#[derive(Clone, Copy)]
pub(crate) enum Direction {
    Capture,
    ClientOutput(u32),
}

#[derive(Clone, Copy)]
pub(crate) struct Transfer {
    pub direction: Direction,
    pub frame: u64,
    pub samples: *mut std::ffi::c_void,
    pub frames: u32,
}

#[derive(Clone, Copy)]
#[repr(u32)]
pub(crate) enum ClientChange {
    Register,
    Remove = 3,
}

impl ClientChange {
    pub(crate) const fn from_raw(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Register),
            3 => Some(Self::Remove),
            _ => None,
        }
    }
}
