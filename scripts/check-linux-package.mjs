import { spawn, spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { loadContract } from './support-contract.mjs';
import { productContact } from './product-metadata.mjs';

const languages = JSON.parse(readFileSync(new URL("../packaging/localization/languages.json", import.meta.url), "utf8"));
const localizedDocuments = languages.flatMap(language => [language.appTerms, language.driverTerms]);

const requiredDependencies = [
  ['libpipewire-0.3-0t64'], ['liblilv-0-0'], ['libgcc-s1'], ['libc6'],
  ['libwebkit2gtk-4.1-0'], ['libgtk-3-0', 'libgtk-3-0t64'],
  ['librsvg2-common'],
  ['pipewire'], ['wireplumber'], ['gnome-keyring'], ['libpam-gnome-keyring'],
  ['libayatana-appindicator3-1', 'libappindicator3-1'],
  ['zenity'], ['pkexec'], ['polkit-1-auth-agent', 'lxpolkit'],
  ['systemd'], ['util-linux'],
];
// Async contributions need PipeWire 1.2; source muting needs WirePlumber 0.5.
const requiredAudioDependencies = [
  ...['libpipewire-0.3-0t64', 'pipewire'].map(name => [name, loadContract().contract.minimums.linux.pipewire.split('.').map(Number)]),
  ['wireplumber', loadContract().contract.minimums.linux.wireplumber.split('.').map(Number)],
];

// Release dependency floors use plain upstream versions, not distro epochs/revisions.
function satisfiesAudioMinimum(term, name, minimum) {
  const match = term.trim().match(/^([a-z0-9][a-z0-9+.-]*)\s+\((?:>=|>>|=)\s*(\d+)\.(\d+)\.(\d+)\)$/);
  if (!match || match[1] !== name) return false;
  const version = match.slice(2).map(Number);
  if (!version.every(Number.isSafeInteger)) return false;
  for (let index = 0; index < minimum.length; index++) {
    if (version[index] !== minimum[index]) return version[index] > minimum[index];
  }
  return true;
}
const executables = ['usr/bin/mixrelay-desktop', 'usr/bin/mixrelay-engine-service', 'usr/bin/mixrelay-driver-service'];
const wirePlumberFiles = [
  'usr/share/wireplumber/scripts/mixrelay-source-mute.lua',
  'usr/share/wireplumber/wireplumber.conf.d/90-mixrelay-source-mute.conf',
];
const uninstallFiles = [
  'usr/lib/mixrelay/uninstall.sh',
  'usr/share/applications/mixrelay-uninstall.desktop',
];
const VERSION_RECEIPT = 'usr/share/mixrelay-driver/version.json';
const dataFiles = [
  'usr/share/doc/mix-relay/CHANGELOG.md',
  ...localizedDocuments.map(filename => `usr/share/doc/mix-relay/${filename}`),
  'usr/lib/systemd/user/mixrelay-driver.service',
  'etc/xdg/autostart/mixrelay-driver.desktop',
  'usr/lib/mixrelay-driver/session-start.sh',
  'usr/lib/mixrelay-driver/before-change.sh',
  VERSION_RECEIPT,
  'usr/share/doc/mix-relay-driver/CHANGELOG.md',
  ...localizedDocuments.map(filename => `usr/share/doc/mix-relay-driver/${filename}`),
  'usr/share/doc/mix-relay-driver/third-party/truce-rack/LICENSE-MIT',
  'usr/share/doc/mix-relay-driver/third-party/truce-rack/LICENSE-APACHE',
];
const lifecycleHooks = {
  preinst: 'driver-before-change.sh', prerm: 'driver-pre-remove.sh',
  postinst: 'driver-after-install.sh', postrm: 'driver-after-remove.sh',
};
const MAX_LISTING_BYTES = 4 * 1024 * 1024;
const MAX_PACKAGE_CONTENT_BYTES = 1024 * 1024 * 1024;
const MAX_LISTING_ENTRIES = 50_000;
const MAX_TAR_STREAM_BYTES = MAX_PACKAGE_CONTENT_BYTES + MAX_LISTING_ENTRIES * 1024 + 1024;
const MAX_EXECUTABLE_BYTES = 256 * 1024 * 1024;
const MAX_ELF_OUTPUT_BYTES = 3 * MAX_EXECUTABLE_BYTES;
const MAX_STDERR_BYTES = 16 * 1024;
const PROCESS_TIMEOUT_MS = 120_000;

const expectedElf = {
  amd64: { class: 2, machine: 62, name: 'x86-64' },
  arm64: { class: 2, machine: 183, name: 'AArch64' },
};

function inspectionEnvironment() {
  const env = { ...process.env, LC_ALL: 'C', LANG: 'C' };
  // GNU tar reads options from TAR_OPTIONS before explicit argv, including
  // checkpoint actions that can execute arbitrary commands. Listing uses a
  // separate environment that opts into numeric ownership explicitly.
  delete env.TAR_OPTIONS;
  return env;
}

function packageMetadata({ architecture, depends, contents, packageName, homepage, maintainer }) {
  if (packageName !== 'mix-relay') throw new Error(`Unsupported package: ${packageName}`);
  const { metadata } = loadContract().contract;
  if (homepage?.trim() !== metadata.homepage || maintainer?.trim() !== productContact(metadata)) {
    throw new Error('Package Homepage and Maintainer must match the public product metadata');
  }
  const normalizedArchitecture = architecture.trim();
  if (!Object.hasOwn(expectedElf, normalizedArchitecture)) {
    throw new Error(`Unsupported package architecture: ${normalizedArchitecture}`);
  }
  // A required library must occur in every alternative of at least one clause:
  // "required | unrelated" does not guarantee the runtime library is installed.
  const parseDependencyTerm = term => {
    const normalized = term.trim();
    if (/^[a-z0-9][a-z0-9+.-]*:[a-z0-9-]+(?:\s|$)/.test(normalized)
      || /\[[^\]]*\]|(?:^|\s)<[^>]*>/.test(normalized)) {
      throw new Error(`Unsupported dependency qualifier or restriction: ${normalized}`);
    }
    return normalized.match(/^([a-z0-9][a-z0-9+.-]*)(?:\s+\([^)]*\))?$/)?.[1];
  };
  const clauses = depends.split(',').map(clause => clause.split('|').map(parseDependencyTerm));
  for (const alternatives of requiredDependencies) {
    if (!clauses.some(clause => clause.length && clause.every(name => alternatives.includes(name)))) {
      throw new Error(`Missing required dependency: ${alternatives.join(' or ')}`);
    }
  }
  for (const [name, minimum] of requiredAudioDependencies) {
    if (!depends.split(',').some(clause => clause.split('|').every(term => satisfiesAudioMinimum(term, name, minimum)))) {
      throw new Error(`Missing required dependency: ${name} (>= ${minimum.join('.')})`);
    }
  }
  const files = new Map();
  let listingEntries = 0;
  let totalRegularBytes = 0;
  for (const line of contents.split('\n')) {
    if (!line) continue;
    const match = line.match(/^([-dl][rwxstST-]{9})\s+(\S+)\s+(\d+)\s+\S+\s+\S+\s+(.+)$/);
    if (!match) throw new Error(`Unrecognized Debian payload listing entry: ${line.slice(0, 200)}`);
    listingEntries += 1;
    if (listingEntries > MAX_LISTING_ENTRIES) throw new Error('Debian payload has too many entries');
    const size = Number(match[3]);
    if (!Number.isSafeInteger(size) || size < 0) throw new Error('Invalid Debian payload entry size');
    const displayedPath = match[4].split(' -> ', 1)[0];
    const path = displayedPath.replace(/^(?:\.\/)+/, '').replace(/\/+$/, '');
    if (!path) continue;
    if (path.startsWith('/') || path.includes('\0') || path.split('/').some(part => part === '.' || part === '..')) {
      throw new Error('Unsafe payload path');
    }
    if (files.has(path)) throw new Error(`Duplicate payload path: ${path}`);
    const entry = { mode: match[1], owner: match[2], size, memberPath: displayedPath };
    files.set(path, entry);
    if (entry.mode[0] === '-') {
      totalRegularBytes += size;
      if (!Number.isSafeInteger(totalRegularBytes) || totalRegularBytes > MAX_PACKAGE_CONTENT_BYTES) {
        throw new Error('Debian package payload exceeds the bounded inspection size');
      }
    }
  }
  const executableEntries = [];
  for (const path of executables) {
    const file = files.get(path);
    if (!file || file.size === 0 || file.mode[0] !== '-') {
      throw new Error(`Missing non-empty regular executable payload: ${path}`);
    }
    if (file.owner !== '0/0' || file.mode !== '-rwxr-xr-x') {
      throw new Error(`Invalid Debian executable ownership or permissions: ${path}`);
    }
    if (file.size > MAX_EXECUTABLE_BYTES) {
      throw new Error(`Executable payload exceeds the bounded inspection size: ${path}`);
    }
    executableEntries.push({ path, size: file.size, memberPath: file.memberPath });
  }
  for (const path of wirePlumberFiles) {
    const file = files.get(path);
    if (!file || file.size === 0 || file.mode[0] !== '-') {
      throw new Error(`Missing non-empty regular WirePlumber payload: ${path}`);
    }
    if (file.owner !== '0/0' || file.mode !== '-rw-r--r--') {
      throw new Error(`Invalid WirePlumber payload ownership or permissions: ${path}`);
    }
  }
  for (const path of uninstallFiles) {
    const file = files.get(path);
    if (!file || file.size === 0 || file.mode !== '-rw-r--r--' || file.owner !== '0/0') {
      throw new Error(`Missing or invalid uninstall payload: ${path}`);
    }
  }
  for (const path of dataFiles) {
    const file = files.get(path);
    if (!file || file.size === 0 || file.mode !== '-rw-r--r--' || file.owner !== '0/0') {
      throw new Error(`Missing or invalid package documentation: ${path}`);
    }
  }
  return { architecture: normalizedArchitecture, files, executableEntries };
}

