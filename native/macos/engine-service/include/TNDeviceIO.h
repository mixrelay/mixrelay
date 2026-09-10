#ifndef TN_DEVICE_IO_H
#define TN_DEVICE_IO_H
#include <stdbool.h>
#include <stdint.h>
#include "TNAggregateGroup.h"
typedef struct TNDeviceIO TNDeviceIO;
/* Set once before connecting. Rust owns the complete callback context. */
typedef bool (*TNRenderHook)(void *, const void *, void *, uint32_t);
void tn_device_io_render_hook(TNDeviceIO *io, void *context, TNRenderHook hook);
bool tn_device_io_direct_binding(TNDeviceIO *io, uint32_t *device, uint32_t *offset, uint32_t *total);
TNDeviceIO *tn_device_io_create_group(TNAggregateGroup *group, const char *uid,
    bool input, uint32_t channels, uint32_t rate);
/* One owning worker creates/connects/disconnects/destroys and runs SRC.
 * CoreAudio <-> native SPSC <-> worker <-> graph SPSC <-> serialized graph.
 * Each side owns only its cursor; no converter or scratch is shared mutably. */
TNDeviceIO *tn_device_io_create(const char *uid, bool input, uint32_t channels, uint32_t rate);
bool tn_device_io_connect(TNDeviceIO *io);
/* 0=connected; -1/-2/-3/-5=preflight stage, other values=native OSStatus.
 * The Rust owner decides reporting/recovery; this boundary never retries. */
int32_t tn_device_io_connect_status(TNDeviceIO *io, uint32_t expected_device);
/* Control-only buffer facts; state resolves immutable UID/group metadata
 * without racing the connection worker's mutable device handle. */
typedef struct TNDeviceBufferInfo {
    uint32_t device, frames;
    double minimum, maximum;
    uint32_t settable;
} TNDeviceBufferInfo;
_Static_assert(sizeof(TNDeviceBufferInfo) == 32, "Rust buffer snapshot ABI");
bool tn_device_io_buffer_info(TNDeviceIO *io, TNDeviceBufferInfo *info);
bool tn_device_buffer_write(uint32_t device, uint32_t frames);
uint32_t tn_device_buffer_size(uint32_t device);
/* False retains registration/storage: retry on the same control owner. Never
 * free callback context merely because a vendor stop/unregister call failed. */
bool tn_device_io_disconnect(TNDeviceIO *io);
bool tn_device_io_destroy(TNDeviceIO *io);
bool tn_device_io_alive(TNDeviceIO *io); /* control-only property queries */
uint64_t tn_device_io_ticks(TNDeviceIO *io);
/* The connect/liveness owner alone queries rate; frame progress is atomic. */
uint32_t tn_device_io_native_rate(TNDeviceIO *io);
/* Control owner only. Zero means no proof: in particular, a user Aggregate
 * may contain unsynchronized subdevices despite having a single object ID. */
uint32_t tn_device_io_clock_device(TNDeviceIO *io);
uint64_t tn_device_io_clock_discontinuities(TNDeviceIO *io);
uint64_t tn_device_io_native_frames(TNDeviceIO *io);
uint64_t tn_device_io_graph_frames(TNDeviceIO *io);
uint64_t tn_device_io_overflows(TNDeviceIO *io);
uint32_t tn_device_io_quantum(TNDeviceIO *io, bool graph);
void tn_device_io_output_epoch(TNDeviceIO *io, uint64_t epoch);
/* Native callbacks and graph never share a ring cursor. The worker owns the
 * opposite side of each SPSC and its SRC state. Queued values are frames in
 * that ring's clock: graph=true uses graph rate, false uses native rate. */
uint32_t tn_device_io_queued(TNDeviceIO *io, bool graph);
void tn_device_io_worker_discard(TNDeviceIO *io);
uint32_t tn_device_io_graph_read(TNDeviceIO *io, float *samples, uint32_t frames, uint64_t epoch);
uint32_t tn_device_io_graph_write(TNDeviceIO *io, const float *samples, uint32_t frames);
uint32_t tn_device_io_worker_read(TNDeviceIO *io, float *samples, uint32_t frames);
uint32_t tn_device_io_worker_write(TNDeviceIO *io, const float *samples, uint32_t frames);
/* Actual registered CoreAudio callback. Native fixtures supply complete native
 * buffer/timestamp extents and serialize the same role ownership as the OS. */
int32_t tn_device_io_process(uint32_t device, const void *now, const void *input,
    const void *input_time, void *output, const void *output_time, void *context);
#endif
