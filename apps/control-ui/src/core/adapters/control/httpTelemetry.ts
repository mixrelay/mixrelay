import type { MeterTelemetryListener } from '../../ports/control/TelemetryPort';
import { TelemetryDecoder } from './telemetry';

class EventParser {
  private buffer = '';
  private decoder = new TextDecoder();
  push(chunk: Uint8Array) {
    this.buffer += this.decoder.decode(chunk, { stream: true });
    if (this.buffer.length > 4 * 1024 * 1024) throw new Error('telemetry event too large');
    const events: string[] = [];
    let end = this.buffer.indexOf('\n\n');
    while (end !== -1) {
      const event = this.buffer.slice(0, end);
      this.buffer = this.buffer.slice(end + 2);
      const data = event
        .split('\n')
        .filter((line) => line.startsWith('data:'))
        .map((line) => line.slice(5).trimStart())
        .join('\n');
      if (data) events.push(data);
      end = this.buffer.indexOf('\n\n');
    }
    return events;
  }
}

async function pump(
  reader: ReadableStreamDefaultReader<Uint8Array>,
  active: () => boolean,
  receive: (data: string) => void,
) {
  const parser = new EventParser();
  while (active()) {
    const chunk = await reader.read();
    if (chunk.done) throw new Error('telemetry ended');
    if (active()) for (const data of parser.push(chunk.value)) receive(data);
  }
}

/** Same-origin Vite bridge passes SSE unchanged over the engine's existing Unix socket. */
export async function openHttpTelemetry(
  origin: string,
  request: typeof fetch,
  listener: MeterTelemetryListener,
): Promise<() => void> {
  const abort = new AbortController();
  const timeout = setTimeout(() => abort.abort(), 5000);
  try {
    const response = await request(`${origin}/api/v1/telemetry`, { signal: abort.signal });
    if (!response.ok || !response.body) throw new Error('telemetry unavailable');
    const reader = response.body.getReader();
    let active = true;
    const close = () => {
      active = false;
      abort.abort();
      void reader.cancel().catch(() => undefined);
    };
    await new Promise<void>((resolve, reject) => {
      let started = false;
      const decoder = new TelemetryDecoder();
      const receive = (data: string) => {
        const packet = decoder.sse(data);
        const engine = response.headers.get('X-MixRelay-Engine');
        if (engine && engine !== packet.engineIncarnation)
          throw new Error('telemetry engine changed');
        if (!started && !packet.state) throw new Error('missing initial snapshot');
        listener.window(packet);
        started = true;
        clearTimeout(timeout);
        resolve();
      };
      void pump(reader, () => active, receive).catch((error: unknown) => {
        if (!active) return;
        close();
        if (!started) reject(error);
        else listener.ended();
      });
    });
    return close;
  } catch (error) {
    abort.abort();
    throw error;
  } finally {
    clearTimeout(timeout);
  }
}
