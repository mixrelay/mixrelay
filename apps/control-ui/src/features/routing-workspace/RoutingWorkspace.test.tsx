import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import type { ReactNode } from 'react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { AudioGraphModel, AudioGraphNode } from '../../core/audio-graph/model';
import {
  type AudioSessionStatus,
  type ControlPort,
  ControlPortError,
  type ExternalProcessorDescriptor,
  type RuntimeGraphDocument,
} from '../../core/control-port/types';
import { i18n } from '../../core/i18n';
import { MixRelayProvider } from '../../core/theme/MixRelayProvider';
import type { VirtualEndpointUpdate } from './model';
import { RoutingWorkspace } from './RoutingWorkspace';
import { duplexReference, virtualPolicy, virtualSpec } from './virtualEndpointFixtures';

vi.mock('../../core/audio-graph/AudioGraphCanvas', () => ({
  AudioGraphCanvas: ({
    ariaLabel,
    model,
    onModelChange,
    onNodeGainChange,
    onNodeParameterChange,
    onNodeEndpointChange,
    viewport,
    nodeMenuItems,
    onAddNode,
    renderNodeContent,
  }: {
    ariaLabel: string;
    model: AudioGraphModel;
    onModelChange: (model: AudioGraphModel, kind: 'layout' | 'runtime') => void;
    onNodeGainChange?: (nodeId: string, gainDb: number) => void;
    onNodeParameterChange?: (nodeId: string, parameterId: string, value: number) => void;
    onNodeEndpointChange?: (nodeId: string, value: VirtualEndpointUpdate) => void;
    viewport?: { x: number; y: number; zoom: number };
    nodeMenuItems?: readonly unknown[];
    onAddNode?: (itemId: string, position: { x: number; y: number }) => void;
    renderNodeContent?: (node: AudioGraphNode) => ReactNode;
  }) => (
    <section aria-label={ariaLabel}>
      <span data-testid="model-revision">{model.revision}</span>
      <output data-testid="node-sizes">
        {JSON.stringify(Object.fromEntries(model.nodes.map((node) => [node.id, node.size])))}
      </output>
      {model.nodes.map((node) => (
        <div key={`content:${node.id}`}>{renderNodeContent?.(node)}</div>
      ))}
      <button type="button" onClick={() => onAddNode?.('recorder', { x: 120, y: 60 })}>
        add recorder
      </button>
      <button
        type="button"
        onClick={() => onAddNode?.(`virtual/input/${virtualSpec.id}`, { x: 0, y: 0 })}
      >
        share fixture source
      </button>
      {model.nodes.map((node) => (
        <button
          key={node.id}
          type="button"
          onClick={() =>
            onNodeEndpointChange?.(node.id, {
              displayName: node.appearance.title,
              renderChannels: 2,
              captureChannels: 2,
              sampleRate: 44100,
            })
          }
        >
          rate {node.id}
        </button>
      ))}
      <button type="button" onClick={() => onNodeGainChange?.('input', -6)}>
        change gain
      </button>
      <button
        type="button"
        disabled={
          !model.nodes.some((node) =>
            node.appearance.parameterControls?.some((parameter) => parameter.id === 'external:42'),
          )
        }
        onClick={() => onNodeParameterChange?.('processor-1', 'external:42', 0.2)}
      >
        lower parameter
      </button>
      <button
        type="button"
        onClick={() => onNodeParameterChange?.('processor-1', 'external:42', 0.8)}
      >
        raise parameter
      </button>
      <output data-testid="edge-count">{model.edges.length}</output>
      <output data-testid="node-statuses">
        {JSON.stringify(
          model.nodes.map((node) => ({
            id: node.id,
            status: node.appearance.status,
            tone: node.appearance.statusTone,
          })),
        )}
      </output>
      <output data-testid="node-menu">{JSON.stringify(nodeMenuItems)}</output>
      <output data-testid="layout">
        {JSON.stringify({
          positions: Object.fromEntries(model.nodes.map((node) => [node.id, node.position])),
          viewport,
        })}
      </output>
      <button
        onClick={() =>
          onModelChange({ ...model, edges: [], revision: model.revision + 1 }, 'runtime')
        }
        type="button"
      >
        edit graph
      </button>
      <button
        onClick={() =>
          onModelChange(
            {
              ...model,
              nodes: model.nodes.map((node) => ({
                ...node,
                position: { x: node.position.x + 20, y: node.position.y },
              })),
            },
            'layout',
          )
        }
        type="button"
      >
        move node
      </button>
      <button
        onClick={() =>
          onModelChange(
            {
              ...model,
              nodes: model.nodes.map((node) => ({
                ...node,
                size: { width: 400, height: 220 },
              })),
            },
            'layout',
          )
        }
        type="button"
      >
        resize node
      </button>
    </section>
  ),
}));

const document: RuntimeGraphDocument = {
  edges: [
    {
      channelMapping: { mode: 'auto' },
      destination: { nodeId: 'default-virtual-io', portId: 'to_os' },
      id: 'direct',
      source: { nodeId: 'default-virtual-io', portId: 'from_os' },
    },
  ],
  nodes: [
    {
      config: {
        ...virtualSpec,
        render: { channels: 2 },
        capture: { channels: 2 },
        reference: duplexReference,
      },
      id: 'default-virtual-io',
      label: 'MixRelay Virtual I/O',
      type: 'virtual_endpoint',
    },
  ],
  revision: 1,
  schemaVersion: 2,
};

