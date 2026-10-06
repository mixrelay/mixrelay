//! Explicit synthetic stream admission; invokes the production format validator off realtime.

use super::{
    DeviceIo,
    stream_format::{StreamFormat, StreamFormats},
};

/// Admits one interleaved packed Float32 stream matching a synthetic owner's declared geometry.
///
/// Returns `false` for null owners, registered callbacks or unsupported rates/layouts.
/// Real device preparation still requires actual `CoreAudio` stream descriptions and listeners.
///
/// # Safety
/// `io` is null or a retained, freshly created fixture owner that has never registered native I/O.
/// Its sole fixture controller excludes callbacks and graph/worker queue access during admission.
/// It supplies complete matching Float32 ABL storage to subsequent serialized callback invocations.
#[must_use]
pub unsafe fn admit_float_callback(io: *mut DeviceIo) -> bool {
    // SAFETY: fixture owns the stable allocation throughout this control-only admission.
    let Some(io) = (unsafe { io.as_ref() }) else {
        return false;
    };
    // SAFETY: no callback or other control owner accesses these cells during preparation.
    let control = unsafe { &*io.control.get() };
    if control.lifecycle.registered() {
        return false;
    }
    // SAFETY: this never-registered fixture exclusively owns its callback cell.
    let callback = unsafe { &mut *io.callback.get() };
    let mut formats = StreamFormats::default();
    if !formats.accept(
        &StreamFormat {
            rate: f64::from(control.connection.rate),
            format: u32::from_be_bytes(*b"lpcm"),
            flags: 0x09,
            bits: 32,
            channels: io.selection.channels,
            frame_bytes: io.selection.channels * 4,
        },
        callback.selection.limit,
    ) || formats.selected_rate(callback.selection).is_none()
    {
        return false;
    }
    callback.formats = formats.into_layout();
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    #[test]
    fn explicit_fixture_admission_uses_the_native_rate_and_channel_validator() {
        // SAFETY: null fixtures own no resource; each non-null owner is unpublished and drained.
        assert!(!unsafe { admit_float_callback(ptr::null_mut()) });
        for channels in [1, 2, 32, 64] {
            for (rate, accepted) in [(1, false), (8_000, true), (96_000, true), (384_001, false)] {
                let io = DeviceIo::new(c"synthetic-admission", true, channels, rate).unwrap();
                // SAFETY: this local fixture exclusively owns all cells and has no registration.
                assert_eq!(
                    unsafe { admit_float_callback(ptr::from_ref(&io).cast_mut()) },
                    accepted
                );
            }
        }
    }
}
