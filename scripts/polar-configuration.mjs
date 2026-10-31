import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

export const POLAR_CONFIGURATION_KEYS = [
  'MIXRELAY_POLAR_ORGANIZATION_ID',
  'MIXRELAY_POLAR_BENEFIT_ID',
  'MIXRELAY_POLAR_CHECKOUT_URL',
];
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const NIL = '00000000-0000-0000-0000-000000000000';
const PREFIX = 'MIXRELAY_POLAR_V1|';
const SUFFIX = '|END_POLAR';

export function validateLicensingBuildEnvironment(env, { required = false, release = false } = {}) {
  const values = POLAR_CONFIGURATION_KEYS.map(key => env[key]);
  const sandbox = env.MIXRELAY_POLAR_SANDBOX;
  if (sandbox !== undefined && sandbox !== '1') throw new Error('MIXRELAY_POLAR_SANDBOX must be unset or 1');
  if (release && sandbox !== undefined) throw new Error('Polar sandbox is forbidden in release builds');
  if (values.every(value => value === undefined) && sandbox === undefined) {
    if (required) throw new Error('Release requires valid MIXRELAY_POLAR_* configuration');
    return undefined;
  }
  if (values.some(value => typeof value !== 'string' || !value.trim())) {
    throw new Error('MIXRELAY_POLAR_* build configuration must be all unset or all non-empty');
  }
  const [organization, benefit, checkout] = values;
  if (![organization, benefit].every(value => value.length === 36 && UUID.test(value) && value !== NIL)) {
    throw new Error('Polar organisation and benefit IDs must be canonical non-nil UUIDs');
  }
  // Permit only persistent checkout links, without customer data or redirect parameters.
  const pattern = sandbox === '1'
    ? /^https:\/\/sandbox-api\.polar\.sh\/v1\/checkout-links\/polar_cl_[A-Za-z0-9_-]+\/redirect$/
    : /^https:\/\/buy\.polar\.sh\/polar_cl_[A-Za-z0-9_-]+$/;
  if (!pattern.test(checkout) || /[\r\n]/.test(checkout)) throw new Error('Polar checkout URL must be a persistent link on the Polar checkout host');
  return { organization, benefit, checkout, sandbox: sandbox === '1' };
}

export function licensingBuildMarker(env, options = {}) {
  const config = validateLicensingBuildEnvironment(env, options);
  return config ? `${PREFIX}${config.organization}|${config.benefit}|${config.checkout}|${config.sandbox ? 'sandbox' : 'production'}${SUFFIX}` : '';
}

/** Read public payment values from the existing private release configuration. */
export function releaseLicensingEnvironment(env, config) {
  const polar = config.polar ?? {};
  const configured = {
    MIXRELAY_POLAR_ORGANIZATION_ID: polar.organizationId,
    MIXRELAY_POLAR_BENEFIT_ID: polar.benefitId,
    MIXRELAY_POLAR_CHECKOUT_URL: polar.checkoutUrl,
  };
  for (const key of POLAR_CONFIGURATION_KEYS) {
    if (env[key] !== undefined && configured[key] !== undefined && env[key] !== configured[key]) {
      throw new Error(`Private release configuration conflicts with ${key}`);
    }
  }
  const result = { ...env };
  for (const key of POLAR_CONFIGURATION_KEYS) if (configured[key] !== undefined) result[key] = configured[key];
  validateLicensingBuildEnvironment(result, { required: true, release: true });
  return result;
}

/** Verify the exact configuration used by executable runtime code, not a sidecar file. */
function assertSliceLicensing(bytes, expectedEnv) {
  const configurations = new Map();
  for (let start = bytes.indexOf(PREFIX); start >= 0; start = bytes.indexOf(PREFIX, start + PREFIX.length)) {
    const end = bytes.indexOf(SUFFIX, start);
    if (end < 0 || end - start > 2048) continue;
    const marker = bytes.subarray(start, end + SUFFIX.length).toString('utf8');
    const fields = marker.slice(PREFIX.length, -SUFFIX.length).split('|');
    // The parser's own prefix/suffix literals can also occur in executable strings.
    if (fields.length !== 4 || !UUID.test(fields[0]) || !UUID.test(fields[1])) continue;
    const [organization, benefit, checkout, environment] = fields;
    if (environment !== 'production') throw new Error('Candidate app must use Polar production');
    const actual = {
      MIXRELAY_POLAR_ORGANIZATION_ID: organization,
      MIXRELAY_POLAR_BENEFIT_ID: benefit,
      MIXRELAY_POLAR_CHECKOUT_URL: checkout,
    };
    validateLicensingBuildEnvironment(actual, { required: true, release: true });
    configurations.set(marker, actual);
  }
  if (configurations.size === 0) throw new Error('Built app is missing Polar payment configuration');
  if (configurations.size !== 1) throw new Error('Built app contains conflicting Polar payment configuration');
  const [[marker, actual]] = configurations;
  if (expectedEnv && marker !== licensingBuildMarker(expectedEnv, {required:true,release:true})) {
    throw new Error('Built app Polar configuration differs from release configuration');
  }
  return actual;
}

