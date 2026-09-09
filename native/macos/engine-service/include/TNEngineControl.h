#ifndef TN_ENGINE_CONTROL_H
#define TN_ENGINE_CONTROL_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <xpc/xpc.h>

#ifdef __cplusplus
extern "C" {
#endif

enum {
    TN_ENGINE_SOURCE_ID_MAX_BYTES = 128,
};

typedef struct TNEngineSourceRequest {
    char source_id[TN_ENGINE_SOURCE_ID_MAX_BYTES];
    uint32_t capacity_frames;
    uint32_t channels;
    uint32_t transport_version;
} TNEngineSourceRequest;

typedef struct TNEngineSourceAttachment {
    int descriptor;
    size_t mapped_size;
    uint64_t producer_lease;
    uint64_t source_epoch;
} TNEngineSourceAttachment;

/* Only clocked source transport version 2 is supported. */
xpc_object_t tn_engine_control_create_clocked_source_request(
    const char *source_id, uint32_t capacity_frames, uint32_t channels);
bool tn_engine_control_parse_source_request(
    xpc_object_t message,
    TNEngineSourceRequest *request);

/* The creator borrows its FD; XPC retains a duplicate. Lease metadata is atomic
 * with the descriptor. Parsing leaves output unchanged on every error. */
xpc_object_t tn_engine_control_create_source_attachment_reply(
    xpc_object_t request, int descriptor, size_t mapped_size,
    uint64_t producer_lease, uint64_t source_epoch);
bool tn_engine_control_parse_source_attachment(
    xpc_object_t message, TNEngineSourceAttachment *attachment);
void tn_engine_control_source_attachment_close(TNEngineSourceAttachment *attachment);

#ifdef __cplusplus
}
#endif

#endif
