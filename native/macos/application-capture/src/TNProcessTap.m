#import "TNProcessTap.h"

#import "TNAudioRing.h"
#include "../../common/TNCallbackClock.h"

#import <CoreAudio/AudioHardware.h>
#import <CoreAudio/AudioHardwareTapping.h>
#import <CoreAudio/CATapDescription.h>
#import <AppKit/AppKit.h>
#import <Foundation/Foundation.h>
#include <stdatomic.h>
#include <math.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <dispatch/dispatch.h>

static const size_t TNMaximumAudioProcesses = 4096;
static const size_t TNMinimumRingCapacityFrames = 256;
static const size_t TNMaximumRingCapacityFrames = 1U << 20U;
static NSString * const TNProcessRestoreEnabledKey = @"processRestoreEnabled";
static NSString * const TNBundleIDsKey = @"bundleIDs";
enum { TNMaximumChannels = 64 };
/* Failed Core Audio unregisters must retain their callback context. Admission is
 * capped so a permanently failing driver cannot grow this quarantine forever. */
enum { TNMaximumOwnedSessions = 32 };
static _Atomic unsigned TNOwnedSessions;

struct TNProcessTapSession {
    AudioObjectID tap_id;
    AudioObjectID aggregate_device_id;
    AudioDeviceIOProcID io_proc_id;
    AudioStreamBasicDescription format;
    TNAudioRing *ring;
    _Atomic uint64_t callback_count;
    _Atomic uint64_t captured_frames;
    _Atomic uint32_t peak_bits[TNMaximumChannels];
    _Atomic bool running;
    _Atomic bool retiring;
    _Atomic uint64_t format_epoch;
    _Atomic uint64_t validated_epoch;
    _Atomic uint64_t format_generation;
    _Atomic uint64_t sample_rate_bits;
    TNCallbackClock clock;
    _Atomic unsigned active_callbacks;
    _Atomic unsigned active_listeners;
    bool format_listener_registered;
    bool owns_admission;
    bool resume_after_refresh;
};

static double TNLoadSampleRate(const TNProcessTapSession *session) {
    uint64_t bits = atomic_load_explicit(&session->sample_rate_bits, memory_order_acquire);
    double rate;
    memcpy(&rate, &bits, sizeof(rate));
    return rate;
}

static void TNStoreSampleRate(TNProcessTapSession *session, double rate) {
    uint64_t bits;
    memcpy(&bits, &rate, sizeof(bits));
    atomic_store_explicit(&session->sample_rate_bits, bits, memory_order_release);
}

static bool TNDeliveryAllowed(const TNProcessTapSession *session) {
    return !atomic_load_explicit(&session->retiring, memory_order_acquire) &&
        atomic_load_explicit(&session->format_epoch, memory_order_acquire) ==
        atomic_load_explicit(&session->validated_epoch, memory_order_acquire);
}

static OSStatus TNFormatChanged(AudioObjectID object, UInt32 count,
    const AudioObjectPropertyAddress *addresses, void *context) {
    (void)object; (void)count; (void)addresses;
    TNProcessTapSession *session = context;
    atomic_fetch_add_explicit(&session->active_listeners, 1, memory_order_acquire);
    atomic_fetch_add_explicit(&session->format_epoch, 1, memory_order_release);
    atomic_fetch_sub_explicit(&session->active_listeners, 1, memory_order_release);
    return noErr;
}

static TNProcessTapError TNError(TNProcessTapErrorCode code, OSStatus status) {
    return (TNProcessTapError){.code = code, .os_status = status};
}

static void TNSetError(
    TNProcessTapError *destination,
    TNProcessTapErrorCode code,
    OSStatus status
) {
    if (destination != NULL) {
        *destination = TNError(code, status);
    }
}

static NSString *TNStringKey(const char *key) {
    return [NSString stringWithUTF8String:key];
}

static AudioObjectPropertyAddress TNPropertyAddress(AudioObjectPropertySelector selector) {
    return (AudioObjectPropertyAddress){
        .mSelector = selector,
        .mScope = kAudioObjectPropertyScopeGlobal,
        .mElement = kAudioObjectPropertyElementMain,
    };
}

