import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { StorybookMockControlPort } from '../core/control-port/StorybookMockControlPort';
import type { ControlPort, RuntimeGraphDocument, ToneNariStatus } from '../core/control-port/types';
import { I18nProvider, i18n, resolveLocale } from '../core/i18n';
import { ToneNariProvider } from '../core/theme/ToneNariProvider';
import { App } from './App';

function controlPortWithStatus(
  controlPort: ControlPort,
  getStatus: ControlPort['getStatus'],
): ControlPort {
  return {
    recorderStatus: controlPort.recorderStatus.bind(controlPort),
    controlRecorder: controlPort.controlRecorder.bind(controlPort),
    applyGraph: controlPort.applyGraph.bind(controlPort),
    getGraph: controlPort.getGraph.bind(controlPort),
    getGraphStatus: controlPort.getGraphStatus.bind(controlPort),
    getMeters: controlPort.getMeters.bind(controlPort),
    getProcessorEditor: controlPort.getProcessorEditor.bind(controlPort),
    getProcessors: controlPort.getProcessors.bind(controlPort),
    getStatus,
    rescanProcessors: controlPort.rescanProcessors.bind(controlPort),
    setProcessorEditor: controlPort.setProcessorEditor.bind(controlPort),
    setProcessorParameter: controlPort.setProcessorParameter.bind(controlPort),
  };
}

describe('App composition', () => {
  const graph: RuntimeGraphDocument = { edges: [], nodes: [], revision: 1, schemaVersion: 2 };
  const status: ToneNariStatus = {
    bufferFrames: null,
    driverState: 'not_installed',
    overruns: 0,
    protocolVersion: 1,
    sampleRate: null,
    underruns: 0,
  };
  let controlPort: StorybookMockControlPort;
  beforeEach(() => {
    controlPort = new StorybookMockControlPort({
      graph,
      graphStatus: { activeRevision: 1, protocolVersion: 1, submittedRevision: 1 },
      status,
    });
  });

  it('renders the Core to Feature to Screen composition', async () => {
    await i18n.changeLanguage('en');
    render(
      <ToneNariProvider>
        <I18nProvider>
          <App controlPort={controlPort} />
        </I18nProvider>
      </ToneNariProvider>,
    );

    expect(screen.getByRole('heading', { name: 'Graphs' })).toBeInTheDocument();
    expect(await screen.findByRole('button', { name: /Untitled Graph/u })).toHaveAttribute(
      'aria-current',
      'page',
    );
    const newGraph = screen.getByRole('button', { name: 'New graph' });
    await waitFor(() => expect(newGraph).toBeEnabled());
    fireEvent.click(newGraph);
    const dialog = screen.getByRole('dialog', { name: 'New graph' });
    expect(dialog).toBeInTheDocument();
    fireEvent.change(within(dialog).getByRole('textbox', { name: 'Graph name' }), {
      target: { value: 'Broadcast' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Create' }));
    expect(await screen.findByRole('button', { name: /Broadcast/u })).toHaveAttribute(
      'aria-current',
      'page',
    );
    expect(screen.getByRole('textbox', { name: 'Graph name' })).toHaveValue('Broadcast');
    expect(screen.getByRole('region', { name: 'Routing workspace' })).toBeInTheDocument();
  });

  it('resolves supported locales and returns mock control status', async () => {
    expect(resolveLocale('ko-KR')).toBe('ko-KR');
    expect(resolveLocale('en-US')).toBe('en');

    await expect(controlPort.getStatus()).resolves.toBe(status);
    await expect(controlPort.getGraph()).resolves.toBe(graph);
    await expect(controlPort.getGraphStatus()).resolves.toMatchObject({ activeRevision: 1 });
    await expect(controlPort.applyGraph({ ...graph, revision: 2 })).resolves.toMatchObject({
      revision: 2,
      state: 'active',
    });
  });

  it('shows unknown health rather than inventing installation state when status is unavailable', async () => {
    const unavailablePort = controlPortWithStatus(
      controlPort,
      vi.fn().mockRejectedValue(new Error('offline')),
    );
    const failedView = render(
      <ToneNariProvider>
        <I18nProvider>
          <App controlPort={unavailablePort} />
        </I18nProvider>
      </ToneNariProvider>,
    );
    await act(async () => Promise.resolve());
    expect(screen.getByRole('img', { name: 'Audio status unknown' })).toHaveAttribute(
      'data-tone',
      'warning',
    );
    expect(screen.queryByText('Not installed')).not.toBeInTheDocument();
    failedView.unmount();

    let finishStatus!: (value: ToneNariStatus) => void;
    const pendingStatus = new Promise<ToneNariStatus>((resolve) => {
      finishStatus = resolve;
    });
    const pendingView = render(
      <ToneNariProvider>
        <I18nProvider>
          <App controlPort={controlPortWithStatus(controlPort, () => pendingStatus)} />
        </I18nProvider>
      </ToneNariProvider>,
    );
    pendingView.unmount();
    await act(async () => finishStatus({ ...status, driverState: 'ready' }));
  });
});