it.each([true, false])(
  'uses canonical identities from both node views and blocks absent IDs (valid=%s)',
  async (valid) => {
    const change = vi.fn();
    const graph = {
      ...document,
      edges: [],
      nodes: ['device-a', 'device-b'].map((id, index) => ({
        id: `graph-node-${index}`,
        label: 'Same display name',
        type: 'virtual_endpoint',
        config: {
          ...(valid ? { id } : {}),
          render: { channels: 2 },
          capture: { channels: 2 },
          reference: duplexReference,
        },
      })),
    };
    render(
      <MixRelayProvider>
        <I18nextProvider i18n={i18n}>
          <RoutingWorkspace
            controlPort={port({ getGraph: vi.fn().mockResolvedValue(graph) })}
            deviceRateAction={{ change, enabled: true, error: null }}
          />
        </I18nextProvider>
      </MixRelayProvider>,
    );
    for (const button of await screen.findAllByRole('button', { name: /^rate graph-node-/ }))
      fireEvent.click(button);
    expect(change.mock.calls).toEqual(
      valid
        ? [
            ['device-a', 44100],
            ['device-a', 44100],
            ['device-b', 44100],
            ['device-b', 44100],
          ]
        : [],
    );
  },
);

function port(overrides: Partial<ControlPort> = {}): ControlPort {
  return {
    getProcessorEditorFrame: vi.fn().mockRejectedValue(new Error('No native frame fixture')),
    sendProcessorEditorInput: vi.fn().mockResolvedValue(undefined),
    recorderStatus: vi.fn().mockResolvedValue({
      state: 'stopped',
      path: null,
      sampleRate: null,
      encoding: null,
      frames: 0,
      error: null,
    }),
    controlRecorder: vi.fn().mockRejectedValue(new Error('No recorder fixture configured')),
    applyGraph: vi.fn().mockResolvedValue({ protocolVersion: 1, revision: 2, state: 'active' }),
    getGraph: vi.fn().mockResolvedValue(document),
    getGraphStatus: vi
      .fn()
      .mockResolvedValue({ activeRevision: 2, protocolVersion: 1, submittedRevision: 2 }),
    getMeters: vi.fn().mockResolvedValue({ nodes: [], protocolVersion: 1, revision: 1 }),
    getProcessors: vi.fn().mockResolvedValue({ processors: [], protocolVersion: 1 }),
    getProcessorEditor: vi.fn().mockImplementation((nodeId: string) =>
      Promise.resolve({
        failure: null,
        height: null,
        nodeId,
        protocolVersion: 1,
        resizable: false,
        state: 'closed',
        width: null,
      }),
    ),
    rescanProcessors: vi.fn().mockResolvedValue({ processors: [], protocolVersion: 1 }),
    getStatus: vi.fn().mockResolvedValue({}),
    setProcessorParameter: vi.fn().mockResolvedValue({
      nodeId: 'processor-1',
      parameterId: 1,
      protocolVersion: 1,
      value: 0.5,
    }),
    setProcessorEditor: vi.fn().mockImplementation((nodeId: string, open: boolean) =>
      Promise.resolve({
        failure: null,
        height: null,
        nodeId,
        protocolVersion: 1,
        resizable: false,
        state: open ? 'opening' : 'closed',
        width: null,
      }),
    ),
    ...overrides,
  };
}

function expectActiveWorkspace() {
  expect(screen.getByRole('textbox', { name: 'Graph name' }).closest('section')).toHaveAttribute(
    'data-state',
    'active',
  );
  expect(screen.queryByText('Active')).not.toBeInTheDocument();
}

function renderWorkspace(controlPort: ControlPort, onBusyChange?: (busy: boolean) => void) {
  return render(workspace(controlPort, onBusyChange));
}

function workspace(
  controlPort: ControlPort,
  onBusyChange?: (busy: boolean) => void,
  graphId?: string,
) {
  return (
    <MixRelayProvider>
      <I18nextProvider i18n={i18n}>
        <RoutingWorkspace controlPort={controlPort} onBusyChange={onBusyChange} graphId={graphId} />
      </I18nextProvider>
    </MixRelayProvider>
  );
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((onResolve, onReject) => {
    resolve = onResolve;
    reject = onReject;
  });
  return { promise, reject, resolve };
}

afterEach(() => {
  vi.useRealTimers();
});

it('adds Recorder from the output menu as a graph node without file settings', async () => {
  await i18n.changeLanguage('en');
  const source = port();
  renderWorkspace(source);
  const add = await screen.findByRole('button', { name: 'add recorder' });
  expect(screen.getByTestId('node-menu')).toHaveTextContent('"id":"recorder","label":"Recorder"');
  fireEvent.click(add);
  await waitFor(() => expect(source.applyGraph).toHaveBeenCalled());
  expect(source.applyGraph).toHaveBeenLastCalledWith(
    expect.objectContaining({
      nodes: expect.arrayContaining([
        expect.objectContaining({ type: 'recorder', config: { channels: 2 } }),
      ]),
    }),
  );
});

