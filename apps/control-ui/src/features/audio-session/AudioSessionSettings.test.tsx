import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { expect, it, vi } from 'vitest';
import { StorybookMockControlPort } from '../../core/control-port/StorybookMockControlPort';
import type {
  AudioSessionStatus,
  GraphApplyResponse,
  RuntimeGraphDocument,
} from '../../core/control-port/types';
import { i18n } from '../../core/i18n';
import { ToneNariProvider } from '../../core/theme/ToneNariProvider';
import { AudioSessionSettings } from './AudioSessionSettings';

const session: AudioSessionStatus = {
  format: { sampleRate: 48000, blockFrames: 128 },
  clockAuthority: 'engine_timer',
  observedDeviceSampleRate: null,
  issue: null,
  transportLatencyFrames: null,
  capabilities: {
    backend: 'contract_only',
    supportedSampleRates: [48000],
    supportedBlockFrames: [128],
    canReconfigureLive: true,
    hardwareInput: false,
    hardwareOutput: false,
    virtualEndpoint: true,
    applicationCapture: false,
    sampleRateConversion: false,
    driftCorrection: false,
  },
};
const graph: RuntimeGraphDocument = { schemaVersion: 2, revision: 1, nodes: [], edges: [] };

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

function port() {
  return Object.assign(
    new StorybookMockControlPort({
      status: {
        protocolVersion: 1,
        driverState: 'ready',
        sampleRate: 48000,
        bufferFrames: 128,
        underruns: 0,
        overruns: 0,
      },
      graph,
      graphStatus: { protocolVersion: 1, activeRevision: 2, submittedRevision: 2 },
    }),
    {
      reconfigureSession: vi.fn(
        async (): Promise<GraphApplyResponse> => ({
          protocolVersion: 1,
          revision: 2,
          state: 'active',
        }),
      ),
    },
  );
}

function view(
  controlPort: ReturnType<typeof port>,
  onApplied = vi.fn(),
  onBusyChange = vi.fn(),
  sessionStatus = session,
  disabled = false,
) {
  return (
    <I18nextProvider i18n={i18n}>
      <ToneNariProvider>
        <AudioSessionSettings
          disabled={disabled}
          controlPort={controlPort}
          session={sessionStatus}
          onApplied={onApplied}
          onBusyChange={onBusyChange}
        />
      </ToneNariProvider>
    </I18nextProvider>
  );
}

function chooseBuffer(value = '128') {
  fireEvent.change(screen.getByRole('combobox'), { target: { value } });
}

it('stacks native and request errors and allows retry of the same single supported buffer', async () => {
  await i18n.changeLanguage('en');
  const controlPort = port();
  controlPort.reconfigureSession.mockRejectedValueOnce(new Error('Failed'));
  render(
    <I18nextProvider i18n={i18n}>
      <ToneNariProvider>
        <AudioSessionSettings
          controlPort={controlPort}
          session={{ ...session, issue: 'stored_format_unavailable' }}
          sessionAlerts={['Native format failed', 'Preferences unavailable']}
          onApplied={vi.fn()}
          onBusyChange={vi.fn()}
        />
      </ToneNariProvider>
    </I18nextProvider>,
  );
  chooseBuffer();
  await waitFor(() =>
    expect(screen.getByRole('alert')).toHaveTextContent(i18n.t('audioSession.changeFailed')),
  );
  expect(screen.getAllByRole('alert')).toHaveLength(1);
  expect(screen.getByRole('alert')).toHaveTextContent('Native format failed');
  expect(screen.getByRole('alert')).toHaveTextContent('Preferences unavailable');
  fireEvent.click(screen.getByRole('button', { name: 'Apply' }));
  await waitFor(() => expect(controlPort.reconfigureSession).toHaveBeenCalledTimes(2));
  expect(controlPort.reconfigureSession).toHaveBeenLastCalledWith({
    expectedRevision: 1,
    format: session.format,
  });
});

