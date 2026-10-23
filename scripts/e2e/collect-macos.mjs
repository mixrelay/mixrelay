import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readdirSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';

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
capture('coreaudio-devices.txt', 'system_profiler', ['SPAudioDataType', '-json']);
capture('coreaudio-probe.txt', 'zsh', ['scripts/probe-macos-roundtrip.sh', '--list']);
for (const [role, path] of [
  ['desktop', '/Applications/MixRelay/MixRelay.app'],
  ['engine', '/Library/Application Support/MixRelay/bin/mixrelay-engine-service'],
  ['driver', '/Library/Application Support/MixRelay/bin/mixrelay-driver-service'],
]) capture(`${role}-entitlements.txt`, 'codesign', ['-d', '--entitlements', ':-', path]);
capture('coreaudio-system.log', 'log', ['show', '--last', '5m', '--style', 'compact', '--predicate',
  'process CONTAINS[c] "mixrelay" OR ((process == "coreaudiod" OR process == "tccd") AND eventMessage CONTAINS[c] "mixrelay")']);
capture('desktop-windows.txt', 'osascript', ['-e',
  'tell application "System Events" to get {name, name of windows} of every process whose background only is false'], 15000);
const processes = capture('product-processes.txt', 'pgrep', ['-u', String(process.getuid()), '-fl', 'mixrelay|MixRelay']);
const engine = processes.stdout?.split('\n').find(line => line.includes('/bin/mixrelay-engine-service'))?.match(/^(\d+) /)?.[1];
if (engine) capture('engine-sample-command.log', 'sample', [engine, '3', '-file', path('engine.sample.txt')]);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const { values } = parseArgs({ options: { output: { type: 'string' } } });
  collectMacos(values.output);
}
