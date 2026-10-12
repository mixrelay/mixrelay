import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { audioMeterKey } from '../../core/audio-meter/AudioMeterStore';
import type { MeterSnapshot } from '../../core/control-port/types';
import { useRoutingMeters } from './useRoutingMeters';

const key = audioMeterKey('source', 'out');
function frame(peak = 1): MeterSnapshot {
  return {
    protocolVersion: 1,
    revision: 7,
    nodes: [{ nodeId: 'source', portId: 'out', peaks: [peak] }],
  };
}
function port() {
  return { getMeters: vi.fn<() => Promise<MeterSnapshot>>().mockResolvedValue(frame()) };
}
async function advance(milliseconds = 0) {
  await act(async () => vi.advanceTimersByTimeAsync(milliseconds));
}
function visibility(state: DocumentVisibilityState) {
  vi.spyOn(document, 'visibilityState', 'get').mockReturnValue(state);
  act(() => document.dispatchEvent(new Event('visibilitychange')));
}
beforeEach(() => {
  vi.useFakeTimers();
  vi.spyOn(document, 'visibilityState', 'get').mockReturnValue('visible');
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe('routing meter freshness', () => {
  it('accepts changing levels on the same graph revision at the 50 ms cadence', async () => {
    const source = port();
    const { result } = renderHook(() => useRoutingMeters(source));
    await advance();
    expect(result.current.levels(key)).toEqual([100]);
    source.getMeters.mockResolvedValue(frame(0.001));
    await advance(49);
    expect(source.getMeters).toHaveBeenCalledTimes(1);
    await advance(1);
    expect(source.getMeters).toHaveBeenCalledTimes(2);
    expect(result.current.levels(key)).toEqual([0]);
  });

  it('clears a failed poll immediately and waits 500 ms before retrying', async () => {
    const source = port();
    const { result } = renderHook(() => useRoutingMeters(source));
    await advance();
    expect(result.current.levels(key)).toEqual([100]);
    source.getMeters.mockRejectedValueOnce(new Error('engine unavailable'));
    await advance(50);
    expect(result.current.levels(key)).toEqual([]);
    await advance(499);
    expect(source.getMeters).toHaveBeenCalledTimes(2);
    await advance(1);
    expect(source.getMeters).toHaveBeenCalledTimes(3);
    expect(result.current.levels(key)).toEqual([100]);
  });

  it('expires the last frame after 250 ms even while the next request is hung', async () => {
    const source = port();
    const pending = Promise.withResolvers<MeterSnapshot>();
    const { result } = renderHook(() => useRoutingMeters(source));
    await advance();
    source.getMeters.mockReturnValue(pending.promise);
    await advance(249);
    expect(result.current.levels(key)).toEqual([100]);
    await advance(1);
    expect(result.current.levels(key)).toEqual([]);
    await advance(5_000);
    expect(source.getMeters).toHaveBeenCalledTimes(2);
    await act(async () => pending.resolve(frame()));
    expect(result.current.levels(key)).toEqual([]);
    source.getMeters.mockResolvedValue(frame(0.001));
    await advance(50);
    expect(source.getMeters).toHaveBeenCalledTimes(3);
    expect(result.current.levels(key)).toEqual([0]);
  });

  it('rejects a response whose own request age reaches the stale threshold', async () => {
    const source = port();
    const pending = Promise.withResolvers<MeterSnapshot>();
    source.getMeters.mockReturnValueOnce(pending.promise);
    const { result } = renderHook(() => useRoutingMeters(source));
    await advance(250);
    await act(async () => pending.resolve(frame()));
    expect(result.current.levels(key)).toEqual([]);
    await advance(50);
    expect(result.current.levels(key)).toEqual([100]);
  });

  it('refreshes the expiry deadline for each accepted frame', async () => {
    const source = port();
    const { result } = renderHook(() => useRoutingMeters(source));
    await advance(200);
    expect(source.getMeters).toHaveBeenCalledTimes(5);
    source.getMeters.mockReturnValue(new Promise(() => {}));
    await advance(249);
    expect(result.current.levels(key)).toEqual([100]);
    await advance(1);
    expect(result.current.levels(key)).toEqual([]);
  });

  it('does not treat a stale response as fresh when the wall clock moves backward', async () => {
    const source = port();
    const pending = Promise.withResolvers<MeterSnapshot>();
    source.getMeters.mockReturnValueOnce(pending.promise);
    const { result } = renderHook(() => useRoutingMeters(source));
    await advance(250);
    vi.setSystemTime(Date.now() - 60_000);
    await act(async () => pending.resolve(frame()));
    expect(result.current.levels(key)).toEqual([]);
  });
});

describe('routing meter invalidation subscriptions', () => {
  it('clears a deleted node from the next snapshot without disturbing surviving meters', async () => {
    const source = port();
    const survivingKey = audioMeterKey('processor', 'bus:1');
    const survivingNode = { nodeId: 'processor', portId: 'bus:1', peaks: [1, 0.001] };
    source.getMeters.mockResolvedValue({
      ...frame(),
      nodes: [...frame().nodes, survivingNode],
    });
    const { result } = renderHook(() => useRoutingMeters(source));
    await advance();
    const store = result.current;
    expect(store.levels(key)).toEqual([100]);
    expect(store.levels(survivingKey)).toEqual([100, 0]);
    const deletedListener = vi.fn(() => store.levels(key));
    const survivingListener = vi.fn(() => store.levels(survivingKey));
    const unsubscribeDeleted = store.subscribe(key, deletedListener);
    const unsubscribeSurviving = store.subscribe(survivingKey, survivingListener);

    source.getMeters.mockResolvedValue({ ...frame(), revision: 8, nodes: [survivingNode] });
    await advance(50);
    expect(store.levels(key)).toEqual([]);
    expect(deletedListener).toHaveBeenCalledTimes(1);
    expect(deletedListener).toHaveReturnedWith([]);
    expect(store.levels(survivingKey)).toEqual([100, 0]);
    expect(survivingListener).not.toHaveBeenCalled();
    await advance(50);
    expect(store.levels(key)).toEqual([]);
    expect(deletedListener).toHaveBeenCalledTimes(1);
    expect(store.levels(survivingKey)).toEqual([100, 0]);
    unsubscribeDeleted();
    unsubscribeSurviving();
  });

  it.each(['failure', 'stale', 'hidden', 'unmount'] as const)(
    'notifies meter subscribers when invalidated by %s',
    async (reason) => {
      const source = port();
      const { result, unmount } = renderHook(() => useRoutingMeters(source));
      await advance();
      const store = result.current;
      const listener = vi.fn(() => store.levels(key));
      const unsubscribe = store.subscribe(key, listener);
      if (reason === 'failure') {
        source.getMeters.mockRejectedValueOnce(new Error('unavailable'));
        await advance(50);
      } else if (reason === 'stale') {
        source.getMeters.mockReturnValue(new Promise(() => {}));
        await advance(250);
      } else if (reason === 'hidden') {
        visibility('hidden');
      } else {
        unmount();
      }
      expect(listener).toHaveBeenCalledTimes(1);
      expect(listener).toHaveReturnedWith([]);
      expect(store.levels(key)).toEqual([]);
      unsubscribe();
    },
  );
});

describe('routing meter visibility lifecycle', () => {
  it('does not poll while initially hidden and starts when visible', async () => {
    visibility('hidden');
    const source = port();
    const { result } = renderHook(() => useRoutingMeters(source));
    await advance(1_000);
    expect(source.getMeters).not.toHaveBeenCalled();
    visibility('visible');
    await advance();
    expect(source.getMeters).toHaveBeenCalledTimes(1);
    expect(result.current.levels(key)).toEqual([100]);
  });

  it('clears existing levels immediately on hide and pauses scheduled polling', async () => {
    const source = port();
    const { result } = renderHook(() => useRoutingMeters(source));
    await advance();
    visibility('hidden');
    expect(result.current.levels(key)).toEqual([]);
    await advance(1_000);
    expect(source.getMeters).toHaveBeenCalledTimes(1);
    visibility('visible');
    await advance();
    expect(source.getMeters).toHaveBeenCalledTimes(2);
    expect(result.current.levels(key)).toEqual([100]);
  });

  it('ignores a pending response after hide/show without overlapping requests', async () => {
    const source = port();
    const pending = Promise.withResolvers<MeterSnapshot>();
    source.getMeters.mockReturnValueOnce(pending.promise);
    const { result } = renderHook(() => useRoutingMeters(source));
    visibility('hidden');
    visibility('visible');
    await advance(100);
    expect(source.getMeters).toHaveBeenCalledTimes(1);
    await act(async () => pending.resolve(frame()));
    expect(result.current.levels(key)).toEqual([]);
    await advance(50);
    expect(source.getMeters).toHaveBeenCalledTimes(2);
    expect(result.current.levels(key)).toEqual([100]);
  });

  it('discards a response that settles while hidden and resumes only on visibility', async () => {
    const source = port();
    const pending = Promise.withResolvers<MeterSnapshot>();
    source.getMeters.mockReturnValueOnce(pending.promise);
    const { result } = renderHook(() => useRoutingMeters(source));
    visibility('hidden');
    await act(async () => pending.resolve(frame()));
    await advance(1_000);
    expect(result.current.levels(key)).toEqual([]);
    expect(source.getMeters).toHaveBeenCalledTimes(1);
    visibility('visible');
    await advance();
    expect(result.current.levels(key)).toEqual([100]);
  });
});

describe('routing meter owner lifecycle', () => {
  it('shares one pending native request across StrictMode effect replay', async () => {
    const source = port();
    const pending = Promise.withResolvers<MeterSnapshot>();
    source.getMeters.mockReturnValueOnce(pending.promise);
    const { result, unmount } = renderHook(() => useRoutingMeters(source), {
      reactStrictMode: true,
    });
    await advance(100);
    expect(source.getMeters).toHaveBeenCalledTimes(1);
    await act(async () => pending.resolve(frame()));
    expect(result.current.levels(key)).toEqual([100]);
    await advance(50);
    expect(source.getMeters).toHaveBeenCalledTimes(2);
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  });

  it('replaces the store, clears the old store and ignores late old-port responses', async () => {
    const first = port(),
      second = port();
    const pending = Promise.withResolvers<MeterSnapshot>();
    const { result, rerender } = renderHook(({ source }) => useRoutingMeters(source), {
      initialProps: { source: first },
    });
    await advance();
    const oldStore = result.current;
    first.getMeters.mockReturnValue(pending.promise);
    await advance(50);
    second.getMeters.mockResolvedValue(frame(0.001));
    rerender({ source: second });
    await advance();
    expect(result.current).not.toBe(oldStore);
    expect(oldStore.levels(key)).toEqual([]);
    expect(result.current.levels(key)).toEqual([0]);
    await act(async () => pending.resolve(frame()));
    expect(oldStore.levels(key)).toEqual([]);
    expect(result.current.levels(key)).toEqual([0]);
  });

  it('clears on unmount and neither late settlement nor visibility can revive the store', async () => {
    const source = port();
    const pending = Promise.withResolvers<MeterSnapshot>();
    const { result, unmount } = renderHook(() => useRoutingMeters(source));
    await advance();
    const oldStore = result.current;
    source.getMeters.mockReturnValue(pending.promise);
    await advance(50);
    unmount();
    expect(oldStore.levels(key)).toEqual([]);
    await act(async () => pending.resolve(frame()));
    visibility('hidden');
    visibility('visible');
    await advance(1_000);
    expect(source.getMeters).toHaveBeenCalledTimes(2);
    expect(oldStore.levels(key)).toEqual([]);
    expect(vi.getTimerCount()).toBe(0);
  });
});

it('ignores a rejected native telemetry response after the store is disposed', async () => {
  const source = port();
  const pending = Promise.withResolvers<MeterSnapshot>();
  source.getMeters.mockReturnValueOnce(pending.promise);
  const { result, unmount } = renderHook(() => useRoutingMeters(source));
  await advance();
  const store = result.current;
  unmount();
  await act(async () => pending.reject(new Error('old transport')));
  await advance(1000);
  expect(source.getMeters).toHaveBeenCalledTimes(1);
  expect(store.levels(key)).toEqual([]);
  expect(vi.getTimerCount()).toBe(0);
});
