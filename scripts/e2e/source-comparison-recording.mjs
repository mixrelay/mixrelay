import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { compareMacosSources } from './macos-source-comparison.mjs';

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
      assert.equal(current.state, 'recording');
      const stopped = await controlTake({ expectedRevision: revision,
        expectedRecording: { state: current.state, path: current.path }, request: { action: 'stop' } });
      writeFileSync(join(directory, 'recording-stop.json'), JSON.stringify(stopped, null, 2) + '\n', { flag: 'wx' });
      assert.equal(stopped.state, 'stopped');
      assert.equal(stopped.error, null);
    } catch (error) { errors.push(error); }
  }
  if (errors.length) throw new AggregateError(errors, errors.map(error => error.message).join('; '));
}
