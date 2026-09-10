import { act, renderHook } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import type { AudioGraphNode } from './model';
import { graphNodeFaceGeometry, useAudioNodeLayout } from './useAudioNodeLayout';

const updateNodeInternals = vi.fn();
vi.mock('@xyflow/react', () => ({
  useUpdateNodeInternals: () => updateNodeInternals,
}));

function node(type: 'audio_input' | 'recorder' | 'provider'): AudioGraphNode {
  return {
    id: 'node',
    position: { x: 0, y: 0 },
    runtime: { type, config: {} },
    appearance: { title: 'Node', category: 'Audio', icon: 'N', meterChannels: 2 },
    inputPorts: [{ id: 'in', label: 'Input' }],
    outputPorts: [{ id: 'out', label: 'Output' }],
  };
}

it.each(['audio_input', 'recorder', 'provider'] as const)(
  '%s fits intrinsic controls and borders without a separate resize footer',
  (type) => {
    const graphNode = node(type);
    const face = graphNodeFaceGeometry(graphNode);
    const { result } = renderHook(() =>
      useAudioNodeLayout(graphNode, { width: null, height: null }),
    );
    act(() => result.current.onContentHeightChange(200));
    expect(result.current.minHeight).toBe(face.height + 200 + 2);
    act(() => result.current.onContentHeightChange(24));
    expect(result.current.minHeight).toBe(Math.max(type === 'provider' ? 88 : 0, face.height + 26));
  },
);

it('keeps explicit plugin height without adding a footer to its port geometry', () => {
  const graphNode = { ...node('provider'), size: { width: 400, height: 350 } };
  const { result } = renderHook(() => useAudioNodeLayout(graphNode, { width: 400, height: 200 }));
  expect(result.current.ports.height).toBe(350);
  expect(graphNode.size).toEqual({ width: 400, height: 350 });
});

it('fits native editor geometry with only the header and card borders', () => {
  const { result } = renderHook(() =>
    useAudioNodeLayout(node('provider'), { width: 400, height: 200 }),
  );
  expect(result.current.ports.height).toBe(228);
});
