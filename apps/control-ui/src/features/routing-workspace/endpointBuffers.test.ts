import { expect, it, vi } from 'vitest';

import type { AudioGraphModel } from '../../core/audio-graph/model';
import type { NativeBufferSettings, VirtualEndpointSettings } from '../../core/control-port/types';
import { i18n } from '../../core/i18n';
import { changeEndpointBuffer, presentEndpointBuffers } from './endpointBuffers';
import { runtimeGraphToAudioGraph } from './model';
import { duplexReference, virtualPolicy, virtualSpec } from './virtualEndpointFixtures';

const buffer: NativeBufferSettings = {
  currentFrames: 128,
  preferredFrames: null,
  minimumFrames: 32,
  maximumFrames: 512,
  canSet: true,
};
function graph(): AudioGraphModel {
  return runtimeGraphToAudioGraph({
    schemaVersion: 2,
    revision: 1,
    edges: [],
    nodes: ['first', 'second'].map((id) => ({
      id,
      type: 'virtual_endpoint',
      label: null,
      config: { ...virtualSpec, reference: duplexReference },
    })),
  });
}
function settings(nativeBuffer: NativeBufferSettings | null = buffer): VirtualEndpointSettings {
  return {
    ...virtualPolicy,
    devices: virtualPolicy.devices.map((device) => ({ ...device, nativeBuffer })),
  };
}
function action(enabled = true) {
  return { enabled, error: null, change: vi.fn() };
}

it('shares one endpoint readback across every output node and hides controls on input nodes', () => {
  const view = presentEndpointBuffers(graph(), settings(), action(), i18n.t);
  const outputs = view.nodes.filter((node) => node.runtime?.binding?.side === 'sink');
  expect(outputs).toHaveLength(2);
  for (const node of outputs)
    expect(node.appearance.endpointControl?.bufferControl).toMatchObject({
      currentFrames: 128,
      preferredFrames: null,
      options: [32, 64, 128, 256, 512],
      disabled: false,
    });
  for (const node of view.nodes.filter((node) => node.runtime?.binding?.side === 'source'))
    expect(node.appearance.endpointControl?.bufferControl).toBeUndefined();
  const accepted = presentEndpointBuffers(
    view,
    settings({ ...buffer, currentFrames: 256, preferredFrames: 256 }),
    action(),
    i18n.t,
  );
  for (const node of accepted.nodes.filter((node) => node.runtime?.binding?.side === 'sink'))
    expect(node.appearance.endpointControl?.bufferControl?.currentFrames).toBe(256);
});

it('keeps settings visible while pending, with errors scoped to the shared device ID', () => {
  const pending = {
    ...action(false),
    error: { endpointId: virtualSpec.id, message: 'Try again.' },
  };
  const view = presentEndpointBuffers(graph(), settings(), pending, i18n.t);
  const control = view.nodes.find((node) => node.runtime?.binding?.side === 'sink')?.appearance
    .endpointControl?.bufferControl;
  expect(control).toMatchObject({ currentFrames: 128, disabled: true, error: 'Try again.' });
  const otherError = presentEndpointBuffers(
    graph(),
    settings(),
    { ...pending, error: { endpointId: 'other', message: 'Other failure' } },
    i18n.t,
  );
  expect(
    otherError.nodes.find((node) => node.runtime?.binding?.side === 'sink')?.appearance
      .endpointControl?.bufferControl?.error,
  ).toBeNull();
});

it('keeps distinct virtual devices independent even when their graph nodes share a view', () => {
  const model = graph();
  const distinct = {
    ...model,
    nodes: model.nodes.map((node) =>
      node.runtime?.binding?.nodeId === 'second'
        ? {
            ...node,
            runtime: { ...node.runtime, config: { ...virtualSpec, id: 'other' } },
          }
        : node,
    ),
  };
  const observed = settings();
  const first = observed.devices[0];
  if (!first) throw new Error('fixture missing');
  observed.devices.push({
    ...first,
    spec: { ...first.spec, id: 'other' },
    nativeBuffer: { ...buffer, currentFrames: 512 },
  });
  const change = action();
  const view = presentEndpointBuffers(distinct, observed, change, i18n.t);
  expect(
    view.nodes.find((node) => node.id === 'first::host-sink')?.appearance.endpointControl
      ?.bufferControl?.currentFrames,
  ).toBe(128);
  expect(
    view.nodes.find((node) => node.id === 'second::host-sink')?.appearance.endpointControl
      ?.bufferControl?.currentFrames,
  ).toBe(512);
  changeEndpointBuffer(distinct, 'second::host-sink', 256, change);
  expect(change.change).toHaveBeenCalledExactlyOnceWith('other', 256);
});

it('distinguishes unused saved values and readonly capability from actual I/O readback', () => {
  const view = presentEndpointBuffers(
    graph(),
    settings({ ...buffer, currentFrames: null, preferredFrames: 256 }),
    undefined,
    i18n.t,
  );
  expect(
    view.nodes.find((node) => node.runtime?.binding?.side === 'sink')?.appearance.endpointControl
      ?.bufferControl,
  ).toMatchObject({ currentFrames: null, preferredFrames: 256, disabled: true });
  const readonly = presentEndpointBuffers(
    graph(),
    settings({ ...buffer, canSet: false }),
    action(),
    i18n.t,
  );
  expect(
    readonly.nodes.find((node) => node.runtime?.binding?.side === 'sink')?.appearance
      .endpointControl?.bufferControl?.disabled,
  ).toBe(true);
});

it.each([undefined, settings(null)])(
  'hides unsupported platform or absent owner capabilities',
  (observed) => {
    const view = presentEndpointBuffers(graph(), observed, action(), i18n.t);
    for (const node of view.nodes)
      expect(node.appearance.endpointControl?.bufferControl).toBeUndefined();
  },
);

it('leaves ordinary hardware and nodes without endpoint settings untouched', () => {
  const model = graph();
  const source = model.nodes[0];
  if (!source) throw new Error('fixture missing');
  const hardware = {
    ...source,
    runtime: {
      type: 'hardware_output',
      binding: { nodeId: 'physical', side: 'sink' as const },
      config: { id: virtualSpec.id },
    },
  };
  const plain = { ...source, appearance: { ...source.appearance, endpointControl: undefined } };
  const view = presentEndpointBuffers(
    { ...model, nodes: [hardware, plain] },
    settings(),
    action(),
    i18n.t,
  );
  expect(view.nodes[0]?.appearance.endpointControl?.bufferControl).toBeUndefined();
  expect(view.nodes[1]).toBe(plain);
});

it('dispatches runtime commands only from a virtual output using stable endpoint ID', () => {
  const change = action();
  const model = graph();
  changeEndpointBuffer(model, 'first::host-sink', 256, change);
  expect(change.change).toHaveBeenCalledExactlyOnceWith(virtualSpec.id, 256);
  changeEndpointBuffer(model, 'first::host-source', 512, change);
  changeEndpointBuffer(model, 'missing', 512, change);
  changeEndpointBuffer(null, 'first::host-sink', 512, change);
  changeEndpointBuffer(model, 'first::host-sink', 512, undefined);
  changeEndpointBuffer(model, 'first::host-sink', 512, { ...change, enabled: false });
  expect(change.change).toHaveBeenCalledTimes(1);
});
