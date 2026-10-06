import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { App } from '../../app/App';
import { StorybookMockControlPort } from '../../core/control-port/StorybookMockControlPort';
import type { DriverPort } from '../../core/driver/DriverPort';
import { I18nProvider, i18n } from '../../core/i18n';
import { MixRelayProvider } from '../../core/theme/MixRelayProvider';
import type { LicensingPort } from '../../core/licensing/LicensingPort';
import { DriverManagementAccess } from './DriverManagementAccess';
import { readyDriver } from './driverFixtures';

function native(): DriverPort {
  return {
    getSnapshot: vi.fn().mockResolvedValue(readyDriver),
    mutate: vi.fn().mockResolvedValue({ ...readyDriver, revision: 8 }),
  };
}
function view(port: DriverPort | null, paid = false) {
  const result = render(
    <MixRelayProvider>
      <I18nProvider>
        <DriverManagementAccess port={port} allowMultiDevice={paid}>
          {(action) => action}
        </DriverManagementAccess>
      </I18nProvider>
    </MixRelayProvider>,
  );
  if (port) fireEvent.click(screen.getByRole('button', { name: 'Driver settings' }));
  return result;
}
function graphPort() {
  return new StorybookMockControlPort({
    graph: { schemaVersion: 2, revision: 1, nodes: [], edges: [] },
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
}
beforeEach(async () => {
  await i18n.changeLanguage('en');
  // jsdom has no layout. Give the real Floating UI detached-reference check
  // visible geometry; keep popup accessibility and option selection unmocked.
  vi.spyOn(Element.prototype, 'getBoundingClientRect').mockReturnValue(
    new DOMRect(20, 20, 240, 32),
  );
  vi.spyOn(document.documentElement, 'clientWidth', 'get').mockReturnValue(1024);
  vi.spyOn(document.documentElement, 'clientHeight', 'get').mockReturnValue(768);
});
afterEach(() => vi.restoreAllMocks());

it('chooses exact directional PCM tuples and distinguishes 32-bit float from integer', async () => {
  const port = native();
  view(port);
  const select = await screen.findByRole('combobox', { name: 'Playback format' });
  await waitFor(() => expect(select).toBeEnabled());
  fireEvent.click(select);
  expect(
    await screen.findByRole('option', { name: '48 kHz · 2 ch · 32-bit Float' }),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole('option', { name: '44.1 kHz · 2 ch · 24-bit PCM' }));
  await waitFor(() =>
    expect(port.mutate).toHaveBeenCalledExactlyOnceWith({
      action: 'set_format',
      expectedRevision: 7,
      endpointId: 'device-a',
      direction: 'render',
      format: { sampleRate: 44100, channels: 2, sampleFormat: 'signed24' },
    }),
  );
  expect(screen.getByRole('combobox', { name: 'Playback format' })).toHaveValue(
    '48 kHz · 2 ch · 32-bit Float',
  );
  expect(screen.getByRole('combobox', { name: 'Capture format' })).toHaveValue(
    '48 kHz · 2 ch · 16-bit PCM',
  );
});

it('configures a free nominal rate when native PCM tuples describe only the current Linux rate', async () => {
  const port = native();
  const device = readyDriver.settings.devices[0]!;
  const currentDevice = {
    ...device,
    supportedSampleRates: [48000],
    nativeFormats: {
      ...device.nativeFormats,
      render: {
        ...device.nativeFormats.render,
        supported: device.nativeFormats.render.supported.filter(
          (format) => format.sampleRate === 48000,
        ),
      },
    },
  };
  const initial = {
    ...readyDriver,
    settings: { ...readyDriver.settings, devices: [currentDevice] },
  };
  vi.mocked(port.getSnapshot).mockResolvedValue(initial);
  vi.mocked(port.mutate).mockResolvedValue({
    ...initial,
    revision: 8,
    settings: {
      ...initial.settings,
      devices: [
        {
          ...currentDevice,
          spec: { ...device.spec, defaultSampleRate: 44100 },
          nativeFormats: {
            ...currentDevice.nativeFormats,
            render: {
              current: { sampleRate: 44100, channels: 2, sampleFormat: 'float32' },
              supported: [{ sampleRate: 44100, channels: 2, sampleFormat: 'float32' }],
            },
          },
        },
      ],
    },
  });
  view(port);
  const rate = await screen.findByRole('combobox', { name: 'Sample rate' });
  await waitFor(() => expect(rate).toBeEnabled());
  fireEvent.click(rate);
  fireEvent.click(await screen.findByRole('option', { name: '44.1 kHz' }));
  fireEvent.click(screen.getByRole('combobox', { name: 'Playback format' }));
  const currentFloat = await screen.findByRole('option', { name: '48 kHz · 2 ch · 32-bit Float' });
  expect(
    screen.queryByRole('option', { name: '44.1 kHz · 2 ch · 24-bit PCM' }),
  ).not.toBeInTheDocument();
  fireEvent.click(currentFloat);
  expect(port.mutate).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: 'Apply' }));
  await waitFor(() =>
    expect(port.mutate).toHaveBeenCalledExactlyOnceWith({
      action: 'configure',
      expectedRevision: 7,
      spec: { ...device.spec, defaultSampleRate: 44100 },
    }),
  );
  await waitFor(() =>
    expect(screen.getByRole('combobox', { name: 'Playback format' })).toHaveValue(
      '44.1 kHz · 2 ch · 32-bit Float',
    ),
  );
  expect(screen.queryByRole('button', { name: 'Create device' })).not.toBeInTheDocument();
  expect(screen.queryByRole('button', { name: 'Remove device' })).not.toBeInTheDocument();
});

