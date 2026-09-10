/* Lab-only instrumentation translation unit. The included production source is
 * unchanged; only its ring calls are wrapped to record callback boundaries.
 * No trace allocation, locks or file IO occur on either realtime callback. */
#define _POSIX_C_SOURCE 200809L
#include "MRPcmRing.h"
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
#include <math.h>

static uint32_t trace_read(MRPcmRing *ring, float *samples, uint32_t frames);
static uint32_t trace_write(MRPcmRing *ring, const float *samples, uint32_t frames);
#define mr_pcm_ring_read trace_read
#define mr_pcm_ring_write trace_write
#include "MRPipeWire.c"
#undef mr_pcm_ring_read
#undef mr_pcm_ring_write

enum { TRACE_CAPACITY = 100000 };
typedef struct {
    uint64_t actual_ns, position, clock_ns, read, written, overflows, starvations;
    uint32_t clock_id, frames, returned;
    uint32_t stereo_mismatches, silent_frames;
    float peak, stereo_error;
} TraceEvent;
typedef struct { TraceEvent *events; uint32_t count; } TraceBank;
static TraceBank producer, consumer;
static MRPWCycle consumer_cycle;

static uint64_t trace_now(void) {
    struct timespec value;
    clock_gettime(CLOCK_MONOTONIC, &value);
    return (uint64_t)value.tv_sec * 1000000000u + (uint64_t)value.tv_nsec;
}

int mr_drift_trace_start(void) {
    producer.events = calloc(TRACE_CAPACITY, sizeof(TraceEvent));
    consumer.events = calloc(TRACE_CAPACITY, sizeof(TraceEvent));
    if (!producer.events || !consumer.events) return -1;
    // Prefault the owned trace pages on the control thread.
    for (size_t index = 0; index < TRACE_CAPACITY * sizeof(TraceEvent); index += 4096) {
        ((volatile unsigned char *)producer.events)[index] = 0;
        ((volatile unsigned char *)consumer.events)[index] = 0;
    }
    return 0;
}

void mr_drift_trace_cycle(const MRPWCycle *cycle) { consumer_cycle = *cycle; }

static TraceEvent begin_event(MRPcmRing *ring, uint32_t frames) {
    MRPcmRingStats stats = mr_pcm_ring_stats(ring);
    return (TraceEvent) {
        .actual_ns = trace_now(), .frames = frames,
        .read = stats.read, .written = stats.written,
        .overflows = stats.overflows, .starvations = stats.starvations,
    };
}

static void trace_pcm(TraceEvent *event, const float *samples, uint32_t channels) {
    for (uint32_t i = 0; i < event->frames; i++) {
        float left = samples[(size_t)i * channels];
        float right = channels > 1 ? samples[(size_t)i * channels + 1] : left;
        float error = fabsf(left - right);
        if (error > 0.0001f) event->stereo_mismatches++;
        if (error > event->stereo_error) event->stereo_error = error;
        if (fabsf(left) > event->peak) event->peak = fabsf(left);
        if (fabsf(left) < 0.00001f) event->silent_frames++;
    }
}

static uint32_t trace_read(MRPcmRing *ring, float *samples, uint32_t frames) {
    TraceEvent value = begin_event(ring, frames);
    value.clock_id = consumer_cycle.clock_id;
    value.position = consumer_cycle.position;
    value.clock_ns = consumer_cycle.monotonic_ns;
    value.returned = mr_pcm_ring_read(ring, samples, frames);
    trace_pcm(&value, samples, mr_pcm_ring_stats(ring).channels);
    if (consumer.events && consumer.count < TRACE_CAPACITY) consumer.events[consumer.count++] = value;
    return value.returned;
}

static uint32_t trace_write(MRPcmRing *ring, const float *samples, uint32_t frames) {
    TraceEvent value = begin_event(ring, frames);
    MRPWStream *stream = (MRPWStream *)((char *)ring - offsetof(MRPWStream, ring));
    struct spa_io_position *position = atomic_load_explicit(&stream->position, memory_order_acquire);
    if (position) {
        value.clock_id = position->clock.id;
        value.position = position->clock.position;
        value.clock_ns = position->clock.nsec;
    }
    value.returned = mr_pcm_ring_write(ring, samples, frames);
    trace_pcm(&value, samples, mr_pcm_ring_stats(ring).channels);
    if (producer.events && producer.count < TRACE_CAPACITY) producer.events[producer.count++] = value;
    return value.returned;
}

static void save_bank(FILE *file, const char *kind, const TraceBank *bank) {
    for (uint32_t index = 0; index < bank->count; index++) {
        const TraceEvent *v = &bank->events[index];
        fprintf(file, "%s,%llu,%u,%llu,%llu,%u,%u,%llu,%llu,%llu,%llu,%u,%u,%.9f,%.9f\n", kind,
            (unsigned long long)v->actual_ns, v->clock_id, (unsigned long long)v->position,
            (unsigned long long)v->clock_ns, v->frames, v->returned,
            (unsigned long long)v->read, (unsigned long long)v->written,
            (unsigned long long)v->overflows, (unsigned long long)v->starvations,
            v->stereo_mismatches, v->silent_frames, v->peak, v->stereo_error);
    }
}

int mr_drift_trace_save(const char *path) {
    FILE *file = fopen(path, "wx");
    if (!file) return -1;
    fputs("kind,actualNs,clockId,position,clockNs,frames,returned,read,written,overflows,starvations,stereoMismatches,silentFrames,peak,stereoError\n", file);
    save_bank(file, "producer", &producer);
    save_bank(file, "consumer", &consumer);
    int result = fclose(file);
    if (producer.count == TRACE_CAPACITY || consumer.count == TRACE_CAPACITY) result = -1;
    free(producer.events); free(consumer.events);
    return result;
}
