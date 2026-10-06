import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { StorybookMockControlPort } from '../core/control-port/StorybookMockControlPort';
import type {
  AudioSessionStatus,
  ControlPort,
  RuntimeGraphDocument,
} from '../core/control-port/types';
import { I18nProvider } from '../core/i18n';
import { MixRelayProvider } from '../core/theme/MixRelayProvider';
import {
  unknownNativeFormats,
  virtualPolicy,
  virtualSpec,
} from '../features/routing-workspace/virtualEndpointFixtures';
import { StudioScreen } from './StudioScreen';
import styles from './StudioScreen.module.css';
import type { DriverSession } from '../features/driver-status/useDriverSession';

const callbacks = vi.hoisted(
  () => new Map<ControlPort, (document: RuntimeGraphDocument) => void>(),
);
interface WorkspaceBinding {
  controlPort: ControlPort;
  graphId?: string;
  sessionDocument?: RuntimeGraphDocument;
  audioSession?: AudioSessionStatus;
  onBusyChange: (busy: boolean) => void;
  onTitleChange: (title: string) => void;
  deviceDriverAction: {
    change: (endpointId: string, update: import('../core/ui/AudioNodeCard').EndpointUpdate) => void;
    enabled: boolean;
    error: { endpointId: string | null; message: string } | null;
  };
}
const workspaceBindings = vi.hoisted(() => new Map<ControlPort, WorkspaceBinding>());
const workspaceRenders = vi.hoisted(() => [] as WorkspaceBinding[]);
const sessionDisabled = vi.hoisted(() => new Map<ControlPort, boolean>());
vi.mock('../features/audio-session/AudioSessionControl', () => ({
  AudioSessionControl: ({
    controlPort,
    onApplied,
    disabled,
  }: {
    controlPort: ControlPort;
    onApplied: (document: RuntimeGraphDocument) => void;
    disabled: boolean;
  }) => {
    callbacks.set(controlPort, onApplied);
    sessionDisabled.set(controlPort, disabled);
    return null;
  },
}));
vi.mock('../features/routing-workspace/RoutingWorkspace', () => ({
  RoutingWorkspace: (binding: WorkspaceBinding) => {
    workspaceBindings.set(binding.controlPort, binding);
    workspaceRenders.push(binding);
    return <output data-testid="document">{JSON.stringify(binding.sessionDocument)}</output>;
  },
}));
function port(revision: number) {
  return new StorybookMockControlPort({
    graph: { revision, nodes: [], edges: [], schemaVersion: 2 },
    graphStatus: { protocolVersion: 1, activeRevision: revision, submittedRevision: revision },
    status: {
      protocolVersion: 1,
      driverState: 'ready',
      sampleRate: 48000,
      bufferFrames: 128,
      underruns: 0,
      overruns: 0,
    },
  });
}
function view(controlPort: ControlPort, driverSession?: DriverSession) {
  return (
    <MixRelayProvider>
      <I18nProvider>
        <StudioScreen controlPort={controlPort} driverSession={driverSession} />
      </I18nProvider>
    </MixRelayProvider>
  );
}

function newGraphShortcut(
  target: EventTarget,
  modifiers: { ctrlKey?: boolean; metaKey?: boolean } = {},
) {
  const event = new KeyboardEvent('keydown', {
    bubbles: true,
    cancelable: true,
    key: 'N',
    shiftKey: true,
    ...modifiers,
  });
  act(() => target.dispatchEvent(event));
  return event;
}

