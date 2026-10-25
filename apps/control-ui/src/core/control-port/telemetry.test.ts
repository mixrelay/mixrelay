import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import {
  type ControlStateSnapshot,
  type ControlTelemetry,
  graphControls,
  TelemetryDecoder,
  TelemetryHub,
} from './telemetry';
import type { MeterTelemetryListener } from './types';

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

function state(generation = 1): ControlStateSnapshot {
  return {
    generation,
    status: {
      protocolVersion: 1,
      driverState: 'ready',
      sampleRate: 48000,
      bufferFrames: 128,
      underruns: 0,
      overruns: 0,
    },
    graph: { protocolVersion: 1, activeRevision: 2, submittedRevision: 2 },
    controls: {
      revision: 2,
      activeRevision: 2,
      structureGeneration: 1,
      graphs: [{ id: 'a', nodes: [] }],
    },
    editors: [],
  };
}
function packet(generation = 1, engineIncarnation = 'a'): ControlTelemetry {
  return {
    engineIncarnation,
    sequence: generation,
    state: state(generation),
    meters: { protocolVersion: 1, revision: 2, nodes: [] },
  };
}
function source() {
  const listeners: MeterTelemetryListener[] = [];
  const close = vi.fn();
  const open = vi.fn(async (listener: MeterTelemetryListener) => {
    listeners.push(listener);
    return close;
  });
  return { listeners, close, open, hub: new TelemetryHub(open) };
}

it('shares one stream across state and meter consumers and replays state to late consumers', async () => {
  const { hub, open, listeners, close } = source();
  const receive = vi.fn();
  const stopState = hub.subscribeState(receive);
  const stopMeters = await hub.subscribeMeters({ window: vi.fn(), ended: vi.fn() });
  listeners[0]?.window(packet());
  expect(open).toHaveBeenCalledOnce();
  expect(receive).toHaveBeenLastCalledWith(state(), 'a');
  const late = vi.fn();
  const stopLate = hub.subscribeState(late);
  expect(late).toHaveBeenCalledWith(state(), 'a');
  listeners[0]?.window(packet(2));
  listeners[0]?.window(packet(1));
  expect(hub.current()?.generation).toBe(2);
  expect(graphControls(hub.current(), 'a')).toEqual({ graphId: 'a', revision: 2, nodes: [] });
  expect(graphControls(hub.current(), 'missing')).toBeUndefined();
  stopState();
  stopMeters();
  expect(close).not.toHaveBeenCalled();
  stopLate();
  expect(close).toHaveBeenCalledOnce();
  expect(vi.getTimerCount()).toBe(0);
});

it('invalidates on incarnation change and silence, then retries without accepting old attempts', async () => {
  const { hub, open, listeners, close } = source();
  const receive = vi.fn();
  const stop = hub.subscribeState(receive);
  await Promise.resolve();
  await Promise.resolve();
  listeners[0]?.window(packet(8));
  listeners[0]?.window(packet(1, 'replacement'));
  expect(hub.current()?.generation).toBe(1);
  await vi.advanceTimersByTimeAsync(2000);
  expect(hub.current()).toBeUndefined();
  expect(receive).toHaveBeenLastCalledWith(undefined, 'replacement');
  expect(close).toHaveBeenCalledOnce();
  await vi.advanceTimersByTimeAsync(5000);
  expect(open).toHaveBeenCalledTimes(2);
  listeners[0]?.window(packet(99));
  expect(hub.current()).toBeUndefined();
  listeners[1]?.window(packet(2, 'replacement'));
  expect(hub.current()?.generation).toBe(2);
  stop();
});

it('rejects refused meter subscriptions, retries state push and retires unresolved attempts', async () => {
  const open = vi.fn().mockRejectedValueOnce(new Error('offline')).mockResolvedValue(vi.fn());
  const hub = new TelemetryHub(open);
  const stop = hub.subscribeState(vi.fn());
  await expect(hub.subscribeMeters({ window: vi.fn(), ended: vi.fn() })).rejects.toThrow('offline');
  await vi.advanceTimersByTimeAsync(5000);
  expect(open).toHaveBeenCalledTimes(2);
  stop();
  const pending = Promise.withResolvers<() => void>();
  const close = vi.fn();
  const late = new TelemetryHub(() => pending.promise);
  const end = late.subscribeState(vi.fn());
  end();
  pending.resolve(close);
  await Promise.resolve();
  await Promise.resolve();
  expect(close).toHaveBeenCalledOnce();
});

