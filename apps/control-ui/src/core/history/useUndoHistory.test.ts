import { act, renderHook } from '@testing-library/react';
import { useEffect } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { useUndoHistory } from './useUndoHistory';

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe('live control history debouncing', () => {
  it('commits 500 live values on one control as one undo after 400 ms idle', () => {
    const { result } = renderHook(() => useUndoHistory<number, string>());
    act(() => {
      for (let value = 1; value <= 500; value += 1) {
        result.current.recordDebounced(value - 1, value, 'level', 'input:gain');
        vi.advanceTimersByTime(1);
      }
      vi.advanceTimersByTime(398);
    });
    expect(vi.getTimerCount()).toBe(1);
    act(() => vi.advanceTimersByTime(1));
    expect(vi.getTimerCount()).toBe(0);
    expect(result.current.undo()).toEqual({ kind: 'level', value: 0 });
    expect(result.current.undo()).toBeNull();
    expect(result.current.redo()).toEqual({ kind: 'level', value: 500 });
  });

  it('separates two controls and a later gesture on the first control', () => {
    const { result } = renderHook(() => useUndoHistory<number, string>());
    act(() => {
      result.current.recordDebounced(0, 1, 'level', 'compressor:threshold');
      vi.advanceTimersByTime(20);
      result.current.recordDebounced(1, 2, 'level', 'compressor:ratio');
      vi.advanceTimersByTime(20);
      result.current.recordDebounced(2, 3, 'level', 'compressor:threshold');
    });
    expect(result.current.undo()?.value).toBe(2);
    expect(result.current.undo()?.value).toBe(1);
    expect(result.current.undo()?.value).toBe(0);
    expect(result.current.undo()).toBeNull();
  });

  it('flushes an unfinished gesture for immediate undo and cancels its timer', () => {
    const { result } = renderHook(() => useUndoHistory<number, string>());
    act(() => result.current.recordDebounced(0, 42, 'level', 'input:gain'));
    expect(result.current.undo()).toEqual({ kind: 'level', value: 0 });
    expect(vi.getTimerCount()).toBe(0);
    act(() => vi.advanceTimersByTime(1000));
    expect(result.current.undo()).toBeNull();
    expect(result.current.redo()?.value).toBe(42);
  });

  it('keeps separate gestures separated after an idle boundary and invalidates stale redo', () => {
    const { result } = renderHook(() => useUndoHistory<number, string>());
    act(() => {
      result.current.recordDebounced(0, 1, 'level', 'gain');
      vi.advanceTimersByTime(400);
      result.current.recordDebounced(1, 2, 'level', 'gain');
    });
    expect(result.current.undo()?.value).toBe(1);
    act(() => result.current.recordDebounced(1, 3, 'level', 'gain'));
    expect(result.current.redo()).toBeNull();
    expect(result.current.undo()?.value).toBe(1);
    expect(result.current.undo()?.value).toBe(0);
  });
});

describe('history boundaries and disposal', () => {
  it('places structural edits after a pending control gesture', () => {
    const { result } = renderHook(() => useUndoHistory<number, string>());
    act(() => {
      result.current.recordDebounced(0, 1, 'level', 'gain');
      result.current.record(1, 2, 'remove-node');
    });
    expect(result.current.undo()).toEqual({ kind: 'remove-node', value: 1 });
    expect(result.current.undo()).toEqual({ kind: 'level', value: 0 });
  });

  it('does not merge different kinds even if a caller reuses its group identifier', () => {
    const { result } = renderHook(() => useUndoHistory<number, string>());
    act(() => {
      result.current.recordDebounced(0, 1, 'gain', 'node');
      result.current.recordDebounced(1, 2, 'threshold', 'node');
    });
    expect(result.current.undo()).toEqual({ kind: 'threshold', value: 1 });
    expect(result.current.undo()).toEqual({ kind: 'gain', value: 0 });
  });

  it('clears pending and committed entries when the owning control port changes', () => {
    const { result, rerender } = renderHook(
      ({ controlPort }) => {
        const history = useUndoHistory<number, string>();
        useEffect(() => {
          void controlPort;
          history.clear();
        }, [controlPort, history.clear]);
        return history;
      },
      { initialProps: { controlPort: {} } },
    );
    act(() => {
      result.current.record(0, 1, 'graph');
      result.current.recordDebounced(1, 2, 'level', 'gain');
    });
    rerender({ controlPort: {} });
    expect(vi.getTimerCount()).toBe(0);
    act(() => vi.advanceTimersByTime(400));
    expect(result.current.undo()).toBeNull();
    expect(result.current.redo()).toBeNull();
  });

  it('removes a pending timer on unmount instead of committing in the background', () => {
    const { result, unmount } = renderHook(() => useUndoHistory<number, string>());
    act(() => result.current.recordDebounced(0, 1, 'level', 'gain'));
    expect(vi.getTimerCount()).toBe(1);
    unmount();
    expect(vi.getTimerCount()).toBe(0);
    act(() => vi.advanceTimersByTime(500));
    expect(vi.getTimerCount()).toBe(0);
  });
});
