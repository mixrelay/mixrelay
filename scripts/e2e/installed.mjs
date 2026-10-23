import assert from 'node:assert/strict';
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { parseArgs } from 'node:util';
import { setTimeout as delay } from 'node:timers/promises';
import { digest, loadContract, requiredScenarios } from '../support-contract.mjs';
import { validateCandidate, artifactBytes } from '../qualification.mjs';
import { engineSocket, driverSocket, socketJson, socketRequest } from '../local-socket.mjs';
import { engineOperations } from '../lab/engine-operations.mjs';
import { observeEnvironment, validateEnvironment } from './environment.mjs';
import { requireDisposableHost, installAndVerify, launchInstalled } from './package.mjs';
import { nativeSignal, analyseRecording } from './native-signal.mjs';

async function eventually(read, predicate, description) {
  const deadline = Date.now() + 30000;
  let last;
  while (Date.now() < deadline) {
    try { last = await read(); if (predicate(last)) return last; } catch (error) { last = error.message; }
    await delay(200);
  }
  throw new Error(`${description}: ${JSON.stringify(last)}`);
}

export function signalGraph(original, graphId, spec) {
  return { ...original, revision: original.revision + 1, expectedGraphId: graphId,
    nodes: [{ id: 'qa-source', type: 'virtual_endpoint', config: { ...spec, reference: { render: true, capture: true } } },
      { id: 'qa-recorder', type: 'recorder', config: { channels: 2 } }],
    edges: [{ id: 'qa-recording', source: { nodeId: 'qa-source', portId: 'from_os' },
      destination: { nodeId: 'qa-recorder', portId: 'audio' }, channelMapping: { mode: 'auto' } }] };
}

