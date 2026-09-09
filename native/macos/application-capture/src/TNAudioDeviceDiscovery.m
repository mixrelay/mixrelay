#import "TNAudioDeviceDiscovery.h"

#import <CoreAudio/AudioHardware.h>
#import <Foundation/Foundation.h>
#include <stdlib.h>

static const size_t TNMaximumAudioDevices = 1024;

static TNProcessTapError TNDeviceError(TNProcessTapErrorCode code, OSStatus status) {
    return (TNProcessTapError){.code = code, .os_status = status};
}

static AudioObjectPropertyAddress TNDeviceProperty(
    AudioObjectPropertySelector selector,
    AudioObjectPropertyScope scope
) {
    return (AudioObjectPropertyAddress){
        .mSelector = selector,
        .mScope = scope,
        .mElement = kAudioObjectPropertyElementMain,
    };
}

static bool TNReadDeviceID(
    AudioObjectPropertySelector selector,
    AudioDeviceID *device_id
) {
    AudioObjectPropertyAddress address = TNDeviceProperty(
        selector,
        kAudioObjectPropertyScopeGlobal
    );
    UInt32 size = sizeof(*device_id);
    return AudioObjectGetPropertyData(
        kAudioObjectSystemObject,
        &address,
        0,
        NULL,
        &size,
        device_id
    ) == noErr;
}

static CFStringRef TNReadDeviceString(
    AudioDeviceID device_id,
    AudioObjectPropertySelector selector
) {
    AudioObjectPropertyAddress address = TNDeviceProperty(
        selector,
        kAudioObjectPropertyScopeGlobal
    );
    CFStringRef value = NULL;
    UInt32 size = sizeof(value);
    if (AudioObjectGetPropertyData(
        device_id,
        &address,
        0,
        NULL,
        &size,
        &value
    ) != noErr) {
        return NULL;
    }
    return value;
}

static bool TNReadDeviceFlag(
    AudioDeviceID device_id,
    AudioObjectPropertySelector selector,
    bool fallback
) {
    AudioObjectPropertyAddress address = TNDeviceProperty(
        selector,
        kAudioObjectPropertyScopeGlobal
    );
    UInt32 value = fallback ? 1U : 0U;
    UInt32 size = sizeof(value);
    if (AudioObjectGetPropertyData(
        device_id,
        &address,
        0,
        NULL,
        &size,
        &value
    ) != noErr) {
        return fallback;
    }
    return value != 0;
}

static uint32_t TNDeviceChannelCount(
    AudioDeviceID device_id,
    AudioObjectPropertyScope scope
) {
    AudioObjectPropertyAddress address = TNDeviceProperty(
        kAudioDevicePropertyStreamConfiguration,
        scope
    );
    UInt32 size = 0;
    if (AudioObjectGetPropertyDataSize(
        device_id,
        &address,
        0,
        NULL,
        &size
    ) != noErr || size < sizeof(AudioBufferList)) {
        return 0;
    }
    AudioBufferList *buffers = malloc(size);
    if (buffers == NULL) {
        return 0;
    }
    if (AudioObjectGetPropertyData(
        device_id,
        &address,
        0,
        NULL,
        &size,
        buffers
    ) != noErr) {
        free(buffers);
        return 0;
    }
    uint32_t channels = 0;
    for (UInt32 index = 0; index < buffers->mNumberBuffers; ++index) {
        const uint32_t next = buffers->mBuffers[index].mNumberChannels;
        channels = UINT32_MAX - channels < next ? UINT32_MAX : channels + next;
    }
    free(buffers);
    return channels;
}

TNProcessTapError tn_audio_device_visit(
    TNAudioDeviceVisitor visitor,
    void *context
) {
    if (visitor == NULL) {
        return TNDeviceError(TN_PROCESS_TAP_INVALID_ARGUMENT, noErr);
    }
    AudioObjectPropertyAddress address = TNDeviceProperty(
        kAudioHardwarePropertyDevices,
        kAudioObjectPropertyScopeGlobal
    );
    UInt32 byte_count = 0;
    OSStatus status = AudioObjectGetPropertyDataSize(
        kAudioObjectSystemObject,
        &address,
        0,
        NULL,
        &byte_count
    );
    if (status != noErr) {
        return TNDeviceError(TN_PROCESS_TAP_CORE_AUDIO_FAILURE, status);
    }
    const size_t device_count = byte_count / sizeof(AudioDeviceID);
    if (device_count > TNMaximumAudioDevices) {
        return TNDeviceError(TN_PROCESS_TAP_CORE_AUDIO_FAILURE, kAudioHardwareIllegalOperationError);
    }
    AudioDeviceID *device_ids = calloc(device_count, sizeof(*device_ids));
    if (device_ids == NULL && device_count > 0) {
        return TNDeviceError(TN_PROCESS_TAP_OUT_OF_MEMORY, noErr);
    }
    status = AudioObjectGetPropertyData(
        kAudioObjectSystemObject,
        &address,
        0,
        NULL,
        &byte_count,
        device_ids
    );
    if (status != noErr) {
        free(device_ids);
        return TNDeviceError(TN_PROCESS_TAP_CORE_AUDIO_FAILURE, status);
    }

    AudioDeviceID default_input = kAudioObjectUnknown;
    AudioDeviceID default_output = kAudioObjectUnknown;
    (void)TNReadDeviceID(kAudioHardwarePropertyDefaultInputDevice, &default_input);
    (void)TNReadDeviceID(kAudioHardwarePropertyDefaultOutputDevice, &default_output);

    for (size_t index = 0; index < device_count; ++index) {
        const AudioDeviceID device_id = device_ids[index];
        if (!TNReadDeviceFlag(device_id, kAudioDevicePropertyDeviceIsAlive, true) ||
            TNReadDeviceFlag(device_id, kAudioDevicePropertyIsHidden, false)) {
            continue;
        }
        CFStringRef uid = TNReadDeviceString(device_id, kAudioDevicePropertyDeviceUID);
        CFStringRef name = TNReadDeviceString(device_id, kAudioObjectPropertyName);
        if (uid == NULL || name == NULL) {
            if (uid != NULL) {
                CFRelease(uid);
            }
            if (name != NULL) {
                CFRelease(name);
            }
            continue;
        }
        NSString *uid_string = (__bridge NSString *)uid;
        NSString *name_string = (__bridge NSString *)name;
        if (uid_string.length > 0 && name_string.length > 0) {
            const uint32_t input_channels = TNDeviceChannelCount(
                device_id,
                kAudioObjectPropertyScopeInput
            );
            const uint32_t output_channels = TNDeviceChannelCount(
                device_id,
                kAudioObjectPropertyScopeOutput
            );
            if (input_channels > 0 || output_channels > 0) {
                visitor(
                    context,
                    uid_string.UTF8String,
                    name_string.UTF8String,
                    input_channels,
                    output_channels,
                    device_id == default_input,
                    device_id == default_output
                );
            }
        }
        CFRelease(uid);
        CFRelease(name);
    }
    free(device_ids);
    return TNDeviceError(TN_PROCESS_TAP_OK, noErr);
}
