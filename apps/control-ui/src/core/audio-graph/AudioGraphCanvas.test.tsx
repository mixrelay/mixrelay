import { MantineProvider } from '@mantine/core';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import type { ComponentType, CSSProperties, PropsWithChildren } from 'react';
import { useState } from 'react';
import { assert, beforeEach, describe, expect, it, vi } from 'vitest';
import { AudioMeterStore } from '../audio-meter/AudioMeterStore';
import { mixrelayTheme } from '../theme/MixRelayProvider';
import { NodeOptionsSection } from '../ui/NodeOptionsSection';
import { useNodeOptionsState } from '../ui/nodeOptionsState';
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
const connectionHarness: MockItem = { inProgress: false, toHandle: null };
const previewHarness = { enabled: false, sourcePortId: 'out', targetPortId: 'in', zoom: 1 };
const resizeHarness = { height: 480 };

beforeEach(() => {
  connectionHarness.inProgress = false;
  connectionHarness.toHandle = null;
  previewHarness.enabled = false;
  previewHarness.sourcePortId = 'out';
  previewHarness.targetPortId = 'in';
  previewHarness.zoom = 1;
});

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

function applyMockNodeDimensions(node: MockItem, changes: MockItem[]) {
  const change = changes.find(
    (candidate) => candidate.type === 'dimensions' && candidate.id === node.id,
  );
  if (!change?.dimensions) return node;
  const dimensions = change.dimensions as { width: number; height: number };
  return {
    ...node,
    measured: dimensions,
    ...(change.setAttributes === true || change.setAttributes === 'width'
      ? { width: dimensions.width }
      : {}),
    ...(change.setAttributes === true || change.setAttributes === 'height'
      ? { height: dimensions.height }
      : {}),
  };
}

function useMockFlowState(initial: unknown) {
  const [state, setState] = useState(initial);
  const applyChanges = (changes: MockItem[]) =>
    setState((current: unknown) =>
      Array.isArray(current)
        ? (current as MockItem[]).map((node) => applyMockNodeDimensions(node, changes))
        : current,
    );
  return [state, setState, applyChanges];
}

function mockConnectionNode(id: string, side: 'source' | 'target', pitch: number) {
  return {
    id,
    internals: {
      handleBounds: {
        [side]: Array.from({ length: 4 }, (_, index) => ({
          id: `${side === 'source' ? 'out' : 'in'}-${index + 1}`,
          y: index * pitch,
          height: 8,
        })),
      },
    },
  };
}

