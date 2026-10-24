import assert from 'node:assert/strict';
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';
import { engineOperations } from '../lab/engine-operations.mjs';
import { invoke } from '../lab/macos-installed-capture-acceptance.mjs';
import { driverSocket, engineSocket, socketJson, socketRequest } from '../local-socket.mjs';
import { artifactBytes, validateCandidate } from '../qualification.mjs';
import { digest, loadContract, requiredScenarios } from '../support-contract.mjs';
import { validateBaseline } from './baseline.mjs';
import { collectLinux } from './collect-linux.mjs';
import { collectMacos } from './collect-macos.mjs';
import { observeEnvironment, validateEnvironment } from './environment.mjs';
import { finishAfterCleanup, startInstalledUi } from './installed-ui.mjs';
import { analyseRecording, nativeSignal } from './native-signal.mjs';
import { installAndVerify, launchInstalled, requireDisposableHost } from './package.mjs';
import { removeInstalled } from './removal.mjs';
import { wholeSystemCapture } from './system-capture.mjs';
import {
  isolateFreshState,
  prepareUpgradeDriver,
  preservedState,
  restoreUpgradeDriver,
  saveWorkspace,
  upgradeGraph,
  verifyAuthoredGraph,
  verifyPreservedState,
} from './upgrade-state.mjs';

async function eventually(read, predicate, description, { timeoutMs = 30000, now = Date.now, wait = delay } = {}) {
  const deadline = now() + timeoutMs;
  let last;
  while (now() < deadline) {
    try {
      last = await read();
      if (predicate(last)) return last;
    } catch (error) {
      last = error.message;
    }
    await wait(200);
  }
  throw new Error(`${description}: ${JSON.stringify(last)}`);
}

export function installedReadyStatus(api, { accelerator = process.env.MIXRELAY_E2E_VM_ACCELERATOR,
  now = Date.now, wait = delay } = {}) {
  // TCG guest startup exceeded 30s while the independent Driver already answered.
  // Require the same ready observation, with a bounded emulated-CPU budget.
  return eventually(() => api('status'), value => value.driverState === 'ready',
    'Installed engine/Driver not ready', { timeoutMs: accelerator === 'tcg' ? 120000 : 30000, now, wait });
}

export function signalGraph(original, graphId, spec) {
  const template = JSON.parse(
    readFileSync(
      new URL('../../crates/audio-graph/src/fixtures/qualification-stereo.json', import.meta.url),
    ),
  );
  return {
    ...original,
    revision: original.revision + 1,
    expectedGraphId: graphId,
    nodes: template.nodes.map((node) =>
      node.type === 'virtual_endpoint'
        ? { ...node, config: { ...spec, reference: node.config.reference } }
        : node,
    ),
    edges: template.edges,
  };
}

// The desktop has already collected the operation receipt. A pending graph
// publication still needs its audio-thread acknowledgement before recording.
export async function applyDesktopGraph(document, { invoke, active, api }) {
  const response = await invoke('control_apply_graph', { document });
  assert.ok([200, 202].includes(response.status), JSON.stringify(response));
  assert.equal(response.body.protocolVersion, 1);
  assert.equal(response.body.revision, document.revision);
  assert.ok(['active', 'pending'].includes(response.body.state), JSON.stringify(response));
  await active(document.revision);
  const observed = await api('graph');
  assert.equal(observed.revision, document.revision);
  assert.deepEqual(observed.nodes, document.nodes);
  assert.deepEqual(observed.edges, document.edges);
  return observed;
}

// Native preparation owns first-use consent. Qualification submits one change and
// collects its receipt; permission errors must fail instead of being replayed here.
export async function applyNativeGraph(document, { operation }) {
  const reply = await operation('PUT', '/api/v1/graph', JSON.stringify(document));
  assert.ok(reply.status >= 200 && reply.status < 300, `Native graph: HTTP ${reply.status} ${reply.body}`);
  assert.equal(reply.json.revision, document.revision);
  assert.ok(['active', 'pending'].includes(reply.json.state), JSON.stringify(reply));
}

