import { expect, it } from 'vitest';
import type { RuntimeGraphDocument } from '../../core/control-port/types';
import { acknowledgedNodeControl, controlChanges, rebaseControls } from './controlReconciliation';
import { runtimeGraphToAudioGraph } from './model';

const before: RuntimeGraphDocument = {
  schemaVersion: 2,
  revision: 3,
  edges: [],
  nodes: [{ id: 'gain', type: 'gain', label: 'Gain', config: { channels: 2, gainDb: 0 } }],
};

it('acknowledged controls only patch the addressed node and preserve topology/configuration', () => {
  const after = acknowledgedNodeControl(before, 'gain', { muted: true, gainDb: -0.12345 }, 4);
  expect(after.revision).toBe(4);
  expect(after.edges).toBe(before.edges);
  expect(after.nodes[0]?.config).toEqual({ channels: 2, gainDb: Math.fround(-0.12345) });
  expect(after.nodes[0]?.control).toEqual({ enabled: true, muted: true });
  expect(before.nodes[0]?.config).toEqual({ channels: 2, gainDb: 0 });
  expect(acknowledgedNodeControl(before, 'deleted', { enabled: false }, 4).nodes[0]).toBe(
    before.nodes[0],
  );
});

function first<T>(items: T[]): T {
  const item = items[0];
  if (!item) throw new Error('Missing fixture item');
  return item;
}

it('rebases only changed controls without moving nodes or discarding local mute', () => {
  const after = structuredClone(before);
  after.revision = 4;
  after.nodes[0] = {
    ...first(after.nodes),
    config: { channels: 2, gainDb: -6 },
    control: { enabled: false, muted: false },
  };
  const changes = controlChanges(before, after);
  expect(changes).toEqual([{ id: 'gain', type: 'gain', value: { enabled: false, gainDb: -6 } }]);
  const model = runtimeGraphToAudioGraph(before);
  first(model.nodes).position = { x: 120, y: 250 };
  const runtime = first(model.nodes).runtime;
  if (!runtime || !changes) throw new Error('fixture');
  runtime.control = { enabled: true, muted: true };
  const result = rebaseControls(model, changes);
  expect(first(result.nodes).position).toEqual({ x: 120, y: 250 });
  expect(first(result.nodes).runtime?.control).toEqual({ enabled: false, muted: true });
  expect(first(result.nodes).runtime?.config).toEqual({ channels: 2, gainDb: -6 });
  expect(runtime.control.enabled).toBe(true);
});

it('structural changes require explicit recovery, including deletion or node type replacement', () => {
  expect(controlChanges(before, { ...before, nodes: [] })).toBeNull();
  const after = structuredClone(before);
  first(after.nodes).type = 'recorder';
  expect(controlChanges(before, after)).toBeNull();
  after.nodes[0] = { ...first(before.nodes), config: { channels: 1, gainDb: 0 } };
  expect(controlChanges(before, after)).toBeNull();
});

it('default controls and object key order do not manufacture structural conflicts', () => {
  const after = {
    ...before,
    revision: 9,
    nodes: [
      {
        ...first(before.nodes),
        config: { gainDb: 0, channels: 2 },
        control: { enabled: true, muted: false },
      },
    ],
  };
  expect(controlChanges(before, after)).toEqual([]);
});