it('configures the existing identity and requires a checkbox before explicit removal', async () => {
  const port = native();
  view(port, true);
  const name = await screen.findByRole('textbox', { name: 'Device name' });
  await waitFor(() => expect(name).toBeEnabled());
  fireEvent.change(name, { target: { value: 'Broadcast' } });
  fireEvent.click(screen.getByRole('button', { name: 'Apply' }));
  await waitFor(() =>
    expect(port.mutate).toHaveBeenCalledWith({
      action: 'configure',
      expectedRevision: 7,
      spec: { ...readyDriver.settings.devices[0]!.spec, displayName: 'Broadcast' },
    }),
  );
  fireEvent.click(screen.getByRole('button', { name: 'Remove device' }));
  const dialog = await screen.findByRole('dialog', { name: 'Remove device' });
  const remove = within(dialog).getByRole('button', { name: 'Remove device' });
  expect(remove).toBeDisabled();
  fireEvent.click(within(dialog).getByRole('checkbox'));
  fireEvent.click(remove);
  await waitFor(() =>
    expect(port.mutate).toHaveBeenLastCalledWith({
      action: 'remove',
      expectedRevision: 8,
      endpointId: 'device-a',
    }),
  );
});

it('creates a named device from the advertised template without graph API calls', async () => {
  const port = native();
  view(port, true);
  fireEvent.click(await screen.findByRole('button', { name: 'Create device' }));
  const dialog = await screen.findByRole('dialog', { name: 'Create device' });
  fireEvent.change(within(dialog).getByRole('textbox', { name: 'Device name' }), {
    target: { value: 'Loopback' },
  });
  fireEvent.click(within(dialog).getByRole('button', { name: 'Create' }));
  await waitFor(() =>
    expect(port.mutate).toHaveBeenCalledWith(
      expect.objectContaining({
        action: 'create',
        expectedRevision: 7,
        spec: expect.objectContaining({ displayName: 'Loopback', defaultSampleRate: 48000 }),
      }),
    ),
  );
});

it.each(['not_installed', 'installed_not_loaded', 'unavailable'] as const)(
  'shows %s without a fake installation or restart action',
  async (state) => {
    const port = native();
    vi.mocked(port.getSnapshot).mockResolvedValue({
      ...readyDriver,
      state,
      settings: { ...readyDriver.settings, devices: [] },
    });
    view(port);
    await waitFor(() => expect(port.getSnapshot).toHaveBeenCalled());
    await act(async () => Promise.resolve());
    expect(screen.queryByRole('button', { name: 'Create device' })).not.toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: /Install driver|Restart audio services/ }),
    ).not.toBeInTheDocument();
    expect(port.mutate).not.toHaveBeenCalled();
  },
);

it.each(['trial', 'licensed'] as const)(
  'allows multi-device actions only for paid App entitlement, not %s fallback',
  async (status) => {
    const license: LicensingPort = {
      getStatus: vi
        .fn()
        .mockResolvedValue({ status, configured: true, remainingDays: 10, expiresAtUnix: null }),
      activate: vi.fn(),
      validate: vi.fn(),
      deactivate: vi.fn(),
      openCheckout: vi.fn(),
    };
    render(
      <MixRelayProvider>
        <I18nProvider>
          <App
            driverPort={native()}
            controlPort={graphPort()}
            licensingPort={license}
            updatesPort={null}
          />
        </I18nProvider>
      </MixRelayProvider>,
    );
    await screen.findByRole('heading', { name: 'MixRelay' });
    fireEvent.click(screen.getByRole('button', { name: 'Driver settings' }));
    const dialog = await screen.findByRole('dialog', { name: 'Driver settings' });
    await within(dialog).findByRole('combobox', { name: 'Playback format' });
    if (status === 'licensed') {
      expect(within(dialog).getByRole('button', { name: 'Create device' })).toBeInTheDocument();
      expect(within(dialog).getByRole('button', { name: 'Remove device' })).toBeInTheDocument();
    } else {
      expect(
        within(dialog).queryByRole('button', { name: 'Create device' }),
      ).not.toBeInTheDocument();
      expect(
        within(dialog).queryByRole('button', { name: 'Remove device' }),
      ).not.toBeInTheDocument();
    }
  },
);

it('keeps Driver management accessible when the app license check fails', async () => {
  const driver = native();
  const license: LicensingPort = {
    getStatus: vi.fn().mockRejectedValue('unavailable'),
    activate: vi.fn(),
    validate: vi.fn(),
    deactivate: vi.fn(),
    openCheckout: vi.fn(),
  };
  render(
    <MixRelayProvider>
      <I18nProvider>
        <App driverPort={driver} licensingPort={license} updatesPort={null} />
      </I18nProvider>
    </MixRelayProvider>,
  );
  await screen.findByRole('alert');
  fireEvent.click(screen.getByRole('button', { name: 'Driver settings' }));
  const dialog = await screen.findByRole('dialog', { name: 'Driver settings' });
  expect(
    await within(dialog).findByRole('combobox', { name: 'Playback format' }),
  ).toBeInTheDocument();
  expect(screen.getAllByRole('button', { name: 'Driver settings' })).toHaveLength(1);
});
