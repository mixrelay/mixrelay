import { createConnection } from 'node:net';
import { homedir } from 'node:os';
import { join } from 'node:path';

export const socketPath = join(
  homedir(),
  'Library/Application Support/io.mixrelay.desktop/control/control.sock',
);

function decode(chunks) {
  const response = JSON.parse(Buffer.concat(chunks).toString('utf8'));
  if (response.id !== 1 || !Number.isInteger(response.status)) throw new Error('Invalid response');
  if (response.status < 200 || response.status >= 300)
    throw new Error(response.body?.code ?? 'Control rejected');
  return response.body;
}

/** One bounded exchange. A failed write is never retried. */
export function exchange(command, path = socketPath, timeout = 35000) {
  return new Promise((resolve, reject) => {
    const socket = createConnection(path);
    const chunks = [];
    let size = 0;
    let settled = false;
    const finish = (error, value) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      socket.destroy();
      if (error) reject(error);
      else resolve(value);
    };
    const timer = setTimeout(() => finish(new Error('MixRelay did not respond')), timeout);
    socket.on('error', (error) => finish(error));
    socket.on('end', () => finish(new Error('MixRelay disconnected')));
    socket.on('connect', () => socket.write(`${JSON.stringify({ id: 1, command })}\n`));
    socket.on('data', (chunk) => {
      size += chunk.length;
      if (size > 4 * 1024 * 1024) return finish(new Error('Response too large'));
      chunks.push(chunk);
      if (!chunk.includes(10)) return;
      try {
        finish(null, decode(chunks));
      } catch (error) {
        finish(error);
      }
    });
  });
}
