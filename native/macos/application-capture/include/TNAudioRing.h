#ifndef TN_AUDIO_RING_H
#define TN_AUDIO_RING_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct TNAudioRing TNAudioRing;

enum {
    TN_AUDIO_RING_MAX_CAPACITY_FRAMES = 1U << 20U,
};

typedef void *(*TNAudioRingAllocate)(void *context, size_t count, size_t size);
typedef void (*TNAudioRingDeallocate)(void *context, void *memory);

typedef struct TNAudioRingAllocator {
    void *context;
    TNAudioRingAllocate allocate;
    TNAudioRingDeallocate deallocate;
} TNAudioRingAllocator;

TNAudioRing *tn_audio_ring_create(size_t capacity_frames, uint32_t channels);
TNAudioRing *tn_audio_ring_create_with_allocator(
    size_t capacity_frames,
    uint32_t channels,
    TNAudioRingAllocator allocator
);
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
size_t tn_audio_ring_available_frames(const TNAudioRing *ring);
uint64_t tn_audio_ring_dropped_frames(const TNAudioRing *ring);

#endif
