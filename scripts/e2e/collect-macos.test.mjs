import assert from 'node:assert/strict';
import test from 'node:test';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { collectCoreaudioLog, collectCoreaudioServers, collectMacosLoad, snapshotMacosProcesses } from './collect-macos.mjs';

test('post-signal evidence records process CPU without command arguments or changing host services', t => {
  const directory = mkdtempSync(join(tmpdir(), 'mr-post-signal-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  snapshotMacosProcesses(directory, (program, args, options) => {
    assert.equal(program, 'ps');
    assert.ok(args.at(-1).endsWith('comm='));
    assert.ok(options.timeout <= 10000);
    return { status: 0, stdout: '13563 1 Ss 96.0 433016 /System/Library/VTEncoderXPCService\n' };
  });
  assert.match(readFileSync(join(directory, 'native-probe-processes.txt'), 'utf8'), /VTEncoderXPCService\n\n.*exit=0/s);
});

test('immediate failed-PCM collection preserves bounded hardware and CPU diagnostics even if one command fails', t => {
  const directory = mkdtempSync(join(tmpdir(), 'mr-native-load-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const calls = [];
  collectMacosLoad(directory, (program, args, options) => {
    calls.push(program);
    assert.ok(options.timeout <= 10000);
    assert.equal(program === 'top' ? args[args.indexOf('-l') + 1] : '2', '2');
    return program === 'ps' ? { status: 1, stderr: 'process snapshot failed' } : { status: 0, stdout: `${program} diagnostic` };
  });
  assert.deepEqual(calls, ['sysctl', 'ps', 'top']);
  assert.match(readFileSync(join(directory, 'native-failure-processes.txt'), 'utf8'), /process snapshot failed.*\nexit=1/);
  assert.match(readFileSync(join(directory, 'native-failure-load.txt'), 'utf8'), /top diagnostic/);
});

test('large hosted logs retain events after the old 16 MiB buffer boundary', t => {
  const directory = mkdtempSync(join(tmpdir(), 'mr-ca-log-'));
  t.after(() => rmSync(directory, { recursive: true }));
  const path = join(directory, 'system.log');
  const log = collectCoreaudioLog(path, (program, args, options) => {
    assert.equal(program, 'log');
    assert.ok(args.includes('--predicate'));
    return spawnSync(process.execPath, ['-e',
      "process.stdout.write('x'.repeat(17 * 1024 * 1024) + '\\nlate permission granted\\n'); process.stderr.write('late diagnostic\\n');"], options);
  });
  assert.ok(log.length > 17 * 1024 * 1024);
  assert.ok(log.endsWith('late permission granted\nlate diagnostic\n\nexit=0; signal=; error=\n'));
  assert.equal(readFileSync(path, 'utf8'), log);
});

test('CoreAudio samples include the daemon and isolated HAL hosts across users', () => {
  const calls = [];
  collectCoreaudioServers((...args) => {
    calls.push(args);
    return { stdout: '10139 /usr/sbin/coreaudiod\n10185 /System/Library/Frameworks/CoreAudio.framework/Versions/A/XPCServices/com.apple.audio.Core-Audio-Driver-Service.xpc/Contents/MacOS/com.apple.audio.Core-Audio-Driver-Service\n10185 duplicate\nnot-a-pid\n' };
  }, name => `/evidence/live-${name}`);
  assert.deepEqual(calls, [
    ['coreaudio-server-processes.txt', 'pgrep', ['-fl', 'coreaudiod|Core-Audio-Driver-Service']],
    ['coreaudio-server-10139-sample-command.log', 'sudo', ['-n', 'sample', '10139', '1', '-file', '/evidence/live-coreaudio-server-10139.sample.txt']],
    ['coreaudio-server-10185-sample-command.log', 'sudo', ['-n', 'sample', '10185', '1', '-file', '/evidence/live-coreaudio-server-10185.sample.txt']],
  ]);
});

test('missing CoreAudio servers leave the command diagnostic without sampling invalid PIDs', () => {
  const calls = [];
  collectCoreaudioServers((...args) => {
    calls.push(args);
    return { status: 1, stderr: 'process lookup failed' };
  }, name => name);
  assert.equal(calls.length, 1);
  assert.equal(calls[0][0], 'coreaudio-server-processes.txt');
});

test('the hosted registrar PID discovers remote hosts whose executable name was missed by pgrep', () => {
  const calls = [];
  collectCoreaudioServers((...args) => {
    calls.push(args);
    if (args[1] === 'pgrep') return { stdout: '4415 /usr/sbin/coreaudiod\n' };
    if (args[1] === 'ps') return { stdout: args[2][1] === '4420'
      ? ' 4420 /System/Library/Frameworks/CoreAudio.framework/Versions/A/XPCServices/com.apple.audio.DriverHelper.xpc/Contents/MacOS/com.apple.audio.DriverHelper\n'
      : ' 9999 /usr/bin/unrelated\n' };
    return {};
  }, name => name, [
    'HALS_RemotePlugInRegistrar.mm:421 Creating remote driver service: <CFString>{contents = "MixRelayHAL.driver"}, pid: 4420',
    'Creating remote driver service: MixRelayHAL.driver, pid: 4420',
    'Creating remote driver service: MixRelayHAL.driver, pid: 9999',
    'Creating remote driver service: NullAudio.driver, pid: 4421',
  ].join('\n'));
  assert.deepEqual(calls.filter(call => call[1] === 'ps').map(call => call[2][1]), ['4420', '9999']);
  assert.deepEqual(calls.filter(call => call[1] === 'sudo').map(call => call[2][2]), ['4415', '4420']);
});
