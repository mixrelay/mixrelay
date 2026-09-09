import type { MeterSnapshot } from '../control-port/types';

const EMPTY_LEVELS: readonly number[] = [];
type Listener = () => void;

export class AudioMeterStore {
  readonly #levels = new Map<string, readonly number[]>();
  readonly #listeners = new Map<string, Set<Listener>>();

  /** Invalid telemetry must not keep displaying the last positive signal. */
  public clear(): void {
    const keys = [...this.#levels.keys()];
    this.#levels.clear();
    for (const key of keys) this.#notify(key);
  }

  public update(snapshot: MeterSnapshot): void {
    const present = new Set<string>();
    for (const node of snapshot.nodes) {
      const key = audioMeterKey(node.nodeId, node.portId);
      const levels = node.peaks.map(peakToPercent);
      present.add(key);
      if (!sameLevels(this.#levels.get(key), levels)) {
        this.#levels.set(key, levels);
        this.#notify(key);
      }
    }
    for (const key of this.#levels.keys()) {
      if (!present.has(key)) {
        this.#levels.delete(key);
        this.#notify(key);
      }
    }
  }

  public levels(key: string): readonly number[] {
    return this.#levels.get(key) ?? EMPTY_LEVELS;
  }

  public subscribe(key: string, listener: Listener): () => void {
    const listeners = this.#listeners.get(key) ?? new Set<Listener>();
    listeners.add(listener);
    this.#listeners.set(key, listeners);
    return () => {
      listeners.delete(listener);
      if (listeners.size === 0) this.#listeners.delete(key);
    };
  }

  #notify(key: string): void {
    for (const listener of this.#listeners.get(key) ?? []) listener();
  }
}

export function audioMeterKey(nodeId: string, portId: string): string {
  return `${encodeURIComponent(nodeId)}:${encodeURIComponent(portId)}`;
}

export function emptyAudioMeterLevels(): readonly number[] {
  return EMPTY_LEVELS;
}

function peakToPercent(peak: number): number {
  if (!Number.isFinite(peak) || peak <= 0) return 0;
  const decibels = 20 * Math.log10(peak);
  return Math.max(0, Math.min(100, ((decibels + 60) / 60) * 100));
}

function sameLevels(current: readonly number[] | undefined, next: readonly number[]): boolean {
  return (
    current?.length === next.length &&
    next.every((level, index) => Math.abs(level - (current[index] ?? 0)) < 0.1)
  );
}
