#ifndef TN_HAL_CORE_H
#define TN_HAL_CORE_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

/* All ownership and registry policy lives in Rust. The native runtime callbacks
 * below only adapt the existing C11-atomic transport/clock layout and Apple ABI.
 * Every callback is synchronous. Native callbacks must never call the registry.
 * Host callbacks may reenter it: they are always invoked after releasing locks. */
typedef struct TNHALCore TNHALCore;
typedef struct TNHALCoreGeometry {
    uint8_t endpoint_id[16];
    uint32_t render_channels;
    uint32_t capture_channels;
} TNHALCoreGeometry;
typedef struct TNHALCoreTimestamp {
    uint64_t sample_time, host_time, seed;
} TNHALCoreTimestamp;
typedef struct TNHALCoreSnapshot {
    uint32_t device_id, input_stream_id, output_stream_id;
    uint32_t sample_rate, render_channels, capture_channels, client_count;
    bool alive, running, input_active, output_active;
    char name[128];
    char uid[80];
} TNHALCoreSnapshot;
typedef struct TNHALCoreNativeOps {
    bool (*inspect)(int, size_t, TNHALCoreGeometry *);
    void *(*create)(const TNHALCoreGeometry *, uint32_t, int, size_t);
    void (*destroy)(void *);
    bool (*replace)(void *, const TNHALCoreGeometry *, uint32_t, int, size_t);
    bool (*start)(void *, uint64_t);
    void (*stop)(void *);
    void (*remaining_client)(void *);
    bool (*set_rate)(void *, uint32_t);
    bool (*timestamp)(void *, uint64_t, TNHALCoreTimestamp *);
    bool (*io)(void *, bool, uint64_t, float *, uint32_t);
    bool (*requires_replacement)(void *);
    bool (*same_transport)(void *, int, size_t);
} TNHALCoreNativeOps;
enum {
    TN_HAL_CORE_OK = 0,
    TN_HAL_CORE_INVALID = -1,
    TN_HAL_CORE_NOT_FOUND = -2,
    TN_HAL_CORE_BUSY = -3,
    TN_HAL_CORE_NATIVE_ERROR = -4,
    TN_HAL_CORE_CAPACITY = -5,
    TN_HAL_CORE_CHANGE_LIST = 1,
    TN_HAL_CORE_CHANGE_NAME = 2,
    TN_HAL_CORE_CHANGE_FORMAT = 3,
    TN_HAL_CORE_CHANGE_ALIVE = 4,
    TN_HAL_CORE_CHANGE_RUNNING = 5,
    TN_HAL_CORE_CHANGE_STREAM_ACTIVE = 6,
    TN_HAL_CORE_MAX_DEVICES = 64,
};
#define TN_HAL_CORE_CONFIG_ACTION UINT64_C(0x544e6368)
typedef struct TNHALCoreHostOps {
    void *context;
    void (*changed)(void *, uint32_t, uint32_t);
    bool (*request_configuration)(void *, uint32_t, uint64_t, uint64_t);
} TNHALCoreHostOps;

TNHALCore *tn_hal_core_create(const TNHALCoreNativeOps *, const TNHALCoreHostOps *);
TNHALCore *tn_hal_core_create_native(const TNHALCoreHostOps *);
size_t tn_hal_core_io_context_size(void);
size_t tn_hal_core_io_context_alignment(void);
/* Shutdown only: caller must first stop XPC/timers and all host callbacks. */
void tn_hal_core_destroy(TNHALCore *);
/* Consumes fd on EVERY path, including invalid arguments. The mapped transport
 * endpoint identity is verified before any device is published or changed. */
int32_t tn_hal_core_upsert(TNHALCore *, const uint8_t endpoint_id[16],
                          const char *name, uint32_t nominal_rate,
                          int fd, size_t mapped_size, uint32_t *device_id);
int32_t tn_hal_core_remove(TNHALCore *, const uint8_t endpoint_id[16]);
/* Retries failed host configuration requests and reclaims drained retired slots.
 * Call on a serial control queue, never from a real-time callback. */
void tn_hal_core_tick(TNHALCore *);
size_t tn_hal_core_devices(TNHALCore *, uint32_t *, size_t capacity);
uint32_t tn_hal_core_lookup_uid(TNHALCore *, const char *);
bool tn_hal_core_snapshot(TNHALCore *, uint32_t object_id, TNHALCoreSnapshot *);
/* Bounded pinned lookup; no control mutex, allocation, or metadata copying. */
bool tn_hal_core_device_alive(const TNHALCore *, uint32_t device_id);
int32_t tn_hal_core_client(TNHALCore *, uint32_t device_id, uint32_t client_id, bool add);
int32_t tn_hal_core_start(TNHALCore *, uint32_t device_id, uint32_t client_id, uint64_t host_time);
int32_t tn_hal_core_stop(TNHALCore *, uint32_t device_id, uint32_t client_id);
int32_t tn_hal_core_set_rate(TNHALCore *, uint32_t device_id, uint32_t sample_rate);
int32_t tn_hal_core_set_stream_active(TNHALCore *, uint32_t stream_id, bool active);
int32_t tn_hal_core_perform(TNHALCore *, uint32_t device_id, uint64_t action, uint64_t generation);
int32_t tn_hal_core_abort(TNHALCore *, uint32_t device_id, uint64_t action, uint64_t generation);
/* These two entry points do not lock, allocate, wait, or free. A removed device
 * is silenced; a stale object ID never resolves to a replacement device. */
bool tn_hal_core_timestamp(TNHALCore *, uint32_t device_id, uint64_t now, TNHALCoreTimestamp *);
bool tn_hal_core_io(TNHALCore *, uint32_t device_id, uint32_t stream_id,
                   bool render, uint64_t frame, float *, uint32_t frame_count);
typedef struct TNHALCorePropertyAddress { uint32_t selector, scope, element; } TNHALCorePropertyAddress;
typedef struct TNHALCoreNotification { uint32_t object_id; TNHALCorePropertyAddress address; } TNHALCoreNotification;
bool tn_hal_core_has_property(TNHALCore *, uint32_t, const TNHALCorePropertyAddress *);
int32_t tn_hal_core_property_size(TNHALCore *, uint32_t, const TNHALCorePropertyAddress *, uint32_t *);
int32_t tn_hal_core_property_get(TNHALCore *, uint32_t, const TNHALCorePropertyAddress *, uint32_t, const void *, uint32_t, uint32_t *, void *);
int32_t tn_hal_core_property_settable(TNHALCore *, uint32_t, const TNHALCorePropertyAddress *, bool *);
int32_t tn_hal_core_property_set(TNHALCore *, uint32_t, const TNHALCorePropertyAddress *, uint32_t, const void *);
size_t tn_hal_core_notifications(uint32_t, uint32_t, TNHALCoreNotification *, size_t);
/* Nonzero, increasing local XPC connection identity; never a peer counter. */
bool tn_hal_core_control_connected(TNHALCore *, uint64_t authority);
bool tn_hal_core_control_message(TNHALCore *, uint64_t authority, void *xpc_message);
bool tn_hal_core_replacement_request(TNHALCore *, uint64_t authority, uint8_t endpoint_id[16], uint64_t *generation);

#endif
