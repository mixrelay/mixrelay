import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { StorybookMockControlPort } from '../../core/control-port/StorybookMockControlPort';
import type { RecorderRequest, RecorderStatus } from '../../core/control-port/types';
import { I18nProvider, i18n } from '../../core/i18n';
import { MixRelayProvider } from '../../core/theme/MixRelayProvider';
import { AudioNodeCard } from '../../core/ui/AudioNodeCard';
import { NodePowerButton } from '../../core/ui/NodeSignalControls';
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
              path: request.path ?? status.path ?? null,
              sampleRate: request.sampleRate ?? 48000,
              encoding: request.encoding ?? 'float32',
              frames: 96000,
            }
          : { ...status, state: request.action === 'pause' ? 'paused' : 'stopped' };
      return status;
    });
  return { port, write };
}
function view(port: StorybookMockControlPort, enabled = true) {
  const result = render(recorderView(port, enabled));
  fireEvent.click(screen.getByRole('button', { name: 'Options' }));
  return result;
}
function recorderView(
  port: StorybookMockControlPort,
  enabled: boolean,
  onChannelsChange = vi.fn(),
  poweredOn = true,
) {
  return (
    <MixRelayProvider>
      <I18nProvider>
        <RecorderControls
          controlPort={port}
          graphId="graph-a"
          nodeId="recorder-1"
          expectedRevision={1}
          poweredOn={poweredOn}
          enabled={enabled}
          channels={2}
          onChannelsChange={onChannelsChange}
        />
      </I18nProvider>
    </MixRelayProvider>
  );
}
beforeEach(async () => {
  await i18n.changeLanguage('en');
  // Floating UI hides detached/zero-size reference elements; jsdom has no layout engine.
  vi.spyOn(Element.prototype, 'getBoundingClientRect').mockReturnValue(
    new DOMRect(20, 20, 240, 32),
  );
  vi.spyOn(document.documentElement, 'clientWidth', 'get').mockReturnValue(1024);
  vi.spyOn(document.documentElement, 'clientHeight', 'get').mockReturnValue(768);
});

it('does not resume a paused take while its node is powered off', async () => {
  const { port, write } = fixture();
  vi.spyOn(port, 'recorderStatus').mockResolvedValue({
    ...stopped,
    state: 'paused',
    path: '/take.wav',
  });
  render(recorderView(port, true, vi.fn(), false));
  const resume = await screen.findByRole('button', { name: 'Resume' });
  expect(resume).toBeDisabled();
  fireEvent.click(resume);
  expect(write).not.toHaveBeenCalled();
});
afterEach(() => vi.restoreAllMocks());

it('keeps OFF recorder header power available while recording remains disabled', async () => {
  const { port, write } = fixture();
  const onPower = vi.fn();
  render(
    <MixRelayProvider>
      <I18nProvider>
        <AudioNodeCard
          title="Recorder"
          category="Recorder"
          icon="REC"
          inputCount={2}
          outputCount={0}
          onSelect={vi.fn()}
          poweredOff
          headerActions={
            <NodePowerButton
              enabled={false}
              disabled={false}
              label="Enable Recorder"
              onChange={onPower}
            />
          }
          content={
            <RecorderControls
              controlPort={port}
              graphId="graph-a"
              nodeId="recorder-1"
              expectedRevision={1}
              poweredOn={false}
              enabled={false}
              channels={2}
              onChannelsChange={vi.fn()}
            />
          }
        />
      </I18nProvider>
    </MixRelayProvider>,
  );
  const record = await screen.findByRole('button', { name: 'Record' });
  expect(record).toBeDisabled();
  const power = screen.getByRole('button', { name: 'Enable Recorder' });
  const options = screen.getByRole('button', { name: 'Options' });
  expect(options.parentElement).not.toContainElement(power);
  expect(power.closest('[data-header-actions]')).toBeInTheDocument();
  expect(power.closest('[inert]')).toBeNull();
  fireEvent.click(power);
  fireEvent.click(record);
  expect(onPower).toHaveBeenCalledOnce();
  expect(write).not.toHaveBeenCalled();
  expect(options).toHaveAttribute('aria-expanded', 'false');
});

