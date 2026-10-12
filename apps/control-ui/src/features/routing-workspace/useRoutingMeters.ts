import { useEffect, useMemo } from 'react';

import { AudioMeterStore } from '../../core/audio-meter/AudioMeterStore';
import type { ControlPort, MeterSnapshot } from '../../core/control-port/types';

const POLL_MS = 50;
const RETRY_MS = 500;
const STALE_MS = 250;
type MeterPort = Pick<ControlPort, 'getMeters'>;

/** Effect replay reuses the original native request and its age, never a new request. */
class MeterRequestOwner {
  private pending?: { promise: Promise<MeterSnapshot>; started: number };

  constructor(private port: MeterPort) {}

  read() {
    if (this.pending) return this.pending;
    const request = {
      started: performance.now(),
      promise: Promise.resolve().then(() => this.port.getMeters()),
    };
    this.pending = request;
    const release = () => {
      // read() reuses this owner until settlement, so this is its only pending request.
      this.pending = undefined;
    };
    void request.promise.then(release, release);
    return request;
  }
}

/** A freshness watchdog clears stalled telemetry without starting unbounded native requests. */
class MeterSession {
  private disposed = false;
  private pending = false;
  private visibilityEpoch = 0;
  private pollTimer?: ReturnType<typeof setTimeout>;
  private staleTimer?: ReturnType<typeof setTimeout>;

  constructor(
    private requests: MeterRequestOwner,
    private store: AudioMeterStore,
  ) {}

  start() {
    document.addEventListener('visibilitychange', this.visibilityChanged);
    void this.poll();
  }

  private clear() {
    clearTimeout(this.staleTimer);
    this.store.clear();
  }

  private visibilityChanged = () => {
    this.visibilityEpoch += 1;
    clearTimeout(this.pollTimer);
    this.clear();
    if (document.visibilityState === 'visible') void this.poll();
  };

  private async poll() {
    if (this.disposed || this.pending || document.visibilityState !== 'visible') return;
    this.pending = true;
    const epoch = this.visibilityEpoch;
    const request = this.requests.read();
    let delay = POLL_MS;
    try {
      const snapshot = await request.promise;
      if (this.disposed || epoch !== this.visibilityEpoch) return;
      if (performance.now() - request.started >= STALE_MS) {
        this.clear();
        return;
      }
      this.store.update(snapshot);
      clearTimeout(this.staleTimer);
      this.staleTimer = setTimeout(() => this.clear(), STALE_MS);
    } catch {
      if (!this.disposed) this.clear();
      delay = RETRY_MS;
    } finally {
      this.pending = false;
      if (!this.disposed && document.visibilityState === 'visible') {
        this.pollTimer = setTimeout(() => void this.poll(), delay);
      }
    }
  }

  dispose() {
    this.disposed = true;
    document.removeEventListener('visibilitychange', this.visibilityChanged);
    clearTimeout(this.pollTimer);
    this.clear();
  }
}

/** Each engine owner has an independent store; old replies cannot light up its replacement. */
export function useRoutingMeters(controlPort: MeterPort): AudioMeterStore {
  const owner = useMemo(
    () => ({ requests: new MeterRequestOwner(controlPort), store: new AudioMeterStore() }),
    [controlPort],
  );
  useEffect(() => {
    const session = new MeterSession(owner.requests, owner.store);
    session.start();
    return () => session.dispose();
  }, [owner]);
  return owner.store;
}
