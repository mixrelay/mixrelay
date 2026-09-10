#ifndef MR_DEVICE_IO_H
#define MR_DEVICE_IO_H
#include <stdbool.h>
#include <stdint.h>
#include "MRAggregateGroup.h"
typedef struct MRDeviceIO MRDeviceIO;
/* Set once before connecting. Rust owns the complete callback context. */
typedef bool (*MRRenderHook)(void *, const void *, void *, uint32_t);
void mr_device_io_render_hook(MRDeviceIO *io, void *context, MRRenderHook hook);
bool mr_device_io_direct_binding(MRDeviceIO *io, uint32_t *device, uint32_t *offset, uint32_t *total);
MRDeviceIO *mr_device_io_create_group(MRAggregateGroup *group, const char *uid,
    bool input, uint32_t channels, uint32_t rate);
/* One owning worker creates/connects/disconnects/destroys and runs SRC.
 * CoreAudio <-> native SPSC <-> worker <-> graph SPSC <-> serialized graph.
 * Each side owns only its cursor; no converter or scratch is shared mutably. */
MRDeviceIO *mr_device_io_create(const char *uid, bool input, uint32_t channels, uint32_t rate);
bool mr_device_io_connect(MRDeviceIO *io);
/* 0=connected; -1/-2/-3/-5=preflight stage, other values=native OSStatus.
 * The Rust owner decides reporting/recovery; this boundary never retries. */
int32_t mr_device_io_connect_status(MRDeviceIO *io, uint32_t expected_device);
/* Control-only buffer facts; state resolves immutable UID/group metadata
 * without racing the connection worker's mutable device handle. */
typedef struct MRDeviceBufferInfo {
    uint32_t device, frames;
    double minimum, maximum;
    uint32_t settable;
} MRDeviceBufferInfo;
_Static_assert(sizeof(MRDeviceBufferInfo) == 32, "Rust buffer snapshot ABI");
bool mr_device_io_buffer_info(MRDeviceIO *io, MRDeviceBufferInfo *info);
bool mr_device_buffer_write(uint32_t device, uint32_t frames);
uint32_t mr_device_buffer_size(uint32_t device);
/* False retains registration/storage: retry on the same control owner. Never
 * free callback context merely because a vendor stop/unregister call failed. */
bool mr_device_io_disconnect(MRDeviceIO *io);
bool mr_device_io_destroy(MRDeviceIO *io);
bool mr_device_io_alive(MRDeviceIO *io); /* control-only property queries */
uint64_t mr_device_io_ticks(MRDeviceIO *io);
/* The connect/liveness owner alone queries rate; frame progress is atomic. */
uint32_t mr_device_io_native_rate(MRDeviceIO *io);
/* Control owner only. Zero means no proof: in particular, a user Aggregate
 * may contain unsynchronized subdevices despite having a single object ID. */
uint32_t mr_device_io_clock_device(MRDeviceIO *io);
uint64_t mr_device_io_clock_discontinuities(MRDeviceIO *io);
uint64_t mr_device_io_native_frames(MRDeviceIO *io);
uint64_t mr_device_io_graph_frames(MRDeviceIO *io);
uint64_t mr_device_io_overflows(MRDeviceIO *io);
uint32_t mr_device_io_quantum(MRDeviceIO *io, bool graph);
void mr_device_io_output_epoch(MRDeviceIO *io, uint64_t epoch);
/* Native callbacks and graph never share a ring cursor. The worker owns the
 * opposite side of each SPSC and its SRC state. Queued values are frames in
 * that ring's clock: graph=true uses graph rate, false uses native rate. */
uint32_t mr_device_io_queued(MRDeviceIO *io, bool graph);
void mr_device_io_worker_discard(MRDeviceIO *io);
uint32_t mr_device_io_graph_read(MRDeviceIO *io, float *samples, uint32_t frames, uint64_t epoch);
uint32_t mr_device_io_graph_write(MRDeviceIO *io, const float *samples, uint32_t frames);
uint32_t mr_device_io_worker_read(MRDeviceIO *io, float *samples, uint32_t frames);
uint32_t mr_device_io_worker_write(MRDeviceIO *io, const float *samples, uint32_t frames);
/* Actual registered CoreAudio callback. Native fixtures supply complete native
 * buffer/timestamp extents and serialize the same role ownership as the OS. */
int32_t mr_device_io_process(uint32_t device, const void *now, const void *input,
    const void *input_time, void *output, const void *output_time, void *context);
#endif
