import { act, renderHook } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { LicensingPort, LicensingStatus } from '../../core/licensing/LicensingPort';
import { useLicenseSession } from './useLicenseSession';

const trial: LicensingStatus = {
  status: 'trial',
  expiresAtUnix: 1_800_000_000,
  remainingDays: 8,
  configured: true,
};
const expired: LicensingStatus = { ...trial, status: 'expired', remainingDays: 0 };

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

function port(getStatus: LicensingPort['getStatus']): LicensingPort {
  return {
    getStatus,
    activate: vi.fn(),
    validate: vi.fn(),
    deactivate: vi.fn(),
    openCheckout: vi.fn(),
  };
}

afterEach(() => vi.useRealTimers());

describe('useLicenseSession without a native port', () => {
  it('keeps a browser session unlicensed and treats non-error failures as unknown', async () => {
    const { result } = renderHook(() => useLicenseSession(null));
    await act(async () => {
      expect(await result.current.run(async () => undefined)).toBe(true);
      expect(await result.current.run(() => Promise.reject<void>({ reason: 'secret' }))).toBe(
        false,
      );
      expect(await result.current.run(() => Promise.reject<void>('rejected'))).toBe(false);
    });
    expect(result.current.status).toBeNull();
    expect(result.current.error).toBe('rejected');
  });

  it('does not accept a late response after a port is removed', async () => {
    const pending = deferred<LicensingStatus>();
    const initialPort = port(vi.fn().mockReturnValue(pending.promise));
    const { result, rerender } = renderHook(({ value }) => useLicenseSession(value), {
      initialProps: { value: initialPort as LicensingPort | null },
    });
    rerender({ value: null });
    await act(async () => pending.resolve(trial));
    await act(async () => {
      expect(await result.current.run(async () => undefined)).toBe(true);
    });
    expect(result.current.status).toBeNull();
  });
});

describe('useLicenseSession scheduling', () => {
  it('skips a timed read while an operation is busy and blocks duplicate operations', async () => {
    vi.useFakeTimers();
    const native = port(vi.fn().mockResolvedValue(trial));
    const pending = deferred<LicensingStatus>();
    const action = vi.fn().mockReturnValue(pending.promise);
    const { result } = renderHook(() => useLicenseSession(native));
    await act(async () => Promise.resolve());
    let first!: Promise<boolean>;
    let second!: Promise<boolean>;
    act(() => {
      first = result.current.run(action);
      second = result.current.run(action);
    });
    expect(await second).toBe(false);
    await act(async () => vi.advanceTimersByTimeAsync(60_000));
    expect(native.getStatus).toHaveBeenCalledTimes(1);
    expect(action).toHaveBeenCalledTimes(1);
    await act(async () => {
      pending.resolve(trial);
      expect(await first).toBe(true);
    });
  });

  it.each(['resolve', 'reject'])(
    'discards a stale %s timed read after a mutation begins',
    async (outcome) => {
      vi.useFakeTimers();
      const timedRead = deferred<LicensingStatus>();
      const native = port(
        vi.fn().mockResolvedValueOnce(trial).mockReturnValueOnce(timedRead.promise),
      );
      const mutation = deferred<LicensingStatus>();
      const { result } = renderHook(() => useLicenseSession(native));
      await act(async () => Promise.resolve());
      await act(async () => vi.advanceTimersByTimeAsync(60_000));
      let operation!: Promise<boolean>;
      act(() => {
        operation = result.current.run(() => mutation.promise);
      });
      await act(async () => {
        if (outcome === 'resolve') timedRead.resolve(expired);
        else timedRead.reject('storage_unavailable');
      });
      expect(result.current.status).toEqual(trial);
      expect(result.current.error).toBeNull();
      await act(async () => {
        mutation.resolve(trial);
        await operation;
      });
    },
  );
});

describe('useLicenseSession cancellation', () => {
  it.each(['resolve', 'reject'])(
    'ignores a late %s initial read from the previous port',
    async (outcome) => {
      const pending = deferred<LicensingStatus>();
      const oldPort = port(vi.fn().mockReturnValue(pending.promise));
      const nextPort = port(vi.fn().mockResolvedValue(trial));
      const { result, rerender } = renderHook(({ value }) => useLicenseSession(value), {
        initialProps: { value: oldPort },
      });
      rerender({ value: nextPort });
      await act(async () => Promise.resolve());
      await act(async () => {
        if (outcome === 'resolve') pending.resolve(expired);
        else pending.reject('storage_unavailable');
      });
      expect(result.current.status).toEqual(trial);
      expect(result.current.error).toBeNull();
    },
  );

  it.each(['resolve', 'reject'])(
    'ignores a late %s mutation from the previous port',
    async (outcome) => {
      const oldPort = port(vi.fn().mockResolvedValue(trial));
      const nextPort = port(vi.fn().mockResolvedValue(expired));
      const pending = deferred<LicensingStatus>();
      const { result, rerender } = renderHook(({ value }) => useLicenseSession(value), {
        initialProps: { value: oldPort },
      });
      await act(async () => Promise.resolve());
      let operation!: Promise<boolean>;
      act(() => {
        operation = result.current.run(() => pending.promise);
      });
      rerender({ value: nextPort });
      await act(async () => {
        if (outcome === 'resolve') pending.resolve(trial);
        else pending.reject('rejected');
        expect(await operation).toBe(false);
      });
      expect(result.current.status).toEqual(expired);
    },
  );
});

describe('useLicenseSession reconciliation', () => {
  it.each(['resolve', 'reject'])(
    'ignores a late %s local reconciliation from the previous port',
    async (outcome) => {
      const pending = deferred<LicensingStatus>();
      const oldPort = port(
        vi.fn().mockResolvedValueOnce(trial).mockReturnValueOnce(pending.promise),
      );
      const nextPort = port(vi.fn().mockResolvedValue(expired));
      const { result, rerender } = renderHook(({ value }) => useLicenseSession(value), {
        initialProps: { value: oldPort },
      });
      await act(async () => Promise.resolve());
      let operation!: Promise<boolean>;
      act(() => {
        operation = result.current.run(() => Promise.reject<void>('rejected'));
      });
      await act(async () => Promise.resolve());
      expect(oldPort.getStatus).toHaveBeenCalledTimes(2);
      rerender({ value: nextPort });
      await act(async () => {
        if (outcome === 'resolve') pending.resolve(trial);
        else pending.reject('storage_unavailable');
        expect(await operation).toBe(false);
      });
      expect(result.current.status).toEqual(expired);
    },
  );
});
