import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, expect, it, vi } from 'vitest';
import { App } from '../../app/App';
import type { AudioGraphModel } from '../../core/audio-graph/model';
import { StorybookMockControlPort } from '../../core/control-port/StorybookMockControlPort';
import type { DriverPort, DriverSnapshot } from '../../core/driver/DriverPort';
import { I18nProvider, i18n } from '../../core/i18n';
import type { LicensingPort } from '../../core/licensing/LicensingPort';
import { MixRelayProvider } from '../../core/theme/MixRelayProvider';
import { AudioNodeCard, type EndpointUpdate } from '../../core/ui/AudioNodeCard';
import { readyDriver } from './driverFixtures';

// Layout belongs to React Flow's browser tests; keep the real node fields and App/Driver wiring here.
vi.mock('../../core/audio-graph/AudioGraphCanvas', () => ({
  AudioGraphCanvas: ({
    model,
    onNodeEndpointChange,
  }: {
    model: AudioGraphModel;
    onNodeEndpointChange: (nodeId: string, update: EndpointUpdate) => void;
  }) =>
    model.nodes.map((node) => (
      <div key={node.id} data-testid={node.id}>
        <AudioNodeCard
          {...node.appearance}
          inputCount={node.inputPorts.length}
          outputCount={node.outputPorts.length}
          onEndpointChange={(update) => onNodeEndpointChange(node.id, update)}
        />
      </div>
    )),
}));

beforeEach(async () => {
  await i18n.changeLanguage('en');
});

function view(paid = false, snapshot: DriverSnapshot = readyDriver) {
  const device = snapshot.settings.devices[0];
  if (!device) throw new Error('fixture missing');
  const driver: DriverPort = {
    getSnapshot: vi.fn().mockResolvedValue(snapshot),
    mutate: vi.fn().mockResolvedValue({ ...snapshot, revision: 8 }),
  };
  const control = new StorybookMockControlPort({
    graph: {
      schemaVersion: 2,
      revision: 1,
      edges: [],
      nodes: [
        {
          id: device.spec.id,
          label: device.spec.displayName,
          type: 'virtual_endpoint',
          config: { ...device.spec, reference: { render: true, capture: true } },
        },
      ],
    },
    graphStatus: { protocolVersion: 1, activeRevision: 1, submittedRevision: 1 },
    status: {
      protocolVersion: 1,
      driverState: 'ready',
      sampleRate: 48000,
      bufferFrames: 128,
      underruns: 0,
      overruns: 0,
    },
  });
  const license: LicensingPort = {
    getStatus: vi.fn().mockResolvedValue({
      status: paid ? 'licensed' : 'free',
      configured: false,
      remainingDays: paid ? 10 : 0,
      expiresAtUnix: null,
    }),
    activate: vi.fn(),
    validate: vi.fn(),
    deactivate: vi.fn(),
    openCheckout: vi.fn(),
  };
  render(
    <MixRelayProvider>
      <I18nProvider>
        <App
          driverPort={driver}
          controlPort={control}
          licensingPort={license}
          updatesPort={null}
          runtimePort={null}
        />
      </I18nProvider>
    </MixRelayProvider>,
  );
  return { driver, control };
}

async function outputNode() {
  await screen.findAllByRole('button', { name: (name) => name.endsWith('Options') });
  const card = screen.getByTestId('device-a::host-sink');
  if (!(card instanceof HTMLElement)) throw new Error('missing driver output node');
  const scope = within(card);
  fireEvent.click(scope.getByRole('button', { name: (name) => name.endsWith('Options') }));
  await waitFor(() => expect(scope.getByRole('button', { name: /Sample rate$/ })).toBeEnabled());
  return scope;
}

it('edits the Driver from its node without a global settings action or modal', async () => {
  const { driver, control } = view();
  const graphChange = vi.spyOn(control, 'applyGraph');
  const node = await outputNode();
  expect(screen.queryByRole('button', { name: 'Driver settings' })).not.toBeInTheDocument();
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  expect(node.getByRole('textbox', { name: 'Device name' })).toHaveAttribute('readonly');
  fireEvent.click(node.getByRole('button', { name: /Sample rate$/ }));
  fireEvent.click(await screen.findByRole('option', { name: '44.1 kHz' }));
  await waitFor(() =>
    expect(driver.mutate).toHaveBeenCalledExactlyOnceWith({
      action: 'configure',
      expectedRevision: 7,
      spec: { ...readyDriver.settings.devices[0]?.spec, defaultSampleRate: 44100 },
    }),
  );
  expect(node.getByRole('button', { name: /Sample rate$/ })).toHaveTextContent('48 kHz');
  expect(graphChange).not.toHaveBeenCalled();
});

it('renames the same Driver identity from a paid node and retains native readback', async () => {
  const { driver } = view(true);
  const node = await outputNode();
  const name = node.getByRole('textbox', { name: 'Device name' });
  await waitFor(() => expect(name).not.toHaveAttribute('readonly'));
  fireEvent.change(name, { target: { value: 'Broadcast' } });
  fireEvent.blur(name);
  await waitFor(() =>
    expect(driver.mutate).toHaveBeenCalledExactlyOnceWith({
      action: 'configure',
      expectedRevision: 7,
      spec: { ...readyDriver.settings.devices[0]?.spec, displayName: 'Broadcast' },
    }),
  );
  expect(node.getByRole('button', { name: (name) => name.endsWith('Options') })).toHaveAttribute(
    'aria-expanded',
    'true',
  );
});

it('changes shared bit depth through the Driver API and keeps errors in the node', async () => {
  const { driver } = view();
  const node = await outputNode();
  vi.mocked(driver.mutate).mockRejectedValueOnce(new Error('native change failed'));
  fireEvent.click(node.getByRole('button', { name: /Bit depth$/ }));
  fireEvent.click(await screen.findByRole('option', { name: '32-bit Float' }));
  await waitFor(() =>
    expect(driver.mutate).toHaveBeenCalledExactlyOnceWith({
      action: 'set_sample_format',
      expectedRevision: 7,
      endpointId: 'device-a',
      sampleFormat: 'float32',
    }),
  );
  expect(await node.findAllByRole('alert')).not.toHaveLength(0);
  expect(node.getByRole('button', { name: /Bit depth$/ })).toHaveTextContent('Mixed');
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
});

it('updates both views of the same device from confirmed Driver sample-rate readback', async () => {
  const { driver } = view();
  const device = readyDriver.settings.devices[0];
  if (!device) throw new Error('fixture missing');
  vi.mocked(driver.mutate).mockResolvedValueOnce({
    ...readyDriver,
    revision: 8,
    settings: {
      ...readyDriver.settings,
      devices: [{ ...device, spec: { ...device.spec, defaultSampleRate: 44100 } }],
    },
  });
  const output = await outputNode();
  fireEvent.click(output.getByRole('button', { name: /Sample rate$/ }));
  fireEvent.click(await screen.findByRole('option', { name: '44.1 kHz' }));
  await waitFor(() =>
    expect(output.getByRole('button', { name: /Sample rate$/ })).toHaveTextContent('44.1 kHz'),
  );
  const input = within(screen.getByTestId('device-a::host-source'));
  fireEvent.click(input.getByRole('button', { name: (name) => name.endsWith('Options') }));
  expect(input.getByRole('button', { name: /Sample rate$/ })).toHaveTextContent('44.1 kHz');
  expect(input.queryByRole('button', { name: /Buffer size$/ })).not.toBeInTheDocument();
});