// This is the same suite for every contract target. Platform code owns only installation
// and native signal generation. API evidence never substitutes for installed UI evidence.
export async function qualifyInstalled({ targetId, candidate, directory, output, disposable, loaded = loadContract() }) {
  const { contract, sha256 } = loaded;
  const target = contract.qualificationTargets.find(item => item.id === targetId);
  assert.ok(target, 'Unknown qualification target');
  validateCandidate(candidate, directory, loaded);
  const artifact = candidate.artifacts.find(item => item.id === target.artifact);
  const definition = contract.artifacts.find(item => item.id === target.artifact);
  mkdirSync(output, { mode: 0o700 });
  const report = { schemaVersion: 1, targetId, sourceRevision: candidate.sourceRevision,
    contractSha256: sha256, candidateSha256: digest(JSON.stringify(candidate)), artifactSha256: artifact.sha256,
    provenance: { repository: process.env.GITHUB_REPOSITORY, runId: process.env.GITHUB_RUN_ID,
      runAttempt: process.env.GITHUB_RUN_ATTEMPT, suiteRevision: process.env.GITHUB_SHA },
    startedAt: new Date().toISOString(), status: 'failed', cleanup: 'passed', scenarios: [] };
  function evidence(name, value) {
    const bytes = Buffer.from(JSON.stringify(value, null, 2) + '\n');
    writeFileSync(join(output, name), bytes, { flag: 'wx' });
    return { file: name, sha256: digest(bytes) };
  }
  async function scenario(id, work) {
    const scope = requiredScenarios(contract, target).find(item => item.id === id).scope;
    console.log(`${targetId}: ${id}: started`);
    try {
      const files = await work();
      report.scenarios.push({ id, scope, status: 'passed', evidence: files });
      console.log(`${targetId}: ${id}: passed`);
      return true;
    } catch (error) {
      report.scenarios.push({ id, scope, status: 'failed', error: error.message,
        evidence: [evidence(`${id}.failure.json`, { message: error.message, stack: error.stack })] });
      console.log(`${targetId}: ${id}: failed: ${error.message.slice(0, 500)}`);
      return false;
    }
  }
  const { api, operation } = engineOperations({ request: (path, options) => socketRequest(engineSocket(), path, options), now: Date.now, delay }, 'qualification');
  const driver = () => socketJson(driverSocket(), '/api/v1/driver', {
    headers: { host: 'mixrelay-driver' }, timeoutMs: 15000,
  });
  let stop, original, applied, graphId, recorderPath, unconfirmedMutation = false;
  const active = revision => eventually(() => api('graph/status'), status => status.activeRevision === revision, 'Graph did not activate');
  async function apply(document) {
    unconfirmedMutation = true;
    await api('graph', document);
    await active(document.revision);
    const observed = await api('graph');
    unconfirmedMutation = false;
    return observed;
  }
  try {
    report.environment = observeEnvironment();
    validateEnvironment(report.environment, target, contract);
    requireDisposableHost(target.os, disposable);
    const installed = await scenario('PKG-01', async () => {
      installAndVerify(artifact, definition, directory, output, target.os);
      report.environmentBeforeInstall = report.environment;
      report.environment = observeEnvironment();
      validateEnvironment(report.environment, target, contract);
      assert.deepEqual(artifactBytes(directory, artifact), { sha256: artifact.sha256, size: artifact.size });
      return [{ file: 'installed-binaries.json', sha256: digest(readFileSync(join(output, 'installed-binaries.json'))) }];
    });
    if (!installed) throw new Error('Installation or installed artifact identity failed');
    stop = await launchInstalled(target.os, output);
    const ready = await scenario('APP-01', async () => {
      const status = await eventually(() => api('status'), value => value.driverState === 'ready', 'Installed engine/Driver not ready');
      const snapshot = await driver();
      assert.equal(snapshot.state, 'ready');
      assert.equal(snapshot.installedVersion, candidate.version);
      return [evidence('app-ready.json', { status, driver: snapshot })];
    });
    if (!ready) {
      const status = await api('status'), snapshot = await driver();
      const proof = evidence('readiness-disagreement.json', { status, driver: snapshot,
        note: 'APP-01 remains failed. Independent native scenarios run only with a live installed Driver and engine.' });
      report.scenarios.find(item => item.id === 'APP-01').evidence.push(proof);
      assert.equal(snapshot.state, 'ready', 'Installed Driver is unavailable');
      assert.equal(snapshot.installedVersion, candidate.version);
      assert.equal(status.protocolVersion, 1);
      assert.equal(status.enginePower?.enabled, true);
      assert.equal(status.audioSession?.capabilities?.backend, target.os === 'macos' ? 'core_audio' : 'pipe_wire');
    }
    const catalog = await api('graphs');
    original = catalog.document; graphId = catalog.activeId;
    assert.deepEqual(original.nodes, [], 'Reset QA workspace: existing graph is preserved');
    assert.deepEqual(original.edges, []);
    const snapshot = await driver();
    // A fresh free installation already owns one stereo device. Creating another
    // requires paid access, so acceptance uses the actual default without a bypass.
    assert.equal(snapshot.settings.devices.length, 1, 'Reset QA device settings: expected the fresh default device');
    const device = snapshot.settings.devices[0];
    const spec = device.spec;
    assert.equal(spec.displayName, 'MixRelay IO');
    assert.equal(spec.render?.channels, 2); assert.equal(spec.capture?.channels, 2);
    assert.equal(device.observedSampleRate, 48000);
    assert.ok(device.nativeDeviceIds.length > 0, 'Native device was not published');
    try { applied = await apply(signalGraph(original, graphId, spec)); }
    catch (error) {
      report.scenarios.push({ id: 'AUDIO-01', scope: 'native-audio', status: 'failed', error: error.message,
        evidence: [evidence('audio-graph-setup.failure.json', { stage: 'native graph activation', message: error.message })] });
      throw error;
    }
    await scenario('GRAPH-02', async () => {
      const path = join(output, 'qualification.mix');
      async function workspace(action) {
        const current = await api('graphs');
        const reply = await operation('POST', '/api/v1/workspace', JSON.stringify({
          expectedRevision: current.document.revision, expectedCatalogRevision: current.catalogRevision, operation: { action, path },
        }));
        assert.equal(reply.status, 200, reply.body);
        await active(reply.json.document.revision);
        return reply.json;
      }
      await workspace('save');
      const before = applied;
      applied = await apply({ ...applied, expectedGraphId: graphId, revision: applied.revision + 1, edges: [] });
      const opened = await workspace('open');
      applied = opened.document; graphId = opened.activeId;
      assert.deepEqual(applied.nodes, before.nodes);
      assert.deepEqual(applied.edges, before.edges);
      return [evidence('graph-reopen.json', { before, after: applied }), { file: 'qualification.mix', sha256: digest(readFileSync(path)) }];
    });
    recorderPath = `graphs/${encodeURIComponent(graphId)}/recorders/qa-recorder`;
    const recording = join(output, 'recording.wav');
    const start = await api(recorderPath);
    assert.equal(start.state, 'stopped');
    await api(recorderPath, { expectedRevision: applied.revision, expectedRecording: { state: start.state, path: start.path },
      request: { action: 'record', path: recording, sampleRate: 48000, encoding: 'float32' } });
    await scenario('AUDIO-01', async () => {
      await nativeSignal(device, output, target.os);
      return [{ file: 'native-signal.json', sha256: digest(readFileSync(join(output, 'native-signal.json'))) }];
    });
    await scenario('REC-02', async () => {
      const take = await api(recorderPath);
      const stopped = await api(recorderPath, { expectedRevision: applied.revision,
        expectedRecording: { state: take.state, path: take.path }, request: { action: 'stop' } });
      assert.equal(stopped.state, 'stopped');
      assert.equal(stopped.error, null);
      const analysis = analyseRecording(recording);
      return [evidence('recording-analysis.json', { stopped, analysis }), { file: 'recording.wav', sha256: digest(readFileSync(recording)) }];
    });
  } catch (error) { report.error = error.message; }
  finally {
    const cleanupFailed = error => {
      report.cleanup = 'failed';
      report.cleanupError = [report.cleanupError, error.message].filter(Boolean).join('\n');
    };
    try {
      assert.equal(unconfirmedMutation, false, 'Unconfirmed graph publication; reset the QA session');
      if (applied) {
        const current = await api('graphs');
        assert.equal(current.activeId, graphId, 'External selection changed; preserve it');
        assert.deepEqual(current.document, applied, 'External graph change; preserve it');
        if (recorderPath) {
          const take = await api(recorderPath);
          if (['recording', 'paused'].includes(take.state)) await api(recorderPath, { expectedRevision: applied.revision,
            expectedRecording: { state: take.state, path: take.path }, request: { action: 'stop' } });
        }
        await apply({ ...original, expectedGraphId: graphId, revision: applied.revision + 1 });
      }
    } catch (error) { cleanupFailed(error); }
    try { await stop?.(); } catch (error) { cleanupFailed(error); }
    for (const expected of requiredScenarios(contract, target)) if (!report.scenarios.some(item => item.id === expected.id)) {
      report.scenarios.push({ id: expected.id, scope: expected.scope, status: 'blocked', evidence: [],
        error: expected.id === 'GRAPH-01' ? 'Installed UI automation is not implemented' : expected.id === 'AUDIO-11'
          ? 'Whole-system capture adapter and native acceptance are not implemented' : 'A prerequisite failed' });
    }
    report.status = !report.error && report.cleanup === 'passed' && report.scenarios.every(item => item.status === 'passed') ? 'passed' : 'failed';
    report.endedAt = new Date().toISOString();
    writeFileSync(join(output, `${targetId}.qualification.json`), JSON.stringify(report, null, 2) + '\n', { flag: 'wx' });
  }
  return report;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  try {
    const { values } = parseArgs({ options: { target: { type: 'string' }, candidate: { type: 'string' },
      directory: { type: 'string' }, output: { type: 'string' }, 'disposable-environment': { type: 'boolean' }, help: { type: 'boolean' } } });
    if (values.help) console.log('pnpm test:e2e:installed --target ID --candidate FILE --directory FINAL_ARTIFACTS --output NEW_EVIDENCE_DIR --disposable-environment\nInstalls the exact candidate on a clean native desktop QA session. Never run on a personal workstation. Missing scenarios block release.');
    else {
      for (const key of ['target', 'candidate', 'directory', 'output']) assert.ok(values[key], `--${key} is required`);
      const report = await qualifyInstalled({ targetId: values.target, candidate: JSON.parse(readFileSync(values.candidate)),
        directory: resolve(values.directory), output: resolve(values.output), disposable: values['disposable-environment'] });
      console.log(`${report.targetId}: ${report.status}${report.error ? `: ${report.error}` : ''}`);
      process.exitCode = report.status === 'passed' ? 0 : 1;
    }
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
