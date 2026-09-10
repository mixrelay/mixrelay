import { MantineProvider } from '@mantine/core';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ComponentType, CSSProperties, PropsWithChildren } from 'react';
import { useState } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { mixrelayTheme } from '../theme/MixRelayProvider';
import { NodeOptionsSection } from '../ui/NodeOptionsSection';
import { AudioGraphCanvas, type AudioGraphViewport } from './AudioGraphCanvas';
import type { AudioGraphModel } from './model';

type MockItem = Record<string, unknown>;

function CanvasTestProvider({ children }: PropsWithChildren) {
  // jsdom has no layout. Mantine's floating hideDetached middleware can otherwise
  // hide an open menu once its zero-size geometry settles, racing role queries.
  // Keep real menu events/focus behavior; only geometry and transitions use test mode.
  return (
    <MantineProvider env="test" forceColorScheme="dark" theme={mixrelayTheme}>
      {children}
    </MantineProvider>
  );
}

const viewportHarness = {
  current: { x: 0, y: 0, zoom: 1 } as AudioGraphViewport,
  setViewport: vi.fn(),
  lastMoveEnd: null as ((event: Event | null, view: AudioGraphViewport) => void) | null,
};
const updateNodeInternals = vi.fn();

function MockViewportActions({ props }: { props: Record<string, unknown> }) {
  const moveEnd = props.onMoveEnd as (event: Event | null, view: AudioGraphViewport) => void;
  viewportHarness.lastMoveEnd = moveEnd;
  const pan = { x: 120, y: -30, zoom: 1.25 };
  const zoom = { ...pan, zoom: 1.75 };
  const finish = (event: Event | null, viewport: AudioGraphViewport) => {
    viewportHarness.current = { ...viewport };
    moveEnd(event, viewport);
  };
  const initialize = () => {
    const onInit = props.onInit as (instance: object) => void;
    onInit({
      getViewport: () => viewportHarness.current,
      setViewport: (viewport: AudioGraphViewport, options: unknown) => {
        viewportHarness.setViewport(viewport, options);
        finish(null, viewport);
        return Promise.resolve(true);
      },
      fitView: () => Promise.resolve(true),
      screenToFlowPosition: (position: { x: number; y: number }) => position,
    });
  };
  return (
    <>
      <button aria-label="initialize viewport" onClick={initialize} type="button" />
      <button
        aria-label="viewport moving"
        onClick={() => {
          const onMove = props.onMove as
            | ((event: Event, view: AudioGraphViewport) => void)
            | undefined;
          onMove?.(new MouseEvent('mousemove'), pan);
        }}
        type="button"
      />
      <button
        aria-label="finish viewport pan"
        onClick={() => finish(new MouseEvent('mouseup'), pan)}
        type="button"
      />
      <button
        aria-label="finish viewport zoom"
        onClick={() => finish(new WheelEvent('wheel'), zoom)}
        type="button"
      />
      <button
        aria-label="finish programmatic viewport"
        onClick={() => finish(null, zoom)}
        type="button"
      />
    </>
  );
}

function MockConnectionActions({
  edges,
  invalidConnection,
  onConnect,
  onReconnect,
  onReconnectEnd,
  validConnection,
}: {
  edges: MockItem[];
  invalidConnection: MockItem;
  onConnect: (connection: MockItem) => void;
  onReconnect: (edge: MockItem, connection: MockItem) => void;
  onReconnectEnd: (
    event: MouseEvent,
    edge: MockItem,
    handleType: string,
    state: { isValid: boolean },
  ) => void;
  validConnection: MockItem;
}) {
  const finishReconnect = (isValid: boolean) =>
    onReconnectEnd(new MouseEvent('pointerup'), edges[0] ?? {}, 'target', { isValid });
  return (
    <>
      <button aria-label="connect" onClick={() => onConnect(validConnection)} type="button" />
      <button
        aria-label="invalid connect"
        onClick={() => onConnect(invalidConnection)}
        type="button"
      />
      <button
        aria-label="reconnect"
        onClick={() => onReconnect(edges[0] ?? {}, validConnection)}
        type="button"
      />
      <button
        aria-label="invalid reconnect"
        onClick={() => onReconnect({ id: 'missing' }, invalidConnection)}
        type="button"
      />
      <button
        aria-label="finish valid reconnect"
        onClick={() => finishReconnect(true)}
        type="button"
      />
      <button
        aria-label="drop reconnect in empty space"
        onClick={() => finishReconnect(false)}
        type="button"
      />
    </>
  );
}

