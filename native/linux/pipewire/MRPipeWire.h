#ifndef MR_PIPEWIRE_H
#define MR_PIPEWIRE_H
#include <stdint.h>

typedef struct MRPWStream MRPWStream;
typedef struct MRPWDriver MRPWDriver;
typedef struct {
    uint64_t position, monotonic_ns;
    uint32_t clock_id, rate, frames;
} MRPWCycle;
typedef void (*MRPWProcess)(void *userdata, const MRPWCycle *cycle);

/* Control thread only. mode: 0 virtual sink, 1 virtual source, 2 application tap,
 * 3 device input, 4 device output.
 * target is a registry object.serial, not a recyclable object id. */
MRPWStream *mr_pw_stream_open(const char *name, const char *description,
                             const char *target, uint32_t mode, uint32_t channels,
                             uint32_t sample_rate, uint32_t block_frames);
void mr_pw_stream_close(MRPWStream *stream);
int mr_pw_stream_ready(MRPWStream *stream);
int mr_pw_stream_connected(MRPWStream *stream);
/* One native state snapshot: 0 unknown/unavailable, 1 paused, 2 streaming. */
int mr_pw_stream_input_activity(MRPWStream *stream);
/* Actual callback driver ID (high32) and rate (low32); zero means unobserved. */
uint64_t mr_pw_stream_clock(MRPWStream *stream);
/* Negotiated PCM rate (high32) and channels (low32); zero means unobserved. */
uint64_t mr_pw_stream_format(MRPWStream *stream);
/* Last callback graph frames (high32) and stream PCM frames (low32).
 * Variable quantum observation, NOT a negotiated fixed BufferFrameSize setting. */
uint64_t mr_pw_stream_quantum(MRPWStream *stream);
void mr_pw_stream_discard_input(MRPWStream *stream);
typedef struct { uint64_t overflows, starvations; } MRPWTransferCounters;
MRPWTransferCounters mr_pw_stream_counters(MRPWStream *stream);
/* DSP thread only: no locks, allocation, file I/O or PipeWire control calls. */
uint32_t mr_pw_stream_read(MRPWStream *stream, float *samples, uint32_t frames);
uint32_t mr_pw_stream_write(MRPWStream *stream, const float *samples, uint32_t frames);

/* userdata must remain pinned until close returns. Close disconnects and drains
 * RT callbacks before destroying the loop or releasing userdata. */
MRPWDriver *mr_pw_driver_open(MRPWProcess process, void *userdata);
int mr_pw_driver_ready(MRPWDriver *driver);
void mr_pw_driver_close(MRPWDriver *driver);
#endif
