#ifndef TN_CALLBACK_CLOCK_H
#define TN_CALLBACK_CLOCK_H
#include <CoreAudio/CoreAudioTypes.h>
#include <stdbool.h>
#include <stdint.h>

typedef struct TNCallbackClockSnapshot {
    double sample_time;
    uint64_t host_time;
    uint64_t discontinuities;
    uint32_t valid;
} TNCallbackClockSnapshot;

/* Caller-owned Rust storage. Initialize before registering IO; never copy,
 * inspect or clear these bytes while live. Destroy only after callbacks and
 * snapshot readers have drained. */
typedef struct TNCallbackClock { uint64_t storage[8]; } TNCallbackClock;
_Static_assert(sizeof(TNCallbackClock) == 64 && _Alignof(TNCallbackClock) == 8, "clock storage ABI");
_Static_assert(sizeof(TNCallbackClockSnapshot) == 32 && _Alignof(TNCallbackClockSnapshot) == 8, "clock snapshot ABI");

void tn_callback_clock_init(TNCallbackClock *clock);
/* Reset after the IOProc drains; preserve publication and cumulative errors. */
void tn_callback_clock_reset(TNCallbackClock *clock);
void tn_callback_clock_record_values(TNCallbackClock *clock, double sample,
    uint64_t host, bool timestamp_valid, uint64_t frames);
bool tn_callback_clock_snapshot(const TNCallbackClock *clock, TNCallbackClockSnapshot *result);
uint64_t tn_callback_clock_discontinuities(const TNCallbackClock *clock);

/* CoreAudio ABI projection only. Validity/continuity and publication live in Rust. */
static inline void tn_callback_clock_record(TNCallbackClock *clock,
    const AudioTimeStamp *time, uint64_t frames) {
    const AudioTimeStampFlags required = kAudioTimeStampSampleTimeValid | kAudioTimeStampHostTimeValid;
    tn_callback_clock_record_values(clock, time ? time->mSampleTime : 0,
        time ? time->mHostTime : 0, time && (time->mFlags & required) == required, frames);
}
#endif
