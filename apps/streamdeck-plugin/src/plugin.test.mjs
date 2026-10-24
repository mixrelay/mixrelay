import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { resolve } from 'node:path';
import test from 'node:test';
import { WebSocketServer } from 'ws';
import { actionIds } from './action-settings.mjs';

function showActions(socket) {
  for (const [index, action] of [...Object.values(actionIds), actionIds.volume].entries()) {
    socket.send(
      JSON.stringify({
        event: 'willAppear',
        action,
        context: `test-${index}`,
        device: 'test-device',
        payload: {
          controller: index === 4 ? 'Encoder' : 'Keypad',
          coordinates: { column: 0, row: 0 },
          isInMultiAction: false,
          settings: { graphId: 'absent-test-graph', nodeId: 'missing', operation: 'power' },
        },
      }),
    );
  }
}

test('built plugin registers all four actions and a volume encoder with Korean feedback safely', {
  timeout: 8000,
}, async () => {
  const server = new WebSocketServer({ host: '127.0.0.1', port: 0 });
  await once(server, 'listening');
  const messages = [];
  const titleReceived = Promise.withResolvers();
  const received = new Map();
  server.on('connection', (socket) => {
    socket.on('message', (bytes) => {
      const message = JSON.parse(bytes);
      messages.push(message);
      if (message.event === 'registerPlugin') showActions(socket);
      if (['setImage', 'setFeedback'].includes(message.event)) {
        received.set(message.context, message);
        if (received.size === 5) titleReceived.resolve([...received.values()]);
      }
    });
  });
  const child = spawn(
    process.execPath,
    [
      'bin/plugin.js',
      '-port',
      String(server.address().port),
      '-pluginUUID',
      'test-plugin',
      '-registerEvent',
      'registerPlugin',
      '-info',
      JSON.stringify({
        application: { version: '6.6.0', platform: 'mac', language: 'ko' },
        plugin: { uuid: 'io.mixrelay.streamdeck', version: '0.1.0.0' },
        devices: [{ id: 'test-device', name: 'Test', type: 0, size: { columns: 5, rows: 3 } }],
      }),
    ],
    { cwd: resolve('dist/io.mixrelay.streamdeck.sdPlugin'), stdio: ['ignore', 'pipe', 'pipe'] },
  );
  let stderr = '';
  child.stderr.on('data', (chunk) => {
    stderr += chunk;
  });
  try {
    const response = await Promise.race([
      titleReceived.promise,
      once(child, 'exit').then(() => {
        throw new Error(`Plugin exited: ${stderr}`);
      }),
      new Promise((_, reject) => {
        const timer = setTimeout(() => reject(new Error(`No SDK feedback: ${stderr}`)), 5000);
        timer.unref();
      }),
    ]);
    assert.equal(messages[0].event, 'registerPlugin');
    for (const message of response) {
      const value =
        message.event === 'setFeedback'
          ? message.payload.value
          : decodeURIComponent(message.payload.image.split(',')[1]);
      assert.match(value, /연결 끊김|사용 불가/);
    }
    const volumeKey = decodeURIComponent(received.get('test-2').payload.image.split(',')[1]);
    assert.match(volumeKey, />\+<\/text>/);
    assert.match(volumeKey, /translate\(52 10\) scale\(1\.666667\)/);
    assert.match(received.get('test-3').payload.image, /^data:image\/svg\+xml,/);
    assert.ok(
      messages
        .filter((message) => message.event === 'setTitle')
        .every((message) => message.payload.title === ''),
    );
    assert.equal(received.get('test-4').event, 'setFeedback');
  } finally {
    child.kill();
    for (const client of server.clients) client.terminate();
    await new Promise((done) => server.close(done));
  }
});