// This is the same suite for every contract target. Platform code owns only installation
// and native signal generation. API evidence never substitutes for installed UI evidence.
export async function qualifyMode({
  targetId,
  candidate,
  directory,
  output,
  disposable,
  baselineDirectory,
  loaded = loadContract(),
}) {
  const { contract, sha256 } = loaded;
  const target = contract.qualificationTargets.find((item) => item.id === targetId);
  assert.ok(target, 'Unknown qualification target');
  validateCandidate(candidate, directory, loaded);
  const artifact = candidate.artifacts.find((item) => item.id === target.artifact);
  const definition = contract.artifacts.find((item) => item.id === target.artifact);
  mkdirSync(output, { mode: 0o700 });
  const report = {
    schemaVersion: 1,
    targetId,
    sourceRevision: candidate.sourceRevision,
    contractSha256: sha256,
    candidateSha256: digest(JSON.stringify(candidate)),
    artifactSha256: artifact.sha256,
    provenance: {
      repository: process.env.GITHUB_REPOSITORY,
      runId: process.env.GITHUB_RUN_ID,
      runAttempt: process.env.GITHUB_RUN_ATTEMPT,
      suiteRevision: process.env.GITHUB_SHA,
    },
    startedAt: new Date().toISOString(),
    status: 'failed',
    cleanup: 'passed',
    scenarios: [],
  };
  function evidence(name, value) {
    const bytes = Buffer.from(`${JSON.stringify(value, null, 2)}\n`);
    writeFileSync(join(output, name), bytes, { flag: 'wx' });
    return { file: name, sha256: digest(bytes) };
  }
  async function scenario(id, work) {
    const scope = requiredScenarios(contract, target).find((item) => item.id === id).scope;
    console.log(`${targetId}: ${id}: started`);
    try {
      const files = await work();
      report.scenarios.push({ id, scope, status: 'passed', evidence: files });
      console.log(`${targetId}: ${id}: passed`);
      return true;
    } catch (error) {
      report.scenarios.push({
        id,
        scope,
        status: 'failed',
        error: error.message,
        evidence: [evidence(`${id}.failure.json`, { message: error.message, stack: error.stack })],
      });
      console.log(`${targetId}: ${id}: failed: ${error.message.slice(0, 500)}`);
      return false;
    }
  }
  const { api, operation } = engineOperations(
    {
      request: (path, options) => socketRequest(engineSocket(), path, options),
      now: Date.now,
      delay,
    },
    'qualification',
  );
  const driver = (command) =>
    socketJson(driverSocket(), '/api/v1/driver', {
      method: command ? 'PUT' : 'GET',
      json: command,
      headers: { host: 'mixrelay-driver' },
      timeoutMs: 15000,
    });
  let stop,
    ui,
    original,
    applied,
    graphId,
    recorderPath,
    upgradeState,
    packageInstalled = false,
    unconfirmedMutation = false;
  const active = (revision) =>
    eventually(
      () => api('graph/status'),
      (status) => status.activeRevision === revision,
      'Graph did not activate',
    );
  async function apply(document) {
    unconfirmedMutation = true;
    await applyNativeGraph(document, { operation });
    await active(document.revision);
    const observed = await api('graph');
    unconfirmedMutation = false;
    return observed;
  }
  try {
    report.environment = observeEnvironment();
    validateEnvironment(report.environment, target, contract);
    requireDisposableHost(target.os, disposable);
    if (baselineDirectory) {
      assert.ok(
        existsSync(join(baselineDirectory, 'baseline.json')),
        `Missing baseline selection: ${baselineDirectory}`,
      );
      const baseline = JSON.parse(readFileSync(join(baselineDirectory, 'baseline.json')));
      const baselineArtifact = validateBaseline(
        baseline,
        candidate,
        baselineDirectory,
        target.artifact,
      );
      report.baseline = { ...baseline, artifactSha256: baselineArtifact.sha256 };
      report.isolatedFreshState = isolateFreshState(target.os, output, { disposable });
      const baselineOutput = join(output, 'baseline');
      mkdirSync(baselineOutput);
      installAndVerify(baselineArtifact, definition, baselineDirectory, baselineOutput, target.os);
      packageInstalled = true;
      stop = await launchInstalled(target.os, baselineOutput);
      await eventually(
        driver,
        (value) => value.state === 'ready' && value.installedVersion === baseline.version,
        'Baseline Driver not ready',
      );
      const catalog = await eventually(
        () => api('graphs'),
        (value) => !!value.document,
        'Baseline graph unavailable',
      );
      graphId = catalog.activeId;
      const baselineDriver = await prepareUpgradeDriver(driver);
      unconfirmedMutation = true;
      const authored = upgradeGraph(catalog.document, graphId);
      await api('graph', authored);
      const path = join(output, 'upgrade-session.mix');
      const saved = await saveWorkspace(api, operation, active, 'save', path);
      verifyAuthoredGraph(authored, saved);
      unconfirmedMutation = false;
      upgradeState = preservedState(saved, await driver(), path, baselineDriver.defaults);
      evidence('upgrade-state-before.json', upgradeState);
      await stop();
      stop = undefined;
    }
    const installed = await scenario('PKG-01', async () => {
      installAndVerify(artifact, definition, directory, output, target.os);
      packageInstalled = true;
      report.environmentBeforeInstall = report.environment;
      report.environment = observeEnvironment();
      validateEnvironment(report.environment, target, contract);
      assert.deepEqual(artifactBytes(directory, artifact), {
        sha256: artifact.sha256,
        size: artifact.size,
      });
      return [
        {
          file: 'installed-binaries.json',
          sha256: digest(readFileSync(join(output, 'installed-binaries.json'))),
        },
      ];
    });
    if (!installed) throw new Error('Installation or installed artifact identity failed');
    stop = await launchInstalled(target.os, output);
    const ready = await scenario('APP-01', async () => {
      const status = await installedReadyStatus(api);
      const snapshot = await driver();
      assert.equal(snapshot.state, 'ready');
      assert.equal(snapshot.installedVersion, candidate.version);
      return [evidence('app-ready.json', { status, driver: snapshot })];
    });
    if (!ready) {
      const status = await api('status'),
        snapshot = await driver();
      const proof = evidence('readiness-disagreement.json', {
        status,
        driver: snapshot,
        note: 'APP-01 remains failed. Independent native scenarios run only with a live installed Driver and engine.',
      });
      report.scenarios.find((item) => item.id === 'APP-01').evidence.push(proof);
      assert.equal(snapshot.state, 'ready', 'Installed Driver is unavailable');
      assert.equal(snapshot.installedVersion, candidate.version);
      assert.equal(status.protocolVersion, 1);
      assert.equal(status.enginePower?.enabled, true);
      assert.equal(
        status.audioSession?.capabilities?.backend,
        target.os === 'macos' ? 'core_audio' : 'pipe_wire',
      );
    }
    if (upgradeState) {
      const current = await api('graphs');
      verifyPreservedState(upgradeState, current, await driver());
      const reopened = await saveWorkspace(api, operation, active, 'open', upgradeState.path);
      verifyPreservedState(upgradeState, reopened, await driver());
      evidence(
        'upgrade-state-after.json',
        preservedState(reopened, await driver(), upgradeState.path, upgradeState.defaultDevices),
      );
      await restoreUpgradeDriver(driver, upgradeState.defaultDevices);
      graphId = reopened.activeId;
      await apply({
        ...reopened.document,
        nodes: [],
        edges: [],
        expectedGraphId: graphId,
        revision: reopened.document.revision + 1,
      });
    }
    const catalog = await api('graphs');
    original = catalog.document;
    graphId = catalog.activeId;
    assert.deepEqual(original.nodes, [], 'Reset QA workspace: existing graph is preserved');
    assert.deepEqual(original.edges, []);
    ui = await startInstalledUi(target.os, output);
    await scenario('GRAPH-01', async () => {
      await ui.ready();
      const current = await eventually(
        () => api('graphs'),
        (value) => value.document.nodes.length === 2 && value.document.edges.length > 0,
        'Installed UI did not commit its connection',
      );
      const source = current.document.nodes.find((node) => node.type === 'compressor');
      const recorder = current.document.nodes.find((node) => node.type === 'recorder');
      assert.ok(source && recorder, 'Installed UI created different nodes');
      assert.ok(
        current.document.edges.every(
          (edge) => edge.source.nodeId === source.id && edge.destination.nodeId === recorder.id,
        ),
        'Installed UI connected different nodes',
      );
      await active(current.document.revision);
      const proof = evidence('installed-ui-graph.json', current);
      const files = ['installed-ui.png', 'installed-ui-tree.txt'].map((file) => ({
        file,
        sha256: digest(readFileSync(join(output, file))),
      }));
      return [proof, ...files];
    });
    const afterUi = await api('graphs');
    assert.equal(afterUi.activeId, graphId);
    assert.ok(
      afterUi.document.nodes.every((node) => ['compressor', 'recorder'].includes(node.type)),
      'Unexpected external graph change',
    );
    applied = await apply({
      ...original,
      expectedGraphId: graphId,
      revision: afterUi.document.revision + 1,
    });
    const snapshot = await driver();
    // A fresh free installation already owns one stereo device. Creating another
    // requires paid access, so acceptance uses the actual default without a bypass.
    assert.equal(
      snapshot.settings.devices.length,
      1,
      'Reset QA device settings: expected the fresh default device',
    );
    const device = snapshot.settings.devices[0];
    const spec = device.spec;
    assert.equal(spec.displayName, 'MixRelay IO');
    assert.equal(spec.render?.channels, 2);
    assert.equal(spec.capture?.channels, 2);
    assert.equal(device.observedSampleRate, 48000);
    assert.ok(device.nativeDeviceIds.length > 0, 'Native device was not published');
    try {
      applied = await apply(signalGraph(applied, graphId, spec));
    } catch (error) {
      report.scenarios.push({
        id: 'AUDIO-01',
        scope: 'native-audio',
        status: 'failed',
        error: error.message,
        evidence: [
          evidence('audio-graph-setup.failure.json', {
            stage: 'native graph activation',
            message: error.message,
          }),
        ],
      });
      throw error;
    }
    await scenario('GRAPH-02', async () => {
      const path = join(output, 'qualification.mix');
      async function workspace(action) {
        const current = await api('graphs');
        const reply = await operation(
          'POST',
          '/api/v1/workspace',
          JSON.stringify({
            expectedRevision: current.document.revision,
            expectedCatalogRevision: current.catalogRevision,
            operation: { action, path },
          }),
        );
        assert.equal(reply.status, 200, reply.body);
        await active(reply.json.document.revision);
        return reply.json;
      }
      await workspace('save');
      const before = applied;
      applied = await apply({
        ...applied,
        expectedGraphId: graphId,
        revision: applied.revision + 1,
        edges: [],
      });
      const opened = await workspace('open');
      applied = opened.document;
      graphId = opened.activeId;
      assert.deepEqual(applied.nodes, before.nodes);
      assert.deepEqual(applied.edges, before.edges);
      return [
        evidence('graph-reopen.json', { before, after: applied }),
        { file: 'qualification.mix', sha256: digest(readFileSync(path)) },
      ];
    });
    recorderPath = `graphs/${encodeURIComponent(graphId)}/recorders/qa-recorder`;
    const recording = join(output, 'recording.wav');
    const start = await api(recorderPath);
    assert.equal(start.state, 'stopped');
    await api(recorderPath, {
      expectedRevision: applied.revision,
      expectedRecording: { state: start.state, path: start.path },
      request: { action: 'record', path: recording, sampleRate: 48000, encoding: 'float32' },
    });
    await scenario('AUDIO-01', async () => {
      await nativeSignal(device, output, target.os);
      return [
        {
          file: 'native-signal.json',
          sha256: digest(readFileSync(join(output, 'native-signal.json'))),
        },
      ];
    });
    await scenario('REC-02', async () => {
      const take = await api(recorderPath);
      const stopped = await api(recorderPath, {
        expectedRevision: applied.revision,
        expectedRecording: { state: take.state, path: take.path },
        request: { action: 'stop' },
      });
      assert.equal(stopped.state, 'stopped');
      assert.equal(stopped.error, null);
      const analysis = analyseRecording(recording);
      return [
        evidence('recording-analysis.json', { stopped, analysis }),
        { file: 'recording.wav', sha256: digest(readFileSync(recording)) },
      ];
    });
    if (target.os === 'macos')
      await scenario('AUDIO-11', async () => {
        await wholeSystemCapture({
          api,
          graphId,
          document: applied,
          device,
          output,
          apply: async (document) => {
            unconfirmedMutation = true;
            applied = await applyDesktopGraph(document, { invoke, active, api });
            unconfirmedMutation = false;
            return applied;
          },
        });
        return ['whole-system-analysis.json', 'whole-system.wav'].map((file) => ({
          file,
          sha256: digest(readFileSync(join(output, file))),
        }));
      });
  } catch (error) {
    report.error = error.message;
  } finally {
    const cleanupFailed = (error) => {
      report.cleanup = 'failed';
      report.cleanupError = [report.cleanupError, error.message].filter(Boolean).join('\n');
    };
    try {
      assert.equal(
        unconfirmedMutation,
        false,
        'Unconfirmed graph publication; reset the QA session',
      );
      if (applied) {
        const current = await api('graphs');
        assert.equal(current.activeId, graphId, 'External selection changed; preserve it');
        assert.deepEqual(current.document, applied, 'External graph change; preserve it');
        if (recorderPath) {
          const take = await api(recorderPath);
          if (['recording', 'paused'].includes(take.state))
            await api(recorderPath, {
              expectedRevision: applied.revision,
              expectedRecording: { state: take.state, path: take.path },
              request: { action: 'stop' },
            });
        }
        await apply({ ...original, expectedGraphId: graphId, revision: applied.revision + 1 });
      }
    } catch (error) {
      cleanupFailed(error);
    }
    if (
      target.os === 'linux' &&
      (report.error || report.scenarios.some((item) => item.status === 'failed'))
    ) {
      try {
        collectLinux(output);
      } catch (error) {
        cleanupFailed(error);
      }
    }
    if (
      target.os === 'macos' &&
      (report.error || report.scenarios.some((item) => item.status === 'failed'))
    ) {
      try {
        collectMacos(output, 'live-');
      } catch (error) {
        cleanupFailed(error);
      }
    }
    const failedUi = (error) => {
      const result = report.scenarios.find((item) => item.id === 'GRAPH-01');
      if (result) {
        result.status = 'failed';
        result.error = error.message;
        result.evidence.push(
          evidence('installed-ui-completion.failure.json', { message: error.message }),
        );
      }
    };
    const cleanup = async () => {
      try {
        await stop?.();
      } catch (error) {
        cleanupFailed(error);
        if (target.os === 'macos') {
          try {
            collectMacos(output, 'shutdown-');
          } catch (diagnosticError) {
            cleanupFailed(diagnosticError);
          }
        }
      }
      if (packageInstalled)
        await scenario('PKG-02', async () => {
          const retained = ['qualification.mix', 'recording.wav', 'upgrade-session.mix']
            .filter((file) => existsSync(join(output, file)))
            .map((file) => ({ file, sha256: digest(readFileSync(join(output, file))) }));
          removeInstalled(target.os, definition.format, output);
          for (const item of retained)
            assert.equal(
              digest(readFileSync(join(output, item.file))),
              item.sha256,
              `Removal changed ${item.file}`,
            );
          return [{ file: 'removal.log', sha256: digest(readFileSync(join(output, 'removal.log'))) }];
        });
    };
    try {
      const result = await finishAfterCleanup(ui, cleanup);
      if (result && !result.passed) failedUi(new Error(result.error));
    } catch (error) {
      failedUi(error);
      cleanupFailed(error);
    }
    for (const expected of requiredScenarios(contract, target).filter(
      (item) => item.id !== 'UPGRADE-01',
    ))
      if (!report.scenarios.some((item) => item.id === expected.id)) {
        report.scenarios.push({
          id: expected.id,
          scope: expected.scope,
          status: 'blocked',
          evidence: [],
          error: 'A prerequisite failed',
        });
      }
    report.status =
      !report.error &&
      report.cleanup === 'passed' &&
      report.scenarios.every((item) => item.status === 'passed')
        ? 'passed'
        : 'failed';
    report.endedAt = new Date().toISOString();
    writeFileSync(join(output, `${targetId}.mode.json`), `${JSON.stringify(report, null, 2)}\n`, {
      flag: 'wx',
    });
  }
  return report;
}