function elfHeaderArchitecture(header, path) {
  if (!(header instanceof Uint8Array) || header.byteLength < 64
    || header[0] !== 0x7f || header[1] !== 0x45 || header[2] !== 0x4c || header[3] !== 0x46) {
    throw new Error(`Payload is not a complete ELF64 executable: ${path}`);
  }
  if (header[4] !== 2 || header[5] !== 1 || header[6] !== 1) {
    throw new Error(`Payload has an unsupported ELF class, byte order, or version: ${path}`);
  }
  const type = header[16] | (header[17] << 8);
  const machine = header[18] | (header[19] << 8);
  const headerSize = header[52] | (header[53] << 8);
  if (![2, 3].includes(type) || headerSize < 64) {
    throw new Error(`Payload does not have a supported ELF executable header: ${path}`);
  }
  return { machine, name: machine === 62 ? 'x86-64' : machine === 183 ? 'AArch64' : `e_machine=${machine}` };
}

/** Checks metadata and ELF headers without installing or executing package payloads. */
export function validatePackage({ architecture, depends, contents, elfHeaders, packageName, version, versionReceipt, homepage, maintainer }) {
  const { architecture: normalizedArchitecture, executableEntries } = packageMetadata({ architecture, depends, contents, packageName, homepage, maintainer });
  const receipt = JSON.parse(versionReceipt ?? 'null');
  if (!receipt || !/^\d+\.\d+\.\d+$/.test(receipt.version) || receipt.version !== version?.replace(/-\d+$/, '')) {
    throw new Error('Driver version receipt must match the actual package version');
  }
  const expected = expectedElf[normalizedArchitecture];
  const elfArchitectures = {};
  for (const entry of executableEntries) {
    const header = elfHeaders?.[entry.path];
    if (!header) throw new Error(`Missing extracted ELF header: ${entry.path}`);
    const actual = elfHeaderArchitecture(header, entry.path);
    if (actual.machine !== expected.machine) {
      throw new Error(`Unexpected ELF architecture for ${entry.path}: expected ${expected.name}, found ${actual.name}`);
    }
    elfArchitectures[entry.path] = actual.name;
  }
  return {
    packageName,
    architecture: normalizedArchitecture,
    executables,
    elfArchitectures,
    scope: 'metadata-payload-elf-header-only',
  };
}

