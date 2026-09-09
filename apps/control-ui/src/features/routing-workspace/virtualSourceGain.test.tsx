import { act, fireEvent, render, renderHook, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';

import type { AudioGraphModel } from '../../core/audio-graph/model';
import type { RuntimeGraphDocument } from '../../core/control-port/types';
import { useUndoHistory } from '../../core/history/useUndoHistory';
import { ToneNariProvider } from '../../core/theme/ToneNariProvider';
import { AudioNodeCard } from '../../core/ui/AudioNodeCard';
import { graphHistoryGroup } from './graphHistoryGroup';
import {
  audioGraphToRuntimeGraph,
  runtimeGraphToAudioGraph,
  updateRoutingNodeGain,
  updateVirtualEndpoint,
} from './model';

const document: RuntimeGraphDocument = {
  schemaVersion: 2,
  revision: 1,
  edges: [],
  nodes: [
    {
      id: 'virtual',
      label: null,
      type: 'virtual_endpoint',
      config: {
        reference: { render: true, capture: true },
        id: 'virtual',
        displayName: 'ToneNari Virtual I/O',
        defaultSampleRate: 48000,
        supportedSampleRates: [48000],
        render: { channelLayout: 'discrete', channels: 64 },
        capture: { channelLayout: 'discrete', channels: 16 },
      },
    },
  ],
};
const sourceId = 'virtual::host-source';
const sinkId = 'virtual::host-sink';

afterEach(() => vi.useRealTimers());

it('provides virtual source gain alongside endpoint options, without an output gain control', () => {
  const model = runtimeGraphToAudioGraph(document);
  const source = model.nodes.find((node) => node.id === sourceId);
  const sink = model.nodes.find((node) => node.id === sinkId);
  expect(source?.appearance.gainControl?.valueDb).toBe(0);
  expect(sink?.appearance.gainControl).toBeUndefined();
  if (!source) throw new Error('source missing');
  const onGain = vi.fn();
  const onEndpoint = vi.fn();
  render(
    <ToneNariProvider>
      <AudioNodeCard
        {...source.appearance}
        inputCount={0}
        outputCount={64}
        onSelect={vi.fn()}
        onGainChange={onGain}
        onEndpointChange={onEndpoint}
      />
    </ToneNariProvider>,
  );
  fireEvent.click(screen.getByRole('button', { name: 'Options' }));
  expect(screen.getByRole('textbox', { name: 'Device name' })).toBeInTheDocument();
  expect(screen.getAllByRole('combobox')).toHaveLength(2);
  expect(onEndpoint).not.toHaveBeenCalled();
  const slider = screen.getByRole('slider', { name: 'Source gain' });
  fireEvent.keyDown(slider, { key: 'ArrowLeft' });
  expect(onGain).toHaveBeenCalledWith(-0.5);
});

it('serializes the source gain once even when the sink is visited last or first', () => {
  const before = runtimeGraphToAudioGraph(document);
  const changed = updateRoutingNodeGain(before, sourceId, -12);
  expect(changed.nodes.find((node) => node.id === sinkId)).toBe(
    before.nodes.find((node) => node.id === sinkId),
  );
  for (const nodes of [changed.nodes, [...changed.nodes].reverse()]) {
    const saved = audioGraphToRuntimeGraph({ ...changed, nodes }, document);
    expect(saved.nodes).toHaveLength(1);
    expect(saved.nodes[0]?.config).toMatchObject({
      gainDb: -12,
      render: { channels: 64 },
      capture: { channels: 16 },
    });
    const restored = runtimeGraphToAudioGraph(JSON.parse(JSON.stringify(saved)));
    expect(
      restored.nodes.find((node) => node.id === sourceId)?.appearance.gainControl?.valueDb,
    ).toBe(-12);
  }
  const resized = updateVirtualEndpoint(changed, sinkId, {
    displayName: 'Renamed',
    renderChannels: 32,
    captureChannels: 8,
  });
  expect(audioGraphToRuntimeGraph(resized, document).nodes[0]?.config).toMatchObject({
    gainDb: -12,
    displayName: 'Renamed',
  });
});

it('coalesces virtual gain gestures and roundtrips undo/redo through the runtime document', () => {
  vi.useFakeTimers();
  const { result } = renderHook(() => useUndoHistory<AudioGraphModel, 'level'>());
  let current = runtimeGraphToAudioGraph(document);
  act(() => {
    for (const gain of [-1, -2, -6, -12]) {
      const next = updateRoutingNodeGain(current, sourceId, gain);
      const group = graphHistoryGroup(current, next, 'level');
      if (!group) throw new Error('virtual gain must have a control history group');
      result.current.recordDebounced(current, next, 'level', group);
      current = next;
      vi.advanceTimersByTime(20);
    }
  });
  const undone = result.current.undo();
  expect(
    undone?.value.nodes.find((node) => node.id === sourceId)?.appearance.gainControl?.valueDb,
  ).toBe(0);
  expect(result.current.undo()).toBeNull();
  const redone = result.current.redo();
  if (!redone) throw new Error('redo missing');
  expect(audioGraphToRuntimeGraph(redone.value, document).nodes[0]?.config).toMatchObject({
    gainDb: -12,
  });
  expect(vi.getTimerCount()).toBe(0);
});
