/* Test-only PID attachment: the editor and background runtime share a bundle ID.
 * The XCTest process tracker is also used by Appium's FBXCAXClientProxy:
 * https://github.com/appium/WebDriverAgent/blob/master/WebDriverAgentLib/Utilities/FBXCAXClientProxy.m
 * Unsupported Xcode versions fail the UI case; installed code is never modified. */
#import "InstalledApplication.h"
#import <objc/message.h>

static id MRObjectProperty(id object, NSString *name) {
    SEL selector = NSSelectorFromString(name);
    if (object == nil || ![object respondsToSelector:selector]) return nil;
    return ((id (*)(id, SEL))objc_msgSend)(object, selector);
}

XCUIApplication *MRInstalledApplication(pid_t process) {
    id client = MRObjectProperty(XCUIDevice.sharedDevice, @"accessibilityInterface");
    id tracker = MRObjectProperty(client, @"applicationProcessTracker");
    SEL selector = NSSelectorFromString(@"monitoredApplicationWithProcessIdentifier:");
    if (tracker == nil || ![tracker respondsToSelector:selector]) return nil;
    id app = ((id (*)(id, SEL, int))objc_msgSend)(tracker, selector, process);
    return [app isKindOfClass:XCUIApplication.class] ? app : nil;
}
