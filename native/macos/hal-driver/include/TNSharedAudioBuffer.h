#ifndef TN_SHARED_AUDIO_BUFFER_H
#define TN_SHARED_AUDIO_BUFFER_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

enum {
    TN_SHARED_AUDIO_BUFFER_MAX_CHANNELS = 64,
    TN_SHARED_AUDIO_BUFFER_MAX_CAPACITY_FRAMES = 1U << 20U,
};

typedef struct TNSharedAudioBuffer TNSharedAudioBuffer;

/* Explicit producer evidence; Unknown means no current producer observation. */
typedef enum TNProducerActivity {
    TN_PRODUCER_ACTIVITY_UNKNOWN = 0,
    TN_PRODUCER_ACTIVITY_INACTIVE = 1,
    TN_PRODUCER_ACTIVITY_AWAITING_RENDER = 2,
    TN_PRODUCER_ACTIVITY_ACTIVE = 3,
} TNProducerActivity;

void tn_shared_audio_buffer_set_producer_activity(TNSharedAudioBuffer *buffer,
                                                  TNProducerActivity activity);
TNProducerActivity tn_shared_audio_buffer_producer_activity(
    const TNSharedAudioBuffer *buffer);

typedef struct TNSharedAudioBufferStats {
    uint64_t written_frames;
    uint64_t read_frames;
    uint64_t missing_frames;
    uint64_t epoch;
    uint64_t published_through_frame;
} TNSharedAudioBufferStats;

size_t tn_shared_audio_buffer_required_bytes(uint32_t capacity_frames,
                                             uint32_t channels);

/*
 * Initializes a caller-owned, suitably aligned memory region. The region may
 * be backed by anonymous memory for tests or by a shared mmap in production.
 * The current schema has an 80-byte header (version 4), including a bounded
 * producer claim. Allocate via required_bytes; no prior wire schema is accepted.
 */
TNSharedAudioBuffer *tn_shared_audio_buffer_initialize(void *memory,
                                                       size_t memory_size,
                                                       uint32_t capacity_frames,
                                                       uint32_t channels);

/*
 * Validates and attaches to an initialized region without allocating or
 * storing process-local pointers inside the shared layout.
 */
TNSharedAudioBuffer *tn_shared_audio_buffer_attach(void *memory,
                                                   size_t memory_size);

bool tn_shared_audio_buffer_is_lock_free(const TNSharedAudioBuffer *buffer);

uint32_t tn_shared_audio_buffer_capacity(const TNSharedAudioBuffer *buffer);
uint32_t tn_shared_audio_buffer_channels(const TNSharedAudioBuffer *buffer);
uint64_t tn_shared_audio_buffer_published_through(
    const TNSharedAudioBuffer *buffer);

/* Bounded writer-side invalidation. One producer claim and one epoch attempt,
 * no spinning or storage clearing. A busy producer is marked for rearm.
 * Returns zero on contention or epoch exhaustion; retry on a later callback or
 * replace the transport on control. No generation is ever reused. */
uint64_t tn_shared_audio_buffer_try_begin_epoch(TNSharedAudioBuffer *buffer);

/* Reader-side restart request; one atomic mark, no clearing or generation reuse.
 * Marked transports read silence until their sole writer rearms or replaces them. */
uint64_t tn_shared_audio_buffer_request_epoch(TNSharedAudioBuffer *buffer);
bool tn_shared_audio_buffer_epoch_pending(const TNSharedAudioBuffer *buffer);
/* True after generation exhaustion or retirement of a dead producer. The control
 * owner must replace the mapping; normal pending rearm is not retirement. */
bool tn_shared_audio_buffer_requires_replacement(const TNSharedAudioBuffer *buffer);
/* Sole writer/control ownership is required: call only after all its callbacks
 * have drained or its predecessor process exited. Retires a remaining busy
 * claim without clearing/reusing storage. Never use against a live remote writer. */
bool tn_shared_audio_buffer_retire_busy_producer_after_drain(TNSharedAudioBuffer *buffer);

/*
 * Reads and writes interleaved Float32 PCM at an absolute sample-frame index.
 * Both functions are allocation-free and never wait. A read always initializes
 * the complete destination; frames that have not been published for the
 * current epoch and absolute frame index are returned as silence. Write claims
 * the producer before comparing expected_epoch; an observation invalidated by
 * rearm cannot publish its PCM into a new epoch. A failed claim/write is retried
 * only with a fresh valid snapshot by the caller, never via an unchecked path.
 */
bool tn_shared_audio_buffer_write(TNSharedAudioBuffer *buffer,
                                  uint64_t expected_epoch,
                                  uint64_t start_frame,
                                  const float *interleaved_samples,
                                  uint32_t frame_count);
uint32_t tn_shared_audio_buffer_read(TNSharedAudioBuffer *buffer,
                                     uint64_t start_frame,
                                     float *interleaved_samples,
                                     uint32_t frame_count);

TNSharedAudioBufferStats tn_shared_audio_buffer_stats(
    const TNSharedAudioBuffer *buffer);

#ifdef __cplusplus
}
#endif

#endif
