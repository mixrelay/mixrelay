#ifndef TN_APPLICATION_CLOCK_H
#define TN_APPLICATION_CLOCK_H
#include "TNSharedAudioBuffer.h"
#include <stdatomic.h>
#include <stdint.h>
/* Current shared application clock ABI. Policy and initialization live in Rust;
 * C producers access only these atomics and the exported functions below. */
enum { TN_APPLICATION_CLOCK_VERSION = 2U, TN_APPLICATION_CLOCK_ALIGNMENT = 64U };
#define TN_APPLICATION_CLOCK_MAGIC UINT64_C(0x544e415050434c4b)
#define TN_APPLICATION_CLOCK_FRAME_MASK UINT64_C(0x0000ffffffffffff)
typedef struct TNApplicationClock {
    uint64_t magic;
    uint32_t version;
    uint32_t header_bytes;
    _Atomic uint64_t format_word;
    _Atomic uint64_t acknowledged_format;
    _Atomic uint64_t consumer_position;
    _Atomic uint64_t consumer_ticks;
    _Atomic uint32_t consumer_running;
    _Atomic uint32_t native_sample_rate;
    _Atomic uint64_t consumer_sequence;
    _Atomic uint32_t producer_guard;
    _Atomic uint64_t native_frames;
    _Atomic uint32_t native_running;
    _Atomic uint32_t clock_master;
    _Atomic uint64_t producer_lease;
} TNApplicationClock;
size_t tn_application_clock_required_bytes(uint32_t capacity, uint32_t channels);
TNApplicationClock *tn_application_clock_attach(void *memory, size_t size, uint32_t capacity, uint32_t channels);
TNApplicationClock *tn_application_clock_initialize(void *memory, size_t size, uint32_t capacity, uint32_t channels);
bool tn_application_clock_try_producer(TNApplicationClock *clock);
void tn_application_clock_release_producer(TNApplicationClock *clock);
uint64_t tn_application_clock_request_rate(TNApplicationClock *clock, uint32_t rate);
void tn_application_clock_publish_consumer(TNApplicationClock *clock, uint64_t epoch, uint64_t next_frame, uint32_t frames, bool running);
#endif
