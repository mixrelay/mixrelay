import assert from 'node:assert/strict';
import test from 'node:test';
import { prepareGuestKeyring } from './guest-keyring.mjs';

const guest = { disposable: true, platform: 'linux', home: '/home/qa', host: 'mixrelay-qa', uid: 1000,
  env: { GITHUB_ACTIONS: 'true', GITHUB_REPOSITORY: 'mixrelay/mixrelay',
    DBUS_SESSION_BUS_ADDRESS: 'unix:path=/run/user/1000/bus' } };

function fixture(changes = {}) {
  const calls = [];
  return { calls, options: { exists: () => false, read: () => Buffer.from('[keyring]\n'),
    run: (program, args, options) => {
      calls.push({ program, args, options });
      let stdout = '';
      if (args.includes('org.freedesktop.Secret.Service.ReadAlias')) {
        stdout = "(objectpath '/org/freedesktop/secrets/collection/login',)\n";
      } else if (program === 'gdbus') stdout = '(<false>,)\n';
      return { status: 0, stdout };
    }, ...changes } };
}

test('fresh guest keyring uses an empty password and verifies collection readiness without storing app credentials', () => {
  const f = fixture();
  assert.deepEqual(prepareGuestKeyring(guest, f.options), {
    scope: 'host-readiness', collection: 'login', locked: false, passwordProtected: false,
  });
  assert.equal(f.calls.length, 4);
  assert.deepEqual(f.calls[0].options.input, Buffer.from([0]));
  assert.deepEqual(f.calls[0].args, ['--unlock', '--components=secrets']);
  assert.deepEqual(f.calls[1].args, ['--start', '--components=secrets']);
  assert.ok(f.calls.slice(2).every(call => call.program === 'gdbus' && call.args.includes('--timeout')));
  assert.equal(f.calls.some(call => call.args.some(arg => /Prompt|CreateCollection|CreateItem|Unlock/.test(arg))), false);
});

test('native errors identify the OS boundary and stage and stop before later preparation commands', () => {
  for (const [index, stage] of ['initialize', 'start', 'default-collection', 'lock-state'].entries()) {
    const f = fixture();
    const run = f.options.run;
    let calls = 0;
    f.options.run = (...args) => calls++ === index ? { status: 7 } : run(...args);
    assert.throws(() => prepareGuestKeyring(guest, f.options), new RegExp(`Linux guest Secret Service/${stage}`));
    assert.equal(calls, index + 1);
  }
});

test('local hosts, another session and reused keyrings are rejected before native commands', () => {
  for (const change of [{ disposable: false }, { platform: 'darwin' }, { home: '/Users/qa' },
    { host: 'my-mac' }, { uid: 0 }, { env: {} },
    { env: { ...guest.env, DBUS_SESSION_BUS_ADDRESS: 'unix:path=/run/user/1001/bus' } }]) {
    const f = fixture();
    assert.throws(() => prepareGuestKeyring({ ...guest, ...change }, f.options), /disposable qualification guest/);
    assert.equal(f.calls.length, 0);
  }
  const f = fixture({ exists: () => true });
  assert.throws(() => prepareGuestKeyring(guest, f.options), /must be fresh/);
  assert.equal(f.calls.length, 0);
});

test('missing or locked collections, encrypted storage and native failures fail preparation', () => {
  const failures = [
    { read: () => Buffer.from('GnomeKeyring\n\r\0\n') },
    { run: () => ({ status: 0, stdout: "(objectpath '/',)" }) },
    { run: (program, args) => ({ status: 0, stdout: args.includes('org.freedesktop.Secret.Service.ReadAlias')
      ? "(objectpath '/org/freedesktop/secrets/collection/login',)" : '(<true>,)' }) },
    { run: () => ({ status: 1, stdout: '' }) },
    { run: () => ({ status: null, error: Object.assign(new Error('timed out'), { code: 'ETIMEDOUT' }) }) },
  ];
  for (const options of failures) {
    assert.throws(() => prepareGuestKeyring(guest, fixture(options).options));
  }
});