it('blocks buffer writes, including dispatched change events, while graph edits are uncommitted', async () => {
  const controlPort = port();
  const onApplied = vi.fn();
  const onBusyChange = vi.fn();
  render(view(controlPort, onApplied, onBusyChange, session, true));
  expect(screen.getByRole('combobox')).toBeDisabled();
  chooseBuffer();
  await act(async () => {});
  expect(controlPort.reconfigureSession).not.toHaveBeenCalled();
  expect(onApplied).not.toHaveBeenCalled();
  expect(onBusyChange).not.toHaveBeenCalled();
});

it('submits advertised processing buffer 32 without changing the clock-owned rate', async () => {
  await i18n.changeLanguage('en');
  const controlPort = port();
  const editable = {
    ...session,
    capabilities: { ...session.capabilities, supportedBlockFrames: [32, 64, 128, 256] },
  };
  render(view(controlPort, vi.fn(), vi.fn(), editable));
  fireEvent.click(screen.getByRole('combobox', { name: 'Buffer size' }));
  chooseBuffer('32');
  expect(screen.getByRole('combobox', { name: 'Buffer size' })).toHaveValue('32');
  await waitFor(() =>
    expect(controlPort.reconfigureSession).toHaveBeenCalledWith({
      expectedRevision: graph.revision,
      format: { sampleRate: session.format.sampleRate, blockFrames: 32 },
    }),
  );
});

it('exposes all advertised buffers without offering an independent graph sample rate', async () => {
  await i18n.changeLanguage('en');
  const controlPort = port();
  const rates = [44100, 48000, 88200, 96000, 176400, 192000];
  const buffers = [32, 64, 128, 256, 512, 1024, 2048];
  render(
    view(controlPort, vi.fn(), vi.fn(), {
      ...session,
      format: { sampleRate: 192000, blockFrames: 128 },
      capabilities: {
        ...session.capabilities,
        supportedSampleRates: rates,
        supportedBlockFrames: buffers,
      },
    }),
  );
  expect(screen.queryByRole('combobox', { name: 'Sample rate' })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('combobox', { name: 'Buffer size' }));
  for (const buffer of buffers)
    expect(screen.getByRole('option', { name: String(buffer) })).toBeInTheDocument();
  chooseBuffer('2048');
  await waitFor(() =>
    expect(controlPort.reconfigureSession).toHaveBeenCalledWith({
      expectedRevision: graph.revision,
      format: { sampleRate: 192000, blockFrames: 2048 },
    }),
  );
});

it('does not submit stale choices while native format discovery is unavailable', async () => {
  await i18n.changeLanguage('en');
  const controlPort = port();
  const unavailable = {
    ...session,
    capabilities: { ...session.capabilities, supportedBlockFrames: [] },
  };
  const { rerender } = render(view(controlPort, vi.fn(), vi.fn(), unavailable));
  expect(screen.getByRole('combobox')).toBeDisabled();
  chooseBuffer();
  expect(controlPort.reconfigureSession).not.toHaveBeenCalled();
  rerender(view(controlPort));
  expect(screen.getByRole('combobox')).toBeEnabled();
  chooseBuffer();
  await waitFor(() => expect(controlPort.reconfigureSession).toHaveBeenCalledOnce());
});

it('does not deliver an old port final document or clear the new owner busy state', async () => {
  await i18n.changeLanguage('en');
  const oldPort = port();
  const nextPort = port();
  const oldRead = deferred<RuntimeGraphDocument>();
  const nextRead = deferred<RuntimeGraphDocument>();
  vi.spyOn(oldPort, 'getGraph').mockResolvedValueOnce(graph).mockReturnValueOnce(oldRead.promise);
  vi.spyOn(nextPort, 'getGraph').mockReturnValueOnce(nextRead.promise);
  const onApplied = vi.fn();
  const onBusyChange = vi.fn();
  const rendered = render(view(oldPort, onApplied, onBusyChange));
  chooseBuffer();
  await waitFor(() => expect(oldPort.getGraph).toHaveBeenCalledTimes(2));
  rendered.rerender(view(nextPort, onApplied, onBusyChange));
  chooseBuffer();
  await waitFor(() => expect(nextPort.getGraph).toHaveBeenCalledTimes(1));
  const busyCalls = onBusyChange.mock.calls.length;
  await act(async () => oldRead.resolve({ ...graph, revision: 100 }));
  expect(onApplied).not.toHaveBeenCalled();
  expect(onBusyChange).toHaveBeenCalledTimes(busyCalls);
  expect(screen.getByRole('combobox')).toBeDisabled();
  rendered.unmount();
  await act(async () => nextRead.resolve(graph));
  expect(nextPort.reconfigureSession).not.toHaveBeenCalled();
});

