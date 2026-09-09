#include "../TNCallbackClock.h"
#include <assert.h>
#include <pthread.h>
#include <stdio.h>

static AudioTimeStamp stamp(double sample, uint64_t host) {
    return (AudioTimeStamp){.mSampleTime = sample, .mHostTime = host,
        .mFlags = kAudioTimeStampSampleTimeValid | kAudioTimeStampHostTimeValid};
}
static void continuity(void) {
    TNCallbackClock clock;
    tn_callback_clock_init(&clock);
    TNCallbackClockSnapshot read = {0};
    assert(tn_callback_clock_snapshot(&clock, &read) && !read.valid);
    AudioTimeStamp time = stamp(-128, 1000);
    tn_callback_clock_record(&clock, &time, 128);
    time = stamp(0, 2000);
    tn_callback_clock_record(&clock, &time, 256);
    assert(tn_callback_clock_snapshot(&clock, &read));
    assert(read.valid && read.sample_time == 0 && read.host_time == 2000 && !read.discontinuities);
    time = stamp(512, 3000); /* a gap, not ordinary callback arrival jitter */
    tn_callback_clock_record(&clock, &time, 64);
    assert(atomic_load(&clock.discontinuities) == 1);
    time = stamp(576, 2999); /* invalid backwards host clock */
    tn_callback_clock_record(&clock, &time, 128);
    assert(atomic_load(&clock.discontinuities) == 2);
    time = stamp(704, 999999); /* host interval alone must not imply drift */
    tn_callback_clock_record(&clock, &time, 128);
    assert(atomic_load(&clock.discontinuities) == 2);
    time.mFlags = kAudioTimeStampSampleTimeValid;
    tn_callback_clock_record(&clock, &time, 128);
    assert(atomic_load(&clock.discontinuities) == 3);
    tn_callback_clock_record(&clock, NULL, 128);
    assert(atomic_load(&clock.discontinuities) == 3);
    time = stamp(NAN, 9999999);
    tn_callback_clock_record(&clock, &time, 128);
    assert(tn_callback_clock_snapshot(&clock, &read) && !read.valid);
    time = stamp(1, 1); /* new timeline after invalidity */
    tn_callback_clock_record(&clock, &time, 128);
    assert(tn_callback_clock_snapshot(&clock, &read) && read.valid && read.discontinuities == 3);
    atomic_store(&clock.sequence, 1);
    assert(!tn_callback_clock_snapshot(&clock, &read)); /* no reader spin */
}

typedef struct Fixture { TNCallbackClock clock; _Atomic bool done; } Fixture;
static void *writer(void *context) {
    Fixture *fixture = context;
    for (uint64_t i = 1; i <= 100000; ++i) {
        AudioTimeStamp time = stamp((double)i, i * 7);
        tn_callback_clock_record(&fixture->clock, &time, 1);
    }
    atomic_store(&fixture->done, true);
    return NULL;
}
static void concurrent_snapshot(void) {
    Fixture fixture;
    tn_callback_clock_init(&fixture.clock);
    atomic_init(&fixture.done, false);
    assert(atomic_is_lock_free(&fixture.clock.sequence));
    assert(atomic_is_lock_free(&fixture.clock.valid));
    pthread_t thread;
    assert(pthread_create(&thread, NULL, writer, &fixture) == 0);
    while (!atomic_load(&fixture.done)) {
        TNCallbackClockSnapshot read;
        if (tn_callback_clock_snapshot(&fixture.clock, &read) && read.valid) {
            assert(read.host_time == (uint64_t)read.sample_time * 7);
            assert(read.discontinuities == 0);
        }
    }
    assert(pthread_join(thread, NULL) == 0);
}
int main(void) {
    continuity();
    concurrent_snapshot();
    puts("PASS callback timestamps: validity, gaps, backwards clocks, 100000 concurrent publications");
}
