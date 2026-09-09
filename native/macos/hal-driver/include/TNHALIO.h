#ifndef TN_HAL_IO_H
#define TN_HAL_IO_H

#include "TNEndpointTransport.h"

#include <stdbool.h>
#include <stdatomic.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct TNHALIOContext {
    _Atomic uint32_t render_activity;
    uint64_t clock_epoch;
    uint32_t clock_sample_rate;
    bool clock_running;
    _Atomic(TNEndpointTransport *) transport;
    _Atomic(TNSharedAudioBuffer *) render_buffer;
    _Atomic(TNSharedAudioBuffer *) capture_buffer;
    uint32_t render_channels;
    uint32_t capture_channels;
} TNHALIOContext;

/*
 * Initializes a caller-owned context. The context allocates and locks nothing.
 * Binding and unbinding are control-thread operations and must happen while
 * the endpoint has no in-flight IO callbacks.
 */
bool tn_hal_io_initialize(TNHALIOContext *context,
                          uint32_t render_channels,
                          uint32_t capture_channels);
bool tn_hal_io_bind(TNHALIOContext *context,
                    TNEndpointTransport *transport);
void tn_hal_io_unbind(TNHALIOContext *context);
bool tn_hal_io_is_lock_free(const TNHALIOContext *context);
/* Control-plane observation: clock or either PCM epoch has exhausted this
 * mapping. Ordinary pending rearm does not request a new transport. */
bool tn_hal_io_requires_replacement(const TNHALIOContext *context);

/* Invalidates the preceding Core Audio timeline before the first client starts. */
bool tn_hal_io_begin_stream(TNHALIOContext *context);

/* Control-only lifecycle state, retained when no transport is attached. */
void tn_hal_io_clock_state(TNHALIOContext *context, uint32_t sample_rate, bool running);

/* Called by native lifecycle notifications; UNKNOWN when a remaining client's
 * direction is unknown, INACTIVE after the last client stops. No audio inference. */
void tn_hal_io_set_render_activity(TNHALIOContext *context,
                                   TNProducerActivity activity);

/* Converts Core Audio's integral sample-time domain without losing precision. */
bool tn_hal_io_sample_frame(double sample_time, uint64_t *sample_frame);

/* OS output -> ToneNari engine render buffer. */
bool tn_hal_io_write_render(TNHALIOContext *context,
                            uint64_t start_frame,
                            const float *interleaved_samples,
                            uint32_t frame_count);

/* ToneNari engine capture buffer -> OS input. Missing data becomes silence. */
uint32_t tn_hal_io_read_capture(TNHALIOContext *context,
                                uint64_t start_frame,
                                float *interleaved_samples,
                                uint32_t frame_count);

#ifdef __cplusplus
}
#endif

#endif
