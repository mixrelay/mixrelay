/* Apple COM vtable and dispatch-block syntax only. All object/property/format,
 * registry, clock, transport ownership and callback routing policy is Rust.
 * This is the production bundle's narrow Apple ABI entry point. */
#include <CoreAudio/AudioServerPlugIn.h>
#include <CoreFoundation/CFPlugInCOM.h>
#include <dispatch/dispatch.h>
#include <mach/mach_time.h>
#include <stdatomic.h>
#include <string.h>
#include <xpc/xpc.h>
#include "TNHALCore.h"
#include "TNHALControlProtocol.h"
#include "TNHALIO.h"

static TNHALCore *gCore;
static AudioServerPlugInHostRef gHost;
static _Atomic UInt32 gReferences = 1;
static xpc_connection_t gConnection;
static dispatch_source_t gTimer;
static dispatch_queue_t gControlQueue;
static bool gReconnect;
static uint64_t gConnectionGeneration;
static unsigned gHeartbeat;
static AudioServerPlugInDriverInterface gInterface;
static AudioServerPlugInDriverInterface *gInterfacePointer = &gInterface;
static AudioServerPlugInDriverRef gDriver = &gInterfacePointer;

_Static_assert(sizeof(AudioObjectPropertyAddress) == sizeof(TNHALCorePropertyAddress), "property ABI");
_Static_assert(sizeof(AudioStreamBasicDescription) == 40, "ASBD ABI");
_Static_assert(sizeof(AudioStreamRangedDescription) == 56, "ranged ASBD ABI");

static OSStatus result(int32_t status) {
    if (status == TN_HAL_CORE_OK) return noErr;
    if (status == TN_HAL_CORE_NOT_FOUND) return kAudioHardwareBadObjectError;
    return kAudioHardwareIllegalOperationError;
}
static const TNHALCorePropertyAddress *address(const AudioObjectPropertyAddress *value) {
    return (const TNHALCorePropertyAddress *)value;
}
static bool valid(AudioServerPlugInDriverRef driver) { return driver == gDriver && gCore != NULL; }

static void changed(void *context, uint32_t object, uint32_t change) {
    (void)context;
    TNHALCoreNotification notifications[16];
    const size_t count = tn_hal_core_notifications(object, change, notifications, 16);
    for (size_t index = 0; index < count; ++index) {
        AudioObjectPropertyAddress property;
        memcpy(&property, &notifications[index].address, sizeof(property));
        (void)gHost->PropertiesChanged(gHost, notifications[index].object_id, 1, &property);
    }
}
static bool request_configuration(void *context, uint32_t object, uint64_t action, uint64_t generation) {
    (void)context;
    return gHost->RequestDeviceConfigurationChange(gHost, object, action, (void *)(uintptr_t)generation) == noErr;
}
static void request_inventory(void) {
    if (gConnection == NULL) return;
    xpc_object_t request = tn_hal_control_create_inventory_request();
    if (request == NULL) return;
    xpc_connection_send_message(gConnection, request);
    xpc_release(request);
}
static void request_replacement(void) {
    uint8_t endpoint[16];
    uint64_t generation;
    if (gReconnect || gConnection == NULL || !tn_hal_core_replacement_request(gCore, gConnectionGeneration, endpoint, &generation)) return;
    xpc_object_t request = tn_hal_control_create_transport_replacement_request(endpoint, generation);
    if (request == NULL) return;
    xpc_connection_send_message(gConnection, request);
    xpc_release(request);
}
static void open_control_connection(void) {
    if (gConnectionGeneration == UINT64_MAX) return;
    if (gConnection != NULL) {
        xpc_connection_cancel(gConnection);
        xpc_release(gConnection);
        gConnection = NULL;
    }
    const uint64_t generation = ++gConnectionGeneration;
    gReconnect = true;
    if (!tn_hal_core_control_connected(gCore, generation)) return;
    gConnection = xpc_connection_create_mach_service(TN_HAL_CONTROL_SERVICE_NAME, gControlQueue, 0);
    if (gConnection != NULL) {
        gReconnect = false;
        xpc_connection_set_event_handler(gConnection, ^(xpc_object_t event) {
            if (gReconnect || generation != gConnectionGeneration) return;
            if (xpc_get_type(event) == XPC_TYPE_DICTIONARY) {
                (void)tn_hal_core_control_message(gCore, generation, event);
            } else if (event == XPC_ERROR_CONNECTION_INTERRUPTED || event == XPC_ERROR_CONNECTION_INVALID) {
                /* Reject all queued events from this connection immediately.
                 * A new connection gives Rust a fresh inventory authority. */
                gReconnect = true;
            }
        });
        xpc_connection_resume(gConnection);
        request_inventory();
    } else gReconnect = true;
}
static void control_tick(void) {
    tn_hal_core_tick(gCore);
    request_replacement();
    /* Reconnection must also replace the inventory generation namespace. */
    if (++gHeartbeat == 10) {
        gHeartbeat = 0;
        if (gReconnect) open_control_connection();
        else request_inventory();
    }
}
static void connect_control(void) {
    gControlQueue = dispatch_queue_create("io.tonenari.hal.control", DISPATCH_QUEUE_SERIAL);
    open_control_connection();
    gTimer = dispatch_source_create(DISPATCH_SOURCE_TYPE_TIMER, 0, 0, gControlQueue);
    if (gTimer != NULL) {
        dispatch_source_set_timer(gTimer, dispatch_time(DISPATCH_TIME_NOW, 100 * NSEC_PER_MSEC), 100 * NSEC_PER_MSEC, 10 * NSEC_PER_MSEC);
        dispatch_source_set_event_handler(gTimer, ^{
            control_tick();
        });
        dispatch_resume(gTimer);
    }
}

