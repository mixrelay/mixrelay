// Independent silent 128-frame aggregate client for disposable BlackHole diagnosis.
#include <CoreAudio/CoreAudio.h>
#include <CoreFoundation/CoreFoundation.h>
#include <poll.h>
#include <stddef.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef struct {
    _Atomic uint64_t callbacks, breaks, unexpected_frames;
    double next_sample;
    bool has_sample;
} Cadence;

static OSStatus render(AudioDeviceID device, const AudioTimeStamp *now,
                       const AudioBufferList *input, const AudioTimeStamp *input_time,
                       AudioBufferList *output, const AudioTimeStamp *output_time, void *context) {
    (void)device; (void)now; (void)input; (void)input_time;
    Cadence *state = context;
    UInt32 frames = 0;
    for (UInt32 i = 0; i < output->mNumberBuffers; i++) {
        AudioBuffer *buffer = &output->mBuffers[i];
        if (buffer->mNumberChannels != 0) {
            frames = buffer->mDataByteSize / (buffer->mNumberChannels * sizeof(float));
        }
        if (buffer->mData != NULL) memset(buffer->mData, 0, buffer->mDataByteSize);
    }
    if (frames != 128) atomic_fetch_add_explicit(&state->unexpected_frames, 1, memory_order_relaxed);
    if ((output_time->mFlags & kAudioTimeStampSampleTimeValid) != 0) {
        if (state->has_sample && output_time->mSampleTime != state->next_sample) {
            atomic_fetch_add_explicit(&state->breaks, 1, memory_order_relaxed);
        }
        state->next_sample = output_time->mSampleTime + frames;
        state->has_sample = true;
    }
    atomic_fetch_add_explicit(&state->callbacks, 1, memory_order_release);
    return noErr;
}

static int self_test(void) {
    Cadence state = {0};
    float samples[256];
    for (size_t i = 0; i < 256; i++) samples[i] = 0.5f;
    AudioBufferList output = {.mNumberBuffers = 1, .mBuffers = {{2, sizeof(samples), samples}}};
    AudioTimeStamp time = {.mSampleTime = 0, .mFlags = kAudioTimeStampSampleTimeValid};
    if (render(0, NULL, NULL, NULL, &output, &time, &state) != noErr) return 1;
    for (size_t i = 0; i < 256; i++) if (samples[i] != 0) return 1;
    time.mSampleTime = 128;
    render(0, NULL, NULL, NULL, &output, &time, &state);
    if (atomic_load(&state.breaks) != 0 || atomic_load(&state.unexpected_frames) != 0) return 1;
    time.mSampleTime = 512;
    render(0, NULL, NULL, NULL, &output, &time, &state);
    output.mBuffers[0].mDataByteSize /= 2;
    time.mSampleTime = 640;
    render(0, NULL, NULL, NULL, &output, &time, &state);
    return atomic_load(&state.callbacks) == 4 && atomic_load(&state.breaks) == 1 &&
                   atomic_load(&state.unexpected_frames) == 1 ? 0 : 1;
}

static OSStatus aggregate(AudioDeviceID *device) {
    char name[96];
    snprintf(name, sizeof(name), "io.mixrelay.diagnosis.cadence.%ld", (long)getpid());
    CFStringRef uid = CFStringCreateWithCString(NULL, name, kCFStringEncodingUTF8);
    const void *sub_keys[] = {CFSTR(kAudioSubDeviceUIDKey), CFSTR(kAudioSubDeviceDriftCompensationKey)};
    const void *sub_values[] = {CFSTR("BlackHole2ch_UID"), kCFBooleanFalse};
    CFDictionaryRef sub = CFDictionaryCreate(NULL, sub_keys, sub_values, 2,
                                              &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks);
    CFArrayRef devices = sub == NULL ? NULL : CFArrayCreate(NULL, (const void **)&sub, 1, &kCFTypeArrayCallBacks);
    const void *keys[] = {CFSTR(kAudioAggregateDeviceUIDKey), CFSTR(kAudioAggregateDeviceNameKey),
                         CFSTR(kAudioAggregateDeviceSubDeviceListKey), CFSTR(kAudioAggregateDeviceMainSubDeviceKey),
                         CFSTR(kAudioAggregateDeviceIsPrivateKey)};
    const void *values[] = {uid, CFSTR("Independent 128-frame cadence"), devices,
                           CFSTR("BlackHole2ch_UID"), kCFBooleanTrue};
    CFDictionaryRef description = uid == NULL || devices == NULL ? NULL :
        CFDictionaryCreate(NULL, keys, values, 5, &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks);
    OSStatus status = description == NULL ? kAudioHardwareUnspecifiedError :
        AudioHardwareCreateAggregateDevice(description, device);
    if (description != NULL) CFRelease(description);
    if (devices != NULL) CFRelease(devices);
    if (sub != NULL) CFRelease(sub);
    if (uid != NULL) CFRelease(uid);
    return status;
}

