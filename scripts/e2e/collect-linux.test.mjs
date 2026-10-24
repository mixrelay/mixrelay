import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { collectDesktopLogs } from './collect-linux.mjs';

test('Linux diagnostics preserve private editor and runtime startup logs without following links or overwriting evidence', t => {
  const home = mkdtempSync(join(tmpdir(), 'mr-logs-'));
  t.after(() => rmSync(home, { recursive: true, force: true }));
  const output = join(home, 'evidence');
  mkdirSync(output);
  collectDesktopLogs(output, home);
  const logs = join(home, '.local/state/mixrelay/logs');
  mkdirSync(logs, { recursive: true });
  writeFileSync(join(logs, 'desktop-42.log'), 'Driver runtime did not become ready\n');
  writeFileSync(join(logs, 'driver-runtime-43.log'), 'runtime started\n');
  writeFileSync(join(logs, 'unrelated.txt'), 'other');
  symlinkSync(join(logs, 'desktop-42.log'), join(logs, 'desktop-44.log'));
  collectDesktopLogs(output, home);
  assert.deepEqual(readdirSync(output).sort(), ['app-log-desktop-42.log', 'app-log-driver-runtime-43.log']);
  assert.equal(readFileSync(join(output, 'app-log-desktop-42.log'), 'utf8'), 'Driver runtime did not become ready\n');
  assert.equal(readFileSync(join(output, 'app-log-driver-runtime-43.log'), 'utf8'), 'runtime started\n');
  assert.throws(() => collectDesktopLogs(output, home), { code: 'EEXIST' });
});

test('Linux stack collection releases only its own stopped tracee after reaping GDB', () => {
  const result = spawnSync('python3', [fileURLToPath(new URL('./linux-stack.test.py', import.meta.url))], {
    encoding: 'utf8', timeout: 10000,
    env: { ...process.env, PYTHONDONTWRITEBYTECODE: '1' },
  });
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr);
});