it('retains routing and editable explicit device selectors while a device disappears and returns', async () => {
  await i18n.changeLanguage('en');
  const devices = [
    {
      uid: 'microphone',
      displayName: 'Mic',
      inputChannels: 1,
      outputChannels: 0,
      isDefaultInput: true,
      isDefaultOutput: false,
    },
    {
      uid: 'headphones',
      displayName: 'Headphones',
      inputChannels: 0,
      outputChannels: 2,
      isDefaultInput: false,
      isDefaultOutput: false,
    },
  ];
  const graph: RuntimeGraphDocument = {
    schemaVersion: 2,
    revision: 1,
    nodes: [
      {
        id: 'input',
        type: 'audio_device_input',
        label: 'Mic',
        config: {
          selector: { match: 'persistent_uid', uid: 'microphone' },
          outputChannels: 1,
          gainDb: 0,
        },
      },
      {
        id: 'output',
        type: 'audio_device_output',
        label: 'Headphones',
        config: { selector: { match: 'persistent_uid', uid: 'headphones' }, inputChannels: 2 },
      },
    ],
    edges: [
      {
        id: 'route',
        source: { nodeId: 'input', portId: 'audio' },
        destination: { nodeId: 'output', portId: 'audio' },
        channelMapping: { mode: 'auto' },
      },
    ],
  };
  const discovery = vi.fn().mockResolvedValue(devices);
  const source = port({ getGraph: vi.fn().mockResolvedValue(graph), listAudioDevices: discovery });
  renderWorkspace(source);
  await waitFor(() => expect(discovery).toHaveBeenCalled());
  await waitFor(() => expect(screen.getByTestId('edge-count')).toHaveTextContent('1'));
  discovery.mockResolvedValue([devices[0]]);
  act(() => window.dispatchEvent(new Event('focus')));
  await waitFor(() =>
    expect(screen.getByTestId('node-statuses')).toHaveTextContent(
      '"id":"output","status":"Offline","tone":"neutral"',
    ),
  );
  expect(screen.getByTestId('node-statuses')).toHaveTextContent('{"id":"input"}');
  expect(screen.getByTestId('edge-count')).toHaveTextContent('1');
  expect(source.applyGraph).not.toHaveBeenCalled();
  expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: 'change gain' }));
  await waitFor(() => expect(source.applyGraph).toHaveBeenCalled());
  expect(source.applyGraph).toHaveBeenLastCalledWith(
    expect.objectContaining({
      edges: graph.edges,
      nodes: expect.arrayContaining([
        graph.nodes[1],
        expect.objectContaining({ id: 'input', config: expect.objectContaining({ gainDb: -6 }) }),
      ]),
    }),
  );
  discovery.mockResolvedValue(devices);
  act(() => window.dispatchEvent(new Event('focus')));
  await waitFor(() => expect(screen.getByTestId('node-statuses')).not.toHaveTextContent('Offline'));
  expect(source.applyGraph).toHaveBeenCalledTimes(1);
  expect(screen.getByTestId('edge-count')).toHaveTextContent('1');
});

it('applies recorder channel edits through graph history and restores the width on undo', async () => {
  await i18n.changeLanguage('en');
  const geometry = [
    vi
      .spyOn(Element.prototype, 'getBoundingClientRect')
      .mockReturnValue(new DOMRect(20, 20, 240, 32)),
    vi.spyOn(globalThis.document.documentElement, 'clientWidth', 'get').mockReturnValue(1024),
    vi.spyOn(globalThis.document.documentElement, 'clientHeight', 'get').mockReturnValue(768),
  ];
  try {
    const source = port({
      applyGraph: vi
        .fn()
        .mockImplementation((graph: RuntimeGraphDocument) =>
          Promise.resolve({ protocolVersion: 1, revision: graph.revision, state: 'active' }),
        ),
      getGraph: vi.fn().mockResolvedValue({
        schemaVersion: 2,
        revision: 1,
        edges: [],
        nodes: [{ id: 'take', type: 'recorder', label: 'Recorder', config: { channels: 2 } }],
      }),
    });
    render(workspace(source, undefined, 'graph-1'));
    fireEvent.click(await screen.findByRole('button', { name: 'Options' }));
    const channels = screen.getByRole('combobox', { name: 'Channels' });
    await waitFor(() => expect(channels).toBeEnabled());
    fireEvent.click(channels);
    fireEvent.click(await screen.findByRole('option', { name: '1' }));
    const graphWithChannels = (count: number) =>
      expect.objectContaining({
        nodes: [
          expect.objectContaining({ id: 'take', type: 'recorder', config: { channels: count } }),
        ],
      });
    await waitFor(() => expect(source.applyGraph).toHaveBeenLastCalledWith(graphWithChannels(1)));
    expect(channels).toHaveValue('1');
    fireEvent.keyDown(window, { key: 'z', ctrlKey: true });
    await waitFor(() => expect(source.applyGraph).toHaveBeenLastCalledWith(graphWithChannels(2)));
    expect(channels).toHaveValue('2');
    fireEvent.keyDown(window, { key: 'z', ctrlKey: true, shiftKey: true });
    await waitFor(() => expect(source.applyGraph).toHaveBeenLastCalledWith(graphWithChannels(1)));
  } finally {
    for (const mock of geometry) mock.mockRestore();
  }
});

describe('RoutingWorkspace processor discovery evidence', () => {
  it('retains an unapplied draft when a late session document arrives', async () => {
    await i18n.changeLanguage('en');
    const source = port({ applyGraph: vi.fn().mockRejectedValue(new Error('offline')) });
    const view = (sessionDocument: RuntimeGraphDocument) => (
      <MixRelayProvider>
        <I18nextProvider i18n={i18n}>
          <RoutingWorkspace controlPort={source} sessionDocument={sessionDocument} />
        </I18nextProvider>
      </MixRelayProvider>
    );
    const mounted = render(view(document));
    await waitFor(() => expect(screen.getByTestId('edge-count')).toHaveTextContent('2'));
    fireEvent.click(screen.getByRole('button', { name: 'edit graph' }));
    await screen.findByText('Not applied');
    mounted.rerender(view({ ...document, revision: 5 }));
    expect(screen.getByTestId('edge-count')).toHaveTextContent('0');
    expect(screen.getByText('Not applied')).toBeInTheDocument();
    expect(source.applyGraph).toHaveBeenCalledTimes(1);
  });
  it('shows incomplete search in the menu until a real retry finishes cleanly', async () => {
    await i18n.changeLanguage('en');
    const source = port({
      getProcessors: vi.fn().mockResolvedValue({
        processors: [],
        protocolVersion: 1,
        diagnostics: [{ format: 'vst3', stage: 'scan', message: 'Timed out' }],
      }),
    });
    const view = renderWorkspace(source);
    const menu = await screen.findByTestId('node-menu');
    await waitFor(() => expect(menu).toHaveTextContent('Effect search incomplete'));
    expect(menu).not.toHaveTextContent('No installed audio effects');
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    await waitFor(() => expect(menu).toHaveTextContent('No installed audio effects'));
    expect(source.rescanProcessors).toHaveBeenCalledWith([]);
    view.unmount();
  });
});

