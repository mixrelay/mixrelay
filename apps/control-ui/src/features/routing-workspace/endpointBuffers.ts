import type { TFunction } from 'i18next';

import type { AudioGraphModel } from '../../core/audio-graph/model';
import type { VirtualEndpointSettings } from '../../core/control-port/types';
import { nativeBufferChoices } from '../../core/driver/nativeBufferChoices';
import { virtualEndpointIdentity } from './model';

export interface DeviceBufferAction {
  enabled: boolean;
  error: { endpointId: string; message: string } | null;
  change: (endpointId: string, bufferFrames: number) => void;
}

export function changeEndpointBuffer(
  model: AudioGraphModel | null,
  nodeId: string,
  frames: number,
  action: DeviceBufferAction | undefined,
) {
  const node = model?.nodes.find((candidate) => candidate.id === nodeId);
  const endpointId = node && virtualEndpointIdentity(node);
  if (action?.enabled && node?.runtime?.binding?.side === 'sink' && endpointId)
    action.change(endpointId, frames);
}

export function presentEndpointBuffers(
  model: AudioGraphModel,
  settings: VirtualEndpointSettings | undefined,
  action: DeviceBufferAction | undefined,
  t: TFunction,
): AudioGraphModel {
  return {
    ...model,
    nodes: model.nodes.map((node) => {
      const control = node.appearance.endpointControl;
      if (!control) return node;
      const id = virtualEndpointIdentity(node);
      const buffer = settings?.devices.find((device) => device.spec.id === id)?.nativeBuffer;
      const canPresent =
        node.runtime?.type === 'virtual_endpoint' &&
        node.runtime.binding?.side === 'sink' &&
        buffer;
      return {
        ...node,
        appearance: {
          ...node.appearance,
          endpointControl: {
            ...control,
            bufferControl: canPresent
              ? {
                  label: t('driverManagement.bufferSize'),
                  framesLabel: t('driverManagement.frames'),
                  currentFrames: buffer.currentFrames,
                  preferredFrames: buffer.preferredFrames,
                  unselectedLabel: t('driverManagement.unselectedBuffer'),
                  pendingLabel: t('driverManagement.pendingBuffer'),
                  observedLabel: t('driverManagement.observedBuffer'),
                  options: nativeBufferChoices(buffer),
                  disabled: !action?.enabled || !buffer.canSet,
                  error: action?.error?.endpointId === id ? action.error.message : null,
                }
              : undefined,
          },
        },
      };
    }),
  };
}
