import assert from 'node:assert/strict';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { spawnLogged } from './package.mjs';

export async function startInstalledUi(os, output) {
  const directory = resolve(output);
  const log = join(directory, 'installed-ui.log');
  const child = os === 'macos'
    ? spawnLogged('bash', ['scripts/e2e/macos-ui.sh', directory], log)
    : spawnLogged('python3', ['scripts/e2e/linux-ui.py', directory], log);
  let spawnError;
  child.on('error', error => { spawnError = error; });
  const completed = new Promise(resolveExit => child.once('close', (code, signal) => resolveExit({ code, signal })));
  const alive = () => !spawnError && child.exitCode === null && child.signalCode === null;
  return {
    async ready() {
      const deadline = Date.now() + 300000;
      while (!existsSync(join(directory, 'installed-ui.ready')) && alive() && Date.now() < deadline) await delay(100);
      assert.ifError(spawnError);
      assert.ok(existsSync(join(directory, 'installed-ui.ready')), `Installed UI failed: ${readFileSync(log, 'utf8').slice(-5000)}`);
    },
    async finish() {
      writeFileSync(join(directory, 'installed-ui.finish'), 'finish\n', { flag: 'wx' });
      const result = await Promise.race([completed, delay(30000).then(() => null)]);
      if (!result) {
        child.kill('SIGTERM');
        await Promise.race([completed, delay(5000)]);
        if (alive()) child.kill('SIGKILL');
        assert.fail('Installed UI runner did not stop');
      }
      assert.deepEqual(result, { code: 0, signal: null }, `Installed UI runner failed: ${readFileSync(log, 'utf8').slice(-5000)}`);
    },
  };
}
