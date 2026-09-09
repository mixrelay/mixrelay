#ifndef TN_ENDPOINT_TRANSPORT_H
#define TN_ENDPOINT_TRANSPORT_H

#include "TNSharedAudioBuffer.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

enum {
    TN_ENDPOINT_ID_BYTES = 16,
};

typedef struct TNEndpointTransport TNEndpointTransport;

typedef struct TNEndpointTransportConfig {
    uint8_t endpoint_id[TN_ENDPOINT_ID_BYTES];
    uint32_t capacity_frames;
    uint32_t render_channels;
    uint32_t capture_channels;
} TNEndpointTransportConfig;

typedef struct TNEndpointTransportStats {
    uint64_t driver_heartbeat;
    uint64_t engine_heartbeat;
} TNEndpointTransportStats;

/* Native callback credits; deliberately no fabricated host timestamp. */
typedef struct TNEndpointClockSnapshot {
    uint64_t epoch;
    uint64_t render_through;
    uint64_t capture_through;
    uint32_t sample_rate;
    uint32_t running;
    uint32_t capture_frames;
    uint32_t render_frames;
} TNEndpointClockSnapshot;

/* Enter before PCM epoch reset; snapshots stay unavailable until final commit. */
bool tn_endpoint_transport_begin_clock_transition(TNEndpointTransport *transport);
/* Final commit requires an active transition and completed PCM invalidation. */
void tn_endpoint_transport_clock_state(TNEndpointTransport *transport,
                                       uint64_t epoch, uint32_t sample_rate, bool running);
void tn_endpoint_transport_clock_demand(TNEndpointTransport *transport,
                                        bool render, uint64_t through_frame, uint32_t frames);
bool tn_endpoint_transport_clock_snapshot(const TNEndpointTransport *transport,
                                           TNEndpointClockSnapshot *snapshot);
/* A terminal lifecycle tag cannot be reused while remote readers may exist. */
bool tn_endpoint_transport_requires_replacement(const TNEndpointTransport *transport);
/* Stable identity of this mapping, not its endpoint or POSIX shm inode. */
bool tn_endpoint_transport_incarnation(const TNEndpointTransport *transport,
                                       uint8_t incarnation[16]);

size_t tn_endpoint_transport_required_bytes(
    const TNEndpointTransportConfig *config);
TNEndpointTransport *tn_endpoint_transport_initialize(
    void *memory,
    size_t memory_size,
    const TNEndpointTransportConfig *config);
TNEndpointTransport *tn_endpoint_transport_attach(void *memory,
                                                   size_t memory_size);

bool tn_endpoint_transport_config(const TNEndpointTransport *transport,
                                  TNEndpointTransportConfig *config);
TNSharedAudioBuffer *tn_endpoint_transport_render_buffer(
    TNEndpointTransport *transport);
TNSharedAudioBuffer *tn_endpoint_transport_capture_buffer(
    TNEndpointTransport *transport);

uint64_t tn_endpoint_transport_touch_driver(TNEndpointTransport *transport);
uint64_t tn_endpoint_transport_touch_engine(TNEndpointTransport *transport);
TNEndpointTransportStats tn_endpoint_transport_stats(
    const TNEndpointTransport *transport);

#ifdef __cplusplus
}
#endif

#endif
