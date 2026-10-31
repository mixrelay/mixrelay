// Synthetic archives exercise payload inspection without installing or executing code.
import { deflateSync, gzipSync, zstdCompressSync } from 'node:zlib';
import { writeFileSync } from 'node:fs';
import { licensingBuildMarker } from './polar-configuration.mjs';
import { fatBinary, polarEnvironment } from './polar-configuration.fixture.mjs';

export function fixtureTar(entries) {
  const blocks = [];
  for (const [name, value] of Object.entries(entries)) {
    const bytes = Buffer.from(value), header = Buffer.alloc(512);
    header.write(name);
    header.write('0000755\0', 100); header.write('0000000\0', 108); header.write('0000000\0', 116);
    header.write(bytes.length.toString(8).padStart(11, '0') + '\0', 124);
    header.write('00000000000\0', 136); header.fill(32, 148, 156); header.write('0', 156);
    header.write('ustar\0', 257); header.write('00', 263);
    const checksum = header.reduce((sum, value) => sum + value, 0);
    header.write(checksum.toString(8).padStart(6, '0') + '\0 ', 148);
    blocks.push(header, bytes, Buffer.alloc((512 - bytes.length % 512) % 512));
  }
  return Buffer.concat([...blocks, Buffer.alloc(1024)]);
}

export function fixtureCpio(entries) {
  const blocks = [];
  for (const [name, value] of [...Object.entries(entries), ['TRAILER!!!', Buffer.alloc(0)]]) {
    const bytes = Buffer.from(value), path = Buffer.from(name + '\0');
    const fields = [1, 0o100755, 0, 0, 1, 0, bytes.length, 0, 0, 0, 0, path.length, 0];
    const header = Buffer.from('070701' + fields.map(value => value.toString(16).padStart(8, '0')).join(''));
    blocks.push(header, path, Buffer.alloc((4 - (header.length + path.length) % 4) % 4),
      bytes, Buffer.alloc((4 - bytes.length % 4) % 4));
  }
  return Buffer.concat(blocks);
}

export function fixtureXar(entries) {
  let offset = 0;
  const files = Object.entries(entries).map(([name, bytes], index) => {
    const xml = `<file id="${index + 1}"><name>${name}</name><type>file</type><data><length>${bytes.length}</length><offset>${offset}</offset><size>${bytes.length}</size><encoding style="application/octet-stream"/></data></file>`;
    offset += bytes.length;
    return xml;
  });
  const xml = Buffer.from(`<?xml version="1.0"?><xar><toc>${files.join('')}</toc></xar>`);
  const toc = deflateSync(xml), header = Buffer.alloc(28);
  header.write('xar!'); header.writeUInt16BE(28, 4); header.writeUInt16BE(1, 6);
  header.writeBigUInt64BE(BigInt(toc.length), 8); header.writeBigUInt64BE(BigInt(xml.length), 16);
  return Buffer.concat([header, toc, ...Object.values(entries)]);
}

function ar(entries) {
  const blocks = [Buffer.from('!<arch>\n')];
  for (const [name, bytes] of Object.entries(entries)) {
    blocks.push(Buffer.from(`${(name + '/').padEnd(16)}${'0'.padEnd(12)}${'0'.padEnd(6)}${'0'.padEnd(6)}${'100644'.padEnd(8)}${String(bytes.length).padEnd(10)}\x60\n`), bytes);
    if (bytes.length % 2) blocks.push(Buffer.from('\n'));
  }
  return Buffer.concat(blocks);
}

export const macosDesktopPayload = 'Library/Application Support/MixRelay/.installer-inputs/fixture/payload/Applications/MixRelay/MixRelay.app/Contents/MacOS/mixrelay-desktop';

export function installerFixtureBytes(format, payload, { extraEntries = {}, nested = false } = {}) {
  const marker = licensingBuildMarker(polarEnvironment);
  if (format === 'pkg') {
    const binary = payload ?? fatBinary([marker, marker]);
    const contents = gzipSync(fixtureCpio({ [macosDesktopPayload]: binary, ...extraEntries }));
    // productbuild stores expanded components; also exercise embedded component archives.
    if (nested) return fixtureXar({ 'MixRelay.component.pkg': fixtureXar({ Payload: contents }) });
    return fixtureXar({ 'MixRelay.component.pkg/Payload': contents });
  }
  const data = fixtureTar({ 'usr/bin/mixrelay-desktop': payload ?? marker, ...extraEntries });
  if (format === 'arch') return zstdCompressSync(data);
  if (format === 'deb') return ar({ 'debian-binary': Buffer.from('2.0\n'),
    'control.tar.gz': gzipSync(fixtureTar({ control: 'Package: mix-relay\nVersion: 1.2.3\nArchitecture: amd64\n' })),
    'data.tar.gz': gzipSync(data) });
  return Buffer.from('Synthetic Stream Deck fixture');
}

export function writeInstallerFixture(path, format, payload, options) {
  writeFileSync(path, installerFixtureBytes(format, payload, options));
}
