import { act, renderHook } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { LicensingPort, LicensingStatus } from '../../core/licensing/LicensingPort';
import { useLicenseSession } from './useLicenseSession';

const free: LicensingStatus = {
  status: 'free',
  expiresAtUnix: 1_800_000_000,
  remainingDays: 0,
  configured: true,
};
const deactivated: LicensingStatus = { ...free, status: 'free', remainingDays: 0 };

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
    await act(async () => pending.resolve(free));
    await act(async () => {
      expect(await result.current.run(async () => undefined)).toBe(true);
    });
    expect(result.current.status).toBeNull();
  });
});

describe('useLicenseSession retries', () => {
  it('resets the failure count when a manual check succeeds', async () => {
    vi.useFakeTimers();
    const unreached = new Error('background controller unavailable');
    const getStatus = vi
      .fn()
      .mockRejectedValueOnce(unreached)
      .mockRejectedValueOnce(unreached)
      .mockRejectedValueOnce(unreached)
      .mockResolvedValue(free);
    const native = port(getStatus);
    const { result } = renderHook(() => useLicenseSession(native));
    await act(async () => vi.advanceTimersByTimeAsync(2_000));
    expect(getStatus).toHaveBeenCalledTimes(2);
    await act(async () => {
      expect(await result.current.run(() => Promise.resolve(free))).toBe(true);
    });
    // The next unreached check is the first of a new run: no error yet, retried in 2 s.
    await act(async () => vi.advanceTimersByTimeAsync(60_000));
    expect(getStatus).toHaveBeenCalledTimes(3);
    expect(result.current.status).toEqual(free);
    expect(result.current.error).toBeNull();
    await act(async () => vi.advanceTimersByTimeAsync(2_000));
    expect(getStatus).toHaveBeenCalledTimes(4);
  });

  it('leaves the replacement session schedule alone when an old reconciliation settles', async () => {
    vi.useFakeTimers();
    const reconcile = deferred<LicensingStatus>();
    const first = port(vi.fn().mockResolvedValueOnce(free).mockReturnValueOnce(reconcile.promise));
    const second = port(vi.fn().mockResolvedValue(free));
    const { result, rerender } = renderHook(({ value }) => useLicenseSession(value), {
      initialProps: { value: first },
    });
    await act(async () => Promise.resolve());
    let operation!: Promise<boolean>;
    act(() => {
      operation = result.current.run(() => Promise.reject<void>('rejected'));
    });
    await act(async () => Promise.resolve());
    rerender({ value: second });
    await act(async () => {
      reconcile.reject(new Error('gone'));
      expect(await operation).toBe(false);
    });
    // The replacement keeps its regular cadence: no early retry from the old session.
    await act(async () => vi.advanceTimersByTimeAsync(30_000));
    expect(second.getStatus).toHaveBeenCalledTimes(1);
    expect(result.current.error).toBeNull();
  });

  it('keeps checking after a failed manual check supersedes a background retry', async () => {
    vi.useFakeTimers();
    const backgroundRetry = deferred<LicensingStatus>();
    const getStatus = vi
      .fn()
      .mockRejectedValueOnce('storage_unavailable')
      .mockReturnValueOnce(backgroundRetry.promise)
      .mockResolvedValue(free);
    const native = port(getStatus);
    const { result } = renderHook(() => useLicenseSession(native));
    await act(async () => vi.advanceTimersByTimeAsync(2_000));
    expect(getStatus).toHaveBeenCalledTimes(2);
    await act(async () => {
      expect(await result.current.run(() => Promise.reject<void>('request_failed'))).toBe(false);
    });
    await act(async () => backgroundRetry.resolve(free));
    // The superseded retry is discarded, but checking resumes with backoff.
    expect(result.current.status).toBeNull();
    await act(async () => vi.advanceTimersByTimeAsync(4_000));
    expect(getStatus).toHaveBeenCalledTimes(3);
    expect(result.current.status).toEqual(free);
    expect(result.current.error).toBeNull();
  });
});