static NSString *TNApplicationIconDataURL(NSImage *icon) {
    if (icon == nil) {
        return nil;
    }
    CGImageRef source = [icon CGImageForProposedRect:NULL context:nil hints:nil];
    if (source == NULL) {
        return nil;
    }

    static const size_t IconSize = 32;
    CGColorSpaceRef color_space = CGColorSpaceCreateDeviceRGB();
    if (color_space == NULL) {
        return nil;
    }
    const CGBitmapInfo bitmap_info =
        (CGBitmapInfo)kCGImageAlphaPremultipliedLast |
        (CGBitmapInfo)kCGBitmapByteOrder32Big;
    CGContextRef context = CGBitmapContextCreate(
        NULL,
        IconSize,
        IconSize,
        8,
        IconSize * 4,
        color_space,
        bitmap_info
    );
    CGColorSpaceRelease(color_space);
    if (context == NULL) {
        return nil;
    }
    CGContextSetInterpolationQuality(context, kCGInterpolationHigh);
    CGContextDrawImage(context, CGRectMake(0, 0, IconSize, IconSize), source);
    CGImageRef thumbnail = CGBitmapContextCreateImage(context);
    CGContextRelease(context);
    if (thumbnail == NULL) {
        return nil;
    }
    NSBitmapImageRep *representation = [[NSBitmapImageRep alloc] initWithCGImage:thumbnail];
    CGImageRelease(thumbnail);
    NSData *png = [representation representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
    if (png.length == 0) {
        return nil;
    }
    NSString *base64 = [png base64EncodedStringWithOptions:0];
    return [@"data:image/png;base64," stringByAppendingString:base64];
}

static NSArray<NSNumber *> *TNProcessIDsForBundleID(
    NSString *bundle_id,
    TNProcessTapError *error
) {
    AudioObjectPropertyAddress address = TNPropertyAddress(
        kAudioHardwarePropertyProcessObjectList
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
        TNSetError(error, TN_PROCESS_TAP_CORE_AUDIO_FAILURE, status);
        return nil;
    }
    const size_t process_count = byte_count / sizeof(AudioObjectID);
    if (process_count == 0 || process_count > TNMaximumAudioProcesses) {
        TNSetError(error, TN_PROCESS_TAP_APPLICATION_NOT_FOUND, noErr);
        return nil;
    }

    AudioObjectID *process_ids = calloc(process_count, sizeof(*process_ids));
    if (process_ids == NULL) {
        TNSetError(error, TN_PROCESS_TAP_OUT_OF_MEMORY, noErr);
        return nil;
    }
    status = AudioObjectGetPropertyData(
        kAudioObjectSystemObject,
        &address,
        0,
        NULL,
        &byte_count,
        process_ids
    );
    if (status != noErr) {
        free(process_ids);
        TNSetError(error, TN_PROCESS_TAP_CORE_AUDIO_FAILURE, status);
        return nil;
    }

    NSMutableArray<NSNumber *> *matches = [NSMutableArray array];
    for (size_t index = 0; index < process_count; ++index) {
        CFStringRef process_bundle_id = NULL;
        UInt32 property_size = sizeof(process_bundle_id);
        AudioObjectPropertyAddress bundle_address = TNPropertyAddress(
            kAudioProcessPropertyBundleID
        );
        status = AudioObjectGetPropertyData(
            process_ids[index],
            &bundle_address,
            0,
            NULL,
            &property_size,
            &process_bundle_id
        );
        if (status == noErr && process_bundle_id != NULL) {
            if ([bundle_id isEqualToString:(__bridge NSString *)process_bundle_id]) {
                [matches addObject:@(process_ids[index])];
            }
            CFRelease(process_bundle_id);
        }
    }
    free(process_ids);
    if (matches.count == 0) {
        TNSetError(error, TN_PROCESS_TAP_APPLICATION_NOT_FOUND, noErr);
        return nil;
    }
    return matches;
}

TNProcessTapError tn_process_tap_visit_applications(
    TNProcessTapApplicationVisitor visitor,
    void *context
) {
    if (visitor == NULL) {
        return TNError(TN_PROCESS_TAP_INVALID_ARGUMENT, noErr);
    }
    for (NSRunningApplication *application in [NSWorkspace sharedWorkspace].runningApplications) {
        if (application.terminated ||
            application.activationPolicy != NSApplicationActivationPolicyRegular) {
            continue;
        }
        NSString *bundle_id = application.bundleIdentifier;
        NSURL *application_url = application.bundleURL;
        NSString *display_name = application.localizedName;
        if (display_name.length == 0 && application_url != nil) {
            display_name = [[NSFileManager defaultManager] displayNameAtPath:application_url.path];
        }
        const char *bundle_id_utf8 = bundle_id.UTF8String;
        const char *display_name_utf8 = display_name.UTF8String;
        if (bundle_id_utf8 != NULL && bundle_id_utf8[0] != '\0' &&
            display_name_utf8 != NULL && display_name_utf8[0] != '\0') {
            NSImage *icon = application.icon;
            if (icon == nil && application_url != nil) {
                icon = [[NSWorkspace sharedWorkspace] iconForFile:application_url.path];
            }
            NSString *icon_data_url = TNApplicationIconDataURL(icon);
            visitor(
                context,
                bundle_id_utf8,
                display_name_utf8,
                icon_data_url.UTF8String,
                application.processIdentifier
            );
        }
    }
    return TNError(TN_PROCESS_TAP_OK, noErr);
}

static CATapDescription *TNCreateTapDescription(
    NSString *bundle_id,
    bool mute_while_captured,
    TNProcessTapError *error
) {
    const SEL bundle_ids_setter = NSSelectorFromString(@"setBundleIDs:");
    const bool supports_bundle_selection =
        [CATapDescription instancesRespondToSelector:bundle_ids_setter];
    NSArray<NSNumber *> *process_ids = supports_bundle_selection
        ? @[]
        : TNProcessIDsForBundleID(bundle_id, error);
    if (process_ids == nil) return nil;
    CATapDescription *description =
        [[CATapDescription alloc] initStereoMixdownOfProcesses:process_ids];
    if (supports_bundle_selection) {
        [description setValue:@[bundle_id] forKey:TNBundleIDsKey];
    }
    const SEL process_restore_setter = NSSelectorFromString(@"setProcessRestoreEnabled:");
    if ([CATapDescription instancesRespondToSelector:process_restore_setter]) {
        [description setValue:@YES forKey:TNProcessRestoreEnabledKey];
    }
    description.name = @"ToneNari Application Capture";
    description.private = YES;
    description.muteBehavior = mute_while_captured ? CATapMuted : CATapUnmuted;
    return description;
}

static bool TNFormatIsSupported(const AudioStreamBasicDescription *format) {
    const bool is_float = (format->mFormatFlags & kAudioFormatFlagIsFloat) != 0;
    const bool is_packed = (format->mFormatFlags & kAudioFormatFlagIsPacked) != 0;
    return format->mFormatID == kAudioFormatLinearPCM &&
        is_float &&
        is_packed &&
        (format->mFormatFlags & kAudioFormatFlagIsBigEndian) == kAudioFormatFlagsNativeEndian &&
        format->mBitsPerChannel == 32 &&
        isfinite(format->mSampleRate) && format->mSampleRate > 0 &&
        format->mBytesPerFrame == sizeof(float) *
            ((format->mFormatFlags & kAudioFormatFlagIsNonInterleaved) ? 1 : format->mChannelsPerFrame) &&
        format->mFramesPerPacket == 1 && format->mBytesPerPacket == format->mBytesPerFrame &&
        format->mChannelsPerFrame > 0 &&
        format->mChannelsPerFrame <= TNMaximumChannels;
}

static bool TNDeviceIsAlive(AudioObjectID device_id) {
    AudioObjectPropertyAddress address = TNPropertyAddress(kAudioDevicePropertyDeviceIsAlive);
    UInt32 alive = 0;
    UInt32 property_size = sizeof(alive);
    return AudioObjectGetPropertyData(
        device_id,
        &address,
        0,
        NULL,
        &property_size,
        &alive
    ) == noErr && alive != 0;
}

static bool TNWaitForDevice(AudioObjectID device_id) {
    static const size_t MaximumAttempts = 500;
    for (size_t attempt = 0; attempt < MaximumAttempts; ++attempt) {
        if (TNDeviceIsAlive(device_id)) {
            return true;
        }
        usleep(1000);
    }
    return false;
}

static size_t TNFrameCount(
    const TNProcessTapSession *session,
    const AudioBufferList *input_data
) {
    if (input_data == NULL || input_data->mNumberBuffers == 0) {
        return 0;
    }
    const AudioBuffer *buffer = &input_data->mBuffers[0];
    const UInt32 bytes_per_frame = session->format.mBytesPerFrame;
    const UInt32 channels = session->format.mChannelsPerFrame;
    const bool planar = (session->format.mFormatFlags & kAudioFormatFlagIsNonInterleaved) != 0;
    const UInt32 buffers = planar ? channels : 1;
    if (bytes_per_frame == 0 || input_data->mNumberBuffers != buffers ||
        buffer->mDataByteSize % bytes_per_frame != 0) return 0;
    for (UInt32 index = 0; index < buffers; ++index) {
        const AudioBuffer *part = &input_data->mBuffers[index];
        if (part->mData == NULL || part->mDataByteSize != buffer->mDataByteSize ||
            part->mNumberChannels != (planar ? 1 : channels)) return 0;
    }
    return buffer->mDataByteSize / bytes_per_frame;
}

static size_t TNWriteInput(
    TNProcessTapSession *session,
    const AudioBufferList *input_data
) {
    const size_t frames = TNFrameCount(session, input_data);
    if (frames == 0) {
        return 0;
    }
    const bool non_interleaved =
        (session->format.mFormatFlags & kAudioFormatFlagIsNonInterleaved) != 0;
    if (!non_interleaved) {
        return tn_audio_ring_write_interleaved(
            session->ring,
            input_data->mBuffers[0].mData,
            frames
        );
    }
    if (input_data->mNumberBuffers < session->format.mChannelsPerFrame) {
        return 0;
    }
    const float *channel_samples[TNMaximumChannels];
    for (UInt32 channel = 0; channel < session->format.mChannelsPerFrame; ++channel) {
        channel_samples[channel] = input_data->mBuffers[channel].mData;
    }
    return tn_audio_ring_write_planar(session->ring, channel_samples, frames);
}

static void TNRecordPeak(TNProcessTapSession *session, UInt32 channel, float sample) {
    const float magnitude = fabsf(sample);
    if (!isfinite(magnitude)) {
        return;
    }
    uint32_t proposed = 0;
    memcpy(&proposed, &magnitude, sizeof(proposed));
    uint32_t current = atomic_load_explicit(
        &session->peak_bits[channel],
        memory_order_relaxed
    );
    while (
        current < proposed &&
        !atomic_compare_exchange_weak_explicit(
            &session->peak_bits[channel],
            &current,
            proposed,
            memory_order_relaxed,
            memory_order_relaxed
        )
    ) {}
}

static void TNRecordPeaks(
    TNProcessTapSession *session,
    const AudioBufferList *input_data
) {
    const size_t frames = TNFrameCount(session, input_data);
    if (frames == 0) {
        return;
    }
    const UInt32 channels = session->format.mChannelsPerFrame;
    const bool non_interleaved =
        (session->format.mFormatFlags & kAudioFormatFlagIsNonInterleaved) != 0;
    if (!non_interleaved) {
        const float *samples = input_data->mBuffers[0].mData;
        if (samples == NULL) {
            return;
        }
        for (size_t frame = 0; frame < frames; ++frame) {
            for (UInt32 channel = 0; channel < channels; ++channel) {
                TNRecordPeak(session, channel, samples[frame * channels + channel]);
            }
        }
        return;
    }
    if (input_data->mNumberBuffers < channels) {
        return;
    }
    for (UInt32 channel = 0; channel < channels; ++channel) {
        const float *samples = input_data->mBuffers[channel].mData;
        if (samples == NULL) {
            continue;
        }
        for (size_t frame = 0; frame < frames; ++frame) {
            TNRecordPeak(session, channel, samples[frame]);
        }
    }
}

static OSStatus TNProcessTapIOProc(
    AudioObjectID device_id,
    const AudioTimeStamp *current_time,
    const AudioBufferList *input_data,
    const AudioTimeStamp *input_time,
    AudioBufferList *output_data,
    const AudioTimeStamp *output_time,
    void *client_data
) {
    (void)device_id;
    (void)current_time;
    (void)output_data;
    (void)output_time;
    TNProcessTapSession *session = client_data;
    atomic_fetch_add_explicit(&session->active_callbacks, 1, memory_order_acquire);
    atomic_fetch_add_explicit(&session->callback_count, 1, memory_order_relaxed);
    if (TNDeliveryAllowed(session)) {
        tn_callback_clock_record(&session->clock, input_time, TNFrameCount(session, input_data));
        TNRecordPeaks(session, input_data);
        const size_t written = TNWriteInput(session, input_data);
        atomic_fetch_add_explicit(&session->captured_frames, written, memory_order_relaxed);
    }
    atomic_fetch_sub_explicit(&session->active_callbacks, 1, memory_order_release);
    return noErr;
}

static bool TNCreateAggregateDevice(
    TNProcessTapSession *session,
    CATapDescription *description,
    TNProcessTapError *error
) {
    NSString *aggregate_uid = [NSString stringWithFormat:
        @"io.tonenari.capture.%@",
        NSUUID.UUID.UUIDString
    ];
    NSDictionary *tap = @{
        TNStringKey(kAudioSubTapUIDKey): description.UUID.UUIDString,
    };
    NSDictionary *aggregate = @{
        TNStringKey(kAudioAggregateDeviceNameKey): @"ToneNari Application Capture",
        TNStringKey(kAudioAggregateDeviceUIDKey): aggregate_uid,
        TNStringKey(kAudioAggregateDeviceIsPrivateKey): @YES,
        TNStringKey(kAudioAggregateDeviceTapListKey): @[tap],
    };
    const OSStatus status = AudioHardwareCreateAggregateDevice(
        (__bridge CFDictionaryRef)aggregate,
        &session->aggregate_device_id
    );
    if (status != noErr || !TNWaitForDevice(session->aggregate_device_id)) {
        TNSetError(error, TN_PROCESS_TAP_CORE_AUDIO_FAILURE, status);
        return false;
    }
    return true;
}

static bool TNUnregisterIO(TNProcessTapSession *session) {
    if (atomic_load_explicit(&session->running, memory_order_acquire)) {
        if (AudioDeviceStop(session->aggregate_device_id, session->io_proc_id) == noErr)
            atomic_store_explicit(&session->running, false, memory_order_release);
    }
    if (session->io_proc_id != NULL && session->aggregate_device_id != kAudioObjectUnknown) {
        if (AudioDeviceDestroyIOProcID(session->aggregate_device_id, session->io_proc_id) != noErr)
            return false;
        session->io_proc_id = NULL;
    }
    atomic_store_explicit(&session->running, false, memory_order_release);
    return atomic_load_explicit(&session->active_callbacks, memory_order_acquire) == 0;
}

static bool TNDestroyOwnedResources(TNProcessTapSession *session) {
    if (!TNUnregisterIO(session)) return false;
    if (session->format_listener_registered) {
        AudioObjectPropertyAddress address = TNPropertyAddress(kAudioTapPropertyFormat);
        if (AudioObjectRemovePropertyListener(session->tap_id, &address,
            TNFormatChanged, session) != noErr) return false;
        session->format_listener_registered = false;
    }
    if (atomic_load_explicit(&session->active_listeners, memory_order_acquire) != 0) return false;
    if (session->aggregate_device_id != kAudioObjectUnknown) {
        if (AudioHardwareDestroyAggregateDevice(session->aggregate_device_id) != noErr) return false;
        session->aggregate_device_id = kAudioObjectUnknown;
    }
    if (session->tap_id != kAudioObjectUnknown) {
        if (AudioHardwareDestroyProcessTap(session->tap_id) != noErr) return false;
        session->tap_id = kAudioObjectUnknown;
    }
    tn_audio_ring_destroy(session->ring);
    session->ring = NULL;
    return true;
}

static dispatch_queue_t TNCleanupQueue(void) {
    static dispatch_queue_t queue;
    static dispatch_once_t once;
    dispatch_once(&once, ^{ queue = dispatch_queue_create("io.tonenari.capture.retired", DISPATCH_QUEUE_SERIAL); });
    return queue;
}

static void TNRetryCleanup(TNProcessTapSession *session) {
    @autoreleasepool {
        if (!TNDestroyOwnedResources(session)) {
            dispatch_after(dispatch_time(DISPATCH_TIME_NOW, NSEC_PER_SEC), TNCleanupQueue(), ^{
                TNRetryCleanup(session);
            });
            return;
        }
        if (session->owns_admission) atomic_fetch_sub_explicit(&TNOwnedSessions, 1, memory_order_relaxed);
        free(session);
    }
}

TNProcessTapSession *tn_process_tap_create(
    const TNProcessTapConfig *config,
    TNProcessTapError *error
) {
    TNSetError(error, TN_PROCESS_TAP_OK, noErr);
    if (config == NULL || config->bundle_id == NULL || config->bundle_id[0] == '\0' ||
        config->ring_capacity_frames < TNMinimumRingCapacityFrames ||
        config->ring_capacity_frames > TNMaximumRingCapacityFrames) {
        TNSetError(error, TN_PROCESS_TAP_INVALID_ARGUMENT, noErr);
        return NULL;
    }
    {
        NSString *bundle_id = [NSString stringWithUTF8String:config->bundle_id];
        if (bundle_id == nil) {
            TNSetError(error, TN_PROCESS_TAP_INVALID_ARGUMENT, noErr);
            return NULL;
        }
        CATapDescription *description = TNCreateTapDescription(
            bundle_id,
            config->mute_while_captured,
            error
        );
        if (description == nil) {
            return NULL;
        }

        TNProcessTapSession *session = calloc(1, sizeof(*session));
        if (session == NULL) {
            TNSetError(error, TN_PROCESS_TAP_OUT_OF_MEMORY, noErr);
            return NULL;
        }
        unsigned owned = atomic_load_explicit(&TNOwnedSessions, memory_order_relaxed);
        while (owned < TNMaximumOwnedSessions && !atomic_compare_exchange_weak_explicit(
            &TNOwnedSessions, &owned, owned + 1, memory_order_relaxed, memory_order_relaxed)) {}
        if (owned >= TNMaximumOwnedSessions) {
            free(session);
            TNSetError(error, TN_PROCESS_TAP_RESOURCE_BUSY, noErr);
            return NULL;
        }
        session->owns_admission = true;
        tn_callback_clock_init(&session->clock);
        session->tap_id = kAudioObjectUnknown;
        session->aggregate_device_id = kAudioObjectUnknown;

        OSStatus status = AudioHardwareCreateProcessTap(description, &session->tap_id);
        if (status != noErr) {
            TNSetError(error, TN_PROCESS_TAP_CORE_AUDIO_FAILURE, status);
            tn_process_tap_destroy(session);
            return NULL;
        }
        AudioObjectPropertyAddress format_address = TNPropertyAddress(kAudioTapPropertyFormat);
        UInt32 format_size = sizeof(session->format);
        status = AudioObjectGetPropertyData(
            session->tap_id,
            &format_address,
            0,
            NULL,
            &format_size,
            &session->format
        );
        if (status != noErr || format_size != sizeof(session->format) || !TNFormatIsSupported(&session->format)) {
            TNSetError(
                error,
                status == noErr ? TN_PROCESS_TAP_UNSUPPORTED_FORMAT
                                : TN_PROCESS_TAP_CORE_AUDIO_FAILURE,
                status
            );
            tn_process_tap_destroy(session);
            return NULL;
        }
        TNStoreSampleRate(session, session->format.mSampleRate);
        atomic_store_explicit(&session->format_generation, 1, memory_order_release);
        status = AudioObjectAddPropertyListener(session->tap_id, &format_address, TNFormatChanged, session);
        if (status != noErr) {
            TNSetError(error, TN_PROCESS_TAP_CORE_AUDIO_FAILURE, status);
            tn_process_tap_destroy(session);
            return NULL;
        }
        session->format_listener_registered = true;
        session->ring = tn_audio_ring_create(
            config->ring_capacity_frames,
            session->format.mChannelsPerFrame
        );
        if (session->ring == NULL) {
            TNSetError(error, TN_PROCESS_TAP_OUT_OF_MEMORY, noErr);
            tn_process_tap_destroy(session);
            return NULL;
        }
        if (!TNCreateAggregateDevice(session, description, error)) {
            tn_process_tap_destroy(session);
            return NULL;
        }
        status = AudioDeviceCreateIOProcID(
            session->aggregate_device_id,
            TNProcessTapIOProc,
            session,
            &session->io_proc_id
        );
        if (status != noErr) {
            TNSetError(error, TN_PROCESS_TAP_CORE_AUDIO_FAILURE, status);
            tn_process_tap_destroy(session);
            return NULL;
        }
        return session;
    }
}

TNProcessTapError tn_process_tap_start(TNProcessTapSession *session) {
    if (session == NULL) {
        return TNError(TN_PROCESS_TAP_INVALID_ARGUMENT, noErr);
    }
    if (atomic_load_explicit(&session->running, memory_order_acquire)) {
        return TNError(TN_PROCESS_TAP_ALREADY_RUNNING, noErr);
    }
    if (session->io_proc_id == NULL || !TNDeliveryAllowed(session))
        return TNError(TN_PROCESS_TAP_RESOURCE_BUSY, noErr);
    const OSStatus status = AudioDeviceStart(
        session->aggregate_device_id,
        session->io_proc_id
    );
    if (status != noErr) {
        return TNError(TN_PROCESS_TAP_CORE_AUDIO_FAILURE, status);
    }
    atomic_store_explicit(&session->running, true, memory_order_release);
    return TNError(TN_PROCESS_TAP_OK, noErr);
}

TNProcessTapError tn_process_tap_refresh_format(TNProcessTapSession *session) {
    if (session == NULL || atomic_load_explicit(&session->retiring, memory_order_acquire))
        return TNError(TN_PROCESS_TAP_INVALID_ARGUMENT, noErr);
    const uint64_t observed_epoch = atomic_load_explicit(&session->format_epoch, memory_order_acquire);
    AudioStreamBasicDescription format = {0};
    AudioObjectPropertyAddress address = TNPropertyAddress(kAudioTapPropertyFormat);
    UInt32 size = sizeof(format);
    OSStatus status = AudioObjectGetPropertyData(session->tap_id, &address, 0, NULL, &size, &format);
    const bool same_geometry = format.mFormatID == session->format.mFormatID &&
        format.mFormatFlags == session->format.mFormatFlags &&
        format.mBytesPerFrame == session->format.mBytesPerFrame &&
        format.mChannelsPerFrame == session->format.mChannelsPerFrame &&
        format.mBitsPerChannel == session->format.mBitsPerChannel &&
        format.mFramesPerPacket == session->format.mFramesPerPacket &&
        format.mBytesPerPacket == session->format.mBytesPerPacket;
    if (status == noErr && size == sizeof(format) && TNFormatIsSupported(&format) &&
        same_geometry && format.mSampleRate == TNLoadSampleRate(session) &&
        observed_epoch == atomic_load_explicit(&session->validated_epoch, memory_order_acquire))
        return TNError(TN_PROCESS_TAP_OK, noErr);

    /* Invalidate before unregistering. Only the control owner drains the SPSC
     * ring, after the producer is gone. Its allocation/geometry stay immutable
     * for concurrent meter readers. A missed listener is detected by polling. */
    uint64_t epoch = observed_epoch;
    if (epoch == atomic_load_explicit(&session->validated_epoch, memory_order_acquire)) {
        uint64_t expected = epoch;
        if (atomic_compare_exchange_strong_explicit(&session->format_epoch, &expected,
            epoch + 1, memory_order_acq_rel, memory_order_acquire)) ++epoch;
    }
    session->resume_after_refresh |= atomic_load_explicit(&session->running, memory_order_acquire);
    if (!TNUnregisterIO(session)) return TNError(TN_PROCESS_TAP_RESOURCE_BUSY, noErr);
    if (status != noErr) return TNError(TN_PROCESS_TAP_CORE_AUDIO_FAILURE, status);
    if (size != sizeof(format) || !TNFormatIsSupported(&format) || !same_geometry)
        return TNError(TN_PROCESS_TAP_UNSUPPORTED_FORMAT, noErr);

    float discarded[256 * TNMaximumChannels];
    while (tn_audio_ring_read_interleaved(session->ring, discarded, 256) != 0) {}
    for (size_t channel = 0; channel < TNMaximumChannels; ++channel)
        atomic_store_explicit(&session->peak_bits[channel], 0, memory_order_relaxed);
    status = AudioDeviceCreateIOProcID(session->aggregate_device_id, TNProcessTapIOProc, session, &session->io_proc_id);
    if (status != noErr) return TNError(TN_PROCESS_TAP_CORE_AUDIO_FAILURE, status);
    TNStoreSampleRate(session, format.mSampleRate);
    atomic_fetch_add_explicit(&session->format_generation, 1, memory_order_release);
    atomic_store_explicit(&session->validated_epoch, epoch, memory_order_release);
    if (atomic_load_explicit(&session->format_epoch, memory_order_acquire) != epoch)
        return TNError(TN_PROCESS_TAP_RESOURCE_BUSY, noErr);
    if (session->resume_after_refresh) {
        TNProcessTapError result = tn_process_tap_start(session);
        if (result.code != TN_PROCESS_TAP_OK) {
            atomic_fetch_add_explicit(&session->format_epoch, 1, memory_order_release);
            return result;
        }
        session->resume_after_refresh = false;
    }
    return TNError(TN_PROCESS_TAP_OK, noErr);
}

TNProcessTapError tn_process_tap_stop(TNProcessTapSession *session) {
    if (session == NULL) {
        return TNError(TN_PROCESS_TAP_INVALID_ARGUMENT, noErr);
    }
    if (!atomic_load_explicit(&session->running, memory_order_acquire)) {
        return TNError(TN_PROCESS_TAP_NOT_RUNNING, noErr);
    }
    const OSStatus status = AudioDeviceStop(
        session->aggregate_device_id,
        session->io_proc_id
    );
    if (status == noErr) {
        atomic_store_explicit(&session->running, false, memory_order_release);
        session->resume_after_refresh = false;
    }
    return status == noErr
        ? TNError(TN_PROCESS_TAP_OK, noErr)
        : TNError(TN_PROCESS_TAP_CORE_AUDIO_FAILURE, status);
}

void tn_process_tap_destroy(TNProcessTapSession *session) {
    if (session == NULL) {
        return;
    }
    atomic_store_explicit(&session->retiring, true, memory_order_release);
    dispatch_async(TNCleanupQueue(), ^{ TNRetryCleanup(session); });
}

size_t tn_process_tap_read(
    TNProcessTapSession *session,
    float *destination,
    size_t maximum_frames
) {
    return session == NULL || !TNDeliveryAllowed(session)
        ? 0
        : tn_audio_ring_read_interleaved(session->ring, destination, maximum_frames);
}

size_t tn_process_tap_take_peaks(
    TNProcessTapSession *session,
    float *destination,
    size_t maximum_channels
) {
    if (session == NULL || destination == NULL || maximum_channels == 0) {
        return 0;
    }
    const size_t channels = session->format.mChannelsPerFrame < maximum_channels
        ? session->format.mChannelsPerFrame
        : maximum_channels;
    for (size_t channel = 0; channel < channels; ++channel) {
        const uint32_t bits = atomic_exchange_explicit(
            &session->peak_bits[channel],
            0,
            memory_order_acq_rel
        );
        memcpy(&destination[channel], &bits, sizeof(bits));
    }
    return channels;
}

TNProcessTapStats tn_process_tap_stats(const TNProcessTapSession *session) {
    if (session == NULL) {
        return (TNProcessTapStats){0};
    }
    TNCallbackClockSnapshot clock = {0};
    const bool clock_read = tn_callback_clock_snapshot(&session->clock, &clock);
    return (TNProcessTapStats){
        .callback_count = atomic_load_explicit(&session->callback_count, memory_order_relaxed),
        .captured_frames = atomic_load_explicit(&session->captured_frames, memory_order_relaxed),
        .dropped_frames = tn_audio_ring_dropped_frames(session->ring),
        .available_frames = tn_audio_ring_available_frames(session->ring),
        .channels = session->format.mChannelsPerFrame,
        .sample_rate = TNLoadSampleRate(session),
        .running = atomic_load_explicit(&session->running, memory_order_acquire),
        .format_generation = atomic_load_explicit(&session->format_generation, memory_order_acquire),
        .clock_discontinuities = atomic_load(&session->clock.discontinuities),
        .sample_time = clock.sample_time,
        .host_time = clock.host_time,
        .clock_valid = clock_read && clock.valid != 0,
    };
}
