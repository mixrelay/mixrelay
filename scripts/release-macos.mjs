#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { checkVersion } from './product-version.mjs';
import { checkMacosSigning, readMacosSigningConfig, withMacosSigning } from './macos-signing.mjs';
import { checkPackageOutput } from './build-macos-installer-pkgs.mjs';
import { checkNotarizationCredentials, notarizationArguments } from './macos-notarization.mjs';

const root = fileURLToPath(new URL('..', import.meta.url));
/** A failure whose message the owned child already printed. */
class ReportedFailure extends Error {}
/** Compile through the signing wrapper's owned runner so an interruption can stop it. */
async function run(owned, program, args, env) {
  const result = await owned(program, args, { cwd: root, env, stdio: 'inherit' });
  if (result.error || result.status !== 0) throw new Error(`${program} failed (${result.signal ?? result.status ?? result.error?.message})`);
}
try {
  const { values } = parseArgs({ options: { help: { type: 'boolean' }, output: { type: 'string' }, 'from-keychain': { type: 'boolean' }, 'package-only': { type: 'boolean' }, 'skip-notarization': { type: 'boolean' } } });
  if (values.help) {
    console.log('pnpm release:macos --output EMPTY_DIRECTORY [--from-keychain] [--package-only] [--skip-notarization]\nUnlocks the restored private build keychain, verifies signing, builds the universal app, HAL and one signed MixRelay PKG, then notarizes, staples and validates the PKG.\n--package-only reuses compiled signed artifacts and prepares a fresh payload. --skip-notarization produces a signed local test build marked not notarized. Does not install or publish.\nMIXRELAY_SIGNING_CONFIG selects another private restored signing configuration, including notarization credential references.');
  } else {
    if (process.platform !== 'darwin' || !values.output) throw new Error('macOS and --output are required');
    const version = checkVersion();
    const output = resolve(values.output);
    // The PKG step's output rules apply before any compile or signing work starts.
    checkPackageOutput(output);
    const skipNotarization = values['skip-notarization'] ?? false;
    const notarization = skipNotarization ? undefined : readMacosSigningConfig().notarization;
    checkNotarizationCredentials(notarizationArguments(notarization, skipNotarization));
    checkMacosSigning();
    await withMacosSigning(async (signingEnvironment, _config, owned) => {
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
      await run(owned, 'zsh', ['scripts/build-macos-universal-app.sh'], env);
      await run(owned, 'cargo', ['run', '-p', 'mixrelay-hal-bundle', '--', '--output', join(release, 'hal')], env);
    }
    const staging = join(root, 'target/product-release', version);
    mkdirSync(staging, { recursive: true });
    const temporary = mkdtempSync(join(staging, '.package-'));
    const payload = join(temporary, 'payload');
    try {
      // Packaging runs spawnSync commands for up to 30 minutes each, so it runs as an owned
      // child that an interruption can stop. It prints the same result or error as before.
      const stage = {
        payload: { manager: join(bundles, 'MixRelay.app'), hal: join(release, 'hal/MixRelayHAL.driver'), driver: join(release, 'mixrelay-driver-service'), engine: join(release, 'mixrelay-engine-service'), uninstaller: join(bundles, 'Uninstall MixRelay.app'), output: payload },
        packages: { payload, installerCli: join(release, 'mixrelay-macos-installer'), payloadChecker: join(release, 'mixrelay-macos-payload-check'), teamId, version, signingIdentity: installer, output, notarization, skipNotarization },
      };
      const packaged = await owned(process.execPath, [join(root, 'scripts/package-macos-release.mjs'), JSON.stringify(stage)], { cwd: root, env: process.env, stdio: 'inherit' });
      if (packaged.status === 1) throw new ReportedFailure();
      if (packaged.error || packaged.status !== 0) throw new Error(`packaging failed (${packaged.signal ?? packaged.status ?? packaged.error?.message})`);
    } finally {
      rmSync(temporary, { recursive: true, force: true });
    }
    });
  }
} catch (error) {
  if (!(error instanceof ReportedFailure)) console.error(error.message);
  process.exitCode = 1;
}