describe('useLicenseSession scheduling', () => {
  it('skips a timed read while an operation is busy and blocks duplicate operations', async () => {
    vi.useFakeTimers();
    const native = port(vi.fn().mockResolvedValue(free));
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
      pending.resolve(free);
      expect(await first).toBe(true);
    });
  });

  it.each(['resolve', 'reject'])(
    'discards a stale %s timed read after a mutation begins',
    async (outcome) => {
      vi.useFakeTimers();
      const timedRead = deferred<LicensingStatus>();
      const native = port(
        vi.fn().mockResolvedValueOnce(free).mockReturnValueOnce(timedRead.promise),
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
        if (outcome === 'resolve') timedRead.resolve(deactivated);
        else timedRead.reject('storage_unavailable');
      });
      expect(result.current.status).toEqual(free);
      expect(result.current.error).toBeNull();
      await act(async () => {
        mutation.resolve(free);
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
      const nextPort = port(vi.fn().mockResolvedValue(free));
      const { result, rerender } = renderHook(({ value }) => useLicenseSession(value), {
        initialProps: { value: oldPort },
      });
      rerender({ value: nextPort });
      await act(async () => Promise.resolve());
      await act(async () => {
        if (outcome === 'resolve') pending.resolve(deactivated);
        else pending.reject('storage_unavailable');
      });
      expect(result.current.status).toEqual(free);
      expect(result.current.error).toBeNull();
    },
  );

  it.each(['resolve', 'reject'])(
    'ignores a late %s mutation from the previous port',
    async (outcome) => {
      const oldPort = port(vi.fn().mockResolvedValue(free));
      const nextPort = port(vi.fn().mockResolvedValue(deactivated));
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
        if (outcome === 'resolve') pending.resolve(free);
        else pending.reject('rejected');
        expect(await operation).toBe(false);
      });
      expect(result.current.status).toEqual(deactivated);
    },
  );
});

describe('useLicenseSession reconciliation', () => {
  it('returns to the reconciled local free state after a rejected paid validation', async () => {
    const native = port(
      vi
        .fn()
        .mockResolvedValueOnce({ ...free, status: 'licensed' })
        .mockResolvedValueOnce(free),
    );
    const { result } = renderHook(() => useLicenseSession(native));
    await act(async () => Promise.resolve());
    await act(async () => {
      expect(await result.current.run(() => Promise.reject<void>('rejected'))).toBe(false);
    });
    expect(result.current.status).toEqual(free);
    expect(result.current.error).toBe('rejected');
    expect(result.current.busy).toBe(false);
  });

  it('shows a failed local reconciliation without retaining an outdated paid state', async () => {
    const native = port(
      vi
        .fn()
        .mockResolvedValueOnce({ ...free, status: 'licensed' })
        .mockRejectedValueOnce('storage_unavailable'),
    );
    const { result } = renderHook(() => useLicenseSession(native));
    await act(async () => Promise.resolve());
    await act(async () => {
      expect(await result.current.run(() => Promise.reject<void>('wrong_product'))).toBe(false);
    });
    expect(result.current.status).toBeNull();
    expect(result.current.error).toBe('storage_unavailable');
    expect(result.current.busy).toBe(false);
  });

  it('preserves the current free state and resumes local polling after a transient failure', async () => {
    vi.useFakeTimers();
    const native = port(vi.fn().mockResolvedValue(free));
    const { result } = renderHook(() => useLicenseSession(native));
    await act(async () => Promise.resolve());
    await act(async () => {
      expect(await result.current.run(() => Promise.reject<void>('unavailable'))).toBe(false);
    });
    expect(result.current.status).toEqual(free);
    expect(result.current.error).toBe('unavailable');
    await act(async () => vi.advanceTimersByTimeAsync(60_000));
    expect(native.getStatus).toHaveBeenCalledTimes(2);
    expect(result.current.error).toBeNull();
  });

  it.each(['resolve', 'reject'])(
    'ignores a late %s local reconciliation from the previous port',
    async (outcome) => {
      const pending = deferred<LicensingStatus>();
      const oldPort = port(
        vi.fn().mockResolvedValueOnce(free).mockReturnValueOnce(pending.promise),
      );
      const nextPort = port(vi.fn().mockResolvedValue(deactivated));
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
        if (outcome === 'resolve') pending.resolve(free);
        else pending.reject('storage_unavailable');
        expect(await operation).toBe(false);
      });
      expect(result.current.status).toEqual(deactivated);
    },
  );
});
