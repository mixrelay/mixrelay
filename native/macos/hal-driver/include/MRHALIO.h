#ifndef MR_HAL_IO_H
#define MR_HAL_IO_H

#include "MREndpointTransport.h"

#include <stdbool.h>
#include <stdatomic.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct MRHALIOContext {
    _Atomic uint32_t render_activity;
    uint64_t clock_epoch;
    uint32_t clock_sample_rate;
    bool clock_running;
    _Atomic(MREndpointTransport *) transport;
    _Atomic(MRSharedAudioBuffer *) render_buffer;
    _Atomic(void *) capture_buffer; /* Rust-owned bounded loopback storage. */
    uint32_t render_channels;
    uint32_t capture_channels;
} MRHALIOContext;

/*
 * Initializes a caller-owned context. The context allocates and locks nothing.
 * Binding and unbinding are control-thread operations and must happen while
 * the endpoint has no in-flight IO callbacks.
 */
bool mr_hal_io_initialize(MRHALIOContext *context,
                          uint32_t render_channels,
                          uint32_t capture_channels);
bool mr_hal_io_bind(MRHALIOContext *context,
                    MREndpointTransport *transport);
void mr_hal_io_unbind(MRHALIOContext *context);
bool mr_hal_io_is_lock_free(const MRHALIOContext *context);
/* Control-plane observation: clock or either PCM epoch has exhausted this
 * mapping. Ordinary pending rearm does not request a new transport. */
bool mr_hal_io_requires_replacement(const MRHALIOContext *context);

/* Invalidates the preceding Core Audio timeline before the first client starts. */
bool mr_hal_io_begin_stream(MRHALIOContext *context);

/* Control-only lifecycle state, retained when no transport is attached. */
void mr_hal_io_clock_state(MRHALIOContext *context, uint32_t sample_rate, bool running);

/* Called by native lifecycle notifications; UNKNOWN when a remaining client's
 * direction is unknown, INACTIVE after the last client stops. No audio inference. */
void mr_hal_io_set_render_activity(MRHALIOContext *context,
                                   MRProducerActivity activity);

/* Converts Core Audio's integral sample-time domain without losing precision. */
bool mr_hal_io_sample_frame(double sample_time, uint64_t *sample_frame);

/* OS-mixed output -> HAL render and capture rings at the exact sample frame. */
bool mr_hal_io_write_render(MRHALIOContext *context,
                            uint64_t start_frame,
                            const float *interleaved_samples,
                            uint32_t frame_count);

/* HAL mixed capture ring -> OS input. Missing or stale data becomes silence. */
uint32_t mr_hal_io_read_capture(MRHALIOContext *context,
                                uint64_t start_frame,
                                float *interleaved_samples,
                                uint32_t frame_count);

#ifdef __cplusplus
}
#endif

#endif
