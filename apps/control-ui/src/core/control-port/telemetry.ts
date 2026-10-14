import type {
  GraphNodeControls,
  GraphRuntimeStatus,
  MeterTelemetry,
  MeterTelemetryListener,
  MixRelayStatus,
  ProcessorEditorStatus,
} from './types';

export interface ControlStateSnapshot {
  generation: number;
  status: MixRelayStatus;
  graph: GraphRuntimeStatus;
  controls: {
    revision: number;
    activeRevision: number;
    structureGeneration: number;
    graphs: { id: string; nodes: GraphNodeControls['nodes'] }[];
  } | null;
  editors: ProcessorEditorStatus[];
}
export interface ControlTelemetry extends MeterTelemetry {
  state?: ControlStateSnapshot | null;
}
export type StateListener = (
  state: ControlStateSnapshot | undefined,
  engineIncarnation?: string,
) => void;
type Listener = Partial<MeterTelemetryListener> & { state?: StateListener };
type Source = (listener: MeterTelemetryListener) => Promise<() => void>;

/** One native channel/HTTP stream per port. Every consumer shares its attempt fence,
 * incarnation, liveness and complete state cache; old reads never refresh this cache. */
export class TelemetryHub {
  private listeners = new Set<Listener>();
  private opening?: Promise<void>;
  private close?: () => void;
  private attempt = 0;
  private incarnation?: string;
  private snapshot?: ControlStateSnapshot;
  private watchdog?: ReturnType<typeof setTimeout>;
  private retry?: ReturnType<typeof setTimeout>;
  constructor(private source: Source) {}

  current() {
    return this.snapshot;
  }
  subscribeState(state: StateListener) {
    const listener = { state };
    this.listeners.add(listener);
    state(this.snapshot, this.incarnation);
    void this.start().catch(() => undefined);
    return () => this.remove(listener);
  }
  async subscribeMeters(listener: MeterTelemetryListener) {
    this.listeners.add(listener);
    try {
      await this.start();
    } catch (error) {
      this.remove(listener);
      throw error;
    }
    return () => this.remove(listener);
  }
  private remove(listener: Listener) {
    this.listeners.delete(listener);
    if (this.listeners.size) return;
    this.attempt += 1;
    this.close?.();
    this.close = undefined;
    this.opening = undefined;
    clearTimeout(this.retry);
    clearTimeout(this.watchdog);
    this.snapshot = undefined;
  }
  private receive(packet: ControlTelemetry) {
    if (packet.engineIncarnation !== this.incarnation) {
      this.incarnation = packet.engineIncarnation;
      this.snapshot = undefined;
      for (const listener of this.listeners) listener.state?.(undefined);
    }
    if (packet.state && (!this.snapshot || packet.state.generation > this.snapshot.generation)) {
      this.snapshot = packet.state;
      for (const listener of this.listeners)
        listener.state?.(packet.state, packet.engineIncarnation);
    }
    for (const listener of this.listeners) listener.window?.(packet);
    clearTimeout(this.watchdog);
    this.watchdog = setTimeout(() => this.failed(), 2000);
  }
  private failed(notifyMeters = true) {
    this.attempt += 1;
    this.close?.();
    this.close = undefined;
    this.opening = undefined;
    this.snapshot = undefined;
    clearTimeout(this.watchdog);
    for (const listener of [...this.listeners]) {
      listener.state?.(undefined);
      if (listener.window) {
        this.listeners.delete(listener);
        if (notifyMeters) listener.ended?.();
      }
    }
    clearTimeout(this.retry);
    if (this.listeners.size)
      this.retry = setTimeout(() => void this.start().catch(() => undefined), 5000);
  }
  private start(): Promise<void> {
    if (this.close) return Promise.resolve();
    if (this.opening) return this.opening;
    const attempt = ++this.attempt;
    const current = () => attempt === this.attempt;
    this.opening = this.source({
      window: (packet) => {
        if (current()) this.receive(packet);
      },
      ended: () => {
        if (current()) this.failed();
      },
    }).then(
      (close) => {
        if (!current()) {
          close();
          throw new Error('telemetry attempt ended');
        }
        this.close = close;
        this.opening = undefined;
      },
      (error: unknown) => {
        if (current()) this.failed(false);
        throw error;
      },
    );
    return this.opening;
  }
}