function MockDeletionActions({
  edges,
  nodes,
  onDelete,
  onEdgeDoubleClick,
}: {
  edges: MockItem[];
  nodes: MockItem[];
  onDelete: (selection: { edges: MockItem[]; nodes: MockItem[] }) => void;
  onEdgeDoubleClick: (event: MouseEvent, edge: MockItem) => void;
}) {
  return (
    <>
      <button
        aria-label="delete selected"
        onClick={() => onDelete({ edges, nodes: [] })}
        type="button"
      />
      <button
        aria-label="delete node"
        onClick={() => onDelete({ edges: [], nodes: [nodes[1] ?? {}] })}
        type="button"
      />
      <button
        aria-label="delete double click"
        onClick={() => onEdgeDoubleClick(new MouseEvent('dblclick'), edges[0] ?? {})}
        type="button"
      />
    </>
  );
}

function MockMoveAction({ onNodesChange }: { onNodesChange: (changes: MockItem[]) => void }) {
  return (
    <button
      aria-label="move"
      onClick={() =>
        onNodesChange([
          { id: 'processor', position: { x: 220, y: 140 }, type: 'position' },
          { id: 'source', type: 'select' },
          { id: 'missing', position: { x: 1, y: 1 }, type: 'position' },
        ])
      }
      type="button"
    />
  );
}

function useMockFlowState(initial: unknown) {
  const [state, setState] = useState(initial);
  return [state, setState, () => undefined];
}

function MockReactFlow({ children, ...props }: PropsWithChildren<Record<string, unknown>>) {
  const nodes = props.nodes as MockItem[];
  const edges = props.edges as MockItem[];
  const nodeTypes = props.nodeTypes as Record<string, ComponentType<MockItem>>;
  const AudioNode = nodeTypes.audio as ComponentType<MockItem>;
  const validConnection = {
    source: 'processor',
    sourceHandle: 'out',
    target: 'sink',
    targetHandle: 'in',
  };
  const invalidConnection = {
    source: 'source',
    sourceHandle: null,
    target: 'processor',
    targetHandle: null,
  };
  const isValidConnection = props.isValidConnection as (connection: MockItem) => boolean;
  return (
    <div
      data-connection-valid={isValidConnection(validConnection)}
      data-reconnect-valid={isValidConnection(edges[0] ?? {})}
      data-fit-view={props.fitView as boolean}
      data-min-zoom={props.minZoom as number}
      data-max-zoom={props.maxZoom as number}
      data-default-viewport={JSON.stringify(props.defaultViewport)}
      data-testid="react-flow"
    >
      {nodes.map((node, index) => (
        <div className="react-flow__node" data-id={node.id as string} key={node.id as string}>
          <AudioNode data={node.data} selected={index === 1} />
        </div>
      ))}
      <MockConnectionActions
        edges={edges}
        invalidConnection={invalidConnection}
        onConnect={props.onConnect as (connection: MockItem) => void}
        onReconnect={props.onReconnect as (edge: MockItem, connection: MockItem) => void}
        onReconnectEnd={props.onReconnectEnd as never}
        validConnection={validConnection}
      />
      <MockDeletionActions
        edges={edges}
        nodes={nodes}
        onDelete={props.onDelete as never}
        onEdgeDoubleClick={props.onEdgeDoubleClick as never}
      />
      <MockMoveAction onNodesChange={props.onNodesChange as never} />
      <MockViewportActions props={props} />
      {children}
    </div>
  );
}

vi.mock('@xyflow/react', () => ({
  Background: () => <span data-testid="background" />,
  Panel: ({ children }: PropsWithChildren) => <div>{children}</div>,
  BackgroundVariant: { Lines: 'lines' },
  ConnectionMode: { Strict: 'strict' },
  Handle: ({
    'aria-label': ariaLabel,
    id,
    type,
    style,
    ...props
  }: Record<string, string> & { style?: CSSProperties }) => (
    <button
      aria-label={ariaLabel}
      data-handle-id={id}
      data-handle-type={type}
      data-audio-node-id={props['data-audio-node-id']}
      data-audio-port-id={props['data-audio-port-id']}
      data-audio-port-side={props['data-audio-port-side']}
      style={style}
      type="button"
    />
  ),
  Position: { Left: 'left', Right: 'right' },
  NodeResizeControl: ({ position, style }: { position: string; style: CSSProperties }) => (
    <span data-resize-grip={position} style={style} />
  ),
  ReactFlow: MockReactFlow,
  useConnection: (selector: (connection: MockItem) => unknown) =>
    selector({ inProgress: false, toHandle: null }),
  useEdgesState: useMockFlowState,
  useNodesState: useMockFlowState,
  useUpdateNodeInternals: () => updateNodeInternals,
}));

