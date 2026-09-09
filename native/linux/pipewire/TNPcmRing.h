#ifndef TN_PCM_RING_H
#define TN_PCM_RING_H
#include <stdatomic.h>
#include <stdbool.h>
#include <stdint.h>

/* Single producer / single consumer. Each side owns only its cursor; neither
 * may advance the other side to overwrite unread audio. All storage is prepared
 * before callbacks start. Frames (not bytes) are the unit at every boundary. */
typedef struct {
    float *samples;
    uint32_t channels, capacity;
    _Atomic uint64_t written, read;
    _Atomic uint32_t quantum;
    _Atomic uint64_t overflows, starvations;
    _Atomic uint64_t requested_epoch, published_epoch, epoch_start;
    uint64_t writer_epoch, reader_epoch; /* owned by the respective SPSC side */
    bool primed; /* consumer-only */
} TNPcmRing;

bool tn_pcm_ring_init(TNPcmRing *ring, uint32_t channels, uint32_t capacity);
void tn_pcm_ring_clear(TNPcmRing *ring); /* only after both sides have stopped */
uint32_t tn_pcm_ring_write(TNPcmRing *ring, const float *samples, uint32_t frames);
uint32_t tn_pcm_ring_read(TNPcmRing *ring, float *samples, uint32_t frames);
/* The caller already owns clock-crossing priming (e.g. an adaptive bridge).
 * Consume available PCM without a second prefill, retaining epoch guarantees. */
uint32_t tn_pcm_ring_read_available(TNPcmRing *ring, float *samples, uint32_t frames);
void tn_pcm_ring_discard(TNPcmRing *ring); /* consumer-only, re-prime after recovery */
/* Any control thread may invalidate the transport. The producer acknowledges
 * with a frame boundary; only the consumer advances read past obsolete PCM. */
void tn_pcm_ring_restart(TNPcmRing *ring);
#endif
