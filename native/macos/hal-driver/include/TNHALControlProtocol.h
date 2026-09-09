#ifndef TN_HAL_CONTROL_PROTOCOL_H
#define TN_HAL_CONTROL_PROTOCOL_H

#include "TNEndpointTransport.h"

#include <stdbool.h>
#include <stddef.h>
#include <xpc/xpc.h>

#ifdef __cplusplus
extern "C" {
#endif

#define TN_HAL_CONTROL_SERVICE_NAME "io.tonenari.audio.engine"

enum {
    TN_HAL_CONTROL_PROTOCOL_VERSION = 1,
    TN_HAL_ENDPOINT_DISPLAY_NAME_MAX_BYTES = 128,
    /* Framing limit, not a silent truncation or a registry product policy. */
    TN_HAL_CONTROL_INVENTORY_MAX_ENTRIES = 64,
};

typedef struct TNHALControlInventoryEntry {
    uint8_t endpoint_id[TN_ENDPOINT_ID_BYTES];
    char display_name[TN_HAL_ENDPOINT_DISPLAY_NAME_MAX_BYTES];
    uint32_t sample_rate;
    int descriptor;
    size_t mapped_size;
    uint64_t generation;
} TNHALControlInventoryEntry;

typedef struct TNHALControlInventorySnapshot {
    uint64_t generation;
    size_t count;
    TNHALControlInventoryEntry entries[TN_HAL_CONTROL_INVENTORY_MAX_ENTRIES];
} TNHALControlInventorySnapshot;

xpc_object_t tn_hal_control_create_inventory_request(void);
bool tn_hal_control_parse_inventory_request(xpc_object_t message);
/* Creators borrow caller descriptors. The XPC message retains its own copies. */
xpc_object_t tn_hal_control_create_inventory_snapshot(
    const TNHALControlInventoryEntry *entries, size_t count, uint64_t generation);
xpc_object_t tn_hal_control_create_inventory_snapshot_reply(
    xpc_object_t request, const TNHALControlInventoryEntry *entries,
    size_t count, uint64_t generation);
/* Output is unchanged on error, including when a later entry is malformed.
 * Success transfers duplicated descriptors to an empty/previously closed output.
 * Parsing validates framing/backing sizes; transport layout and product rate
 * support must all validate before a registry applies authoritative removals. */
bool tn_hal_control_parse_inventory_snapshot(
    xpc_object_t message, TNHALControlInventorySnapshot *snapshot);
void tn_hal_control_inventory_snapshot_close(TNHALControlInventorySnapshot *snapshot);

/* Typed removal intent. The registry, not the codec, checks current generation. */
xpc_object_t tn_hal_control_create_endpoint_remove(
    const uint8_t endpoint_id[TN_ENDPOINT_ID_BYTES], uint64_t generation);
bool tn_hal_control_parse_endpoint_remove(
    xpc_object_t message, uint8_t endpoint_id[TN_ENDPOINT_ID_BYTES],
    uint64_t *generation);

/* Replacement intent only. The engine compares expected_generation so repeated
 * requests cannot replace a newly published mapping a second time. */
xpc_object_t tn_hal_control_create_transport_replacement_request(
    const uint8_t endpoint_id[TN_ENDPOINT_ID_BYTES], uint64_t expected_generation);
/* Outputs remain unchanged on malformed input. */
bool tn_hal_control_parse_transport_replacement_request(
    xpc_object_t message, uint8_t endpoint_id[TN_ENDPOINT_ID_BYTES],
    uint64_t *expected_generation);

#ifdef __cplusplus
}
#endif

#endif
