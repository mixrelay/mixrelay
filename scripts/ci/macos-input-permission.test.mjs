import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

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