interface MeterBinding {
  nodeId: string;
  portId: string;
  channels: number;
  inputChannels: number | null;
}
interface PackedMeters {
  protocolVersion: 1;
  revision: number;
  /** Present when the bindings differ from the ones this stream carried last. */
  bindings?: MeterBinding[];
  samples: string;
}
interface PackedTelemetry {
  engineIncarnation: string;
  sequence: number;
  meters: PackedMeters;
  state?: ControlStateSnapshot;
}

/** Decodes one ordered stream (a native channel or an SSE response). Meter bindings are
 * sent only when they change, so each stream needs its own decoder. */
export class TelemetryDecoder {
  private bindings?: MeterBinding[];

  /** Raw frames align samples to four bytes so the webview can use Float32Array directly. */
  raw(frame: ArrayBuffer): ControlTelemetry {
    const length = new DataView(frame).getUint32(0, true);
    if (length > frame.byteLength - 4) throw new Error('invalid telemetry header');
    const metadata: PackedTelemetry = JSON.parse(
      new TextDecoder().decode(new Uint8Array(frame, 4, length)),
    );
    const offset = (length + 7) & ~3;
    return this.unpack(metadata, new Float32Array(frame, offset));
  }

  sse(data: string): ControlTelemetry {
    const metadata: PackedTelemetry = JSON.parse(data);
    const bytes = Uint8Array.from(atob(metadata.meters.samples), (character) =>
      character.charCodeAt(0),
    );
    return this.unpack(metadata, new Float32Array(bytes.buffer));
  }

  private unpack(packet: PackedTelemetry, samples: Float32Array): ControlTelemetry {
    const bindings = packet.meters.bindings ?? this.bindings;
    if (!bindings) throw new Error('meter bindings missing');
    const decoded = unpack(packet, bindings, samples);
    this.bindings = bindings;
    return decoded;
  }
}

function unpack(
  packet: PackedTelemetry,
  bindings: MeterBinding[],
  samples: Float32Array,
): ControlTelemetry {
  let offset = 0;
  const take = (count: number) => {
    if (!Number.isSafeInteger(count) || count < 0 || offset + count > samples.length)
      throw new Error('invalid meter binding');
    const values = Array.from(samples.subarray(offset, offset + count));
    offset += count;
    return values;
  };
  const nodes = bindings.map((binding) => {
    const peaks = take(binding.channels);
    const dynamics =
      binding.inputChannels === null
        ? undefined
        : {
            inputPeaks: take(binding.inputChannels),
            gainReductionDb: take(1)[0] as number,
            thresholdDb: take(1)[0] as number,
          };
    return {
      nodeId: binding.nodeId,
      portId: binding.portId,
      peaks,
      ...(dynamics ? { dynamics } : {}),
    };
  });
  if (offset !== samples.length || samples.some((value) => !Number.isFinite(value)))
    throw new Error('invalid meter samples');
  return {
    engineIncarnation: packet.engineIncarnation,
    sequence: packet.sequence,
    state: packet.state,
    meters: {
      protocolVersion: packet.meters.protocolVersion,
      revision: packet.meters.revision,
      nodes,
    },
  };
}

export function graphControls(
  state: ControlStateSnapshot | undefined,
  graphId: string,
): GraphNodeControls | undefined {
  const graph = state?.controls?.graphs.find((graph) => graph.id === graphId);
  return graph && state?.controls
    ? { graphId, revision: state.controls.revision, nodes: graph.nodes }
    : undefined;
}