function MockReactFlow({ children, ...props }: PropsWithChildren<Record<string, unknown>>) {
  const nodes = props.nodes as MockItem[];
  const edges = props.edges as MockItem[];
  const nodeTypes = props.nodeTypes as Record<string, ComponentType<MockItem>>;
  const AudioNode = nodeTypes.audio as ComponentType<MockItem>;
  const validConnection = {
    source: 'processor',
    sourceHandle: previewHarness.sourcePortId,
    target: 'sink',
    targetHandle: previewHarness.targetPortId,
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
      data-pan-activation-key={props.panActivationKeyCode as string}
      data-testid="react-flow"
    >
      {nodes.map((node, index) => (
        <div
          className="react-flow__node"
          data-id={node.id as string}
          data-flow-height={node.height as number | undefined}
          data-flow-measured-height={
            (node.measured as MockItem | undefined)?.height as number | undefined
          }
          key={node.id as string}
        >
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
      <button
        aria-label="resize source"
        onClick={() =>
          (props.onNodesChange as (changes: MockItem[]) => void)([
            {
              id: 'source',
              type: 'dimensions',
              dimensions: { width: 300, height: resizeHarness.height },
              setAttributes: true,
            },
          ])
        }
        type="button"
      />
      <MockViewportActions props={props} />
      {previewHarness.enabled && (
        <svg aria-label="Connection preview">
          {(() => {
            const ConnectionLine = props.connectionLineComponent as ComponentType<MockItem>;
            return (
              <ConnectionLine
                fromHandle={{ id: previewHarness.sourcePortId }}
                fromNode={mockConnectionNode('processor', 'source', 20)}
                fromPosition="right"
                fromX={10}
                fromY={20}
                toHandle={{ id: previewHarness.targetPortId }}
                toNode={mockConnectionNode('sink', 'target', 26)}
                toPosition="left"
                toX={100}
                toY={40}
              />
            );
          })()}
        </svg>
      )}
      <button className="react-flow__pane" type="button" onClick={props.onPaneClick as () => void}>
        Graph background
      </button>
      {children}
    </div>
  );
}

vi.mock('@xyflow/react', () => ({
  Background: () => <span data-testid="background" />,
  Panel: ({ children }: PropsWithChildren) => <div>{children}</div>,
  BackgroundVariant: { Lines: 'lines' },
  ConnectionMode: { Strict: 'strict' },
  getBezierPath: ({ sourceX, sourceY, targetX, targetY }: Record<string, number>) => [
    `M${sourceX},${sourceY}L${targetX},${targetY}`,
  ],
  Handle: ({ id, type, position: _position, ...props }: Record<string, unknown>) => (
    <button
      {...props}
      data-handle-id={id as string}
      data-handle-type={type as string}
      type="button"
    />
  ),
  Position: { Left: 'left', Right: 'right' },
  NodeResizeControl: ({
    position,
    style,
    minHeight,
  }: {
    position: string;
    style: CSSProperties;
    minHeight: number;
  }) => <span data-resize-grip={position} data-min-height={minHeight} style={style} />,
  ReactFlow: MockReactFlow,
  useConnection: (selector: (connection: MockItem) => unknown) => selector(connectionHarness),
  useEdgesState: useMockFlowState,
  useNodesState: useMockFlowState,
  useUpdateNodeInternals: () => updateNodeInternals,
  useViewport: () => ({ zoom: previewHarness.zoom }),
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

it('keeps Options and the same gain slider mounted across external control revisions', async () => {
  const initial = model();
  const source = initial.nodes[0];
  assert(source);
  source.appearance.gainControl = { label: 'Input gain', valueDb: 0 };
  const props = { ariaLabel: 'Graph', onModelChange: vi.fn(), onNodeGainChange: vi.fn() };
  const view = render(<AudioGraphCanvas {...props} model={initial} />, {
    wrapper: CanvasTestProvider,
  });
  const options = screen.getByRole('button', { name: 'Options' });
  fireEvent.click(options);
  const slider = screen.getByRole('slider', { name: 'Input gain' });
  let revision = initial.revision;
  for (const valueDb of [-0.5, 0, -1, 0]) {
    revision += 1;
    view.rerender(
      <AudioGraphCanvas
        {...props}
        model={{
          ...initial,
          revision,
          nodes: initial.nodes.map((node) => ({
            ...node,
            appearance: {
              ...node.appearance,
              ...(node.id === 'source' ? { gainControl: { label: 'Input gain', valueDb } } : {}),
            },
          })),
        }}
      />,
    );
    await waitFor(() => expect(slider).toHaveAttribute('aria-valuenow', String(valueDb)));
    expect(screen.getByRole('slider', { name: 'Input gain' })).toBe(slider);
    expect(options).toHaveAttribute('aria-expanded', 'true');
  }
  fireEvent.click(screen.getByRole('button', { name: 'Graph background' }));
  expect(options).toHaveAttribute('aria-expanded', 'false');
});

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

it.each([
  ['MacIntel', 'Meta'],
  ['Linux x86_64', 'Control'],
])('uses the displayed platform modifier for canvas panning on %s', (platform, modifier) => {
  const identity = vi.spyOn(navigator, 'platform', 'get').mockReturnValue(platform);
  try {
    render(canvas());
    expect(screen.getByTestId('react-flow')).toHaveAttribute('data-pan-activation-key', modifier);
  } finally {
    identity.mockRestore();
  }
});

it.each(['input', 'output'] as const)(
  'shows the exact drag-target channel on the %s side and preserves anchors through Options',
  async (side) => {
    const graph = model();
    const processor = graph.nodes[1];
    assert(processor);
    processor.inputPorts = Array.from({ length: 64 }, (_, index) => ({
      id: `in-${index}`,
      label: `Input ${index + 1}`,
      channelIndex: index,
    }));
    processor.outputPorts = Array.from({ length: 64 }, (_, index) => ({
      id: `out-${index}`,
      label: `Output ${index + 1}`,
      channelIndex: index,
    }));
    processor.appearance.gainControl = { label: 'Input gain', valueDb: 0 };
    connectionHarness.inProgress = true;
    connectionHarness.toHandle = {
      nodeId: 'processor',
      id: `${side === 'input' ? 'in' : 'out'}-63`,
    };
    const view = render(
      <AudioGraphCanvas
        ariaLabel="Graph"
        model={graph}
        onModelChange={vi.fn()}
        onNodeGainChange={vi.fn()}
      />,
      { wrapper: CanvasTestProvider },
    );
    expect(await screen.findByRole('tooltip')).toHaveTextContent('CH 64');
    expect(view.container.querySelectorAll('[data-connection-target]')).toHaveLength(1);
    const handles = Array.from(
      view.container.querySelectorAll<HTMLElement>('[data-audio-node-id="processor"]'),
    );
    expect(handles).toHaveLength(128);
    const anchors = handles.map((handle) => handle.style.top);
    const options = screen.getByRole('button', { name: 'Options' });
    fireEvent.click(options);
    expect(screen.getByRole('slider', { name: 'Input gain' })).toBeVisible();
    expect(handles.map((handle) => handle.style.top)).toEqual(anchors);
    fireEvent.click(options);
    expect(handles.map((handle) => handle.style.top)).toEqual(anchors);
  },
);

it('renders and updates separate source and sink channel mini bars', async () => {
  const graph = model();
  const source = graph.nodes[0];
  const sink = graph.nodes[2];
  assert(source && sink);
  source.appearance.meterBanks = [{ channels: 2, nodeId: 'source', portId: 'from_os' }];
  sink.appearance.meterBanks = [{ channels: 2, nodeId: 'sink', portId: 'to_os' }];
  const store = new AudioMeterStore();
  store.update({
    nodes: [
      { nodeId: 'source', portId: 'from_os', peaks: [1, 0] },
      { nodeId: 'sink', portId: 'to_os', peaks: [0, 1] },
    ],
    protocolVersion: 1,
    revision: graph.revision,
  });
  const view = render(
    <AudioGraphCanvas ariaLabel="Graph" meterStore={store} model={graph} onModelChange={vi.fn()} />,
    { wrapper: CanvasTestProvider },
  );
  const sourceBank = view.container.querySelector('[data-id="source"] [data-meter-port="from_os"]');
  const sinkBank = view.container.querySelector('[data-id="sink"] [data-meter-port="to_os"]');
  expect(sourceBank?.querySelectorAll('[data-orientation="horizontal"]')).toHaveLength(2);
  expect(sinkBank?.querySelectorAll('[data-orientation="horizontal"]')).toHaveLength(2);
  expect(sourceBank?.querySelector('[title="Source channel 1: 100%"]')).not.toBeNull();
  expect(sinkBank?.querySelector('[title="Sink channel 2: 100%"]')).not.toBeNull();

  store.update({
    nodes: [
      { nodeId: 'source', portId: 'from_os', peaks: [0, 1] },
      { nodeId: 'sink', portId: 'to_os', peaks: [1, 0] },
    ],
    protocolVersion: 1,
    revision: graph.revision,
  });
  await waitFor(() => {
    expect(sourceBank?.querySelector('[title="Source channel 2: 100%"]')).not.toBeNull();
    expect(sinkBank?.querySelector('[title="Sink channel 1: 100%"]')).not.toBeNull();
  });
});

function selectPortRectangle(
  start: { x: number; y: number },
  end: { x: number; y: number },
  additive = false,
) {
  const pane = screen.getByRole('button', { name: 'Graph background' });
  for (const [type, point] of [
    ['pointerdown', start],
    ['pointerup', end],
  ] as const) {
    fireEvent(
      pane,
      Object.assign(
        new MouseEvent(type, {
          bubbles: true,
          button: 0,
          clientX: point.x,
          clientY: point.y,
          shiftKey: additive,
        }),
        { pointerId: 1, pointerType: 'mouse', isPrimary: true },
      ),
    );
  }
}

it('maps 64 box-selected outputs to 16 Shift-selected inputs in one graph edit', () => {
  const graph = model();
  const processor = graph.nodes[1];
  const sink = graph.nodes[2];
  assert(processor && sink);
  processor.inputPorts = [];
  processor.outputPorts = Array.from({ length: 64 }, (_, index) => ({
    id: index === 0 ? 'out' : `out-${index}`,
    label: `Output ${index + 1}`,
  }));
  sink.inputPorts = Array.from({ length: 16 }, (_, index) => ({
    id: index === 0 ? 'in' : `in-${index}`,
    label: `Input ${index + 1}`,
  }));
  graph.nodes = [processor, sink];
  graph.edges = [];
  const onChange = vi.fn();
  const view = render(
    <AudioGraphCanvas ariaLabel="Graph" model={graph} onModelChange={onChange} />,
    { wrapper: CanvasTestProvider },
  );
  const handles = view.container.querySelectorAll<HTMLElement>('[data-audio-port-side]');
  for (const [index, handle] of handles.entries()) {
    const input = handle.dataset.audioPortSide === 'input';
    vi.spyOn(handle, 'getBoundingClientRect').mockReturnValue(
      new DOMRect(input ? 100 : 10, (input ? index - 64 : index) * 3 + 1, 2, 2),
    );
  }
  selectPortRectangle({ x: 0, y: 0 }, { x: 20, y: 200 });
  expect(view.container.querySelectorAll('[data-port-selected]')).toHaveLength(64);
  selectPortRectangle({ x: 90, y: 0 }, { x: 110, y: 50 }, true);
  expect(view.container.querySelectorAll('[data-port-selected]')).toHaveLength(80);
  fireEvent.click(screen.getByRole('button', { name: 'connect' }));
  expect(onChange).toHaveBeenCalledOnce();
  const [changed, kind] = onChange.mock.calls[0] as [AudioGraphModel, string];
  expect(kind).toBe('runtime');
  expect(changed.edges).toHaveLength(16);
  expect(changed.edges.map((edge) => [edge.sourcePortId, edge.targetPortId])).toEqual(
    Array.from({ length: 16 }, (_, index) => [
      index === 0 ? 'out' : `out-${index}`,
      index === 0 ? 'in' : `in-${index}`,
    ]),
  );
  expect(view.container.querySelectorAll('[data-port-selected]')).toHaveLength(0);
});

it('previews the released target when one source and several target ports are selected', () => {
  const graph = model();
  const sink = graph.nodes[2];
  assert(sink);
  sink.inputPorts.push({ id: 'in-2', label: 'Sink input 2' });
  previewHarness.enabled = true;
  previewHarness.targetPortId = 'in-2';
  const onChange = vi.fn();
  const props = { ariaLabel: 'Graph', model: graph, onModelChange: onChange };
  const view = render(<AudioGraphCanvas {...props} />, { wrapper: CanvasTestProvider });
  const sinkInputs = view.container.querySelectorAll<HTMLElement>(
    '[data-audio-node-id="sink"][data-audio-port-side="input"]',
  );
  expect(sinkInputs).toHaveLength(2);
  sinkInputs.forEach((handle, index) => {
    vi.spyOn(handle, 'getBoundingClientRect').mockReturnValue(
      new DOMRect(100, 10 + index * 20, 2, 2),
    );
  });
  selectPortRectangle({ x: 90, y: 0 }, { x: 110, y: 50 });
  expect(view.container.querySelectorAll('[data-port-selected]')).toHaveLength(2);
  view.rerender(<AudioGraphCanvas {...props} />);
  expect(view.container.querySelectorAll('.react-flow__connection-path')).toHaveLength(1);
  expect(view.container.querySelector('.react-flow__connection-path')).toHaveAttribute(
    'd',
    'M10,20L100,40',
  );
  fireEvent.click(screen.getByRole('button', { name: 'connect' }));
  expect(onChange).toHaveBeenCalledOnce();
  const [changed] = onChange.mock.calls[0] as [AudioGraphModel];
  expect(changed.edges.at(-1)).toMatchObject({ sourcePortId: 'out', targetPortId: 'in-2' });
});

it.each([true, false])(
  'keeps the dragged source and target paired with selected target group %s',
  (selectTargets) => {
    const graph = model();
    const processor = graph.nodes[1];
    const sink = graph.nodes[2];
    assert(processor && sink);
    processor.inputPorts = [];
    processor.outputPorts = Array.from({ length: 4 }, (_, index) => ({
      id: `out-${index + 1}`,
      label: `Output ${index + 1}`,
    }));
    sink.inputPorts = Array.from({ length: 4 }, (_, index) => ({
      id: `in-${index + 1}`,
      label: `Input ${index + 1}`,
    }));
    graph.nodes = [processor, sink];
    graph.edges = [];
    previewHarness.enabled = true;
    previewHarness.sourcePortId = 'out-3';
    previewHarness.targetPortId = 'in-2';
    previewHarness.zoom = 0.6;
    const onChange = vi.fn();
    const props = { ariaLabel: 'Graph', model: graph, onModelChange: onChange };
    const view = render(<AudioGraphCanvas {...props} />, { wrapper: CanvasTestProvider });
    const handles = view.container.querySelectorAll<HTMLElement>('[data-audio-port-side]');
    for (const handle of handles) {
      const portIndex = Number(handle.dataset.audioPortId?.split('-')[1]) - 1;
      const input = handle.dataset.audioPortSide === 'input';
      vi.spyOn(handle, 'getBoundingClientRect').mockReturnValue(
        new DOMRect(input ? 100 : 10, 10 + portIndex * 20, 2, 2),
      );
    }
    selectPortRectangle({ x: 0, y: 0 }, { x: 20, y: 80 });
    if (selectTargets) selectPortRectangle({ x: 90, y: 0 }, { x: 110, y: 80 }, true);
    expect(view.container.querySelectorAll('[data-port-selected]')).toHaveLength(
      selectTargets ? 8 : 4,
    );
    view.rerender(<AudioGraphCanvas {...props} />);
    expect(view.container.querySelectorAll('.react-flow__connection-path')).toHaveLength(3);
    expect(
      Array.from(view.container.querySelectorAll('.react-flow__connection-path')).map((path) =>
        path.getAttribute('d'),
      ),
    ).toContain('M10,20L100,40');
    expect(
      Array.from(view.container.querySelectorAll('.react-flow__connection-path')).map((path) =>
        path.getAttribute('d'),
      ),
    ).toEqual(['M10,0L100,14', 'M10,20L100,40', 'M10,40L100,66']);
    fireEvent.click(screen.getByRole('button', { name: 'connect' }));
    expect(onChange).toHaveBeenCalledOnce();
    const [changed] = onChange.mock.calls[0] as [AudioGraphModel];
    expect(changed.edges.map((edge) => [edge.sourcePortId, edge.targetPortId])).toEqual([
      ['out-2', 'in-1'],
      ['out-3', 'in-2'],
      ['out-4', 'in-3'],
    ]);
  },
);

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

function installMeasuredResizeObserver() {
  const originalObserver = globalThis.ResizeObserver;
  const observers = new Map<Element, ResizeObserverCallback>();
  globalThis.ResizeObserver = class implements ResizeObserver {
    constructor(private readonly callback: ResizeObserverCallback) {}
    observe(target: Element) {
      observers.set(target, this.callback);
    }
    unobserve(target: Element) {
      observers.delete(target);
    }
    disconnect() {}
  };
  return {
    restore: () => {
      globalThis.ResizeObserver = originalObserver;
    },
    reportHeight: (target: Element, height: number) => {
      const style = (target as HTMLElement).style;
      style.boxSizing = 'border-box';
      style.height = `${height}px`;
      const callback = observers.get(target);
      assert(callback);
      act(() =>
        callback(
          [
            {
              target,
              borderBoxSize: [{ inlineSize: 300, blockSize: height }],
              contentRect: { x: 0, y: 0, top: 0, left: 0, right: 300, bottom: height },
            } as unknown as ResizeObserverEntry,
          ],
          {} as ResizeObserver,
        ),
      );
    },
  };
}

function ControlledResizeCanvas({
  initial,
  committed,
}: {
  initial: AudioGraphModel;
  committed: (model: AudioGraphModel, kind: string) => void;
}) {
  const [graph, setGraph] = useState(initial);
  return (
    <AudioGraphCanvas
      ariaLabel="Routing graph"
      model={graph}
      onModelChange={(next, kind) => {
        committed(next, kind);
        setGraph(next);
      }}
      onNodeGainChange={vi.fn()}
    />
  );
}

function resizedOptionsModel() {
  const graph = model();
  const source = graph.nodes[0];
  assert(source);
  source.size = { width: 300, height: 400 };
  source.appearance.gainControl = { label: 'Input gain', valueDb: 0 };
  source.outputPorts = [
    { id: 'left', label: 'Left', channelIndex: 0 },
    { id: 'right', label: 'Right', channelIndex: 1 },
  ];
  return graph;
}

function resizedNodeElements(container: HTMLElement) {
  const flowNode = container.querySelector<HTMLElement>('.react-flow__node[data-id="source"]');
  assert(flowNode);
  const card = flowNode.querySelector<HTMLElement>('[data-face-mode="standard"]');
  assert(card);
  const options = within(flowNode).getByRole('button', { name: 'Options' });
  const controls = options.parentElement?.parentElement;
  assert(controls);
  const inner = flowNode.querySelector<HTMLElement>('[style*="min-height"]');
  const grip = flowNode.querySelector('[data-resize-grip="bottom-right"]');
  const port = flowNode.querySelector<HTMLElement>('[data-audio-port-id="left"]');
  assert(inner && grip && port);
  return { flowNode, card, options, controls, inner, grip, port };
}

it('keeps stored size and port anchors stable through Options and normalizes open resizes once', async () => {
  const { reportHeight, restore } = installMeasuredResizeObserver();
  try {
    const initial = resizedOptionsModel();
    const committed = vi.fn();
    const view = render(<ControlledResizeCanvas initial={initial} committed={committed} />, {
      wrapper: CanvasTestProvider,
    });
    const { flowNode, card, options, controls, inner, grip, port } = resizedNodeElements(
      view.container,
    );
    reportHeight(controls, 25);
    await waitFor(() =>
      expect(card.style.getPropertyValue('--audio-node-face-height')).toBe('373px'),
    );
    const naturalMinHeight = Number.parseFloat(inner.style.minHeight);
    expect(naturalMinHeight).toBeLessThan(400);
    expect(inner).toHaveStyle({ height: '400px' });
    expect(grip).toHaveAttribute('data-min-height', String(naturalMinHeight));
    const closedTop = port.style.top;

    fireEvent.click(options);
    const panel = options.nextElementSibling;
    assert(panel);
    reportHeight(panel, 37.5);
    reportHeight(controls, 62.5);
    await waitFor(() => expect(inner).toHaveStyle({ height: '437.5px' }));
    expect(inner).toHaveStyle({ minHeight: `${naturalMinHeight + 37.5}px` });
    expect(grip).toHaveAttribute('data-min-height', String(naturalMinHeight + 37.5));
    expect((port as HTMLElement).style.top).toBe(closedTop);
    expect(flowNode).not.toHaveAttribute('data-flow-height');
    expect(committed).not.toHaveBeenCalled();

    resizeHarness.height = 480.5;
    fireEvent.click(screen.getByRole('button', { name: 'resize source' }));
    await waitFor(() => {
      const latest = committed.mock.lastCall?.[0] as AudioGraphModel | undefined;
      expect(latest?.nodes[0]?.size?.height).toBe(443);
      expect(inner).toHaveStyle({ height: '480.5px' });
    });
    expect(flowNode).not.toHaveAttribute('data-flow-height');
    expect(flowNode).toHaveAttribute('data-flow-measured-height', '480.5');
    expect((port as HTMLElement).style.top).not.toBe(closedTop);

    fireEvent.click(options);
    reportHeight(controls, 25);
    await waitFor(() => expect(inner).toHaveStyle({ height: '443px' }));
    await waitFor(() =>
      expect((card as HTMLElement).style.getPropertyValue('--audio-node-face-height')).toBe(
        '416px',
      ),
    );
    expect(committed).toHaveBeenCalledTimes(1);
    const resizedTop = (port as HTMLElement).style.top;
    fireEvent.click(options);
    const reopenedPanel = options.nextElementSibling;
    assert(reopenedPanel);
    reportHeight(reopenedPanel, 37.5);
    reportHeight(controls, 62.5);
    await waitFor(() => expect(inner).toHaveStyle({ height: '480.5px' }));
    expect(committed).toHaveBeenCalledTimes(1);
    expect((port as HTMLElement).style.top).toBe(resizedTop);
    fireEvent.click(screen.getByRole('button', { name: 'resize source' }));
    expect(
      (committed.mock.lastCall?.[0] as AudioGraphModel | undefined)?.nodes[0]?.size?.height,
    ).toBe(443);
    expect(flowNode).not.toHaveAttribute('data-flow-height');
    expect(flowNode).toHaveAttribute('data-flow-measured-height', '480.5');

    resizeHarness.height = 450.5;
    fireEvent.click(screen.getByRole('button', { name: 'resize source' }));
    await waitFor(() => expect(inner).toHaveStyle({ height: '450.5px' }));
    expect(flowNode).not.toHaveAttribute('data-flow-height');
    expect(flowNode).toHaveAttribute('data-flow-measured-height', '450.5');
    expect(
      (committed.mock.lastCall?.[0] as AudioGraphModel | undefined)?.nodes[0]?.size?.height,
    ).toBe(413);
    fireEvent.click(options);
    reportHeight(controls, 25);
    await waitFor(() => expect(inner).toHaveStyle({ height: '413px' }));
  } finally {
    restore();
  }
});

function RecorderDisclosureFixture() {
  const { opened, toggle } = useNodeOptionsState();
  return (
    <>
      <span>Transport</span>
      <NodeOptionsSection label="Options" opened={opened} onToggle={toggle}>
        <div style={{ height: 400 }}>Recording settings</div>
      </NodeOptionsSection>
    </>
  );
}

it('shares one open Options panel across regular and recorder nodes and closes on the pane', () => {
  const graph = model();
  graph.nodes = graph.nodes.map((node) =>
    node.id === 'source'
      ? { ...node, appearance: { ...node.appearance, gainControl: { label: 'Level', valueDb: 0 } } }
      : node,
  );
  render(
    <CanvasTestProvider>
      <AudioGraphCanvas
        ariaLabel="Routing graph"
        model={graph}
        onModelChange={vi.fn()}
        onNodeGainChange={vi.fn()}
        renderNodeContent={(node) => (node.id === 'sink' ? <RecorderDisclosureFixture /> : null)}
      />
    </CanvasTestProvider>,
  );
  const [gain, recorder] = screen.getAllByRole('button', { name: 'Options' });
  assert(gain && recorder);
  fireEvent.click(gain);
  expect(gain).toHaveAttribute('aria-expanded', 'true');
  fireEvent.click(recorder);
  expect(gain).toHaveAttribute('aria-expanded', 'false');
  expect(recorder).toHaveAttribute('aria-expanded', 'true');
  fireEvent.click(gain);
  expect(recorder).toHaveAttribute('aria-expanded', 'false');
  fireEvent.click(screen.getByRole('button', { name: 'Graph background' }));
  expect(gain).toHaveAttribute('aria-expanded', 'false');
  fireEvent.click(recorder);
  fireEvent.click(screen.getByRole('button', { name: 'Graph background' }));
  expect(recorder).toHaveAttribute('aria-expanded', 'false');
});

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
          meterBanks: [{ channels, nodeId: 'take', portId: 'audio' }],
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
