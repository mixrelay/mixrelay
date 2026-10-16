import assert from 'node:assert/strict';
import test from 'node:test';
import { engineOperations } from './engine-operations.mjs';

test('lost replies reuse the same mutation and reclaim bounded receipts across many edits', async () => {
  const receipts = new Map();
  let time = 0, applied = 0, acknowledged = 0;
  const client = engineOperations({
    now: () => time,
    async delay(ms) { time += ms; },
    async request(path, { method = 'GET', headers = {}, body } = {}) {
      const polling = path.startsWith('/api/v1/operations/');
      const id = polling ? path.split('/').at(-1) : headers['x-mixrelay-operation-request'];
      if (method === 'DELETE') {
        assert.ok(receipts.delete(id)); acknowledged++;
        return { status: 204, headers: {}, body: '' };
      }
      if (!polling) {
        assert.equal(method, 'PUT');
        if (!receipts.has(id)) {
          assert.ok(receipts.size < 8, 'server receipt storage exhausted');
          applied++;
          receipts.set(id, { body, lostPoll: false });
          throw new Error('reply lost after mutation was applied');
        }
        assert.equal(receipts.get(id).body, body);
        return { status: 202, headers: { 'x-mixrelay-operation': id }, body: '{}' };
      }
      const receipt = receipts.get(id);
      assert.ok(receipt);
      if (!receipt.lostPoll) { receipt.lostPoll = true; throw new Error('first poll reply lost'); }
      return { status: 200, headers: { 'x-mixrelay-operation-result': id }, body: receipt.body };
    },
  });
  for (let revision = 1; revision <= 16; revision++)
    assert.deepEqual(await client.api('graph', { revision }), { revision });
  assert.equal(applied, 16);
  assert.equal(acknowledged, 16);
  assert.equal(receipts.size, 0);
});

for (const header of ['x-mixrelay-operation', 'x-mixrelay-operation-result']) {
  test(`a foreign ${header} is never accepted or acknowledged`, async () => {
    let requests = 0;
    const client = engineOperations({
      now: () => 0,
      async delay() { assert.fail('foreign replies must fail immediately'); },
      async request() {
        requests++;
        return { status: header.endsWith('-result') ? 200 : 202,
          headers: { [header]: 'other-client-operation' }, body: '{"revision":7}' };
      },
    });
    await assert.rejects(client.api('graph', { revision: 7 }), /foreign .* operation ID/);
    assert.equal(requests, 1);
  });
}
