import { afterEach, expect, it, vi } from 'vitest';
import type { ControlPort } from '../../core/control-port/types';
import { waitForGraphActivation } from './waitForGraphActivation';

afterEach(() => vi.useRealTimers());

it('waits for the accepted revision instead of treating submission as activation', async () => {
  vi.useFakeTimers();
  const getGraphStatus = vi
    .fn()
    .mockResolvedValueOnce({ activeRevision: 4 })
    .mockResolvedValue({ activeRevision: 5 });
  const port = { getGraphStatus } as unknown as ControlPort;
  const result = waitForGraphActivation(port, 5, () => true);
  await vi.advanceTimersByTimeAsync(25);
  await expect(result).resolves.toBe(true);
  expect(getGraphStatus).toHaveBeenCalledTimes(2);
  expect(vi.getTimerCount()).toBe(0);
});

it('stops on cancellation after a pending read without another native request', async () => {
  let resolve!: (value: unknown) => void;
  let current = true;
  const getGraphStatus = vi.fn().mockReturnValue(
    new Promise((done) => {
      resolve = done;
    }),
  );
  const result = waitForGraphActivation(
    { getGraphStatus } as unknown as ControlPort,
    5,
    () => current,
  );
  current = false;
  resolve({ activeRevision: 4 });
  await expect(result).resolves.toBe(false);
  expect(getGraphStatus).toHaveBeenCalledTimes(1);
});

it('bounds activation polling and leaves no timer after the deadline', async () => {
  vi.useFakeTimers();
  const getGraphStatus = vi.fn().mockResolvedValue({ activeRevision: 4 });
  const result = waitForGraphActivation(
    { getGraphStatus } as unknown as ControlPort,
    5,
    () => true,
  ).catch((error: unknown) => error);
  await vi.advanceTimersByTimeAsync(3000);
  expect(await result).toEqual(new Error('Graph activation timed out'));
  expect(getGraphStatus).toHaveBeenCalledTimes(120);
  expect(vi.getTimerCount()).toBe(0);
});
