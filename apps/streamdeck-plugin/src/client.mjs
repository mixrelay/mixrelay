import { createConnection } from 'node:net';
import { homedir } from 'node:os';
import { join } from 'node:path';

export const socketPath = join(
  homedir(),
  'Library/Application Support/io.mixrelay.desktop/control/control.sock',
);

const RESPONSE_LIMIT = 4 * 1024 * 1024;

function decode(frame) {
  const response = JSON.parse(frame.toString('utf8'));
  if (response.id !== 1 || !Number.isInteger(response.status)) throw new Error('Invalid response');
  if (response.status === 102) return { pending: true };
  if (response.status < 200 || response.status >= 300)
    throw new Error(response.body?.code ?? 'Control rejected');
  return { pending: false, body: response.body };
}

/** One bounded exchange. A failed write is never retried. */
export function exchange(command, path = socketPath, timeout = 35000) {
  return new Promise((resolve, reject) => {
    const socket = createConnection(path);
    let parts = [];
    let frameSize = 0;
    let settled = false;
    let timer;
    const resetTimer = () => {
      clearTimeout(timer);
      timer = setTimeout(() => finish(new Error('MixRelay did not respond')), timeout);
    };
    const finish = (error, value) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      socket.destroy();
      if (error) reject(error);
      else resolve(value);
    };
    resetTimer();
    socket.on('error', (error) => finish(error));
    socket.on('end', () => finish(new Error('MixRelay disconnected')));
    socket.on('connect', () => {
      socket.write(`${JSON.stringify({ id: 1, command })}\n`);
      resetTimer();
    });
    socket.on('data', (chunk) => {
      let offset = 0;
      while (!settled && offset < chunk.length) {
        const newline = chunk.indexOf(10, offset);
        const end = newline < 0 ? chunk.length : newline;
        const segment = chunk.subarray(offset, end);
        frameSize += segment.length;
        if (frameSize > RESPONSE_LIMIT) return finish(new Error('Response too large'));
        if (segment.length > 0) parts.push(segment);
        if (newline < 0) return;
        const frame = parts.length === 1 ? parts[0] : Buffer.concat(parts, frameSize);
        parts = [];
        frameSize = 0;
        offset = newline + 1;
        try {
          const response = decode(frame);
          resetTimer();
          if (response.pending) continue;
          finish(null, response.body);
        } catch (error) {
          finish(error);
        }
      }
    });
  });
}
