import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { POLAR_CONFIGURATION_KEYS, assertBinaryLicensing, licensingBuildMarker,
  releaseLicensingEnvironment, validateLicensingBuildEnvironment } from './polar-configuration.mjs';
import { polarEnvironment } from './polar-configuration.fixture.mjs';

const sandbox = { ...polarEnvironment, MIXRELAY_POLAR_SANDBOX:'1',
  MIXRELAY_POLAR_CHECKOUT_URL:'https://sandbox-api.polar.sh/v1/checkout-links/polar_cl_fixture/redirect' };

test('development is wholly unconfigured or bound to canonical organisation, benefit and checkout', () => {
  assert.equal(validateLicensingBuildEnvironment({}),undefined);
  assert.equal(validateLicensingBuildEnvironment(polarEnvironment).sandbox,false);
  for (const key of POLAR_CONFIGURATION_KEYS) {
    for (const value of [undefined,'',' ']) {
      assert.throws(() => validateLicensingBuildEnvironment({...polarEnvironment,[key]:value}),/all unset or all non-empty/);
    }
  }
  for (const key of POLAR_CONFIGURATION_KEYS.slice(0,2)) {
    for (const value of ['1','00000000-0000-0000-0000-000000000000','10000000000040008000000000000001']) {
      assert.throws(() => validateLicensingBuildEnvironment({...polarEnvironment,[key]:value}),/UUID/);
    }
  }
});

test('checkout host and persistent link format cannot be spoofed or carry customer data', () => {
  for (const url of ['http://buy.polar.sh/polar_cl_a','https://buy.polar.sh.evil.invalid/polar_cl_a',
    'https://user:secret@buy.polar.sh/polar_cl_a','https://buy.polar.sh:443/polar_cl_a',
    'https://buy.polar.sh/polar_cl_','https://buy.polar.sh/polar_cl_a?customer_email=private',
    'https://buy.polar.sh/polar_cl_a#secret','https://polar.sh/checkout/a',
    'https://buy.polar.sh/polar_cl_a/../b','https://buy.polar.sh/polar_cl_%61',
    ' https://buy.polar.sh/polar_cl_a','https://buy.polar.sh/polar_cl_a\n']) {
    assert.throws(() => validateLicensingBuildEnvironment({...polarEnvironment,MIXRELAY_POLAR_CHECKOUT_URL:url}),/checkout/);
  }
});

test('sandbox is explicit, isolated and forbidden for every release', () => {
  assert.equal(validateLicensingBuildEnvironment(sandbox).sandbox,true);
  for (const env of [sandbox,{...polarEnvironment,MIXRELAY_POLAR_SANDBOX:'0'},
    {...polarEnvironment,MIXRELAY_POLAR_SANDBOX:''},{MIXRELAY_POLAR_SANDBOX:'1'}]) {
    assert.throws(() => validateLicensingBuildEnvironment(env,{required:true,release:true}));
  }
  assert.throws(() => validateLicensingBuildEnvironment({...sandbox,MIXRELAY_POLAR_SANDBOX:undefined}),/checkout/);
});

test('private release configuration supplies the exact public build environment and refuses conflicts', () => {
  const config = {polar:{organizationId:polarEnvironment.MIXRELAY_POLAR_ORGANIZATION_ID,
    benefitId:polarEnvironment.MIXRELAY_POLAR_BENEFIT_ID,checkoutUrl:polarEnvironment.MIXRELAY_POLAR_CHECKOUT_URL}};
  assert.deepEqual(releaseLicensingEnvironment({PATH:'fixture'},config),{PATH:'fixture',...polarEnvironment});
  assert.throws(() => releaseLicensingEnvironment({},{}),/Release requires/);
  assert.throws(() => releaseLicensingEnvironment({MIXRELAY_POLAR_BENEFIT_ID:'conflict'},config),/conflicts/);
  assert.throws(() => releaseLicensingEnvironment(sandbox,config),/conflicts|sandbox/);
  const source=readFileSync(new URL('./release-macos.mjs',import.meta.url),'utf8');
  assert.ok(source.indexOf('releaseLicensingEnvironment(process.env, releaseConfig)') < source.indexOf('    checkMacosSigning();'));
});

