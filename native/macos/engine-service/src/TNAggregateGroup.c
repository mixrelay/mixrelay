#include "TNAggregateGroup.h"
#include <CoreAudio/CoreAudio.h>
#include <CoreFoundation/CoreFoundation.h>
#include <stddef.h>
#include <stdlib.h>
#include <string.h>

enum { TN_GROUP_MEMBERS = 32, TN_GROUP_CHANNELS = 2048 };
typedef struct Member {
    CFStringRef uid;
    uint32_t device, input, output;
} Member;
struct TNAggregateGroup {
    AudioDeviceID device;
    uint32_t count, primary;
    Float64 rate;
    Member members[TN_GROUP_MEMBERS];
};

static bool property(AudioObjectID id, AudioObjectPropertySelector selector,
    AudioObjectPropertyScope scope, UInt32 size, void *out) {
    AudioObjectPropertyAddress address = {selector, scope, kAudioObjectPropertyElementMain};
    UInt32 actual = size;
    return AudioObjectGetPropertyData(id, &address, 0, NULL, &actual, out) == noErr && actual == size;
}
static AudioDeviceID resolve_uid(CFStringRef uid) {
    AudioDeviceID device = kAudioObjectUnknown;
    AudioObjectPropertyAddress address = {kAudioHardwarePropertyTranslateUIDToDevice,
        kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain};
    UInt32 size = sizeof(device);
    if (AudioObjectGetPropertyData(kAudioObjectSystemObject, &address,
        sizeof(uid), &uid, &size, &device) != noErr) return kAudioObjectUnknown;
    return device;
}
static bool channel_count(AudioDeviceID device, bool input, uint32_t *count) {
    AudioObjectPropertyAddress address = {kAudioDevicePropertyStreamConfiguration,
        input ? kAudioDevicePropertyScopeInput : kAudioDevicePropertyScopeOutput,
        kAudioObjectPropertyElementMain};
    UInt32 size = 0;
    if (AudioObjectGetPropertyDataSize(device, &address, 0, NULL, &size) != noErr ||
        size < offsetof(AudioBufferList, mBuffers) || size > 65536) return false;
    AudioBufferList *buffers = calloc(1, size);
    if (!buffers) return false;
    bool ok = AudioObjectGetPropertyData(device, &address, 0, NULL, &size, buffers) == noErr &&
        size >= offsetof(AudioBufferList, mBuffers) &&
        buffers->mNumberBuffers <= (size - offsetof(AudioBufferList, mBuffers)) / sizeof(AudioBuffer);
    *count = 0;
    if (ok) for (UInt32 i = 0; i < buffers->mNumberBuffers; ++i) {
        if (buffers->mBuffers[i].mNumberChannels > TN_GROUP_CHANNELS - *count) { ok = false; break; }
        *count += buffers->mBuffers[i].mNumberChannels;
    }
    free(buffers);
    return ok;
}

static bool members_ready(TNAggregateGroup *group) {
    for (uint32_t i = 0; i < group->count; ++i) {
        AudioDeviceID device = resolve_uid(group->members[i].uid);
        UInt32 alive = 0;
        Float64 rate = 0;
        uint32_t input = 0, output = 0;
        if (!device || device != group->members[i].device || !property(device, kAudioDevicePropertyDeviceIsAlive,
            kAudioObjectPropertyScopeGlobal, sizeof(alive), &alive) || !alive ||
            !property(device, kAudioDevicePropertyNominalSampleRate,
            kAudioObjectPropertyScopeGlobal, sizeof(rate), &rate) || rate != group->rate ||
            !channel_count(device, true, &input) || !channel_count(device, false, &output) ||
            input != group->members[i].input || output != group->members[i].output) return false;
    }
    return true;
}

/* Read back the OS-owned channel order and correction policy. A requested
 * dictionary is not evidence that CoreAudio accepted a shared timebase. */
