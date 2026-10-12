#!/usr/bin/env node
import { spawn, spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, lstatSync, mkdirSync, readFileSync, rmSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { homedir } from 'node:os';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const DEFAULT_SIGNING_CONFIG = join(homedir(), 'Library/Application Support/MixRelay/Signing/background-stop-20261011/config.json');
export const RETIRED_SIGNING_KEYCHAIN = join(homedir(), 'Library/Caches/io.mixrelay.signing/driver-20261008/release.keychain-db');
const root = fileURLToPath(new URL('..', import.meta.url));
function privateFile(path) {
  const value = lstatSync(path);
  if (!value.isFile() || value.isSymbolicLink() || value.uid !== process.getuid() || (value.mode & 0o077)) throw new Error('Signing configuration must be a regular owner-only file');
}
function command(program, args, secrets = []) {
  const result = spawnSync(program, args, { encoding: 'utf8', timeout: 45000, maxBuffer: 1024 * 1024 });
  if (result.error || result.status !== 0) {
    let message = result.error?.message || result.stderr || 'Signing command failed';
    for (const secret of secrets.filter(Boolean)) message = message.replaceAll(secret, '[redacted]');
    throw new Error(`${program}: ${message.trim()}`);
  }
  return result.stdout.trim();
}
/** Preserve unrelated keychains while excluding the known unusable signing copy. */
export function usableKeychainSearchList(paths) {
  return paths.filter(path => resolve(path) !== RETIRED_SIGNING_KEYCHAIN);
}
function searchList() {
  return command('/usr/bin/security', ['list-keychains', '-d', 'user']).split('\n').filter(Boolean).map(line => JSON.parse(line.trim()));
}
/** Remove only the retired lookup entry; never delete keychain files or change lock policy. */
export function repairSigningSearchList() {
  const original = searchList();
  const usable = usableKeychainSearchList(original);
  if (usable.length !== original.length) command('/usr/bin/security', ['list-keychains', '-d', 'user', '-s', ...usable]);
  return { retiredEntryRemoved: usable.length !== original.length, remainingKeychains: usable.length };
}
/** Unlock the restored keychain; restore unrelated search entries on every exit. */
export function withMacosSigning(action, configPath = process.env.MIXRELAY_SIGNING_CONFIG || DEFAULT_SIGNING_CONFIG) {
  if (process.platform !== 'darwin') throw new Error('macOS signing requires macOS');
  privateFile(configPath);
  const config = JSON.parse(readFileSync(configPath, 'utf8'));
  const keychain = resolve(config.keychain);
  if (dirname(keychain) !== dirname(resolve(configPath)) || !existsSync(keychain)) throw new Error('Use the restored build keychain next to its configuration');
  if (!/^[A-Z0-9]{10}$/.test(config.teamId) || !config.applicationIdentity?.endsWith(`(${config.teamId})`) || !config.installerIdentity?.endsWith(`(${config.teamId})`)) throw new Error('Matching Application and Installer signing identities required');
  const secrets = [config.keychainPassword, config.p12Password];
  command('/usr/bin/security', ['unlock-keychain', '-p', config.keychainPassword, keychain], secrets);
  const identities = command('/usr/bin/security', ['find-identity', '-v', keychain]);
  for (const identity of [config.applicationIdentity, config.installerIdentity]) {
    if (!identities.includes(`"${identity}"`)) throw new Error('Restored keychain is missing a required release identity');
  }
  const original = usableKeychainSearchList(searchList());
  // The preceding release keychain has a known stale password. It must not
  // shadow the restored private keys while this build is running.
  const active = [keychain, ...original.filter(path => path !== keychain)];
  command('/usr/bin/security', ['list-keychains', '-d', 'user', '-s', ...active]);
  // Compilation can outlast the keychain's 300-second idle lock. Refresh only
  // this restored build keychain while its owning build exists; keep OS policy.
  const refresh = spawn(process.execPath, [fileURLToPath(import.meta.url), '--keep-unlocked', resolve(configPath), String(process.pid)], { stdio: ['ignore', 'ignore', 'inherit'] });
  const cleanup = () => {
    refresh.kill('SIGTERM');
    command('/usr/bin/security', ['list-keychains', '-d', 'user', '-s', ...original]);
  };
  let pending = false;
  try {
    const result = action({ ...process.env, APPLE_SIGNING_IDENTITY: config.applicationIdentity, MIXRELAY_CODESIGN_IDENTITY: config.applicationIdentity, MIXRELAY_INSTALLER_IDENTITY: config.installerIdentity, MIXRELAY_SIGNING_KEYCHAIN: keychain }, config);
    if (result && typeof result.then === 'function') {
      pending = true;
      return Promise.resolve(result).finally(cleanup);
    }
    return result;
  } finally {
    if (!pending) cleanup();
  }
}
/** Actual code and PKG signing probe; does not install, change ACLs, or publish anything. */
export function checkMacosSigning(configPath) {
  return withMacosSigning((env, config) => {
    const proof = join(root, '.work/signing-preflight');
    mkdirSync(proof, { recursive: true });
    const executable = join(proof, 'probe');
    copyFileSync('/usr/bin/true', executable);
    command('/usr/bin/codesign', ['--force', '--sign', env.APPLE_SIGNING_IDENTITY, '--keychain', env.MIXRELAY_SIGNING_KEYCHAIN, '--options', 'runtime', '--timestamp', '--identifier', 'io.mixrelay.signing.preflight', executable]);
    command('/usr/bin/codesign', ['--verify', '--strict', executable]);
    const pkg = join(proof, 'probe.pkg');
    if (existsSync(pkg)) rmSync(pkg);
    command('/usr/bin/pkgbuild', ['--nopayload', '--identifier', 'io.mixrelay.signing.preflight', '--version', '0.1.0', '--sign', env.MIXRELAY_INSTALLER_IDENTITY, '--keychain', env.MIXRELAY_SIGNING_KEYCHAIN, '--timestamp', pkg]);
    const signature = command('/usr/sbin/pkgutil', ['--check-signature', pkg]);
    if (!signature.includes(env.MIXRELAY_INSTALLER_IDENTITY) || !signature.includes('trusted')) throw new Error('PKG signature did not pass the release identity gate');
    return { keychainUnlocked: true, applicationSignatureVerified: true, installerSignatureVerified: true, teamId: config.teamId };
  }, configPath);
}
if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  try {
    if (process.argv[2] === '--keep-unlocked') {
      const configPath = process.argv[3];
      const owner = Number(process.argv[4]);
      if (!Number.isSafeInteger(owner) || owner <= 1) throw new Error('Signing refresh requires its build process owner');
      privateFile(configPath);
      const config = JSON.parse(readFileSync(configPath, 'utf8'));
      const refresh = () => {
        try { process.kill(owner, 0); }
        catch { process.exit(0); }
        command('/usr/bin/security', ['unlock-keychain', '-p', config.keychainPassword, config.keychain], [config.keychainPassword, config.p12Password]);
      };
      refresh();
      setInterval(() => { try { refresh(); } catch (error) { console.error(error.message); process.exit(1); } }, 45000);
    } else if (process.argv[2] === '--repair-search-list') console.log(JSON.stringify(repairSigningSearchList()));
    else if (process.argv[2] === '--help') console.log('pnpm signing:check [--repair-search-list]\nUnlocks the restored MixRelay build keychain, signs/verifies code and PKG probes, and restores unrelated keychain search entries. The retired signing copy is excluded on exit.\n--repair-search-list removes only that stale lookup entry without signing, deleting keys or changing lock policy.\nMIXRELAY_SIGNING_CONFIG selects a private local configuration. Passwords never enter repository files or logs.');
    else if (process.argv.length === 2) console.log(JSON.stringify(checkMacosSigning()));
    else throw new Error('Use pnpm signing:check or --help');
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
