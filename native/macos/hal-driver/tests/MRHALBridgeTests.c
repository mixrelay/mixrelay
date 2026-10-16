/* The Rust owner tests control-resource lifetime and reconnect behavior.
 * This fixture exercises the retained SDK COM adapter without starting the HAL. */
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include "MRHALCore.h"
#include "MRHALControlProtocol.h"

static int32_t forwarded_pid;
static uint32_t rendered_client, rendered_device;
static void observed_render_client(const MRHALCore *core, uint32_t device, uint32_t client) {
    rendered_device = device; rendered_client = client;
    mr_hal_core_render_client(core, device, client);
}
#define mr_hal_core_render_client observed_render_client
static int32_t observed_get(MRHALCore *core, uint32_t object, const MRHALCorePropertyAddress *property, uint32_t qs, const void *q, uint32_t capacity, uint32_t *written, void *output, int32_t pid) {
    forwarded_pid = pid;
    return mr_hal_core_property_get(core, object, property, qs, q, capacity, written, output, pid);
}
#define mr_hal_core_property_get observed_get
#include "../driver/MixRelayHALBridge.c"
#undef mr_hal_core_property_get
#undef mr_hal_core_render_client

/* Keep the Rust property's stable FourCC projection tied to the active SDK. */
_Static_assert(kAudioVolumeControlClassID == UINT32_C(0x766c6d65), "volume class ABI");
_Static_assert(kAudioMuteControlClassID == UINT32_C(0x6d757465), "mute class ABI");
_Static_assert(kAudioLevelControlClassID == UINT32_C(0x6c65766c), "level base ABI");
_Static_assert(kAudioBooleanControlClassID == UINT32_C(0x746f676c), "boolean base ABI");
_Static_assert(kAudioLevelControlPropertyScalarValue == UINT32_C(0x6c637376), "scalar ABI");
_Static_assert(kAudioLevelControlPropertyDecibelValue == UINT32_C(0x6c636476), "decibel ABI");
_Static_assert(kAudioLevelControlPropertyDecibelRange == UINT32_C(0x6c636472), "dB range ABI");
_Static_assert(kAudioLevelControlPropertyConvertScalarToDecibels == UINT32_C(0x6c637364), "scalar conversion ABI");
_Static_assert(kAudioLevelControlPropertyConvertDecibelsToScalar == UINT32_C(0x6c636473), "dB conversion ABI");
_Static_assert(kAudioBooleanControlPropertyValue == UINT32_C(0x6263766c), "mute value ABI");
_Static_assert(kAudioControlPropertyScope == UINT32_C(0x63736370), "control scope ABI");
_Static_assert(kAudioControlPropertyElement == UINT32_C(0x63656c6d), "control element ABI");
_Static_assert(kAudioFormatFlagIsFloat == 1U, "Float32 ASBD flag ABI");
_Static_assert(kAudioFormatFlagIsSignedInteger == 4U, "signed integer ASBD flag ABI");
_Static_assert(kAudioFormatFlagIsPacked == 8U, "packed PCM ASBD flag ABI");

