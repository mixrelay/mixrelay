#ifndef TN_APPLICATION_SOURCE_TRANSPORT_H
#define TN_APPLICATION_SOURCE_TRANSPORT_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct TNApplicationSourceTransport TNApplicationSourceTransport;
typedef struct TNEngineSourceAttachment TNEngineSourceAttachment;

typedef enum TNApplicationSourceTransportErrorCode {
    TN_APPLICATION_SOURCE_TRANSPORT_OK = 0,
    TN_APPLICATION_SOURCE_TRANSPORT_INVALID_ARGUMENT = 1,
    TN_APPLICATION_SOURCE_TRANSPORT_ENGINE_UNAVAILABLE = 2,
    TN_APPLICATION_SOURCE_TRANSPORT_INVALID_ATTACHMENT = 3,
    TN_APPLICATION_SOURCE_TRANSPORT_MAPPING_FAILED = 4,
} TNApplicationSourceTransportErrorCode;

typedef struct TNApplicationSourceTransportError {
    TNApplicationSourceTransportErrorCode code;
    int32_t system_error;
} TNApplicationSourceTransportError;

/* Control-worker snapshot; never exposes pointers into another process. */
typedef struct TNApplicationSourceClockSnapshot {
    uint64_t format_word;
    uint64_t epoch;
    uint64_t produced_frames;
    uint64_t consumed_frames;
    uint64_t consumer_ticks;
    uint32_t consumer_running;
    uint32_t clock_master;
} TNApplicationSourceClockSnapshot;

bool tn_application_source_transport_clock(
    TNApplicationSourceTransport *transport, TNApplicationSourceClockSnapshot *snapshot);
bool tn_application_source_transport_reset(
    TNApplicationSourceTransport *transport, uint64_t format_word, uint32_t native_sample_rate);
void tn_application_source_transport_publish_native(
    TNApplicationSourceTransport *transport, uint64_t native_frames, bool running);

TNApplicationSourceTransport *tn_application_source_transport_connect(
    const char *source_id,
    uint32_t capacity_frames,
    uint32_t channels,
    TNApplicationSourceTransportError *error);

/* Takes and clears the attachment, closing its descriptor on every failure.
 * Successful handles are owned by exactly one pump; destroy only after it stops. */
TNApplicationSourceTransport *tn_application_source_transport_attach(
    TNEngineSourceAttachment *attachment,
    uint32_t capacity_frames,
    uint32_t channels,
    TNApplicationSourceTransportError *error);

bool tn_application_source_transport_write(
    TNApplicationSourceTransport *transport,
    const float *interleaved_samples,
    uint32_t frame_count);

void tn_application_source_transport_destroy(
    TNApplicationSourceTransport *transport);

#ifdef __cplusplus
}
#endif

#endif
