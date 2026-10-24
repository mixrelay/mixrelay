import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { appendFileSync, closeSync, copyFileSync, existsSync, mkdirSync, openSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';

export function collectCoreaudioServers(capture, path, systemLog = '') {
  const servers = capture('coreaudio-server-processes.txt', 'pgrep', ['-fl', 'coreaudiod|Core-Audio-Driver-Service']);
  const pids = new Set((servers.stdout ?? '').split('\n')
    .map(line => line.match(/^(\d+)\s/)?.[1]).filter(Boolean));
  // The remote service executable name differs between macOS versions. The
  // registrar identifies the actual MixRelay host even when pgrep misses it.
  const registered = [...systemLog.matchAll(/Creating remote driver service:[^\n]*MixRelayHAL\.driver[^\n]*\bpid:\s*(\d+)/g)]
    .map(match => match[1]);
  for (const pid of new Set(registered)) {
    const process = capture(`coreaudio-remote-${pid}-process.txt`, 'ps', ['-p', pid, '-o', 'pid=,comm=']);
    // Do not sample a stale registrar PID that has already been reused.
    if (/^\s*\d+\s+.*(?:audio|Audio).*$/m.test(process.stdout ?? '')) pids.add(pid);
  }
  for (const pid of pids) {
    capture(`coreaudio-server-${pid}-sample-command.log`, 'sudo',
      ['-n', 'sample', pid, '1', '-file', path(`coreaudio-server-${pid}.sample.txt`)]);
  }
}

export function collectCoreaudioLog(outputPath, run = spawnSync) {
  const descriptor = openSync(outputPath, 'w');
  let result;
  try {
    // Hosted upgrade logs exceed spawnSync's former 16 MiB pipe limit. Write
    // directly to the evidence file so later permission/publication events survive.
    result = run('log', ['show', '--last', '5m', '--style', 'compact', '--predicate',
      'process CONTAINS[c] "mixrelay" OR process == "coreaudiod" OR process CONTAINS[c] "audio" OR (process == "tccd" AND eventMessage CONTAINS[c] "mixrelay")'],
    { stdio: ['ignore', descriptor, descriptor], timeout: 30000 });
  } finally { closeSync(descriptor); }
  appendFileSync(outputPath, `\nexit=${result.status}; signal=${result.signal ?? ''}; error=${result.error?.message ?? ''}\n`);
  return readFileSync(outputPath, 'utf8');
}

export function collectMacos(destination, prefix = '') {
assert.equal(process.platform, 'darwin');
assert.equal(process.env.GITHUB_ACTIONS, 'true');
const output = resolve(destination); mkdirSync(output, { recursive: true });
const path = name => join(output, `${prefix}${name}`);
function capture(file, program, args, timeout = 30000) {
  const result = spawnSync(program, args, { encoding: 'utf8', timeout, maxBuffer: 16 * 1024 * 1024 });
  writeFileSync(path(file), `${result.stdout ?? ''}\n${result.stderr ?? ''}\nexit=${result.status}; signal=${result.signal ?? ''}; error=${result.error?.message ?? ''}\n`);
  return result;
}
const logs = join(homedir(), 'Library/Logs/MixRelay');
if (existsSync(logs)) for (const file of readdirSync(logs).filter(name => /^[A-Za-z0-9_.-]+\.log$/.test(name))) {
  copyFileSync(join(logs, file), path(`app-log-${file}`));
}
const systemLog = collectCoreaudioLog(path('coreaudio-system.log'));
collectCoreaudioServers(capture, path, systemLog);
capture('coreaudio-devices.txt', 'system_profiler', ['SPAudioDataType', '-json']);
capture('coreaudio-probe.txt', 'zsh', ['scripts/probe-macos-roundtrip.sh', '--list']);
for (const [role, path] of [
  ['desktop', '/Applications/MixRelay/MixRelay.app'],
  ['engine', '/Library/Application Support/MixRelay/bin/mixrelay-engine-service'],
  ['driver', '/Library/Application Support/MixRelay/bin/mixrelay-driver-service'],
]) capture(`${role}-entitlements.txt`, 'codesign', ['-d', '--entitlements', ':-', path]);
capture('desktop-windows.txt', 'osascript', ['-e',
  'tell application "System Events" to get {name, name of windows} of every process whose background only is false'], 15000);
const processes = capture('product-processes.txt', 'pgrep', ['-u', String(process.getuid()), '-fl', 'mixrelay|MixRelay']);
for (const role of ['engine', 'driver']) {
  const pid = processes.stdout?.split('\n').find(line => line.includes(`/bin/mixrelay-${role}-service`))?.match(/^(\d+) /)?.[1];
  if (pid) capture(`${role}-sample-command.log`, 'sample', [pid, '3', '-file', path(`${role}.sample.txt`)]);
}
for (const line of processes.stdout?.split('\n') ?? []) {
  const pid = line.match(/^(\d+) .*\/Contents\/MacOS\/mixrelay-desktop(?:\s|$)/)?.[1];
  if (pid) capture(`desktop-${pid}-sample-command.log`, 'sample', [pid, '1', '-file', path(`desktop-${pid}.sample.txt`)]);
}
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const { values } = parseArgs({ options: { output: { type: 'string' } } });
  collectMacos(values.output);
}
