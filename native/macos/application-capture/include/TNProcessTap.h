#ifndef TN_PROCESS_TAP_H
#define TN_PROCESS_TAP_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct TNProcessTapSession TNProcessTapSession;

typedef enum TNProcessTapErrorCode {
    TN_PROCESS_TAP_OK = 0,
    TN_PROCESS_TAP_INVALID_ARGUMENT = 1,
    TN_PROCESS_TAP_UNSUPPORTED_SYSTEM = 2,
    TN_PROCESS_TAP_APPLICATION_NOT_FOUND = 3,
    TN_PROCESS_TAP_CORE_AUDIO_FAILURE = 4,
    TN_PROCESS_TAP_UNSUPPORTED_FORMAT = 5,
    TN_PROCESS_TAP_OUT_OF_MEMORY = 6,
    TN_PROCESS_TAP_ALREADY_RUNNING = 7,
    TN_PROCESS_TAP_NOT_RUNNING = 8,
    TN_PROCESS_TAP_RESOURCE_BUSY = 9
} TNProcessTapErrorCode;

typedef struct TNProcessTapError {
    TNProcessTapErrorCode code;
    int32_t os_status;
} TNProcessTapError;

typedef struct TNProcessTapConfig {
    const char *bundle_id;
    bool mute_while_captured;
    size_t ring_capacity_frames;
} TNProcessTapConfig;

typedef struct TNProcessTapStats {
    uint64_t callback_count;
    uint64_t captured_frames;
    uint64_t dropped_frames;
    size_t available_frames;
    uint32_t channels;
    double sample_rate;
    bool running;
    uint64_t format_generation;
    uint64_t clock_discontinuities;
    double sample_time;
    uint64_t host_time;
    bool clock_valid;
} TNProcessTapStats;

typedef void (*TNProcessTapApplicationVisitor)(
    void *context,
    const char *bundle_id,
    const char *display_name,
    const char *icon_data_url,
    int32_t process_id
);

TNProcessTapError tn_process_tap_visit_applications(
    TNProcessTapApplicationVisitor visitor,
    void *context
);

TNProcessTapSession *tn_process_tap_create(
    const TNProcessTapConfig *config,
    TNProcessTapError *error
);
TNProcessTapError tn_process_tap_start(TNProcessTapSession *session);
TNProcessTapError tn_process_tap_stop(TNProcessTapSession *session);
/* Control-owner only, serialized with read/start/stop. Stats/peaks may run concurrently.
 * A failed refresh suspends delivery; incompatible geometry requires recreation. */
TNProcessTapError tn_process_tap_refresh_format(TNProcessTapSession *session);
/* Consumes ownership without waiting for Core Audio. A bounded native owner retries
 * failed unregister/removal; callback storage is never freed on an error. */
void tn_process_tap_destroy(TNProcessTapSession *session);

size_t tn_process_tap_read(
    TNProcessTapSession *session,
    float *destination,
    size_t maximum_frames
);
size_t tn_process_tap_take_peaks(
    TNProcessTapSession *session,
    float *destination,
    size_t maximum_channels
);
TNProcessTapStats tn_process_tap_stats(const TNProcessTapSession *session);

#endif
