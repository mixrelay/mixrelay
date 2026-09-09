#include "TNAudioRing.h"

#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

typedef struct FailingAllocatorState {
    size_t calls;
    size_t fail_on_call;
} FailingAllocatorState;

static void *failing_allocate(void *context, size_t count, size_t size) {
    FailingAllocatorState *const state = context;
    ++state->calls;
    return state->calls == state->fail_on_call ? NULL : calloc(count, size);
}

static void failing_deallocate(void *context, void *memory) {
    (void)context;
    free(memory);
}

static void test_rejects_invalid_configuration(void) {
    assert(tn_audio_ring_create(0, 2) == NULL);
    assert(tn_audio_ring_create(3, 2) == NULL);
    assert(tn_audio_ring_create(8, 0) == NULL);
    assert(tn_audio_ring_create(8, 65) == NULL);
    assert(tn_audio_ring_create((size_t)TN_AUDIO_RING_MAX_CAPACITY_FRAMES * 2U, 2) == NULL);
    const TNAudioRingAllocator missing_allocate = {0};
    assert(tn_audio_ring_create_with_allocator(8, 2, missing_allocate) == NULL);
    const TNAudioRingAllocator missing_deallocate = {
        .context = NULL,
        .allocate = failing_allocate,
        .deallocate = NULL,
    };
    assert(tn_audio_ring_create_with_allocator(8, 2, missing_deallocate) == NULL);

    FailingAllocatorState state = {.calls = 0U, .fail_on_call = 1U};
    TNAudioRingAllocator allocator = {
        .context = &state,
        .allocate = failing_allocate,
        .deallocate = failing_deallocate,
    };
    assert(tn_audio_ring_create_with_allocator(8, 2, allocator) == NULL);
    state.calls = 0U;
    state.fail_on_call = 2U;
    assert(tn_audio_ring_create_with_allocator(8, 2, allocator) == NULL);
    assert(tn_audio_ring_capacity_frames(NULL) == 0);
    assert(tn_audio_ring_available_frames(NULL) == 0);
    assert(tn_audio_ring_dropped_frames(NULL) == 0);
    tn_audio_ring_destroy(NULL);
}

static void test_interleaved_wraparound_and_bounded_overflow(void) {
    TNAudioRing *ring = tn_audio_ring_create(4, 2);
    assert(ring != NULL);
    assert(tn_audio_ring_capacity_frames(ring) == 4);

    const float first[] = {1, 2, 3, 4, 5, 6};
    assert(tn_audio_ring_write_interleaved(ring, first, 3) == 3);
    assert(tn_audio_ring_available_frames(ring) == 3);

    float output[8] = {0};
    assert(tn_audio_ring_read_interleaved(ring, output, 2) == 2);
    assert(output[0] == 1 && output[1] == 2 && output[2] == 3 && output[3] == 4);

    const float second[] = {7, 8, 9, 10, 11, 12};
    assert(tn_audio_ring_write_interleaved(ring, second, 3) == 3);
    assert(tn_audio_ring_write_interleaved(ring, second, 1) == 0);
    assert(tn_audio_ring_dropped_frames(ring) == 1);

    assert(tn_audio_ring_read_interleaved(ring, output, 4) == 4);
    const float expected[] = {5, 6, 7, 8, 9, 10, 11, 12};
    for (size_t index = 0; index < 8; ++index) {
        assert(output[index] == expected[index]);
    }
    assert(tn_audio_ring_available_frames(ring) == 0);
    tn_audio_ring_destroy(ring);
}

static void test_planar_conversion_and_invalid_calls(void) {
    TNAudioRing *ring = tn_audio_ring_create(4, 2);
    const float left[] = {1, 2};
    const float right[] = {3, 4};
    const float *channels[] = {left, right};
    assert(tn_audio_ring_write_planar(ring, channels, 2) == 2);

    float output[4] = {0};
    assert(tn_audio_ring_read_interleaved(ring, output, 2) == 2);
    assert(output[0] == 1 && output[1] == 3 && output[2] == 2 && output[3] == 4);

    const float *invalid_channels[] = {left, NULL};
    assert(tn_audio_ring_write_planar(ring, invalid_channels, 1) == 0);
    assert(tn_audio_ring_dropped_frames(ring) == 1);
    assert(tn_audio_ring_write_planar(ring, NULL, 1) == 0);
    assert(tn_audio_ring_write_planar(ring, channels, 0) == 0);
    assert(tn_audio_ring_write_interleaved(ring, NULL, 1) == 0);
    assert(tn_audio_ring_write_interleaved(NULL, left, 1) == 0);
    assert(tn_audio_ring_read_interleaved(ring, NULL, 1) == 0);
    assert(tn_audio_ring_read_interleaved(ring, output, 0) == 0);
    assert(tn_audio_ring_read_interleaved(NULL, output, 1) == 0);
    assert(tn_audio_ring_write_interleaved(ring, left, 0) == 0);
    tn_audio_ring_destroy(ring);
}

int main(void) {
    test_rejects_invalid_configuration();
    test_interleaved_wraparound_and_bounded_overflow();
    test_planar_conversion_and_invalid_calls();
    puts("TNAudioRingTests passed");
    return 0;
}
