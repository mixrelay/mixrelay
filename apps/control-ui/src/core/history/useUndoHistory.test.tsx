import { act, renderHook } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { useUndoHistory } from './useUndoHistory';

afterEach(() => vi.useRealTimers());

describe('undo gesture merging boundaries', () => {
  it('merges a short gesture but keeps different operation kinds and late gestures separate', () => {
    vi.useFakeTimers();
    const { result } = renderHook(() => useUndoHistory<number, 'gain' | 'layout'>());
    act(() => {
      result.current.record(0, 1, 'gain', 'fader');
      result.current.record(1, 2, 'gain', 'fader');
    });
    expect(result.current.undo()).toEqual({ kind: 'gain', value: 0 });
    expect(result.current.redo()).toEqual({ kind: 'gain', value: 2 });
    act(() => result.current.record(2, 3, 'gain', 'fader'));
    expect(result.current.undo()).toEqual({ kind: 'gain', value: 2 });
    act(() => {
      result.current.record(2, 4, 'gain', 'fader');
      vi.advanceTimersByTime(601);
      result.current.record(4, 5, 'gain', 'fader');
      result.current.record(5, 6, 'layout', 'fader');
    });
    expect(result.current.undo()).toEqual({ kind: 'layout', value: 5 });
    expect(result.current.undo()).toEqual({ kind: 'gain', value: 4 });
    expect(result.current.undo()).toEqual({ kind: 'gain', value: 2 });
  });

  it('limits immediate and debounced history while retaining the latest 100 edits', () => {
    vi.useFakeTimers();
    const { result } = renderHook(() => useUndoHistory<number, 'gain'>());
    act(() => {
      for (let index = 0; index < 101; index++) result.current.record(index, index + 1, 'gain');
    });
    for (let index = 100; index >= 1; index--) {
      expect(result.current.undo()).toEqual({ kind: 'gain', value: index });
    }
    expect(result.current.undo()).toBeNull();
    act(() => {
      result.current.clear();
      for (let index = 0; index < 101; index++) {
        result.current.recordDebounced(index, index + 1, 'gain', `gesture-${index}`, 10);
        vi.advanceTimersByTime(10);
      }
    });
    for (let index = 100; index >= 1; index--)
      expect(result.current.undo()).toEqual({ kind: 'gain', value: index });
    expect(result.current.undo()).toBeNull();
  });
});

describe('pending history rebasing and cleanup', () => {
  it('rebases both undo and redo entries as well as a live pending gesture', () => {
    vi.useFakeTimers();
    const { result } = renderHook(() => useUndoHistory<number, 'gain' | 'layout'>());
    act(() => {
      result.current.record(0, 1, 'gain');
      result.current.record(1, 2, 'layout');
    });
    expect(result.current.undo()).toEqual({ kind: 'layout', value: 1 });
    act(() => result.current.recordDebounced(1, 3, 'gain', 'fader'));
    act(() => result.current.rebase((value) => value + 10));
    expect(result.current.undo()).toEqual({ kind: 'gain', value: 11 });
    expect(result.current.undo()).toEqual({ kind: 'gain', value: 10 });
    expect(result.current.redo()).toEqual({ kind: 'gain', value: 11 });
  });

  it('flushes a pending gesture before changing its group or operation kind', () => {
    vi.useFakeTimers();
    const { result } = renderHook(() => useUndoHistory<number, 'gain' | 'layout'>());
    act(() => {
      result.current.recordDebounced(0, 1, 'gain', 'a');
      result.current.recordDebounced(1, 2, 'gain', 'a');
      result.current.recordDebounced(2, 3, 'gain', 'b');
      result.current.recordDebounced(3, 4, 'layout', 'b');
    });
    expect(result.current.undo()).toEqual({ kind: 'layout', value: 3 });
    expect(result.current.undo()).toEqual({ kind: 'gain', value: 2 });
    expect(result.current.undo()).toEqual({ kind: 'gain', value: 0 });
  });

  it('cancels a queued commit on clear and on unmount', () => {
    vi.useFakeTimers();
    const { result, unmount } = renderHook(() => useUndoHistory<number, 'gain'>());
    act(() => {
      result.current.recordDebounced(0, 1, 'gain', 'fader');
      result.current.clear();
      vi.advanceTimersByTime(500);
    });
    expect(result.current.undo()).toBeNull();
    expect(result.current.redo()).toBeNull();
    act(() => result.current.recordDebounced(1, 2, 'gain', 'fader'));
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  });
});