function cappedText(stream) {
  let captured = '';
  let length = 0;
  stream.setEncoding('utf8');
  stream.on('data', chunk => {
    if (length < MAX_STDERR_BYTES) captured += chunk.slice(0, MAX_STDERR_BYTES - length);
    length += Buffer.byteLength(chunk);
  });
  return () => captured;
}

function streamPackageTar(archive, tarArguments, spawnProcess, limits) {
  return new Promise((resolvePromise, rejectPromise) => {
    let dpkg;
    let tar;
    try {
      dpkg = spawnProcess('dpkg-deb', ['--fsys-tarfile', archive], {
        env: inspectionEnvironment(),
        stdio: ['ignore', 'pipe', 'pipe'],
      });
      tar = spawnProcess('tar', tarArguments, {
        env: inspectionEnvironment(),
        stdio: ['pipe', 'pipe', 'pipe'],
      });
    } catch (error) {
      dpkg?.kill('SIGKILL');
      tar?.kill('SIGKILL');
      rejectPromise(new Error(`Cannot start bounded Debian tar inspection: ${error.message}`));
      return;
    }
    const dpkgErrorText = cappedText(dpkg.stderr);
    const tarErrorText = cappedText(tar.stderr);
    const capturedOutput = Buffer.alloc(limits.captureBytes);
    let archiveBytes = 0;
    let outputBytes = 0;
    let capturedBytes = 0;
    let dpkgStatus;
    let tarStatus;
    let dpkgClosed = false;
    let tarClosed = false;
    let settled = false;
    let timer;

    const fail = error => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      dpkg.kill('SIGKILL');
      tar.kill('SIGKILL');
      rejectPromise(error);
    };
    const finishIfClosed = () => {
      if (settled || !dpkgClosed || !tarClosed) return;
      if (dpkgStatus !== 0) {
        fail(new Error(`dpkg-deb payload stream failed: ${dpkgErrorText() || `status ${dpkgStatus}`}`));
        return;
      }
      if (tarStatus !== 0) {
        fail(new Error(`tar package inspection failed: ${tarErrorText() || `status ${tarStatus}`}`));
        return;
      }
      if (limits.expectedOutputBytes !== undefined && outputBytes !== limits.expectedOutputBytes) {
        fail(new Error('Extracted executable byte count does not match Debian package metadata'));
        return;
      }
      settled = true;
      clearTimeout(timer);
      resolvePromise({ output: capturedOutput.subarray(0, capturedBytes), archiveBytes, outputBytes });
    };

    timer = setTimeout(() => fail(new Error('Bounded Debian tar inspection timed out')), PROCESS_TIMEOUT_MS);
    dpkg.on('error', error => fail(new Error(`Cannot start dpkg-deb payload stream: ${error.message}`)));
    tar.on('error', error => fail(new Error(`Cannot start tar package inspection: ${error.message}`)));
    dpkg.stdout.on('error', error => fail(new Error(`Cannot read dpkg-deb payload stream: ${error.message}`)));
    tar.stdout.on('error', error => fail(new Error(`Cannot read tar executable output: ${error.message}`)));
    dpkg.stdout.on('data', chunk => {
      archiveBytes += chunk.length;
      if (archiveBytes > MAX_TAR_STREAM_BYTES) fail(new Error('Debian uncompressed payload exceeds the bounded inspection size'));
    });
    tar.stdout.on('data', chunk => {
      const chunkStart = outputBytes;
      const chunkEnd = chunkStart + chunk.length;
      if (chunkEnd > limits.maxOutputBytes) {
        fail(new Error(limits.outputLimitMessage));
        return;
      }
      const capturedEnd = Math.min(capturedOutput.length, chunkEnd);
      if (chunkStart < capturedEnd) {
        chunk.copy(capturedOutput, chunkStart, 0, capturedEnd - chunkStart);
        capturedBytes = capturedEnd;
      }
      outputBytes = chunkEnd;
    });
    dpkg.on('close', (status, signal) => {
      dpkgStatus = status;
      dpkgClosed = true;
      if (signal && !settled) fail(new Error(`dpkg-deb payload stream stopped by ${signal}`));
      finishIfClosed();
    });
    tar.on('close', (status, signal) => {
      tarStatus = status;
      tarClosed = true;
      if (signal && !settled) fail(new Error(`tar package inspection stopped by ${signal}`));
      finishIfClosed();
    });
    tar.stdin.on('error', error => fail(new Error(`Cannot pipe Debian payload into tar: ${error.message}`)));
    dpkg.stdout.pipe(tar.stdin);
  });
}

