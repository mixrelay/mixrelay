import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { StorybookMockControlPort } from '../../core/control-port/StorybookMockControlPort';
import type {
  RecorderRequest,
  RecorderSettings,
  RecorderStatus,
} from '../../core/control-port/types';
import { I18nProvider, i18n } from '../../core/i18n';
import { MixRelayProvider } from '../../core/theme/MixRelayProvider';
import { RecorderControls } from './RecorderControls';

const stopped: RecorderStatus = {
  state: 'stopped',
  path: null,
  sampleRate: null,
  encoding: null,
  frames: 0,
  error: null,
};
function fixture() {
  const port = new StorybookMockControlPort({
    graph: { schemaVersion: 2, revision: 1, nodes: [], edges: [] },
    graphStatus: { protocolVersion: 1, activeRevision: 1, submittedRevision: 1 },
    status: {
      protocolVersion: 1,
      driverState: 'ready',
      sampleRate: 48000,
      bufferFrames: 256,
      underruns: 0,
      overruns: 0,
    },
  });
  let status = stopped;
  vi.spyOn(port, 'recorderStatus').mockImplementation(async () => status);
  const write = vi
    .spyOn(port, 'controlRecorder')
    .mockImplementation(async (_graph, _node, request: RecorderRequest) => {
      status =
        request.action === 'record'
          ? {
              ...status,
              state: 'recording',
              path: '/Desktop/engine-owned.wav',
              sampleRate: 48000,
              encoding: 'pcm16',
              frames: 96000,
            }
          : { ...status, state: request.action === 'pause' ? 'paused' : 'stopped' };
      return status;
    });
  const choose = vi.fn().mockResolvedValue('/Chosen');
  const native = Object.assign(port, { selectRecordingFolder: choose });
  const save = vi.spyOn(port, 'setRecordingSettings');
  return { port: native, write, choose, save, apply: vi.spyOn(port, 'applyGraph') };
}
function recorderView(
  port: StorybookMockControlPort,
  options: {
    enabled?: boolean;
    poweredOn?: boolean;
    graphId?: string;
    nodeId?: string;
    onChannelsChange?: (channels: number) => void;
  } = {},
) {
  return (
    <MixRelayProvider>
      <I18nProvider>
        <RecorderControls
          controlPort={port}
          graphId={options.graphId ?? 'graph-a'}
          nodeId={options.nodeId ?? 'recorder-1'}
          expectedRevision={1}
          poweredOn={options.poweredOn ?? true}
          enabled={options.enabled ?? true}
          channels={2}
          onChannelsChange={options.onChannelsChange ?? vi.fn()}
        />
      </I18nProvider>
    </MixRelayProvider>
  );
}
async function openOptions() {
  fireEvent.click(screen.getByRole('button', { name: i18n.t('routing.nodes.options') }));
  await waitFor(() =>
    expect(screen.getByLabelText(i18n.t('recorder.folder'))).toHaveValue('/Desktop'),
  );
}
beforeEach(async () => {
  await i18n.changeLanguage('en');
  vi.spyOn(Element.prototype, 'getBoundingClientRect').mockReturnValue(
    new DOMRect(20, 20, 240, 32),
  );
  vi.spyOn(document.documentElement, 'clientWidth', 'get').mockReturnValue(1024);
  vi.spyOn(document.documentElement, 'clientHeight', 'get').mockReturnValue(768);
});
afterEach(() => vi.restoreAllMocks());

it.each(['en', 'ko-KR', 'pseudo'])(
  'shows a readonly shared folder and native chooser in %s',
  async (locale) => {
    await i18n.changeLanguage(locale);
    const { port, choose, save, apply } = fixture();
    render(recorderView(port));
    await openOptions();
    const folder = screen.getByLabelText(i18n.t('recorder.folder'));
    expect(folder).toHaveAttribute('readonly');
    fireEvent.click(screen.getByRole('button', { name: i18n.t('recorder.chooseFolder') }));
    await waitFor(() => expect(folder).toHaveValue('/Chosen'));
    expect(choose).toHaveBeenCalledExactlyOnceWith({
      defaultPath: '/Desktop',
      title: i18n.t('recorder.chooseFolder'),
    });
    expect(save).toHaveBeenCalledExactlyOnceWith(
      'graph-a',
      'recorder-1',
      { folder: '/Chosen', sampleRate: null, encoding: 'pcm16' },
      1,
    );
    expect(apply).not.toHaveBeenCalled();
  },
);

