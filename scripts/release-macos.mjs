#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { checkVersion } from './product-version.mjs';
import { checkMacosSigning, withMacosSigning } from './macos-signing.mjs';
import { preparePayload } from './prepare-macos-release-payload.mjs';
import { buildPackages } from './build-macos-installer-pkgs.mjs';

const root = fileURLToPath(new URL('..', import.meta.url));
function run(program, args, env) {
  const result = spawnSync(program, args, { cwd: root, env, stdio: 'inherit' });
  if (result.error || result.status !== 0) throw new Error(`${program} failed (${result.status ?? result.error?.message})`);
}
try {
  const { values } = parseArgs({ options: { help: { type: 'boolean' }, output: { type: 'string' }, 'from-keychain': { type: 'boolean' }, 'package-only': { type: 'boolean' } } });
  if (values.help) {
    console.log('pnpm release:macos --output EMPTY_DIRECTORY [--from-keychain] [--package-only]\nUnlocks the restored private build keychain, verifies signing, and builds the universal app, HAL and one signed MixRelay PKG.\n--package-only reuses compiled signed artifacts and prepares a fresh payload. Does not install, publish or notarize.\nMIXRELAY_SIGNING_CONFIG selects another private restored signing configuration.');
  } else {
    if (process.platform !== 'darwin' || !values.output) throw new Error('macOS and --output are required');
    const version = checkVersion();
    const output = resolve(values.output);
    if (existsSync(output) && readdirSync(output).length) throw new Error('Output directory must be empty');
    checkMacosSigning();
    withMacosSigning((signingEnvironment) => {
    let application = signingEnvironment.APPLE_SIGNING_IDENTITY;
    let installer = signingEnvironment.MIXRELAY_INSTALLER_IDENTITY;
    if (values['from-keychain']) {
      const result = spawnSync('/usr/bin/security', ['find-identity', '-v'], { encoding: 'utf8', timeout: 30000 });
      if (result.status !== 0) throw new Error('Cannot read signing identities');
      const names = [...result.stdout.matchAll(/"(Developer ID (?:Application|Installer): [^"\r\n]+)"/g)].map(match => match[1]);
      const pairs = names.filter(name => name.startsWith('Developer ID Application: ')).flatMap(app => names.filter(name => name.startsWith('Developer ID Installer: ') && name.match(/\(([A-Z0-9]{10})\)$/)?.[1] === app.match(/\(([A-Z0-9]{10})\)$/)?.[1]).map(pkg => [app, pkg]));
      if (pairs.length !== 1) throw new Error('Specify signing identities: the keychain does not contain one unambiguous matching pair');
      [application, installer] = pairs[0];
    }
    const teamId = application?.match(/^Developer ID Application: [^\r\n]+ \(([A-Z0-9]{10})\)$/)?.[1];
    if (!teamId || !installer?.startsWith('Developer ID Installer: ') || !installer.endsWith(`(${teamId})`)) throw new Error('Matching Developer ID Application/Installer identities are required');
    const env = { ...signingEnvironment, APPLE_SIGNING_IDENTITY: application, MIXRELAY_CODESIGN_IDENTITY: application };
    const release = join(root, 'target/universal-apple-darwin/release');
    const bundles = join(release, 'bundle/macos');
    if (!values['package-only']) {
      run('zsh', ['scripts/build-macos-universal-app.sh'], env);
      run('cargo', ['run', '-p', 'mixrelay-hal-bundle', '--', '--output', join(release, 'hal')], env);
    }
    const staging = join(root, 'target/product-release', version);
    mkdirSync(staging, { recursive: true });
    const temporary = mkdtempSync(join(staging, '.package-'));
    const payload = join(temporary, 'payload');
    try {
      preparePayload({ manager: join(bundles, 'MixRelay.app'), hal: join(release, 'hal/MixRelayHAL.driver'), driver: join(release, 'mixrelay-driver-service'), engine: join(release, 'mixrelay-engine-service'), uninstaller: join(bundles, 'Uninstall MixRelay.app'), output: payload });
      console.log(JSON.stringify(buildPackages({ payload, installerCli: join(release, 'mixrelay-macos-installer'), payloadChecker: join(release, 'mixrelay-macos-payload-check'), teamId, version, signingIdentity: installer, output })));
    } finally {
      rmSync(temporary, { recursive: true, force: true });
    }
    });
  }
} catch (error) { console.error(error.message); process.exitCode = 1; }
