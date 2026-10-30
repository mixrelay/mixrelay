import '../test-support/integration';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import { StorybookMockControlPort } from '../core/control-port/StorybookMockControlPort';
import type { RuntimeGraphDocument } from '../core/control-port/types';
import { I18nProvider, i18n } from '../core/i18n';
import { MixRelayProvider } from '../core/theme/MixRelayProvider';
import { midiFixture, midiPortFixture } from '../features/midi/midiFixtures';
import { advance, eventually, find } from '../test-support/fakeTimerQueries';
import { App } from './App';

const defaultMatchMedia = vi.mocked(window.matchMedia).getMockImplementation();

afterEach(async () => {
  await advance();
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  if (defaultMatchMedia) vi.mocked(window.matchMedia).mockImplementation(defaultMatchMedia);
});
async function input(action: Promise<void>) {
  let done = false;
  await Promise.all([
    action.finally(() => {
      done = true;
    }),
    eventually(() => expect(done).toBe(true)),
  ]);
}
it.each([false, true])(
  'maps actual node controls, replaces the sidebar and restores it (narrow: %s)',
  async (narrow) => {
    vi.useFakeTimers();
    installGeometry();
    if (narrow) {
      const media = window.matchMedia('(max-width: 720px)');
      vi.spyOn(window, 'matchMedia').mockImplementation((query) => ({
        ...media,
        media: query,
        matches: query === '(max-width: 720px)',
      }));
    }
    await i18n.changeLanguage('en');
    const graph: RuntimeGraphDocument = {
      revision: 1,
      schemaVersion: 2,
      edges: [],
      nodes: [
        { id: 'gain', type: 'gain', label: 'Monitor trim', config: { channels: 2, gainDb: 0 } },
      ],
    };
    const port = new StorybookMockControlPort({
      graph,
      graphStatus: { activeRevision: 1, submittedRevision: 1, protocolVersion: 1 },
      status: {
        protocolVersion: 1,
        driverState: 'ready',
        bufferFrames: 128,
        sampleRate: 48000,
        overruns: 0,
        underruns: 0,
      },
    });
    const apply = vi.spyOn(port, 'applyGraph');
    const midi = midiPortFixture({ ...midiFixture, bindings: [] });
    const command = vi.spyOn(midi, 'command');
    render(
      <MixRelayProvider>
        <I18nProvider>
          <App
            controlPort={port}
            midiPort={{ command }}
            licensingPort={null}
            driverPort={null}
            runtimePort={null}
            updatesPort={null}
          />
        </I18nProvider>
      </MixRelayProvider>,
    );
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    await find('group', { name: 'Monitor trim' });
    expect(screen.queryByRole('button', { name: 'MIDI controls' })).not.toBeInTheDocument();
    if (narrow) {
      expect(screen.queryByRole('complementary', { name: 'Graphs' })).not.toBeInTheDocument();
      await input(user.click(await find('button', { name: 'Graphs' })));
    }
    const graphs = await find('complementary', { name: 'Graphs' });
    const graphButton = within(graphs).getByRole('button', {
      name: 'Untitled Graph: Delete graph',
    });
    await eventually(() => expect(graphButton).toBeEnabled());
    graphButton.focus();
    expect(graphButton).toHaveFocus();
    await input(user.keyboard('{Control>}m{/Control}'));
    expect(screen.queryByRole('complementary', { name: 'Graphs' })).not.toBeInTheDocument();
    expect(await find('complementary', { name: 'MIDI controls' })).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Graphs' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'MIDI map mode' })).toHaveFocus();
    expectInactiveChrome();
    const target = await find('button', { name: 'Map Monitor trim: Volume' });
    target.focus();
    await input(user.keyboard('{Enter}'));
    expect(command).toHaveBeenCalledWith('midi_learn', {
      assignment: {
        target: { graphId: 'graph-1', nodeId: 'gain' },
        action: 'volume',
        graphName: 'Untitled Graph',
        nodeName: 'Monitor trim',
      },
    });
    await input(user.click(screen.getByText('Monitor trim')));
    await input(user.click(await find('button', { name: 'Map Monitor trim: Power' })));
    await input(user.keyboard('{Control>}z{/Control}{Delete}{Escape}'));
    await eventually(() =>
      expect(screen.getByRole('button', { name: 'MIDI map mode' })).toHaveFocus(),
    );
    expect(command).toHaveBeenCalledWith('midi_cancel', undefined);
    expect(screen.queryByRole('complementary', { name: 'MIDI controls' })).not.toBeInTheDocument();
    expect(await find('complementary', { name: 'Graphs' })).toBeVisible();
    expect(apply).not.toHaveBeenCalled();
    await expectMapEntryMovesFocus(user);
  },
);

