import assert from 'node:assert/strict';
import { randomUUID } from 'node:crypto';
import { existsSync, readFileSync, renameSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';

export function permissionObservation(directory, alive, { now = Date.now, wait = delay } = {}) {
  async function transition(phase) {
    const command = { generation: randomUUID(), phase };
    const pending = join(directory, 'permission-observation.pending.json');
    writeFileSync(pending, JSON.stringify(command), { flag: 'wx' });
    renameSync(pending, join(directory, 'permission-observation.command.json'));
    const deadline = now() + 30000;
    const acknowledgement = join(directory, 'permission-observation.ack.json');
    for (;;) {
      assert.ok(alive(), `macOS permission observer exited during ${phase}`);
      if (existsSync(acknowledgement)) {
        const state = JSON.parse(readFileSync(acknowledgement, 'utf8'));
        if (state.generation === command.generation && state.phase === phase) {
          assert.ok(Number.isFinite(Date.parse(state.at)), 'macOS observer acknowledgement requires an execution timestamp');
          return state;
        }
      }
      assert.ok(now() < deadline, `macOS permission observer did not acknowledge ${phase}`);
      await wait(100);
    }
  }
  return async measure => {
    const errors = [];
    const receipt = {};
    let result;
    try {
      receipt.paused = await transition('paused');
      result = await measure();
    } catch (error) { errors.push(error); }
    try { receipt.resumed = await transition('observing'); }
    catch (error) { errors.push(error); }
    writeFileSync(join(directory, 'native-permission-observation.json'), JSON.stringify(receipt, null, 2) + '\n', { flag: 'wx' });
    if (errors.length === 1) throw errors[0];
    if (errors.length) throw new AggregateError(errors, errors.map(error => error.message).join('; '));
    return result;
  };
}