const virtualMenuSession: AudioSessionStatus = {
  format: { sampleRate: 48000, blockFrames: 128 },
  clockAuthority: 'device',
  observedDeviceSampleRate: 48000,
  issue: null,
  transportLatencyFrames: null,
  capabilities: {
    backend: 'core_audio',
    supportedSampleRates: [48000],
    supportedBlockFrames: [128],
    canReconfigureLive: true,
    hardwareInput: true,
    hardwareOutput: true,
    virtualEndpoint: true,
    applicationCapture: true,
    sampleRateConversion: false,
    driftCorrection: false,
  },
  virtualEndpointSettings: {
    ...virtualPolicy,
    devices: [
      {
        nativeDeviceIds: [],
        spec: {
          ...virtualSpec,
          id: 'virtual',
          displayName: 'MixRelay Virtual I/O',
          capture: { channels: 32, channelLayout: 'discrete' },
        },
        observedSampleRate: 48000,
        supportedSampleRates: [48000],
        canSetSampleRate: true,
      },
    ],
  },
};

it('adds a capture-only shared device as one 32-channel source through the device menu', async () => {
  const spec = {
    ...virtualSpec,
    render: undefined,
    capture: { channels: 32, channelLayout: 'discrete' as const },
  };
  const source = port({
    getGraph: vi.fn().mockResolvedValue({ ...document, nodes: [], edges: [] }),
  });
  render(
    <MixRelayProvider>
      <I18nextProvider i18n={i18n}>
        <RoutingWorkspace
          controlPort={source}
          audioSession={{
            ...virtualMenuSession,
            virtualEndpointSettings: {
              ...virtualPolicy,
              devices: [
                {
                  nativeDeviceIds: [],
                  spec,
                  observedSampleRate: 44100,
                  supportedSampleRates: [44100],
                  canSetSampleRate: false,
                },
              ],
            },
          }}
        />
      </I18nextProvider>
    </MixRelayProvider>,
  );
  await screen.findByTestId('node-menu');
  expect(screen.getByTestId('node-menu')).toHaveTextContent('32 CH');
  expect(screen.getByTestId('node-menu')).toHaveTextContent(
    `"id":"virtual/input/${virtualSpec.id}"`,
  );
  fireEvent.click(screen.getByRole('button', { name: 'share fixture source' }));
  await waitFor(() => expect(source.applyGraph).toHaveBeenCalledTimes(1));
  expect(source.applyGraph).toHaveBeenCalledWith(
    expect.objectContaining({
      nodes: [
        expect.objectContaining({
          config: expect.objectContaining({
            capture: spec.capture,
            reference: { render: true, capture: false },
          }),
        }),
      ],
    }),
  );
});

it('excludes every canonical native device UID from both hardware menus, not similarly named hardware', async () => {
  const source = port({
    listAudioDevices: vi.fn().mockResolvedValue(
      ['native-a', 'native-b', 'physical'].map((uid) => ({
        uid,
        displayName: uid,
        inputChannels: 2,
        outputChannels: 2,
        isDefaultInput: false,
        isDefaultOutput: false,
      })),
    ),
  });
  render(
    <MixRelayProvider>
      <I18nextProvider i18n={i18n}>
        <RoutingWorkspace
          controlPort={source}
          audioSession={{
            ...virtualMenuSession,
            virtualEndpointSettings: {
              ...virtualPolicy,
              devices: ['native-a', 'native-b'].map((uid) => ({
                nativeDeviceIds: [uid],
                spec: { ...virtualSpec, id: uid },
                observedSampleRate: null,
                supportedSampleRates: [],
                canSetSampleRate: false,
              })),
            },
          }}
        />
      </I18nextProvider>
    </MixRelayProvider>,
  );
  await waitFor(() => expect(screen.getByTestId('node-menu')).toHaveTextContent('physical'));
  const menu = screen.getByTestId('node-menu').textContent;
  expect(menu).not.toContain('"label":"native-a"');
  expect(menu).not.toContain('"label":"native-b"');
  expect(menu?.match(/"label":"physical"/g)).toHaveLength(2);
});

it('preserves provider routes while discovery is pending and restores their presentation after discovery', async () => {
  const catalog = deferred<{
    protocolVersion: number;
    processors: ExternalProcessorDescriptor[];
  }>();
  const original: RuntimeGraphDocument = {
    ...document,
    nodes: [
      ...document.nodes,
      { id: 'effect', label: null, type: 'provider', config: { typeId: 'fixture:effect' } },
    ],
    edges: [
      {
        id: 'original-route',
        source: { nodeId: 'effect', portId: 'audio' },
        destination: { nodeId: 'default-virtual-io', portId: 'to_os' },
        channelMapping: { mode: 'explicit', routes: [{ sourceChannel: 1, destinationChannel: 0 }] },
      },
    ],
  };
  const source = port({
    getGraph: vi.fn().mockResolvedValue(original),
    getProcessors: vi.fn().mockReturnValue(catalog.promise),
  });
  renderWorkspace(source);
  await waitFor(() => expect(screen.getByTestId('edge-count')).toHaveTextContent('0'));
  fireEvent.click(screen.getByRole('button', { name: 'edit graph' }));
  await waitFor(() => expect(source.applyGraph).toHaveBeenCalledTimes(1));
  expect(source.applyGraph).toHaveBeenCalledWith(
    expect.objectContaining({ edges: original.edges }),
  );
  await act(async () =>
    catalog.resolve({
      protocolVersion: 1,
      processors: [
        {
          typeId: 'fixture:effect',
          displayName: 'Effect',
          format: 'vst3',
          inputChannels: 2,
          outputChannels: 2,
          parameters: [],
          hasEditor: false,
          vendor: 'Test',
          version: '1',
        },
      ],
    }),
  );
  await waitFor(() => expect(screen.getByTestId('edge-count')).toHaveTextContent('1'));
});