function devicePort() {
  const controlPort = Object.assign(port(1), { setVirtualEndpointSampleRate: vi.fn() });
  let sampleRate = 48000;
  let document: RuntimeGraphDocument = { schemaVersion: 2, revision: 1, nodes: [], edges: [] };
  const nativeStatus: AudioSessionStatus = {
    format: { sampleRate: 96000, blockFrames: 32 },
    clockAuthority: 'device',
    observedDeviceSampleRate: 96000,
    issue: null,
    transportLatencyFrames: null,
    capabilities: {
      backend: 'core_audio',
      supportedSampleRates: [44100, 48000, 96000],
      supportedBlockFrames: [32, 128],
      canReconfigureLive: true,
      hardwareInput: true,
      hardwareOutput: true,
      virtualEndpoint: true,
      applicationCapture: true,
      sampleRateConversion: true,
      driftCorrection: true,
    },
    virtualEndpointSettings: {
      ...virtualPolicy,
      devices: [
        {
          nativeDeviceIds: [],
          nativeFormats: unknownNativeFormats,
          spec: { ...virtualSpec, id: 'device-a' },
          canSetSampleRate: true,
          observedSampleRate: 48000,
          supportedSampleRates: [44100, 48000],
        },
      ],
    },
  };
  const endpointSettings = nativeStatus.virtualEndpointSettings;
  if (!endpointSettings) throw new Error('Missing device fixture');
  vi.spyOn(controlPort, 'getGraph').mockImplementation(async () => document);
  vi.spyOn(controlPort, 'getGraphStatus').mockImplementation(async () => ({
    protocolVersion: 1,
    activeRevision: document.revision,
    submittedRevision: document.revision,
  }));
  vi.spyOn(controlPort, 'getStatus').mockImplementation(async () => ({
    protocolVersion: 1,
    driverState: 'ready',
    sampleRate: 96000,
    bufferFrames: 32,
    underruns: 0,
    overruns: 0,
    audioSession: {
      ...nativeStatus,
      virtualEndpointSettings: {
        ...endpointSettings,
        devices: endpointSettings.devices.map((device) => ({
          ...device,
          observedSampleRate: sampleRate,
        })),
      },
    },
  }));
  controlPort.setVirtualEndpointSampleRate.mockImplementation(async ({ sampleRate: rate }) => {
    sampleRate = rate;
    document = { ...document, revision: 2 };
    return { protocolVersion: 1, revision: 2, state: 'active' };
  });
  return controlPort;
}

it('keeps the new-graph launcher stable across background saves but guards creation', async () => {
  const controlPort = port(1);
  const mutate = vi.spyOn(controlPort, 'mutateGraphCatalog');
  render(view(controlPort));
  const launcher = await screen.findByRole('button', { name: 'New graph' });
  await waitFor(() => expect(launcher).toBeEnabled());
  expect(launcher).toHaveAttribute('aria-keyshortcuts', 'Control+Shift+N');
  expect(launcher.className).toContain(styles.graph);
  expect(launcher.className).toContain(styles.newGraph);
  expect(launcher).not.toHaveClass('mantine-Button-root');
  vi.useFakeTimers();
  try {
    fireEvent.mouseEnter(launcher);
    await act(async () => vi.advanceTimersByTime(500));
    expect(screen.queryByRole('tooltip')).not.toBeInTheDocument();
  } finally {
    vi.useRealTimers();
  }
  const binding = workspaceBindings.get(controlPort);
  if (!binding) throw new Error('Workspace not mounted');
  let transitions = 0;
  let wasDisabled = launcher.hasAttribute('disabled');
  for (let cycle = 0; cycle < 20; cycle += 1) {
    for (const busy of [true, false]) {
      act(() => binding.onBusyChange(busy));
      const disabled = launcher.hasAttribute('disabled');
      if (disabled !== wasDisabled) transitions += 1;
      wasDisabled = disabled;
    }
  }
  expect(transitions).toBe(0);
  act(() => binding.onBusyChange(true));
  fireEvent.click(launcher);
  expect(screen.getByRole('dialog', { name: 'New graph' })).toBeInTheDocument();
  expect(screen.getByRole('button', { name: 'Create' })).toBeDisabled();
  fireEvent.click(screen.getByRole('button', { name: 'Create' }));
  expect(mutate).not.toHaveBeenCalled();
  act(() => binding.onBusyChange(false));
  expect(screen.getByRole('button', { name: 'Create' })).toBeEnabled();
  fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
});

