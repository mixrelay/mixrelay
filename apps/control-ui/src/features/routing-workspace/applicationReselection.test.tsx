import { fireEvent, render, screen } from '@testing-library/react';
import { I18nextProvider } from 'react-i18next';
import { expect, it, vi } from 'vitest';
import type { AudioGraphModel } from '../../core/audio-graph/model';
import { StorybookMockControlPort } from '../../core/control-port/StorybookMockControlPort';
import type { RuntimeGraphDocument } from '../../core/control-port/types';
import { i18n } from '../../core/i18n';
import { ToneNariProvider } from '../../core/theme/ToneNariProvider';
import { AudioNodeCard } from '../../core/ui/AudioNodeCard';
import {
  addRoutingNode,
  audioGraphToRuntimeGraph,
  runtimeGraphToAudioGraph,
  updateRoutingNodeGain,
} from './model';
import { RoutingWorkspace } from './RoutingWorkspace';
import { presentApplicationReselection } from './virtualNodeStatus';

vi.mock('../../core/audio-graph/AudioGraphCanvas', () => ({
  AudioGraphCanvas: ({ model }: { model: AudioGraphModel }) => (
    <output data-testid="status">
      {model.nodes.map((node) => node.appearance.status).join(', ')}
    </output>
  ),
}));

const document: RuntimeGraphDocument = {
  schemaVersion: 2,
  revision: 1,
  edges: [],
  nodes: [
    {
      id: 'legacy',
      label: 'Old application',
      type: 'application_capture',
      config: {
        requiresReselection: true,
        selector: { match: 'process_id', process_id: 123, include_descendants: true },
        gainDb: 0,
        sourcePlayback: 'preserve',
        stream: { channels: 2, channelLayout: 'stereo' },
      },
    },
  ],
};

it.each(['en', 'ko-KR', 'pseudo'])(
  'shows a minimal actionable restored-PID tooltip in %s',
  async (locale) => {
    const raw = runtimeGraphToAudioGraph(document);
    const before = JSON.stringify(raw);
    const t = i18n.getFixedT(locale);
    const node = presentApplicationReselection(raw, t).nodes[0];
    if (!node) throw new Error('missing application');
    const view = render(
      <ToneNariProvider>
        <AudioNodeCard {...node.appearance} inputCount={0} outputCount={2} onSelect={vi.fn()} />
      </ToneNariProvider>,
    );
    const marker = screen.getByRole('img', { name: t('routing.nodeStatus.reselectApplication') });
    expect(marker).toHaveAttribute('data-tone', 'warning');
    fireEvent.mouseEnter(marker);
    expect(await screen.findByRole('tooltip')).toHaveTextContent(
      t('routing.nodeStatus.reselectApplication'),
    );
    expect(JSON.stringify(raw)).toBe(before);
    view.unmount();
  },
);

it('retains the native safety flag and PID through gain edits and serialization', () => {
  const raw = runtimeGraphToAudioGraph(document);
  const presented = presentApplicationReselection(raw, i18n.getFixedT('en'));
  const edited = updateRoutingNodeGain(presented, 'legacy', -6);
  const saved = audioGraphToRuntimeGraph(edited, document);
  expect(saved.nodes[0]?.config).toMatchObject({
    requiresReselection: true,
    gainDb: -6,
    selector: { match: 'process_id', process_id: 123, include_descendants: true },
  });
  expect(saved.nodes[0]?.config).not.toHaveProperty('status');
});

it('clears the warning only for the fresh application selection, without mutating the old document', () => {
  const replacement = addRoutingNode(
    { revision: 1, edges: [], nodes: [] },
    {
      kind: 'application',
      bundleId: '',
      title: 'Chosen app',
      selector: { match: 'process_id', process_id: 456, include_descendants: true },
    },
    { x: 10, y: 20 },
  );
  const node = presentApplicationReselection(replacement, i18n.getFixedT('en')).nodes[0];
  expect(node?.appearance.status).toBeUndefined();
  expect(node?.runtime?.config).not.toHaveProperty('requiresReselection');
  expect(document.nodes[0]?.config).toHaveProperty('requiresReselection', true);
});

it('presents the restored application in the actual workspace without publishing an audio edit', async () => {
  await i18n.changeLanguage('en');
  const port = new StorybookMockControlPort({
    graph: structuredClone(document),
    graphStatus: { protocolVersion: 1, activeRevision: 1, submittedRevision: 1 },
    status: {
      protocolVersion: 1,
      driverState: 'ready',
      sampleRate: 48000,
      bufferFrames: 128,
      underruns: 0,
      overruns: 0,
    },
  });
  const apply = vi.spyOn(port, 'applyGraph');
  const view = render(
    <ToneNariProvider>
      <I18nextProvider i18n={i18n}>
        <RoutingWorkspace controlPort={port} />
      </I18nextProvider>
    </ToneNariProvider>,
  );
  expect(await screen.findByTestId('status')).toHaveTextContent('Select the application again');
  expect(apply).not.toHaveBeenCalled();
  expect((await port.getGraph()).nodes[0]?.config).toHaveProperty('requiresReselection', true);
  view.unmount();
});
