/* Uses ONLY uniquely named temporary aggregate devices composed of the installed
 * ToneNari virtual device. All probes are private. The Rust recovery fixture now
 * owns its private aggregate in-process. No default-device/rate changes, microphone access,
 * driver restart or persistent graph edits. All output is silence. */
#include "../src/TNDeviceIO.c"
#include <assert.h>
#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <unistd.h>

static AudioDeviceID aggregate(const char *name) {
    CFStringRef uid = CFStringCreateWithCString(NULL, name, kCFStringEncodingUTF8);
    const void *subkeys[] = {CFSTR(kAudioSubDeviceUIDKey)};
    const void *subvalues[] = {CFSTR("io.tonenari.audio.endpoint.default")};
    CFDictionaryRef sub = CFDictionaryCreate(NULL, subkeys, subvalues, 1, &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks);
    const void *values[] = {sub};
    CFArrayRef subs = CFArrayCreate(NULL, values, 1, &kCFTypeArrayCallBacks);
    const void *keys[] = {CFSTR(kAudioAggregateDeviceUIDKey), CFSTR(kAudioAggregateDeviceNameKey), CFSTR(kAudioAggregateDeviceSubDeviceListKey), CFSTR(kAudioAggregateDeviceIsPrivateKey)};
    const void *data[] = {uid, uid, subs, kCFBooleanTrue};
    CFDictionaryRef desc = CFDictionaryCreate(NULL, keys, data, 4, &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks);
    AudioDeviceID id = kAudioObjectUnknown;
    OSStatus status = AudioHardwareCreateAggregateDevice(desc, &id);
    CFRelease(desc); CFRelease(subs); CFRelease(sub); CFRelease(uid);
    if (status != noErr) fprintf(stderr, "aggregate creation failed: %d\n", status);
    else fprintf(stderr, "owned aggregate created uid=%s device=%u\n", name, id);
    return status == noErr ? id : kAudioObjectUnknown;
}
static bool destroy_fixture(const char *uid, AudioDeviceID device) {
    if (AudioHardwareDestroyAggregateDevice(device) != noErr) return false;
    TNDeviceIO lookup = {.uid = (char *)uid};
    for (int i = 0; i < 40; ++i) {
        if (!resolve(&lookup)) {
            fprintf(stderr, "owned aggregate removed uid=%s\n", uid);
            return true;
        }
        usleep(50000);
    }
    fprintf(stderr, "owned aggregate still resolves after destroy uid=%s\n", uid);
    return false;
}
static bool connect(TNDeviceIO *io) {
    for (int i = 0; i < 40; ++i) { if (tn_device_io_connect(io)) return true; usleep(50000); }
    return false;
}
static bool progress(TNDeviceIO *io) {
    uint64_t ticks = tn_device_io_ticks(io);
    for (int i = 0; i < 40; ++i) { if (tn_device_io_ticks(io) > ticks + 4) return true; usleep(50000); }
    return false;
}
static int inventory(void) {
    AudioObjectPropertyAddress address = {kAudioHardwarePropertyDevices, kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain};
    UInt32 size = 0;
    if (AudioObjectGetPropertyDataSize(kAudioObjectSystemObject, &address, 0, NULL, &size) != noErr) return 2;
    AudioDeviceID *devices = malloc(size);
    if (!devices) return 2;
    if (AudioObjectGetPropertyData(kAudioObjectSystemObject, &address, 0, NULL, &size, devices) != noErr) { free(devices); return 2; }
    for (UInt32 i = 0; i < size / sizeof(*devices); ++i) {
        CFStringRef uid = NULL;
        Float64 rate = 0;
        if (!get(devices[i], kAudioDevicePropertyDeviceUID, kAudioObjectPropertyScopeGlobal, sizeof(uid), &uid) || !uid) continue;
        char text[512] = {0};
        CFStringGetCString(uid, text, sizeof(text), kCFStringEncodingUTF8);
        CFRelease(uid);
        get(devices[i], kAudioDevicePropertyNominalSampleRate, kAudioObjectPropertyScopeGlobal, sizeof(rate), &rate);
        printf("device=%u uid=%s rate=%.0f\n", devices[i], text, rate);
    }
    free(devices);
    AudioDeviceID input = 0, output = 0;
    get(kAudioObjectSystemObject, kAudioHardwarePropertyDefaultInputDevice, kAudioObjectPropertyScopeGlobal, sizeof(input), &input);
    get(kAudioObjectSystemObject, kAudioHardwarePropertyDefaultOutputDevice, kAudioObjectPropertyScopeGlobal, sizeof(output), &output);
    printf("default_input=%u default_output=%u\n", input, output);
    return 0;
}
/* Recovery of a failed fixture is deliberately exact-UID only. Never scan and
 * remove a prefix, delete a live owner's device, or touch a default endpoint. */
