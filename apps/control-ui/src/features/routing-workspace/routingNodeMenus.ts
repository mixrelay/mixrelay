import type { TFunction } from 'i18next';
import type { AudioGraphNodeMenuItem } from '../../core/audio-graph/AudioGraphCanvas';
import type { AudioGraphModel } from '../../core/audio-graph/model';
import type {
  AudioDeviceDescriptor,
  AudioSessionStatus,
  CapturableApplication,
  ExternalProcessorDescriptor,
} from '../../core/control-port/types';
import { applicationInventorySelection } from './applicationIdentity';
import { chainKinds, effectKinds } from './builtinEffects';
import { virtualEndpointIdentity } from './model';
import { createProcessorMenuItems } from './processorCatalogMenu';
import { virtualDeviceMenuItem } from './virtualEndpointMenu';
export const ADD_PROCESSOR_DIRECTORY_ITEM_ID = 'add-processor-directory';
export const RESCAN_PROCESSORS_ITEM_ID = 'rescan-processors';
export const SYSTEM_AUDIO_ITEM_ID = 'system-audio';
export function audioInputItemId(device: AudioDeviceDescriptor): string {
  return `audio-input:${device.uid}`;
}

export function audioOutputItemId(device: AudioDeviceDescriptor): string {
  return `audio-output:${device.uid}`;
}

function createApplicationMenuItems(
  applications: readonly CapturableApplication[],
  t: TFunction,
  supported = true,
): AudioGraphNodeMenuItem[] {
  const items = new Map<string, AudioGraphNodeMenuItem>();
  for (const application of applications) {
    const selection = applicationInventorySelection(application);
    if (!selection) continue;
    const iconUrl = application.iconDataUrl || undefined;
    const previous = items.get(selection.id);
    if (previous) {
      if (!previous.iconUrl && iconUrl) previous.iconUrl = iconUrl;
      continue;
    }
    items.set(selection.id, {
      disabled: !supported,
      description: supported ? undefined : t('audioSession.captureRateLocked'),
      iconUrl,
      id: selection.id,
      label: application.displayName,
    });
  }
  return items.size
    ? [...items.values()]
    : [{ disabled: true, id: 'no-applications', label: t('routing.noApplications') }];
}