describe('RoutingWorkspace virtual device menu rows', () => {
  const session = virtualMenuSession;
  it.each(['en', 'ko-KR', 'pseudo'])('uses short channel/status metadata in %s', async (locale) => {
    await i18n.changeLanguage(locale);
    const source = port({
      getGraph: vi.fn().mockResolvedValue({ ...document, nodes: [], edges: [] }),
    });
    const menuRows = () => {
      const items = JSON.parse(screen.getByTestId('node-menu').textContent ?? '[]') as {
        children?: {
          children?: { id: string; label: string; description: string; disabled: boolean }[];
        }[];
      }[];
      return items
        .flatMap((item) => item.children ?? [])
        .flatMap((item) => item.children ?? [])
        .filter((item) => item.id.startsWith('virtual/'));
    };
    const view = (audioSession: AudioSessionStatus | undefined) => (
      <MixRelayProvider>
        <I18nextProvider i18n={i18n}>
          <RoutingWorkspace controlPort={source} audioSession={audioSession} />
        </I18nextProvider>
      </MixRelayProvider>
    );
    const rendered = render(view(session));
    await waitFor(() => expect(menuRows()).toHaveLength(2));
    expect(menuRows().map((row) => row.description)).toEqual(['64 CH', '32 CH']);
    expect(menuRows().every((row) => !row.disabled)).toBe(true);
    expect(menuRows().every((row) => row.label === 'MixRelay Virtual I/O')).toBe(true);

    rendered.rerender(
      view({
        ...session,
        capabilities: { ...session.capabilities, virtualEndpoint: false },
      }),
    );
    expect(menuRows().every((row) => row.disabled)).toBe(true);

    rendered.rerender(view(undefined));
    expect(menuRows()).toEqual([]);
    expect(source.applyGraph).not.toHaveBeenCalled();
    rendered.unmount();
  });

  it.each(['en', 'ko-KR', 'pseudo'])(
    'describes attached channels without repeating routing prose in %s',
    async (locale) => {
      await i18n.changeLanguage(locale);
      const source = port({
        getGraph: vi.fn().mockResolvedValue({
          ...document,
          edges: [],
          nodes: [
            {
              id: 'virtual',
              type: 'virtual_endpoint',
              label: null,
              config: {
                id: 'virtual',
                render: { channels: 8 },
                capture: { channels: 16 },
                reference: duplexReference,
              },
            },
          ],
        }),
      });
      const rendered = render(
        <MixRelayProvider>
          <I18nextProvider i18n={i18n}>
            <RoutingWorkspace controlPort={source} audioSession={session} />
          </I18nextProvider>
        </MixRelayProvider>,
      );
      const menu = await screen.findByTestId('node-menu');
      await waitFor(() =>
        expect(menu).toHaveTextContent(`64 CH · ${i18n.t('routing.nodes.virtualAttached')}`),
      );
      expect(menu).toHaveTextContent(`32 CH · ${i18n.t('routing.nodes.virtualAttached')}`);
      const rows = JSON.parse(menu.textContent ?? '[]')
        .flatMap((item: { children?: unknown[] }) => item.children ?? [])
        .flatMap((item: { children?: unknown[] }) => item.children ?? []);
      expect(rows.filter((row: { id: string }) => row.id.startsWith('virtual/'))).toEqual(
        expect.arrayContaining([
          expect.objectContaining({ id: 'virtual/input/virtual', disabled: true }),
          expect.objectContaining({ id: 'virtual/output/virtual', disabled: true }),
        ]),
      );
      expect(source.applyGraph).not.toHaveBeenCalled();
      rendered.unmount();
    },
  );
});

describe('RoutingWorkspace graph-scoped presentation', () => {
  it('waits for the replacement engine document before consuming its saved layout', async () => {
    const first = port();
    const secondDocument = deferred<RuntimeGraphDocument>();
    const saved = {
      positions: {
        'default-virtual-io::host-source': { x: 33, y: 44 },
        'default-virtual-io::host-sink': { x: 900, y: 40 },
      },
    };
    const second = port({
      getGraph: vi.fn().mockReturnValue(secondDocument.promise),
      getGraphLayout: vi
        .fn()
        .mockResolvedValue({ graphId: 'saved', layoutRevision: 5, layout: saved }),
      setGraphLayout: vi.fn(),
    });
    const view = render(workspace(first, undefined, 'saved'));
    await screen.findByRole('region');
    view.rerender(workspace(second, undefined, 'saved'));
    await act(async () => {
      await Promise.resolve();
    });
    await act(async () =>
      secondDocument.resolve({
        ...document,
        nodes: [
          {
            id: 'default-virtual-io',
            label: null,
            type: 'virtual_endpoint',
            config: {
              render: { channels: 2 },
              capture: { channels: 2 },
              reference: duplexReference,
            },
          },
        ],
      }),
    );
    await waitFor(() =>
      expect(JSON.parse(screen.getByTestId('layout').textContent ?? '{}')).toEqual(saved),
    );
    expect(second.setGraphLayout).not.toHaveBeenCalled();
    view.unmount();
  });
});

