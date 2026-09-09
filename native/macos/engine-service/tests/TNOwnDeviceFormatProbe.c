/* Manual integration probe. Default invocation is read-only. --set explicitly
 * changes ToneNari's device-global nominal rate and THIS PROCESS'S client IO
 * buffer, not another application's callback size. Never selects physical/default
 * devices. A fresh probe may correctly observe a different client buffer value.
 * --set-rate changes only the own endpoint nominal rate for OS-to-app following. */
#include "TNOwnDeviceBuffer.h"
#include <CoreAudio/CoreAudio.h>
#include <CoreFoundation/CoreFoundation.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static const AudioObjectPropertyAddress rate_address = {
    kAudioDevicePropertyNominalSampleRate, kAudioObjectPropertyScopeGlobal,
    kAudioObjectPropertyElementMain,
};

static AudioDeviceID own_device(void) {
    CFStringRef uid = CFSTR("io.tonenari.audio.endpoint.default");
    AudioDeviceID device = kAudioObjectUnknown;
    const AudioObjectPropertyAddress address = {
        kAudioHardwarePropertyTranslateUIDToDevice, kAudioObjectPropertyScopeGlobal,
        kAudioObjectPropertyElementMain,
    };
    UInt32 size = sizeof(device);
    if (AudioObjectGetPropertyData(kAudioObjectSystemObject, &address, sizeof(uid),
                                  &uid, &size, &device) != noErr || size != sizeof(device)) {
        return kAudioObjectUnknown;
    }
    return device;
}

static Float64 read_rate(AudioDeviceID device) {
    Float64 rate = 0.0;
    UInt32 size = sizeof(rate);
    if (AudioObjectGetPropertyData(device, &rate_address, 0U, NULL, &size, &rate) != noErr ||
        size != sizeof(rate)) return 0.0;
    return rate;
}

static bool set_rate(AudioDeviceID device, Float64 rate) {
    if (read_rate(device) == rate) return true;
    return AudioObjectSetPropertyData(device, &rate_address, 0U, NULL,
                                      sizeof(rate), &rate) == noErr;
}

static bool confirm(AudioDeviceID device, Float64 rate, uint32_t frames) {
    for (unsigned attempt = 0U; attempt < 200U; ++attempt) {
        if (read_rate(device) == rate &&
            (frames == 0U || tn_own_device_buffer_frames() == frames)) return true;
        (void)usleep(10000U);
    }
    return false;
}

static void print_format(AudioDeviceID device) {
    const AudioObjectPropertyAddress range_address = {
        kAudioDevicePropertyBufferFrameSizeRange, kAudioObjectPropertyScopeGlobal,
        kAudioObjectPropertyElementMain,
    };
    AudioValueRange range = {0};
    UInt32 size = sizeof(range);
    const OSStatus status = AudioObjectGetPropertyData(device, &range_address, 0U, NULL,
                                                      &size, &range);
    printf("{\"uid\":\"io.tonenari.audio.endpoint.default\",\"deviceId\":%u,"
           "\"sampleRate\":%.0f,\"clientBufferFrames\":%u,\"canSet256\":%s,"
           "\"rangeStatus\":%d,\"minimum\":%.0f,\"maximum\":%.0f}\n",
           (unsigned)device, read_rate(device), tn_own_device_buffer_frames(),
           tn_own_device_buffer_can_set(256U) ? "true" : "false", (int)status,
           range.mMinimum, range.mMaximum);
}

int main(int argc, char **argv) {
    const AudioDeviceID device = own_device();
    if (device == kAudioObjectUnknown) {
        fputs("Exact ToneNari endpoint UID is unavailable\n", stderr);
        return 1;
    }
    if (argc == 1) {
        print_format(device);
        return 0;
    }
    const bool rate_only = argc == 3 && strcmp(argv[1], "--set-rate") == 0;
    if (!rate_only && (argc != 4 || strcmp(argv[1], "--set") != 0)) {
        fputs("Usage: probe [--set-rate 44100|48000 | --set 44100|48000 64|128|256|512]\n", stderr);
        return 2;
    }
    // Exact strings avoid accepting overflow, signs or trailing input.
    const Float64 rate = strcmp(argv[2], "44100") == 0 ? 44100.0 :
                         strcmp(argv[2], "48000") == 0 ? 48000.0 : 0.0;
    if (rate_only) {
        const Float64 previous_rate = read_rate(device);
        if (rate == 0.0 || previous_rate == 0.0) {
            fputs("Unsupported request or unreadable native rate; no write performed\n", stderr);
            return 2;
        }
        if (!set_rate(device, rate) || !confirm(device, rate, 0U)) {
            const bool restored = set_rate(device, previous_rate) && confirm(device, previous_rate, 0U);
            fprintf(stderr, "Requested rate was not confirmed; restore confirmed: %s\n",
                    restored ? "true" : "false");
            return 1;
        }
        print_format(device);
        return 0;
    }
    uint32_t frames = 0U;
    const uint32_t supported[] = {64U, 128U, 256U, 512U};
    for (size_t index = 0U; index < sizeof(supported) / sizeof(supported[0]); ++index) {
        char text[8];
        (void)snprintf(text, sizeof(text), "%u", supported[index]);
        if (strcmp(argv[3], text) == 0) frames = supported[index];
    }
    const Float64 previous_rate = read_rate(device);
    const uint32_t previous_frames = tn_own_device_buffer_frames();
    if (rate == 0.0 || frames == 0U || previous_rate == 0.0 || previous_frames == 0U ||
        !tn_own_device_buffer_can_set(frames)) {
        fputs("Unsupported request or unreadable native format; no write performed\n", stderr);
        return 2;
    }
    if (!set_rate(device, rate) || !tn_own_device_buffer_set(frames) ||
        !confirm(device, rate, frames)) {
        const bool rate_restored = set_rate(device, previous_rate);
        const bool buffer_restored = tn_own_device_buffer_set(previous_frames);
        const bool restored = rate_restored && buffer_restored &&
                              confirm(device, previous_rate, previous_frames);
        fprintf(stderr, "Requested pair was not confirmed; restore confirmed: %s\n",
                restored ? "true" : "false");
        print_format(device);
        return 1;
    }
    print_format(device);
    return 0;
}
