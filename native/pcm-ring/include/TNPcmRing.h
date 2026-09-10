#ifndef TN_PCM_RING_H
#define TN_PCM_RING_H
#include <stdbool.h>
#include <stdint.h>
/* Embedded Rust owner. Zeroed storage is empty. Never copy a live ring;
 * clear only after producer, consumer and diagnostic readers have stopped. */
typedef struct { uint64_t storage[15]; } TNPcmRing;
_Static_assert(sizeof(TNPcmRing) == 120 && _Alignof(TNPcmRing) == 8, "PCM owner ABI");
typedef struct {
    uint64_t read, written, overflows, starvations;
    uint32_t channels, queued;
} TNPcmRingStats;
_Static_assert(sizeof(TNPcmRingStats) == 40 && _Alignof(TNPcmRingStats) == 8, "PCM stats ABI");
bool tn_pcm_ring_init(TNPcmRing *ring, uint32_t channels, uint32_t capacity);
void tn_pcm_ring_clear(TNPcmRing *ring);
uint32_t tn_pcm_ring_write(TNPcmRing *ring, const float *samples, uint32_t frames);
uint32_t tn_pcm_ring_read(TNPcmRing *ring, float *samples, uint32_t frames);
/* Caller owns clock-crossing priming; epoch checks and whole-read silence remain. */
uint32_t tn_pcm_ring_read_available(TNPcmRing *ring, float *samples, uint32_t frames);
void tn_pcm_ring_discard(TNPcmRing *ring); /* sole consumer only */
void tn_pcm_ring_restart(TNPcmRing *ring); /* control request, no wait */
TNPcmRingStats tn_pcm_ring_stats(const TNPcmRing *ring); /* independent observations */
#endif