describe('RoutingWorkspace layout history', () => {
  it('undoes and redoes resizing without advancing the audio revision or submitting DSP', async () => {
    const control = port();
    renderWorkspace(control);
    await screen.findByRole('region');
    const revision = screen.getByTestId('model-revision').textContent;
    const originalSizes = screen.getByTestId('node-sizes').textContent;
    fireEvent.click(screen.getByRole('button', { name: 'resize node' }));
    const resized = screen.getByTestId('node-sizes').textContent;
    expect(resized).toContain('"width":400,"height":220');
    expect(screen.getByTestId('model-revision')).toHaveTextContent(revision ?? '');
    fireEvent.keyDown(window, { key: 'z', metaKey: true });
    expect(screen.getByTestId('node-sizes').textContent).toBe(originalSizes);
    expect(screen.getByTestId('model-revision')).toHaveTextContent(revision ?? '');
    fireEvent.keyDown(window, { key: 'z', metaKey: true, shiftKey: true });
    expect(screen.getByTestId('node-sizes').textContent).toBe(resized);
    expect(screen.getByTestId('model-revision')).toHaveTextContent(revision ?? '');
    expect(control.applyGraph).not.toHaveBeenCalled();
  });

  it('restores split positions, saves drag and Undo without applying the DSP graph', async () => {
    vi.useFakeTimers();
    const positions = {
      'default-virtual-io::host-source': { x: 11, y: 22 },
      'default-virtual-io::host-sink': { x: 800, y: 40 },
    };
    const viewport = { x: -90, y: 30, zoom: 0.75 };
    const virtual = {
      ...document,
      nodes: [
        {
          id: 'default-virtual-io',
          label: 'MixRelay Virtual I/O',
          type: 'virtual_endpoint',
          config: {
            reference: duplexReference,
            render: { channels: 2 },
            capture: { channels: 2 },
          },
        },
      ],
    };
    const response = { graphId: 'saved', layoutRevision: 3, layout: { positions, viewport } };
    const setGraphLayout = vi.fn().mockResolvedValue({ ...response, layoutRevision: 4 });
    const control = port({
      getGraph: vi.fn().mockResolvedValue(virtual),
      getGraphLayout: vi.fn().mockResolvedValue(response),
      setGraphLayout,
    });
    const rendered = render(workspace(control, undefined, 'saved'));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(JSON.parse(screen.getByTestId('layout').textContent ?? '{}')).toEqual({
      positions,
      viewport,
    });
    fireEvent.click(screen.getByRole('button', { name: 'move node' }));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(400);
    });
    expect(setGraphLayout).toHaveBeenLastCalledWith('saved', {
      expectedLayoutRevision: 3,
      layout: {
        positions: {
          'default-virtual-io::host-source': { x: 31, y: 22 },
          'default-virtual-io::host-sink': { x: 820, y: 40 },
        },
        sizes: {},
        viewport,
      },
    });
    fireEvent.keyDown(window, { key: 'z', metaKey: true });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(400);
    });
    expect(setGraphLayout).toHaveBeenLastCalledWith('saved', {
      expectedLayoutRevision: 4,
      layout: { positions, sizes: {}, viewport },
    });
    expect(control.applyGraph).not.toHaveBeenCalled();
    rendered.unmount();
  });
});

describe('RoutingWorkspace history owner', () => {
  it('does not replay a pending gain gesture into a replacement control port', async () => {
    await i18n.changeLanguage('en');
    vi.useFakeTimers();
    const inputGraph: RuntimeGraphDocument = {
      schemaVersion: 2,
      revision: 1,
      edges: [],
      nodes: [
        {
          id: 'input',
          label: 'Mic',
          type: 'audio_device_input',
          config: {
            selector: { match: 'persistent_uid', uid: 'mic' },
            outputChannels: 2,
            gainDb: 0,
          },
        },
      ],
    };
    const first = port({ getGraph: vi.fn().mockResolvedValue(inputGraph) });
    const second = port({ getGraph: vi.fn().mockResolvedValue(inputGraph) });
    const view = renderWorkspace(first);
    await act(async () => {});
    fireEvent.click(screen.getByRole('button', { name: 'change gain' }));
    await act(async () => {});
    expect(first.applyGraph).toHaveBeenCalledTimes(1);
    view.rerender(workspace(second));
    await act(async () => {});
    await act(async () => vi.advanceTimersByTimeAsync(500));
    fireEvent.keyDown(window, { key: 'z', ctrlKey: true });
    await act(async () => {});
    expect(second.applyGraph).not.toHaveBeenCalled();
    view.unmount();
    expect(vi.getTimerCount()).toBe(0);
  });
});

describe('RoutingWorkspace application', () => {
  it('loads the engine graph and immediately applies runtime edits', async () => {
    await i18n.changeLanguage('en');
    const controlPort = port();
    renderWorkspace(controlPort);

    expect(screen.getAllByText('Loading engine')).toHaveLength(2);
    expect(
      await screen.findByRole('region', { name: 'MixRelay virtual I/O routing graph' }),
    ).toBeInTheDocument();
    expectActiveWorkspace();
    fireEvent.click(screen.getByRole('button', { name: 'edit graph' }));
    await waitFor(() => expect(controlPort.applyGraph).toHaveBeenCalledOnce());
    await waitFor(expectActiveWorkspace);
  });

  it('keeps layout-only movement local to the editor', async () => {
    await i18n.changeLanguage('en');
    const controlPort = port();
    renderWorkspace(controlPort);
    await screen.findByRole('region');

    fireEvent.click(screen.getByRole('button', { name: 'move node' }));
    expect(controlPort.applyGraph).not.toHaveBeenCalled();
    expectActiveWorkspace();
  });

  it('polls pending graph activation without blocking the UI', async () => {
    await i18n.changeLanguage('en');
    vi.useFakeTimers();
    const getGraphStatus = vi
      .fn()
      .mockResolvedValueOnce({ activeRevision: 1, protocolVersion: 1, submittedRevision: 2 })
      .mockResolvedValueOnce({ activeRevision: 2, protocolVersion: 1, submittedRevision: 2 });
    const controlPort = port({
      applyGraph: vi.fn().mockResolvedValue({ protocolVersion: 1, revision: 2, state: 'pending' }),
      getGraphStatus,
    });
    const onBusyChange = vi.fn();
    renderWorkspace(controlPort, onBusyChange);
    expect(onBusyChange).toHaveBeenLastCalledWith(true);
    await act(async () => Promise.resolve());
    expect(onBusyChange).toHaveBeenLastCalledWith(false);
    fireEvent.click(screen.getByRole('button', { name: 'edit graph' }));
    await act(async () => Promise.resolve());
    expect(screen.getByText('Applying')).toBeInTheDocument();
    expect(onBusyChange).toHaveBeenLastCalledWith(true);
    await act(async () => vi.advanceTimersByTimeAsync(200));
    expectActiveWorkspace();
    expect(onBusyChange).toHaveBeenLastCalledWith(false);
  });
});