function model(): AudioGraphModel {
  return {
    revision: 1,
    nodes: [
      {
        id: 'source',
        position: { x: 0, y: 0 },
        inputPorts: [],
        outputPorts: [{ id: 'out', label: 'Source output' }],
        appearance: { category: 'Source', detail: 'App', icon: 'S', title: 'Source' },
      },
      {
        id: 'processor',
        position: { x: 100, y: 0 },
        inputPorts: [{ id: 'in', label: 'Processor input' }],
        outputPorts: [{ id: 'out', label: 'Processor output' }],
        appearance: { category: 'Processor', detail: 'Gain', icon: 'P', title: 'Processor' },
      },
      {
        id: 'sink',
        position: { x: 300, y: 0 },
        inputPorts: [{ id: 'in', label: 'Sink input' }],
        outputPorts: [],
        appearance: { category: 'Output', detail: 'OBS', icon: 'O', title: 'Sink' },
      },
    ],
    edges: [
      {
        id: 'source-to-processor',
        sourceNodeId: 'source',
        sourcePortId: 'out',
        targetNodeId: 'processor',
        targetPortId: 'in',
      },
    ],
  };
}

function canvas(viewport?: AudioGraphViewport, onViewportChange = vi.fn()) {
  return (
    <CanvasTestProvider>
      <AudioGraphCanvas
        ariaLabel="Routing graph"
        model={model()}
        onModelChange={vi.fn()}
        onViewportChange={onViewportChange}
        viewport={viewport}
      />
    </CanvasTestProvider>
  );
}

function resetViewportHarness() {
  viewportHarness.current = { x: 0, y: 0, zoom: 1 };
  viewportHarness.setViewport.mockClear();
  viewportHarness.lastMoveEnd = null;
}

it('resets the resize grip translation independently of stylesheet transforms', () => {
  const { container } = render(canvas());
  const grips = container.querySelectorAll('[data-resize-grip="bottom-right"]');
  expect(grips).toHaveLength(3);
  for (const grip of grips) expect(grip).toHaveStyle({ translate: 'none' });
});

function RecorderDisclosureFixture() {
  const [opened, setOpened] = useState(false);
  return (
    <>
      <span>Transport</span>
      <NodeOptionsSection label="Options" opened={opened} onToggle={() => setOpened(!opened)}>
        <div style={{ height: 400 }}>Recording settings</div>
      </NodeOptionsSection>
    </>
  );
}

function recorderCanvas(channels: number) {
  const graph: AudioGraphModel = {
    revision: channels,
    edges: [],
    nodes: [
      {
        id: 'take',
        position: { x: 0, y: 0 },
        runtime: { type: 'recorder', config: { channels } },
        appearance: {
          title: 'Recorder',
          category: 'Output',
          icon: 'REC',
          meterChannels: channels,
          channelCount: channels,
        },
        inputPorts: Array.from({ length: channels }, (_, channelIndex) => ({
          id: `audio:${channelIndex}`,
          label: 'Recorder input',
          channelIndex,
        })),
        outputPorts: [],
      },
    ],
  };
  return (
    <CanvasTestProvider>
      <AudioGraphCanvas
        ariaLabel="Recorder graph"
        model={graph}
        onModelChange={vi.fn()}
        renderNodeContent={() => <RecorderDisclosureFixture />}
      />
    </CanvasTestProvider>
  );
}

