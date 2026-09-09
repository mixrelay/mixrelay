#include "TNAudioRing.h"

#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>

struct TNAudioRing {
    float *samples;
    size_t capacity_frames;
    size_t frame_mask;
    uint32_t channels;
    _Atomic uint64_t read_frame;
    _Atomic uint64_t write_frame;
    _Atomic uint64_t dropped_frames;
    TNAudioRingAllocator allocator;
};

static bool tn_is_power_of_two(size_t value) {
    return value != 0 && (value & (value - 1)) == 0;
}

static void *tn_default_allocate(void *context, size_t count, size_t size) {
    (void)context;
    return calloc(count, size);
}

static void tn_default_deallocate(void *context, void *memory) {
    (void)context;
    free(memory);
}

TNAudioRing *tn_audio_ring_create(size_t capacity_frames, uint32_t channels) {
    const TNAudioRingAllocator allocator = {
        .context = NULL,
        .allocate = tn_default_allocate,
        .deallocate = tn_default_deallocate,
    };
    return tn_audio_ring_create_with_allocator(capacity_frames, channels, allocator);
}

TNAudioRing *tn_audio_ring_create_with_allocator(
    size_t capacity_frames,
    uint32_t channels,
    TNAudioRingAllocator allocator
) {
    if (!tn_is_power_of_two(capacity_frames) ||
        capacity_frames > TN_AUDIO_RING_MAX_CAPACITY_FRAMES || channels == 0 ||
        channels > 64 || allocator.allocate == NULL || allocator.deallocate == NULL) {
        return NULL;
    }

    TNAudioRing *ring = allocator.allocate(allocator.context, 1, sizeof(*ring));
    if (ring == NULL) {
        return NULL;
    }
    ring->allocator = allocator;
    ring->samples = allocator.allocate(
        allocator.context,
        capacity_frames * (size_t)channels,
        sizeof(float)
    );
    if (ring->samples == NULL) {
        allocator.deallocate(allocator.context, ring);
        return NULL;
    }
    ring->capacity_frames = capacity_frames;
    ring->frame_mask = capacity_frames - 1;
    ring->channels = channels;
    return ring;
}

void tn_audio_ring_destroy(TNAudioRing *ring) {
    if (ring == NULL) {
        return;
    }
    ring->allocator.deallocate(ring->allocator.context, ring->samples);
    ring->allocator.deallocate(ring->allocator.context, ring);
}

static bool tn_audio_ring_reserve_write(
    TNAudioRing *ring,
    size_t frames,
    uint64_t *write_frame
) {
    if (ring == NULL || frames == 0) {
        return false;
    }
    const uint64_t write = atomic_load_explicit(&ring->write_frame, memory_order_relaxed);
    const uint64_t read = atomic_load_explicit(&ring->read_frame, memory_order_acquire);
    const uint64_t available = ring->capacity_frames - (write - read);
    if (frames > available) {
        atomic_fetch_add_explicit(&ring->dropped_frames, frames, memory_order_relaxed);
        return false;
    }
    *write_frame = write;
    return true;
}

size_t tn_audio_ring_write_interleaved(
    TNAudioRing *ring,
    const float *samples,
    size_t frames
) {
    uint64_t write_frame = 0;
    if (samples == NULL || !tn_audio_ring_reserve_write(ring, frames, &write_frame)) {
        return 0;
    }

    const size_t start_frame = (size_t)write_frame & ring->frame_mask;
    const size_t first_frames = frames < ring->capacity_frames - start_frame
        ? frames
        : ring->capacity_frames - start_frame;
    const size_t first_samples = first_frames * ring->channels;
    memcpy(
        &ring->samples[start_frame * ring->channels],
        samples,
        first_samples * sizeof(float)
    );
    if (first_frames < frames) {
        const size_t remaining_samples = (frames - first_frames) * ring->channels;
        memcpy(ring->samples, &samples[first_samples], remaining_samples * sizeof(float));
    }
    atomic_store_explicit(&ring->write_frame, write_frame + frames, memory_order_release);
    return frames;
}

size_t tn_audio_ring_write_planar(
    TNAudioRing *ring,
    const float *const *channel_samples,
    size_t frames
) {
    uint64_t write_frame = 0;
    if (channel_samples == NULL || !tn_audio_ring_reserve_write(ring, frames, &write_frame)) {
        return 0;
    }
    for (uint32_t channel = 0; channel < ring->channels; ++channel) {
        if (channel_samples[channel] == NULL) {
            atomic_fetch_add_explicit(&ring->dropped_frames, frames, memory_order_relaxed);
            return 0;
        }
    }
    for (size_t frame = 0; frame < frames; ++frame) {
        const size_t destination_frame = ((size_t)write_frame + frame) & ring->frame_mask;
        for (uint32_t channel = 0; channel < ring->channels; ++channel) {
            ring->samples[destination_frame * ring->channels + channel] =
                channel_samples[channel][frame];
        }
    }
    atomic_store_explicit(&ring->write_frame, write_frame + frames, memory_order_release);
    return frames;
}

size_t tn_audio_ring_read_interleaved(
    TNAudioRing *ring,
    float *destination,
    size_t maximum_frames
) {
    if (ring == NULL || destination == NULL || maximum_frames == 0) {
        return 0;
    }
    const uint64_t read_frame = atomic_load_explicit(&ring->read_frame, memory_order_relaxed);
    const uint64_t write_frame = atomic_load_explicit(&ring->write_frame, memory_order_acquire);
    const size_t available = (size_t)(write_frame - read_frame);
    const size_t frames = maximum_frames < available ? maximum_frames : available;
    const size_t start_frame = (size_t)read_frame & ring->frame_mask;
    const size_t first_frames = frames < ring->capacity_frames - start_frame
        ? frames
        : ring->capacity_frames - start_frame;
    const size_t first_samples = first_frames * ring->channels;
    memcpy(
        destination,
        &ring->samples[start_frame * ring->channels],
        first_samples * sizeof(float)
    );
    if (first_frames < frames) {
        const size_t remaining_samples = (frames - first_frames) * ring->channels;
        memcpy(&destination[first_samples], ring->samples, remaining_samples * sizeof(float));
    }
    atomic_store_explicit(&ring->read_frame, read_frame + frames, memory_order_release);
    return frames;
}

size_t tn_audio_ring_capacity_frames(const TNAudioRing *ring) {
    return ring == NULL ? 0 : ring->capacity_frames;
}

size_t tn_audio_ring_available_frames(const TNAudioRing *ring) {
    if (ring == NULL) {
        return 0;
    }
    const uint64_t read = atomic_load_explicit(&ring->read_frame, memory_order_acquire);
    const uint64_t write = atomic_load_explicit(&ring->write_frame, memory_order_acquire);
    return (size_t)(write - read);
}

uint64_t tn_audio_ring_dropped_frames(const TNAudioRing *ring) {
    return ring == NULL
        ? 0
        : atomic_load_explicit(&ring->dropped_frames, memory_order_relaxed);
}
