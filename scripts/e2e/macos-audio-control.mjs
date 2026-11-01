import { MINIMUM_MACOS } from '../macos-minimum.mjs';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { launchCadenceControl, requireCadenceExecution, stopCadenceControl, waitCadenceReady } from './macos-cadence-control.mjs';

// BlackHole v0.7.1, built without source edits as an independent host control.
const revision = 'e2b22aaaba4e507a097131704bf96dabc004d9cf';
const driver = '/Library/Audio/Plug-Ins/HAL/BlackHole.driver';

export function requireRemovedCandidate(report) {
  assert.equal(report.cleanup, 'passed', 'Product removal must finish before the independent control');
  assert.ok(report.scenarios.some(item => item.id === 'AUDIO-01' && ['passed', 'failed'].includes(item.status)),
    'The candidate audio verdict must already exist');
}

export async function macosAudioControl(evidence, target) {
  assert.equal(process.platform, 'darwin');
  assert.equal(process.env.GITHUB_ACTIONS, 'true', 'Independent control requires a disposable hosted session');
  assert.match(target, /^macos-\d+-(?:arm64|x64)$/);
  const report = JSON.parse(readFileSync(join(evidence, `${target}.qualification.json`), 'utf8'));
  requireRemovedCandidate(report);
  assert.equal(existsSync('/Library/Audio/Plug-Ins/HAL/MixRelayHAL.driver'), false, 'MixRelay HAL must be removed');
  assert.equal(existsSync(driver), false, 'An existing BlackHole installation must be preserved');
  const output = join(evidence, 'independent-audio-control');
  mkdirSync(output);
  const workspace = mkdtempSync(join(tmpdir(), 'mr-audio-control-'));
  let installed = false;
  let cadence;
  const result = { source: 'https://github.com/ExistentialAudio/BlackHole', revision,
    candidateSource: report.sourceRevision, candidateStatus: report.status, status: 'failed' };
  let sequence = 0;
  function run(program, args, timeout = 30000) {
    const child = spawnSync(program, args, { encoding: 'utf8', timeout, maxBuffer: 8 * 1024 * 1024 });
    writeFileSync(join(output, `${++sequence}-${program.replaceAll('/', '_')}.log`),
      `${child.stdout ?? ''}\n${child.stderr ?? ''}\nexit=${child.status}; error=${child.error?.message ?? ''}\n`);
    assert.ifError(child.error);
    assert.equal(child.status, 0, `${program} failed during independent control`);
    return child.stdout;
  }
  try {
    run('git', ['clone', '--quiet', '--no-checkout', 'https://github.com/ExistentialAudio/BlackHole.git', workspace]);
    run('git', ['-C', workspace, 'checkout', '--detach', revision]);
    assert.equal(run('git', ['-C', workspace, 'rev-parse', 'HEAD']).trim(), revision);
    run('xcodebuild', ['-project', join(workspace, 'BlackHole.xcodeproj'), '-scheme', 'BlackHole',
      '-configuration', 'Release', '-derivedDataPath', join(workspace, 'build'),
      `MACOSX_DEPLOYMENT_TARGET=${MINIMUM_MACOS}`, 'CODE_SIGNING_ALLOWED=NO'], 300000);
    const built = join(workspace, 'build/Build/Products/Release/BlackHole.driver');
    run('codesign', ['--force', '--sign', '-', built]);
    run('codesign', ['--verify', '--deep', '--strict', built]);
    run('sudo', ['-n', 'ditto', built, driver]);
    installed = true;
    run('sudo', ['-n', 'chown', '-R', 'root:wheel', driver]);
    run('sudo', ['-n', 'killall', 'coreaudiod']);
    const bundle = resolve('target/macos-probes/io.mixrelay.qualification.probe.app');
    assert.ok(existsSync(bundle), 'Use the same permission-authorized probe as the candidate');
    run('codesign', ['--verify', '--deep', '--strict', bundle]);
    const deadline = Date.now() + 30000;
    for (;;) {
      const devices = run('zsh', ['scripts/probe-macos-roundtrip.sh', '--list']);
      if (devices.includes('BlackHole2ch_UID')) break;
      assert.ok(Date.now() < deadline, 'Independent device was not published');
      await new Promise(resolve => setTimeout(resolve, 1000));
    }
    const companion = join(workspace, 'macos-cadence-control');
    run('xcrun', ['clang', '-std=c17', '-Wall', '-Wextra', '-Wpedantic', '-Wconversion',
      '-Wsign-conversion', '-Werror', `-mmacosx-version-min=${MINIMUM_MACOS}`, 'scripts/e2e/macos-cadence-control.c',
      '-framework', 'CoreAudio', '-framework', 'CoreFoundation', '-o', companion]);
    run(companion, ['--self-test']);
    cadence = launchCadenceControl(companion, output);
    await waitCadenceReady(cadence);
    const stdout = join(output, 'native-probe.stdout.json');
    const stderr = join(output, 'native-probe.stderr.log');
    run('open', ['-W', '-n', '-g', '--stdout', stdout, '--stderr', stderr, bundle, '--args',
      '--json', '--timing', '--capture-file', join(output, 'native-signal.f32'),
      '--seconds', '3', '--channels', '2', 'BlackHole2ch_UID'], 60000);
    const measured = JSON.parse(readFileSync(stdout, 'utf8'));
    result.measured = measured;
    result.cadence = await stopCadenceControl(cadence);
    cadence = null;
    requireCadenceExecution(result.cadence);
    assert.equal(measured.inputAuthorization?.final, 3);
    assert.equal(measured.inputAuthorization?.requested, false, 'The independent control must reuse established consent');
    assert.equal(measured.complete, true);
    assert.equal(measured.channelOrder, true);
    assert.equal(measured.output.rate, 48000);
    assert.equal(measured.output.bufferFrames, 512, 'The measurement client keeps its original native buffer');
    for (const field of ['missingFrames', 'repeatedFrames', 'corruptFrames', 'nonfiniteSamples'])
      assert.equal(measured[field], 0, field);
    result.status = 'passed';
  } catch (error) {
    result.error = error.message;
    throw error;
  } finally {
    try {
      let cadenceError;
      if (cadence && !cadence.closed) {
        try { result.cadence = await stopCadenceControl(cadence); }
        catch (error) { cadenceError = error; }
      }
      if (cadence) assert.equal(cadence.closed, true, 'Cadence I/O must exit before removing its driver');
      if (installed) {
        const executable = resolve('target/macos-probes/io.mixrelay.qualification.probe.app/Contents/MacOS/MRRoundTripProbe');
        const pattern = `^${executable.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}( |$)`;
        const active = spawnSync('pgrep', ['-u', String(process.getuid()), '-f', pattern], { encoding: 'utf8', timeout: 10000 });
        assert.ifError(active.error);
        assert.equal(active.status, 1, 'The control probe must exit before its driver is removed');
        run('sudo', ['-n', 'rm', '-rf', driver]);
        run('sudo', ['-n', 'killall', 'coreaudiod']);
      }
      if (cadenceError) throw cadenceError;
      result.cleanup = 'passed';
    } catch (error) {
      result.status = 'failed';
      result.cleanup = 'failed';
      result.cleanupError = error.message;
      throw error;
    } finally {
      writeFileSync(join(output, 'report.json'), `${JSON.stringify(result, null, 2)}\n`);
      rmSync(workspace, { recursive: true, force: true });
    }
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  assert.equal(process.argv.length, 4, 'Usage: node scripts/e2e/macos-audio-control.mjs EVIDENCE TARGET');
  await macosAudioControl(resolve(process.argv[2]), process.argv[3]);
}
