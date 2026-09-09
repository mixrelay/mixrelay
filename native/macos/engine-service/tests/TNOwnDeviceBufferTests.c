/* Fake CoreAudio property boundary. No real device or global/default setting is
 * touched; every non-system call must target the translated exact ToneNari UID. */
#include "TNOwnDeviceBuffer.h"
#include <CoreAudio/CoreAudio.h>
#include <CoreFoundation/CoreFoundation.h>
#include <assert.h>
#include <math.h>
#include <stdio.h>
#include <string.h>

static AudioDeviceID device_id = 701U;
static UInt32 current_frames = 512U;
static AudioValueRange frame_range = {15.0, 4096.0};
static OSStatus lookup_status, get_status, range_status, settable_status, set_status;
static Boolean settable = true;
static bool short_lookup, short_value, short_range, acknowledge_only;
static unsigned writes;

OSStatus AudioObjectGetPropertyData(AudioObjectID object,
    const AudioObjectPropertyAddress *address, UInt32 qualifier_size,
    const void *qualifier, UInt32 *size, void *data) {
    assert(address->mScope == kAudioObjectPropertyScopeGlobal);
    assert(address->mElement == kAudioObjectPropertyElementMain);
    if (address->mSelector == kAudioHardwarePropertyTranslateUIDToDevice) {
        assert(object == kAudioObjectSystemObject);
        assert(qualifier_size == sizeof(CFStringRef) && qualifier != NULL);
        assert(CFEqual(*(const CFStringRef *)qualifier, CFSTR("io.tonenari.audio.endpoint.default")));
        memcpy(data, &device_id, sizeof(device_id));
        *size = short_lookup ? 1U : (UInt32)sizeof(device_id);
        return lookup_status;
    }
    assert(object == device_id && device_id != kAudioObjectUnknown);
    assert(qualifier_size == 0U && qualifier == NULL);
    if (address->mSelector == kAudioDevicePropertyBufferFrameSize) {
        memcpy(data, &current_frames, sizeof(current_frames));
        *size = short_value ? 1U : (UInt32)sizeof(current_frames);
        return get_status;
    }
    assert(address->mSelector == kAudioDevicePropertyBufferFrameSizeRange);
    memcpy(data, &frame_range, sizeof(frame_range));
    *size = short_range ? 1U : (UInt32)sizeof(frame_range);
    return range_status;
}

OSStatus AudioObjectIsPropertySettable(AudioObjectID object,
    const AudioObjectPropertyAddress *address, Boolean *result) {
    assert(object == device_id);
    assert(address->mSelector == kAudioDevicePropertyBufferFrameSize);
    *result = settable;
    return settable_status;
}

OSStatus AudioObjectSetPropertyData(AudioObjectID object,
    const AudioObjectPropertyAddress *address, UInt32 qualifier_size,
    const void *qualifier, UInt32 size, const void *data) {
    assert(object == device_id && object != kAudioObjectUnknown);
    assert(address->mSelector == kAudioDevicePropertyBufferFrameSize);
    assert(qualifier_size == 0U && qualifier == NULL && size == sizeof(UInt32));
    ++writes;
    if (set_status == noErr && !acknowledge_only) memcpy(&current_frames, data, sizeof(current_frames));
    return set_status;
}

int main(void) {
    assert(tn_own_device_buffer_frames() == 512U);
    const uint32_t offered[] = {64U, 128U, 256U, 512U};
    for (size_t index = 0U; index < sizeof(offered) / sizeof(offered[0]); ++index) {
        assert(tn_own_device_buffer_can_set(offered[index]));
        assert(tn_own_device_buffer_set(offered[index]));
        assert(tn_own_device_buffer_frames() == offered[index]);
    }
    assert(!tn_own_device_buffer_set(0U));
    assert(!tn_own_device_buffer_set(14U));
    assert(!tn_own_device_buffer_set(4097U));
    assert(writes == 4U);
    lookup_status = -1;
    assert(tn_own_device_buffer_frames() == 0U && !tn_own_device_buffer_set(128U));
    lookup_status = noErr;
    short_lookup = true;
    assert(tn_own_device_buffer_frames() == 0U);
    short_lookup = false;
    device_id = kAudioObjectUnknown;
    assert(tn_own_device_buffer_frames() == 0U && !tn_own_device_buffer_can_set(128U));
    device_id = 701U;
    short_value = true;
    assert(tn_own_device_buffer_frames() == 0U);
    short_value = false;
    get_status = -1;
    assert(tn_own_device_buffer_frames() == 0U);
    get_status = noErr;
    settable = false;
    assert(!tn_own_device_buffer_set(128U));
    settable = true;
    settable_status = -1;
    assert(!tn_own_device_buffer_set(128U));
    settable_status = noErr;
    range_status = -1;
    assert(!tn_own_device_buffer_set(128U));
    range_status = noErr;
    short_range = true;
    assert(!tn_own_device_buffer_set(128U));
    short_range = false;
    frame_range.mMinimum = NAN;
    assert(!tn_own_device_buffer_set(128U));
    frame_range = (AudioValueRange){15.0, INFINITY};
    assert(!tn_own_device_buffer_set(128U));
    frame_range = (AudioValueRange){15.0, 4096.0};
    set_status = -1;
    assert(!tn_own_device_buffer_set(128U));
    set_status = noErr;
    acknowledge_only = true;
    assert(tn_own_device_buffer_set(128U));
    assert(tn_own_device_buffer_frames() == 512U);
    puts("TNOwnDeviceBufferTests passed: exact UID, range/settable errors, acknowledgement vs observation");
    return 0;
}
