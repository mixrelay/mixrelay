import { describe, expect, it } from 'vitest';
import type { ExternalProcessorDescriptor } from '../../core/control-port/types';
import { i18n } from '../../core/i18n';
import { createProcessorMenuItems, processorCatalogPlaceholder } from './processorCatalogMenu';

it('groups all formats into sorted submenus with unchanged selectable plug-in IDs', () => {
  const formats = ['au', 'auv3', 'vst3', 'clap', 'lv2'] as const;
  const processors: ExternalProcessorDescriptor[] = formats.flatMap((format) =>
    ['Zeta', 'Alpha'].map((displayName) => ({
      format,
      displayName,
      typeId: `${format}:${displayName}`,
      vendor: 'Test',
      version: '1',
      inputChannels: 2,
      outputChannels: 2,
      hasEditor: true,
      parameters: [],
    })),
  );
  const items = createProcessorMenuItems(processors, i18n.t.bind(i18n), 'Empty');
  expect(items.map((item) => item.id)).toEqual(
    formats.map((format) => `processor-format:${format}`),
  );
  for (const item of items) {
    expect(item.kind).toBeUndefined();
    expect(item.children?.map((child) => child.label)).toEqual(['Alpha', 'Zeta']);
    expect(item.children?.map((child) => child.id)).toEqual(
      ['Alpha', 'Zeta'].map(
        (name) => `processor:${item.id.replace('processor-format:', '')}:${name}`,
      ),
    );
  }
  expect(processors[0]?.displayName).toBe('Zeta');
  expect(createProcessorMenuItems([], i18n.t.bind(i18n), 'Scanning')).toEqual([
    { disabled: true, id: 'no-processors', label: 'Scanning' },
  ]);
});

describe('empty processor menu evidence', () => {
  it('does not claim there are no installed effects during a scan', () => {
    expect(processorCatalogPlaceholder({ busy: true, failure: null, diagnostics: [] })).toBe(
      'routing.processorControl.scanning',
    );
  });
  it('does not claim there are no installed effects after transport or native failure', () => {
    expect(processorCatalogPlaceholder({ busy: false, failure: 'catalog', diagnostics: [] })).toBe(
      'routing.processorControl.incomplete',
    );
    expect(
      processorCatalogPlaceholder({
        busy: false,
        failure: null,
        diagnostics: [{ format: 'vst3', stage: 'scan', message: 'Timed out' }],
      }),
    ).toBe('routing.processorControl.incomplete');
  });
  it('reports no effects only for a completed clean scan', () => {
    expect(processorCatalogPlaceholder({ busy: false, failure: null, diagnostics: [] })).toBe(
      'routing.noProcessors',
    );
  });
});
