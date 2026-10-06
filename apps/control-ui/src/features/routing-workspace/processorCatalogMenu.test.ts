import { describe, expect, it } from 'vitest';
import type { ExternalProcessorDescriptor, ProviderPorts } from '../../core/control-port/types';
import { i18n } from '../../core/i18n';
import {
  createProcessorMenuItems,
  processorCatalogPlaceholder,
  processorLayoutItemId,
  processorSelectionForMenuItem,
} from './processorCatalogMenu';

function stereoLayout(layoutId: string): ProviderPorts {
  const main = (direction: 'Input' | 'Output') => ({
    id: 'audio',
    name: `Main ${direction}`,
    kind: 'main' as const,
    channels: 2,
    channelFormat: 'stereo' as const,
  });
  return { layoutId, inputs: [main('Input')], outputs: [main('Output')] };
}

it('lists every format as sorted direct leaves with distinguishable identities', () => {
  const formats = ['au', 'auv3', 'vst3', 'clap', 'lv2'] as const;
  const processors: ExternalProcessorDescriptor[] = formats.flatMap((format) =>
    ['Zeta', 'Alpha'].map((displayName) => ({
      format,
      displayName,
      typeId: `${format}:${displayName}`,
      vendor: 'Test',
      version: '1',
      layouts: [stereoLayout('layout-v1:stereo')],
      defaultLayoutId: 'layout-v1:stereo',
      hasEditor: true,
      parameters: [],
    })),
  );
  const items = createProcessorMenuItems(processors, i18n.t.bind(i18n), 'Empty');
  expect(items).toHaveLength(10);
  expect(items.map((item) => item.label)).toEqual([
    'Alpha',
    'Alpha',
    'Alpha',
    'Alpha',
    'Alpha',
    'Zeta',
    'Zeta',
    'Zeta',
    'Zeta',
    'Zeta',
  ]);
  expect(new Set(items.map((item) => item.id)).size).toBe(10);
  for (const item of items) {
    expect(item.kind).toBeUndefined();
    expect(item.children).toBeUndefined();
    const selected = processorSelectionForMenuItem(item.id, processors);
    expect(selected).not.toBeNull();
    expect(item.id).toBe(`processor:${selected?.processor.typeId}`);
    expect(item.description).toBe(
      `${i18n.t(`routing.groups.${selected?.processor.format}`)} · Test · Stereo`,
    );
    expect(selected?.ports.layoutId).toBe('layout-v1:stereo');
  }
  expect(processors[0]?.displayName).toBe('Zeta');
  expect(createProcessorMenuItems([], i18n.t.bind(i18n), 'Scanning')).toEqual([
    { disabled: true, id: 'no-processors', label: 'Scanning' },
  ]);
});

it('offers explicit multi-layout choices with concise topology labels and opaque exact keys', () => {
  const stereo = stereoLayout('layout-v1:sha256:stereo');
  const sidechain: ProviderPorts = {
    layoutId: 'layout-v1:sha256:sidechain',
    inputs: [
      {
        id: 'audio',
        name: 'Main Input',
        kind: 'main',
        channels: 2,
        channelFormat: 'stereo',
      },
      {
        id: 'bus:1',
        name: 'Detector',
        kind: 'sidechain',
        channels: 2,
        channelFormat: 'stereo',
      },
    ],
    outputs: stereo.outputs,
  };
  const duplicate = { ...sidechain, layoutId: 'layout-v1:sha256:duplicate' };
  const processor: ExternalProcessorDescriptor = {
    typeId: 'vst3:vendor:effect',
    displayName: 'Effect',
    vendor: 'Vendor',
    version: '1',
    format: 'vst3',
    hasEditor: true,
    parameters: [],
    layouts: [stereo, sidechain, duplicate],
    defaultLayoutId: stereo.layoutId,
  };
  const items = createProcessorMenuItems([processor], i18n.t.bind(i18n), 'Empty');
  expect(items.every((item) => item.children === undefined)).toBe(true);
  expect(items.map((item) => item.label)).toEqual([
    'Effect · Stereo',
    'Effect · Stereo + Sidechain 2 ch · 1',
    'Effect · Stereo + Sidechain 2 ch · 2',
  ]);
  expect(items.map((item) => item.id)).toEqual([
    processorLayoutItemId(processor.typeId, stereo.layoutId),
    processorLayoutItemId(processor.typeId, sidechain.layoutId),
    processorLayoutItemId(processor.typeId, duplicate.layoutId),
  ]);
  expect(processorSelectionForMenuItem(items[1]?.id ?? '', [processor])).toEqual({
    processor,
    ports: sidechain,
  });
  expect(items[1]?.description).toContain('VST3 · Vendor');
  expect(items[1]?.description).toContain('Detector');
  expect(processorSelectionForMenuItem(`processor:${processor.typeId}`, [processor])).toBeNull();
  expect(
    processorSelectionForMenuItem(processorLayoutItemId(processor.typeId, 'unknown'), [processor]),
  ).toBeNull();

  const separatorPairs = [
    processorLayoutItemId('vst3:one:two', 'layout:three'),
    processorLayoutItemId('vst3:one', 'two:layout:three'),
  ];
  expect(new Set(separatorPairs).size).toBe(2);
});

it('localizes layout directions and bus roles without showing machine-kind keys', () => {
  const directional: ProviderPorts = {
    layoutId: 'fingerprint-directional',
    inputs: [
      {
        id: 'input-main',
        name: 'Program',
        kind: 'main',
        channels: 1,
        channelFormat: 'mono',
      },
      {
        id: 'input-sidechain',
        name: 'Detector',
        kind: 'sidechain',
        channels: 2,
        channelFormat: 'stereo',
      },
    ],
    outputs: [
      {
        id: 'output-main',
        name: 'Program',
        kind: 'main',
        channels: 2,
        channelFormat: 'stereo',
      },
    ],
  };
  const processor: ExternalProcessorDescriptor = {
    typeId: 'vst3:directional',
    displayName: 'Effect',
    vendor: 'Test',
    version: '1',
    format: 'vst3',
    hasEditor: false,
    layouts: [directional, stereoLayout('fingerprint-stereo')],
    defaultLayoutId: directional.layoutId,
    parameters: [],
  };
  const items = createProcessorMenuItems([processor], i18n.getFixedT('ko-KR'), '없음');
  const selected = items[0];
  expect(selected?.label).toBe('Effect · 모노 입력 → 스테레오 출력 + 사이드체인 2채널');
  expect(selected?.description).toContain('입력:');
  expect(JSON.stringify(items).toLowerCase()).not.toContain('sidechain');
});

it('rejects duplicate layout identities instead of selecting the default or first duplicate', () => {
  const first = stereoLayout('duplicate:opaque-id');
  const processor: ExternalProcessorDescriptor = {
    typeId: 'vst3:vendor:duplicate',
    displayName: 'Duplicate',
    vendor: 'Vendor',
    version: '1',
    format: 'vst3',
    hasEditor: false,
    layouts: [first, { ...first, inputs: [] }],
    defaultLayoutId: first.layoutId,
    parameters: [],
  };
  expect(createProcessorMenuItems([processor], i18n.t.bind(i18n), 'Empty')).toEqual([]);
  expect(processorSelectionForMenuItem(`processor:${processor.typeId}`, [processor])).toBeNull();
  expect(
    processorSelectionForMenuItem(processorLayoutItemId(processor.typeId, first.layoutId), [
      processor,
    ]),
  ).toBeNull();
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
