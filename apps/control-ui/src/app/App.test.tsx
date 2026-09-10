import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { StorybookMockControlPort } from '../core/control-port/StorybookMockControlPort';
import type { ControlPort, MixRelayStatus, RuntimeGraphDocument } from '../core/control-port/types';
import { I18nProvider, i18n, resolveLocale } from '../core/i18n';
import { MixRelayProvider } from '../core/theme/MixRelayProvider';
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
    getProcessorEditorFrame: controlPort.getProcessorEditorFrame.bind(controlPort),
    sendProcessorEditorInput: controlPort.sendProcessorEditorInput.bind(controlPort),
    getProcessors: controlPort.getProcessors.bind(controlPort),
    getStatus,
    rescanProcessors: controlPort.rescanProcessors.bind(controlPort),
    setProcessorEditor: controlPort.setProcessorEditor.bind(controlPort),
    setProcessorParameter: controlPort.setProcessorParameter.bind(controlPort),
  };
}

describe('App composition', () => {
  const graph: RuntimeGraphDocument = { edges: [], nodes: [], revision: 1, schemaVersion: 2 };
  const status: MixRelayStatus = {
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
      <MixRelayProvider>
        <I18nProvider>
          <App controlPort={controlPort} />
        </I18nProvider>
      </MixRelayProvider>,
    );

    expect(screen.getByRole('heading', { name: 'MixRelay' })).toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'Graphs' })).not.toBeInTheDocument();
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
      <MixRelayProvider>
        <I18nProvider>
          <App controlPort={unavailablePort} />
        </I18nProvider>
      </MixRelayProvider>,
    );
    await act(async () => Promise.resolve());
    expect(screen.getByRole('button', { name: 'Audio status unknown' })).toBeDisabled();
    expect(screen.queryByText('Not installed')).not.toBeInTheDocument();
    failedView.unmount();

    let finishStatus!: (value: MixRelayStatus) => void;
    const pendingStatus = new Promise<MixRelayStatus>((resolve) => {
      finishStatus = resolve;
    });
    const pendingView = render(
      <MixRelayProvider>
        <I18nProvider>
          <App controlPort={controlPortWithStatus(controlPort, () => pendingStatus)} />
        </I18nProvider>
      </MixRelayProvider>,
    );
    pendingView.unmount();
    await act(async () => finishStatus({ ...status, driverState: 'ready' }));
  });
});

it('keeps file actions in the graph header and toggles navigation without changing the graph', async () => {
  await i18n.changeLanguage('en');
  const controlPort = new StorybookMockControlPort({
    graph: { edges: [], nodes: [], revision: 1, schemaVersion: 2 },
    graphStatus: { activeRevision: 1, protocolVersion: 1, submittedRevision: 1 },
    status: {
      bufferFrames: null,
      driverState: 'not_installed',
      overruns: 0,
      protocolVersion: 1,
      sampleRate: null,
      underruns: 0,
    },
  });
  const workspaceFile = vi.fn().mockResolvedValue(null);
  Object.assign(controlPort, { workspaceFile });
  const applyGraph = vi.spyOn(controlPort, 'applyGraph');
  render(
    <MixRelayProvider>
      <I18nProvider>
        <App controlPort={controlPort} />
      </I18nProvider>
    </MixRelayProvider>,
  );
  await waitFor(() => expect(screen.queryByText('Loading engine')).not.toBeInTheDocument());
  expect(screen.queryByText('Active')).not.toBeInTheDocument();
  const name = screen.getByRole('textbox', { name: 'Graph name' });
  const file = screen.getByRole('button', { name: 'File' });
  expect(file.closest('header')).toBe(name.closest('header'));
  const sidebar = screen.getByRole('complementary', { name: 'Graphs' });
  expect(within(sidebar).getByRole('button', { name: 'New graph' })).toBeInTheDocument();
  const toggle = screen.getByRole('button', { name: 'Graphs' });
  expect(toggle.closest('header')).toBeNull();
  expect(toggle).toHaveAttribute('aria-expanded', 'true');
  const callsBefore = applyGraph.mock.calls.length;
  fireEvent.click(toggle);
  expect(toggle).toHaveAttribute('aria-expanded', 'false');
  expect(sidebar).toHaveAttribute('hidden');
  expect(screen.getByRole('heading', { name: 'MixRelay' })).toBeVisible();
  expect(screen.getByRole('textbox', { name: 'Graph name' })).toBe(name);
  expect(file).toBeInTheDocument();
  fireEvent.click(toggle);
  expect(sidebar).not.toHaveAttribute('hidden');
  expect(applyGraph).toHaveBeenCalledTimes(callsBefore);
  expect(workspaceFile).not.toHaveBeenCalled();
  fireEvent.click(file);
  // jsdom has no anchor geometry, so Floating UI may hide the mounted menu.
  const menu = await screen.findByRole('menu', { hidden: true });
  expect(within(menu).getByText('Open…')).toBeInTheDocument();
  expect(within(menu).getByText('Save')).toBeInTheDocument();
});
