import assert from 'node:assert/strict';
import test from 'node:test';
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
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

test('native ad-hoc cdhash requirement compiles to a blob that validates only the signed code',
  { skip: process.platform !== 'darwin' }, () => {
    const directory = mkdtempSync(join(tmpdir(), 'mr-ui-csreq-'));
    const runner = join(directory, 'DerivedData/Build/Products/Debug/InstalledUITests-Runner.app');
    const executable = join(runner, 'Contents/MacOS/runner');
    const blob = join(directory, 'runner.csreq');
    const command = (program, args) => {
      const result = spawnSync(program, args, { encoding: 'utf8' });
      assert.equal(result.error, undefined);
      assert.equal(result.status, 0, result.stderr);
      return result;
    };
    try {
      mkdirSync(join(runner, 'Contents/MacOS'), { recursive: true });
      copyFileSync('/usr/bin/true', executable);
      writeFileSync(join(runner, 'Contents/Info.plist'), `<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>CFBundleIdentifier</key><string>io.mixrelay.qualification.ui.xctrunner</string>
<key>CFBundleExecutable</key><string>runner</string><key>CFBundlePackageType</key><string>APPL</string></dict></plist>`);
      let sql;
      // Run the production signing/requirement path on a local app fixture.
      // Xcode and TCC remain disposable-session boundaries; no host grant is written.
      runMacosUi(directory, directory, {
        runCommand(program, args) {
          if (program === 'xcodebuild') return { stdout: '', stderr: '' };
          if (program === 'sudo') {
            if (args[1] === 'sqlite3') sql = args.at(-1);
            return { stdout: '', stderr: '' };
          }
          return command(program, args);
        },
      });
      const { requirement } = JSON.parse(readFileSync(join(directory, 'installed-ui-runner-expected.json')));
      assert.match(requirement, /^cdhash H"[0-9a-f]+"(?: or cdhash H"[0-9a-f]+")*$/);
      const bytes = readFileSync(blob);
      assert.ok(bytes.length > 0);
      assert.ok(sql.includes(`'io.mixrelay.qualification.ui.xctrunner',0,2,4,1,X'${bytes.toString('hex')}'`));
      command('codesign', ['--verify', '--strict', '-R', blob, runner]);
      // Replacing the code must invalidate the exact blob used for the grant.
      copyFileSync('/usr/bin/false', executable);
      command('codesign', ['--force', '--sign', '-', runner]);
      const rejected = spawnSync('codesign', ['--verify', '--strict', '-R', blob, runner], { encoding: 'utf8' });
      assert.notEqual(rejected.status, 0);
      assert.match(rejected.stderr, /failed to satisfy specified code requirement/);
    } finally {
      rmSync(directory, { recursive: true, force: true });
    }
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
