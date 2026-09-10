/* Objective-C compiler bridge only. Rust owns capture admission, native handles,
 * process selection, callbacks, reconciliation, and deferred resource retirement. */
#import "MRProcessTap.h"
#import <CoreAudio/AudioHardware.h>
#import <CoreAudio/AudioHardwareTapping.h>
#import <CoreAudio/CATapDescription.h>
#import <AppKit/AppKit.h>
#import <Foundation/Foundation.h>

static NSString *MRApplicationIconDataURL(NSImage *icon) {
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

MRProcessTapError mr_process_tap_visit_applications(
    MRProcessTapApplicationVisitor visitor,
    void *context
) {
    if (visitor == NULL) {
        return ((MRProcessTapError){MR_PROCESS_TAP_INVALID_ARGUMENT, noErr});
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
            NSString *icon_data_url = MRApplicationIconDataURL(icon);
            visitor(
                context,
                bundle_id_utf8,
                display_name_utf8,
                icon_data_url.UTF8String,
                application.processIdentifier
            );
        }
    }
    return ((MRProcessTapError){MR_PROCESS_TAP_OK, noErr});
}


bool mr_tap_description_supports_bundle_selection(void) {
    return [CATapDescription instancesRespondToSelector:NSSelectorFromString(@"setBundleIDs:")];
}

void *mr_tap_description_create(const char *bundle, const AudioObjectID *processes,
    size_t count, bool mute) {
    @autoreleasepool {
        NSMutableArray<NSNumber *> *ids = [NSMutableArray arrayWithCapacity:count];
        for (size_t index = 0; index < count; ++index) [ids addObject:@(processes[index])];
        CATapDescription *description = [[CATapDescription alloc] initStereoMixdownOfProcesses:ids];
        if (mr_tap_description_supports_bundle_selection()) {
            [description setValue:@[[NSString stringWithUTF8String:bundle]] forKey:@"bundleIDs"];
        }
        if ([CATapDescription instancesRespondToSelector:NSSelectorFromString(@"setProcessRestoreEnabled:")]) {
            [description setValue:@YES forKey:@"processRestoreEnabled"];
        }
        description.name = @"MixRelay Application Capture";
        description.private = YES;
        description.muteBehavior = mute ? CATapMuted : CATapUnmuted;
        return (__bridge_retained void *)description;
    }
}

void mr_tap_description_release(void *description) {
    (void)(__bridge_transfer CATapDescription *)description;
}

OSStatus mr_tap_aggregate_create(void *raw_description, AudioObjectID *device) {
    @autoreleasepool {
        CATapDescription *description = (__bridge CATapDescription *)raw_description;
        NSString *(^key)(const char *) = ^NSString *(const char *value) {
            return [NSString stringWithUTF8String:value];
        };
        NSDictionary *tap = @{key(kAudioSubTapUIDKey): description.UUID.UUIDString};
        NSDictionary *aggregate = @{
            key(kAudioAggregateDeviceNameKey): @"MixRelay Application Capture",
            key(kAudioAggregateDeviceUIDKey): [@"io.mixrelay.capture." stringByAppendingString:NSUUID.UUID.UUIDString],
            key(kAudioAggregateDeviceIsPrivateKey): @YES,
            key(kAudioAggregateDeviceTapListKey): @[tap],
        };
        return AudioHardwareCreateAggregateDevice((__bridge CFDictionaryRef)aggregate, device);
    }
}