it('ignores ended and rejected opens from retired attempts and retries a failed retry', async () => {
  const pending = Promise.withResolvers<() => void>();
  const listeners: MeterTelemetryListener[] = [];
  const source = vi.fn((listener: MeterTelemetryListener) => {
    listeners.push(listener);
    return pending.promise;
  });
  const hub = new TelemetryHub(source);
  const stop = hub.subscribeState(vi.fn());
  stop();
  listeners[0]?.ended();
  pending.reject(new Error('retired opening'));
  await vi.advanceTimersByTimeAsync(0);
  const refusing = vi.fn().mockRejectedValue(new Error('refused'));
  const retrying = new TelemetryHub(refusing);
  const close = retrying.subscribeState(vi.fn());
  await vi.advanceTimersByTimeAsync(5000);
  expect(refusing).toHaveBeenCalledTimes(2);
  close();
});

function packed() {
  const samples = new Float32Array([0.5, 0.25, 0.75, 6, -12]);
  const metadata = {
    engineIncarnation: 'a',
    sequence: 1,
    state: state(),
    meters: {
      protocolVersion: 1,
      revision: 2,
      bindings: [
        { nodeId: 'n', portId: 'out', channels: 2, inputChannels: 1 },
        { nodeId: 'empty', portId: 'in', channels: 0, inputChannels: null },
      ],
      samples: btoa(String.fromCharCode(...new Uint8Array(samples.buffer))),
    },
  };
  const json = new TextEncoder().encode(
    JSON.stringify({ ...metadata, meters: { ...metadata.meters, samples: '' } }),
  );
  const offset = (json.length + 7) & ~3;
  const frame = new ArrayBuffer(offset + samples.byteLength);
  new DataView(frame).setUint32(0, json.length, true);
  new Uint8Array(frame, 4, json.length).set(json);
  new Float32Array(frame, offset).set(samples);
  return { metadata, frame };
}

it('decodes base64 SSE and raw ArrayBuffer identically, retaining actual dynamics', () => {
  const { metadata, frame } = packed();
  const decoded = new TelemetryDecoder().raw(frame);
  expect(decoded).toEqual(new TelemetryDecoder().sse(JSON.stringify(metadata)));
  expect(decoded.meters.nodes[0]).toEqual({
    nodeId: 'n',
    portId: 'out',
    peaks: [0.5, 0.25],
    dynamics: { inputPeaks: [0.75], gainReductionDb: 6, thresholdDb: -12 },
  });
  expect(decoded.state).toEqual(state());
});

it('reuses the bindings of a stream until they change and refuses a stream joined without them', () => {
  const { metadata } = packed();
  const decoder = new TelemetryDecoder();
  const first = decoder.sse(JSON.stringify(metadata));
  const { bindings: _omitted, ...meters } = metadata.meters;
  const steady = { ...metadata, sequence: 2, meters };
  expect(decoder.sse(JSON.stringify(steady)).meters.nodes).toEqual(first.meters.nodes);
  expect(() => new TelemetryDecoder().sse(JSON.stringify(steady))).toThrow(
    'meter bindings missing',
  );
});

it('rejects truncated headers, invalid bindings, excess samples and nonfinite values', () => {
  expect(() => new TelemetryDecoder().raw(new ArrayBuffer(3))).toThrow();
  const { metadata, frame } = packed();
  new DataView(frame).setUint32(0, frame.byteLength, true);
  expect(() => new TelemetryDecoder().raw(frame)).toThrow('invalid telemetry header');
  for (const channels of [-1, 0.5, 99, 0]) {
    const invalid = structuredClone(metadata);
    const binding = invalid.meters.bindings[0];
    if (binding) binding.channels = channels;
    expect(() => new TelemetryDecoder().sse(JSON.stringify(invalid))).toThrow();
  }
  metadata.meters.samples = btoa(
    String.fromCharCode(...new Uint8Array(new Float32Array([Number.NaN, 0, 0, 0, 0]).buffer)),
  );
  expect(() => new TelemetryDecoder().sse(JSON.stringify(metadata))).toThrow(
    'invalid meter samples',
  );
});
