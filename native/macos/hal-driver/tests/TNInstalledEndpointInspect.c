// Read-only acceptance probe: query the installed HAL, not the engine's desired state.
#include <CoreAudio/CoreAudio.h>
#include <stdio.h>
#include <stddef.h>
#include <stdlib.h>

static void require(OSStatus status) {
    if (status != noErr) {
        fprintf(stderr, "CoreAudio property failed: %d\n", (int)status);
        exit(2);
    }
}

static UInt32 channels(AudioDeviceID device, AudioObjectPropertyScope scope) {
    AudioObjectPropertyAddress address = {
        kAudioDevicePropertyStreamConfiguration, scope, kAudioObjectPropertyElementMain
    };
    UInt32 size = 0;
    require(AudioObjectGetPropertyDataSize(device, &address, 0, NULL, &size));
    if (size < offsetof(AudioBufferList, mBuffers) || size > 65536U) exit(2);
    AudioBufferList *buffers = calloc(1, size);
    if (buffers == NULL) exit(2);
    require(AudioObjectGetPropertyData(device, &address, 0, NULL, &size, buffers));
    if (buffers->mNumberBuffers > (size - offsetof(AudioBufferList, mBuffers)) / sizeof(AudioBuffer)) exit(2);
    UInt32 count = 0;
    for (UInt32 i = 0; i < buffers->mNumberBuffers; i++) count += buffers->mBuffers[i].mNumberChannels;
    free(buffers);
    return count;
}

int main(void) {
    CFStringRef uid = CFSTR("io.tonenari.audio.endpoint.default");
    AudioDeviceID device = kAudioObjectUnknown;
    AudioObjectPropertyAddress address = {
        kAudioHardwarePropertyTranslateUIDToDevice,
        kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain
    };
    UInt32 size = sizeof(device);
    require(AudioObjectGetPropertyData(kAudioObjectSystemObject, &address, sizeof(uid), &uid, &size, &device));
    if (device == kAudioObjectUnknown) return 2;
    address.mSelector = kAudioObjectPropertyName;
    CFStringRef name = NULL;
    size = sizeof(name);
    require(AudioObjectGetPropertyData(device, &address, 0, NULL, &size, &name));
    if (name == NULL) return 2;
    // CoreFoundation preserves localized names without hand-written escaping.
    int render = (int)channels(device, kAudioDevicePropertyScopeOutput);
    int capture = (int)channels(device, kAudioDevicePropertyScopeInput);
    CFNumberRef r = CFNumberCreate(NULL, kCFNumberIntType, &render);
    CFNumberRef c = CFNumberCreate(NULL, kCFNumberIntType, &capture);
    const void *keys[] = {CFSTR("displayName"), CFSTR("renderChannels"), CFSTR("captureChannels")};
    const void *values[] = {name, r, c};
    CFDictionaryRef result = CFDictionaryCreate(NULL, keys, values, 3,
        &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks);
    // Property lists preserve Unicode and are straightforward to consume with plutil.
    CFDataRef data = CFPropertyListCreateData(NULL, result, kCFPropertyListXMLFormat_v1_0, 0, NULL);
    if (data == NULL) return 2;
    fwrite(CFDataGetBytePtr(data), 1, (size_t)CFDataGetLength(data), stdout);
    CFRelease(data); CFRelease(result); CFRelease(r); CFRelease(c); CFRelease(name);
    return 0;
}
