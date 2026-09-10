import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { expect, it, vi } from 'vitest';
import { StorybookMockControlPort } from '../../core/control-port/StorybookMockControlPort';
import { i18n } from '../../core/i18n';
import { EnginePowerControl } from './EnginePowerControl';

function fixture() {
  return new StorybookMockControlPort({
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
}

it('switches through control IPC and waits for native acknowledgement without changing the graph', async () => {
  await i18n.changeLanguage('en');
  const port = fixture();
  const status = {
    ...(await port.getStatus()),
    enginePower: { enabled: true, transitioning: false },
  };
  const graph = await port.getGraph();
  const switchPower = vi.fn().mockResolvedValue({ revision: graph.revision });
  const controlPort = Object.assign(port, { setEnginePower: switchPower });
  vi.spyOn(port, 'getStatus').mockResolvedValue({
    ...status,
    enginePower: { enabled: false, transitioning: false },
  });
  const onApplied = vi.fn();
  const onBusyChange = vi.fn();
  render(
    <I18nextProvider i18n={i18n}>
      <EnginePowerControl
        controlPort={controlPort}
        status={status}
        onApplied={onApplied}
        onBusyChange={onBusyChange}
      />
    </I18nextProvider>,
  );
  fireEvent.click(screen.getByRole('button', { name: 'Turn engine off' }));
  expect(screen.getByRole('button', { name: 'Switching engine…' })).toBeDisabled();
  await waitFor(() => expect(onApplied).toHaveBeenCalledWith(graph));
  expect(switchPower).toHaveBeenCalledExactlyOnceWith(false);
  expect(onBusyChange.mock.calls).toEqual([[true], [false]]);
  expect(screen.getByRole('button', { name: 'Turn engine on' })).toHaveAttribute(
    'aria-pressed',
    'false',
  );
});

it('shows failure without inventing an off state', async () => {
  await i18n.changeLanguage('en');
  const port = fixture();
  const status = {
    ...(await port.getStatus()),
    enginePower: { enabled: true, transitioning: false },
  };
  const controlPort = Object.assign(port, {
    setEnginePower: vi.fn().mockRejectedValue(new Error('unavailable')),
  });
  render(
    <I18nextProvider i18n={i18n}>
      <EnginePowerControl
        controlPort={controlPort}
        status={status}
        onApplied={vi.fn()}
        onBusyChange={vi.fn()}
      />
    </I18nextProvider>,
  );
  fireEvent.click(screen.getByRole('button', { name: 'Turn engine off' }));
  await waitFor(() => expect(screen.getByRole('alert')).toBeVisible());
  expect(screen.getByRole('button', { name: 'Turn engine off' })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
});
