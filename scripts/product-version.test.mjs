import assert from 'node:assert/strict';
import test from 'node:test';
import { checkVersion, productVersion, setVersion } from './product-version.mjs';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';

test('all release entry points share the Cargo product version', () => {
  assert.equal(checkVersion(), productVersion());
  const desktop = JSON.parse(readFileSync(new URL('../apps/desktop/src-tauri/tauri.conf.json', import.meta.url)));
  assert.equal(desktop.identifier, 'io.mixrelay.desktop');
  assert.equal(desktop.productName, 'MixRelay');
  const linux = JSON.parse(readFileSync(new URL('../apps/desktop/src-tauri/tauri.linux.conf.json', import.meta.url)));
  assert.deepEqual(linux.bundle.externalBin, ['binaries/mixrelay-driver-service', 'binaries/mixrelay-engine-service']);
  for (const format of ['deb', 'rpm']) {
    const config = linux.bundle.linux[format];
    assert.equal(config.recommends, undefined);
    assert.ok(config.files['/usr/lib/systemd/user/mixrelay-driver.service']);
  }
});

function versionFixture(t) {
  const parent = new URL('../target/test-artifacts/', import.meta.url);
  mkdirSync(parent, { recursive: true });
  const directory = mkdtempSync(new URL('product-version-', parent));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const paths = ['Cargo.toml', 'package.json', 'apps/control-ui/package.json', 'apps/desktop/package.json', 'apps/desktop/src-tauri/tauri.conf.json', 'native/macos/hal-driver/driver/Info.plist'];
  for (const path of paths) {
    mkdirSync(dirname(join(directory, path)), { recursive: true });
    copyFileSync(new URL(`../${path}`, import.meta.url), join(directory, path));
  }
  return { directory, paths };
}

test('one version command updates app and HAL metadata together', t => {
  const { directory } = versionFixture(t);
  assert.equal(setVersion('0.2.0', directory), '0.2.0');
  assert.equal(checkVersion(directory), '0.2.0');
  const hal = readFileSync(join(directory, 'native/macos/hal-driver/driver/Info.plist'), 'utf8');
  assert.match(hal, /<key>CFBundleShortVersionString<\/key>\s*<string>0\.2\.0<\/string>/);
  assert.equal(setVersion('0.1.0', directory), '0.1.0');
});

test('invalid HAL metadata rejects a version change before writing any manifest', t => {
  const { directory, paths } = versionFixture(t);
  writeFileSync(join(directory, 'native/macos/hal-driver/driver/Info.plist'), '<plist/>');
  const before = paths.map(path => readFileSync(join(directory, path)));
  assert.throws(() => setVersion('0.2.0', directory), /missing CFBundle/);
  paths.forEach((path, index) => assert.deepEqual(readFileSync(join(directory, path)), before[index]));
});