interface MenuOptions {
  applications: readonly CapturableApplication[];
  audioDevices: readonly AudioDeviceDescriptor[];
  processors: readonly ExternalProcessorDescriptor[];
  model: AudioGraphModel | null;
  t: TFunction;
  session?: AudioSessionStatus;
  processorEmptyLabel: string;
  paid: boolean;
}
function menuChoices({
  applications,
  audioDevices,
  processors,
  model,
  t,
  session,
  processorEmptyLabel,
  paid,
}: MenuOptions) {
  const virtualIds = new Set(
    session?.virtualEndpointSettings?.devices.flatMap((device) => device.nativeDeviceIds),
  );
  const hostDevices = audioDevices.filter((device) => !virtualIds.has(device.uid));
  const inputs = hostDevices.filter((device) => device.inputChannels > 0);
  const outputs = hostDevices.filter((device) => device.outputChannels > 0);
  const inputLimit =
    !paid &&
    (model?.nodes.filter(
      (node) =>
        node.runtime?.type === 'application_capture' || node.runtime?.type === 'audio_device_input',
    ).length ?? 0) >= 2;
  const pluginLimit =
    !paid && (model?.nodes.filter((node) => node.runtime?.type === 'provider').length ?? 0) >= 2;
  const recorderLimit = !paid && model?.nodes.some((node) => node.runtime?.type === 'recorder');
  const applicationItems = createApplicationMenuItems(
    applications,
    t,
    session?.capabilities.applicationCapture,
  ).map((item) =>
    inputLimit ? { ...item, disabled: true, description: t('licensing.limits.inputs') } : item,
  );
  const usedOutputs = outputIdentities(model);
  const inputItems = hardwareItems({
    devices: inputs,
    side: 'input',
    supported: session?.capabilities.hardwareInput,
    limited: inputLimit,
    t,
    used: usedOutputs,
    paid,
  });
  const outputItems = hardwareItems({
    devices: outputs,
    side: 'output',
    supported: session?.capabilities.hardwareOutput,
    limited: false,
    t,
    used: usedOutputs,
    paid,
  });
  const processorItems = createProcessorMenuItems(processors, t, processorEmptyLabel).map((item) =>
    pluginLimit ? { ...item, disabled: true, description: t('licensing.limits.plugins') } : item,
  );
  const systemItems: AudioGraphNodeMenuItem[] =
    session?.capabilities.backend === 'core_audio'
      ? [
          {
            id: SYSTEM_AUDIO_ITEM_ID,
            label: t('studioGraph.nodes.systemAudio.name'),
            disabled: inputLimit || !session.capabilities.applicationCapture,
            description: inputLimit
              ? t('licensing.limits.inputs')
              : session.capabilities.applicationCapture
                ? t('studioGraph.nodes.systemAudio.description')
                : t('audioSession.captureRateLocked'),
          },
        ]
      : [];
  return { applicationItems, systemItems, inputItems, outputItems, processorItems, recorderLimit };
}
function outputIdentities(model: AudioGraphModel | null): Set<string> {
  return new Set(
    (model?.nodes ?? []).flatMap((node) => {
      const identity = virtualEndpointIdentity(node);
      if (identity && node.inputPorts.length) return [`driver:${identity}`];
      if (node.runtime?.type !== 'audio_device_output') return [];
      const config = node.runtime.config as { selector?: { uid?: string; match?: string } };
      return [`hardware:${config.selector?.uid ?? config.selector?.match ?? node.id}`];
    }),
  );
}
function hardwareItems({
  devices,
  side,
  supported,
  limited,
  t,
  used,
  paid,
}: {
  devices: readonly AudioDeviceDescriptor[];
  side: 'input' | 'output';
  supported: boolean | undefined;
  limited: boolean;
  t: TFunction;
  used: Set<string>;
  paid: boolean;
}): AudioGraphNodeMenuItem[] {
  return devices.length
    ? devices.map((device) => {
        const outputLimit =
          side === 'output' && !paid && used.size >= 2 && !used.has(`hardware:${device.uid}`);
        const quota = limited || outputLimit;
        const channels = side === 'input' ? device.inputChannels : device.outputChannels;
        return {
          id: side === 'input' ? audioInputItemId(device) : audioOutputItemId(device),
          label: device.displayName,
          disabled: quota || supported === false,
          description: hardwareDescription(quota, side, supported, channels, t),
        };
      })
    : [
        {
          disabled: true,
          id: `no-${side}-devices`,
          label: t(side === 'input' ? 'routing.noInputDevices' : 'routing.noOutputDevices'),
        },
      ];
}
function limitedVirtualMenu(
  side: 'input' | 'output',
  { model, t, session, paid }: MenuOptions,
): AudioGraphNodeMenuItem {
  const item = virtualDeviceMenuItem(side, model, t, session);
  if (paid) return item;
  const used = new Set(
    (model?.nodes ?? []).flatMap((node) => {
      const id = virtualEndpointIdentity(node);
      return id ? [id] : [];
    }),
  );
  const outputs = outputIdentities(model);
  return {
    ...item,
    children: item.children?.map((child) => {
      const device = session?.virtualEndpointSettings?.devices.find(
        ({ spec }) => child.id === `virtual/${side}/${encodeURIComponent(spec.id)}`,
      );
      const limited =
        child.id === `virtual-new-${side}` ||
        (!!device &&
          ((used.size >= 1 && !used.has(device.spec.id)) ||
            (side === 'output' && outputs.size >= 2 && !outputs.has(`driver:${device.spec.id}`))));
      return limited
        ? {
            ...child,
            disabled: true,
            description: t(
              device && used.has(device.spec.id)
                ? 'licensing.limits.outputs'
                : 'licensing.limits.virtualDevices',
            ),
          }
        : child;
    }),
  };
}
export function createNodeMenuItems(options: MenuOptions): AudioGraphNodeMenuItem[] {
  const { t } = options;
  const { applicationItems, systemItems, inputItems, outputItems, processorItems, recorderLimit } =
    menuChoices(options);
  return [
    {
      children: [
        limitedVirtualMenu('input', options),
        { id: 'virtual-input-divider', kind: 'divider', label: '' },
        ...systemItems,
        {
          id: 'running-applications-label',
          kind: 'label',
          label: t('routing.groups.runningApplications'),
        },
        ...applicationItems,
        { id: 'source-divider', kind: 'divider', label: '' },
        { id: 'audio-inputs-label', kind: 'label', label: t('routing.groups.audioInputs') },
        ...inputItems,
      ],
      id: 'source',
      label: t('routing.groups.source'),
    },
    {
      children: [
        {
          id: 'compressor',
          label: t('routing.nodes.compressor'),
        },
        {
          id: 'limiter',
          label: t('routing.nodes.limiter'),
        },
        { id: 'processor-divider', kind: 'divider', label: '' },
        ...effectKinds.map((kind) => ({ id: `builtin:${kind}`, label: t(`effects.${kind}`) })),
        {
          id: 'effect-chains',
          label: t('effects.chains'),
          children: chainKinds.map((kind) => ({
            id: `effect-chain:${kind}`,
            label: t(`effects.${kind}`),
          })),
        },
        ...processorItems,
        { id: 'processor-actions-divider', kind: 'divider', label: '' },
        {
          description: t('routing.addProcessorDirectoryDescription'),
          id: ADD_PROCESSOR_DIRECTORY_ITEM_ID,
          label: t('routing.addProcessorDirectory'),
        },
        { id: RESCAN_PROCESSORS_ITEM_ID, label: t('routing.rescanProcessors') },
      ],
      id: 'processor',
      label: t('routing.groups.processor'),
    },
    {
      children: [
        limitedVirtualMenu('output', options),
        {
          id: 'recorder',
          label: t('recorder.title'),
          disabled: recorderLimit === true,
          description: recorderLimit ? t('recorder.freeLimit') : undefined,
        },
        { id: 'virtual-output-divider', kind: 'divider', label: '' },
        { id: 'audio-outputs-label', kind: 'label', label: t('routing.groups.audioOutputs') },
        ...outputItems,
      ],
      id: 'output',
      label: t('routing.groups.output'),
    },
  ];
}

function hardwareDescription(
  quota: boolean,
  side: 'input' | 'output',
  supported: boolean | undefined,
  channels: number,
  t: TFunction,
) {
  if (quota) return t(`licensing.limits.${side === 'input' ? 'inputs' : 'outputs'}`);
  if (supported === false) return t('audioSession.unsupported');
  return `${channels} CH`;
}
