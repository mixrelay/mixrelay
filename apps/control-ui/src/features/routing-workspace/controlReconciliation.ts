import type { AudioGraphModel } from '../../core/audio-graph/model';
import type {
  NodeControlRequest,
  RuntimeGraphDocument,
  RuntimeGraphNode,
} from '../../core/control-port/types';

type Change = { id: string; type: string; value: Omit<NodeControlRequest, 'expectedRevision'> };
const gainTypes = new Set([
  'gain',
  'virtual_endpoint',
  'audio_device_input',
  'application_capture',
]);

/** Reflect an acknowledged absolute control command without rereading or resubmitting the graph. */
export function acknowledgedNodeControl(
  document: RuntimeGraphDocument,
  nodeId: string,
  change: Omit<NodeControlRequest, 'expectedRevision'>,
  revision: number,
): RuntimeGraphDocument {
  const { gainDb, ...control } = change;
  return {
    ...document,
    revision,
    nodes: document.nodes.map((node) =>
      node.id !== nodeId
        ? node
        : {
            ...node,
            control: { enabled: true, muted: false, ...node.control, ...control },
            config:
              gainDb === undefined ? node.config : { ...config(node), gainDb: Math.fround(gainDb) },
          },
    ),
  };
}

function ordered(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(ordered);
  if (value && typeof value === 'object')
    return Object.fromEntries(
      Object.entries(value)
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([key, item]) => [key, ordered(item)]),
    );
  return value;
}

function config(node: RuntimeGraphNode): Record<string, unknown> {
  return node.config && typeof node.config === 'object' ? { ...node.config } : {};
}

function changedGain(old: RuntimeGraphNode, node: RuntimeGraphNode) {
  const gain = config(node).gainDb ?? 0;
  return gainTypes.has(node.type) && gain !== (config(old).gainDb ?? 0) && typeof gain === 'number'
    ? gain
    : undefined;
}

function structural(document: RuntimeGraphDocument) {
  return ordered({
    ...document,
    revision: 0,
    nodes: document.nodes.map((node) => {
      const { control: _control, ...rest } = node;
      if (!gainTypes.has(node.type)) return rest;
      const value = config(node);
      delete value.gainDb;
      return { ...rest, config: value };
    }),
  });
}

/** Only an exact topology/config match permits external controls to rebase edit history. */
export function controlChanges(
  before: RuntimeGraphDocument,
  after: RuntimeGraphDocument,
): Change[] | null {
  if (JSON.stringify(structural(before)) !== JSON.stringify(structural(after))) return null;
  const previous = new Map(before.nodes.map((node) => [node.id, node]));
  return after.nodes.flatMap((node) => {
    const old = previous.get(node.id);
    if (!old) return [];
    const value: Change['value'] = {};
    for (const key of ['enabled', 'muted'] as const) {
      const defaultValue = key === 'enabled';
      if ((old.control?.[key] ?? defaultValue) !== (node.control?.[key] ?? defaultValue))
        value[key] = node.control?.[key] ?? defaultValue;
    }
    const gain = changedGain(old, node);
    if (gain !== undefined) value.gainDb = gain;
    return Object.keys(value).length ? [{ id: node.id, type: node.type, value }] : [];
  });
}

/** Preserve every layout/topology edit while replacing only externally changed control fields. */
export function rebaseControls(
  model: AudioGraphModel,
  changes: readonly Change[],
): AudioGraphModel {
  const byId = new Map(changes.map((change) => [change.id, change]));
  return {
    ...model,
    nodes: model.nodes.map((node) => {
      const runtime = node.runtime;
      const change = byId.get(runtime?.binding?.nodeId ?? node.id);
      if (!runtime || !change || runtime.type !== change.type) return node;
      const { gainDb, ...control } = change.value;
      const appearance = { ...node.appearance };
      let config = runtime.config;
      if (gainDb !== undefined && config && typeof config === 'object') {
        config = { ...config, gainDb };
        if (appearance.gainControl)
          appearance.gainControl = { ...appearance.gainControl, valueDb: gainDb };
        if (runtime.type === 'gain') appearance.detail = `${gainDb.toFixed(1)} dB`;
      }
      return {
        ...node,
        appearance,
        runtime: {
          ...runtime,
          config,
          control: {
            enabled: true,
            muted: false,
            ...runtime.control,
            ...control,
          },
        },
      };
    }),
  };
}