it('aligns recorder handles with content-sized meter rows through channel edits and disclosure changes', () => {
  const view = render(recorderCanvas(2));
  const face = screen.getByRole('button', { name: 'Recorder, Output' });
  const root = face.closest('[data-face-mode]');
  expect(root?.parentElement).toHaveAttribute('data-node-type', 'recorder');
  expect(root?.parentElement?.style.width).toBe('');
  expect(root).toHaveStyle({ '--audio-node-face-height': '65px' });
  expect(screen.getByLabelText('Recorder input, CH 1')).toHaveStyle({ top: '41px' });
  expect(screen.getByLabelText('Recorder input, CH 2')).toHaveStyle({ top: '54px' });
  expect(screen.queryByText('2 CH')).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: 'Options' }));
  expect(screen.getByText('Recording settings')).toBeInTheDocument();
  expect(root).toHaveStyle({ '--audio-node-face-height': '65px' });
  expect(screen.getByLabelText('Recorder input, CH 2')).toHaveStyle({ top: '54px' });
  for (const channels of [8, 1, 2]) {
    updateNodeInternals.mockClear();
    view.rerender(recorderCanvas(channels));
    expect(root).toHaveStyle({ '--audio-node-face-height': `${39 + channels * 13}px` });
    for (let index = 0; index < channels; index += 1)
      expect(screen.getByLabelText(`Recorder input, CH ${index + 1}`)).toHaveStyle({
        top: `${41 + index * 13}px`,
      });
    expect(screen.queryByLabelText(`Recorder input, CH ${channels + 1}`)).not.toBeInTheDocument();
    expect(updateNodeInternals).toHaveBeenCalledWith('take');
  }
});

describe('AudioGraphCanvas viewport contract', () => {
  beforeEach(resetViewportHarness);

  it('keeps automatic fit for callers without a saved viewport', () => {
    render(canvas());
    expect(screen.getByTestId('react-flow')).toHaveAttribute('data-fit-view', 'true');
    expect(screen.getByTestId('react-flow')).not.toHaveAttribute('data-default-viewport');
    fireEvent.click(screen.getByRole('button', { name: 'initialize viewport' }));
    expect(viewportHarness.setViewport).not.toHaveBeenCalled();
  });

  it('restores a saved viewport without fit or a programmatic autosave echo', () => {
    const viewport = { x: -180, y: 70, zoom: 0.75 };
    const onViewportChange = vi.fn();
    render(canvas(viewport, onViewportChange));
    expect(screen.getByTestId('react-flow')).toHaveAttribute('data-fit-view', 'false');
    expect(screen.getByTestId('react-flow')).toHaveAttribute(
      'data-default-viewport',
      JSON.stringify(viewport),
    );
    fireEvent.click(screen.getByRole('button', { name: 'initialize viewport' }));
    expect(viewportHarness.setViewport).toHaveBeenCalledWith(viewport, { duration: 0 });
    fireEvent.click(screen.getByRole('button', { name: 'finish programmatic viewport' }));
    expect(onViewportChange).not.toHaveBeenCalled();
  });

  it('uses the latest external value when initialization finishes late', () => {
    const onViewportChange = vi.fn();
    const view = render(canvas({ x: 20, y: 40, zoom: 1 }, onViewportChange));
    const latest = { x: -60, y: -80, zoom: 1.5 };
    view.rerender(canvas(latest, onViewportChange));
    fireEvent.click(screen.getByRole('button', { name: 'initialize viewport' }));
    expect(viewportHarness.setViewport).toHaveBeenCalledExactlyOnceWith(latest, { duration: 0 });
    expect(onViewportChange).not.toHaveBeenCalled();
  });

  it('restores external changes but ignores equal coordinates on unrelated renders', () => {
    const onViewportChange = vi.fn();
    const view = render(canvas(undefined, onViewportChange));
    fireEvent.click(screen.getByRole('button', { name: 'initialize viewport' }));
    const restored = { x: 200, y: 300, zoom: 0.8 };
    view.rerender(canvas(restored, onViewportChange));
    view.rerender(canvas({ ...restored }, onViewportChange));
    expect(viewportHarness.setViewport).toHaveBeenCalledExactlyOnceWith(restored, { duration: 0 });
    expect(screen.getByTestId('react-flow')).toHaveAttribute('data-fit-view', 'false');
    expect(onViewportChange).not.toHaveBeenCalled();
  });

  it('reports user pan and zoom only when each gesture ends, without graph mutations', () => {
    const onViewportChange = vi.fn();
    const onModelChange = vi.fn();
    render(
      <CanvasTestProvider>
        <AudioGraphCanvas
          ariaLabel="Routing graph"
          model={model()}
          onModelChange={onModelChange}
          onViewportChange={onViewportChange}
        />
      </CanvasTestProvider>,
    );
    fireEvent.click(screen.getByRole('button', { name: 'viewport moving' }));
    expect(onViewportChange).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'finish viewport pan' }));
    expect(onViewportChange).toHaveBeenNthCalledWith(1, { x: 120, y: -30, zoom: 1.25 });
    fireEvent.click(screen.getByRole('button', { name: 'finish viewport zoom' }));
    expect(onViewportChange).toHaveBeenNthCalledWith(2, { x: 120, y: -30, zoom: 1.75 });
    expect(onViewportChange).toHaveBeenCalledTimes(2);
    expect(onModelChange).not.toHaveBeenCalled();
  });

  it('does not restore or emit again when a completed user gesture is acknowledged', () => {
    const onViewportChange = vi.fn();
    const view = render(canvas(undefined, onViewportChange));
    fireEvent.click(screen.getByRole('button', { name: 'initialize viewport' }));
    fireEvent.click(screen.getByRole('button', { name: 'finish viewport pan' }));
    const saved = { x: 120, y: -30, zoom: 1.25 };
    view.rerender(canvas(saved, onViewportChange));
    expect(viewportHarness.setViewport).not.toHaveBeenCalled();
    expect(onViewportChange).toHaveBeenCalledExactlyOnceWith(saved);
  });

  it('allows optional viewport callbacks to be omitted', () => {
    render(
      <CanvasTestProvider>
        <AudioGraphCanvas ariaLabel="Routing graph" model={model()} onModelChange={vi.fn()} />
      </CanvasTestProvider>,
    );
    expect(() =>
      fireEvent.click(screen.getByRole('button', { name: 'finish viewport pan' })),
    ).not.toThrow();
  });

  it('ignores a delayed move-end callback from an unmounted graph', () => {
    const onViewportChange = vi.fn();
    const view = render(canvas(undefined, onViewportChange));
    const delayedEnd = viewportHarness.lastMoveEnd;
    view.unmount();
    delayedEnd?.(new MouseEvent('mouseup'), { x: 500, y: 200, zoom: 1 });
    expect(onViewportChange).not.toHaveBeenCalled();
  });
});