it('records to Desktop with Options closed and proposes a fresh path after Stop', async () => {
  const { port, write } = fixture();
  const defaults = vi
    .fn()
    .mockResolvedValueOnce('/Desktop/first.wav')
    .mockResolvedValue('/Desktop/next.wav');
  const native = Object.assign(port, { defaultRecordingPath: defaults });
  render(recorderView(native, true));
  const record = screen.getByRole('button', { name: 'Record' });
  await waitFor(() => expect(record).toBeEnabled());
  expect(screen.getByRole('button', { name: 'Options' })).toHaveAttribute('aria-expanded', 'false');
  fireEvent.click(record);
  await waitFor(() =>
    expect(write).toHaveBeenCalledWith(
      'graph-a',
      'recorder-1',
      {
        action: 'record',
        path: '/Desktop/first.wav',
        sampleRate: null,
        encoding: 'pcm16',
      },
      1,
    ),
  );
  fireEvent.click(screen.getByRole('button', { name: 'Stop' }));
  await waitFor(() => expect(defaults).toHaveBeenCalledTimes(2));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Record' })).toBeEnabled());
  fireEvent.click(screen.getByRole('button', { name: 'Record' }));
  await waitFor(() =>
    expect(write).toHaveBeenLastCalledWith(
      'graph-a',
      'recorder-1',
      expect.objectContaining({ path: '/Desktop/next.wav' }),
      1,
    ),
  );
});

it('does not replace an edited path with a late Desktop suggestion', async () => {
  const { port, write } = fixture();
  let resolve: (path: string) => void = () => undefined;
  const defaults = vi.fn().mockImplementation(
    () =>
      new Promise<string>((done) => {
        resolve = done;
      }),
  );
  view(Object.assign(port, { defaultRecordingPath: defaults }));
  await waitFor(() => expect(defaults).toHaveBeenCalledOnce());
  fireEvent.change(screen.getByRole('textbox'), { target: { value: '/chosen/manual.wav' } });
  resolve('/Desktop/late.wav');
  await waitFor(() => expect(screen.getByRole('textbox')).toHaveValue('/chosen/manual.wav'));
  fireEvent.click(screen.getByRole('button', { name: 'Record' }));
  await waitFor(() =>
    expect(write).toHaveBeenCalledWith(
      'graph-a',
      'recorder-1',
      expect.objectContaining({ path: '/chosen/manual.wav' }),
      1,
    ),
  );
});

it('shows Desktop resolution errors but permits a manually selected destination', async () => {
  const { port } = fixture();
  view(
    Object.assign(port, {
      defaultRecordingPath: vi.fn().mockRejectedValue(new Error('Desktop unavailable')),
    }),
  );
  expect(await screen.findByRole('alert')).toHaveTextContent('Desktop unavailable');
  expect(screen.getByRole('button', { name: 'Record' })).toBeDisabled();
  fireEvent.change(screen.getByRole('textbox'), { target: { value: '/chosen/manual.wav' } });
  await waitFor(() => expect(screen.getByRole('button', { name: 'Record' })).toBeEnabled());
  expect(screen.queryByRole('alert')).not.toBeInTheDocument();
});

it('can start a fresh Desktop take when reopening a failed recorder', async () => {
  const { port, write } = fixture();
  vi.spyOn(port, 'recorderStatus').mockResolvedValue({
    ...stopped,
    state: 'failed',
    path: '/Desktop/failed.wav',
    error: 'disk write failed',
  });
  render(
    recorderView(
      Object.assign(port, {
        defaultRecordingPath: vi.fn().mockResolvedValue('/Desktop/retry.wav'),
      }),
      true,
    ),
  );
  const record = screen.getByRole('button', { name: 'Record' });
  await waitFor(() => expect(record).toBeEnabled());
  fireEvent.click(record);
  await waitFor(() =>
    expect(write).toHaveBeenCalledWith(
      'graph-a',
      'recorder-1',
      expect.objectContaining({ path: '/Desktop/retry.wav' }),
      1,
    ),
  );
});

