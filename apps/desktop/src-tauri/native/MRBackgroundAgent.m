#import <Foundation/Foundation.h>
#import <ServiceManagement/ServiceManagement.h>
#include <stdio.h>

// Retire the local pre-release LaunchAgent experiment before a cold launch.
// The caller already owns the serialized Connect admission and has checked
// that neither the Driver nor engine lease is held. Never register a new job.
int MRRetireBackgroundAgent(char *message, size_t capacity) {
    @autoreleasepool {
        if (![[NSBundle.mainBundle bundleIdentifier] isEqualToString:@"io.mixrelay.desktop"]) {
            snprintf(message, capacity, "%s", "Background service requires the installed MixRelay app");
            return 1;
        }
        SMAppService *service = [SMAppService agentServiceWithPlistName:@"io.mixrelay.driver.runtime.plist"];
        NSError *error = nil;
        if ([service unregisterAndReturnError:&error]) return 0;
        // An already absent registration is confirmed by the service state;
        // preserve all other errors rather than guessing legacy error domains.
        if (service.status == SMAppServiceStatusNotRegistered) return 0;
        snprintf(message, capacity, "Previous background service cleanup failed (%s:%ld): %s",
                 error.domain.UTF8String ?: "unknown", (long)error.code,
                 error.localizedDescription.UTF8String ?: "Unknown error");
        return 1;
    }
}