function stateEvidence(directory, mode, flatten) {
  return ['upgrade-state-before.json', 'upgrade-state-after.json', 'upgrade-session.mix']
    .filter((name) => existsSync(join(directory, name)))
    .flatMap((name) =>
      flatten(directory, mode, [
        { file: name, sha256: digest(readFileSync(join(directory, name))) },
      ]),
    );
}

export async function qualifyInstalled(options, runMode = qualifyMode) {
  const { output, targetId } = options;
  mkdirSync(output, { mode: 0o700 });
  const freshOutput = join(output, 'fresh');
  const fresh = await runMode({ ...options, output: freshOutput, baselineDirectory: undefined });
  const baselineDirectory = options.baselineDirectory || join(options.directory, 'baseline');
  const upgradeOutput = join(output, 'upgrade');
  const upgrade = await runMode({ ...options, output: upgradeOutput, baselineDirectory });
  const flatten = (directory, prefix, items) =>
    items.map((item) => {
      const file = `${prefix}-${item.file}`;
      copyFileSync(join(directory, item.file), join(output, file));
      return { file, sha256: item.sha256 };
    });
  const report = {
    ...fresh,
    endedAt: upgrade.endedAt,
    baseline: upgrade.baseline,
    cleanupError:
      [fresh.cleanupError, upgrade.cleanupError].filter(Boolean).join('\n') || undefined,
    status: fresh.status === 'passed' && upgrade.status === 'passed' ? 'passed' : 'failed',
    cleanup: fresh.cleanup === 'passed' && upgrade.cleanup === 'passed' ? 'passed' : 'failed',
    scenarios: fresh.scenarios.map((item) => ({
      ...item,
      evidence: flatten(freshOutput, 'fresh', item.evidence),
    })),
  };
  const upgradeEvidence = upgrade.scenarios.flatMap((item) =>
    flatten(upgradeOutput, 'upgrade', item.evidence),
  );
  for (const [mode, directory, result] of [
    ['fresh', freshOutput, fresh],
    ['upgrade', upgradeOutput, upgrade],
  ]) {
    const file = `${mode}-report.json`;
    writeFileSync(join(output, file), `${JSON.stringify(result, null, 2)}\n`, { flag: 'wx' });
    upgradeEvidence.push({ file, sha256: digest(readFileSync(join(output, file))) });
    upgradeEvidence.push(...stateEvidence(directory, mode, flatten));
  }
  report.scenarios.push({
    id: 'UPGRADE-01',
    scope: 'artifact',
    status: upgrade.status,
    error:
      upgrade.scenarios.find((item) => item.status === 'failed')?.error ??
      upgrade.error ??
      upgrade.cleanupError,
    evidence: upgradeEvidence,
  });
  writeFileSync(
    join(output, `${targetId}.qualification.json`),
    `${JSON.stringify(report, null, 2)}\n`,
    { flag: 'wx' },
  );
  return report;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const { values } = parseArgs({
      options: {
        target: { type: 'string' },
        candidate: { type: 'string' },
        baseline: { type: 'string' },
        directory: { type: 'string' },
        output: { type: 'string' },
        'disposable-environment': { type: 'boolean' },
        help: { type: 'boolean' },
      },
    });
    if (values.help)
      console.log(
        'pnpm test:e2e:installed --target ID --candidate FILE --directory FINAL_ARTIFACTS --output NEW_EVIDENCE_DIR --disposable-environment\nRuns fresh install/removal, then baseline-to-candidate upgrade/removal in the same disposable session. Baseline defaults to FINAL_ARTIFACTS/baseline; override with --baseline DIRECTORY. Never run on a personal workstation. Missing scenarios block release.',
      );
    else {
      for (const key of ['target', 'candidate', 'directory', 'output'])
        assert.ok(values[key], `--${key} is required`);
      const report = await qualifyInstalled({
        targetId: values.target,
        candidate: JSON.parse(readFileSync(values.candidate)),
        directory: resolve(values.directory),
        output: resolve(values.output),
        baselineDirectory: values.baseline && resolve(values.baseline),
        disposable: values['disposable-environment'],
      });
      console.log(`${report.targetId}: ${report.status}${report.error ? `: ${report.error}` : ''}`);
      process.exitCode = report.status === 'passed' ? 0 : 1;
    }
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
