#include "TNOwnDeviceBuffer.h"

#include <CoreAudio/CoreAudio.h>
#include <CoreFoundation/CoreFoundation.h>
#include <math.h>

static const AudioObjectPropertyAddress TN_BUFFER_ADDRESS = {
    kAudioDevicePropertyBufferFrameSize, kAudioObjectPropertyScopeGlobal,
    kAudioObjectPropertyElementMain,
};

static AudioDeviceID tn_own_buffer_device(void) {
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

uint32_t tn_own_device_buffer_frames(void) {
    const AudioDeviceID device = tn_own_buffer_device();
    UInt32 frames = 0U;
    UInt32 size = sizeof(frames);
    if (device == kAudioObjectUnknown ||
        AudioObjectGetPropertyData(device, &TN_BUFFER_ADDRESS, 0U, NULL, &size, &frames) != noErr ||
        size != sizeof(frames)) return 0U;
    return frames;
}

static bool tn_buffer_can_set(AudioDeviceID device, uint32_t frames) {
    Boolean settable = false;
    if (frames == 0U || device == kAudioObjectUnknown ||
        AudioObjectIsPropertySettable(device, &TN_BUFFER_ADDRESS, &settable) != noErr || !settable) {
        return false;
    }
    const AudioObjectPropertyAddress range_address = {
        kAudioDevicePropertyBufferFrameSizeRange, kAudioObjectPropertyScopeGlobal,
        kAudioObjectPropertyElementMain,
    };
    AudioValueRange range = {0};
    UInt32 size = sizeof(range);
    if (AudioObjectGetPropertyData(device, &range_address, 0U, NULL, &size, &range) != noErr ||
        size != sizeof(range) || !isfinite(range.mMinimum) || !isfinite(range.mMaximum)) {
        return false;
    }
    return (double)frames >= range.mMinimum && (double)frames <= range.mMaximum;
}

bool tn_own_device_buffer_can_set(uint32_t frames) {
    return tn_buffer_can_set(tn_own_buffer_device(), frames);
}

bool tn_own_device_buffer_set(uint32_t frames) {
    const AudioDeviceID device = tn_own_buffer_device();
    if (!tn_buffer_can_set(device, frames)) return false;
    // This changes only the calling client's IO view. Readback confirmation in
    // that same process must never be called device-global quantum negotiation.
    return AudioObjectSetPropertyData(device, &TN_BUFFER_ADDRESS, 0U, NULL,
                                      sizeof(frames), &frames) == noErr;
}
