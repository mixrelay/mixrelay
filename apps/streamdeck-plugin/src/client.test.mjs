import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { exchange } from './client.mjs';

async function fixture(run, reply) {
  const folder = await mkdtemp(join(tmpdir(), 'mr-ipc-'));
  const path = join(folder, 'control.sock');
  const server = createServer((socket) =>
    socket.once('data', (data) => reply(socket, JSON.parse(data))),
  );
  await new Promise((resolve) => server.listen(path, resolve));
  try {
    await run(path);
  } finally {
    await new Promise((resolve) => server.close(resolve));
    await rm(folder, { recursive: true });
  }
}
test('bounded newline response supports fragmented UTF-8', async () => {
  await fixture(
    async (path) => assert.deepEqual(await exchange({ type: 'snapshot' }, path), { label: '녹음' }),
    (socket, request) => {
      assert.deepEqual(request, { id: 1, command: { type: 'snapshot' } });
      const bytes = Buffer.from('{"id":1,"status":200,"body":{"label":"녹음"}}\n');
      socket.write(bytes.subarray(0, 38));
      socket.end(bytes.subarray(38));
    },
  );
});
test('rejected and uncertain writes are sent once, never replayed', async () => {
  let received = 0;
  await fixture(
    async (path) => {
      await assert.rejects(exchange({ type: 'node' }, path), /graph_revision_stale/);
      assert.equal(received, 1);
    },
    (socket) => {
      received++;
      socket.end('{"id":1,"status":409,"body":{"code":"graph_revision_stale"}}\n');
    },
  );
  await fixture(
    async (path) => {
      await assert.rejects(exchange({ type: 'node' }, path));
      assert.equal(received, 2);
    },
    (socket) => {
      received++;
      socket.end();
    },
  );
});

test('accepted node publication is a successful acknowledgement, not an active-state promise', async () => {
  await fixture(
    async (path) =>
      assert.deepEqual(await exchange({ type: 'node' }, path), { revision: 9, state: 'pending' }),
    (socket) => socket.end('{"id":1,"status":202,"body":{"revision":9,"state":"pending"}}\n'),
  );
});
