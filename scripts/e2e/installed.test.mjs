import assert from 'node:assert/strict';
import test from 'node:test';
import { signalGraph } from './installed.mjs';

test('unity-level acceptance records the Driver mix without adding a feedback return', () => {
  const original = { schemaVersion: 1, revision: 10, nodes: [], edges: [] };
  const spec = { id: 'default-device', render: { channels: 2 }, capture: { channels: 2 } };
  const graph = signalGraph(original, 'selected-graph', spec);
  const endpoint = graph.nodes.find((node) => node.type === 'virtual_endpoint');
  const recorder = graph.nodes.find((node) => node.type === 'recorder');
  assert.equal(endpoint.config.id, spec.id);
  assert.deepEqual(endpoint.config.reference, { render: true, capture: false });
  assert.deepEqual(
    new Set(
      graph.edges
        .filter((edge) => edge.source.nodeId === endpoint.id && edge.source.portId === 'from_os')
        .map((edge) => `${edge.destination.nodeId}/${edge.destination.portId}`),
    ),
    new Set([`${recorder.id}/audio`]),
  );
  assert.equal(graph.expectedGraphId, 'selected-graph');
  assert.equal(graph.revision, 11);
  assert.deepEqual(original.nodes, []);
});

test('fresh and upgrade share the target, retain independent full suites and produce one gate report', async (t) => {
  const { writeFileSync, mkdirSync } = await import('node:fs');
  const { join } = await import('node:path');
  const { qualifyInstalled } = await import('./installed.mjs');
  const { qualificationFixture } = await import('./qualification.fixture.mjs');
  const { readReports, evaluateQualification } = await import('../qualification.mjs');
  const f = qualificationFixture(t),
    calls = [];
  const target = f.reports[2];
  const output = join(f.directory, 'dual-mode');
  const runMode = async (options) => {
    calls.push(options);
    mkdirSync(options.output);
    writeFileSync(join(options.output, 'unit-evidence.txt'), 'Synthetic scenario evidence\n');
    return {
      ...structuredClone(target),
      scenarios: target.scenarios.filter((item) => item.id !== 'UPGRADE-01'),
    };
  };
  const report = await qualifyInstalled(
    { targetId: target.targetId, candidate: f.candidate, directory: f.directory, output },
    runMode,
  );
  assert.equal(calls.length, 2);
  assert.equal(calls[0].baselineDirectory, undefined);
  assert.equal(calls[1].baselineDirectory, join(f.directory, 'baseline'));
  assert.equal(calls[0].targetId, calls[1].targetId);
  assert.equal(report.status, 'passed');
  const reports = readReports(output);
  assert.equal(reports.length, 1);
  assert.ok(
    report.scenarios
      .find((item) => item.id === 'UPGRADE-01')
      .evidence.some((item) => item.file === 'upgrade-report.json'),
  );
  const all = f.reports.map((item) => (item.targetId === target.targetId ? reports[0] : item));
  assert.equal(evaluateQualification(f.candidate, all).passed, true);
  writeFileSync(join(output, 'upgrade-unit-evidence.txt'), 'changed upgrade evidence');
  assert.throws(() => readReports(output), /changed raw evidence/);
});

test('upgrade failures cannot be hidden by passing fresh results', async (t) => {
  const { writeFileSync, mkdirSync } = await import('node:fs');
  const { join } = await import('node:path');
  const { qualifyInstalled } = await import('./installed.mjs');
  const { qualificationFixture } = await import('./qualification.fixture.mjs');
  const { evaluateQualification } = await import('../qualification.mjs');
  const f = qualificationFixture(t),
    target = f.reports[2];
  for (const failure of [
    'Previous app entitlement policy rejected',
    'Missing baseline artifact: macos-universal-pkg',
    'Native audio failed after upgrade',
    'Removal failed after upgrade',
    'Runtime shutdown failed after upgrade',
  ]) {
    const output = join(f.directory, `case-${failure.slice(0, 10)}`);
    const report = await qualifyInstalled(
      { targetId: target.targetId, candidate: f.candidate, directory: f.directory, output },
      async (options) => {
        mkdirSync(options.output);
        writeFileSync(join(options.output, 'unit-evidence.txt'), 'Synthetic scenario evidence\n');
        const result = {
          ...structuredClone(target),
          scenarios: target.scenarios.filter((item) => item.id !== 'UPGRADE-01'),
        };
        if (options.baselineDirectory) {
          result.status = 'failed';
          result.error = failure;
          if (failure.startsWith('Missing baseline'))
            result.scenarios = result.scenarios.map((item) => ({
              ...item,
              status: 'blocked',
              error: 'A prerequisite failed',
            }));
          if (failure.startsWith('Runtime shutdown')) {
            result.error = undefined;
            result.cleanup = 'failed';
            result.cleanupError = failure;
          }
        }
        return result;
      },
    );
    assert.equal(report.status, 'failed');
    const upgrade = report.scenarios.find((item) => item.id === 'UPGRADE-01');
    assert.equal(upgrade.status, 'failed');
    assert.equal(upgrade.error, failure);
    const all = f.reports.map((item) => (item.targetId === target.targetId ? report : item));
    assert.equal(evaluateQualification(f.candidate, all).passed, false);
  }
});

test('a passing upgrade cannot hide a failed fresh-install scenario', async (t) => {
  const { writeFileSync, mkdirSync } = await import('node:fs');
  const { join } = await import('node:path');
  const { qualifyInstalled } = await import('./installed.mjs');
  const { qualificationFixture } = await import('./qualification.fixture.mjs');
  const { evaluateQualification } = await import('../qualification.mjs');
  const f = qualificationFixture(t),
    target = f.reports[2];
  const report = await qualifyInstalled(
    {
      targetId: target.targetId,
      candidate: f.candidate,
      directory: f.directory,
      output: join(f.directory, 'fresh-failure'),
    },
    async (options) => {
      mkdirSync(options.output);
      writeFileSync(join(options.output, 'unit-evidence.txt'), 'Synthetic scenario evidence\n');
      const result = {
        ...structuredClone(target),
        scenarios: target.scenarios.filter((item) => item.id !== 'UPGRADE-01'),
      };
      if (!options.baselineDirectory) {
        result.status = 'failed';
        result.scenarios = result.scenarios.map((item) =>
          item.id === 'GRAPH-01'
            ? { ...item, status: 'failed', error: 'Fresh UI graph failed' }
            : item,
        );
      }
      return result;
    },
  );
  assert.equal(report.scenarios.find((item) => item.id === 'UPGRADE-01').status, 'passed');
  assert.equal(report.scenarios.find((item) => item.id === 'GRAPH-01').status, 'failed');
  assert.equal(report.status, 'failed');
  const all = f.reports.map((item) => (item.targetId === target.targetId ? report : item));
  assert.equal(evaluateQualification(f.candidate, all).passed, false);
});
