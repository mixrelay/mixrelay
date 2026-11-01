import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { systemClock, timerScheduler } from './timers';

beforeEach(() => vi.useFakeTimers({ now: 1_000 }));
afterEach(() => vi.useRealTimers());

it('runs a task after its delay unless cancelled, using the timers installed at call time', () => {
  const ran = vi.fn();
  const skipped = vi.fn();
  timerScheduler.after(100, ran);
  const cancel = timerScheduler.after(100, skipped);
  cancel();
  vi.advanceTimersByTime(99);
  expect(ran).not.toHaveBeenCalled();
  vi.advanceTimersByTime(1);
  expect(ran).toHaveBeenCalledOnce();
  expect(skipped).not.toHaveBeenCalled();
  cancel();
});

it('reads the current wall clock', () => {
  expect(systemClock.now()).toBe(1_000);
  vi.advanceTimersByTime(25);
  expect(systemClock.now()).toBe(1_025);
});
