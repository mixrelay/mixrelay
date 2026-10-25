import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { homedir, hostname } from 'node:os';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

export function prepareGuestKeyring(context, { run = spawnSync, exists = existsSync, read = readFileSync } = {}) {
  const { disposable, platform, home, host, uid, env } = context;
  assert.ok(disposable && platform === 'linux' && home === '/home/qa' && host === 'mixrelay-qa'
    && uid > 0 && env.GITHUB_ACTIONS === 'true' && env.GITHUB_REPOSITORY === 'mixrelay/mixrelay'
    && env.DBUS_SESSION_BUS_ADDRESS === `unix:path=/run/user/${uid}/bus`,
  'Keyring preparation requires the disposable qualification guest');
  const keyring = `${home}/.local/share/keyrings/login.keyring`;
  assert.equal(exists(keyring), false, 'Qualification keyring must be fresh');
  function command(stage, program, args, input) {
    const result = run(program, args, { input, encoding: 'utf8', timeout: 10000, maxBuffer: 65536 });
    if (result.error) throw new Error(`Linux guest Secret Service/${stage}: ${result.error.code ?? result.error.message}`, { cause: result.error });
    assert.equal(result.status, 0, `Linux guest Secret Service/${stage}: ${program} exit ${result.status}, signal ${result.signal ?? 'none'}`);
    return result.stdout.trim();
  }
  // GNOME includes LF in the password; EOF alone means no password was supplied.
  // A NUL supplies an empty C string so daemon restart needs no password prompt.
  command('initialize', 'gnome-keyring-daemon', ['--unlock', '--components=secrets'], Buffer.from([0]));
  // --start waits for initialization through the daemon control socket.
  command('start', 'gnome-keyring-daemon', ['--start', '--components=secrets']);
  function dbus(stage, path, method, ...args) {
    return command(stage, 'gdbus', ['call', '--session', '--timeout', '5', '--dest', 'org.freedesktop.secrets',
      '--object-path', path, '--method', method, ...args]);
  }
  const alias = dbus('default-collection', '/org/freedesktop/secrets', 'org.freedesktop.Secret.Service.ReadAlias', 'default');
  assert.equal(alias, "(objectpath '/org/freedesktop/secrets/collection/login',)", 'Missing default login collection');
  const locked = dbus('lock-state', '/org/freedesktop/secrets/collection/login', 'org.freedesktop.DBus.Properties.Get',
    'org.freedesktop.Secret.Collection', 'Locked');
  assert.equal(locked, '(<false>,)', 'Qualification keyring is locked');
  assert.equal(read(keyring).subarray(0, 10).toString(), '[keyring]\n', 'Qualification keyring has a nonempty password');
  return { scope: 'host-readiness', collection: 'login', locked: false, passwordProtected: false };
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const result = prepareGuestKeyring({ disposable: process.argv[2] === '--disposable-environment',
    platform: process.platform, home: homedir(), host: hostname(), uid: process.getuid(), env: process.env });
  console.log(JSON.stringify(result));
}
