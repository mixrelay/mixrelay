import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { arch, platform, release } from 'node:os';
import { digest, matchesVersion, ciMatrices } from '../support-contract.mjs';

const run = (program, args) => execFileSync(program, args, { encoding: 'utf8', timeout: 15000, maxBuffer: 8 * 1024 * 1024 }).trim();
export function parseOsRelease(text) {
  return Object.fromEntries(text.split('\n').filter(line => /^[A-Z_]+=/.test(line)).map(line => {
    const index = line.indexOf('=');
    return [line.slice(0, index), line.slice(index + 1).replace(/^"(.*)"$/, '$1')];
  }));
}
export function versionAtLeast(actual, minimum) {
  const parts = value => value.split('.').map(Number);
  const left = parts(actual), right = parts(minimum);
  if (![...left, ...right].every(Number.isSafeInteger)) return false;
  for (let i = 0; i < Math.max(left.length, right.length); i++) {
    const difference = (left[i] ?? 0) - (right[i] ?? 0);
    if (difference !== 0) return difference > 0;
  }
  return true;
}
function version(text) {
  const value = text.match(/\d+\.\d+(?:\.\d+)?/);
  if (!value) throw new Error('Dependency did not report a version');
  return value[0];
}

export function observeEnvironment() {
  const os = platform() === 'darwin' ? 'macos' : platform();
  const observed = { os, arch: arch(), kernel: release(), kind: 'native-session' };
  observed.desktopSession = { type: process.env.XDG_SESSION_TYPE ?? (os === 'macos' ? 'aqua' : 'unknown'),
    desktop: process.env.XDG_CURRENT_DESKTOP ?? null,
    display: process.env.WAYLAND_DISPLAY ? 'wayland' : process.env.DISPLAY ? 'x11' : null };
  if (os === 'macos') {
    observed.version = run('sw_vers', ['-productVersion']);
    observed.osRelease = run('sw_vers', []);
    observed.packageInventorySha256 = digest(run('pkgutil', ['--pkgs']));
    try { if (run('sysctl', ['-in', 'sysctl.proc_translated']) === '1') observed.translated = true; } catch {}
  } else if (os === 'linux') {
    observed.osRelease = readFileSync('/etc/os-release', 'utf8');
    const distro = parseOsRelease(observed.osRelease);
    observed.distro = distro.ID;
    observed.version = distro.VERSION_ID || distro.BUILD_ID || 'rolling';
    if (existsSync('/.dockerenv') || existsSync('/run/.containerenv')
      || existsSync('/run/systemd/container')) observed.kind = 'container';
    observed.dependencies = {
      pipewire: version(run('pipewire', ['--version'])),
      wireplumber: version(run('wireplumber', ['--version'])),
      glibc: version(run('getconf', ['GNU_LIBC_VERSION'])),
    };
    const packages = observed.distro === 'arch' ? run('pacman', ['-Q'])
      : run('dpkg-query', ['-W', '-f=${Package}\t${Version}\t${Architecture}\n']);
    observed.packageInventorySha256 = digest(packages);
  } else throw new Error(`Unsupported qualification host: ${os}`);
  return observed;
}

export function validateEnvironment(observed, target, contract) {
  if (observed.os !== target.os || observed.arch !== target.arch || observed.translated
    || observed.kind !== 'native-session') throw new Error(`Wrong native qualification host: ${target.id}`);
  if (target.distro && observed.distro !== target.distro) throw new Error(`Wrong distribution: ${target.id}`);
  if (!matchesVersion(observed.version, target)) throw new Error(`Wrong OS version: ${target.id}: ${observed.version}`);
  if (observed.os === 'macos' && !versionAtLeast(observed.version, contract.minimums.macos)) throw new Error(`Unsupported macOS version: ${observed.version}`);
  if (!observed.kernel || !observed.osRelease || !/^[a-f0-9]{64}$/.test(observed.packageInventorySha256)) {
    throw new Error(`Incomplete environment fingerprint: ${target.id}`);
  }
  if (observed.os === 'linux') {
    for (const [name, minimum] of Object.entries(contract.minimums.linux)) {
      if (!observed.dependencies?.[name] || !versionAtLeast(observed.dependencies[name], minimum)) {
        throw new Error(`Unsupported ${name} on ${target.id}: requires ${minimum}`);
      }
    }
  }
}

export function validateCodeCheckEnvironment(observed, targetId, contract) {
  const mac = contract.codeChecks.macos.find(target => target.id === targetId);
  const linux = ciMatrices(contract).linux.include.find(target => target.id === targetId);
  const distro = contract.codeChecks.linuxDistributions.find(item => item.id === linux?.distro);
  const expected = mac ?? (linux && { ...linux, os: 'linux', version: distro.version });
  if (!expected || observed.os !== expected.os || observed.arch !== expected.arch || observed.translated
    || (linux && observed.distro !== linux.distro)) throw new Error(`Wrong source CI host: ${targetId}`);
  const versionMatch = expected.version === 'rolling' ? 'rolling' : expected.version.includes('.') ? 'minor' : 'major';
  if (!matchesVersion(observed.version, { version: expected.version, versionMatch })) throw new Error(`Wrong source CI OS version: ${targetId}`);
}