test('release and Cargo build preflights fail without configuration before invoking any build or signing tool', () => {
  const script = fileURLToPath(new URL('./polar-configuration.mjs',import.meta.url));
  const clean = {...process.env};
  for (const key of [...POLAR_CONFIGURATION_KEYS,'MIXRELAY_POLAR_SANDBOX','MIXRELAY_DESKTOP_CODE_CHECK']) delete clean[key];
  for (const [command,env,expected] of [
    ['check-release',clean,1],['build-marker',{...clean,PROFILE:'release'},1],
    ['build-marker',{...clean,PROFILE:'debug'},0],
    ['build-marker',{...clean,PROFILE:'release',MIXRELAY_DESKTOP_CODE_CHECK:'1'},0],
    ['check-release',{...clean,...sandbox},1],['check-release',{...clean,...polarEnvironment},0],
  ]) {
    const result=spawnSync(process.execPath,[script,command],{encoding:'utf8',env});
    assert.equal(result.status,expected,result.stderr);
  }
});

test('candidate checks read the production configuration from actual executable bytes', () => {
  const marker=licensingBuildMarker(polarEnvironment);
  const binary=Buffer.concat([Buffer.from([0,255,0]),Buffer.from(marker),Buffer.from([0,13])]);
  assert.deepEqual(assertBinaryLicensing(binary,polarEnvironment),polarEnvironment);
  const parserLiterals=Buffer.from('MIXRELAY_POLAR_V1|\0|END_POLAR\0');
  assert.deepEqual(assertBinaryLicensing(Buffer.concat([parserLiterals,binary,parserLiterals,binary]),polarEnvironment),polarEnvironment);
  const wrongArchitecture=Buffer.from(marker.replace('polar_cl_fixture','polar_cl_other_architecture'));
  assert.throws(() => assertBinaryLicensing(Buffer.concat([binary,wrongArchitecture]),polarEnvironment), /conflicting/);
  for (const bytes of [Buffer.from('unconfigured app'), Buffer.from(marker.replace('|END_POLAR','')),
    Buffer.from(licensingBuildMarker(sandbox)),Buffer.from(marker.replace('polar_cl_fixture','polar_cl_wrong'))]) {
    assert.throws(() => assertBinaryLicensing(bytes,polarEnvironment),/configuration|production/);
  }
});

test('Linux release CI passes repository Actions variables into the actual build container', () => {
  const workflow=readFileSync(new URL('../.github/workflows/build-linux-release.yml',import.meta.url),'utf8');
  for (const key of POLAR_CONFIGURATION_KEYS) {
    assert.ok(workflow.includes(`${key}: \u0024{{ vars.${key} }}`));
    assert.ok(workflow.includes(`-e ${key}`));
  }
});

test('every Mach-O architecture requires its own matching production marker', async () => {
  const { fatBinary } = await import('./polar-configuration.fixture.mjs');
  const marker = licensingBuildMarker(polarEnvironment);
  for (const wide of [false, true]) for (const littleEndian of [false, true]) {
    const options = { wide, littleEndian };
    assert.deepEqual(assertBinaryLicensing(fatBinary([marker, marker], options), polarEnvironment), polarEnvironment);
    assert.deepEqual(assertBinaryLicensing(fatBinary([marker, marker], options)), polarEnvironment);
    for (const payloads of [[marker, 'unconfigured Intel'], ['unconfigured ARM', marker],
      [marker.slice(0, -5), marker.slice(-5)], [marker, marker.replace('polar_cl_fixture', 'polar_cl_other')]]) {
      assert.throws(() => assertBinaryLicensing(fatBinary(payloads, options), polarEnvironment), /configuration/);
    }
    assert.throws(() => assertBinaryLicensing(fatBinary([marker, marker.replace('polar_cl_fixture', 'polar_cl_other')], options)), /configuration/);
    const truncated = fatBinary([marker, marker.slice(0, -5)], options);
    assert.throws(() => assertBinaryLicensing(Buffer.concat([truncated, Buffer.from(marker.slice(-5))]), polarEnvironment), /missing/);
  }
});

test('invalid fat architecture tables cannot hide missing slices', async () => {
  const { fatBinary } = await import('./polar-configuration.fixture.mjs');
  const marker = licensingBuildMarker(polarEnvironment);
  for (const mutate of [
    bytes => bytes.subarray(0, 20),
    bytes => { bytes.writeUInt32BE(0, 4); return bytes; },
    bytes => { bytes.writeUInt32BE(0xffffffff, 4); return bytes; },
    bytes => { bytes.writeUInt32BE(0xffffffff, 36); return bytes; },
    bytes => { bytes.writeUInt32BE(48, 36); return bytes; },
    bytes => { bytes.writeUInt32BE(0, 40); return bytes; },
    bytes => { bytes.writeUInt32BE(0, 48); return bytes; },
    bytes => { bytes.writeUInt32BE(0x01000007, 8); return bytes; },
  ]) assert.throws(() => assertBinaryLicensing(mutate(fatBinary([marker, marker])), polarEnvironment), /Mach-O/);
});
