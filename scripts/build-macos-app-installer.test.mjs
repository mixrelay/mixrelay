import assert from 'node:assert/strict';
import test from 'node:test';
import { buildAppInstaller, parseAppInstallerArgs } from './build-macos-app-installer.mjs';
import { buildPackages, distribution } from './build-macos-installer-pkgs.mjs';

test('product build uses the full native transaction, never a separate app/optional Driver transaction', () => {
  assert.equal(buildAppInstaller, buildPackages);
  const xml = distribution('MixRelay.component.pkg', '0.1.1');
  assert.match(xml, /<title>MixRelay<\/title>/);
  assert.match(xml, /customize="never"/);
  // Without it Installer re-executes as Intel and asks Apple silicon Macs for Rosetta.
  assert.match(xml, /hostArchitectures="arm64,x86_64"/);
  assert.equal((xml.match(/<line choice=/g) ?? []).length, 1);
  assert.doesNotMatch(xml, /driverNeedsInstall|active=|choice="driver"|MixRelay Driver/);
});
test('complete product packaging requires one prepared payload and rejects legacy split inputs', () => {
  assert.throws(() => parseAppInstallerArgs(['--app', '/app', '--driverPkg', '/driver.pkg']), /unknown|unsupported|argument/i);
  assert.throws(() => parseAppInstallerArgs(['--payload', '/payload']), /missing/i);
});

test('every macOS build and the Installer share the app minimum macOS version', async () => {
  const { readFileSync } = await import('node:fs');
  const { MINIMUM_MACOS } = await import('./build-macos-installer-pkgs.mjs');
  assert.match(MINIMUM_MACOS, /^\d+\.\d+$/);
  assert.match(distribution('MixRelay.component.pkg', '0.1.1'), new RegExp(`<allowed-os-versions><os-version min="${MINIMUM_MACOS.replace('.', '\\.')}"/></allowed-os-versions>`));
  // Native code compiled for an older target would load on systems the app rejects, and
  // a newer one would make the Installer accept systems the binaries cannot run on.
  for (const path of [
    'scripts/build-macos-universal-app.sh', 'scripts/build-macos-universal-engine.sh', 'scripts/build-macos-driver-launcher.sh',
    'apps/engine-service/build.rs', 'native/macos/common/build.rs', 'native/macos/process-tap/build.rs',
    'native/macos/driver-control/build.rs', 'native/macos/hal-driver/bundle/src/build.rs',
  ]) {
    const source = readFileSync(new URL(`../${path}`, import.meta.url), 'utf8');
    const targets = [...source.matchAll(/(?:version-min=|MACOSX_DEPLOYMENT_TARGET=?"?)(\d+\.\d+)/g)].map(match => match[1]);
    assert.ok(targets.length > 0, `${path} declares its macOS target`);
    assert.deepEqual([...new Set(targets)], [MINIMUM_MACOS], path);
  }
});
