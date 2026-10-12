import assert from 'node:assert/strict';
import test from 'node:test';
import { buildAppInstaller, parseAppInstallerArgs } from './build-macos-app-installer.mjs';
import { buildPackages, distribution } from './build-macos-installer-pkgs.mjs';

test('product build uses the full native transaction, never a separate app/optional Driver transaction', () => {
  assert.equal(buildAppInstaller, buildPackages);
  const xml = distribution('MixRelay.component.pkg', '0.1.1');
  assert.match(xml, /<title>MixRelay<\/title>/);
  assert.match(xml, /customize="never"/);
  assert.equal((xml.match(/<line choice=/g) ?? []).length, 1);
  assert.doesNotMatch(xml, /driverNeedsInstall|active=|choice="driver"|MixRelay Driver/);
});
test('complete product packaging requires one prepared payload and rejects legacy split inputs', () => {
  assert.throws(() => parseAppInstallerArgs(['--app', '/app', '--driverPkg', '/driver.pkg']), /unknown|unsupported|argument/i);
  assert.throws(() => parseAppInstallerArgs(['--payload', '/payload']), /missing/i);
});
