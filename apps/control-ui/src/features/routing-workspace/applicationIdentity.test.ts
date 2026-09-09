import { describe, expect, it } from 'vitest';

import type { AudioGraphModel } from '../../core/audio-graph/model';
import type {
  CapturableApplication,
  RuntimeApplicationSelector,
} from '../../core/control-port/types';
import { applicationItemId, applicationSelectorKey } from './applicationIdentity';
import { presentRoutingApplicationIcons } from './model';

function application(selector?: RuntimeApplicationSelector, processId = 10): CapturableApplication {
  return {
    bundleId: 'org.example.player',
    displayName: 'Player',
    processId,
    selector,
    iconDataUrl: `data:image/png;base64,pid-${processId}`,
  };
}
function graph(selector: unknown): AudioGraphModel {
  return {
    revision: 7,
    edges: [],
    nodes: [
      {
        id: 'source',
        position: { x: 12, y: 24 },
        inputPorts: [],
        outputPorts: [],
        appearance: { title: 'My saved label', category: 'Application', icon: 'APP' },
        runtime: { type: 'application_capture', config: { selector, gainDb: -6 } },
      },
    ],
  };
}
function process(processId: number): RuntimeApplicationSelector {
  return { match: 'process_id', process_id: processId, include_descendants: true };
}

describe('application presentation identity across platforms', () => {
  it('keeps menu choices distinct for instances sharing an application ID', () => {
    const first = application(process(10)),
      second = application(process(20), 20);
    expect(applicationItemId(first)).not.toBe(applicationItemId(second));
    expect(applicationItemId({ ...first, displayName: 'Translated name' })).toBe(
      applicationItemId(first),
    );
    expect(
      [first, second].find((item) => applicationItemId(item) === applicationItemId(second)),
    ).toBe(second);
  });

  it('matches a PID-selected node to that exact instance without changing capture config', () => {
    const original = graph(process(20));
    const shown = presentRoutingApplicationIcons(original, [
      application(process(10)),
      application(process(20), 20),
    ]);
    expect(shown.nodes[0]?.appearance.iconUrl).toBe('data:image/png;base64,pid-20');
    expect(shown.nodes[0]?.runtime).toBe(original.nodes[0]?.runtime);
    expect(shown.nodes[0]?.appearance.title).toBe('My saved label');
    expect(shown.revision).toBe(7);
  });

  it('preserves macOS bundle selection and old inventory responses without a selector', () => {
    const original = graph({
      match: 'bundle_id',
      bundle_id: 'org.example.player',
      include_descendants: true,
    });
    const shown = presentRoutingApplicationIcons(original, [application()]);
    expect(shown.nodes[0]?.appearance.iconUrl).toBe('data:image/png;base64,pid-10');
    expect(presentRoutingApplicationIcons(shown, [application()])).toBe(shown);
  });

  it('matches explicit executable selectors by path, never filename alone', () => {
    const selected: RuntimeApplicationSelector = { match: 'executable_path', path: '/opt/player' };
    const shown = presentRoutingApplicationIcons(graph(selected), [
      application({ match: 'executable_path', path: '/other/player' }),
      application(selected, 20),
    ]);
    expect(shown.nodes[0]?.appearance.iconUrl).toBe('data:image/png;base64,pid-20');
  });

  it('clears an old icon on disappearance rather than guessing a new PID from its name', () => {
    const shown = presentRoutingApplicationIcons(graph(process(10)), [application(process(10))]);
    const gone = presentRoutingApplicationIcons(shown, [application(process(20), 20)]);
    expect(gone.nodes[0]?.appearance.iconUrl).toBeUndefined();
    expect(gone.nodes[0]?.runtime).toBe(shown.nodes[0]?.runtime);
  });

  it('never invents one application icon for all-except or malformed selectors', () => {
    for (const selector of [
      null,
      {},
      { match: 'process_id', process_id: '10' },
      { match: 'process_id', process_id: -1 },
      { match: 'bundle_id', bundle_id: '' },
      { match: 'all_except', bundle_ids: ['org.example.player'] },
    ]) {
      expect(applicationSelectorKey(selector)).toBeUndefined();
      expect(
        presentRoutingApplicationIcons(graph(selector), [application()]).nodes[0]?.appearance
          .iconUrl,
      ).toBeUndefined();
    }
  });
});
