import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { AudioGraphModel } from '../../core/audio-graph/model';
import { TauriControlPort } from '../../core/control-port/TauriControlPort';
import type { ProcessorEditorStatus } from '../../core/control-port/types';
import { i18n } from '../../core/i18n';
import { MixRelayProvider } from '../../core/theme/MixRelayProvider';
import { ProcessorCatalogAlert, ProcessorEditorAlerts } from './ProcessorControlAlerts';
import { useProcessorCatalog } from './useProcessorCatalog';
import { useProcessorEditors } from './useProcessorEditors';

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  window.localStorage.clear();
});

describe('processor native scan diagnostics', () => {
  it.each(['en', 'ko-KR', 'pseudo'])(
    'shows partial native discovery in %s and retries the scanner, not the cached catalog',
    async (locale) => {
      await i18n.changeLanguage(locale);
      const invoke = vi
        .fn()
        .mockResolvedValueOnce({
          status: 200,
          body: {
            protocolVersion: 1,
            processors: [],
            diagnostics: [
              {
                format: 'vst3',
                stage: 'probe',
                pluginId: 'fixture',
                message: '<script>denied</script>',
              },
            ],
          },
        })
        .mockResolvedValue({ status: 200, body: { protocolVersion: 1, processors: [] } });
      const source = new TauriControlPort(invoke);
      function Panel() {
        return <ProcessorCatalogAlert catalog={useProcessorCatalog(source)} />;
      }
      const { container } = render(
        <I18nextProvider i18n={i18n}>
          <MixRelayProvider>
            <Panel />
          </MixRelayProvider>
        </I18nextProvider>,
      );
      expect(await screen.findByRole('alert')).toHaveTextContent(
        i18n.t('routing.processorControl.partial'),
      );
      expect(container.querySelector('details')).not.toHaveAttribute('open');
      expect(container.querySelector('li')).toHaveTextContent('VST3');
      expect(container.querySelector('li')).toHaveTextContent('<script>denied</script>');
      expect(container.querySelector('script')).toBeNull();
      fireEvent.click(
        screen.getByRole('button', { name: i18n.t('routing.processorControl.retry') }),
      );
      await waitFor(() => expect(screen.queryByRole('alert')).not.toBeInTheDocument());
      expect(invoke).toHaveBeenLastCalledWith('control_rescan_processors', { paths: [] });
    },
  );
});

describe('processor control recovery alerts', () => {
  it.each(['en', 'ko-KR', 'pseudo'])(
    'renders a localized scan error and working retry in %s',
    async (locale) => {
      await i18n.changeLanguage(locale);
      const invoke = vi
        .fn()
        .mockResolvedValueOnce({
          status: 503,
          body: { code: 'control_unavailable', parameters: {} },
        })
        .mockResolvedValue({ status: 200, body: { protocolVersion: 1, processors: [] } });
      const source = new TauriControlPort(invoke);
      function Panel() {
        return <ProcessorCatalogAlert catalog={useProcessorCatalog(source)} />;
      }
      render(
        <I18nextProvider i18n={i18n}>
          <MixRelayProvider>
            <Panel />
          </MixRelayProvider>
        </I18nextProvider>,
      );
      expect(await screen.findByRole('alert')).toHaveTextContent(
        i18n.t('routing.processorControl.catalog'),
      );
      fireEvent.click(
        screen.getByRole('button', { name: i18n.t('routing.processorControl.retry') }),
      );
      await waitFor(() => expect(screen.queryByRole('alert')).not.toBeInTheDocument());
      expect(invoke).toHaveBeenCalledTimes(2);
      expect(invoke).toHaveBeenLastCalledWith('control_get_processors', undefined);
    },
  );

  it('shows a rejected automatic open and keeps retry explicit', async () => {
    await i18n.changeLanguage('en');
    const open: ProcessorEditorStatus = {
      protocolVersion: 1,
      nodeId: 'effect',
      state: 'open',
      failure: null,
      height: null,
      width: null,
      resizable: false,
    };
    const pending = Promise.withResolvers<ProcessorEditorStatus>();
    const source = {
      getProcessorEditor: vi.fn().mockResolvedValue({ ...open, state: 'closed' }),
      setProcessorEditor: vi
        .fn()
        .mockRejectedValueOnce(new Error('rejected'))
        .mockReturnValue(pending.promise),
    };
    const graph: AudioGraphModel = {
      revision: 1,
      edges: [],
      nodes: [
        {
          id: 'effect',
          inputPorts: [],
          outputPorts: [],
          position: { x: 0, y: 0 },
          appearance: {
            title: 'My effect',
            category: 'Processor',
            icon: 'effect',
            hasProcessorEditor: true,
          },
          runtime: { type: 'provider', config: { typeId: 'au:fixture' } },
        },
      ],
    };
    function Panel() {
      const editors = useProcessorEditors(source, graph);
      return (
        <>
          <output data-testid="native-state">{editors.statuses.get('effect')?.state}</output>
          <ProcessorEditorAlerts editors={editors} />
        </>
      );
    }
    render(
      <I18nextProvider i18n={i18n}>
        <MixRelayProvider>
          <Panel />
        </MixRelayProvider>
      </I18nextProvider>,
    );
    expect(await screen.findByRole('alert')).toHaveTextContent('My effect');
    expect(screen.getByTestId('native-state')).toHaveTextContent('closed');
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(screen.getByRole('button', { name: 'Retry' })).toBeDisabled();
    await act(async () => pending.resolve(open));
    expect(source.setProcessorEditor.mock.calls).toEqual([
      ['effect', true, 1],
      ['effect', true, 1],
    ]);
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    expect(screen.getByTestId('native-state')).toHaveTextContent('open');
  });
});
