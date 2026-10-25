/* Control-thread marshalling and persistence of the driver's bounded IO evidence. */
#include "MRProbeHalTrace.h"
#include <CoreFoundation/CoreFoundation.h>
#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static MRProbeHalTraceResult mr_trace_result(const char *stage, OSStatus status, int error) {
    return (MRProbeHalTraceResult){stage, status, error};
}

MRProbeHalTraceResult mr_probe_hal_trace_enable(AudioDeviceID device, bool enabled) {
    const AudioObjectPropertyAddress address = {'mrdi', kAudioObjectPropertyScopeGlobal, 0};
    const UInt32 value = enabled ? 1U : 0U;
    CFDataRef data = CFDataCreate(NULL, (const UInt8 *)&value, sizeof(value));
    if (data == NULL) return mr_trace_result("hal_trace_flag_allocate", noErr, ENOMEM);
    const OSStatus status = AudioObjectSetPropertyData(device, &address, 0U, NULL, sizeof(data), &data);
    CFRelease(data);
    return mr_trace_result(status == noErr ? NULL : enabled ? "hal_trace_enable" : "hal_trace_disable", status, 0);
}

static MRProbeHalTraceResult mr_trace_read(AudioDeviceID device, CFDataRef *data) {
    const AudioObjectPropertyAddress address = {'mrdo', kAudioObjectPropertyScopeGlobal, 0};
    UInt32 size = 0U;
    OSStatus status = AudioObjectGetPropertyDataSize(device, &address, 0U, NULL, &size);
    if (status != noErr) return mr_trace_result("hal_trace_size", status, 0);
    if (size != sizeof(*data)) return mr_trace_result("hal_trace_reference_size", noErr, EINVAL);
    status = AudioObjectGetPropertyData(device, &address, 0U, NULL, &size, data);
    if (status != noErr) return mr_trace_result("hal_trace_read", status, 0);
    if (size != sizeof(*data) || *data == NULL || CFGetTypeID(*data) != CFDataGetTypeID())
        return mr_trace_result("hal_trace_reference_type", noErr, EINVAL);
    return mr_trace_result(NULL, noErr, 0);
}

static bool mr_trace_write(FILE *file, const uint64_t *words) {
    fprintf(file, "{\"version\":3,\"issued\":%llu,\"dropped\":%llu,\"capacity\":8192,\"rows\":[",
            (unsigned long long)words[3], (unsigned long long)words[4]);
    bool comma = false;
    for (UInt32 row = 0U; row < 8192U; ++row) {
        const uint64_t *values = words + 5U + row * 11U;
        if (values[0] == 0U) continue;
        fprintf(file, "%s[", comma ? "," : "");
        for (UInt32 column = 0U; column < 11U; ++column) {
            if (column == 7U) fprintf(file, ",\"%016llx\"", (unsigned long long)values[column]);
            else fprintf(file, "%s%llu", column == 0U ? "" : ",", (unsigned long long)values[column]);
        }
        fprintf(file, "]");
        comma = true;
    }
    fprintf(file, "]}\n");
    return ferror(file) == 0;
}

MRProbeHalTraceResult mr_probe_hal_trace_save(AudioDeviceID device, const char *path) {
    CFDataRef data = NULL;
    MRProbeHalTraceResult result = mr_trace_read(device, &data);
    /* v3 has five UInt64 header fields and 8192 eleven-word rows. CFData's byte
     * pointer has no UInt64 alignment contract, so copy before interpreting it. */
    const size_t expected = (5U + 8192U * 11U) * sizeof(uint64_t);
    uint64_t *words = NULL;
    if (result.stage == NULL && CFDataGetLength(data) != (CFIndex)expected)
        result = mr_trace_result("hal_trace_payload_size", noErr, EINVAL);
    if (result.stage == NULL) {
        words = malloc(expected);
        if (words == NULL) result = mr_trace_result("hal_trace_payload_allocate", noErr, ENOMEM);
        else memcpy(words, CFDataGetBytePtr(data), expected);
    }
    if (data != NULL) CFRelease(data);
    if (result.stage == NULL && (words[0] != 3U || words[1] != 10U || words[2] != 8192U))
        result = mr_trace_result("hal_trace_payload_version", noErr, EINVAL);
    if (result.stage == NULL) {
        FILE *file = fopen(path, "wx");
        if (file == NULL) result = mr_trace_result("hal_trace_file_open", noErr, errno);
        else {
            if (!mr_trace_write(file, words)) result = mr_trace_result("hal_trace_file_write", noErr, errno);
            if (fclose(file) != 0 && result.stage == NULL)
                result = mr_trace_result("hal_trace_file_close", noErr, errno);
        }
    }
    free(words);
    return result;
}