static bool inspect_fixture(int fd, size_t size, MRHALCoreGeometry *geometry) {
    (void)fd; (void)size;
    memset(geometry, 0, sizeof(*geometry));
    memset(geometry->endpoint_id, 1, 16);
    geometry->render_channels = geometry->capture_channels = 2;
    return true;
}
static void *create_fixture(const MRHALCoreGeometry *geometry, uint32_t rate, int fd, size_t size) {
    (void)geometry; (void)rate; (void)fd; (void)size;
    return malloc(1);
}
static void destroy_fixture(void *runtime) { free(runtime); }
static void fixture_changed(void *context, uint32_t object, uint32_t change) { (void)context; (void)object; (void)change; }
static bool fixture_request(void *context, uint32_t object, uint64_t action, uint64_t generation) {
    (void)context; (void)object; (void)action; (void)generation; return false;
}
static bool replace_fixture(void *runtime, const MRHALCoreGeometry *geometry, uint32_t rate, int fd, size_t size) {
    (void)runtime; (void)geometry; (void)rate; (void)fd; (void)size; return false;
}
static bool start_fixture(void *runtime, uint64_t time) { (void)runtime; (void)time; return true; }
static void stop_fixture(void *runtime) { (void)runtime; }
static bool rate_fixture(void *runtime, uint32_t rate) { (void)runtime; (void)rate; return false; }
static uint32_t transport_device;
static uint32_t transport_value(void) {
    AudioObjectPropertyAddress property = {kAudioDevicePropertyTransportType, kAudioObjectPropertyScopeGlobal, 0};
    uint32_t value = 0, written = 0;
    assert(property_get(gDriver, transport_device, getpid(), &property, 0, NULL, 4, &written, &value) == noErr);
    assert(written == 4);
    return value;
}
static bool timestamp_fixture(void *runtime, uint64_t time, MRHALCoreTimestamp *stamp) {
    (void)runtime; (void)time; (void)stamp;
    assert(transport_value() == kAudioDeviceTransportTypeVirtual);
    return false;
}
static bool io_fixture(void *runtime, bool render, uint64_t frame, float *data, uint32_t count) {
    (void)runtime; (void)render; (void)frame; (void)data; (void)count;
    assert(transport_value() == kAudioDeviceTransportTypeVirtual);
    return false;
}
static bool replacement_fixture(void *runtime) { (void)runtime; return false; }
static bool same_fixture(void *runtime, int fd, size_t size) { (void)runtime; (void)fd; (void)size; return false; }
static void transport_identity(void) {
    const MRHALCoreNativeOps native = {inspect_fixture, create_fixture, destroy_fixture, replace_fixture,
        start_fixture, stop_fixture, stop_fixture, rate_fixture, timestamp_fixture, io_fixture,
        replacement_fixture, same_fixture};
    const MRHALCoreHostOps host = {NULL, fixture_changed, fixture_request};
    gCore = mr_hal_core_create(&native, &host);
    assert(gCore != NULL);
    FILE *file = tmpfile();
    assert(file != NULL);
    uint8_t endpoint[16]; memset(endpoint, 1, sizeof(endpoint));
    uint32_t device = 0;
    assert(mr_hal_core_upsert(gCore, endpoint, "Fixture", 48000, dup(fileno(file)), 24, &device) == MR_HAL_CORE_OK);
    fclose(file);

    xpc_object_t inventory = mr_hal_control_create_inventory_snapshot(NULL, 0, 1);
    assert(inventory != NULL);
    xpc_object_t policy = xpc_array_create(NULL, 0);
    xpc_object_t bundle = xpc_string_create("com.test.Copied");
    xpc_array_append_value(policy, bundle);
    xpc_dictionary_set_value(inventory, "transport_compatibility", policy);
    xpc_release(bundle); xpc_release(policy);
    /* Avoid authoritative device removal: establish policy on an empty registry,
     * then recreate this fixture with its new object generation. */
    assert(mr_hal_core_control_connected(gCore, 1));
    assert(mr_hal_core_control_message(gCore, 1, inventory));
    xpc_release(inventory);
    file = tmpfile(); assert(file != NULL);
    assert(mr_hal_core_upsert(gCore, endpoint, "Fixture", 48000, dup(fileno(file)), 24, &device) == MR_HAL_CORE_OK);
    fclose(file);
    CFMutableStringRef identity = CFStringCreateMutable(NULL, 0);
    CFStringAppendCString(identity, "com.test.Copied", kCFStringEncodingUTF8);
    AudioServerPlugInClientInfo client = {.mClientID = 7, .mProcessID = getpid(), .mBundleID = identity};
    assert(add_client(gDriver, device, &client) == noErr);
    CFStringReplaceAll(identity, CFSTR("com.test.Changed"));
    CFRelease(identity);
    client.mBundleID = NULL;
    AudioObjectPropertyAddress property = {kAudioDevicePropertyTransportType, kAudioObjectPropertyScopeGlobal, 0};
    uint32_t value = 0, written = 0;
    assert(property_get(gDriver, device, getpid(), &property, 0, NULL, 4, &written, &value) == noErr);
    assert(forwarded_pid == getpid() && written == 4 && value == kAudioDeviceTransportTypeUSB);
    assert(property_get(gDriver, device, 0, &property, 0, NULL, 4, &written, &value) == noErr);
    assert(forwarded_pid == 0 && value == kAudioDeviceTransportTypeVirtual);
    transport_device = device;
    Boolean will = false, in_place = false;
    assert((*gDriver)->WillDoIOOperation(gDriver, device, 7, kAudioServerPlugInIOOperationWriteMix, &will, &in_place) == noErr);
    assert(will && in_place);
    /* WillDo is a startup query, not an IO-thread scope. */
    assert(transport_value() == kAudioDeviceTransportTypeUSB);
    assert(will_io(NULL, device, 7, 0, &will, &in_place) == kAudioHardwareBadObjectError);
    assert(transport_value() == kAudioDeviceTransportTypeUSB);
    AudioServerPlugInIOCycleInfo cycle = {0};
    float samples[2] = {0};
    assert(start_io(gDriver, device, 7) == noErr);
    assert((*gDriver)->BeginIOOperation(gDriver, device, 7, kAudioServerPlugInIOOperationWriteMix, 1, &cycle) == noErr);
    assert(transport_value() == kAudioDeviceTransportTypeVirtual);
    /* Native IO reenters the property path; its nested scope cannot end the cycle. */
    assert(do_io(gDriver, device, device + 2, 7, kAudioServerPlugInIOOperationWriteMix, 1, &cycle, samples, NULL) == kAudioHardwareIllegalOperationError);
    assert(transport_value() == kAudioDeviceTransportTypeVirtual);
    assert(rendered_client == 7 && rendered_device == device);
    assert(do_io(gDriver, device, device + 2, 7, 0, 1, NULL, samples, NULL) == kAudioHardwareBadObjectError);
    assert(transport_value() == kAudioDeviceTransportTypeVirtual);
    assert((*gDriver)->EndIOOperation(gDriver, device, 7, kAudioServerPlugInIOOperationWriteMix, 1, &cycle) == noErr);
    assert(transport_value() == kAudioDeviceTransportTypeUSB);
    /* Standalone DoIO and timestamp must clear every success/error return. */
    assert(do_io(gDriver, device, device + 2, 7, kAudioServerPlugInIOOperationWriteMix, 1, &cycle, samples, NULL) == kAudioHardwareIllegalOperationError);
    assert(transport_value() == kAudioDeviceTransportTypeUSB);
    assert(do_io(NULL, device, device + 2, 7, 0, 1, &cycle, samples, NULL) == kAudioHardwareBadObjectError);
    assert(transport_value() == kAudioDeviceTransportTypeUSB);
    assert(do_io(gDriver, device, device + 2, 7, 0, 1, &cycle, samples, NULL) == noErr);
    assert(transport_value() == kAudioDeviceTransportTypeUSB);
    Float64 sample = 0; UInt64 time = 0, seed = 0;
    assert(timestamp(gDriver, device, 7, &sample, &time, &seed) == kAudioHardwareIllegalOperationError);
    assert(transport_value() == kAudioDeviceTransportTypeUSB);
    assert(timestamp(NULL, device, 7, &sample, &time, &seed) == kAudioHardwareBadObjectError);
    assert(transport_value() == kAudioDeviceTransportTypeUSB);
    /* The Rust FFI entry points also scope IO when called without the bridge. */
    MRHALCoreTimestamp stamp = {0};
    assert(!mr_hal_core_timestamp(gCore, device, mach_absolute_time(), &stamp));
    assert(transport_value() == kAudioDeviceTransportTypeUSB);
    assert(!mr_hal_core_io(gCore, device, device + 2, true, 0, samples, 1));
    assert(transport_value() == kAudioDeviceTransportTypeUSB);
    assert(begin_io(NULL, device, 7, 0, 1, &cycle) == kAudioHardwareBadObjectError);
    assert(transport_value() == kAudioDeviceTransportTypeUSB);
    assert(stop_io(gDriver, device, 7) == noErr);
    property.mSelector = kAudioDevicePropertyDeviceIsAlive;
    assert(property_get(gDriver, device, getpid(), &property, 0, NULL, 4, &written, &value) == noErr);
    assert(forwarded_pid == getpid() && value == 1);
    assert(remove_client(gDriver, device, &client) == noErr);
    client.mClientID = 8;
    assert(add_client(gDriver, device, &client) == noErr);
    assert(remove_client(gDriver, device, &client) == noErr);
    assert(add_client(gDriver, device, NULL) == kAudioHardwareBadObjectError);
    mr_hal_core_destroy(gCore); gCore = NULL;
}

