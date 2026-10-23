// Test-only XCTest PID selection. Its macOS processID accessors are checked at runtime;
// unsupported Xcode versions fail the UI case without changing the installed application.
#import "InstalledApplication.h"

@interface XCUIApplication (InstalledProcess)
- (void)setProcessID:(pid_t)process;
- (pid_t)processID;
@end

XCUIApplication *MRInstalledApplication(pid_t process) {
    XCUIApplication *app = [[XCUIApplication alloc] initWithURL:
        [NSURL fileURLWithPath:@"/Applications/MixRelay/MixRelay.app"]];
    if (![app respondsToSelector:@selector(setProcessID:)] || ![app respondsToSelector:@selector(processID)]) return nil;
    [app setProcessID:process];
    return app.processID == process ? app : nil;
}
