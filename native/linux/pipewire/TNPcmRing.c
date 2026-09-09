#ifndef _GNU_SOURCE
#define _GNU_SOURCE
#endif
#include "TNPcmRing.h"
#include <sys/mman.h>
#include <stdlib.h>
#include <string.h>

bool tn_pcm_ring_init(TNPcmRing *r, uint32_t channels, uint32_t capacity) {
    if (!r || !channels || channels > 64 || capacity < 4 || capacity > 65536)
        return false;
    memset(r, 0, sizeof(*r));
    atomic_init(&r->written, 0);
    atomic_init(&r->read, 0);
    atomic_init(&r->quantum, 0);
    atomic_init(&r->overflows, 0);
    atomic_init(&r->starvations, 0);
    atomic_init(&r->requested_epoch, 0);
    atomic_init(&r->published_epoch, 0);
    atomic_init(&r->epoch_start, 0);
    if (!atomic_is_lock_free(&r->written) || !atomic_is_lock_free(&r->read) ||
        !atomic_is_lock_free(&r->quantum) || !atomic_is_lock_free(&r->overflows))
        return false;
    /* These large, zero-initialized PCM slabs have an explicit native lifetime.
     * Unmapping on retirement returns them to the OS instead of relying on
     * per-thread malloc arenas to release repeated channel-reconfiguration blocks.
     * Creation/destruction remain control-thread-only, never in process callbacks. */
    void *samples = mmap(NULL, (size_t)capacity * channels * sizeof(float),
                         PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    r->samples = samples == MAP_FAILED ? NULL : samples;
    r->channels = channels;
    r->capacity = capacity;
    return r->samples != NULL;
}

void tn_pcm_ring_clear(TNPcmRing *r) {
    if (r->samples) munmap(r->samples, (size_t)r->capacity * r->channels * sizeof(float));
    r->samples = NULL;
}

void tn_pcm_ring_discard(TNPcmRing *r) {
    /* Consumer advances only its own cursor. Published frames prepared before
     * activation are stale; a concurrent producer may publish new frames later. */
    atomic_store_explicit(&r->read, atomic_load_explicit(&r->written, memory_order_acquire), memory_order_release);
    r->primed = false;
}

void tn_pcm_ring_restart(TNPcmRing *r) {
    /* No cursor changes and no wait for either real-time owner. A write that
     * was already in flight belongs to its old epoch and cannot leak on resume. */
    atomic_fetch_add_explicit(&r->requested_epoch, 1, memory_order_release);
}

uint32_t tn_pcm_ring_write(TNPcmRing *r, const float *src, uint32_t frames) {
    uint64_t written = atomic_load_explicit(&r->written, memory_order_relaxed);
    uint64_t epoch = atomic_load_explicit(&r->requested_epoch, memory_order_acquire);
    if (epoch != r->writer_epoch) {
        atomic_store_explicit(&r->epoch_start, written, memory_order_relaxed);
        atomic_store_explicit(&r->published_epoch, epoch, memory_order_release);
        r->writer_epoch = epoch;
    }
    uint64_t read = atomic_load_explicit(&r->read, memory_order_acquire);
    if (!frames) return 0;
    if (frames > r->capacity / 2 || written - read > r->capacity - frames) {
        atomic_fetch_add_explicit(&r->overflows, 1, memory_order_relaxed);
        return 0; /* retain every unread frame; never race the consumer */
    }
    uint32_t quantum = atomic_load_explicit(&r->quantum, memory_order_relaxed);
    if (frames > quantum)
        atomic_store_explicit(&r->quantum, frames, memory_order_relaxed);
    uint32_t start = (uint32_t)(written % r->capacity);
    uint32_t first = frames < r->capacity - start ? frames : r->capacity - start;
    memcpy(r->samples + (size_t)start * r->channels, src,
           (size_t)first * r->channels * sizeof(float));
    memcpy(r->samples, src + (size_t)first * r->channels,
           (size_t)(frames - first) * r->channels * sizeof(float));
    atomic_store_explicit(&r->written, written + frames, memory_order_release);
    return frames;
}

static uint32_t read_pcm(TNPcmRing *r, float *dst, uint32_t frames, bool prefill) {
    uint64_t epoch = atomic_load_explicit(&r->published_epoch, memory_order_acquire);
    uint64_t epoch_start = atomic_load_explicit(&r->epoch_start, memory_order_relaxed);
    if (epoch != atomic_load_explicit(&r->requested_epoch, memory_order_acquire)) {
        memset(dst, 0, (size_t)frames * r->channels * sizeof(float));
        r->primed = false;
        return 0;
    }
    if (epoch != r->reader_epoch) {
        uint64_t previous = atomic_load_explicit(&r->read, memory_order_relaxed);
        /* An explicit consumer discard may already have crossed this boundary.
         * Epoch acknowledgement must never rewind and replay discarded samples. */
        if (epoch_start > previous)
            atomic_store_explicit(&r->read, epoch_start, memory_order_release);
        r->reader_epoch = epoch;
        r->primed = false;
    }
    uint64_t read = atomic_load_explicit(&r->read, memory_order_relaxed);
    uint64_t written = atomic_load_explicit(&r->written, memory_order_acquire);
    uint32_t quantum = atomic_load_explicit(&r->quantum, memory_order_relaxed);
    uint32_t reserve = 2 * (frames > quantum ? frames : quantum);
    if (reserve > r->capacity) reserve = r->capacity;
    /* Independent callbacks in the same server clock have a scheduling phase.
     * Prefill two native quantums; do not confuse graph sub-blocks with native
     * quantum. On starvation re-prime explicitly instead of replaying samples. */
    if ((prefill && !r->primed && written - read < reserve) || written - read < frames) {
        memset(dst, 0, (size_t)frames * r->channels * sizeof(float));
        if (r->primed)
            atomic_fetch_add_explicit(&r->starvations, 1, memory_order_relaxed);
        r->primed = false;
        return 0;
    }
    r->primed = true;
    uint32_t start = (uint32_t)(read % r->capacity);
    uint32_t first = frames < r->capacity - start ? frames : r->capacity - start;
    memcpy(dst, r->samples + (size_t)start * r->channels,
           (size_t)first * r->channels * sizeof(float));
    memcpy(dst + (size_t)first * r->channels, r->samples,
           (size_t)(frames - first) * r->channels * sizeof(float));
    atomic_store_explicit(&r->read, read + frames, memory_order_release);
    /* A concurrent pause invalidates this in-flight read as well. Advancing our
     * cursor is harmless: the next epoch's start is at or beyond these frames. */
    if (epoch != atomic_load_explicit(&r->requested_epoch, memory_order_acquire)) {
        memset(dst, 0, (size_t)frames * r->channels * sizeof(float));
        r->primed = false;
        return 0;
    }
    return frames;
}

uint32_t tn_pcm_ring_read(TNPcmRing *r, float *dst, uint32_t frames) {
    return read_pcm(r, dst, frames, true);
}
uint32_t tn_pcm_ring_read_available(TNPcmRing *r, float *dst, uint32_t frames) {
    return read_pcm(r, dst, frames, false);
}