bool tn_aggregate_group_ready(TNAggregateGroup *group) {
    if (!group || !group->device || !members_ready(group)) return false;
    CFArrayRef uids = NULL;
    CFStringRef main_uid = NULL;
    bool ok = property(group->device, kAudioAggregateDevicePropertyFullSubDeviceList,
        kAudioObjectPropertyScopeGlobal, sizeof(uids), &uids) && uids &&
        CFArrayGetCount(uids) == group->count &&
        property(group->device, kAudioAggregateDevicePropertyMainSubDevice,
        kAudioObjectPropertyScopeGlobal, sizeof(main_uid), &main_uid) && main_uid &&
        CFEqual(main_uid, group->members[group->primary].uid);
    if (ok) for (uint32_t i = 0; i < group->count; ++i)
        if (!CFEqual(CFArrayGetValueAtIndex(uids, i), group->members[i].uid)) { ok = false; break; }
    if (uids) CFRelease(uids);
    if (main_uid) CFRelease(main_uid);
    if (!ok) return false;
    AudioObjectID active[TN_GROUP_MEMBERS];
    AudioObjectPropertyAddress address = {kAudioAggregateDevicePropertyActiveSubDeviceList,
        kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain};
    UInt32 size = sizeof(active);
    if (AudioObjectGetPropertyData(group->device, &address, 0, NULL, &size, active) != noErr ||
        size != group->count * sizeof(AudioObjectID)) return false;
    for (uint32_t i = 0; i < group->count; ++i) {
        bool found = false;
        for (uint32_t j = 0; j < group->count; ++j)
            if (active[j] == resolve_uid(group->members[i].uid)) found = true;
        if (!found) return false;
    }
    /* ActiveSubDeviceList contains physical AudioDevices, not the aggregate's
     * AudioSubDevice objects. Read drift from CoreAudio's current composition,
     * not a nonexistent drift property on the underlying physical device. */
    CFDictionaryRef composition = NULL;
    if (!property(group->device, kAudioAggregateDevicePropertyComposition,
        kAudioObjectPropertyScopeGlobal, sizeof(composition), &composition) || !composition) return false;
    CFArrayRef members = CFDictionaryGetValue(composition, CFSTR(kAudioAggregateDeviceSubDeviceListKey));
    ok = members && CFGetTypeID(members) == CFArrayGetTypeID() && CFArrayGetCount(members) == group->count;
    if (ok) for (uint32_t i = 0; i < group->count; ++i) {
        CFDictionaryRef member = CFArrayGetValueAtIndex(members, i);
        if (!member || CFGetTypeID(member) != CFDictionaryGetTypeID()) { ok = false; break; }
        CFStringRef uid = CFDictionaryGetValue(member, CFSTR(kAudioSubDeviceUIDKey));
        CFNumberRef drift = CFDictionaryGetValue(member, CFSTR(kAudioSubDeviceDriftCompensationKey));
        int enabled = -1;
        if (!uid || !CFEqual(uid, group->members[i].uid) || !drift ||
            CFGetTypeID(drift) != CFNumberGetTypeID() || !CFNumberGetValue(drift, kCFNumberIntType, &enabled) ||
            (enabled != 0) != (i != group->primary)) { ok = false; break; }
    }
    CFRelease(composition);
    uint32_t input = 0, output = 0, expected_input = 0, expected_output = 0;
    for (uint32_t i = 0; i < group->count; ++i) {
        expected_input += group->members[i].input; expected_output += group->members[i].output;
    }
    return ok && channel_count(group->device, true, &input) &&
        channel_count(group->device, false, &output) && input == expected_input && output == expected_output;
}

/* One acknowledged release attempt. Failure retains the entire caller-owned
 * object; Rust controls admission, retries and final lifetime. */
bool tn_aggregate_group_try_destroy(TNAggregateGroup *group) {
    if (!group) return true;
    OSStatus status = group->device ? AudioHardwareDestroyAggregateDevice(group->device) : noErr;
    if (status != noErr && status != kAudioHardwareBadObjectError && status != kAudioHardwareBadDeviceError)
        return false;
    for (uint32_t i = 0; i < group->count; ++i) if (group->members[i].uid) CFRelease(group->members[i].uid);
    free(group);
    return true;
}

bool tn_aggregate_member_info(const char *uid, TNAggregateMemberInfo *info) {
    if (!uid || !info) return false;
    memset(info, 0, sizeof(*info));
    CFStringRef name = CFStringCreateWithCString(NULL, uid, kCFStringEncodingUTF8);
    if (!name) return false;
    info->device = resolve_uid(name);
    CFRelease(name);
    return info->device &&
        property(info->device, kAudioDevicePropertyTransportType,
            kAudioObjectPropertyScopeGlobal, sizeof(info->transport), &info->transport) &&
        property(info->device, kAudioDevicePropertyNominalSampleRate,
            kAudioObjectPropertyScopeGlobal, sizeof(info->rate), &info->rate) &&
        property(info->device, kAudioDevicePropertyDeviceIsAlive,
            kAudioObjectPropertyScopeGlobal, sizeof(info->alive), &info->alive) &&
        channel_count(info->device, true, &info->input) &&
        channel_count(info->device, false, &info->output);
}