function machoSlices(bytes) {
  if (bytes.length < 4) return [bytes];
  const magic = bytes.readUInt32BE(0);
  const littleEndian = magic === 0xbebafeca || magic === 0xbfbafeca;
  const wide = magic === 0xcafebabf || magic === 0xbfbafeca;
  if (![0xcafebabe, 0xbebafeca, 0xcafebabf, 0xbfbafeca].includes(magic)) return [bytes];
  const invalid = () => { throw new Error('Invalid Mach-O architecture table or slice'); };
  const read = offset => bytes[littleEndian ? 'readUInt32LE' : 'readUInt32BE'](offset);
  const stride = wide ? 32 : 20;
  if (bytes.length < 8) invalid();
  const count = read(4);
  if (!count || count > Math.floor((bytes.length - 8) / stride)) invalid();
  const tableEnd = 8 + count * stride;
  const ranges = [];
  const architectures = new Set();
  for (let index = 0; index < count; index++) {
    const entry = 8 + index * stride;
    const cpu = read(entry), subtype = read(entry + 4);
    const architecture = `${cpu}:${subtype}`;
    if (architectures.has(architecture)) invalid();
    architectures.add(architecture);
    const value = offset => wide
      ? bytes[littleEndian ? 'readBigUInt64LE' : 'readBigUInt64BE'](offset)
      : BigInt(read(offset));
    const offsetValue = value(entry + 8), sizeValue = value(entry + (wide ? 16 : 12));
    if (offsetValue > BigInt(bytes.length) || sizeValue > BigInt(bytes.length)) invalid();
    const offset = Number(offsetValue), size = Number(sizeValue), end = offset + size;
    const alignment = read(entry + (wide ? 24 : 16));
    if (alignment > (wide ? 63 : 31) || offsetValue % (1n << BigInt(alignment)) !== 0n
      || (wide && read(entry + 28) !== 0) || offset < tableEnd || size < 28 || end > bytes.length
      || ranges.some(range => offset < range.end && end > range.offset)) invalid();
    const slice = bytes.subarray(offset, end);
    const sliceMagic = slice.readUInt32BE(0);
    const sliceLittleEndian = sliceMagic === 0xcefaedfe || sliceMagic === 0xcffaedfe;
    if (![0xfeedface, 0xfeedfacf, 0xcefaedfe, 0xcffaedfe].includes(sliceMagic)) invalid();
    const sliceRead = at => slice[sliceLittleEndian ? 'readUInt32LE' : 'readUInt32BE'](at);
    const headerSize = sliceMagic === 0xfeedfacf || sliceMagic === 0xcffaedfe ? 32 : 28;
    if (size < headerSize || sliceRead(4) !== cpu || sliceRead(8) !== subtype
      || sliceRead(20) > size - headerSize) invalid();
    ranges.push({ offset, end, slice });
  }
  return ranges.map(range => range.slice);
}

export function assertBinaryLicensing(bytes, expectedEnv) {
  let actual;
  for (const slice of machoSlices(bytes)) {
    const configured = assertSliceLicensing(slice, expectedEnv ?? actual);
    actual ??= configured;
  }
  return actual;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const [command, binary] = process.argv.slice(2);
    if (command === 'check-release') validateLicensingBuildEnvironment(process.env, {required:true,release:true});
    else if (command === 'build-marker') {
      process.stdout.write(licensingBuildMarker(process.env, {
        required: process.env.PROFILE === 'release' && process.env.MIXRELAY_DESKTOP_CODE_CHECK !== '1',
        release: process.env.PROFILE === 'release',
      }));
    } else if (command === 'check-binary' && binary) assertBinaryLicensing(readFileSync(binary), process.env);
    else throw new Error('Expected check-release, build-marker or check-binary EXECUTABLE');
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