function listNumericOwnedContents(archive, spawnProcess) {
  return streamPackageTar(archive, ['--numeric-owner', '-tvf', '-'], spawnProcess, {
    maxOutputBytes: MAX_LISTING_BYTES,
    captureBytes: MAX_LISTING_BYTES,
    outputLimitMessage: 'Debian payload listing exceeds the bounded inspection size',
  }).then(result => result.output.toString('utf8'));
}

async function extractOneElfHeader(archive, entry, spawnProcess) {
  const result = await streamPackageTar(
    archive,
    ['-xO', '-f', '-', '--', entry.memberPath],
    spawnProcess,
    {
      maxOutputBytes: entry.size,
      expectedOutputBytes: entry.size,
      captureBytes: Math.min(entry.size, 64),
      outputLimitMessage: 'tar returned more executable bytes than package metadata declared',
    },
  );
  return { header: result.output, archiveBytes: result.archiveBytes };
}

async function extractElfHeaders(archive, executableEntries, spawnProcess = spawn) {
  const outputBytesExpected = executableEntries.reduce((total, entry) => total + entry.size, 0);
  if (outputBytesExpected > MAX_ELF_OUTPUT_BYTES) {
    throw new Error('Required executable payloads exceed the bounded extraction size');
  }
  const headers = {};
  let archiveBytesTotal = 0;
  // One member per tar invocation makes its stdout unambiguously belong to that exact path;
  // tar's archive-order output for multiple operands is not assumed to match operand order.
  for (const entry of executableEntries) {
    const result = await extractOneElfHeader(archive, entry, spawnProcess);
    archiveBytesTotal += result.archiveBytes;
    if (archiveBytesTotal > MAX_TAR_STREAM_BYTES * executableEntries.length) {
      throw new Error('Repeated Debian payload streams exceed the bounded inspection size');
    }
    headers[entry.path] = result.header;
  }
  return headers;
}

