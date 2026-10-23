import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readdirSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join, resolve } from 'node:path';
import { parseArgs } from 'node:util';

const { values } = parseArgs({ options: { output: { type: 'string' } } });
assert.equal(process.platform, 'darwin');
assert.equal(process.env.GITHUB_ACTIONS, 'true');
const output = resolve(values.output); mkdirSync(output, { recursive: true });
function capture(file, program, args, timeout = 30000) {
  const result = spawnSync(program, args, { encoding: 'utf8', timeout, maxBuffer: 16 * 1024 * 1024 });
  writeFileSync(join(output, file), `${result.stdout ?? ''}\n${result.stderr ?? ''}\nexit=${result.status}; signal=${result.signal ?? ''}; error=${result.error?.message ?? ''}\n`);
  return result;
}
const logs = join(homedir(), 'Library/Logs/MixRelay');
if (existsSync(logs)) for (const file of readdirSync(logs).filter(name => /^[A-Za-z0-9_-]+\.log$/.test(name))) {
  copyFileSync(join(logs, file), join(output, `app-log-${file}`));
}
capture('coreaudio-devices.txt', 'system_profiler', ['SPAudioDataType', '-json']);
capture('coreaudio-probe.txt', 'zsh', ['scripts/probe-macos-roundtrip.sh', '--list']);
capture('coreaudio-system.log', 'log', ['show', '--last', '5m', '--style', 'compact', '--predicate',
  'process CONTAINS[c] "mixrelay" OR ((process == "coreaudiod" OR process == "tccd") AND eventMessage CONTAINS[c] "mixrelay")']);
capture('desktop-windows.txt', 'osascript', ['-e',
  'tell application "System Events" to get {name, name of windows} of every process whose background only is false'], 15000);
const processes = capture('product-processes.txt', 'pgrep', ['-u', String(process.getuid()), '-fl', 'mixrelay|MixRelay']);
const engine = processes.stdout?.split('\n').find(line => line.includes('/bin/mixrelay-engine-service'))?.match(/^(\d+) /)?.[1];
if (engine) capture('engine-sample-command.log', 'sample', [engine, '3', '-file', join(output, 'engine.sample.txt')]);
