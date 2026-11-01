import assert from 'node:assert/strict';
import test from 'node:test';
import { gzipSync } from 'node:zlib';
import { assertInstallerLicensing } from './installer-configuration.mjs';
import { fixtureCpio, fixtureTar, fixtureXar, installerFixtureBytes, macosDesktopPayload } from './installer-configuration.fixture.mjs';
import { fatBinary, polarEnvironment } from './polar-configuration.fixture.mjs';
import { licensingBuildMarker } from './polar-configuration.mjs';

const marker = licensingBuildMarker(polarEnvironment);

for (const format of ['pkg', 'deb', 'arch']) {
  test(`${format} reads the desktop configuration without executing installer scripts`, () => {
    const archive = installerFixtureBytes(format, undefined, {
      extraEntries: { 'postinstall': 'exit 99\n' },
    });
    assert.deepEqual(assertInstallerLicensing(archive, format, polarEnvironment), polarEnvironment);
  });

  test(`${format} cannot use configuration in another payload file`, () => {
    const archive = installerFixtureBytes(format, 'Unconfigured desktop', {
      extraEntries: { 'public-config.txt': marker, 'usr/bin/mixrelay-service': marker },
    });
    assert.throws(() => assertInstallerLicensing(archive, format, polarEnvironment), /missing Polar payment configuration/);
  });

  test(`${format} rejects unsafe archive members before extracting the desktop`, () => {
    const archive = installerFixtureBytes(format, undefined, { extraEntries: { '../outside': marker } });
    assert.throws(() => assertInstallerLicensing(archive, format, polarEnvironment), /Unsafe installer payload member/);
  });
}

test('PKG inspection supports embedded component archives and checks every desktop architecture', () => {
  assert.deepEqual(assertInstallerLicensing(installerFixtureBytes('pkg', undefined, { nested: true }),
    'pkg', polarEnvironment), polarEnvironment);
  for (const payloads of [[marker, 'Unconfigured Intel'], ['Unconfigured ARM', marker]]) {
    assert.throws(() => assertInstallerLicensing(installerFixtureBytes('pkg', fatBinary(payloads)),
      'pkg', polarEnvironment), /missing Polar payment configuration/);
  }
});

test('installer inspection rejects missing or ambiguous desktop payloads', () => {
  assert.throws(() => assertInstallerLicensing(fixtureXar({ Distribution: Buffer.from(marker) }),
    'pkg', polarEnvironment), /Absent PKG component payload/);
  assert.throws(() => assertInstallerLicensing(fixtureTar({ 'public-config.txt': marker }),
    'arch', polarEnvironment), /absent installer payload/);
  const desktop = fixtureTar({ 'usr/bin/mixrelay-desktop': marker });
  const duplicate = Buffer.concat([desktop.subarray(0, -1024), desktop]);
  assert.throws(() => assertInstallerLicensing(duplicate, 'arch', polarEnvironment), /Ambiguous/);
});

test('PKG inspection cannot ignore embedded components when expanded payloads also exist', () => {
  const payload = binary => gzipSync(fixtureCpio({ [macosDesktopPayload]: binary }));
  const archive = fixtureXar({
    'Expanded.component.pkg/Payload': payload(fatBinary([marker, marker])),
    'Embedded.component.pkg': fixtureXar({ Payload: payload('Unconfigured desktop') }),
  });
  assert.throws(() => assertInstallerLicensing(archive, 'pkg', polarEnvironment), /Ambiguous/);
});
