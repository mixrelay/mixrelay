#ifndef TN_AUDIO_RING_H
#define TN_AUDIO_RING_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

/* Rust-owned opaque SPSC ring. One serialized IOProc writes; one pump reads.
 * Destroy only after callback, consumer and stat-reader access has drained. */
typedef struct TNAudioRing TNAudioRing;

enum {
    TN_AUDIO_RING_MAX_CAPACITY_FRAMES = 1U << 20U,
};

TNAudioRing *tn_audio_ring_create(size_t capacity_frames, uint32_t channels);
void tn_audio_ring_destroy(TNAudioRing *ring);

size_t tn_audio_ring_write_interleaved(
    TNAudioRing *ring,
    const float *samples,
    size_t frames
);
size_t tn_audio_ring_write_planar(
    TNAudioRing *ring,
    const float *const *channel_samples,
    size_t frames
);
size_t tn_audio_ring_read_interleaved(
    TNAudioRing *ring,
    float *destination,
    size_t maximum_frames
);

size_t tn_audio_ring_capacity_frames(const TNAudioRing *ring);
/* Bounded diagnostic observation, not an atomic pair of producer/consumer cursors. */
size_t tn_audio_ring_available_frames(const TNAudioRing *ring);
uint64_t tn_audio_ring_dropped_frames(const TNAudioRing *ring);

#endif
