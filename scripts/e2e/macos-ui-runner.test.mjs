import assert from 'node:assert/strict';
import test from 'node:test';
import { designatedRequirement, runMacosUi } from './macos-ui-runner.mjs';

const hostedRequirement = '# designated => cdhash H"32ba2e5952d60363e6b223f989834320e067bc61"\n';

test('real hosted ad-hoc output is accepted on either codesign output stream', () => {
  for (const stream of ['stdout', 'stderr']) {
    assert.equal(designatedRequirement({ [stream]: hostedRequirement }),
      'cdhash H"32ba2e5952d60363e6b223f989834320e067bc61"');
  }
  assert.throws(() => designatedRequirement({ stderr: 'Executable=/runner\n' }), /one designated requirement/);
  assert.throws(() => designatedRequirement({ stdout: hostedRequirement, stderr: hostedRequirement }), /one designated requirement/);
});

test('Accessibility is granted to the signed generated runner that runs without another build', () => {
  const calls = [], files = new Map();
  runMacosUi('/evidence', '/tools', {
    runCommand(program, args) {
      calls.push([program, args]);
      if (program === 'plutil') return { stdout: 'io.mixrelay.qualification.ui.xctrunner\n' };
      if (program === 'codesign' && args[0] === '-d') return { stdout: hostedRequirement, stderr: 'Executable=/runner\n' };
      return { stdout: '', stderr: '' };
    },
    read: () => Buffer.from([0xfa, 0xde]),
    write: (path, data) => files.set(path, data),
  });
  assert.equal(calls[0][1][0], 'build-for-testing');
  const sign = calls.findIndex(([p, args]) => p === 'codesign' && args[0] === '--force');
  const grant = calls.findIndex(([p]) => p === 'sudo');
  assert.ok(sign > 0 && grant > sign);
  const compile = calls.findIndex(([p]) => p === 'csreq');
  assert.equal(calls[compile][1][1], '=cdhash H"32ba2e5952d60363e6b223f989834320e067bc61"');
  assert.deepEqual(calls[compile + 1], ['codesign', ['--verify', '--strict', '-R', '/tools/runner.csreq', calls[sign][1].at(-1)]]);
  assert.ok(compile + 1 < grant);
  const sql = calls[grant][1].at(-1);
  assert.match(sql, /'io\.mixrelay\.qualification\.ui\.xctrunner',0,2,4,1,X'fade'/);
  const expected = JSON.parse(files.get('/evidence/installed-ui-runner-expected.json'));
  assert.equal(expected.path, calls[sign][1].at(-1));
  assert.equal(expected.bundle, 'io.mixrelay.qualification.ui.xctrunner');
  assert.ok(expected.requirement.includes('cdhash'));
  assert.deepEqual(calls.filter(([p]) => p === 'xcodebuild').map(([, args]) => args[0]),
    ['build-for-testing', 'test-without-building']);
  assert.equal(calls.at(-1)[1].at(-1), '/evidence/installed-ui.xcresult');
  assert.ok(calls.slice(grant + 1).every(([p]) => p !== 'codesign'));
});

test('an unexpected generated identity fails before signing, granting, or running tests', () => {
  const calls = [];
  assert.throws(() => runMacosUi('/evidence', '/tools', {
    runCommand(program) {
      calls.push(program);
      return { stdout: 'io.mixrelay.qualification.ui\n' };
    },
  }), /io.mixrelay.qualification.ui/);
  assert.deepEqual(calls, ['xcodebuild', 'plutil']);
});