it('shows compact icon-only transport above closed Options and keeps the transport visible when folded', async () => {
  const { port } = fixture();
  render(recorderView(port, true));
  const record = screen.getByRole('button', { name: 'Record' });
  expect(record).toHaveTextContent('');
  expect(record.querySelector('svg')).not.toBeNull();
  const time = screen.getByLabelText('Recorded time');
  expect(time.compareDocumentPosition(record) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  const options = screen.getByRole('button', { name: 'Options' });
  expect(options).toHaveAttribute('aria-expanded', 'false');
  expect(record.compareDocumentPosition(options) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
  fireEvent.click(options);
  expect(screen.getByRole('textbox', { name: 'WAV file (absolute path)' })).toBeInTheDocument();
  fireEvent.click(options);
  expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
  expect(record).toBeInTheDocument();
  expect(screen.getByLabelText('Recorded time')).toHaveTextContent('00:00:00.000');
});

it('offers channels 1 through 8 from default 2 and locks channel edits while recording or paused', async () => {
  const { port } = fixture();
  const change = vi.fn();
  render(recorderView(port, true, change));
  fireEvent.click(screen.getByRole('button', { name: 'Options' }));
  const channels = screen.getByRole('combobox', { name: 'Channels' });
  await waitFor(() => expect(channels).toBeEnabled());
  expect(channels).toHaveValue('2');
  fireEvent.click(channels);
  for (let count = 1; count <= 8; count += 1)
    expect(await screen.findByRole('option', { name: String(count) })).toBeInTheDocument();
  expect(screen.queryByRole('option', { name: '9' })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('option', { name: '8' }));
  expect(change).toHaveBeenCalledExactlyOnceWith(8);
  fireEvent.change(screen.getByRole('textbox'), { target: { value: '/take.wav' } });
  fireEvent.click(screen.getByRole('button', { name: 'Record' }));
  await waitFor(() => expect(channels).toBeDisabled());
  expect(screen.getByRole('button', { name: 'Record' })).toHaveAttribute('data-active', 'true');
  fireEvent.click(screen.getByRole('button', { name: 'Pause' }));
  await screen.findByRole('button', { name: 'Resume' });
  expect(channels).toBeDisabled();
});

it('records WAV options, locks an active take, resumes without changing the file, then stops', async () => {
  const { port, write } = fixture();
  view(port);
  const path = screen.getByRole('textbox', { name: 'WAV file (absolute path)' });
  await waitFor(() => expect(path).toBeEnabled());
  fireEvent.change(path, { target: { value: '/takes/microphone.wav' } });
  fireEvent.click(screen.getByRole('combobox', { name: 'WAV sample rate' }));
  fireEvent.click(await screen.findByRole('option', { name: '96 kHz' }));
  fireEvent.click(screen.getByRole('combobox', { name: 'Bit depth' }));
  fireEvent.click(await screen.findByRole('option', { name: '24-bit PCM' }));
  fireEvent.click(screen.getByRole('button', { name: 'Record' }));
  await waitFor(() =>
    expect(write).toHaveBeenCalledWith(
      'graph-a',
      'recorder-1',
      {
        action: 'record',
        path: '/takes/microphone.wav',
        sampleRate: 96000,
        encoding: 'pcm24',
      },
      1,
    ),
  );
  expect(path).toBeDisabled();
  expect(screen.getByLabelText('Recorded time')).toHaveTextContent('00:00:01.000');
  fireEvent.click(screen.getByRole('button', { name: 'Pause' }));
  await screen.findByRole('button', { name: 'Resume' });
  expect(screen.getByRole('combobox', { name: 'Bit depth' })).toBeDisabled();
  fireEvent.click(screen.getByRole('button', { name: 'Resume' }));
  await waitFor(() =>
    expect(write).toHaveBeenLastCalledWith('graph-a', 'recorder-1', { action: 'record' }, 1),
  );
  fireEvent.click(screen.getByRole('button', { name: 'Stop' }));
  await waitFor(() => expect(path).toBeEnabled());
  expect(write).toHaveBeenLastCalledWith('graph-a', 'recorder-1', { action: 'stop' }, 1);
});

it('keeps rejected take settings editable and retries without graph mutation', async () => {
  const { port, write } = fixture();
  write.mockRejectedValueOnce(new Error('Destination already exists'));
  view(port);
  const path = screen.getByRole('textbox', { name: 'WAV file (absolute path)' });
  await waitFor(() => expect(path).toBeEnabled());
  fireEvent.change(path, { target: { value: 'relative.wav' } });
  expect(screen.getByRole('button', { name: 'Record' })).toBeDisabled();
  fireEvent.change(path, { target: { value: '/takes/new.wav' } });
  fireEvent.click(screen.getByRole('button', { name: 'Record' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Destination already exists');
  expect(path).toBeEnabled();
  fireEvent.change(path, { target: { value: '/takes/next.wav' } });
  fireEvent.click(screen.getByRole('button', { name: 'Record' }));
  await waitFor(() =>
    expect(write).toHaveBeenLastCalledWith(
      'graph-a',
      'recorder-1',
      {
        action: 'record',
        path: '/takes/next.wav',
        sampleRate: null,
        encoding: 'pcm16',
      },
      1,
    ),
  );
});

it('uses the native save picker and preserves the draft on cancel', async () => {
  const { port } = fixture();
  const picker = vi.fn().mockResolvedValueOnce('/takes/chosen.wav').mockResolvedValueOnce(null);
  const withPicker = Object.assign(port, { selectRecordingPath: picker });
  view(withPicker);
  const choose = screen.getByRole('button', { name: 'Choose file…' });
  await waitFor(() => expect(choose).toBeEnabled());
  fireEvent.click(choose);
  await waitFor(() => expect(screen.getByRole('textbox')).toHaveValue('/takes/chosen.wav'));
  expect(picker).toHaveBeenCalledWith({ defaultPath: undefined, title: 'Choose file…' });
  fireEvent.click(choose);
  await waitFor(() => expect(picker).toHaveBeenCalledTimes(2));
  expect(screen.getByRole('textbox')).toHaveValue('/takes/chosen.wav');
});

it('does not let a blocked graph start a take', async () => {
  const { port, write } = fixture();
  view(port, false);
  await screen.findByTitle('Stopped');
  expect(screen.getByRole('button', { name: 'Record' })).toBeDisabled();
  expect(write).not.toHaveBeenCalled();
});

it('keeps the existing take pausable, resumable and stoppable when graph edits become blocked', async () => {
  const { port, write } = fixture();
  const ui = view(port);
  const path = screen.getByRole('textbox');
  await waitFor(() => expect(path).toBeEnabled());
  fireEvent.change(path, { target: { value: '/takes/live.wav' } });
  fireEvent.click(screen.getByRole('button', { name: 'Record' }));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Pause' })).toBeEnabled());
  ui.rerender(recorderView(port, false));
  fireEvent.click(screen.getByRole('button', { name: 'Pause' }));
  const resume = await screen.findByRole('button', { name: 'Resume' });
  expect(resume).toBeEnabled();
  fireEvent.click(resume);
  await waitFor(() => expect(screen.getByRole('button', { name: 'Pause' })).toBeEnabled());
  fireEvent.click(screen.getByRole('button', { name: 'Stop' }));
  await waitFor(() =>
    expect(write).toHaveBeenLastCalledWith('graph-a', 'recorder-1', { action: 'stop' }, 1),
  );
  expect(screen.getByRole('button', { name: 'Record' })).toBeDisabled();
});