TNAggregateGroup *tn_aggregate_group_create(const char *const *uids,
    const TNAggregateMemberInfo *members_info, uint32_t count,
    uint32_t primary, int32_t *status) {
    if (!status) return NULL;
    *status = 1;
    if (!uids || !members_info || count < 2 || count > TN_GROUP_MEMBERS || primary >= count) return NULL;
    TNAggregateGroup *group = calloc(1, sizeof(*group));
    if (!group) { *status = 2; return NULL; }
    group->count = count; group->primary = primary; group->rate = members_info[primary].rate;
    for (uint32_t i = 0; i < count; ++i) {
        if (!uids[i] || !uids[i][0]) goto failure;
        group->members[i].uid = CFStringCreateWithCString(NULL, uids[i], kCFStringEncodingUTF8);
        if (!group->members[i].uid) { *status = 2; goto failure; }
        group->members[i].device = members_info[i].device;
        group->members[i].input = members_info[i].input;
        group->members[i].output = members_info[i].output;
    }
    /* The validated Rust plan can go stale while native objects are published.
     * Recheck identity/geometry before creating, then read back again before IO. */
    if (!members_ready(group)) { *status = 2; goto failure; }
    *status = 2;
    CFMutableArrayRef members = CFArrayCreateMutable(NULL, 0, &kCFTypeArrayCallBacks);
    if (!members) goto failure;
    for (uint32_t i = 0; i < count; ++i) {
        int drift = i == primary ? 0 : 1;
        int quality = kAudioAggregateDriftCompensationHighQuality;
        CFNumberRef enabled = CFNumberCreate(NULL, kCFNumberIntType, &drift);
        CFNumberRef high = CFNumberCreate(NULL, kCFNumberIntType, &quality);
        if (!enabled || !high) {
            if (enabled) CFRelease(enabled);
            if (high) CFRelease(high);
            CFRelease(members); goto failure;
        }
        const void *keys[] = {CFSTR(kAudioSubDeviceUIDKey), CFSTR(kAudioSubDeviceDriftCompensationKey),
            CFSTR(kAudioSubDeviceDriftCompensationQualityKey)};
        const void *values[] = {group->members[i].uid, enabled, high};
        CFDictionaryRef member = CFDictionaryCreate(NULL, keys, values, 3,
            &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks);
        if (!member) { CFRelease(enabled); CFRelease(high); CFRelease(members); goto failure; }
        CFArrayAppendValue(members, member);
        CFRelease(member); CFRelease(enabled); CFRelease(high);
    }
    CFUUIDRef uuid = CFUUIDCreate(NULL);
    if (!uuid) { CFRelease(members); goto failure; }
    CFStringRef unique = CFUUIDCreateString(NULL, uuid);
    if (!unique) { CFRelease(uuid); CFRelease(members); goto failure; }
    CFStringRef uid = CFStringCreateWithFormat(NULL, NULL, CFSTR("io.tonenari.private-group.%@"), unique);
    CFRelease(unique); CFRelease(uuid);
    if (!uid) { CFRelease(members); goto failure; }
    const void *keys[] = {CFSTR(kAudioAggregateDeviceUIDKey), CFSTR(kAudioAggregateDeviceNameKey),
        CFSTR(kAudioAggregateDeviceSubDeviceListKey), CFSTR(kAudioAggregateDeviceMainSubDeviceKey),
        CFSTR(kAudioAggregateDeviceIsPrivateKey), CFSTR(kAudioAggregateDeviceIsStackedKey)};
    int one = 1;
    int zero = 0;
    CFNumberRef yes = CFNumberCreate(NULL, kCFNumberIntType, &one);
    CFNumberRef no = CFNumberCreate(NULL, kCFNumberIntType, &zero);
    if (!yes || !no) {
        if (yes) CFRelease(yes);
        if (no) CFRelease(no);
        CFRelease(uid); CFRelease(members); goto failure;
    }
    const void *values[] = {uid, CFSTR("ToneNari private I/O"), members,
        group->members[primary].uid, yes, no};
    CFDictionaryRef description = CFDictionaryCreate(NULL, keys, values, 6,
        &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks);
    CFRelease(yes); CFRelease(no);
    if (!description) { CFRelease(uid); CFRelease(members); goto failure; }
    OSStatus result = AudioHardwareCreateAggregateDevice(description, &group->device);
    CFRelease(description); CFRelease(uid); CFRelease(members);
    if (result != noErr) goto failure;
    *status = 0;
failure:
    /* Even partial creation transfers ownership. A NULL-only error contract
     * would lose the object if native cleanup itself failed. */
    return group;
}

bool tn_aggregate_group_binding(TNAggregateGroup *group, const char *uid, bool input, uint32_t channels,
    uint32_t *device, uint32_t *offset, uint32_t *total) {
    if (!group || !uid || !device || !offset || !total || !tn_aggregate_group_ready(group)) return false;
    CFStringRef selected = CFStringCreateWithCString(NULL, uid, kCFStringEncodingUTF8);
    if (!selected) return false;
    bool found = false;
    *offset = 0; *total = 0;
    for (uint32_t i = 0; i < group->count; ++i) {
        if (CFEqual(selected, group->members[i].uid)) {
            *offset = *total;
            found = channels && channels <= (input ? group->members[i].input : group->members[i].output);
        }
        *total += input ? group->members[i].input : group->members[i].output;
    }
    CFRelease(selected);
    *device = group->device;
    return found;
}
