import assert from 'node:assert/strict';
import test from 'node:test';
import { once } from 'node:events';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { installedPaths, requireDisposableHost, spawnLogged } from './package.mjs';

test('installation refuses an unacknowledged host before any installer or privilege command runs', () => {
  assert.throws(() => requireDisposableHost('macos', false), /disposable-environment/);
  assert.throws(() => requireDisposableHost('linux', false), /disposable-environment/);
  assert.ok(installedPaths('macos').every(path => path.startsWith('/')));
  assert.ok(installedPaths('linux').every(path => path.startsWith('/usr/bin/')));
});

test('a background controller inheriting stderr cannot keep the completed desktop test open', async t => {
  const directory = mkdtempSync(join(tmpdir(), 'mixrelay-desktop-lifetime-'));
  const pidFile = join(directory, 'runtime.pid'), logFile = join(directory, 'desktop.log');
  t.after(() => {
    try { process.kill(Number(readFileSync(pidFile, 'utf8')), 'SIGTERM'); }
    catch (error) { if (!['ENOENT', 'ESRCH'].includes(error.code)) throw error; }
    rmSync(directory, { recursive: true, force: true });
  });
  const child = spawnLogged(process.execPath, ['-e', `
    const {spawn}=require('node:child_process'), {writeFileSync}=require('node:fs');
    const runtime=spawn(process.execPath,['-e','setTimeout(()=>{},5000)'],{stdio:['ignore','ignore','inherit']});
    writeFileSync(process.argv[1],String(runtime.pid));
    runtime.unref(); process.stderr.write('desktop completed\\n');
  `, pidFile], logFile);
  const [code] = await once(child, 'close', { signal: AbortSignal.timeout(1500) });
  assert.equal(code, 0);
  assert.match(readFileSync(logFile, 'utf8'), /desktop completed/);
});
