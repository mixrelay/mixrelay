//! Build-time C declarations for the small native ABI boundary.
//! Imported by Cargo build scripts and the standalone HAL build example. This
//! module deliberately has no dependency on audio processing or OS libraries.

use crate::formats;

/// Generates declarations from the same constants used by Rust validation.
pub(super) fn render() -> String {
    // Fail the native build too if product policy is accidentally expanded or
    // allocation bounds no longer describe the canonical buffer catalog.
    assert!(
        formats::SAMPLE_RATES
            .windows(2)
            .all(|pair| pair[0] < pair[1])
    );
    assert!(
        formats::SAMPLE_RATES
            .iter()
            .all(|rate| *rate > 0 && *rate <= 192_000)
    );
    assert!(
        formats::BUFFER_SIZES
            .windows(2)
            .all(|pair| pair[0] < pair[1])
    );
    assert!(
        formats::BUFFER_SIZES
            .iter()
            .all(|frames| (32..=2048).contains(frames) && frames.is_power_of_two())
    );
    assert_eq!(formats::MAX_BUFFER_FRAMES, 2048);
    let rates = formats::SAMPLE_RATES
        .iter()
        .map(|rate| format!("{rate}U"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r"/* Generated from tonenari-audio-io/src/formats.rs. Do not edit. */
#ifndef TN_AUDIO_FORMATS_H
#define TN_AUDIO_FORMATS_H
#include <stdbool.h>
#include <stdint.h>
enum {{ TN_AUDIO_SAMPLE_RATE_COUNT = {} }};
static const uint32_t TN_AUDIO_SAMPLE_RATES[TN_AUDIO_SAMPLE_RATE_COUNT] = {{{rates}}};
static inline bool tn_audio_sample_rate_supported(double rate) {{
    for (uint32_t i = 0; i < TN_AUDIO_SAMPLE_RATE_COUNT; ++i)
        if (rate == (double)TN_AUDIO_SAMPLE_RATES[i]) return true;
    return false;
}}
#endif
",
        formats::SAMPLE_RATES.len()
    )
}

/// Places the generated header in the caller's isolated build output directory.
pub(super) fn write_header(directory: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    std::fs::create_dir_all(directory)?;
    let path = directory.join("TNAudioFormats.h");
    std::fs::write(&path, render())?;
    Ok(path)
}
