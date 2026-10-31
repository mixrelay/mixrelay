import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type {
  MidiAction,
  MidiBinding,
  MidiCommand,
  MidiControl,
  MidiDevice,
  MidiMode,
  MidiPort,
  MidiStatus,
} from './MidiPort';

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}
function object(value: unknown): Record<string, unknown> {
  if (!isObject(value)) throw new Error('Invalid MIDI response');
  return value;
}
function string(value: unknown): string {
  if (typeof value !== 'string') throw new Error('Invalid MIDI text');
  return value;
}
function array(value: unknown): unknown[] {
  if (!Array.isArray(value)) throw new Error('Invalid MIDI list');
  return value;
}
function integer(value: unknown, max: number): number {
  if (typeof value !== 'number' || !Number.isInteger(value) || value < 0 || value > max)
    throw new Error('Invalid MIDI number');
  return value;
}
function device(value: unknown): MidiDevice {
  const entry = object(value);
  return { id: string(entry.id), name: string(entry.name) };
}
function control(value: unknown): MidiControl {
  const entry = object(value);
  if (entry.kind !== 'note' && entry.kind !== 'cc') throw new Error('Invalid MIDI kind');
  return {
    device: device(entry.device),
    channel: integer(entry.channel, 15),
    kind: entry.kind,
    number: integer(entry.number, 127),
  };
}
function target(value: unknown) {
  const entry = object(value);
  return { graphId: string(entry.graphId), nodeId: string(entry.nodeId) };
}
export function midiAction(value: unknown): MidiAction {
  if (
    value === 'power' ||
    value === 'mute' ||
    value === 'volume' ||
    value === 'record' ||
    value === 'pause' ||
    value === 'stop'
  )
    return value;
  throw new Error('Invalid MIDI action');
}
export function midiMode(value: unknown): MidiMode {
  if (value === 'absolute' || value === 'twos_complement' || value === 'offset64') return value;
  throw new Error('Invalid MIDI mode');
}
function gain(value: unknown): number {
  if (typeof value !== 'number' || !Number.isFinite(value) || value < -60 || value > 12)
    throw new Error('Invalid MIDI range');
  return value;
}
function binding(value: unknown): MidiBinding {
  const entry = object(value);
  if (typeof entry.stepDb !== 'number' || ![0.5, 1, 2, 3].includes(entry.stepDb))
    throw new Error('Invalid MIDI step');
  return {
    control: control(entry.control),
    target: target(entry.target),
    action: midiAction(entry.action),
    mode: midiMode(entry.mode),
    stepDb: entry.stepDb,
    graphName: string(entry.graphName),
    nodeName: string(entry.nodeName),
    minDb: gain(entry.minDb),
    maxDb: gain(entry.maxDb),
  };
}
function learn(value: unknown): MidiStatus['learn'] {
  const entry = object(value);
  if (entry.state === 'captured') return { state: 'captured', control: control(entry.control) };
  if (entry.state === 'idle' || entry.state === 'waiting') return { state: entry.state };
  throw new Error('Invalid MIDI learn state');
}
export function midiStatus(value: unknown): MidiStatus {
  const entry = object(value);
  const targets: MidiStatus['targets'] = [];
  if (entry.snapshot !== null)
    for (const value of array(object(entry.snapshot).graphs)) {
      const graph = object(value);
      const graphId = string(graph.id);
      const graphName = string(graph.name);
      for (const value of array(graph.nodes)) {
        const node = object(value);
        const nodeId = string(node.nodeId);
        targets.push({
          target: { graphId, nodeId },
          label: `${graphName} / ${node.label === null ? '' : string(node.label)}`,
        });
      }
    }
  if (typeof entry.mapMode !== 'boolean') throw new Error('Invalid MIDI map mode');
  return {
    mapMode: entry.mapMode,
    targetsAvailable: entry.snapshot !== null,
    bindings: array(entry.bindings).map(binding),
    devices: array(entry.devices).map(device),
    unavailable: entry.unavailable === null ? null : string(entry.unavailable),
    learn: learn(entry.learn),
    targets,
  };
}
export class TauriMidiPort implements MidiPort {
  constructor(
    private invokeCommand: (
      name: string,
      args?: Record<string, unknown>,
    ) => Promise<unknown> = invoke,
    private listenToggle: (listener: () => void) => Promise<() => void> = (listener) =>
      listen('midi-map-toggle', listener),
  ) {}
  subscribeMapToggle(listener: () => void) {
    let active = true;
    let stop: (() => void) | undefined;
    void this.listenToggle(() => {
      if (active) listener();
    })
      .then((unsubscribe) => {
        if (active) stop = unsubscribe;
        else unsubscribe();
      })
      .catch((error: unknown) => console.error('MIDI menu listener failed', error));
    return () => {
      active = false;
      stop?.();
    };
  }
  async command(name: MidiCommand, args?: Record<string, unknown>): Promise<MidiStatus> {
    return midiStatus(await this.invokeCommand(name, args));
  }
}
export const localMidiPort: MidiPort | null =
  '__TAURI_INTERNALS__' in window ? new TauriMidiPort() : null;