describe('RoutingWorkspace recovery', () => {
  it('preserves undo and redo after a rejection and reapplies only on explicit retry', async () => {
    await i18n.changeLanguage('en');
    const controlPort = port({
      applyGraph: vi
        .fn()
        .mockRejectedValueOnce(
          new ControlPortError(409, { code: 'graph_revision_stale', parameters: {} }),
        )
        .mockImplementation(async (doc: RuntimeGraphDocument) => ({
          protocolVersion: 1,
          revision: doc.revision,
          state: 'active',
        })),
      getGraph: vi
        .fn()
        .mockResolvedValueOnce(document)
        .mockResolvedValue({ ...document, revision: 9 }),
      getGraphStatus: vi
        .fn()
        .mockResolvedValue({ activeRevision: 9, submittedRevision: 9, protocolVersion: 1 }),
    });
    renderWorkspace(controlPort);
    await screen.findByRole('region');
    const originalEdges = screen.getByTestId('edge-count').textContent;
    fireEvent.click(screen.getByRole('button', { name: 'edit graph' }));
    await screen.findByText('Not applied');
    fireEvent.keyDown(window, { key: 'z', metaKey: true });
    expect(screen.getByTestId('edge-count').textContent).toBe(originalEdges);
    fireEvent.keyDown(window, { key: 'z', ctrlKey: true, shiftKey: true });
    expect(screen.getByTestId('edge-count')).toHaveTextContent('0');
    expect(controlPort.applyGraph).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole('button', { name: 'Overwrite engine with these edits' }));
    await waitFor(expectActiveWorkspace);
    expect(controlPort.applyGraph).toHaveBeenLastCalledWith(
      expect.objectContaining({ revision: 10, edges: [] }),
    );
  });

  it('explicit reload adopts engine state and clears incompatible history without a write', async () => {
    await i18n.changeLanguage('en');
    const controlPort = port({
      applyGraph: vi.fn().mockRejectedValue(new Error('offline')),
      getGraphStatus: vi
        .fn()
        .mockResolvedValue({ activeRevision: 1, submittedRevision: 1, protocolVersion: 1 }),
    });
    renderWorkspace(controlPort);
    await screen.findByRole('region');
    const originalEdges = screen.getByTestId('edge-count').textContent;
    fireEvent.click(screen.getByRole('button', { name: 'edit graph' }));
    await screen.findByText('Not applied');
    fireEvent.click(screen.getByRole('button', { name: 'Discard edits and reload engine' }));
    await waitFor(expectActiveWorkspace);
    expect(screen.getByTestId('edge-count').textContent).toBe(originalEdges);
    fireEvent.keyDown(window, { key: 'z', metaKey: true });
    expect(controlPort.applyGraph).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId('edge-count').textContent).toBe(originalEdges);
  });
});

describe('RoutingWorkspace parameter persistence warnings', () => {
  it('shows parameter save failure without rebuilding or rolling back applied audio', async () => {
    await i18n.changeLanguage('en');
    const processor: ExternalProcessorDescriptor = {
      displayName: 'Fixture',
      format: 'vst3',
      hasEditor: false,
      inputChannels: 2,
      outputChannels: 2,
      typeId: 'vst3:fixture',
      vendor: 'Test',
      version: '1',
      parameters: [
        {
          id: 42,
          name: 'Threshold',
          canAutomate: true,
          defaultValue: 0.5,
          isReadOnly: false,
          stepCount: 0,
        },
      ],
    };
    const controlPort = port({
      getGraph: vi.fn().mockResolvedValue({
        ...document,
        edges: [],
        nodes: [
          {
            id: 'processor-1',
            label: 'Fixture',
            type: 'provider',
            config: {
              typeId: processor.typeId,
              parameters: { version: 1, parameterValues: { '42': 0.5 } },
            },
          },
        ],
      }),
      getGraphStatus: vi
        .fn()
        .mockResolvedValue({ protocolVersion: 1, activeRevision: 1, submittedRevision: 1 }),
      getProcessors: vi.fn().mockResolvedValue({ protocolVersion: 1, processors: [processor] }),
      setProcessorParameter: vi
        .fn()
        .mockResolvedValueOnce({
          protocolVersion: 1,
          nodeId: 'processor-1',
          parameterId: 42,
          value: 0.2,
          persistenceError: 'disk full',
        })
        .mockResolvedValueOnce({
          protocolVersion: 1,
          nodeId: 'processor-1',
          parameterId: 42,
          value: 0.8,
        }),
    });
    renderWorkspace(controlPort);
    await screen.findByRole('region');
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'lower parameter' })).toBeEnabled(),
    );
    fireEvent.click(screen.getByRole('button', { name: 'lower parameter' }));
    const warning = await screen.findByText(
      /Audio is active, but graph changes could not be saved/,
    );
    expectActiveWorkspace();
    expect(controlPort.setProcessorParameter).toHaveBeenLastCalledWith('processor-1', 42, 0.2, 1);
    fireEvent.click(screen.getByRole('button', { name: 'raise parameter' }));
    await waitFor(() => expect(warning).not.toBeInTheDocument());
    expect(controlPort.setProcessorParameter).toHaveBeenLastCalledWith('processor-1', 42, 0.8, 1);
    expect(controlPort.applyGraph).not.toHaveBeenCalled();
    expect(controlPort.getGraph).toHaveBeenCalledTimes(1);
  });
});

