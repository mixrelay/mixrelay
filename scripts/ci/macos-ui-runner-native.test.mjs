import assert from 'node:assert/strict';
import test from 'node:test';
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { runMacosUi } from '../e2e/macos-ui-runner.mjs';

test('native ad-hoc cdhash requirement compiles to a blob that validates only the signed code',
  () => {
    assert.equal(process.platform, 'darwin', 'Run the native signing boundary on macOS');
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