static HRESULT query(void *driver, REFIID uuid, LPVOID *output) {
    if (driver != gDriver || output == NULL) return E_NOINTERFACE;
    CFUUIDRef requested = CFUUIDCreateFromUUIDBytes(NULL, uuid);
    const bool supported = requested != NULL && (CFEqual(requested, IUnknownUUID) || CFEqual(requested, kAudioServerPlugInDriverInterfaceUUID));
    if (requested != NULL) CFRelease(requested);
    if (!supported) { *output = NULL; return E_NOINTERFACE; }
    atomic_fetch_add_explicit(&gReferences, 1, memory_order_relaxed);
    *output = gDriver;
    return S_OK;
}
static ULONG add_ref(void *driver) {
    if (driver != gDriver) return 0;
    return atomic_fetch_add_explicit(&gReferences, 1, memory_order_relaxed) + 1;
}
static ULONG release_ref(void *driver) {
    if (driver != gDriver) return 0;
    UInt32 value = atomic_load_explicit(&gReferences, memory_order_relaxed);
    while (value > 0 && !atomic_compare_exchange_weak_explicit(&gReferences, &value, value - 1, memory_order_relaxed, memory_order_relaxed)) {}
    /* The HAL owns the factory singleton for process lifetime. Releasing a COM
     * query does not destroy XPC blocks while callbacks can still arrive. */
    return value > 0 ? value - 1 : 0;
}
static OSStatus initialize(AudioServerPlugInDriverRef driver, AudioServerPlugInHostRef host) {
    if (driver != gDriver || host == NULL || gCore != NULL) return kAudioHardwareIllegalOperationError;
    if (tn_hal_core_io_context_size() != sizeof(TNHALIOContext) || tn_hal_core_io_context_alignment() != _Alignof(TNHALIOContext)) return kAudioHardwareIllegalOperationError;
    gHost = host;
    const TNHALCoreHostOps callbacks = {NULL, changed, request_configuration};
    gCore = tn_hal_core_create_native(&callbacks);
    if (gCore == NULL) return kAudioHardwareIllegalOperationError;
    connect_control();
    return noErr;
}
static OSStatus create_device(AudioServerPlugInDriverRef driver, CFDictionaryRef description, const AudioServerPlugInClientInfo *client, AudioObjectID *device) {
    (void)driver; (void)description; (void)client; (void)device;
    return kAudioHardwareUnsupportedOperationError;
}
static OSStatus destroy_device(AudioServerPlugInDriverRef driver, AudioObjectID device) {
    (void)driver; (void)device;
    return kAudioHardwareUnsupportedOperationError;
}
static OSStatus add_client(AudioServerPlugInDriverRef driver, AudioObjectID device, const AudioServerPlugInClientInfo *client) {
    return valid(driver) && client != NULL ? result(tn_hal_core_client(gCore, device, client->mClientID, true)) : kAudioHardwareBadObjectError;
}
static OSStatus remove_client(AudioServerPlugInDriverRef driver, AudioObjectID device, const AudioServerPlugInClientInfo *client) {
    return valid(driver) && client != NULL ? result(tn_hal_core_client(gCore, device, client->mClientID, false)) : kAudioHardwareBadObjectError;
}
static OSStatus perform(AudioServerPlugInDriverRef driver, AudioObjectID device, UInt64 action, void *info) {
    return valid(driver) ? result(tn_hal_core_perform(gCore, device, action, (uintptr_t)info)) : kAudioHardwareBadObjectError;
}
static OSStatus abort_change(AudioServerPlugInDriverRef driver, AudioObjectID device, UInt64 action, void *info) {
    return valid(driver) ? result(tn_hal_core_abort(gCore, device, action, (uintptr_t)info)) : kAudioHardwareBadObjectError;
}
static Boolean has_property(AudioServerPlugInDriverRef driver, AudioObjectID object, pid_t pid, const AudioObjectPropertyAddress *property) {
    (void)pid;
    return valid(driver) && tn_hal_core_has_property(gCore, object, address(property));
}
static OSStatus settable(AudioServerPlugInDriverRef driver, AudioObjectID object, pid_t pid, const AudioObjectPropertyAddress *property, Boolean *output) {
    (void)pid;
    if (!valid(driver) || output == NULL) return kAudioHardwareBadObjectError;
    bool value = false;
    OSStatus status = tn_hal_core_property_settable(gCore, object, address(property), &value);
    if (status == noErr) *output = value;
    return status;
}
static OSStatus property_size(AudioServerPlugInDriverRef driver, AudioObjectID object, pid_t pid, const AudioObjectPropertyAddress *property, UInt32 qualifier_size, const void *qualifier, UInt32 *output) {
    (void)pid; (void)qualifier_size; (void)qualifier;
    return valid(driver) ? tn_hal_core_property_size(gCore, object, address(property), output) : kAudioHardwareBadObjectError;
}
static OSStatus property_get(AudioServerPlugInDriverRef driver, AudioObjectID object, pid_t pid, const AudioObjectPropertyAddress *property, UInt32 qualifier_size, const void *qualifier, UInt32 capacity, UInt32 *written, void *output) {
    (void)pid;
    return valid(driver) ? tn_hal_core_property_get(gCore, object, address(property), qualifier_size, qualifier, capacity, written, output) : kAudioHardwareBadObjectError;
}
static OSStatus property_set(AudioServerPlugInDriverRef driver, AudioObjectID object, pid_t pid, const AudioObjectPropertyAddress *property, UInt32 qualifier_size, const void *qualifier, UInt32 size, const void *input) {
    (void)pid; (void)qualifier_size; (void)qualifier;
    return valid(driver) ? tn_hal_core_property_set(gCore, object, address(property), size, input) : kAudioHardwareBadObjectError;
}
static OSStatus start_io(AudioServerPlugInDriverRef driver, AudioObjectID device, UInt32 client) {
    return valid(driver) ? result(tn_hal_core_start(gCore, device, client, mach_absolute_time())) : kAudioHardwareBadObjectError;
}
static OSStatus stop_io(AudioServerPlugInDriverRef driver, AudioObjectID device, UInt32 client) {
    return valid(driver) ? result(tn_hal_core_stop(gCore, device, client)) : kAudioHardwareBadObjectError;
}
static OSStatus timestamp(AudioServerPlugInDriverRef driver, AudioObjectID device, UInt32 client, Float64 *sample, UInt64 *host, UInt64 *seed) {
    (void)client;
    if (!valid(driver) || sample == NULL || host == NULL || seed == NULL) return kAudioHardwareBadObjectError;
    TNHALCoreTimestamp value;
    if (!tn_hal_core_timestamp(gCore, device, mach_absolute_time(), &value)) return kAudioHardwareIllegalOperationError;
    *sample = (Float64)value.sample_time; *host = value.host_time; *seed = value.seed;
    return noErr;
}
static OSStatus will_io(AudioServerPlugInDriverRef driver, AudioObjectID device, UInt32 client, UInt32 operation, Boolean *will, Boolean *in_place) {
    (void)client;
    if (!valid(driver) || will == NULL || in_place == NULL || !tn_hal_core_device_alive(gCore, device)) return kAudioHardwareBadObjectError;
    *will = operation == kAudioServerPlugInIOOperationReadInput || operation == kAudioServerPlugInIOOperationWriteMix;
    *in_place = true;
    return noErr;
}
static OSStatus boundary_io(AudioServerPlugInDriverRef driver, AudioObjectID device, UInt32 client, UInt32 operation, UInt32 frames, const AudioServerPlugInIOCycleInfo *cycle) {
    (void)device; (void)client; (void)operation; (void)frames; (void)cycle;
    return valid(driver) ? noErr : kAudioHardwareBadObjectError;
}
static OSStatus do_io(AudioServerPlugInDriverRef driver, AudioObjectID device, AudioObjectID stream, UInt32 client, UInt32 operation, UInt32 frames, const AudioServerPlugInIOCycleInfo *cycle, void *main_buffer, void *secondary) {
    (void)client; (void)secondary;
    if (!valid(driver) || cycle == NULL || main_buffer == NULL) return kAudioHardwareBadObjectError;
    const bool render = operation == kAudioServerPlugInIOOperationWriteMix;
    if (!render && operation != kAudioServerPlugInIOOperationReadInput) return noErr;
    uint64_t frame;
    if (!tn_hal_io_sample_frame(render ? cycle->mOutputTime.mSampleTime : cycle->mInputTime.mSampleTime, &frame)) return kAudioHardwareIllegalOperationError;
    return tn_hal_core_io(gCore, device, stream, render, frame, main_buffer, frames) ? noErr : kAudioHardwareIllegalOperationError;
}

static AudioServerPlugInDriverInterface gInterface = {
    NULL, query, add_ref, release_ref, initialize, create_device, destroy_device,
    add_client, remove_client, perform, abort_change, has_property, settable,
    property_size, property_get, property_set, start_io, stop_io, timestamp,
    will_io, boundary_io, do_io, boundary_io
};

void *ToneNariHAL_Create(CFAllocatorRef allocator, CFUUIDRef requested) {
    (void)allocator;
    return requested != NULL && CFEqual(requested, kAudioServerPlugInTypeUUID) ? gDriver : NULL;
}
