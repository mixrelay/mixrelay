import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, expect, it, vi } from 'vitest';
import { i18n } from '../../core/i18n';
import {
  EditorPointerQueue,
  editorImagePoint,
  NativeProcessorSurface,
} from './NativeProcessorSurface';

const frame = { instanceId: '7', session: 1, width: 500, height: 200, pngBase64: 'AAAA' };

it.each([
  { width: 250, height: 250, clientX: 135, clientY: 145 },
  { width: 1000, height: 200, clientX: 510, clientY: 120 },
])('maps centered contained image coordinates in $width x $height bounds', (sample) => {
  expect(editorImagePoint(frame, { left: 10, top: 20, ...sample }, sample, false)).toEqual({
    x: 250,
    y: 100,
  });
});

it('rejects letterbox clicks but clamps captured drag and release positions', () => {
  const bounds = { left: 10, top: 20, width: 250, height: 250 };
  const pointer = { clientX: 135, clientY: 30 };
  expect(editorImagePoint(frame, bounds, pointer, false)).toBeNull();
  expect(editorImagePoint(frame, bounds, pointer, true)).toEqual({ x: 250, y: 0 });
  expect(editorImagePoint(frame, bounds, { clientX: 300, clientY: 400 }, true)).toEqual({
    x: 500,
    y: 200,
  });
  expect(editorImagePoint(frame, { ...bounds, width: 0 }, pointer, true)).toBeNull();
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it('coalesces motion while preserving ordered down and up', async () => {
  const first = Promise.withResolvers<void>();
  const port = {
    getProcessorEditorFrame: vi.fn(),
    sendProcessorEditorInput: vi
      .fn()
      .mockReturnValueOnce(first.promise)
      .mockResolvedValue(undefined),
  };
  const queue = new EditorPointerQueue(port, 'effect', vi.fn());
  queue.image(frame);
  queue.push('down', 1, 2);
  for (let x = 0; x < 200; x++) queue.push('move', x, 10);
  queue.push('up', 199, 10);
  expect(port.sendProcessorEditorInput).toHaveBeenCalledTimes(1);
  first.resolve();
  await waitFor(() => expect(port.sendProcessorEditorInput).toHaveBeenCalledTimes(3));
  expect(port.sendProcessorEditorInput.mock.calls.map((call) => call[1].kind)).toEqual([
    'down',
    'move',
    'up',
  ]);
  expect(port.sendProcessorEditorInput.mock.calls[1]?.[1].x).toBe(199);
});

it('sends cancellation after an in-flight down when the node is removed', async () => {
  const first = Promise.withResolvers<void>();
  const port = {
    getProcessorEditorFrame: vi.fn(),
    sendProcessorEditorInput: vi
      .fn()
      .mockReturnValueOnce(first.promise)
      .mockResolvedValue(undefined),
  };
  const queue = new EditorPointerQueue(port, 'effect', vi.fn());
  queue.image(frame);
  queue.push('down');
  queue.dispose();
  first.resolve();
  await waitFor(() => expect(port.sendProcessorEditorInput).toHaveBeenCalledTimes(2));
  expect(port.sendProcessorEditorInput.mock.calls[1]?.[1]).toMatchObject({
    kind: 'cancel',
    instanceId: '7',
    session: 1,
  });
});

it.each([100, 250])('maps clicks and preserves completed capture at height %s', async (height) => {
  vi.stubGlobal('PointerEvent', MouseEvent);
  const port = {
    getProcessorEditorFrame: vi.fn().mockResolvedValue(frame),
    sendProcessorEditorInput: vi.fn().mockResolvedValue(undefined),
  };
  render(
    <I18nextProvider i18n={i18n}>
      <NativeProcessorSurface port={port} nodeId="effect" title="AUBandpass" />
    </I18nextProvider>,
  );
  await screen.findByRole('img', { name: 'AUBandpass' });
  const surface = screen.getByRole('application');
  surface.setPointerCapture = vi.fn();
  surface.hasPointerCapture = vi.fn().mockReturnValue(true);
  surface.releasePointerCapture = () => {
    fireEvent.lostPointerCapture(surface);
  };
  vi.spyOn(surface, 'getBoundingClientRect').mockReturnValue({
    x: 0,
    y: 0,
    left: 0,
    top: 0,
    width: 250,
    height,
    right: 250,
    bottom: height,
    toJSON: () => ({}),
  });
  if (height === 250) {
    fireEvent.pointerDown(surface, { button: 0, clientX: 125, clientY: 20 });
    expect(port.sendProcessorEditorInput).not.toHaveBeenCalled();
    expect(surface.setPointerCapture).not.toHaveBeenCalled();
  }
  await act(async () => {
    fireEvent.pointerDown(surface, { button: 0, clientX: 125, clientY: height / 2 });
    fireEvent.pointerUp(surface, { button: 0, clientX: 125, clientY: height / 2 });
  });
  expect(port.sendProcessorEditorInput.mock.calls.map((call) => call[1].kind)).toEqual([
    'down',
    'up',
  ]);
  expect(port.sendProcessorEditorInput.mock.calls[0]?.[1]).toMatchObject({ x: 250, y: 100 });
});
