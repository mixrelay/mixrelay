import { afterEach, describe, expect, it, vi } from 'vitest';
import { StorybookMockControlPort } from '../../core/control-port/StorybookMockControlPort';
import type { RecorderStatus } from '../../core/control-port/types';
import { absoluteRecordingPath, RecorderController, recordingDuration } from './RecorderController';

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
  const read = vi.spyOn(port, 'recorderStatus').mockResolvedValue(stopped);
  const write = vi
    .spyOn(port, 'controlRecorder')
    .mockResolvedValue({ ...stopped, state: 'recording' });
  const controller = new RecorderController(port, 'graph-a', 'recorder-1');
  return { controller, read, write };
}
const flush = async () => {
  for (let i = 0; i < 8; i += 1) await Promise.resolve();
};
afterEach(() => vi.useRealTimers());

describe('RecorderController ownership', () => {
  it('keeps a restarted owner isolated from the preceding in-flight response', async () => {
    vi.useFakeTimers();
    const { controller, read } = fixture();
    let resolve: (status: RecorderStatus) => void = () => undefined;
    read.mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    controller.start();
    await flush();
    controller.dispose();
    controller.start();
    resolve({ ...stopped, state: 'recording', frames: 100 });
    await flush();
    expect(read).toHaveBeenCalledTimes(2);
    expect(controller.getSnapshot().status).toEqual(stopped);
    controller.dispose();
  });
});

describe('RecorderController requests', () => {
  it('serializes status and commands, rejecting duplicate button presses', async () => {
    vi.useFakeTimers();
    const { controller, read, write } = fixture();
    let resolve: (status: RecorderStatus) => void = () => undefined;
    read.mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    controller.start();
    await flush();
    controller.command({
      action: 'record',
      path: '/takes/one.wav',
      sampleRate: null,
      encoding: 'float32',
    });
    controller.command({ action: 'record', path: '/takes/two.wav' });
    expect(write).not.toHaveBeenCalled();
    resolve(stopped);
    await flush();
    expect(write).toHaveBeenCalledExactlyOnceWith('graph-a', 'recorder-1', {
      action: 'record',
      path: '/takes/one.wav',
      sampleRate: null,
      encoding: 'float32',
    });
    expect(controller.getSnapshot().status?.state).toBe('recording');
    controller.dispose();
  });

  it('preserves the last status after a rejected command and allows retry', async () => {
    vi.useFakeTimers();
    const { controller, write } = fixture();
    controller.start();
    await flush();
    write.mockRejectedValueOnce(new Error('Disk full'));
    controller.command({ action: 'record', path: '/take.wav' });
    await flush();
    expect(controller.getSnapshot()).toMatchObject({
      status: stopped,
      busy: false,
      error: 'Disk full',
    });
    await vi.advanceTimersByTimeAsync(1000);
    expect(controller.getSnapshot().error).toBe('Disk full');
    controller.command({ action: 'record', path: '/other/take.wav' });
    await flush();
    expect(controller.getSnapshot().error).toBeNull();
    controller.dispose();
  });

  it('ignores late completion after disposal and never continues polling', async () => {
    vi.useFakeTimers();
    const { controller, read } = fixture();
    let resolve: (status: RecorderStatus) => void = () => undefined;
    read.mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    controller.start();
    await flush();
    controller.dispose();
    resolve(stopped);
    await flush();
    await vi.advanceTimersByTimeAsync(5000);
    expect(controller.getSnapshot().status).toBeNull();
    expect(read).toHaveBeenCalledTimes(1);
  });

  it('does not overlap slow polling reads', async () => {
    vi.useFakeTimers();
    const { controller, read } = fixture();
    read.mockImplementation(() => new Promise(() => undefined));
    controller.start();
    await flush();
    await vi.advanceTimersByTimeAsync(5000);
    expect(read).toHaveBeenCalledTimes(1);
    controller.dispose();
  });
});

it('derives duration from saved frames only', () => {
  expect(recordingDuration({ ...stopped, frames: 48000 * 3661, sampleRate: 48000 })).toBe(
    '01:01:01.000',
  );
  expect(recordingDuration(null)).toBe('00:00:00.000');
  expect(recordingDuration({ ...stopped, frames: 59952, sampleRate: 48000 })).toBe('00:00:01.249');
  expect(recordingDuration({ ...stopped, frames: 4410, sampleRate: 44100 })).toBe('00:00:00.100');
});

it('polls active recording every 100ms without extrapolating frames, then slows while paused', async () => {
  vi.useFakeTimers();
  const { controller, read, write } = fixture();
  const recording: RecorderStatus = {
    ...stopped,
    state: 'recording',
    frames: 59952,
    sampleRate: 48000,
  };
  read.mockResolvedValue(recording);
  controller.start();
  await flush();
  await vi.advanceTimersByTimeAsync(99);
  expect(read).toHaveBeenCalledTimes(1);
  expect(recordingDuration(controller.getSnapshot().status)).toBe('00:00:01.249');
  await vi.advanceTimersByTimeAsync(1);
  expect(read).toHaveBeenCalledTimes(2);
  const paused: RecorderStatus = { ...recording, state: 'paused' };
  write.mockResolvedValue(paused);
  read.mockResolvedValue(paused);
  controller.command({ action: 'pause' });
  await flush();
  await vi.advanceTimersByTimeAsync(999);
  expect(read).toHaveBeenCalledTimes(2);
  expect(recordingDuration(controller.getSnapshot().status)).toBe('00:00:01.249');
  await vi.advanceTimersByTimeAsync(1);
  expect(read).toHaveBeenCalledTimes(3);
  controller.dispose();
});

it.each(['/takes/a.wav', 'C:\\takes\\a.wav', '\\\\server\\share\\a.wav'])(
  'accepts absolute platform path %s',
  (path) => expect(absoluteRecordingPath(path)).toBe(true),
);
it.each(['', 'take.wav', '~/take.wav', 'C:take.wav'])('rejects relative path %s', (path) =>
  expect(absoluteRecordingPath(path)).toBe(false),
);