describe('AudioGraphCanvas persisted zoom limits', () => {
  beforeEach(resetViewportHarness);

  it('accepts both persisted zoom boundaries instead of clamping to React Flow defaults', () => {
    const onViewportChange = vi.fn();
    const minimum = { x: -500, y: 240, zoom: 0.05 };
    const maximum = { x: 900, y: -120, zoom: 8 };
    const view = render(canvas(minimum, onViewportChange));
    expect(screen.getByTestId('react-flow')).toHaveAttribute('data-min-zoom', '0.05');
    expect(screen.getByTestId('react-flow')).toHaveAttribute('data-max-zoom', '8');
    fireEvent.click(screen.getByRole('button', { name: 'initialize viewport' }));
    expect(viewportHarness.setViewport).toHaveBeenLastCalledWith(minimum, { duration: 0 });
    view.rerender(canvas(maximum, onViewportChange));
    expect(viewportHarness.setViewport).toHaveBeenLastCalledWith(maximum, { duration: 0 });
    expect(onViewportChange).not.toHaveBeenCalled();
  });
});

describe('AudioGraphCanvas discovery menu lifecycle', () => {
  it('reports both add and context menu open/close to the inventory owner', async () => {
    const onOpened = vi.fn();
    render(
      <CanvasTestProvider>
        <AudioGraphCanvas
          addNodeLabel="Add node"
          ariaLabel="Routing graph"
          model={model()}
          nodeMenuItems={[{ id: 'test', label: 'Test source' }]}
          onModelChange={vi.fn()}
          onNodeMenuOpenedChange={onOpened}
        />
      </CanvasTestProvider>,
    );
    expect(onOpened).toHaveBeenLastCalledWith(false);
    fireEvent.click(screen.getByRole('button', { name: 'Add node' }));
    await waitFor(() => expect(onOpened).toHaveBeenLastCalledWith(true));
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Test source' }));
    await waitFor(() => expect(onOpened).toHaveBeenLastCalledWith(false));
    fireEvent.contextMenu(screen.getByRole('button', { name: 'Processor, Processor, Gain' }));
    await waitFor(() => expect(onOpened).toHaveBeenLastCalledWith(true));
    fireEvent.pointerDown(screen.getByRole('button', { name: 'Source, Source, App' }), {
      button: 0,
    });
    await waitFor(() => expect(onOpened).toHaveBeenLastCalledWith(false));
  });
});

