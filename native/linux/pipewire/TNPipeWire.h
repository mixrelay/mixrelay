#ifndef TN_PIPEWIRE_H
#define TN_PIPEWIRE_H
#include <stdint.h>

typedef struct TNPWStream TNPWStream;
typedef struct TNPWDriver TNPWDriver;
typedef struct {
    uint64_t position, monotonic_ns;
    uint32_t clock_id, rate, frames;
} TNPWCycle;
typedef void (*TNPWProcess)(void *userdata, const TNPWCycle *cycle);

/* Control thread only. mode: 0 virtual sink, 1 virtual source, 2 application tap,
 * 3 device input, 4 device output.
 * target is a registry object.serial, not a recyclable object id. */
TNPWStream *tn_pw_stream_open(const char *name, const char *description,
                             const char *target, uint32_t mode, uint32_t channels,
                             uint32_t sample_rate, uint32_t block_frames);
void tn_pw_stream_close(TNPWStream *stream);
int tn_pw_stream_ready(TNPWStream *stream);
int tn_pw_stream_connected(TNPWStream *stream);
/* One native state snapshot: 0 unknown/unavailable, 1 paused, 2 streaming. */
int tn_pw_stream_input_activity(TNPWStream *stream);
/* Actual callback driver ID (high32) and rate (low32); zero means unobserved. */
uint64_t tn_pw_stream_clock(TNPWStream *stream);
/* Negotiated PCM rate (high32) and channels (low32); zero means unobserved. */
uint64_t tn_pw_stream_format(TNPWStream *stream);
/* Last callback graph frames (high32) and stream PCM frames (low32).
 * Variable quantum observation, NOT a negotiated fixed BufferFrameSize setting. */
uint64_t tn_pw_stream_quantum(TNPWStream *stream);
void tn_pw_stream_discard_input(TNPWStream *stream);
typedef struct { uint64_t overflows, starvations; } TNPWTransferCounters;
TNPWTransferCounters tn_pw_stream_counters(TNPWStream *stream);
/* DSP thread only: no locks, allocation, file I/O or PipeWire control calls. */
uint32_t tn_pw_stream_read(TNPWStream *stream, float *samples, uint32_t frames);
uint32_t tn_pw_stream_write(TNPWStream *stream, const float *samples, uint32_t frames);

/* userdata must remain pinned until close returns. Close disconnects and drains
 * RT callbacks before destroying the loop or releasing userdata. */
TNPWDriver *tn_pw_driver_open(TNPWProcess process, void *userdata);
int tn_pw_driver_ready(TNPWDriver *driver);
void tn_pw_driver_close(TNPWDriver *driver);
#endif
