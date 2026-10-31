import { spawnSync } from 'node:child_process';
import { assertBinaryLicensing } from './polar-configuration.mjs';

// Bound decompressed payloads and listings while inspecting archives only through stdout.
const MAX_PAYLOAD_BYTES = 1024 * 1024 * 1024;
const MAX_LISTING_BYTES = 8 * 1024 * 1024;
const MACOS_DESKTOP = /^(?:Library\/Application Support\/MixRelay\/\.installer-inputs\/[A-Za-z0-9-]+\/payload\/)?Applications\/MixRelay\/MixRelay\.app\/Contents\/MacOS\/mixrelay-desktop$/;

function archiveCommand(archive, args, maxBuffer) {
  const input = Buffer.isBuffer(archive) ? archive : undefined;
  const result = spawnSync('bsdtar', ['-f', input ? '-' : archive, ...args], {
    input, maxBuffer, timeout: 120000, env: { ...process.env, LC_ALL: 'C', TAR_OPTIONS: '' },
  });
  if (result.error || result.signal || result.status !== 0) {
    throw new Error(`Cannot inspect installer payload with bsdtar: ${result.error?.message ?? result.stderr?.toString().trim() ?? result.signal}`);
  }
  return result.stdout;
}

function entries(archive) {
  const names = archiveCommand(archive, ['-t'], MAX_LISTING_BYTES).toString('utf8').split('\n').filter(Boolean);
  const details = archiveCommand(archive, ['-tv'], MAX_LISTING_BYTES).toString('utf8').split('\n').filter(Boolean);
  if (names.length !== details.length) throw new Error('Invalid installer payload listing');
  return names.map((member, index) => {
    const path = member.replace(/^\.\//, '');
    if (path.startsWith('/') || path.split('/').includes('..') || /[\x00-\x1f\\*?\[\]]/.test(path)) {
      throw new Error('Unsafe installer payload member');
    }
    return { member, path, regular: details[index].startsWith('-') };
  });
}

function extract(archive, entry) {
  if (!entry.regular) throw new Error(`Installer payload is not a regular file: ${entry.path}`);
  // Member names were checked for wildcard characters before being used as operands.
  return archiveCommand(archive, ['-xO', '--no-recursion', '--', entry.member], MAX_PAYLOAD_BYTES);
}

function exactlyOne(items, description) {
  if (items.length !== 1) throw new Error(`Ambiguous or absent installer payload: ${description}`);
  return items[0];
}

export function assertInstallerLicensing(archive, format, expectedEnv) {
  if (format === 'streamDeckPlugin') return;
  const contents = entries(archive);
  let payloads;
  if (format === 'pkg') {
    const direct = contents.filter(entry => /(?:^|\/)Payload$/.test(entry.path));
    const components = contents.filter(entry => entry.path.endsWith('.pkg') && entry.regular);
    payloads = [
      ...direct.map(entry => extract(archive, entry)),
      ...components.map(component => {
        const bytes = extract(archive, component);
        const payload = exactlyOne(entries(bytes).filter(entry => entry.path === 'Payload'), 'PKG Payload');
        return extract(bytes, payload);
      }),
    ];
    if (!payloads.length) throw new Error('Absent PKG component payload');
  } else if (format === 'deb') {
    const data = exactlyOne(contents.filter(entry => /^data\.tar(?:\.(?:gz|xz|zst|bz2|lzma))?$/.test(entry.path)), 'DEB data');
    payloads = [extract(archive, data)];
  } else if (format === 'arch') payloads = [archive];
  else throw new Error(`Unsupported installer format: ${format}`);
  const desktops = payloads.flatMap(payload => entries(payload)
    .filter(entry => format === 'pkg' ? MACOS_DESKTOP.test(entry.path) : entry.path === 'usr/bin/mixrelay-desktop')
    .map(entry => ({ payload, entry })));
  const desktop = exactlyOne(desktops, 'desktop executable');
  return assertBinaryLicensing(extract(desktop.payload, desktop.entry), expectedEnv);
}
