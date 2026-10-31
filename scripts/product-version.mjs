#!/usr/bin/env node
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const manifests = ['package.json', 'apps/control-ui/package.json', 'apps/desktop/package.json', 'apps/desktop/src-tauri/tauri.conf.json'];
const halManifest = 'native/macos/hal-driver/driver/Info.plist';
// `cargo build --locked` rejects a lockfile whose workspace crate versions differ from their manifests.
// Path packages have no `source` line; registry and git packages keep their own versions.
const lockedPackage = /^\[\[package\]\]\nname = "([^"\n]+)"\nversion = "([^"\n]+)"\n(?!source = )/gm;
export function productVersion(directory = root) {
  const cargo = readFileSync(resolve(directory, 'Cargo.toml'), 'utf8');
  const version = cargo.match(/\[workspace.package\]\s*\nversion = "([^"\n]+)"/)?.[1];
  if (!version || !/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version)) throw new Error('Cargo workspace must define a stable product version');
  return version;
}
export function workspaceMembers(directory = root) {
  const members = readFileSync(resolve(directory, 'Cargo.toml'), 'utf8').match(/^members = \[([^\]]*)\]/m)?.[1];
  if (!members) throw new Error('Cargo workspace must list its members');
  return [...members.matchAll(/"([^"\n]+)"/g)].map(([, member]) => member);
}
// Crates that inherit the product version. Vendored crates and fixtures keep their own versions.
export function productCrates(directory = root) {
  return workspaceMembers(directory).flatMap(member => {
    const manifest = readFileSync(resolve(directory, member, 'Cargo.toml'), 'utf8');
    const section = manifest.match(/^\[package\]\n((?:(?!\[)[^\n]*(?:\n|$))*)/m)?.[1] ?? '';
    if (!/^version(?:\.workspace\s*=\s*true|\s*=\s*\{\s*workspace\s*=\s*true\s*\})\s*$/m.test(section)) return [];
    const name = section.match(/^name = "([^"\n]+)"/m)?.[1];
    if (!name) throw new Error(`${member}/Cargo.toml must name its package`);
    return [name];
  });
}
function lockedVersions(lock) {
  return new Map([...lock.matchAll(lockedPackage)].map(([, name, version]) => [name, version]));
}
function updatedLock(lock, crates, version) {
  const locked = lockedVersions(lock);
  const missing = crates.filter(name => !locked.has(name));
  if (missing.length) throw new Error(`Cargo.lock is missing workspace crates: ${missing.join(', ')}`);
  const names = new Set(crates);
  return lock.replace(lockedPackage, (entry, name) => names.has(name) ? `[[package]]\nname = "${name}"\nversion = "${version}"\n` : entry);
}
export function checkVersion(directory = root) {
  const version = productVersion(directory);
  for (const filename of manifests) {
    if (JSON.parse(readFileSync(resolve(directory, filename), 'utf8')).version !== version) throw new Error(`${filename} differs from MixRelay ${version}`);
  }
  const hal = readFileSync(resolve(directory, halManifest), 'utf8');
  for (const key of ['CFBundleShortVersionString', 'CFBundleVersion']) {
    if (hal.match(new RegExp(`<key>${key}</key>\\s*<string>([^<]+)</string>`))?.[1] !== version) throw new Error(`${halManifest} ${key} differs from MixRelay ${version}`);
  }
  const locked = lockedVersions(readFileSync(resolve(directory, 'Cargo.lock'), 'utf8'));
  for (const name of productCrates(directory)) {
    if (locked.get(name) !== version) throw new Error(`Cargo.lock ${name} differs from MixRelay ${version}`);
  }
  return version;
}
export function setVersion(version, directory = root) {
  if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version)) throw new Error('Use stable MAJOR.MINOR.PATCH');
  const paths = manifests.map(name => resolve(directory, name));
  const values = paths.map(path => JSON.parse(readFileSync(path, 'utf8')));
  const cargoPath = resolve(directory, 'Cargo.toml');
  const cargo = readFileSync(cargoPath, 'utf8');
  const halPath = resolve(directory, halManifest);
  let hal = readFileSync(halPath, 'utf8');
  for (const key of ['CFBundleShortVersionString', 'CFBundleVersion']) {
    if (!new RegExp(`<key>${key}</key>\\s*<string>[^<]+</string>`).test(hal)) throw new Error(`${halManifest} is missing ${key}`);
  }
  productVersion(directory); // Validate all inputs before changing any of them.
  const lockPath = resolve(directory, 'Cargo.lock');
  const lock = updatedLock(readFileSync(lockPath, 'utf8'), productCrates(directory), version);
  writeFileSync(cargoPath, cargo.replace(/(\[workspace.package\]\s*\nversion = ")[^"\n]+/, `$1${version}`));
  paths.forEach((path, index) => writeFileSync(path, `${JSON.stringify({ ...values[index], version }, null, 2)}\n`));
  for (const key of ['CFBundleShortVersionString', 'CFBundleVersion']) hal = hal.replace(new RegExp(`(<key>${key}</key>\\s*<string>)[^<]+`), `$1${version}`);
  writeFileSync(halPath, hal);
  writeFileSync(lockPath, lock);
  return checkVersion(directory);
}
if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  try {
    const [action = 'check', version, ...extra] = process.argv.slice(2);
    if (action === '--help') console.log('pnpm product:version [check | set MAJOR.MINOR.PATCH]\nCargo workspace is the product version; app and all native components ship together.\nset also updates the Cargo.lock entries of workspace crates that inherit it, so `cargo build --locked` stays valid.');
    else if (action === 'check' && version === undefined) console.log(`MixRelay ${checkVersion()}`);
    else if (action === 'set' && version && extra.length === 0) console.log(`MixRelay ${setVersion(version)}`);
    else throw new Error('Use --help for product version commands');
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
