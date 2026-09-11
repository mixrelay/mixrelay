import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { once } from 'node:events';
import { resolve } from 'node:path';
import test from 'node:test';
import { WebSocketServer } from 'ws';

test('built plugin registers with official SDK and renders an unavailable target safely', {
  timeout: 8000,
}, async () => {
  const server = new WebSocketServer({ host: '127.0.0.1', port: 0 });
  await once(server, 'listening');
  const messages = [];
  const titleReceived = Promise.withResolvers();
  server.on('connection', (socket) => {
    socket.on('message', (bytes) => {
      const message = JSON.parse(bytes);
      messages.push(message);
      if (message.event === 'registerPlugin')
        socket.send(
          JSON.stringify({
            event: 'willAppear',
            action: 'io.mixrelay.streamdeck.node',
            context: 'test-key',
            device: 'test-device',
            payload: {
              controller: 'Keypad',
              coordinates: { column: 0, row: 0 },
              isInMultiAction: false,
              settings: { graphId: 'absent-test-graph', nodeId: 'missing', operation: 'power' },
            },
          }),
        );
      if (message.event === 'setTitle') titleReceived.resolve(message);
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
        application: { version: '6.6.0', platform: 'mac', language: 'en' },
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
    assert.match(response.payload.title, /Disconnected|Unavailable/);
  } finally {
    child.kill();
    for (const client of server.clients) client.terminate();
    await new Promise((done) => server.close(done));
  }
});