function installGeometry() {
  vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockReturnValue(1024);
  vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockReturnValue(768);
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockReturnValue(
    new DOMRect(0, 0, 1024, 768),
  );
  class MeasuredObserver implements ResizeObserver {
    constructor(private callback: ResizeObserverCallback) {}
    observe(target: Element) {
      const size = [{ inlineSize: 1024, blockSize: 768 }];
      this.callback(
        [
          {
            target,
            contentRect: new DOMRect(0, 0, 1024, 768),
            borderBoxSize: size,
            contentBoxSize: size,
            devicePixelContentBoxSize: size,
          },
        ],
        this,
      );
    }
    disconnect() {}
    unobserve() {}
  }
  vi.stubGlobal('ResizeObserver', MeasuredObserver);
}

it('undoes graph deletion from the graph list with the original target IDs, then redoes and restores again', async () => {
  vi.useFakeTimers();
  installGeometry();
  await i18n.changeLanguage('en');
  const port = new StorybookMockControlPort({
    graph: {
      revision: 1,
      schemaVersion: 2,
      edges: [],
      nodes: [
        { id: 'gain', type: 'gain', label: 'Monitor trim', config: { channels: 2, gainDb: 0 } },
      ],
    },
    graphStatus: { activeRevision: 1, submittedRevision: 1, protocolVersion: 1 },
    status: {
      protocolVersion: 1,
      driverState: 'ready',
      bufferFrames: 128,
      sampleRate: 48000,
      overruns: 0,
      underruns: 0,
    },
  });
  const mutate = vi.spyOn(port, 'mutateGraphCatalog');
  render(
    <MixRelayProvider>
      <I18nProvider>
        <App
          controlPort={port}
          midiPort={midiPortFixture()}
          licensingPort={null}
          driverPort={null}
          runtimePort={null}
          updatesPort={null}
        />
      </I18nProvider>
    </MixRelayProvider>,
  );
  const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
  await find('group', { name: 'Monitor trim' });
  const deleteGraph = await find('button', { name: 'Untitled Graph: Delete graph' });
  await eventually(() => expect(deleteGraph).toBeEnabled());
  await input(user.click(deleteGraph));
  await input(user.click(await find('button', { name: 'Delete graph' })));
  await eventually(() =>
    expect(screen.queryByRole('group', { name: 'Monitor trim' })).not.toBeInTheDocument(),
  );
  const sidebar = await find('complementary', { name: 'Graphs' });
  const newGraph = within(sidebar).getByRole('button', { name: 'Untitled Graph: Delete graph' });
  await eventually(() => expect(newGraph).toBeEnabled());
  newGraph.focus();
  await input(user.keyboard('{Control>}z{/Control}'));
  await find('group', { name: 'Monitor trim' });
  expect((await port.getGraphCatalog()).activeId).toBe('graph-1');
  expect((await port.getGraph()).nodes[0]?.id).toBe('gain');
  const remaining = await find('button', { name: 'Untitled Graph: Delete graph' });
  await eventually(() => expect(remaining).toBeEnabled());
  remaining.focus();
  await input(user.keyboard('{Control>}{Shift>}z{/Shift}{/Control}'));
  await eventually(() =>
    expect(screen.queryByRole('group', { name: 'Monitor trim' })).not.toBeInTheDocument(),
  );
  const owner = within(sidebar).getByRole('button', { name: 'Untitled Graph: Delete graph' });
  await eventually(() => expect(owner).toBeEnabled());
  owner.focus();
  await input(user.keyboard('{Control>}z{/Control}'));
  await find('group', { name: 'Monitor trim' });
  expect((await port.getGraphCatalog()).activeId).toBe('graph-1');
  expect(mutate).toHaveBeenCalledWith(expect.objectContaining({ action: 'undo_delete' }));
  expect(mutate).toHaveBeenCalledWith(expect.objectContaining({ action: 'redo_delete' }));
});

function expectInactiveChrome() {
  const toggle = screen.getByRole('button', { name: 'MIDI map mode' });
  const theme = screen.getByRole('button', { name: /Appearance:/ });
  expect(theme.closest('[inert]')).not.toBeNull();
  expect(toggle.closest('header')).not.toBe(theme.closest('header'));
  expect(toggle.closest('header')?.querySelector('input')).toBe(
    screen.getByRole('textbox', { name: i18n.t('studio.graphName') }),
  );
  expect(screen.getByRole('textbox', { name: i18n.t('studio.graphName') })).toBeDisabled();
  expect(screen.getByRole('button', { name: 'Diagnostics' }).closest('[inert]')).not.toBeNull();
}

async function expectMapEntryMovesFocus(user: ReturnType<typeof userEvent.setup>) {
  for (const control of [
    () => screen.getByRole('button', { name: /Appearance:/ }),
    () => screen.getByRole('textbox', { name: i18n.t('studio.graphName') }),
    () => screen.getByRole('button', { name: 'Diagnostics' }),
  ]) {
    control().focus();
    await input(user.keyboard('{Control>}m{/Control}'));
    await eventually(() =>
      expect(screen.getByRole('button', { name: 'MIDI map mode' })).toHaveFocus(),
    );
    await input(user.keyboard('{Escape}'));
  }
}
