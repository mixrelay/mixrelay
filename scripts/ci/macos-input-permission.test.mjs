import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

test('finite measurement retains a latency activity only while I/O runs and releases it on failure', t => {
  assert.equal(process.platform, 'darwin');
  const directory = mkdtempSync(join(tmpdir(), 'mr-probe-activity-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const fixture = join(directory, 'activity.m');
  writeFileSync(fixture, `
#import <Foundation/Foundation.h>
#import <objc/runtime.h>
#include <assert.h>
#include <stdbool.h>
extern bool mr_probe_with_activity(bool (*measure)(void *), void *context);
static id token;
static bool active, refuse;
static unsigned begins, ends, measurements;
static id begin(id receiver, SEL selector, NSActivityOptions options, NSString *reason) {
    assert(!active);
    assert((options & NSActivityLatencyCritical) == NSActivityLatencyCritical);
    assert((options & NSActivityUserInitiatedAllowingIdleSystemSleep) == NSActivityUserInitiatedAllowingIdleSystemSleep);
    assert(reason.length > 0);
    begins++;
    active = !refuse;
    return active ? token : nil;
}
static void end(id receiver, SEL selector, id activity) {
    assert(active && activity == token);
    active = false; ends++;
}
static bool measure(void *context) { assert(active); measurements++; return *(bool *)context; }
int main(void) {
    @autoreleasepool {
        token = [NSObject new];
        Class processClass = object_getClass([NSProcessInfo processInfo]);
        method_setImplementation(class_getInstanceMethod(processClass, @selector(beginActivityWithOptions:reason:)), (IMP)begin);
        method_setImplementation(class_getInstanceMethod(processClass, @selector(endActivity:)), (IMP)end);
        bool result = true;
        assert(mr_probe_with_activity(measure, &result));
        assert(!active && begins == 1 && ends == 1 && measurements == 1);
        result = false;
        assert(!mr_probe_with_activity(measure, &result));
        assert(!active && begins == 2 && ends == 2 && measurements == 2);
        refuse = true;
        assert(!mr_probe_with_activity(measure, &result));
        assert(!active && begins == 3 && ends == 2 && measurements == 2);
        [token release];
    }
}
`);
  const source = fileURLToPath(new URL('../../native/macos/hal-driver/tests/MRProbeActivity.m', import.meta.url));
  const executable = join(directory, 'activity');
  execFileSync('xcrun', ['clang', '-mmacosx-version-min=14.2', source, fixture,
    '-framework', 'Foundation', '-o', executable]);
  execFileSync(executable, [], { timeout: 10000 });
});

test('daemon consent does not query AVFoundation while GUI denial remains authoritative', (t) => {
  assert.equal(process.platform, 'darwin', 'Run the native permission boundary on macOS');
  const directory = mkdtempSync(join(tmpdir(), 'mr-input-consent-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const fixture = join(directory, 'permission.m');
  writeFileSync(fixture, `
#import <AVFoundation/AVFoundation.h>
#import <Security/AuthSession.h>
#import <objc/runtime.h>
#include <assert.h>
extern long mr_microphone_authorization_status(void);
static SessionAttributeBits session_attributes;
static long status = AVAuthorizationStatusDenied;
static unsigned queries;
OSStatus mr_test_session_get_info(SecuritySessionId session, SecuritySessionId *identifier, SessionAttributeBits *attributes) {
    assert(session == callerSecuritySession);
    *attributes = session_attributes;
    return errSecSuccess;
}
static long fake_status(id receiver, SEL selector, id media) {
    queries++;
    return status;
}
int main(void) {
    @autoreleasepool {
        Method method = class_getClassMethod([AVCaptureDevice class], @selector(authorizationStatusForMediaType:));
        assert(method);
        method_setImplementation(method, (IMP)fake_status);
        session_attributes = sessionIsRoot;
        assert(mr_microphone_authorization_status() == 4);
        assert(queries == 0);
        session_attributes = sessionHasGraphicAccess;
        assert(mr_microphone_authorization_status() == AVAuthorizationStatusDenied);
        assert(queries == 1);
        status = AVAuthorizationStatusAuthorized;
        assert(mr_microphone_authorization_status() == AVAuthorizationStatusAuthorized);
        assert(queries == 2);
    }
}
`);
  const executable = join(directory, 'permission');
  const source = fileURLToPath(new URL('../../native/macos/engine-service/src/MRMicrophonePermission.m', import.meta.url));
  execFileSync('xcrun', ['clang', '-fobjc-arc', '-fblocks', '-mmacosx-version-min=14.2',
    '-DSessionGetInfo=mr_test_session_get_info', source, fixture,
    '-framework', 'AVFoundation', '-framework', 'Security', '-framework', 'Foundation', '-o', executable]);
  execFileSync(executable, [], { timeout: 10000 });
});

test('measurement consent completes before device preparation and denial or timeout never starts a probe', t => {
  assert.equal(process.platform, 'darwin');
  const directory = mkdtempSync(join(tmpdir(), 'mr-probe-consent-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const source = fileURLToPath(new URL('../../native/macos/hal-driver/tests/MRRoundTripProbe.c', import.meta.url));
  const fixture = join(directory, 'probe-consent.c');
  writeFileSync(fixture, `
#define main mr_probe_measurement_main
#include ${JSON.stringify(source)}
#undef main
#include <assert.h>
static long current, decision;
static unsigned queries, requests, measurements;
long mr_microphone_authorization_status(void) { queries++; return current; }
void mr_microphone_request_authorization(void) { requests++; if (decision >= 0) current = decision; }
bool mr_probe_with_activity(bool (*measure)(void *), void *context) { measurements++; return measure(context); }
static bool authorize(long initial, long answer, MRInputAuthorization *result) {
    current = initial; decision = answer; queries = requests = 0;
    return mr_authorize_input(0, result);
}
int main(void) {
    MRInputAuthorization result;
    assert(authorize(3, -1, &result));
    assert(queries == 1 && requests == 0 && !result.requested);
    for (long refused = 1; refused <= 2; refused++) {
        assert(!authorize(refused, -1, &result));
        assert(requests == 0 && result.final == refused);
    }
    assert(authorize(4, -1, &result));
    assert(requests == 0 && result.final == 4);
    assert(authorize(0, 3, &result));
    assert(queries == 2 && requests == 1 && result.requested && result.initial == 0 && result.final == 3);
    assert(!authorize(0, 2, &result));
    assert(requests == 1 && result.final == 2);
    assert(!authorize(0, -1, &result));
    assert(requests == 1 && result.final == 0);
    current = 2; requests = queries = 0;
    char *arguments[] = {"probe", "io.mixrelay.test-device-must-not-be-opened"};
    assert(mr_probe_measurement_main(2, arguments) == 2);
    assert(queries == 1 && requests == 0);
    assert(measurements == 0);
    char *consent[] = {"probe", "--authorize-input", "--json"};
    current = 3; requests = queries = 0;
    assert(mr_probe_measurement_main(3, consent) == 0);
    assert(queries == 1 && requests == 0 && measurements == 0);
    current = 0; decision = 3; requests = queries = 0;
    assert(mr_probe_measurement_main(3, consent) == 0);
    assert(queries == 2 && requests == 1 && measurements == 0);
    current = 2; requests = queries = 0;
    assert(mr_probe_measurement_main(3, consent) == 2);
    assert(queries == 1 && requests == 0 && measurements == 0);
    MROptions options = {0};
    char *help[] = {"probe", "--help"};
    assert(mr_options(2, help, &options) == 0);
    assert(queries == 1 && requests == 0);
    assert(measurements == 0);
}
`);
  const executable = join(directory, 'probe-consent');
  execFileSync('xcrun', ['clang', '-std=c17', '-O2', '-mmacosx-version-min=14.2',
    '-Wall', '-Wextra', '-Wpedantic', '-Wconversion', '-Wsign-conversion', '-Wshadow', '-Werror',
    fixture, fileURLToPath(new URL('../../native/macos/hal-driver/tests/MRProbeHalTrace.c', import.meta.url)),
    fileURLToPath(new URL('../../native/macos/hal-driver/tests/MRProbeAuthorization.c', import.meta.url)),
    '-framework', 'CoreAudio', '-framework', 'CoreFoundation', '-o', executable]);
  execFileSync(executable, [], { timeout: 10000 });
});