/* The SDK GetPropertyData entry must forward every requester: an app reads the
 * measured input latency, while coreaudiod (PID 0) reads only the change marker. */
static void input_latency_requester(void) {
    const MRHALCoreNativeOps native = {inspect_fixture, create_fixture, destroy_fixture, replace_fixture,
        start_fixture, stop_fixture, stop_fixture, rate_fixture, timestamp_fixture, io_fixture,
        replacement_fixture, same_fixture};
    const MRHALCoreHostOps host = {NULL, fixture_changed, fixture_request};
    gCore = mr_hal_core_create(&native, &host);
    assert(gCore != NULL);
    FILE *file = tmpfile();
    assert(file != NULL);
    uint8_t endpoint[16]; memset(endpoint, 1, sizeof(endpoint));
    uint32_t device = 0;
    assert(mr_hal_core_upsert(gCore, endpoint, "Fixture", 48000, dup(fileno(file)), 24, &device) == MR_HAL_CORE_OK);
    fclose(file);

    xpc_object_t inventory = mr_hal_control_create_inventory_snapshot(NULL, 0, 1);
    assert(inventory != NULL);
    xpc_object_t latencies = xpc_array_create(NULL, 0);
    xpc_object_t latency = xpc_dictionary_create(NULL, NULL, 0);
    xpc_dictionary_set_uint64(latency, "version", 1);
    xpc_dictionary_set_string(latency, "operation", "latency");
    xpc_dictionary_set_data(latency, "endpoint_id", endpoint, sizeof(endpoint));
    xpc_dictionary_set_uint64(latency, "rate", 48000);
    xpc_dictionary_set_uint64(latency, "engine_pid", 4242);
    xpc_dictionary_set_uint64(latency, "lease", 1);
    xpc_dictionary_set_uint64(latency, "revision", 1);
    xpc_dictionary_set_uint64(latency, "known", 1);
    xpc_dictionary_set_uint64(latency, "frames", 321);
    xpc_array_append_value(latencies, latency);
    xpc_dictionary_set_value(inventory, "input_latencies", latencies);
    xpc_release(latency); xpc_release(latencies);
    assert(mr_hal_core_control_connected(gCore, 1));
    assert(mr_hal_core_control_message(gCore, 1, inventory));
    xpc_release(inventory);

    AudioObjectPropertyAddress property = {kAudioDevicePropertyLatency, kAudioObjectPropertyScopeInput, 0};
    uint32_t value = 0, written = 0;
    assert(property_get(gDriver, device, getpid(), &property, 0, NULL, 4, &written, &value) == noErr);
    assert(forwarded_pid == getpid() && written == 4 && value == 321);
    /* One published change so far: coreaudiod's marker reads 1, never the frames. */
    assert(property_get(gDriver, device, 0, &property, 0, NULL, 4, &written, &value) == noErr);
    assert(forwarded_pid == 0 && written == 4 && value == 1);
    mr_hal_core_destroy(gCore); gCore = NULL;
}