static OSStatus disable_input(AudioDeviceID device, AudioDeviceIOProcID proc) {
    AudioObjectPropertyAddress property = {kAudioDevicePropertyIOProcStreamUsage, kAudioObjectPropertyScopeInput, 0};
    UInt32 size = 0;
    OSStatus status = AudioObjectGetPropertyDataSize(device, &property, 0, NULL, &size);
    const size_t header = offsetof(AudioHardwareIOProcStreamUsage, mStreamIsOn);
    if (status != noErr || size < header || size > 65536) return kAudioHardwareBadPropertySizeError;
    const UInt32 capacity = size;
    AudioHardwareIOProcStreamUsage *usage = calloc(1, size);
    if (usage == NULL) return kAudioHardwareUnspecifiedError;
    _Static_assert(sizeof(usage->mIOProc) == sizeof(proc), "IOProc identifier size");
    memcpy(&usage->mIOProc, &proc, sizeof(proc));
    status = AudioObjectGetPropertyData(device, &property, 0, NULL, &size, usage);
    if (status == noErr && size >= header && size <= capacity && usage->mNumberStreams <= (size - header) / sizeof(UInt32)) {
        for (UInt32 i = 0; i < usage->mNumberStreams; i++) usage->mStreamIsOn[i] = 0;
        status = AudioObjectSetPropertyData(device, &property, 0, NULL, size, usage);
        if (status == noErr) status = AudioObjectGetPropertyData(device, &property, 0, NULL, &size, usage);
        if (status == noErr && size >= header && size <= capacity && usage->mNumberStreams <= (size - header) / sizeof(UInt32)) {
            for (UInt32 i = 0; i < usage->mNumberStreams; i++) {
                if (usage->mStreamIsOn[i] != 0) status = kAudioHardwareIllegalOperationError;
            }
        } else if (status == noErr) status = kAudioHardwareBadPropertySizeError;
    } else if (status == noErr) status = kAudioHardwareBadPropertySizeError;
    free(usage);
    return status;
}

int main(int argc, char **argv) {
    if (argc == 2 && strcmp(argv[1], "--self-test") == 0) return self_test();
    const char *hosted = getenv("GITHUB_ACTIONS");
    if (argc != 1 || hosted == NULL || strcmp(hosted, "true") != 0) {
        fprintf(stderr, "Usage: macos-cadence-control [--self-test]; native I/O requires disposable GitHub Actions\n");
        return 2;
    }
    AudioDeviceID device = kAudioObjectUnknown;
    AudioDeviceIOProcID proc = NULL;
    Cadence state = {0};
    bool started = false;
    OSStatus status = aggregate(&device);
    if (status != noErr) goto cleanup;
    Float64 rate = 48000;
    AudioObjectPropertyAddress property = {kAudioDevicePropertyNominalSampleRate, kAudioObjectPropertyScopeGlobal, 0};
    status = AudioObjectSetPropertyData(device, &property, 0, NULL, sizeof(rate), &rate);
    if (status != noErr) goto cleanup;
    UInt32 rate_size = sizeof(rate);
    status = AudioObjectGetPropertyData(device, &property, 0, NULL, &rate_size, &rate);
    if (status != noErr || rate != 48000) { status = kAudioHardwareIllegalOperationError; goto cleanup; }
    UInt32 frames = 128, size = sizeof(frames);
    property.mSelector = kAudioDevicePropertyBufferFrameSize;
    status = AudioObjectSetPropertyData(device, &property, 0, NULL, size, &frames);
    if (status != noErr) goto cleanup;
    status = AudioObjectGetPropertyData(device, &property, 0, NULL, &size, &frames);
    if (status != noErr || frames != 128) { status = kAudioHardwareIllegalOperationError; goto cleanup; }
    status = AudioDeviceCreateIOProcID(device, render, &state, &proc);
    if (status != noErr) goto cleanup;
    // This silent output client must not request separate microphone consent.
    status = disable_input(device, proc);
    if (status != noErr) goto cleanup;
    status = AudioDeviceStart(device, proc);
    if (status != noErr) goto cleanup;
    started = true;
    printf("{\"state\":\"ready\",\"bufferFrames\":%u,\"rate\":48000,\"inputStreamsEnabled\":false}\n", frames);
    fflush(stdout);
    // EOF stops the owned client. The bound also covers a lost parent.
    struct pollfd input = {.fd = STDIN_FILENO, .events = POLLIN | POLLHUP};
    if (poll(&input, 1, 120000) <= 0) status = kAudioHardwareUnspecifiedError;
cleanup:
    if (started) {
        OSStatus stopped = AudioDeviceStop(device, proc);
        if (stopped != noErr) status = stopped;
    }
    if (proc != NULL) {
        OSStatus destroyed = AudioDeviceDestroyIOProcID(device, proc);
        if (destroyed != noErr) status = destroyed;
    }
    if (device != kAudioObjectUnknown) {
        OSStatus destroyed = AudioHardwareDestroyAggregateDevice(device);
        if (destroyed != noErr) status = destroyed;
    }
    printf("{\"state\":\"stopped\",\"status\":%d,\"callbacks\":%llu,\"clockBreaks\":%llu,\"unexpectedFrames\":%llu}\n",
           (int)status, (unsigned long long)atomic_load(&state.callbacks),
           (unsigned long long)atomic_load(&state.breaks), (unsigned long long)atomic_load(&state.unexpected_frames));
    return status == noErr ? 0 : 1;
}
