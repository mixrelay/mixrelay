import assert from 'node:assert/strict';
import test from 'node:test';
import { runMacosUi } from './macos-ui-runner.mjs';

test('Accessibility is granted to the signed generated runner that runs without another build', () => {
  const calls = [], files = new Map();
  runMacosUi('/evidence', '/tools', {
    runCommand(program, args) {
      calls.push([program, args]);
      if (program === 'plutil') return { stdout: 'io.mixrelay.qualification.ui.xctrunner\n' };
      if (program === 'codesign' && args[0] === '-d') return { stderr: 'designated => identifier "io.mixrelay.qualification.ui.xctrunner" and cdhash H"abcd"\n' };
      return { stdout: '', stderr: '' };
    },
    read: () => Buffer.from([0xfa, 0xde]),
    write: (path, data) => files.set(path, data),
  });
  assert.equal(calls[0][1][0], 'build-for-testing');
  const sign = calls.findIndex(([p, args]) => p === 'codesign' && args[0] === '--force');
  const grant = calls.findIndex(([p]) => p === 'sudo');
  assert.ok(sign > 0 && grant > sign);
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
