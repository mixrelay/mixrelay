/* Opt-in owned signal source. Never uses or changes the system default output. */
#import <AVFAudio/AVFAudio.h>
#import <Foundation/Foundation.h>
#import <CoreAudio/CoreAudio.h>
#import <AudioUnit/AudioUnit.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static AudioDeviceID TNVirtualDevice(void) {
    CFStringRef uid = CFSTR("io.tonenari.audio.endpoint.default");
    AudioDeviceID device = kAudioObjectUnknown;
    AudioObjectPropertyAddress address = { kAudioHardwarePropertyTranslateUIDToDevice,
        kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain };
    UInt32 size = sizeof(device);
    if (AudioObjectGetPropertyData(kAudioObjectSystemObject, &address, sizeof(uid), &uid, &size, &device) != noErr || size != sizeof(device))
        return kAudioObjectUnknown;
    return device;
}

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        if (argc != 2 || strcmp(argv[1], "--tonenari-virtual-only") != 0) return EXIT_FAILURE;
        fprintf(stderr, "owned tone: resolving exact ToneNari UID\n");
        const AudioDeviceID device = TNVirtualDevice();
        if (device == kAudioObjectUnknown) {
            fprintf(stderr, "exact ToneNari virtual device absent; refusing default-output fallback\n");
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
        printf("ready io.tonenari.audio.endpoint.default\n");
        fflush(stdout);
        /* EOF/quit from the owning fixture is the lifetime boundary. */
        char command[32];
        (void)fgets(command, sizeof(command), stdin);
        [engine stop];
    }
    return EXIT_SUCCESS;
}
