#ifndef TN_CALLBACK_CLOCK_H
#define TN_CALLBACK_CLOCK_H
#include <CoreAudio/CoreAudioTypes.h>
#include <math.h>
#include <stdbool.h>
#include <stdatomic.h>
#include <stdint.h>
#include <string.h>

/* One IOProc is the writer. Readers never wait for the callback, and every
 * published field is atomic (a seqlock over plain C fields would be a data race).
 * These timestamps describe the input/output buffer, NOT callback arrival time.
 * They prove continuity of one stream, never synchronization of two devices. */
typedef struct TNCallbackClockSnapshot {
    double sample_time;
    uint64_t host_time;
    uint64_t discontinuities;
    uint32_t valid;
} TNCallbackClockSnapshot;

typedef struct TNCallbackClock {
    bool primed; /* callback-owned; reset only after unregister has drained */
    double next_sample;
    uint64_t previous_host;
    _Atomic uint64_t sequence, sample_bits, host_time, discontinuities;
    _Atomic uint32_t valid;
} TNCallbackClock;

static inline void tn_callback_clock_init(TNCallbackClock *clock) {
    clock->primed = false;
    clock->next_sample = 0;
    clock->previous_host = 0;
    atomic_init(&clock->sequence, 0);
    atomic_init(&clock->sample_bits, 0);
    atomic_init(&clock->host_time, 0);
    atomic_init(&clock->discontinuities, 0);
    atomic_init(&clock->valid, 0);
}

static inline void tn_callback_clock_record(TNCallbackClock *clock,
    const AudioTimeStamp *time, uint64_t frames) {
    const AudioTimeStampFlags required = kAudioTimeStampSampleTimeValid | kAudioTimeStampHostTimeValid;
    bool valid = time && frames && (time->mFlags & required) == required &&
        isfinite(time->mSampleTime) && fabs(time->mSampleTime) < 0x1p52;
    bool broken = clock->primed && (!valid ||
        fabs(time->mSampleTime - clock->next_sample) > 0.5 ||
        time->mHostTime <= clock->previous_host);
    clock->primed = valid;
    uint64_t bits = 0;
    if (valid) {
        clock->next_sample = time->mSampleTime + (double)frames;
        clock->previous_host = time->mHostTime;
        memcpy(&bits, &time->mSampleTime, sizeof(bits));
    }
    /* SC ordering makes the bounded snapshot auditable on both arm64/x86_64.
     * No allocation, property query, lock, CAS loop or timer in the callback. */
    atomic_fetch_add(&clock->sequence, 1);
    atomic_store(&clock->sample_bits, bits);
    atomic_store(&clock->host_time, valid ? time->mHostTime : 0);
    if (broken) atomic_fetch_add(&clock->discontinuities, 1);
    atomic_store(&clock->valid, valid ? 1U : 0U);
    atomic_fetch_add(&clock->sequence, 1);
}

static inline bool tn_callback_clock_snapshot(const TNCallbackClock *clock,
    TNCallbackClockSnapshot *result) {
    uint64_t before = atomic_load(&clock->sequence);
    if (before & 1U) return false;
    uint64_t bits = atomic_load(&clock->sample_bits);
    TNCallbackClockSnapshot snapshot = {
        .host_time = atomic_load(&clock->host_time),
        .discontinuities = atomic_load(&clock->discontinuities),
        .valid = atomic_load(&clock->valid),
    };
    memcpy(&snapshot.sample_time, &bits, sizeof(bits));
    if (before != atomic_load(&clock->sequence)) return false;
    *result = snapshot;
    return true;
}
#endif