it('does not issue a native reconfiguration after unmounting during the initial read', async () => {
  await i18n.changeLanguage('en');
  const controlPort = port();
  const read = deferred<RuntimeGraphDocument>();
  vi.spyOn(controlPort, 'getGraph').mockReturnValue(read.promise);
  const onApplied = vi.fn();
  const onBusyChange = vi.fn();
  const rendered = render(view(controlPort, onApplied, onBusyChange));
  chooseBuffer();
  rendered.unmount();
  const busyCalls = onBusyChange.mock.calls.length;
  await act(async () => read.resolve(graph));
  expect(controlPort.reconfigureSession).not.toHaveBeenCalled();
  expect(onApplied).not.toHaveBeenCalled();
  expect(onBusyChange).toHaveBeenCalledTimes(busyCalls);
});

it('ignores a rejected old request after replacing the port', async () => {
  await i18n.changeLanguage('en');
  const oldPort = port();
  const read = deferred<RuntimeGraphDocument>();
  vi.spyOn(oldPort, 'getGraph').mockReturnValue(read.promise);
  const onBusyChange = vi.fn();
  const rendered = render(view(oldPort, vi.fn(), onBusyChange));
  chooseBuffer();
  rendered.rerender(view(port(), vi.fn(), onBusyChange));
  const busyCalls = onBusyChange.mock.calls.length;
  await act(async () => read.reject(new Error('old connection failed')));
  expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  expect(onBusyChange).toHaveBeenCalledTimes(busyCalls);
  expect(screen.getByRole('combobox')).toBeEnabled();
});

it.each(['reconfiguration', 'activation'] as const)(
  'stops the old continuation after unmounting during %s',
  async (stage) => {
    await i18n.changeLanguage('en');
    const controlPort = port();
    const pending = deferred<{
      protocolVersion: 1;
      revision: number;
      state: 'active';
      activeRevision: number;
      submittedRevision: number;
    }>();
    vi.spyOn(controlPort, 'getGraph');
    const status = vi.spyOn(controlPort, 'getGraphStatus');
    if (stage === 'reconfiguration')
      controlPort.reconfigureSession.mockReturnValueOnce(pending.promise);
    else status.mockReturnValueOnce(pending.promise);
    const onApplied = vi.fn();
    const onBusyChange = vi.fn();
    const rendered = render(view(controlPort, onApplied, onBusyChange));
    chooseBuffer();
    await waitFor(() =>
      expect(stage === 'activation' ? status : controlPort.reconfigureSession).toHaveBeenCalled(),
    );
    rendered.unmount();
    const busyCalls = onBusyChange.mock.calls.length;
    await act(async () =>
      pending.resolve({
        protocolVersion: 1,
        revision: 2,
        state: 'active',
        activeRevision: 2,
        submittedRevision: 2,
      }),
    );
    expect(controlPort.getGraph).toHaveBeenCalledTimes(1);
    expect(status).toHaveBeenCalledTimes(stage === 'activation' ? 1 : 0);
    expect(onApplied).not.toHaveBeenCalled();
    expect(onBusyChange).toHaveBeenCalledTimes(busyCalls);
  },
);