describe('RoutingWorkspace persistence warnings', () => {
  it('keeps applied audio active and clears the warning only after a successful save', async () => {
    await i18n.changeLanguage('en');
    const controlPort = port({
      applyGraph: vi
        .fn()
        .mockResolvedValueOnce({
          protocolVersion: 1,
          revision: 2,
          state: 'active',
          persistenceError: 'graph_catalog_save_failed',
        })
        .mockResolvedValueOnce({ protocolVersion: 1, revision: 3, state: 'active' }),
    });
    const onBusyChange = vi.fn();
    renderWorkspace(controlPort, onBusyChange);
    await screen.findByRole('region');
    fireEvent.click(screen.getByRole('button', { name: 'edit graph' }));
    const warning = await screen.findByText(
      /Audio is active, but graph changes could not be saved/,
    );
    expect(warning).toHaveAttribute('role', 'status');
    expectActiveWorkspace();
    expect(onBusyChange).toHaveBeenLastCalledWith(false);
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    expect(controlPort.applyGraph).toHaveBeenCalledTimes(1);
    expect(controlPort.getGraph).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole('button', { name: 'edit graph' }));
    await waitFor(() => expect(warning).not.toBeInTheDocument());
    expect(controlPort.applyGraph).toHaveBeenCalledTimes(2);
  });
});

describe('RoutingWorkspace inventory failures', () => {
  it('reports inventory failure and retries without reloading the graph', async () => {
    await i18n.changeLanguage('en');
    const listAudioDevices = vi
      .fn()
      .mockRejectedValueOnce(new Error('offline'))
      .mockResolvedValue([]);
    const controlPort = port({ listAudioDevices });
    renderWorkspace(controlPort);
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Could not refresh apps and audio devices',
    );
    fireEvent.click(screen.getByRole('button', { name: /^Retry$/ }));
    await waitFor(() => expect(screen.queryByRole('alert')).not.toBeInTheDocument());
    expect(listAudioDevices).toHaveBeenCalledTimes(2);
    expect(controlPort.getGraph).toHaveBeenCalledTimes(1);
    expect(controlPort.applyGraph).not.toHaveBeenCalled();
  });
});

describe('RoutingWorkspace failures', () => {
  it('keeps rejected edits and presents stable and unexpected failures', async () => {
    await i18n.changeLanguage('en');
    const protocolFailure = port({
      applyGraph: vi
        .fn()
        .mockRejectedValue(
          new ControlPortError(422, { code: 'graph_invalid', parameters: { issues: [] } }),
        ),
    });
    const view = renderWorkspace(protocolFailure);
    await screen.findByRole('region');
    fireEvent.click(screen.getByRole('button', { name: 'edit graph' }));
    expect(
      await screen.findByText('The engine rejected an invalid audio graph.'),
    ).toBeInTheDocument();
    expect(screen.getByText('Not applied')).toBeInTheDocument();
    expect(screen.getByTestId('edge-count')).toHaveTextContent('0');
    view.unmount();

    renderWorkspace(port({ applyGraph: vi.fn().mockRejectedValue(new Error('offline')) }));
    await screen.findByRole('region');
    fireEvent.click(screen.getByRole('button', { name: 'edit graph' }));
    expect(await screen.findByText('The audio control request failed.')).toBeInTheDocument();
  });
});

it.each(['en', 'ko-KR', 'pseudo'])(
  'does not misdiagnose a rate binding failure as a channel mismatch in %s',
  async (language) => {
    await i18n.changeLanguage(language);
    renderWorkspace(
      port({
        applyGraph: vi.fn().mockRejectedValue(
          new ControlPortError(422, {
            code: 'graph_binding_unsupported',
            parameters: { reason: 'sample rate unsupported' },
          }),
        ),
      }),
    );
    await screen.findByRole('region');
    fireEvent.click(screen.getByRole('button', { name: 'edit graph' }));
    expect(
      await screen.findByText(i18n.t('routing.errors.graph_binding_unsupported')),
    ).toBeInTheDocument();
    expect(
      screen.queryByText('The node channel configuration does not match its graph bindings.'),
    ).not.toBeInTheDocument();
  },
);

describe('RoutingWorkspace failure recovery', () => {
  it('shows unavailable state when the initial graph cannot be loaded', async () => {
    await i18n.changeLanguage('en');
    renderWorkspace(port({ getGraph: vi.fn().mockRejectedValue(new Error('offline')) }));
    expect(await screen.findAllByText('Engine unavailable')).toHaveLength(2);
  });

  it('returns a pending polling failure to an editable state', async () => {
    await i18n.changeLanguage('en');
    const controlPort = port({
      applyGraph: vi.fn().mockResolvedValue({ protocolVersion: 1, revision: 2, state: 'pending' }),
      getGraphStatus: vi
        .fn()
        .mockRejectedValue(
          new ControlPortError(503, { code: 'control_unavailable', parameters: {} }),
        ),
    });
    renderWorkspace(controlPort);
    await screen.findByRole('region');
    fireEvent.click(screen.getByRole('button', { name: 'edit graph' }));
    expect(await screen.findByText('The local audio engine is unavailable.')).toBeInTheDocument();
    expect(screen.getByText('Not applied')).toBeInTheDocument();
  });

  it('ignores initial responses after unmount and cancels a scheduled activation poll', async () => {
    const graphLoad = deferred<RuntimeGraphDocument>();
    const resolvedView = renderWorkspace(port({ getGraph: vi.fn(() => graphLoad.promise) }));
    resolvedView.unmount();
    await act(async () => graphLoad.resolve(document));

    const failedLoad = deferred<RuntimeGraphDocument>();
    const rejectedView = renderWorkspace(port({ getGraph: vi.fn(() => failedLoad.promise) }));
    rejectedView.unmount();
    await act(async () => failedLoad.reject(new Error('offline')));

    vi.useFakeTimers();
    const pollingView = renderWorkspace(
      port({
        applyGraph: vi
          .fn()
          .mockResolvedValue({ protocolVersion: 1, revision: 2, state: 'pending' }),
        getGraphStatus: vi
          .fn()
          .mockResolvedValue({ activeRevision: 1, protocolVersion: 1, submittedRevision: 2 }),
      }),
    );
    await act(async () => Promise.resolve());
    fireEvent.click(screen.getByRole('button', { name: 'edit graph' }));
    await act(async () => Promise.resolve());
    const timerCount = vi.getTimerCount();
    expect(timerCount).toBeGreaterThan(0);
    pollingView.unmount();
    expect(vi.getTimerCount()).toBeLessThan(timerCount);
  });
});
