/* Native MixRelay stream/driver boundary probe. No engine service or user graph.
 * Uses the production MRPipeWire/MRPcmRing files from an immutable lab snapshot. */
#define _POSIX_C_SOURCE 200809L
#include "MRPipeWire.h"
#include <math.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>

#ifdef MR_DRIFT_TRACE
int mr_drift_trace_start(void);
void mr_drift_trace_cycle(const MRPWCycle *cycle);
int mr_drift_trace_save(const char *path);
#endif

typedef struct {
    MRPWStream *stream;
    uint64_t start_ns, warmup_ns, frames, silent, invalid, gap, maximum_gap, jumps, crossings;
    uint64_t clock_changes;
    uint64_t stereo_mismatches;
    uint32_t clock_id, clock_rate;
    float previous;
    double peak, energy, maximum_stereo_error;
    bool measuring;
} Probe;

static void process(void *data, const MRPWCycle *cycle) {
#ifdef MR_DRIFT_TRACE
    mr_drift_trace_cycle(cycle);
#endif
    Probe *p = data;
    if (!p->start_ns) p->start_ns = cycle->monotonic_ns;
    float pcm[32768 * 2];
    if (cycle->rate != 48000 || cycle->frames > 32768) { p->invalid++; return; }
    mr_pw_stream_read(p->stream, pcm, cycle->frames);
    if (cycle->monotonic_ns - p->start_ns < p->warmup_ns) return;
    if (p->clock_id && p->clock_id != cycle->clock_id) p->clock_changes++;
    p->clock_id = cycle->clock_id; p->clock_rate = cycle->rate;
    for (uint32_t n = 0; n < cycle->frames; n++) {
        float value = pcm[n * 2];
        if (!isfinite(value) || !isfinite(pcm[n * 2 + 1])) { p->invalid++; continue; }
        double stereo_error = fabsf(value - pcm[n * 2 + 1]);
        if (stereo_error > 0.0001f) p->stereo_mismatches++;
        if (stereo_error > p->maximum_stereo_error) p->maximum_stereo_error = stereo_error;
        if (fabs(value) > p->peak) p->peak = fabs(value);
        p->energy += value * value;
        if (p->measuring && fabsf(value - p->previous) > 0.05f) p->jumps++;
        if (p->previous <= 0 && value > 0) p->crossings++;
        if (fabsf(value) < 0.00001f) { p->silent++; p->gap++; }
        else { if (p->gap > p->maximum_gap) p->maximum_gap = p->gap; p->gap = 0; }
        p->previous = value; p->measuring = true; p->frames++;
    }
}
int main(int argc, char **argv) {
    if (argc != 4) return 2;
    double seconds = strtod(argv[2], NULL), warmup = strtod(argv[3], NULL);
    if (seconds < 1 || seconds > 600 || warmup < 0 || warmup >= seconds) return 2;
    Probe p = { .warmup_ns = (uint64_t)(warmup * 1e9) };
#ifdef MR_DRIFT_TRACE
    const char *trace_path = getenv("MIXRELAY_LAB_DRIFT_TRACE");
    if (!trace_path || mr_drift_trace_start() < 0) return 2;
#endif
#ifdef MR_DRIFT_FORMAT_API
    p.stream = mr_pw_stream_open("io.mixrelay.lab-drift-capture", "MixRelay drift probe", argv[1], 3, 2, 48000, 128);
#else
    p.stream = mr_pw_stream_open("io.mixrelay.lab-drift-capture", "MixRelay drift probe", argv[1], 3, 2);
#endif
    if (!p.stream) return 1;
    MRPWDriver *driver = mr_pw_driver_open(process, &p);
    if (!driver) { mr_pw_stream_close(p.stream); return 1; }
    struct timespec duration = { .tv_sec = (time_t)seconds, .tv_nsec = (long)((seconds - (time_t)seconds) * 1e9) };
    nanosleep(&duration, NULL);
    mr_pw_driver_close(driver);
    uint64_t capture_clock = mr_pw_stream_clock(p.stream);
    uint64_t format = mr_pw_stream_format(p.stream);
    MRPWTransferCounters counters = mr_pw_stream_counters(p.stream);
    mr_pw_stream_close(p.stream);
#ifdef MR_DRIFT_TRACE
    if (mr_drift_trace_save(trace_path) < 0) return 1;
#endif
    printf("{\"frames\":%llu,\"silentFrames\":%llu,\"invalid\":%llu,\"maximumGap\":%llu,\"jumps\":%llu,\"crossings\":%llu,\"peak\":%.9f,\"rms\":%.9f,\"observedFrequency\":%.6f,\"driverClockId\":%u,\"driverRate\":%u,\"captureClockId\":%u,\"captureClockRate\":%u,\"pcmRate\":%u,\"pcmChannels\":%u,\"clockChanges\":%llu,\"ringOverflows\":%llu,\"ringStarvations\":%llu,\"stereoMismatches\":%llu,\"maximumStereoError\":%.9f}\n",
        (unsigned long long)p.frames, (unsigned long long)p.silent, (unsigned long long)p.invalid,
        (unsigned long long)p.maximum_gap, (unsigned long long)p.jumps, (unsigned long long)p.crossings,
        p.peak, p.frames ? sqrt(p.energy / p.frames) : 0, p.frames ? (double)p.crossings * 48000 / p.frames : 0,
        p.clock_id, p.clock_rate, (uint32_t)(capture_clock >> 32), (uint32_t)capture_clock,
        (uint32_t)(format >> 32), (uint32_t)format, (unsigned long long)p.clock_changes,
        (unsigned long long)counters.overflows, (unsigned long long)counters.starvations,
        (unsigned long long)p.stereo_mismatches, p.maximum_stereo_error);
    return p.frames && !p.invalid && !p.stereo_mismatches && p.peak > 0.2 ? 0 : 1;
}
