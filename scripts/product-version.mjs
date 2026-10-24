#!/usr/bin/env node
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const manifests = ['package.json', 'apps/control-ui/package.json', 'apps/desktop/package.json', 'apps/desktop/src-tauri/tauri.conf.json'];
const halManifest = 'native/macos/hal-driver/driver/Info.plist';
export function productVersion(directory = root) {
  const cargo = readFileSync(resolve(directory, 'Cargo.toml'), 'utf8');
  const version = cargo.match(/\[workspace.package\]\s*\nversion = "([^"\n]+)"/)?.[1];
  if (!version || !/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version)) throw new Error('Cargo workspace must define a stable product version');
  return version;
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
  writeFileSync(cargoPath, cargo.replace(/(\[workspace.package\]\s*\nversion = ")[^"\n]+/, `$1${version}`));
  paths.forEach((path, index) => writeFileSync(path, `${JSON.stringify({ ...values[index], version }, null, 2)}\n`));
  for (const key of ['CFBundleShortVersionString', 'CFBundleVersion']) hal = hal.replace(new RegExp(`(<key>${key}</key>\\s*<string>)[^<]+`), `$1${version}`);
  writeFileSync(halPath, hal);
  return checkVersion(directory);
}
if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  try {
    const [action = 'check', version, ...extra] = process.argv.slice(2);
    if (action === '--help') console.log('pnpm product:version [check | set MAJOR.MINOR.PATCH]\nCargo workspace is the product version; app and all native components ship together.');
    else if (action === 'check' && version === undefined) console.log(`MixRelay ${checkVersion()}`);
    else if (action === 'set' && version && extra.length === 0) console.log(`MixRelay ${setVersion(version)}`);
    else throw new Error('Use --help for product version commands');
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