static int cleanup_owned(const char *uid) {
    const char *prefix = "io.tonenari.device-recovery-probe.";
    if (strncmp(uid, prefix, strlen(prefix))) return 7;
    char *end = NULL;
    long pid = strtol(uid + strlen(prefix), &end, 10);
    if (!end || *end || pid <= 0 || pid > INT32_MAX ||
        kill((pid_t)pid, 0) == 0 || errno != ESRCH) return 7;
    TNDeviceIO probe = {.uid = (char *)uid};
    if (!resolve(&probe)) return 0;
    AudioClassID kind = 0;
    AudioDeviceID input = 0, output = 0;
    CFArrayRef members = NULL;
    if (!get(probe.device, kAudioObjectPropertyClass, kAudioObjectPropertyScopeGlobal, sizeof(kind), &kind) ||
        kind != kAudioAggregateDeviceClassID ||
        !get(kAudioObjectSystemObject, kAudioHardwarePropertyDefaultInputDevice, kAudioObjectPropertyScopeGlobal, sizeof(input), &input) ||
        !get(kAudioObjectSystemObject, kAudioHardwarePropertyDefaultOutputDevice, kAudioObjectPropertyScopeGlobal, sizeof(output), &output) ||
        probe.device == input || probe.device == output ||
        !get(probe.device, kAudioAggregateDevicePropertyFullSubDeviceList, kAudioObjectPropertyScopeGlobal, sizeof(members), &members) || !members) return 7;
    bool owned = CFArrayGetCount(members) == 1 &&
        CFEqual(CFArrayGetValueAtIndex(members, 0), CFSTR("io.tonenari.audio.endpoint.default"));
    CFRelease(members);
    if (!owned) return 7;
    OSStatus result = AudioHardwareDestroyAggregateDevice(probe.device);
    printf("cleanup exact_uid=%s device=%u status=%d\n", uid, probe.device, result);
    return result == noErr ? 0 : 8;
}
int main(int argc, char **argv) {
    if (argc == 2 && !strcmp(argv[1], "--inventory")) return inventory();
    if (argc == 3 && !strcmp(argv[1], "--cleanup-owned")) return cleanup_owned(argv[2]);
    char uid[128];
    snprintf(uid, sizeof(uid), "io.tonenari.device-recovery-probe.%d", getpid());
    AudioDeviceID device = aggregate(uid);
    if (device == kAudioObjectUnknown) return 2;
    TNDeviceIO *in = tn_device_io_create(uid, true, 2, 48000);
    TNDeviceIO *out = tn_device_io_create(uid, false, 2, 48000);
    int result = 1;
    if (!in || !out) goto cleanup;
    if (!connect(in)) { fprintf(stderr, "input connect failed\n"); goto cleanup; }
    if (!connect(out)) { fprintf(stderr, "output connect failed\n"); goto cleanup; }
    if (!progress(in) || !progress(out)) { fprintf(stderr, "native callback progress failed\n"); goto cleanup; }
    for (int cycle = 0; cycle < 4; ++cycle) {
        uint64_t before_in = tn_device_io_ticks(in), before_out = tn_device_io_ticks(out);
        if (!destroy_fixture(uid, device)) goto cleanup;
        device = kAudioObjectUnknown;
        usleep(100000);
        if (tn_device_io_alive(in) || tn_device_io_alive(out)) goto cleanup;
        bool stopped_in = tn_device_io_disconnect(in);
        bool stopped_out = tn_device_io_disconnect(out);
        if (!stopped_in || !stopped_out) goto cleanup;
        device = aggregate(uid);
        if (!device || !connect(in) || !connect(out) || !progress(in) || !progress(out)) goto cleanup;
        printf("recovery=%d input_ticks=%llu output_ticks=%llu\n", cycle + 1,
            (unsigned long long)(tn_device_io_ticks(in) - before_in),
            (unsigned long long)(tn_device_io_ticks(out) - before_out));
    }
    result = 0;
cleanup: {
    bool destroyed_in = tn_device_io_destroy(in);
    bool destroyed_out = tn_device_io_destroy(out);
    if (!destroyed_in || !destroyed_out) result = 5;
    if (device && !destroy_fixture(uid, device)) result = 3;
    printf("result=%d; owned private aggregate destroyed; no default device or graph edits\n", result);
    return result;
    }
}
