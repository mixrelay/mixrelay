import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

function run(program, args) {
  const result = spawnSync(program, args, { encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 });
  process.stdout.write(result.stdout ?? '');
  process.stderr.write(result.stderr ?? '');
  if (result.error || result.status !== 0) throw result.error ?? new Error(`${program} exited ${result.status}`);
  return result;
}

export function designatedRequirement({ stdout = '', stderr = '' }) {
  const matches = [...`${stdout}\n${stderr}`.matchAll(/^(?:#\s*)?designated => (.+)\r?$/gm)];
  assert.equal(matches.length, 1, 'Signed XCTest runner must have one designated requirement');
  return matches[0][1].trim();
}

export function runMacosUi(output, tools, { runCommand = run, read = readFileSync, write = writeFileSync } = {}) {
  const build = ['-project', join(tools, 'MixRelayInstalledUI.xcodeproj'), '-scheme', 'InstalledUI',
    '-destination', 'platform=macOS', '-derivedDataPath', join(tools, 'DerivedData'),
    '-parallel-testing-enabled', 'NO', 'CODE_SIGNING_ALLOWED=NO'];
  runCommand('xcodebuild', ['build-for-testing', ...build]);
  const runner = join(tools, 'DerivedData/Build/Products/Debug/InstalledUITests-Runner.app');
  const bundle = runCommand('plutil', ['-extract', 'CFBundleIdentifier', 'raw', '-o', '-', join(runner, 'Contents/Info.plist')]).stdout.trim();
  assert.equal(bundle, 'io.mixrelay.qualification.ui.xctrunner');
  runCommand('codesign', ['--force', '--deep', '--sign', '-', runner]);
  runCommand('codesign', ['--verify', '--deep', '--strict', runner]);
  const requirement = designatedRequirement(runCommand('codesign', ['-d', '-r-', runner]));
  const csreq = join(tools, 'runner.csreq');
  runCommand('csreq', ['-r', `=${requirement}`, '-b', csreq]);
  runCommand('codesign', ['--verify', '--strict', '-R', csreq, runner]);
  const bytes = read(csreq).toString('hex');
  assert.match(bytes, /^(?:[0-9a-f]{2})+$/);
  // This grant is confined to the disposable hosted session and to the exact
  // generated runner's code requirement. test-without-building preserves it.
  const sql = `INSERT OR REPLACE INTO access
    (service,client,client_type,auth_value,auth_reason,auth_version,csreq,
     indirect_object_identifier_type,indirect_object_identifier,flags,last_modified)
    VALUES ('kTCCServiceAccessibility','${bundle}',0,2,4,1,X'${bytes}',0,'UNUSED',0,strftime('%s','now'));`;
  runCommand('sudo', ['-n', 'sqlite3', '/Library/Application Support/com.apple.TCC/TCC.db', sql]);
  runCommand('sudo', ['-n', 'killall', '-u', 'root', 'tccd']);
  write(join(output, 'installed-ui-runner-expected.json'), `${JSON.stringify({ bundle, path: runner, requirement }, null, 2)}\n`);
  runCommand('xcodebuild', ['test-without-building', ...build, '-resultBundlePath', join(output, 'installed-ui.xcresult')]);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  assert.equal(process.platform, 'darwin');
  assert.equal(process.env.GITHUB_ACTIONS, 'true', 'TCC setup requires a disposable hosted runner');
  runMacosUi(resolve(process.argv[2]), resolve(process.argv[3]));
}
