import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { compareMacosSources } from './macos-source-comparison.mjs';
import { analyseSourceRecording } from './source-recording-analysis.mjs';

function analyseFinalizedTake(directory) {
  const bytes = readFileSync(join(directory, 'recording.wav'));
  const analysis = { recordingSha256: createHash('sha256').update(bytes).digest('hex'), ...analyseSourceRecording(bytes) };
  writeFileSync(join(directory, 'recording-analysis.json'), JSON.stringify(analysis, null, 2) + '\n', { flag: 'wx' });
  assert.ok(analysis.passed, 'Engine recording lost, repeated or changed source PCM; see recording-analysis.json');
}

// Keep the same from_os graph active through both source orders and finalize its take.
export async function recordSourceComparison({ api, controlTake, recorderPath, revision,
  device, directory, measurementBoundary, compare = compareMacosSources }) {
  mkdirSync(directory, { recursive: true });
  const before = await api(recorderPath);
  assert.equal(before.state, 'stopped', 'Required recording must be finalized before source diagnosis');
  await controlTake({ expectedRevision: revision,
    expectedRecording: { state: before.state, path: before.path },
    request: { action: 'record', path: join(directory, 'recording.wav'), sampleRate: 48000, encoding: 'float32' } });
  const errors = [];
  try { await compare(device, directory, { measurementBoundary }); }
  catch (error) { errors.push(error); }
  finally {
    try {
      const current = await api(recorderPath);
      if (current.state === 'failed') {
        // The recorder closes a failed take after draining its accepted prefix.
        // Preserve that terminal failure independently of its finalized PCM analysis.
        writeFileSync(join(directory, 'recording-stop.json'), JSON.stringify(current, null, 2) + '\n', { flag: 'wx' });
        errors.push(new Error(`Recorder failed: ${current.error ?? 'unknown failure'}`));
        analyseFinalizedTake(directory);
      } else {
        assert.equal(current.state, 'recording');
        const stopped = await controlTake({ expectedRevision: revision,
          expectedRecording: { state: current.state, path: current.path }, request: { action: 'stop' } });
        writeFileSync(join(directory, 'recording-stop.json'), JSON.stringify(stopped, null, 2) + '\n', { flag: 'wx' });
        assert.equal(stopped.state, 'stopped');
        assert.equal(stopped.error, null);
        analyseFinalizedTake(directory);
      }
    } catch (error) { errors.push(error); }
  }
  if (errors.length) throw new AggregateError(errors, errors.map(error => error.message).join('; '));
}
