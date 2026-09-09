import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const read = (path) => readFileSync(new URL(path, import.meta.url), 'utf8');
const lock = JSON.parse(read('../../infra/lab/sysvad-baseline.json'));
const build = read('./guest-windows-sysvad-build.cmd');
const prepare = read('./guest-windows-sysvad-prepare.cmd');

test('SysVAD baseline pins upstream provenance and build input hashes', () => {
  assert.match(lock.revision, /^[a-f0-9]{40}$/);
  assert.match(lock.wilRevision, /^[a-f0-9]{40}$/);
  assert.equal(lock.license, 'MS-PL');
  assert.ok(build.includes(lock.sourceArchiveSha256.toUpperCase()));
  for (const [name, hash] of Object.entries(lock.packages)) {
    assert.match(hash, /^[a-f0-9]{64}$/);
    if (name.includes('SDK')) assert.ok(prepare.includes(hash.toUpperCase()));
  }
});

test('SysVAD build uses upstream MSBuild projects and preserves failed-job evidence', () => {
  assert.ok(build.includes('if exist "%job%" exit /b 2'));
  assert.ok(build.includes('set result=%errorlevel%'));
  assert.ok(build.includes('echo %result%'));
  assert.match(build, /msbuild\.exe EndpointsCommon\\EndpointsCommon\.vcxproj/);
  assert.match(build, /msbuild\.exe TabletAudioSample\\TabletAudioSample\.vcxproj/);
  assert.match(build, /source\\Directory\.Build\.props/);
  assert.doesNotMatch(build, /cl\.exe|link\.exe|devcon\.exe|pnputil\.exe|bcdedit|signtool/i);
  assert.doesNotMatch(build + prepare, /ExecutionPolicy|Set-MpPreference|StrongName\\Verification/i);
});

test('sample verification uses native parent identity, not a localized display name', () => {
  const verify = read('./guest-windows-sysvad-verify.cmd');
  assert.ok(verify.includes('DEVPKEY_Device_Parent'));
  assert.ok(verify.includes('$devices[0].PNPDeviceID'));
  assert.doesNotMatch(verify, /Where-Object FriendlyName/);
  assert.ok(verify.includes('$render.Count -eq 0 -or $capture.Count -eq 0'));
});

test('baseline installation is scoped and does not claim product isolation qualification', () => {
  const install = read('./guest-windows-sysvad-install.cmd');
  const packaging = read('./guest-windows-sysvad-package.cmd');
  assert.ok(install.includes("'Root\\sysvad_ComponentizedAudioSample'"));
  assert.ok(install.includes('refusing duplicate installation'));
  assert.doesNotMatch(install, /devcon\.exe (remove|update)|bcdedit|shutdown/i);
  assert.ok(packaging.includes('unmet PRODUCT gate'));
  assert.ok(packaging.includes('infverif.exe /u /v'));
  assert.ok(read('./guest-windows-sysvad-audio.cmd').includes('DEVPKEY_Device_Parent'));
  assert.ok(read('./SysvadAudioProbe.cpp').includes('NOT ToneNari routed audio'));
});

test('interactive smoke cleanup resolves account identity and preserves running or foreign tasks', () => {
  const cleanup = read('./cleanup-windows-sysvad-audio-task.cmd');
  assert.ok(cleanup.includes('System.Security.Principal.SecurityIdentifier'));
  assert.ok(cleanup.includes('$owner -ne $expected'));
  assert.ok(cleanup.includes("$t.State -eq 'Running'"));
  assert.ok(cleanup.includes('$t.Actions.Count -ne 1'));
  assert.ok(cleanup.includes("$t.Actions[0].Arguments -ne '/d /c C:\\Users\\state303\\sysvad-audio.cmd'"));
  assert.ok(cleanup.includes('sysvad-audio-v1.exit-code'));
  assert.doesNotMatch(cleanup, /Remove-Item|pnputil|devcon|bcdedit|shutdown/i);
});