it.each([
  ['MacIntel', { metaKey: true }],
  ['Linux x86_64', { ctrlKey: true }],
] as const)(
  'preserves text editing and uses the platform new-graph modifier on %s',
  async (platform, modifier) => {
    const identity = vi.spyOn(navigator, 'platform', 'get').mockReturnValue(platform);
    render(view(port(1)));
    const launcher = await screen.findByRole('button', { name: 'New graph' });
    expect(launcher).toHaveAttribute(
      'aria-keyshortcuts',
      `${platform === 'MacIntel' ? 'Meta' : 'Control'}+Shift+N`,
    );

    const input = document.createElement('input');
    input.type = 'text';
    const textarea = document.createElement('textarea');
    const editable = document.createElement('div');
    editable.setAttribute('contenteditable', 'true');
    const nested = document.createElement('span');
    editable.append(nested);
    document.body.append(input, textarea, editable);
    try {
      for (const target of [input, textarea, nested]) {
        const event = newGraphShortcut(target, modifier);
        expect(event.defaultPrevented, `shortcut intercepted from ${target.tagName}`).toBe(false);
        expect(screen.queryByRole('dialog', { name: 'New graph' })).not.toBeInTheDocument();
      }

      const claimed = new KeyboardEvent('keydown', {
        bubbles: true,
        cancelable: true,
        ...modifier,
        key: 'n',
        shiftKey: true,
      });
      claimed.preventDefault();
      act(() => document.body.dispatchEvent(claimed));
      expect(screen.queryByRole('dialog', { name: 'New graph' })).not.toBeInTheDocument();

      const wrongModifier = platform === 'MacIntel' ? { ctrlKey: true } : { metaKey: true };
      expect(newGraphShortcut(document.body, wrongModifier).defaultPrevented).toBe(false);
      expect(screen.queryByRole('dialog', { name: 'New graph' })).not.toBeInTheDocument();
      const canvasEvent = newGraphShortcut(document.body, modifier);
      expect(canvasEvent.defaultPrevented).toBe(true);
      expect(screen.getByRole('dialog', { name: 'New graph' })).toBeInTheDocument();
    } finally {
      input.remove();
      textarea.remove();
      editable.remove();
      identity.mockRestore();
    }
  },
);

it('uses a visible chevron tab to reopen the sidebar without changing the graph or header', async () => {
  const controlPort = port(1);
  const mutate = vi.spyOn(controlPort, 'mutateGraphCatalog');
  render(view(controlPort));
  await screen.findByRole('button', { name: 'Untitled Graph' });
  const logo = screen.getByRole('heading', { name: 'MixRelay' });
  const boundary = screen.getByRole('button', { name: 'Graphs' });
  expect(boundary.className).toContain(styles.sidebarBoundary);
  expect(boundary).toHaveAttribute('aria-expanded', 'true');
  fireEvent.click(boundary);
  const tab = screen.getByRole('button', { name: 'Graphs' });
  expect(tab.className).toContain(styles.sidebarReopen);
  expect(tab.className).not.toContain(styles.sidebarBoundary);
  expect(tab).toHaveAttribute('aria-expanded', 'false');
  expect(tab.querySelector('svg')).toHaveAttribute('aria-hidden', 'true');
  expect(screen.queryByRole('navigation', { name: 'Graphs' })).not.toBeInTheDocument();
  expect(logo).toBeVisible();
  expect(logo.parentElement).not.toContainElement(tab);
  fireEvent.click(tab);
  expect(screen.getByRole('navigation', { name: 'Graphs' })).toBeVisible();
  expect(screen.getByRole('button', { name: 'Graphs' }).className).toContain(
    styles.sidebarBoundary,
  );
  expect(mutate).not.toHaveBeenCalled();
});

