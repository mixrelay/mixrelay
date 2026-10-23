import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { digest } from '../support-contract.mjs';

const run = (program, args, timeout = 120000) => execFileSync(program, args, { encoding: 'utf8', timeout, maxBuffer: 8 * 1024 * 1024 });
export const installedPaths = os => os === 'macos' ? [
  '/Applications/MixRelay/MixRelay.app/Contents/MacOS/mixrelay-desktop',
  '/Library/Application Support/MixRelay/bin/mixrelay-engine-service',
  '/Library/Application Support/MixRelay/bin/mixrelay-driver-service',
  '/Library/Audio/Plug-Ins/HAL/MixRelayHAL.driver/Contents/MacOS/MixRelayHAL',
] : ['/usr/bin/mixrelay-desktop', '/usr/bin/mixrelay-engine-service', '/usr/bin/mixrelay-driver-service'];

export function requireDisposableHost(os, acknowledged) {
  assert.equal(acknowledged, true, 'Installation requires --disposable-environment on a resettable QA machine');
  assert.notEqual(process.getuid(), 0, 'Run as the desktop session user, with noninteractive sudo for installation');
  assert.ok(!process.env.MIXRELAY_IPC_DIR, 'Qualification uses the normal installed sockets');
  assert.ok(installedPaths(os).every(path => !existsSync(path)), 'Reset the QA machine: MixRelay is already installed');
  run('sudo', ['-n', 'true']); // Fail before Installer can open an authentication prompt.
  if (os === 'linux') {
    assert.ok(process.env.DISPLAY || process.env.WAYLAND_DISPLAY, 'A real desktop user session is required');
    run('systemctl', ['--user', 'is-active', 'pipewire.service', 'wireplumber.service']);
  }
}

export function installAndVerify(artifact, definition, directory, output, os) {
  const archive = join(directory, artifact.file);
  const extracted = join(output, 'payload');
  const install = (program, args, timeout) => {
    try { writeFileSync(join(output, 'package-install.log'), run(program, args, timeout), { flag: 'wx' }); }
    catch (error) {
      if (!existsSync(join(output, 'package-install.log'))) writeFileSync(join(output, 'package-install.log'),
        `${error.stdout ?? ''}\n${error.stderr ?? ''}\n${error.message}`, { flag: 'wx' });
      throw error;
    }
  };
  if (os === 'macos') {
    run('pkgutil', ['--check-signature', archive]);
    run('spctl', ['--assess', '--type', 'install', archive]);
    run('xcrun', ['stapler', 'validate', archive]);
    run('pkgutil', ['--expand-full', archive, extracted]);
    install('sudo', ['-n', 'installer', '-pkg', archive, '-target', '/'], 300000);
  } else {
    mkdirSync(extracted);
    if (definition.format === 'deb') {
      run('dpkg-deb', ['--extract', archive, extracted]);
      install('sudo', ['-n', 'env', 'DEBIAN_FRONTEND=noninteractive', 'apt-get', 'install', '-y', archive], 1200000);
    } else {
      run('bsdtar', ['-xf', archive, '-C', extracted]);
      install('sudo', ['-n', 'pacman', '-U', '--noconfirm', archive], 1200000);
    }
  }
  const entries = readdirSync(extracted, { recursive: true });
  const binaries = installedPaths(os).map(path => {
    const sources = os === 'macos'
      ? entries.filter(entry => entry.endsWith(path) && lstatSync(join(extracted, entry)).isFile())
      : [path.slice(1)];
    assert.equal(sources.length, 1, `Ambiguous or absent package payload: ${path}`);
    assert.ok(lstatSync(path).isFile(), `Installed executable is not a regular file: ${path}`);
    const expected = digest(readFileSync(join(extracted, sources[0])));
    assert.equal(digest(readFileSync(path)), expected, `Installed executable differs from candidate: ${path}`);
    return { path, sha256: expected };
  });
  writeFileSync(join(output, 'installed-binaries.json'), JSON.stringify({ artifact, binaries }, null, 2) + '\n', { flag: 'wx' });
}

export async function launchInstalled(os, output) {
  if (os === 'macos') {
    run('open', ['-a', '/Applications/MixRelay/MixRelay.app']);
    return async () => {
      // This QA session was empty before installation; target only the installed app path.
      let pids;
      try { pids = run('pgrep', ['-u', String(process.getuid()), '-f', '^/Applications/MixRelay/MixRelay.app/Contents/MacOS/mixrelay-desktop( |$)']).trim(); }
      catch (error) { if (error.status === 1) return; throw error; }
      for (const pid of pids.split('\n')) process.kill(Number(pid), 'SIGTERM');
      for (let attempt = 0; attempt < 50; attempt++) {
        if (pids.split('\n').every(pid => { try { process.kill(Number(pid), 0); return false; } catch (error) { if (error.code === 'ESRCH') return true; throw error; } })) return;
        await delay(100);
      }
      throw new Error('Installed app did not stop');
    };
  }
  run('systemctl', ['--user', 'start', 'mixrelay-driver.service']);
  const child = spawn('/usr/bin/mixrelay-desktop', [], { stdio: ['ignore', 'pipe', 'pipe'] });
  let log = '', launchError;
  child.on('error', error => { launchError = error; });
  for (const stream of [child.stdout, child.stderr]) stream.on('data', bytes => { if (log.length < 1024 * 1024) log += bytes.toString(); });
  return async () => {
    try {
      if (child.exitCode === null && child.signalCode === null) child.kill('SIGTERM');
      await Promise.race([new Promise(resolve => { if (child.exitCode !== null || child.signalCode !== null || launchError) resolve(); else child.once('exit', resolve); }), delay(5000)]);
      assert.ok(launchError || child.exitCode !== null || child.signalCode !== null, 'Installed app did not stop');
      if (launchError) throw launchError;
    } finally {
      writeFileSync(join(output, 'desktop.log'), log, { flag: 'wx' });
      run('systemctl', ['--user', 'stop', 'mixrelay-driver.service']);
    }
  };
}