describe('AudioGraphCanvas context menu wiring', () => {
  it('executes a real menu click as node deletion, including incident cables', async () => {
    const onModelChange = vi.fn();
    render(
      <CanvasTestProvider>
        <AudioGraphCanvas ariaLabel="Routing graph" model={model()} onModelChange={onModelChange} />
      </CanvasTestProvider>,
    );
    fireEvent.contextMenu(screen.getByRole('button', { name: 'Processor, Processor, Gain' }), {
      clientX: 140,
      clientY: 100,
    });
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Delete node' }));
    expect(onModelChange).toHaveBeenCalledWith(
      expect.objectContaining({ edges: [], revision: 2 }),
      'runtime',
    );
    expect(onModelChange.mock.calls[0]?.[0].nodes.map((node: { id: string }) => node.id)).toEqual([
      'source',
      'sink',
    ]);
  });

  it('executes the input-port menu as channel disconnection, not node removal', async () => {
    const onModelChange = vi.fn();
    render(
      <CanvasTestProvider>
        <AudioGraphCanvas ariaLabel="Routing graph" model={model()} onModelChange={onModelChange} />
      </CanvasTestProvider>,
    );
    fireEvent.contextMenu(screen.getByLabelText('Processor input, CH 1'), {
      clientX: 140,
      clientY: 100,
    });
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Disconnect channel' }));
    expect(onModelChange).toHaveBeenCalledWith(
      expect.objectContaining({ nodes: model().nodes, edges: [], revision: 2 }),
      'runtime',
    );
  });

  it('closes the context menu on another node pointer-down without changing the graph', async () => {
    const onModelChange = vi.fn();
    render(
      <CanvasTestProvider>
        <AudioGraphCanvas ariaLabel="Routing graph" model={model()} onModelChange={onModelChange} />
      </CanvasTestProvider>,
    );
    fireEvent.contextMenu(screen.getByRole('button', { name: 'Processor, Processor, Gain' }));
    await screen.findByRole('menu');
    fireEvent.pointerDown(screen.getByRole('button', { name: 'Source, Source, App' }), {
      button: 0,
    });
    await waitFor(() => expect(screen.queryByRole('menu')).not.toBeInTheDocument());
    expect(onModelChange).not.toHaveBeenCalled();
  });
});

describe('AudioGraphCanvas', () => {
  it('adapts React Flow interactions into predictable graph commands', () => {
    const onModelChange = vi.fn();
    render(
      <CanvasTestProvider>
        <AudioGraphCanvas ariaLabel="Routing graph" model={model()} onModelChange={onModelChange} />
      </CanvasTestProvider>,
    );

    expect(screen.getByRole('region', { name: 'Routing graph' })).toBeInTheDocument();
    expect(screen.getByTestId('react-flow')).toHaveAttribute('data-connection-valid', 'true');
    expect(screen.getByTestId('react-flow')).toHaveAttribute('data-reconnect-valid', 'true');
    expect(screen.getByLabelText('Source output, CH 1')).toHaveAttribute(
      'data-handle-type',
      'source',
    );
    expect(screen.getByLabelText('Processor input, CH 1')).toHaveAttribute(
      'data-handle-type',
      'target',
    );
    expect(screen.getByTestId('background')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Source, Source, App' }));
    fireEvent.click(screen.getByRole('button', { name: 'Processor, Processor, Gain' }));

    fireEvent.click(screen.getByRole('button', { name: 'invalid connect' }));
    fireEvent.click(screen.getByRole('button', { name: 'invalid reconnect' }));
    fireEvent.click(screen.getByRole('button', { name: 'finish valid reconnect' }));
    expect(onModelChange).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole('button', { name: 'drop reconnect in empty space' }));
    expect(onModelChange).toHaveBeenLastCalledWith(
      expect.objectContaining({ edges: [], revision: 2 }),
      'runtime',
    );

    fireEvent.click(screen.getByRole('button', { name: 'connect' }));
    expect(onModelChange).toHaveBeenLastCalledWith(
      expect.objectContaining({ revision: 2, edges: expect.arrayContaining([expect.any(Object)]) }),
      'runtime',
    );
    fireEvent.click(screen.getByRole('button', { name: 'reconnect' }));
    fireEvent.click(screen.getByRole('button', { name: 'delete selected' }));
    fireEvent.click(screen.getByRole('button', { name: 'delete node' }));
    fireEvent.click(screen.getByRole('button', { name: 'delete double click' }));
    fireEvent.click(screen.getByRole('button', { name: 'move' }));
    expect(onModelChange).toHaveBeenCalledTimes(7);
    expect(onModelChange).toHaveBeenLastCalledWith(
      expect.objectContaining({
        nodes: expect.arrayContaining([
          expect.objectContaining({ id: 'processor', position: { x: 220, y: 140 } }),
        ]),
      }),
      'layout',
    );
  });
});