int main(void) {
    assert(MixRelayHAL_Create(NULL, NULL) == NULL);
    assert(MixRelayHAL_Create(NULL, kAudioServerPlugInTypeUUID) == gDriver);
    assert(add_ref(NULL) == 0 && release_ref(NULL) == 0);
    assert(initialize(gDriver, NULL) == kAudioHardwareIllegalOperationError);
    assert(gCore == NULL);
    void *output = NULL;
    assert(query(gDriver, CFUUIDGetUUIDBytes(IUnknownUUID), &output) == S_OK);
    assert(output == gDriver);
    assert(release_ref(gDriver) == 1);
    assert(query(gDriver, CFUUIDGetUUIDBytes(kAudioServerPlugInDriverInterfaceUUID), &output) == S_OK);
    assert(output == gDriver);
    assert(release_ref(gDriver) == 1);
    CFUUIDRef unknown = CFUUIDCreate(NULL);
    assert(unknown != NULL);
    assert(query(gDriver, CFUUIDGetUUIDBytes(unknown), &output) == E_NOINTERFACE);
    assert(output == NULL);
    CFRelease(unknown);
    assert(release_ref(gDriver) == 0);
    assert(release_ref(gDriver) == 0);
    assert(add_ref(gDriver) == 1);
    MRHALCoreNotification latency_notice = {0};
    assert(mr_hal_core_notifications(10, MR_HAL_CORE_CHANGE_INPUT_LATENCY, &latency_notice, 1) == 1);
    assert(latency_notice.object_id == 10);
    assert(latency_notice.address.selector == kAudioDevicePropertyLatency);
    assert(latency_notice.address.scope == kAudioObjectPropertyScopeInput);
    transport_identity();
    input_latency_requester();
    puts("PASS HAL COM lifetime, transport and latency PID forwarding, copied and nullable bundle identity, balanced IO scopes");
    return 0;
}