export async function inspectPackage(path, run = spawnSync, spawnProcess = spawn) {
  const archive = resolve(path);
  const field = name => {
    // Field names follow the archive for dpkg-deb --field.
    const result = run('dpkg-deb', ['--field', archive, name], {
      encoding: 'utf8', env: inspectionEnvironment(), timeout: 30_000, maxBuffer: 64 * 1024,
    });
    if (result.error || result.status !== 0 || result.signal) throw new Error(`Cannot read package ${name}`);
    return result.stdout;
  };
  const packageDetails = {
    packageName: field('Package').trim(),
    version: field('Version').trim(),
    homepage: field('Homepage'),
    maintainer: field('Maintainer'),
    architecture: field('Architecture'),
    depends: field('Depends'),
    contents: await listNumericOwnedContents(archive, spawnProcess),
  };
  for (const [script, source] of Object.entries(lifecycleHooks)) {
    const expectedHook = readFileSync(new URL(`../packaging/linux/${source}`, import.meta.url), 'utf8');
    const result = run('dpkg-deb', ['--info', archive, script], {
      encoding: 'utf8', env: inspectionEnvironment(), timeout: 30_000, maxBuffer: 64 * 1024,
    });
    if (result.error || result.status !== 0 || result.signal || result.stdout !== expectedHook) {
      throw new Error(`Missing or mismatched package lifecycle hook: ${script}`);
    }
  }
  const { architecture, executableEntries, files } = packageMetadata(packageDetails);
  const elfHeaders = await extractElfHeaders(archive, executableEntries, spawnProcess);
  const receipt = files.get(VERSION_RECEIPT);
  if (receipt.size > 64) throw new Error('Driver version receipt is oversized');
  const extracted = await extractOneElfHeader(archive, receipt, spawnProcess);
  packageDetails.versionReceipt = extracted.header.toString('utf8');
  return validatePackage({
    ...packageDetails,
    architecture,
    elfHeaders,
  });
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    if (process.argv.length !== 3) throw new Error('Usage: node scripts/check-linux-package.mjs PACKAGE.deb');
    console.log(JSON.stringify(await inspectPackage(process.argv[2]), null, 2));
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
