import assert from 'node:assert/strict';
import test from 'node:test';
import { collectCoreaudioServers } from './collect-macos.mjs';

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