it('sends device edits through the independent Driver owner during graph title edits', async () => {
  const controlPort = devicePort();
  const binding = () => {
    const current = workspaceBindings.get(controlPort);
    if (!current) throw new Error('Workspace not mounted');
    return current;
  };
  const changeDevice = vi.fn().mockResolvedValue(null);
  const driverSession: DriverSession = {
    port: null,
    snapshot: { protocolVersion: 1, revision: 4, state: 'ready', installedVersion: '0.1.0', settings: virtualPolicy },
    busy: false, error: null, endpointId: null,
    allowMultiDevice: false,
    refresh: vi.fn().mockResolvedValue(undefined), mutate: vi.fn().mockResolvedValue(null), changeDevice,
  };
  render(view(controlPort, driverSession));
  await waitFor(() => expect(binding().deviceDriverAction.enabled).toBe(true));
  act(() => binding().onTitleChange('Unsaved title'));
  expect(binding().deviceDriverAction.enabled).toBe(true);
  expect(sessionDisabled.get(controlPort)).toBe(true);
  const update = { displayName: 'Monitor', renderChannels: 2, captureChannels: 2,
    nativeFormat: { direction: 'render' as const, format: { sampleRate: 44100, channels: 2, sampleFormat: 'signed24' as const } } };
  await act(async () => {
    binding().deviceDriverAction.change('device-a', update);
  });
  expect(changeDevice).toHaveBeenCalledExactlyOnceWith('device-a', update);
  expect(controlPort.setVirtualEndpointSampleRate).not.toHaveBeenCalled();
});

describe('Studio session document ownership', () => {
  it('never shows the previous graph snapshot when selection keeps the same audio revision', async () => {
    const controlPort = port(7);
    const first = { schemaVersion: 2 as const, revision: 7, nodes: [], edges: [] };
    const second = {
      ...first,
      nodes: [{ id: 'other-device', type: 'audio_device_output', label: null, config: {} }],
    };
    const catalog = {
      protocolVersion: 1,
      catalogRevision: 1,
      activeId: 'a',
      graphs: [
        { id: 'a', name: 'Graph A' },
        { id: 'b', name: 'Graph B' },
      ],
      document: first,
    };
    vi.spyOn(controlPort, 'getGraphCatalog').mockResolvedValue(catalog);
    vi.spyOn(controlPort, 'mutateGraphCatalog').mockResolvedValue({
      ...catalog,
      catalogRevision: 2,
      activeId: 'b',
      document: second,
    });
    const rendered = render(view(controlPort));
    await screen.findByRole('button', { name: 'Graph B' });
    act(() => callbacks.get(controlPort)?.({ ...first, revision: 8 }));
    const oldCompletion = callbacks.get(controlPort);
    workspaceRenders.length = 0;
    fireEvent.click(screen.getByRole('button', { name: 'Graph B' }));
    await waitFor(() => expect(workspaceBindings.get(controlPort)?.graphId).toBe('b'));
    const nextView = workspaceRenders.filter((binding) => binding.graphId === 'b');
    expect(nextView.length).toBeGreaterThan(0);
    expect(nextView.every((binding) => binding.sessionDocument === second)).toBe(true);
    act(() => oldCompletion?.({ ...first, revision: 9 }));
    expect(workspaceBindings.get(controlPort)?.sessionDocument).toBe(second);
    rendered.unmount();
  });
  it('does not adopt a higher revision document from the previously connected engine', async () => {
    const a = port(100),
      b = port(1);
    const rendered = render(view(a));
    await waitFor(() => expect(screen.getByTestId('document')).toHaveTextContent('"revision":100'));
    const lateCallback = callbacks.get(a);
    rendered.rerender(view(b));
    await waitFor(() => expect(screen.getByTestId('document')).toHaveTextContent('"revision":1,'));
    act(() => callbacks.get(b)?.({ revision: 2, nodes: [], edges: [], schemaVersion: 2 }));
    expect(screen.getByTestId('document')).toHaveTextContent('"revision":2,');
    act(() => lateCallback?.({ revision: 101, nodes: [], edges: [], schemaVersion: 2 }));
    expect(screen.getByTestId('document')).toHaveTextContent('"revision":2,');
    expect(screen.getByTestId('document')).not.toHaveTextContent('"revision":101');
    rendered.unmount();
  });
});