it('keeps closed Options compact and sends action-only Record/Pause/Resume/Stop', async () => {
  const { port, write, apply } = fixture();
  render(recorderView(port));
  expect(screen.queryByLabelText('Recording folder')).not.toBeInTheDocument();
  const record = screen.getByRole('button', { name: 'Record' });
  await waitFor(() => expect(record).toBeEnabled());
  fireEvent.click(record);
  await waitFor(() => expect(screen.getByRole('button', { name: 'Pause' })).toBeEnabled());
  fireEvent.click(screen.getByRole('button', { name: 'Pause' }));
  fireEvent.click(await screen.findByRole('button', { name: 'Resume' }));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Stop' })).toBeEnabled());
  fireEvent.click(screen.getByRole('button', { name: 'Stop' }));
  await waitFor(() => expect(write).toHaveBeenCalledTimes(4));
  expect(write.mock.calls.map((call) => call[2])).toEqual([
    { action: 'record' },
    { action: 'pause' },
    { action: 'record' },
    { action: 'stop' },
  ]);
  expect(apply).not.toHaveBeenCalled();
});

it('saves encoding and sample rate independently without graph mutation', async () => {
  const { port, save, apply } = fixture();
  render(recorderView(port));
  await openOptions();
  fireEvent.click(screen.getByRole('combobox', { name: 'WAV sample rate' }));
  fireEvent.click(await screen.findByRole('option', { name: '96 kHz' }));
  await waitFor(() => expect(save).toHaveBeenCalledTimes(1));
  await waitFor(() => expect(screen.getByRole('combobox', { name: 'Bit depth' })).toBeEnabled());
  fireEvent.click(screen.getByRole('combobox', { name: 'Bit depth' }));
  fireEvent.click(await screen.findByRole('option', { name: '24-bit PCM' }));
  await waitFor(() =>
    expect(save).toHaveBeenLastCalledWith(
      'graph-a',
      'recorder-1',
      { folder: '/Desktop', sampleRate: 96000, encoding: 'pcm24' },
      1,
    ),
  );
  expect(apply).not.toHaveBeenCalled();
});

it('blocks Record until the settings save completes and preserves folder after cancellation', async () => {
  const { port, choose, save, write } = fixture();
  let finish: (settings: RecorderSettings) => void = () => undefined;
  save.mockImplementationOnce(
    () =>
      new Promise((done) => {
        finish = done;
      }),
  );
  render(recorderView(port));
  await openOptions();
  fireEvent.click(screen.getByRole('button', { name: 'Choose folder…' }));
  await waitFor(() => expect(save).toHaveBeenCalledTimes(1));
  expect(screen.getByRole('button', { name: 'Record' })).toBeDisabled();
  expect(write).not.toHaveBeenCalled();
  finish({ folder: '/Chosen', sampleRate: null, encoding: 'pcm16' });
  await waitFor(() => expect(screen.getByRole('button', { name: 'Record' })).toBeEnabled());
  choose.mockResolvedValueOnce(null);
  fireEvent.click(screen.getByRole('button', { name: 'Choose folder…' }));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Record' })).toBeEnabled());
  expect(screen.getByLabelText('Recording folder')).toHaveValue('/Chosen');
  expect(save).toHaveBeenCalledTimes(1);
});

it('shows a save failure and permits an explicit chooser retry without recording an unsaved edit', async () => {
  const { port, save, write } = fixture();
  save.mockRejectedValueOnce(new Error('Disk full'));
  render(recorderView(port));
  await openOptions();
  fireEvent.click(screen.getByRole('button', { name: 'Choose folder…' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Disk full');
  expect(screen.getByRole('button', { name: 'Record' })).toBeDisabled();
  expect(write).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: 'Choose folder…' }));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Record' })).toBeEnabled());
});

it('shows settings load failure without fabricating a folder', async () => {
  const { port } = fixture();
  vi.spyOn(port, 'recordingSettings').mockRejectedValue(new Error('Desktop unavailable'));
  render(recorderView(port));
  expect(await screen.findByRole('alert')).toHaveTextContent('Desktop unavailable');
  expect(screen.getByRole('button', { name: 'Record' })).toBeDisabled();
});

