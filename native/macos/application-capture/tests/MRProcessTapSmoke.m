#import "MRProcessTap.h"

#import <Foundation/Foundation.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

int main(int argument_count, const char *arguments[]) {
    @autoreleasepool {
        if (argument_count != 2) {
            fprintf(stderr, "usage: MRProcessTapSmoke <bundle-id>\n");
            return EXIT_FAILURE;
        }
        MRProcessTapConfig config = {
            .bundle_id = arguments[1],
            .mute_while_captured = true,
            .ring_capacity_frames = 16384,
        };
        MRProcessTapError error = {0};
        MRProcessTapSession *session = mr_process_tap_create(&config, &error);
        if (session == NULL) {
            fprintf(stderr, "create failed: code=%d os_status=%d\n", error.code, error.os_status);
            return EXIT_FAILURE;
        }
        error = mr_process_tap_start(session);
        if (error.code != MR_PROCESS_TAP_OK) {
            fprintf(stderr, "start failed: code=%d os_status=%d\n", error.code, error.os_status);
            mr_process_tap_destroy(session);
            return EXIT_FAILURE;
        }
        float samples[4096] = {0};
        uint64_t read_frames = 0;
        float peak = 0;
        for (size_t iteration = 0; iteration < 200; ++iteration) {
            usleep(10000);
            const size_t frames = mr_process_tap_read(session, samples, 2048);
            read_frames += frames;
            for (size_t sample = 0; sample < frames * 2; ++sample) {
                const float magnitude = samples[sample] < 0 ? -samples[sample] : samples[sample];
                peak = magnitude > peak ? magnitude : peak;
            }
        }
        MRProcessTapStats stats = mr_process_tap_stats(session);
        printf(
            "running=%d callbacks=%" PRIu64 " captured=%" PRIu64 " dropped=%" PRIu64
            " available=%zu read=%" PRIu64 " peak=%.6f "
            "channels=%u sample_rate=%.0f\n",
            stats.running,
            stats.callback_count,
            stats.captured_frames,
            stats.dropped_frames,
            stats.available_frames,
            read_frames,
            peak,
            stats.channels,
            stats.sample_rate
        );
        mr_process_tap_destroy(session);
    }
    return EXIT_SUCCESS;
}
