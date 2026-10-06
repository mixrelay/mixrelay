/* Opt-in owned signal source. Never uses or changes the system default output. */
#import <AVFAudio/AVFAudio.h>
#import <Foundation/Foundation.h>
#import <CoreAudio/CoreAudio.h>
#import <AudioUnit/AudioUnit.h>
#include <math.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static AudioDeviceID MRVirtualDevice(const char *device_uid) {
    static const char prefix[] = "io.mixrelay.audio.endpoint.";
    const size_t prefix_size = sizeof(prefix) - 1;
    if (strlen(device_uid) != prefix_size + 32 ||
        strncmp(device_uid, prefix, prefix_size) != 0) return kAudioObjectUnknown;
    bool nonzero = false;
    for (size_t index = prefix_size; index < prefix_size + 32; ++index) {
        const char digit = device_uid[index];
        if (!((digit >= '0' && digit <= '9') || (digit >= 'a' && digit <= 'f')))
            return kAudioObjectUnknown;
        nonzero |= digit != '0';
    }
    if (!nonzero) return kAudioObjectUnknown;
    CFStringRef uid = CFStringCreateWithCString(NULL, device_uid, kCFStringEncodingUTF8);
    if (uid == NULL) return kAudioObjectUnknown;
    AudioDeviceID device = kAudioObjectUnknown;
    AudioObjectPropertyAddress address = { kAudioHardwarePropertyTranslateUIDToDevice,
        kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain };
    UInt32 size = sizeof(device);
    const OSStatus status = AudioObjectGetPropertyData(kAudioObjectSystemObject,
        &address, sizeof(uid), &uid, &size, &device);
    CFRelease(uid);
    return status == noErr && size == sizeof(device) ? device : kAudioObjectUnknown;
}

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        if (argc != 3 || strcmp(argv[1], "--mixrelay-virtual-only") != 0) return EXIT_FAILURE;
        fprintf(stderr, "owned tone: resolving exact MixRelay UID\n");
        const AudioDeviceID device = MRVirtualDevice(argv[2]);
        if (device == kAudioObjectUnknown) {
            fprintf(stderr, "exact MixRelay virtual device absent; refusing default-output fallback\n");
            return EXIT_FAILURE;
        }
        fprintf(stderr, "owned tone: configuring explicit virtual AudioUnit\n");
        AVAudioEngine *engine = [[AVAudioEngine alloc] init];
        AudioUnit unit = engine.outputNode.audioUnit;
        if (unit == NULL || AudioUnitSetProperty(unit, kAudioOutputUnitProperty_CurrentDevice,
                kAudioUnitScope_Global, 0, &device, sizeof(device)) != noErr) return EXIT_FAILURE;
        AudioDeviceID actual = kAudioObjectUnknown;
        UInt32 bytes = sizeof(actual);
        if (AudioUnitGetProperty(unit, kAudioOutputUnitProperty_CurrentDevice,
                kAudioUnitScope_Global, 0, &actual, &bytes) != noErr || actual != device) return EXIT_FAILURE;
        AVAudioFormat *format = [[AVAudioFormat alloc] initStandardFormatWithSampleRate:48000 channels:2];
        __block double phase = 0;
        AVAudioSourceNode *source = [[AVAudioSourceNode alloc] initWithFormat:format
            renderBlock:^OSStatus(BOOL *silent, const AudioTimeStamp *time,
                AVAudioFrameCount frames, AudioBufferList *output) {
                (void)time; *silent = NO;
                for (AVAudioFrameCount frame = 0; frame < frames; ++frame) {
                    const float sample = (float)(sin(phase * 2.0 * M_PI) * 0.05);
                    phase += 440.0 / 48000.0;
                    if (phase >= 1.0) phase -= 1.0;
                    for (UInt32 channel = 0; channel < output->mNumberBuffers; ++channel)
                        ((float *)output->mBuffers[channel].mData)[frame] = sample;
                }
                return noErr;
            }];
        [engine attachNode:source];
        [engine connect:source to:engine.mainMixerNode format:format];
        NSError *error = nil;
        fprintf(stderr, "owned tone: starting explicit virtual output\n");
        if (![engine startAndReturnError:&error]) {
            fprintf(stderr, "owned virtual tone start failed: %s\n", error.localizedDescription.UTF8String);
            return EXIT_FAILURE;
        }
        printf("ready %s\n", argv[2]);
        fflush(stdout);
        /* Only the owning fixture can move this source between exact owned UIDs. */
        char command[128];
        while (fgets(command, sizeof(command), stdin) != NULL) {
            command[strcspn(command, "\r\n")] = '\0';
            if (strcmp(command, "quit") == 0) break;
            if (strncmp(command, "route ", 6) != 0) {
                [engine stop];
                return EXIT_FAILURE;
            }
            const AudioDeviceID next = MRVirtualDevice(command + 6);
            if (next == kAudioObjectUnknown) {
                [engine stop];
                return EXIT_FAILURE;
            }
            [engine stop];
            if (AudioUnitSetProperty(unit, kAudioOutputUnitProperty_CurrentDevice,
                    kAudioUnitScope_Global, 0, &next, sizeof(next)) != noErr) return EXIT_FAILURE;
            actual = kAudioObjectUnknown;
            bytes = sizeof(actual);
            if (AudioUnitGetProperty(unit, kAudioOutputUnitProperty_CurrentDevice,
                    kAudioUnitScope_Global, 0, &actual, &bytes) != noErr || actual != next ||
                ![engine startAndReturnError:&error]) return EXIT_FAILURE;
            printf("ready %s\n", command + 6);
            fflush(stdout);
        }
        [engine stop];
    }
    return EXIT_SUCCESS;
}
