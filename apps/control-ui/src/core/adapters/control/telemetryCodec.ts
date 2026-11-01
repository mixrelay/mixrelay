import type { ControlStateSnapshot, ControlTelemetry } from '../../domain/control/state';

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