it('still publishes a current owner session exactly once', async () => {
  await i18n.changeLanguage('en');
  const controlPort = port();
  const next = { ...graph, revision: 2 };
  vi.spyOn(controlPort, 'getGraph').mockResolvedValueOnce(graph).mockResolvedValueOnce(next);
  const onApplied = vi.fn();
  const onBusyChange = vi.fn();
  render(view(controlPort, onApplied, onBusyChange));
  chooseBuffer();
  chooseBuffer();
  await waitFor(() => expect(onApplied).toHaveBeenCalledWith(next));
  expect(onApplied).toHaveBeenCalledTimes(1);
  expect(controlPort.reconfigureSession).toHaveBeenCalledTimes(1);
  expect(onBusyChange.mock.calls).toEqual([[true], [false]]);
});

it('follows external format changes in untouched fields without sending a native write', async () => {
  await i18n.changeLanguage('en');
  const controlPort = port();
  const rendered = render(view(controlPort));
  const nextSession: AudioSessionStatus = {
    ...session,
    format: { sampleRate: 44100, blockFrames: 256 },
    capabilities: {
      ...session.capabilities,
      supportedSampleRates: [44100, 48000],
      supportedBlockFrames: [128, 256],
    },
  };
  rendered.rerender(view(controlPort, vi.fn(), vi.fn(), nextSession));
  expect(screen.queryByRole('combobox', { name: 'Sample rate' })).not.toBeInTheDocument();
  expect(screen.getByRole('combobox', { name: 'Buffer size' })).toHaveValue('256');
  expect(controlPort.reconfigureSession).not.toHaveBeenCalled();
  rendered.rerender(view(port(), vi.fn(), vi.fn(), nextSession));
  expect(screen.getByRole('combobox', { name: 'Buffer size' })).toHaveValue('256');
});

it('uses the current clock rate for a selection after an external format change', async () => {
  await i18n.changeLanguage('en');
  const controlPort = port();
  const changed = {
    ...session,
    format: { sampleRate: 44100, blockFrames: 512 },
    capabilities: { ...session.capabilities, supportedBlockFrames: [128, 256, 512] },
  };
  const rendered = render(view(controlPort));
  rendered.rerender(view(controlPort, vi.fn(), vi.fn(), changed));
  chooseBuffer('256');
  await waitFor(() =>
    expect(controlPort.reconfigureSession).toHaveBeenCalledExactlyOnceWith({
      expectedRevision: graph.revision,
      format: { sampleRate: 44100, blockFrames: 256 },
    }),
  );
  // An unchanged stale status poll must not flash back to the old buffer.
  rendered.rerender(view(controlPort, vi.fn(), vi.fn(), { ...changed }));
  expect(screen.getByRole('combobox')).toHaveValue('256');
  rendered.rerender(view(port(), vi.fn(), vi.fn(), changed));
  expect(screen.getByRole('combobox')).toHaveValue('512');
});

it('defers external field updates during apply and adopts them when the request finishes', async () => {
  await i18n.changeLanguage('en');
  const controlPort = port();
  const pending = deferred<GraphApplyResponse>();
  controlPort.reconfigureSession.mockReturnValueOnce(pending.promise);
  const editable = {
    ...session,
    capabilities: {
      ...session.capabilities,
      supportedSampleRates: [44100, 48000],
      supportedBlockFrames: [128, 256],
    },
  };
  const applied = vi.fn();
  const busy = vi.fn();
  const rendered = render(view(controlPort, applied, busy, editable));
  chooseBuffer();
  await waitFor(() => expect(controlPort.reconfigureSession).toHaveBeenCalledTimes(1));
  rendered.rerender(
    view(controlPort, applied, busy, {
      ...editable,
      format: { sampleRate: 44100, blockFrames: 256 },
    }),
  );
  expect(screen.getByRole('combobox', { name: 'Buffer size' })).toHaveValue('128');
  expect(screen.getByRole('combobox', { name: 'Buffer size' })).toBeDisabled();
  await act(async () => pending.resolve({ protocolVersion: 1, revision: 2, state: 'active' }));
  expect(screen.getByRole('combobox', { name: 'Buffer size' })).toHaveValue('256');
  expect(controlPort.reconfigureSession).toHaveBeenCalledTimes(1);
  expect(controlPort.reconfigureSession).toHaveBeenCalledWith({
    expectedRevision: graph.revision,
    format: session.format,
  });
});