it('ignores a late folder selection when graph and node props are replaced', async () => {
  const { port, choose, save } = fixture();
  let finish: (folder: string) => void = () => undefined;
  choose.mockImplementationOnce(
    () =>
      new Promise((done) => {
        finish = done;
      }),
  );
  const result = render(recorderView(port));
  await openOptions();
  fireEvent.click(screen.getByRole('button', { name: 'Choose folder…' }));
  await waitFor(() => expect(choose).toHaveBeenCalledTimes(1));
  result.rerender(recorderView(port, { graphId: 'graph-b', nodeId: 'recorder-2' }));
  finish('/wrong');
  await waitFor(() => expect(screen.getByRole('button', { name: 'Record' })).toBeEnabled());
  expect(save).not.toHaveBeenCalled();
  expect(screen.getByLabelText('Recording folder')).toHaveValue('/Desktop');
});

it('preserves channels 1–8 and locks channel/settings edits for active takes', async () => {
  const { port } = fixture();
  const onChannelsChange = vi.fn();
  render(recorderView(port, { onChannelsChange }));
  await openOptions();
  fireEvent.click(screen.getByRole('combobox', { name: 'Channels' }));
  const options = await screen.findAllByRole('option');
  expect(options.map((option) => option.textContent)).toEqual([
    '1',
    '2',
    '3',
    '4',
    '5',
    '6',
    '7',
    '8',
  ]);
  fireEvent.click(screen.getByRole('option', { name: '8' }));
  expect(onChannelsChange).toHaveBeenCalledWith(8);
  fireEvent.click(screen.getByRole('button', { name: 'Record' }));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Pause' })).toBeEnabled());
  for (const label of ['Channels', 'WAV sample rate', 'Bit depth'])
    expect(screen.getByRole('combobox', { name: label })).toBeDisabled();
  expect(screen.getByRole('button', { name: 'Choose folder…' })).toBeDisabled();
});

it('blocks new takes for a blocked graph but preserves controls for its existing take', async () => {
  const { port, write } = fixture();
  const result = render(recorderView(port, { enabled: false }));
  expect(screen.getByRole('button', { name: 'Record' })).toBeDisabled();
  expect(write).not.toHaveBeenCalled();
  result.rerender(recorderView(port));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Record' })).toBeEnabled());
  fireEvent.click(screen.getByRole('button', { name: 'Record' }));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Pause' })).toBeEnabled());
  result.rerender(recorderView(port, { enabled: false }));
  fireEvent.click(screen.getByRole('button', { name: 'Pause' }));
  expect(await screen.findByRole('button', { name: 'Resume' })).toBeEnabled();
  expect(screen.getByRole('button', { name: 'Stop' })).toBeEnabled();
});

it('does not resume a paused take while the engine is powered off', async () => {
  const { port, write } = fixture();
  vi.spyOn(port, 'recorderStatus').mockResolvedValue({
    ...stopped,
    state: 'paused',
    path: '/take.wav',
  });
  render(recorderView(port, { poweredOn: false }));
  expect(await screen.findByRole('button', { name: 'Resume' })).toBeDisabled();
  expect(write).not.toHaveBeenCalled();
});

it('keeps Stop available and shows actual take format when canonical preferences cannot be read', async () => {
  const { port, write } = fixture();
  vi.spyOn(port, 'recorderStatus').mockResolvedValue({
    ...stopped,
    state: 'recording',
    sampleRate: 48000,
    encoding: 'pcm24',
  });
  vi.spyOn(port, 'recordingSettings').mockRejectedValue(new Error('Preferences unavailable'));
  render(recorderView(port));
  expect(await screen.findByRole('alert')).toHaveTextContent('Preferences unavailable');
  fireEvent.click(screen.getByRole('button', { name: 'Options' }));
  expect(screen.getByRole('combobox', { name: 'WAV sample rate' })).toHaveValue('48 kHz');
  expect(screen.getByRole('combobox', { name: 'Bit depth' })).toHaveValue('24-bit PCM');
  expect(screen.getByRole('button', { name: 'Pause' })).toBeEnabled();
  fireEvent.click(screen.getByRole('button', { name: 'Stop' }));
  await waitFor(() =>
    expect(write).toHaveBeenCalledExactlyOnceWith('graph-a', 'recorder-1', { action: 'stop' }, 1),
  );
});
